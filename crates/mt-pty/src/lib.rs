//! PTY 生命周期:spawn / read / write / resize / kill。**不含任何 UI 与 VT 解析**。
//!
//! # 从 `src-tauri/src/pty.rs` 移入的范围
//!
//! | 现有代码 | 去向 |
//! |---|---|
//! | `create_pty` / `write_pty` / `resize_pty` / `kill_pty` | 本 crate,去掉 `#[tauri::command]`,改成普通方法 |
//! | AI 命令识别(`claude`/`codex`/`opencode`/`pi`/`grok` + ↑ 历史/Tab 补全的行快照兜底与输出回扫) | `mt-ai`,它需要的只是「用户键入的字节」这一路输入 |
//! | 裸 Esc / Ctrl+C 的用户打断识别(`note_user_interrupt`) | `mt-ai`,同上 |
//! | `arm_ssh_autofill` | 保留在本 crate(它就是往 PTY 写字节) |
//! | ConPTY 便携 DLL 预载(`conpty_bootstrap.rs`) | 本 crate,原样搬,仍须早于任何 `openpty` |
//!
//! 「用户键入的字节」这一路由 [`PtySession::set_input_observer`] 提供:上层把
//! 观察器挂上去就能拿到每一次写入的原始字节,本 crate 不解释它们,**也不知道
//! 有 AI 这回事**。
//!
//! # 明确**不要**移过来的东西
//!
//! 以下代码在 GPUI 架构下没有存在意义,移植时直接删掉,不要试图保留:
//!
//! - **16ms 批量缓冲**:原本是为了摊薄 `emit('pty-output')` 的 IPC 开销。现在
//!   reader 线程读到的字节直接进 `mt-terminal` 的 grid,没有 IPC。
//! - **有界 channel + 4MB/1MB 双水位背压 + `set_pty_flow_paused` + 30s 超时兜底**:
//!   原本是拿来在 WebView 边界上人工造一条背压链路。现在解析速度就是本进程的
//!   速度,读慢了 ConPTY 自然阻塞刷屏进程,背压是天然的。
//! - **`kill_all_ptys` 孤儿回收**:原本是为了兜住 WebView2 renderer 被 OOM 杀掉后
//!   页面重载、旧 PTY 无人引用却继续运行。GPUI 是单进程,进程没了 PTY 也就没了。
//!
//! 这三块是本次改造在后端侧最大的一笔净删除,详见 `docs/gpui-migration.md`。
//!
//! 同时删掉的还有 `PtyManager`:`HashMap<pty_id, PtyInstance>` + 自增 id + 十张
//! 旁路状态表(以及为它们准备的 `purge_pty_state`)。GPUI 侧每个 pane 直接持有
//! 一个 [`PtySession`],所有权即生命周期,注册表和 id 分配都没有存在理由。
//!
//! # 模块地图
//!
//! - [`conpty`] —— 便携 ConPTY 预载(**必须早于任何 spawn**,见该模块文档)
//! - [`ssh`] —— SSH 密码自动填充状态机 + 远程启动器 argv 拼装
//! - `launch`(内部) —— cwd / WSL 启动器重写 / 环境变量装配,导出见下方 re-export
//!
//! # 用法梗概
//!
//! ```no_run
//! # use mt_pty::{PtyOptions, PtySession, PtySpawn};
//! mt_pty::conpty::initialize_default(); // 进程内一次,且早于任何 spawn
//!
//! let spec = PtySpawn {
//!     program: "pwsh.exe".into(),
//!     args: vec![],
//!     cwd: Some(r"D:\Git\mini-term".into()),
//!     env: vec![],           // 应用注入的内部变量
//!     rows: 24,
//!     cols: 80,
//! };
//! let options = PtyOptions::default()
//!     .with_user_env(vec![("FOO".into(), "1".into())]) // 项目级 env(会被过滤)
//!     .on_exit(|code| eprintln!("子进程退出:{code:?}"));
//!
//! let session = PtySession::spawn_with_options(spec, options, |bytes| {
//!     // 直接喂给 VT 状态机
//!     let _ = bytes;
//! })?;
//! session.set_input_observer(|bytes| { let _ = bytes; }); // 键入字节的旁路
//! session.write(b"ls\r")?;
//! # Ok::<(), anyhow::Error>(())
//! ```

use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use parking_lot::Mutex;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub mod conpty;
mod launch;
pub mod ssh;

pub use conpty::{ConptyBootstrapDecision, choose_conpty_bootstrap};
pub use launch::{
    WslOverride, build_wslenv_value, decide_wsl_override, fallback_local_cwd, fallback_windows_cwd,
};
pub use ssh::SshAutofill;

/// reader 单次读取的缓冲区大小。
///
/// 比原实现的 4KB 大一档:那个尺寸是为了配合「有界 channel + 16ms 批缓冲」把
/// 在途内存卡在 2MB,而现在字节读出来就直接进 VT 状态机,一次多读一点纯赚。
const READ_CHUNK: usize = 64 * 1024;

/// PTY 尺寸的缺省值。上层挂载后会立刻按真实尺寸 resize。
pub const INITIAL_PTY_COLS: u16 = 80;
pub const INITIAL_PTY_ROWS: u16 = 24;

/// 终端焦点事件的 CSI 序列(TUI 开启 DEC 私有模式 1004 后,终端会在获得/失去
/// 焦点时把它们写进 PTY)。它们不是用户按键:本 crate 据此不解除 SSH 密码自动
/// 填充;上层从 [`PtySession::set_input_observer`] 拿到写入字节后若要区分
/// 「用户敲的」与「终端自动发的」,直接比对这两个常量,不必各自再写一份。
pub const FOCUS_IN_SEQ: &[u8] = b"\x1b[I";
pub const FOCUS_OUT_SEQ: &[u8] = b"\x1b[O";

/// 退出监听的轮询节奏:头 [`EXIT_POLL_FAST_WINDOW`] 内快轮询(短命令退出得快,
/// 状态要跟得上),之后降速常驻。
///
/// 为什么不靠 reader 的 EOF(原实现的做法):**Windows ConPTY 不给 EOF**。
/// 伪控制台的输出管道由 conhost 持有,子进程退出后管道依旧敞着,要等
/// `ClosePseudoConsole()`(即 master 被销毁)才收口 —— 实测子进程退出后
/// `try_wait` 已返回 `Some(0)`,而 reader 仍稳稳阻塞在 `read` 上。
/// 于是退出监听独立成一条 watcher 线程,轮询子进程本身。
const EXIT_POLL_FAST: Duration = Duration::from_millis(50);
const EXIT_POLL_SLOW: Duration = Duration::from_millis(250);
const EXIT_POLL_FAST_WINDOW: Duration = Duration::from_secs(2);
const OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

type BoxedChild = Box<dyn Child + Send + Sync>;
type BoxedMaster = Box<dyn MasterPty + Send>;
type BoxedWriter = Box<dyn Write + Send>;
type InputObserver = Box<dyn FnMut(&[u8]) + Send>;
type ExitCallback = Box<dyn FnOnce(Option<u32>) + Send>;
type ExitStatusCallback = Box<dyn FnOnce(PtyExitStatus) + Send>;
type ExitFinalizer = Box<dyn FnOnce() + Send>;
type MasterCloser = std::sync::mpsc::Sender<BoxedMaster>;

/// 子进程结束后的 PTY 输出收口结果。
///
/// 正常路径只有在 reader 已交付完最后一批输出后才返回 [`Self::Drained`]。
/// reader 失败返回 [`Self::OutputDrainFailed`];若后代进程继续持有 PTY/pipe
/// 导致 reader 无法结束,watcher 会在有界等待后返回
/// [`Self::OutputDrainTimedOut`]。两种异常都让上层明确失效恢复历史。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyExitStatus {
    Drained(Option<u32>),
    OutputDrainFailed(Option<u32>),
    OutputDrainTimedOut(Option<u32>),
}

impl PtyExitStatus {
    pub fn exit_code(self) -> Option<u32> {
        match self {
            Self::Drained(exit_code)
            | Self::OutputDrainFailed(exit_code)
            | Self::OutputDrainTimedOut(exit_code) => exit_code,
        }
    }
}

#[derive(Clone, Copy)]
enum OutputPumpResult {
    Drained,
    Failed,
}

struct OutputPumpCompletion {
    sender: Option<std::sync::mpsc::Sender<OutputPumpResult>>,
    result: OutputPumpResult,
}

impl OutputPumpCompletion {
    fn new(sender: std::sync::mpsc::Sender<OutputPumpResult>) -> Self {
        Self {
            sender: Some(sender),
            result: OutputPumpResult::Failed,
        }
    }

    fn mark_drained(&mut self) {
        self.result = OutputPumpResult::Drained;
    }
}

impl Drop for OutputPumpCompletion {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(self.result);
        }
    }
}

struct OutputDeliveryState {
    accepting: bool,
    active: usize,
    cancelled: bool,
    finalizer: Option<ExitFinalizer>,
}

struct OutputDelivery {
    state: Mutex<OutputDeliveryState>,
}

struct OutputDeliveryGuard<'a> {
    delivery: &'a OutputDelivery,
}

impl Drop for OutputDeliveryGuard<'_> {
    fn drop(&mut self) {
        self.delivery.finish_delivery();
    }
}

impl OutputDelivery {
    fn new() -> Self {
        Self {
            state: Mutex::new(OutputDeliveryState {
                accepting: true,
                active: 0,
                cancelled: false,
                finalizer: None,
            }),
        }
    }

    fn deliver(&self, callback: impl FnOnce()) -> bool {
        {
            let mut state = self.state.lock();
            if !state.accepting {
                return false;
            }
            state.active += 1;
        }
        let _guard = OutputDeliveryGuard { delivery: self };
        callback();
        true
    }

    fn finish_delivery(&self) {
        let finalizer = {
            let mut state = self.state.lock();
            debug_assert!(state.active > 0);
            state.active -= 1;
            if state.active == 0 && !state.cancelled {
                state.finalizer.take()
            } else {
                None
            }
        };
        if let Some(finalizer) = finalizer {
            finalizer();
        }
    }

    fn finalize(&self, callback: impl FnOnce() + Send + 'static) {
        let finalizer = {
            let mut state = self.state.lock();
            state.accepting = false;
            if state.cancelled {
                return;
            }
            debug_assert!(state.finalizer.is_none());
            if state.active == 0 {
                Some(Box::new(callback) as ExitFinalizer)
            } else {
                state.finalizer = Some(Box::new(callback));
                None
            }
        };
        if let Some(finalizer) = finalizer {
            finalizer();
        }
    }

    fn close(&self) {
        let finalizer = {
            let mut state = self.state.lock();
            state.accepting = false;
            state.cancelled = true;
            state.finalizer.take()
        };
        drop(finalizer);
    }
}

struct ExitWatcher {
    child: Arc<Mutex<BoxedChild>>,
    master: Arc<Mutex<Option<BoxedMaster>>>,
    writer: Arc<Mutex<Option<BoxedWriter>>>,
    master_closer: MasterCloser,
    closing: Arc<std::sync::atomic::AtomicBool>,
    output_delivery: Arc<OutputDelivery>,
    output_done: std::sync::mpsc::Receiver<OutputPumpResult>,
    output_drain_timeout: Duration,
    on_exit: Option<ExitCallback>,
    on_exit_status: Option<ExitStatusCallback>,
}

/// 一个活着的 PTY 会话。持有 master 端、子进程句柄和写入端。
pub struct PtySession {
    /// watcher 在子进程自然退出时也会取走 master,确保 ConPTY 输出泵收口。
    master: Arc<Mutex<Option<BoxedMaster>>>,
    child: Arc<Mutex<BoxedChild>>,
    writer: Arc<Mutex<Option<BoxedWriter>>>,
    /// Master 销毁可能阻塞;实际 drop 始终交给预先启动的后台 worker。
    master_closer: MasterCloser,
    /// 写入路径的旁路观察器(见 [`PtySession::set_input_observer`])。
    input_observer: Arc<Mutex<Option<InputObserver>>>,
    /// SSH 密码自动填充状态,与 reader 线程共享。
    autofill: Arc<Mutex<Option<SshAutofill>>>,
    /// 上次已应用的尺寸 (cols, rows),[`PtySession::resize`] 用它做同尺寸去重。
    last_size: Mutex<(u16, u16)>,
    /// 会话正在被上层关闭。置位后退出 watcher 不再回调 —— `Drop` 里的 kill
    /// 不该被当成「子进程自己退出了」上报。
    closing: Arc<std::sync::atomic::AtomicBool>,
    /// Rejects output reads that begin after exit finalization or active close.
    output_delivery: Arc<OutputDelivery>,
    /// cwd 命中 WSL UNC 时的重写结果,上层可据此提示用户一次。
    wsl_override: Option<WslOverride>,
}

/// 创建 PTY 所需的参数。字段刻意保持贫瘠 —— 现有 `create_pty` 的其余参数
/// (AI 识别相关、状态上报相关)属于 `mt-ai`,不该经过这里。
#[derive(Debug, Clone)]
pub struct PtySpawn {
    /// shell 可执行文件路径或名字。
    pub program: String,
    /// 传给 shell 的参数。
    pub args: Vec<String>,
    /// 工作目录;`None` 表示继承当前进程。
    pub cwd: Option<String>,
    /// 追加到子进程环境的键值对(应用注入的内部变量走这里)。
    pub env: Vec<(String, String)>,
    pub rows: u16,
    pub cols: u16,
}

/// 起 PTY 时的可选行为。所有字段都有合理缺省,`PtyOptions::default()` 即原
/// `create_pty` 的行为。**新能力一律加在这里**,`PtySpawn` 的字段保持不变。
pub struct PtyOptions {
    /// 注入 TERM/COLORTERM/LANG/LC_CTYPE/LESSCHARSET(默认 `true`)。
    /// 顺序在 [`PtySpawn::env`] 与 [`Self::user_env`] 之前,后两者可以覆盖它们。
    pub terminal_env: bool,
    /// 用户 / 项目级环境变量。与 [`PtySpawn::env`] 的区别是**会被过滤**:
    /// 命中 [`Self::reserved_env_prefixes`] 的 key 与 `WSLENV` 一律丢弃。
    pub user_env: Vec<(String, String)>,
    /// 保留 key 前缀。默认 `["MINITERM_"]` —— 应用内部协议变量的命名空间,
    /// 用户手改配置也不该能覆盖它们。调用方可按自己的命名空间替换。
    pub reserved_env_prefixes: Vec<String>,
    /// cwd 命中 WSL UNC(`\\wsl$\...`)时改用 `wsl.exe` 启动(默认 `true`)。
    pub wsl_cwd_rewrite: bool,
    /// 子进程退出时回调一次,参数是退出码(取不到为 `None`)。
    ///
    /// 在一条独立的 watcher 线程上调用。reader 正常结束或已返回失败时，回调
    /// 严格晚于最后一次 `on_output`。若 drain 超时，watcher 会停止接纳新输出并
    /// 及时返回；已经开始的 `on_output` 完成后才触发退出回调。
    /// 会话被 [`Drop`] 掉时不会回调 —— 那是上层自己关掉的,不是子进程退出。
    pub on_exit: Option<ExitCallback>,
    /// Extended exit callback used by lifecycle owners that must distinguish a clean
    /// output drain from reader failure or timeout.
    pub on_exit_status: Option<ExitStatusCallback>,
    /// 子进程退出后等待 reader 输出泵收口的最大时长。
    pub output_drain_timeout: Duration,
}

impl Default for PtyOptions {
    fn default() -> Self {
        Self {
            terminal_env: true,
            user_env: Vec::new(),
            reserved_env_prefixes: vec!["MINITERM_".to_string()],
            wsl_cwd_rewrite: true,
            on_exit: None,
            on_exit_status: None,
            output_drain_timeout: OUTPUT_DRAIN_TIMEOUT,
        }
    }
}

impl std::fmt::Debug for PtyOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtyOptions")
            .field("terminal_env", &self.terminal_env)
            .field("user_env", &self.user_env)
            .field("reserved_env_prefixes", &self.reserved_env_prefixes)
            .field("wsl_cwd_rewrite", &self.wsl_cwd_rewrite)
            .field("on_exit", &self.on_exit.is_some())
            .field("on_exit_status", &self.on_exit_status.is_some())
            .field("output_drain_timeout", &self.output_drain_timeout)
            .finish()
    }
}

impl PtyOptions {
    pub fn with_user_env(mut self, user_env: Vec<(String, String)>) -> Self {
        self.user_env = user_env;
        self
    }

    pub fn with_reserved_env_prefixes(mut self, prefixes: Vec<String>) -> Self {
        self.reserved_env_prefixes = prefixes;
        self
    }

    pub fn with_wsl_cwd_rewrite(mut self, enabled: bool) -> Self {
        self.wsl_cwd_rewrite = enabled;
        self
    }

    pub fn with_output_drain_timeout(mut self, timeout: Duration) -> Self {
        self.output_drain_timeout = timeout;
        self
    }

    /// 注册退出回调(见 [`Self::on_exit`])。
    pub fn on_exit<F>(mut self, callback: F) -> Self
    where
        F: FnOnce(Option<u32>) + Send + 'static,
    {
        self.on_exit = Some(Box::new(callback));
        self.on_exit_status = None;
        self
    }

    /// Registers an exit callback that also reports whether PTY output drained cleanly.
    pub fn on_exit_status<F>(mut self, callback: F) -> Self
    where
        F: FnOnce(PtyExitStatus) + Send + 'static,
    {
        self.on_exit_status = Some(Box::new(callback));
        self.on_exit = None;
        self
    }
}

impl PtySession {
    /// 起一个 PTY 并 spawn 子进程,行为等价于 `spawn_with_options(spec,
    /// PtyOptions::default(), on_output)`。
    ///
    /// `on_output` 在**独立的 reader 线程**上被调用,每次拿到一段刚读出的字节。
    /// 调用方(`mt-terminal`)在这里把字节喂进 VT 状态机 —— 这就是整条数据流的全部,
    /// 中间没有 channel、没有缓冲窗口、没有序列化。
    pub fn spawn<F>(spec: PtySpawn, on_output: F) -> Result<Self>
    where
        F: FnMut(&[u8]) + Send + 'static,
    {
        Self::spawn_with_options(spec, PtyOptions::default(), on_output)
    }

    /// 起一个 PTY,并按 [`PtyOptions`] 做启动前的预处理
    /// (WSL 启动器重写 / 终端环境变量 / 用户 env 过滤 / 退出回调)。
    pub fn spawn_with_options<F>(
        spec: PtySpawn,
        mut options: PtyOptions,
        on_output: F,
    ) -> Result<Self>
    where
        F: FnMut(&[u8]) + Send + 'static,
    {
        // on_exit 要移进退出 watcher 线程,其余字段还要留给 plan 用,先摘出来。
        let on_exit = options.on_exit.take();
        let on_exit_status = options.on_exit_status.take();
        let output_drain_timeout = options.output_drain_timeout;
        let plan = launch::plan(&spec, &options);
        Self::spawn_planned(
            plan,
            spec.rows,
            spec.cols,
            output_drain_timeout,
            on_exit,
            on_exit_status,
            on_output,
        )
    }

    fn spawn_planned<F>(
        plan: launch::LaunchPlan,
        rows: u16,
        cols: u16,
        output_drain_timeout: Duration,
        on_exit: Option<ExitCallback>,
        on_exit_status: Option<ExitStatusCallback>,
        mut on_output: F,
    ) -> Result<Self>
    where
        F: FnMut(&[u8]) + Send + 'static,
    {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("openpty 失败")?;

        let mut cmd = CommandBuilder::new(&plan.program);
        for arg in &plan.args {
            cmd.arg(arg);
        }
        if let Some(cwd) = &plan.cwd {
            cmd.cwd(cwd);
        }
        for (k, v) in &plan.env {
            cmd.env(k, v);
        }

        // This worker is a fallible constructor step. Start it before the child so a
        // thread-creation failure cannot leave a spawned process without an owner.
        let master_closer = spawn_drop_worker("mini-term-pty-master-closer")
            .context("start PTY master closer failed")?;

        let child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("spawn `{}` 失败", plan.program))?;
        // slave 必须在 spawn 后立刻丢弃,否则子进程退出时 master 侧读不到 EOF。
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .context("clone reader 失败")?;
        let writer: Arc<Mutex<Option<BoxedWriter>>> = Arc::new(Mutex::new(Some(
            pair.master.take_writer().context("take writer 失败")?,
        )));
        let master = Arc::new(Mutex::new(Some(pair.master)));
        let child: Arc<Mutex<BoxedChild>> = Arc::new(Mutex::new(child));
        let autofill: Arc<Mutex<Option<SshAutofill>>> = Arc::new(Mutex::new(None));
        let (output_done_tx, output_done_rx) = std::sync::mpsc::channel();
        let output_delivery = Arc::new(OutputDelivery::new());

        let autofill_for_reader = Arc::clone(&autofill);
        let writer_for_reader = Arc::clone(&writer);
        let output_delivery_for_reader = Arc::clone(&output_delivery);
        std::thread::spawn(move || {
            let mut completion = OutputPumpCompletion::new(output_done_tx);
            let mut buf = [0u8; READ_CHUNK];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        completion.mark_drained();
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                    Ok(n) => {
                        let chunk = &buf[..n];
                        if !output_delivery_for_reader.deliver(|| {
                            // SSH 密码自动填充先于交付:命中提示就直接回写密码,
                            // 不经 `write` —— 那是用户输入通道,不该被自动填充污染。
                            pump_autofill(&autofill_for_reader, &writer_for_reader, chunk);
                            on_output(chunk);
                        }) {
                            break;
                        }
                    }
                }
            }
        });

        let closing = Arc::new(std::sync::atomic::AtomicBool::new(false));
        spawn_exit_watcher(ExitWatcher {
            child: Arc::clone(&child),
            master: Arc::clone(&master),
            writer: Arc::clone(&writer),
            master_closer: master_closer.clone(),
            closing: Arc::clone(&closing),
            output_delivery: Arc::clone(&output_delivery),
            output_done: output_done_rx,
            output_drain_timeout,
            on_exit,
            on_exit_status,
        });

        Ok(Self {
            master,
            child,
            writer,
            master_closer,
            input_observer: Arc::new(Mutex::new(None)),
            autofill,
            last_size: Mutex::new((cols, rows)),
            closing,
            output_delivery,
            wsl_override: plan.wsl_override,
        })
    }

    /// cwd 命中 WSL UNC 时的启动器重写结果(未命中为 `None`)。
    /// 命中意味着**用户配置的 shell 被无视**,改用 `wsl.exe` 启动,值得提示一次。
    pub fn wsl_override(&self) -> Option<&WslOverride> {
        self.wsl_override.as_ref()
    }
    /// Returns the native child process identifier when the backend exposes it.
    ///
    /// The dedicated terminal host uses this only as an attachment diagnostic:
    /// stable routing continues to use `TerminalSessionId` plus
    /// `TerminalIncarnationId` rather than treating a reusable OS pid as identity.
    pub fn process_id(&self) -> Option<u32> {
        self.child.lock().process_id()
    }

    /// 挂一个写入路径的旁路观察器:每次 [`write`](Self::write) 的原始字节都会
    /// 先交给它,再写进 PTY。上层用它把「用户键入」转发给别的模块做分析
    /// (本 crate 不解释这些字节,也不因它们改变任何行为)。
    ///
    /// 两点约定:
    /// - 观察器在**调用 `write` 的线程**上同步执行,里面别做慢活;
    /// - 观察器里**不要回写同一个 `PtySession`**,会自锁。
    ///
    /// 注意 SSH 密码自动填充的回写不走 `write`,因此**不会**经过观察器 ——
    /// 明文密码不会漏给上层。
    pub fn set_input_observer<F>(&self, observer: F)
    where
        F: FnMut(&[u8]) + Send + 'static,
    {
        *self.input_observer.lock() = Some(Box::new(observer));
    }

    /// 摘掉输入观察器。
    pub fn clear_input_observer(&self) {
        *self.input_observer.lock() = None;
    }

    /// 注册 SSH 密码自动填充:后续 PTY 输出命中密码提示时自动回写一次密码。
    /// 再次调用会重置状态(覆盖密码、清除已完成标记)。
    ///
    /// `disarm_on_input`:用户首次真实输入时是否解除本 autofill —— 远程项目
    /// pane 传 `true`,「SSH 连接」菜单路径传 `false`
    /// (见 [`ssh::SshAutofill`] 的字段注释)。
    pub fn arm_ssh_autofill(&self, password: String, disarm_on_input: bool) {
        *self.autofill.lock() = Some(SshAutofill::new(password, disarm_on_input));
    }

    /// 用户向 PTY 真实输入时调用:仅当该 autofill 标了 `disarm_on_input` 才解除
    /// 并清除明文密码。[`write`](Self::write) 已自动调用它,一般无需手动调。
    ///
    /// 语义:SSH 认证阶段用户不打字(ssh 自驱动 publickey,失败才由 autofill 灌
    /// 密码);一旦用户按键即说明会话已进入交互 shell,此后 `su` / `mysql -p` /
    /// `passwd` 等以 "password:" 结尾的提示都不该再被灌入 SSH 登录密码 ——
    /// 尤其 publickey 登录成功时全程无密码提示、autofill 永不自解除,
    /// 不在此解除则它终身待命并泄露密码。
    pub fn disarm_ssh_autofill_on_user_input(&self) {
        let mut guard = self.autofill.lock();
        if guard.as_ref().is_some_and(SshAutofill::disarm_on_input) {
            *guard = None;
        }
    }

    /// 往 PTY 写字节(用户键入、粘贴、拖入的文件路径都走这里)。
    ///
    /// 顺序:通知输入观察器 → 解除 SSH 自动填充(焦点事件除外)→ 分块写入。
    /// 观察器排在写入**之前**:上层拿这一路做的判定(例如为焦点事件开一个
    /// 重绘冷却窗口)必须在子进程响应抵达 reader 之前就建立起来。
    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        if let Some(observer) = self.input_observer.lock().as_mut() {
            observer(bytes);
        }
        // 焦点进/出序列不是用户按键(TUI 开 DEC 1004 后由终端自动发送),
        // 据此解除会让认证期碰上焦点切换的会话再也填不进密码。
        if bytes != FOCUS_IN_SEQ && bytes != FOCUS_OUT_SEQ {
            self.disarm_ssh_autofill_on_user_input();
        }
        let mut writer = self.writer.lock();
        let writer = writer.as_mut().context("PTY 已关闭")?;
        write_chunked(&mut **writer, bytes)
    }

    /// 调整 PTY 尺寸。尺寸与上次相同时直接返回(见 [`resize_if_changed`](Self::resize_if_changed))。
    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        self.resize_if_changed(rows, cols).map(|_| ())
    }

    /// 同 [`resize`](Self::resize),但返回**是否真的下发了 resize**。
    ///
    /// 同尺寸去重:挂载 / 切 tab 等路径会重复上报未变的尺寸,而 ConPTY 收到
    /// resize(即使同尺寸)会让 TUI 应用整屏重绘 —— 帧高于视口时每次重绘都往
    /// scrollback 漏一份残留。尺寸没变就不透传。
    ///
    /// 上层若要在「真的 resize 了」之后做别的事(例如开一个重绘冷却窗口),
    /// 用这个返回值判断,不要自己再存一份尺寸。
    pub fn resize_if_changed(&self, rows: u16, cols: u16) -> Result<bool> {
        let mut last_size = self.last_size.lock();
        if *last_size == (cols, rows) {
            return Ok(false);
        }
        self.master
            .lock()
            .as_ref()
            .context("PTY 已关闭")?
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("resize 失败")?;
        *last_size = (cols, rows);
        Ok(true)
    }

    pub fn kill(&mut self) -> Result<()> {
        self.child.lock().kill().context("kill 失败")
    }

    /// 非阻塞地看一眼子进程是否已退出。
    pub fn try_wait(&mut self) -> Result<Option<u32>> {
        Ok(self.child.lock().try_wait()?.map(|s| s.exit_code()))
    }
}

impl Drop for PtySession {
    /// 先杀子进程,再把 master 丢到后台线程上销毁。
    ///
    /// Windows 上销毁 master 会触发 `ClosePseudoConsole()`,它是同步的,会一直
    /// 阻塞到该控制台会话里的每个进程都退出。子进程里还挂着长跑的 AI 进程时,
    /// 这一下在调用线程上永远不返回 —— 整个 UI 卡死成「未响应」。
    /// 先 kill 让 ConPTY 知道主进程没了,再后台销毁,UI 线程一秒都不等。
    fn drop(&mut self) {
        self.closing
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.output_delivery.close();
        let _ = self.child.lock().kill();
        drop(self.writer.lock().take());
        if let Some(master) = self.master.lock().take() {
            queue_background_drop(&self.master_closer, master);
        }
    }
}

fn spawn_drop_worker<T: Send + 'static>(name: &str) -> std::io::Result<std::sync::mpsc::Sender<T>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            for value in receiver {
                drop(value);
            }
        })?;
    Ok(sender)
}

fn queue_background_drop<T>(sender: &std::sync::mpsc::Sender<T>, value: T) {
    if let Err(error) = sender.send(value) {
        // Dropping here could be the unbounded ClosePseudoConsole call this
        // worker exists to isolate, so a failed worker deliberately leaks it.
        std::mem::forget(error.0);
    }
}

/// Windows ConPTY 无法一次处理大量输入数据(粘贴长文本时只剩最后一行)。
/// 将数据按行拆分,每行写入后加短暂延迟,给 ConPTY 时间消化。
/// 短数据(普通键盘输入)直接写入不受影响。
fn write_chunked(writer: &mut dyn Write, bytes: &[u8]) -> Result<()> {
    const CHUNK_THRESHOLD: usize = 128;
    const INTER_LINE_DELAY: Duration = Duration::from_millis(1);

    if !cfg!(windows) || bytes.len() <= CHUNK_THRESHOLD || !bytes.contains(&b'\n') {
        writer.write_all(bytes)?;
        writer.flush()?;
        return Ok(());
    }

    // 按行拆分写入,保留每行的换行符
    let mut start = 0;
    while start < bytes.len() {
        let end = match bytes[start..].iter().position(|&b| b == b'\n') {
            Some(pos) => start + pos + 1, // 包含 \n
            None => bytes.len(),          // 最后一段无换行
        };
        writer.write_all(&bytes[start..end])?;
        writer.flush()?;
        start = end;
        if start < bytes.len() {
            std::thread::sleep(INTER_LINE_DELAY);
        }
    }
    Ok(())
}

/// 把一段 PTY 输出喂给 SSH 密码自动填充;命中密码提示则直接回写密码 + 回车。
fn pump_autofill(
    autofill: &Arc<Mutex<Option<SshAutofill>>>,
    writer: &Arc<Mutex<Option<BoxedWriter>>>,
    chunk: &[u8],
) {
    let password = {
        let mut guard = autofill.lock();
        match guard.as_mut() {
            Some(state) if !state.is_done() => state.feed(&String::from_utf8_lossy(chunk)),
            _ => None,
        }
    };
    if let Some(password) = password {
        let mut writer = writer.lock();
        let Some(writer) = writer.as_mut() else {
            return;
        };
        let _ = writer.write_all(password.as_bytes());
        let _ = writer.write_all(b"\r");
        let _ = writer.flush();
    }
}

/// 退出监听线程:轮询子进程,退出后回调一次即结束。
///
/// 每轮只在锁内做一次**非阻塞** `try_wait`,绝不持锁阻塞等待 —— 否则并发的
/// [`PtySession::kill`] 会卡在锁上,把 UI 线程一起拖住。
///
/// 线程自终结:会话被销毁时 [`Drop`] 会 kill 子进程,下一轮 `try_wait` 即拿到
/// 结果退出循环(`closing` 已置位,不回调)。
fn spawn_exit_watcher(watcher: ExitWatcher) {
    use std::sync::atomic::Ordering;

    let ExitWatcher {
        child,
        master,
        writer,
        master_closer,
        closing,
        output_delivery,
        output_done,
        output_drain_timeout,
        mut on_exit,
        mut on_exit_status,
    } = watcher;

    std::thread::spawn(move || {
        let started = Instant::now();
        loop {
            if closing.load(Ordering::Relaxed) {
                return;
            }
            let exit_code = match child.lock().try_wait() {
                Ok(Some(status)) => Some(status.exit_code()),
                Ok(None) => {
                    // 还活着
                    let interval = if started.elapsed() < EXIT_POLL_FAST_WINDOW {
                        EXIT_POLL_FAST
                    } else {
                        EXIT_POLL_SLOW
                    };
                    std::thread::sleep(interval);
                    continue;
                }
                // 句柄已被回收之类:确定退不出更多信息,报「退出码未知」收场,
                // 免得线程空转到进程结束。
                Err(_) => None,
            };
            drop(writer.lock().take());
            if let Some(master) = master.lock().take() {
                queue_background_drop(&master_closer, master);
            }
            let status = wait_for_output_drain(&output_done, output_drain_timeout, exit_code);
            output_delivery.finalize(move || {
                if !closing.load(Ordering::Relaxed) {
                    if let Some(callback) = on_exit_status.take() {
                        callback(status);
                    }
                    if let Some(callback) = on_exit.take() {
                        callback(status.exit_code());
                    }
                }
            });
            return;
        }
    });
}

fn wait_for_output_drain(
    output_done: &std::sync::mpsc::Receiver<OutputPumpResult>,
    timeout: Duration,
    exit_code: Option<u32>,
) -> PtyExitStatus {
    match output_done.recv_timeout(timeout) {
        Ok(OutputPumpResult::Drained) => PtyExitStatus::Drained(exit_code),
        Ok(OutputPumpResult::Failed) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            PtyExitStatus::OutputDrainFailed(exit_code)
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            PtyExitStatus::OutputDrainTimedOut(exit_code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn smoke_spec() -> PtySpawn {
        let (program, args) = if cfg!(windows) {
            (
                "cmd.exe",
                vec!["/c".to_string(), "echo mt-pty-smoke".to_string()],
            )
        } else {
            (
                "/bin/sh",
                vec!["-c".to_string(), "echo mt-pty-smoke".to_string()],
            )
        };
        PtySpawn {
            program: program.to_string(),
            args,
            cwd: None,
            env: Vec::new(),
            rows: INITIAL_PTY_ROWS,
            cols: INITIAL_PTY_COLS,
        }
    }

    fn interactive_spec() -> PtySpawn {
        PtySpawn {
            program: if cfg!(windows) { "cmd.exe" } else { "/bin/sh" }.to_string(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            rows: INITIAL_PTY_ROWS,
            cols: INITIAL_PTY_COLS,
        }
    }

    // === 分块写入(Windows ConPTY 粘贴长文本会只剩最后一行) ===

    #[test]
    fn write_chunked_passes_short_input_through() {
        let mut sink: Vec<u8> = Vec::new();
        write_chunked(&mut sink, b"ls -la\r").unwrap();
        assert_eq!(sink, b"ls -la\r");
    }

    #[test]
    fn write_chunked_preserves_every_byte_of_long_multiline_paste() {
        // 拆行写入不得丢字节、不得改顺序:拼回来必须与原文逐字节相等。
        let payload: String = (0..40)
            .map(|i| format!("line {i} with some padding text\n"))
            .collect();
        let mut sink: Vec<u8> = Vec::new();
        write_chunked(&mut sink, payload.as_bytes()).unwrap();
        assert_eq!(sink, payload.as_bytes());
    }

    #[test]
    fn write_chunked_handles_trailing_segment_without_newline() {
        let payload = format!("{}\nno trailing newline", "x".repeat(200));
        let mut sink: Vec<u8> = Vec::new();
        write_chunked(&mut sink, payload.as_bytes()).unwrap();
        assert_eq!(sink, payload.as_bytes());
    }

    // === 端到端:起真进程 → 收输出 → 收退出码 ===

    #[test]
    fn spawn_streams_output_and_reports_exit_code() {
        enum SmokeEvent {
            Output(Vec<u8>),
            Exit(PtyExitStatus),
        }

        let (tx, rx) = mpsc::channel();
        let exit_tx = tx.clone();
        let session = PtySession::spawn_with_options(
            smoke_spec(),
            PtyOptions::default().on_exit_status(move |status| {
                let _ = exit_tx.send(SmokeEvent::Exit(status));
            }),
            move |bytes| {
                let _ = tx.send(SmokeEvent::Output(bytes.to_vec()));
            },
        )
        .expect("spawn 失败");

        let deadline = Instant::now() + Duration::from_secs(30);
        let mut output = Vec::new();
        let exit_status = loop {
            let event = rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("输出与退出回调未在 30s 内全部触发");
            match event {
                SmokeEvent::Output(bytes) => output.extend_from_slice(&bytes),
                SmokeEvent::Exit(status) => {
                    assert!(
                        output
                            .windows(b"mt-pty-smoke".len())
                            .any(|window| window == b"mt-pty-smoke"),
                        "退出回调不得越过最后一批 PTY 输出"
                    );
                    break status;
                }
            }
        };

        assert_eq!(exit_status, PtyExitStatus::Drained(Some(0)));
        assert!(
            session.master.lock().is_none(),
            "自然退出必须释放 PTY master"
        );
        assert!(
            session.writer.lock().is_none(),
            "自然退出必须释放 PTY writer"
        );
        drop(session);
    }

    #[test]
    fn output_drain_timeout_is_bounded_and_closes_late_output() {
        let (_done_tx, done_rx) = mpsc::channel::<OutputPumpResult>();
        let started = Instant::now();
        let status = wait_for_output_drain(&done_rx, Duration::from_millis(20), Some(7));

        assert_eq!(status, PtyExitStatus::OutputDrainTimedOut(Some(7)));
        assert!(
            started.elapsed() < Duration::from_millis(250),
            "输出泵超时必须有界"
        );
    }

    #[test]
    fn output_drain_failure_is_distinct_from_clean_eof() {
        let (done_tx, done_rx) = mpsc::channel();
        done_tx.send(OutputPumpResult::Failed).unwrap();

        assert_eq!(
            wait_for_output_drain(&done_rx, Duration::from_secs(1), Some(9)),
            PtyExitStatus::OutputDrainFailed(Some(9))
        );
    }

    #[test]
    fn timeout_finalization_defers_exit_until_admitted_output_finishes() {
        let delivery = Arc::new(OutputDelivery::new());
        let order = Arc::new(Mutex::new(Vec::new()));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let output_delivery = delivery.clone();
        let output_order = order.clone();
        let output = std::thread::spawn(move || {
            assert!(output_delivery.deliver(|| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                output_order.lock().push("output");
            }));
        });
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        let (_done_tx, done_rx) = mpsc::channel::<OutputPumpResult>();
        let started = Instant::now();
        let status = wait_for_output_drain(&done_rx, Duration::from_millis(20), Some(7));
        assert_eq!(status, PtyExitStatus::OutputDrainTimedOut(Some(7)));
        assert!(started.elapsed() < Duration::from_millis(250));

        let exit_started = Instant::now();
        let exit_order = order.clone();
        delivery.finalize(move || {
            exit_order.lock().push("exit");
        });
        assert!(exit_started.elapsed() < Duration::from_millis(100));
        assert!(order.lock().is_empty());
        assert!(
            !delivery.deliver(|| order.lock().push("late-output")),
            "a new output delivery was accepted after exit finalization"
        );

        release_tx.send(()).unwrap();
        output.join().unwrap();
        assert_eq!(&*order.lock(), &["output", "exit"]);
    }

    #[test]
    fn on_exit_keeps_the_legacy_option_code_signature() {
        let options = PtyOptions::default().on_exit(|exit_code: Option<u32>| {
            let _ = exit_code;
        });
        assert!(options.on_exit.is_some());
        assert!(options.on_exit_status.is_none());
    }

    #[test]
    fn blocking_resource_destruction_runs_off_the_watcher_path() {
        struct BlockingDrop {
            entered: mpsc::Sender<()>,
            release: mpsc::Receiver<()>,
        }

        impl Drop for BlockingDrop {
            fn drop(&mut self) {
                let _ = self.entered.send(());
                let _ = self.release.recv();
            }
        }

        let closer = spawn_drop_worker("mini-term-pty-test-closer").unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let started = Instant::now();
        queue_background_drop(
            &closer,
            BlockingDrop {
                entered: entered_tx,
                release: release_rx,
            },
        );
        assert!(started.elapsed() < Duration::from_millis(100));
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        release_tx.send(()).unwrap();
        drop(closer);
    }

    #[test]
    fn dropping_session_does_not_report_child_exit() {
        // 上层关 pane(drop)时 Drop 会 kill 子进程,那不是「子进程自己退出了」,
        // 不该回调 —— 否则 UI 刚删掉的 pane 又收到一条退出通知。
        let (tx, rx) = mpsc::channel();
        let session = PtySession::spawn_with_options(
            interactive_spec(),
            PtyOptions::default().on_exit(move |code| {
                let _ = tx.send(code);
            }),
            |_| {},
        )
        .expect("spawn 失败");
        drop(session);

        assert!(
            rx.recv_timeout(Duration::from_millis(800)).is_err(),
            "上层主动关闭会话不该触发退出回调"
        );
    }

    #[test]
    fn write_notifies_input_observer_with_raw_bytes() {
        let seen = Arc::new(Mutex::new(Vec::<u8>::new()));
        let observer_sink = Arc::clone(&seen);

        let session = PtySession::spawn(interactive_spec(), |_| {}).expect("spawn 失败");
        session.set_input_observer(move |bytes| observer_sink.lock().extend_from_slice(bytes));
        session.write(b"hello\r").expect("write 失败");
        assert_eq!(&*seen.lock(), b"hello\r");

        // 摘掉之后不再收到
        session.clear_input_observer();
        session.write(b"more\r").expect("write 失败");
        assert_eq!(&*seen.lock(), b"hello\r");
    }

    #[test]
    fn resize_dedupes_identical_size() {
        let session = PtySession::spawn(interactive_spec(), |_| {}).expect("spawn 失败");
        // 初始尺寸来自 spec,重复上报同尺寸不得下发 resize
        assert!(
            !session
                .resize_if_changed(INITIAL_PTY_ROWS, INITIAL_PTY_COLS)
                .unwrap()
        );
        assert!(session.resize_if_changed(30, 100).unwrap());
        assert!(!session.resize_if_changed(30, 100).unwrap());
    }

    // === SSH 自动填充在 session 层的解除语义 ===

    #[test]
    fn user_input_disarms_autofill_when_flagged() {
        let session = PtySession::spawn(interactive_spec(), |_| {}).expect("spawn 失败");
        session.arm_ssh_autofill("secret".into(), true);
        session.write(b"l").expect("write 失败");
        assert!(
            session.autofill.lock().is_none(),
            "远程项目 pane 的 autofill 应在用户首次输入后解除"
        );
    }

    #[test]
    fn user_input_keeps_autofill_when_not_flagged() {
        // 「SSH 连接」菜单路径:arm 后紧跟的 ssh 命令写入不得解除,
        // 否则密码提示到达前 autofill 已被删。
        let session = PtySession::spawn(interactive_spec(), |_| {}).expect("spawn 失败");
        session.arm_ssh_autofill("secret".into(), false);
        session.write(b"ssh u@h\r").expect("write 失败");
        assert!(session.autofill.lock().is_some());
    }

    #[test]
    fn focus_events_do_not_disarm_autofill() {
        // 焦点进/出序列由终端自动发送,不是用户按键 —— 认证期碰上焦点切换
        // 不能把 autofill 解除掉,否则密码永远灌不进去。
        let session = PtySession::spawn(interactive_spec(), |_| {}).expect("spawn 失败");
        session.arm_ssh_autofill("secret".into(), true);
        session.write(FOCUS_IN_SEQ).expect("write 失败");
        session.write(FOCUS_OUT_SEQ).expect("write 失败");
        assert!(session.autofill.lock().is_some());
    }

    /// 可从测试侧读回写入内容的 writer。
    struct SharedSink(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedSink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn pump_autofill_writes_password_and_newline_once() {
        let autofill = Arc::new(Mutex::new(Some(SshAutofill::new("secret".into(), false))));
        let written = Arc::new(Mutex::new(Vec::<u8>::new()));
        let sink: Arc<Mutex<Option<BoxedWriter>>> =
            Arc::new(Mutex::new(Some(Box::new(SharedSink(Arc::clone(&written))))));

        pump_autofill(&autofill, &sink, b"Last login: Mon\r\n");
        assert!(written.lock().is_empty(), "普通输出不该触发回写");

        pump_autofill(&autofill, &sink, b"root@host's password: ");
        assert_eq!(&*written.lock(), b"secret\r", "密码后必须补一个回车");

        // 再来一次提示不该重复灌
        pump_autofill(&autofill, &sink, b"root@host's password: ");
        assert_eq!(&*written.lock(), b"secret\r");
        assert!(autofill.lock().as_ref().is_some_and(SshAutofill::is_done));
    }
}
