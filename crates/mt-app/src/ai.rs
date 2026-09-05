//! AI 感知的接线层:把 [`mt_ai::AiPerception`] 装进 GPUI 壳。
//!
//! ```text
//! 用户键入 ─┬─→ perception.observe_input(pane_id, bytes)   ← 必须在写 PTY 之前
//!           └─→ pty.write(bytes)
//! 子进程输出 ┬─→ emulator.advance(bytes)
//!            └─→ perception.observe_output(pane_id, bytes)
//! hook / monitor ─→ StatusSink ─→ mpsc channel ─→ 主线程任务 ─→ AppStore ─→ cx.notify()
//! ```
//!
//! **为什么状态要过一道 channel**:hook server 与 500ms 轮询都在后台线程上,
//! 而 gpui 的 `Entity` 只能在主线程碰。与终端重绘唤醒同一套路数(见 `pane.rs`),
//! 后台线程只管往 channel 里丢,主线程上的前台任务醒来后再改 store。

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::channel::mpsc::{self, UnboundedSender};
use mt_ai::{AgentRoute, AiPerception, SessionIdentity, StatusChange, StatusSink};
use mt_identity::AgentEventId;
use parking_lot::Mutex;

/// 后台线程送上来的 AI 事件。稳定路由在进入 channel 前取快照；`pty_id`
/// 被复用时，旧事件因此仍携带旧 incarnation，由 store 拒绝。
pub enum AiEvent {
    /// 状态变化(原 `pty-status-change`)。
    Status {
        change: StatusChange,
        route: Option<AgentRoute>,
        event_id: AgentEventId,
        sequence: u64,
    },
    /// hook 上报的会话身份(原 `pty-ai-session`)。
    Session {
        identity: SessionIdentity,
        route: Option<AgentRoute>,
        event_id: AgentEventId,
        sequence: u64,
    },
}

struct ChannelSink {
    tx: UnboundedSender<AiEvent>,
    routes: Arc<Mutex<HashMap<u32, AgentRoute>>>,
    live_panes: Arc<Mutex<Vec<u32>>>,
    next_sequence: Arc<AtomicU64>,
}

impl ChannelSink {
    fn route(&self, pty_id: u32) -> Option<Option<AgentRoute>> {
        if !self.live_panes.lock().contains(&pty_id) {
            return None;
        }
        Some(self.routes.lock().get(&pty_id).cloned())
    }

    fn sequence(&self) -> Option<u64> {
        allocate_event_sequence(&self.next_sequence)
    }
}

impl StatusSink for ChannelSink {
    fn status_changed(&self, change: StatusChange) {
        let Some(route) = self.route(change.pty_id) else {
            return;
        };
        let Some(sequence) = self.sequence() else {
            eprintln!("[ai] agent event sequence exhausted; dropping status event");
            return;
        };
        let _ = self.tx.unbounded_send(AiEvent::Status {
            change,
            route,
            event_id: AgentEventId::new(),
            sequence,
        });
    }

    fn session_identified(&self, identity: SessionIdentity) {
        let Some(route) = self.route(identity.pty_id) else {
            return;
        };
        let Some(sequence) = self.sequence() else {
            eprintln!("[ai] agent event sequence exhausted; dropping session event");
            return;
        };
        let _ = self.tx.unbounded_send(AiEvent::Session {
            identity,
            route,
            event_id: AgentEventId::new(),
            sequence,
        });
    }
}

fn allocate_event_sequence(counter: &AtomicU64) -> Option<u64> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .ok()
        .and_then(|previous| previous.checked_add(1))
}

fn remote_agent_status_enabled_value(value: Option<&OsStr>) -> bool {
    value != Some(OsStr::new("0"))
}

pub fn remote_agent_status_enabled() -> bool {
    remote_agent_status_enabled_value(std::env::var_os("MINI_TERM_REMOTE_AGENT_STATUS").as_deref())
}

/// AI 感知 + 它需要的上层信息:活 pane 列表、稳定路由与 hook 端口。
#[derive(Clone)]
pub struct AiBridge {
    perception: AiPerception,
    /// monitor 线程每 500ms 读一次。`mt-ai` 不认识 PTY,列表只能由这里提供。
    live_panes: Arc<Mutex<Vec<u32>>>,
    /// 事件进入后台 channel 前读取的稳定路由。
    routes: Arc<Mutex<HashMap<u32, AgentRoute>>>,
    /// 本地 Hook、PTY fallback 与远端轮询共用的进程单调事件序号。
    next_sequence: Arc<AtomicU64>,
    data_dir: PathBuf,
}

impl AiBridge {
    /// 建桥并把接收端交出去。`hook_enabled` 为真时顺带起 hook server。
    pub fn new(hook_enabled: bool) -> (Self, mpsc::UnboundedReceiver<AiEvent>) {
        let (tx, rx) = mpsc::unbounded();
        let routes = Arc::new(Mutex::new(HashMap::new()));
        let live_panes = Arc::new(Mutex::new(Vec::new()));
        let next_sequence = Arc::new(AtomicU64::new(0));
        let perception = AiPerception::new(Arc::new(ChannelSink {
            tx,
            routes: routes.clone(),
            live_panes: live_panes.clone(),
            next_sequence: next_sequence.clone(),
        }));
        // 数据目录统一走 mt_config —— hook-server.json 与 usage.db 必须落在
        // 与装机版同一个目录下(见迁移文档的技术债清单)。
        let data_dir = crate::app_data_dir();

        let bridge = Self {
            perception,
            live_panes,
            routes,
            next_sequence,
            data_dir,
        };

        if hook_enabled
            && let Err(err) = bridge.perception.start_hook_server(bridge.data_dir.clone())
        {
            eprintln!("[ai] hook server 起不来: {err}");
        }

        // 输入检测那一路(无 hook 时的降级判定)不依赖 hook server,轮询恒开。
        let panes = bridge.live_panes.clone();
        bridge
            .perception
            .start_monitor(Box::new(move || panes.lock().clone()));

        (bridge, rx)
    }

    pub fn perception(&self) -> &AiPerception {
        &self.perception
    }

    /// hook server 端口;0 = 没起来。注入给子进程的 `MINITERM_HOOK_PORT`。
    pub fn hook_port(&self) -> u16 {
        self.perception.hooks().get_port()
    }

    /// 远端轮询在完成并通过 generation/epoch 围栏后从同一序号源取号。
    pub fn next_event_sequence(&self) -> Option<u64> {
        allocate_event_sequence(&self.next_sequence)
    }

    /// 运行时开关 hook server(设置页「Hook 事件」的落点,原 `toggle_hook_server`)。
    ///
    /// **起服务器要绑端口 + 写 `hook-server.json`**,调用方一律丢
    /// `cx.background_executor()`;成功了才写配置(原版 `handleToggleHook` 的同一
    /// 顺序 —— 端口被占时配置不该记成「已开」)。
    pub fn set_hook_enabled(&self, enabled: bool) -> Result<(), String> {
        self.perception
            .set_hook_server_enabled(&self.data_dir, enabled)
    }

    /// hook server 当前状态(原 `get_hook_status`)。纯内存读,不碰盘。
    pub fn hook_status(&self) -> mt_ai::HookStatusInfo {
        mt_ai::hook_server::hook_status(self.perception.hooks())
    }

    /// 登记一个活着的 pane 及其稳定路由(新建 PTY 后、自动 resume 前调用)。
    pub fn add_pane(&self, pane_id: u32, route: Option<AgentRoute>) {
        if let Some(route) = route {
            self.routes.lock().insert(pane_id, route);
        } else {
            self.routes.lock().remove(&pane_id);
        }
        let mut panes = self.live_panes.lock();
        if !panes.contains(&pane_id) {
            panes.push(pane_id);
        }
    }

    /// 注销 pane:先移除路由，再清轮询列表与 `mt-ai` 内部旁路状态。
    pub fn remove_pane(&self, pane_id: u32) {
        self.routes.lock().remove(&pane_id);
        self.live_panes.lock().retain(|id| *id != pane_id);
        self.perception.pane_closed(pane_id);
    }

    /// 退出时收摊:停 hook server 并删掉端口文件。
    ///
    /// 不做这一步的话,`hook-server.json` 会留着一个已经死掉的端口 —— 下一次
    /// 起的 AI 会话若没继承 `MINITERM_HOOK_PORT`,就会照着这个文件往空气里汇报。
    /// (装机版与 GPUI 壳同时在跑时两者会互抢这个文件,dev 期已知,见交付说明。)
    pub fn shutdown(&self) {
        // 没起过就别动那个文件 —— 它可能是另一个壳(装机版)的
        if self.hook_port() > 0 {
            self.perception.stop_hook_server(&self.data_dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollback_gate_disables_only_explicit_zero() {
        assert!(!remote_agent_status_enabled_value(Some(OsStr::new("0"))));
        assert!(remote_agent_status_enabled_value(None));
        assert!(remote_agent_status_enabled_value(Some(OsStr::new("1"))));
        assert!(remote_agent_status_enabled_value(Some(OsStr::new("false"))));
    }

    #[test]
    fn event_sequences_are_monotonic_and_fail_on_overflow() {
        let counter = AtomicU64::new(0);
        assert_eq!(allocate_event_sequence(&counter), Some(1));
        assert_eq!(allocate_event_sequence(&counter), Some(2));
        counter.store(u64::MAX, Ordering::Relaxed);
        assert_eq!(allocate_event_sequence(&counter), None);
    }

    #[test]
    fn observer_teardown_is_idempotent_and_suppresses_delayed_events() {
        let (tx, mut rx) = mpsc::unbounded();
        let sink = Arc::new(ChannelSink {
            tx,
            routes: Arc::new(Mutex::new(HashMap::new())),
            live_panes: Arc::new(Mutex::new(Vec::new())),
            next_sequence: Arc::new(AtomicU64::new(0)),
        });
        // No monitor, Hook server, PTY, or filesystem setup is needed here.
        let bridge = AiBridge {
            perception: AiPerception::new(sink.clone()),
            live_panes: sink.live_panes.clone(),
            routes: sink.routes.clone(),
            next_sequence: sink.next_sequence.clone(),
            data_dir: PathBuf::new(),
        };
        let host = mt_identity::ExecutionHostId::derive("test", &mt_identity::HostInstallId::new());
        let repo = mt_identity::RepoId::derive(&host, "/repo/.git");
        let route = AgentRoute {
            execution_host_id: host,
            worktree_id: mt_identity::WorktreeId::derive(&repo, "/repo", None),
            tab_id: mt_identity::TabId::new(),
            pane_key: mt_identity::PaneKey::new(),
            terminal_session_id: mt_identity::TerminalSessionId::new(),
            terminal_incarnation_id: mt_identity::TerminalIncarnationId::new(),
        };
        bridge.add_pane(7, Some(route.clone()));
        bridge.perception().observe_input(7, b"codex\r");
        let change = StatusChange {
            pty_id: 7,
            status: "ai-working".into(),
            cause: None,
            agent: Some("codex".into()),
        };
        sink.status_changed(change.clone());
        bridge.remove_pane(7);
        bridge.remove_pane(7);
        assert!(bridge.live_panes.lock().is_empty());
        assert!(bridge.routes.lock().is_empty());
        assert!(!bridge.perception().tracker().is_ai_session(7));
        assert_eq!(bridge.perception().status_of(7), "idle");
        sink.status_changed(change.clone());
        sink.session_identified(SessionIdentity {
            pty_id: 7,
            agent: Some("codex".into()),
            session_id: "delayed".into(),
            cwd: None,
        });
        assert_eq!(sink.next_sequence.load(Ordering::Relaxed), 1);
        let AiEvent::Status { route: captured, .. } = rx.try_recv().unwrap() else {
            panic!("expected the event queued before exit");
        };
        assert_eq!(captured, Some(route));
        assert!(rx.try_recv().is_err());

        bridge.add_pane(7, None);
        sink.status_changed(change);
        assert!(matches!(rx.try_recv().unwrap(), AiEvent::Status { route: None, .. }));
    }
}
