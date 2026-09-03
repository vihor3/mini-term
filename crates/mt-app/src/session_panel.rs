//! AI 历史面板(右侧抽屉)。对应 `src/components/SessionList.tsx`。
//!
//! ```text
//! 项目切换 ─→ refresh(force=false)
//!              ├─ background: mt_ai::sessions::get_ai_sessions(宿主,秒出)
//!              ├─ background: mt_ai::sessions::scan_session_lineage(分支边,树视图用)
//!              └─ background: mt_ai::sessions::get_wsl_ai_sessions(9P + 可能的 VM 冷启动)
//!                     ↓ 各自回主线程 setState,按时间戳降序混排
//! 右键一行 ─→ 查看 / 在当前终端恢复 / 新标签恢复 / 复制命令
//! 树模式点一行 ─→ 已在跑就跳过去,没在跑就新终端 resume
//! ```
//!
//! **三个慢函数必须丢后台**(看板技术债清单明示):`get_ai_session_content`、
//! `get_wsl_ai_sessions` 与 `scan_session_lineage` 原本靠 Tauri 命令层挪出主线程;
//! 现在是普通同步函数,WSL 冷启动秒级,落在 GPUI 主线程上就是整个窗口卡住。
//!
//! **惰性加载那道闸不许绕过**:`visible` / `stale` 双标记(收起时项目切换不去扫)
//! 是 GPUI 侧补的防线 —— 旧版收起时组件根本没挂载。分支边扫描同样挂在这道闸后面。
//!
//! # 来源三选一(唯一分流开关)
//!
//! [`crate::ssh_conn::session_source`] 是**唯一**判据(BB-b 接上):
//!
//! ```text
//! Remote(conn)  → 只走 remote_ssh::ai_sessions([后台]);宿主 / lineage / WSL
//!                 三路一条都不发 —— 本地 `get_ai_sessions` 对远程 POSIX 路径
//!                 会去本机 `~/.claude/projects` 找同名编码目录,命中的是
//!                 **另一台机器**上同路径的会话
//! BrokenRemote  → 空表 + 断链提示。**绝不退回 Local**(同上,会把本机会话
//!                 贴到远程项目上)
//! Local         → 宿主 + lineage + 可选 WSL,三路并发,照旧
//! ```
//!
//! # 与旧版的偏差
//!
//! - 会话正文查看是**面板内预览**而不是独立弹窗(形态不同,动作齐全);
//! - **断链时多一句提示**(原版是静默空列表:它的 `ssh_remote_ai_sessions`
//!   在后端把断链吞成空表,前端看不出区别)。空列表配「一条会话都没有」会让人
//!   以为远端真没会话,而实情是连接被删了 —— 记为改良。
//!
//! # 与 [`crate::branch_family`] 的分工
//!
//! pane 右键「查看会话分支」的悬停家族面板是**另一个组件**:它挂在菜单的自绘
//! 子菜单挂载点上、取数口是 pane 自己的 `ai_session`、只画那一支家族。两边共用
//! 的是纯逻辑([`crate::session_branch`])与节点点击行为([`jump_to_session`])
//! —— 点同一种节点必须有同一种行为。

use std::collections::HashMap;

use gpui::{
    AnyElement, App, Context, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Render, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Task,
    Window, div, prelude::FluentBuilder, px,
};
use gpui_component::ActiveTheme as _;
use gpui_component::scroll::Scrollbar;
use gpui_component::text::{TextView, TextViewStyle};
use mt_ai::{
    AgentActivity, AgentConnectivity, AgentProvider,
    sessions::{AiSession, AiSessionMessage, LineageEdge},
};
use mt_identity::WorktreeId;
use mt_ui::icons::{AiVendor, BrandIcon, StatusDot, StatusKind};
use mt_ui::tooltip::Tooltip;

use crate::i18n::{t, tr};
use crate::menu;
use crate::notify::ToastKind;
use crate::pane::TerminalRecovery;
use crate::session_branch::{build_session_tree, flatten_session_tree, merge_lineage_edges};
use crate::store::{
    AgentTargetView, AppStore, RemoteAgentProbeCapability, TerminalDiagnosticView,
    orca_worktree_context_enabled,
};
use crate::toast;
use crate::tree::{AiSessionRef, PaneStatus};
use crate::ui;

/// 一页多少条(与旧版 `PAGE_SIZE` 同值)。
const PAGE_SIZE: usize = 20;

/// 会话正文一屏渲染多少条消息。
///
/// **不是观感取舍,是硬约束**:每条消息的正文是一个 [`TextView`],首帧那次
/// markdown 解析(含代码块高亮)是**同步**跑在主线程上的(组件只把后续更新
/// 丢后台),且每个 TextView 常驻一前一后两个 task。一个长会话上千条消息全铺
/// 出去就是开面板即卡窗口 —— 所以按页给,底下留「加载更多」。
const PREVIEW_PAGE_SIZE: usize = 40;

/// 该会话对应的 resume 命令;id 形态异常返回 `None`。
///
/// sessionId 会被原样拼进写进 PTY 的命令行,必须过白名单:字母数字与 `-_`
/// (Claude UUID、Codex rollout id 与 Grok UUIDv7 的实际形态)。两个来源
/// ——持久化布局与会话记录文件内容——都不是可信输入,空格/引号/管道/换行
/// 等一切 shell 元字符在此拦截(逐条对照 `src/utils/aiResume.ts`)。
pub fn build_resume_command(agent: &str, session_id: &str) -> Option<String> {
    if session_id.is_empty() || session_id.len() > 128 {
        return None;
    }
    if !session_id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    Some(match agent {
        "codex" => format!("codex resume {session_id}"),
        "grok" => format!("grok --resume {session_id}"),
        _ => format!("claude --resume {session_id}"),
    })
}

/// 点一个会话节点该发生什么。对应 `src/utils/sessionJump.ts::jumpToSession`。
///
/// ```text
/// live 存在   → 切项目 + 激活那个 pane
/// live 不存在 →
///   ├─ 会话来自 WSL / 远程 → 提示「无法在本机恢复」,不做任何事
///   ├─ agent 无 resume 能力 → 静默不做
///   └─ 否则 → 新开终端 + 写 resume 命令 + 回写会话身份
/// ```
///
/// **自由函数**而不是 [`SessionPanel`] 的方法:分支树视图(本面板)与
/// pane 右键的悬停家族面板([`crate::branch_family`])是两个组件、点同一种节点,
/// 行为必须逐字相同。返回的 `Task` 由调用方持有 —— 丢掉就等于取消。
pub(crate) fn jump_to_session(
    store: &Entity<AppStore>,
    session: AiSession,
    window: &mut Window,
    cx: &mut App,
) -> Task<()> {
    if orca_worktree_context_enabled() {
        let run_id = {
            let store = store.read(cx);
            store.active_worktree_id().and_then(|worktree_id| {
                let targets = store.agent_target_views_for_worktree(worktree_id);
                session_agent_target(&session.session_type, &session.id, &targets)
                    .map(|target| target.run_id.clone())
            })
        };
        if let Some(run_id) = run_id {
            AppStore::activate_agent_run(store, &run_id, window, cx);
            return Task::ready(());
        }
    } else if let Some((project_id, pane_id, _)) =
        store.read(cx).find_live_session_pane(&session.id)
    {
        store.update(cx, |store, cx| {
            store.set_active_project(&project_id, cx);
            store.activate_pane(&project_id, &pane_id, window, cx);
        });
        crate::workbench_area::activate_terminal_page(window, cx);
        return Task::ready(());
    }

    // WSL / SSH 远程来源的会话记录在别的机器/发行版里,本地 resume 恢复不了。
    // 走 toast(原版 `pushNotification`,kind `wsl-info`)—— Q 批写这段时
    // mt-app 还没有 toast 体系,退而用通用提示框;Z 批的 `toast.rs` 到位后回换,
    // 与原版同形(不打断、右下角自动消失)。
    if session.wsl_distro.is_some() || session.ssh_connection_id.is_some() {
        let (project_id, project_name) = store
            .read(cx)
            .active_project()
            .map(|p| (p.id.clone(), p.name.clone()))
            .unwrap_or_default();
        toast::push_message(
            ToastKind::WslInfo,
            project_id,
            project_name,
            t("sessionList", "branchTree.remoteResumeUnsupported").to_string(),
            cx,
        );
        return Task::ready(());
    }

    let Some(command) = build_resume_command(&session.session_type, &session.id) else {
        // opencode / pi 之类没有 resume 能力的 agent:静默不做
        return Task::ready(());
    };
    let Some(project_id) = store.read(cx).active_project_id.clone() else {
        return Task::ready(());
    };
    let anchor = store.read(cx).active_pane_id(&project_id);
    // `claude --resume` 只认「启动目录」对应的会话桶:子目录里起的会话在项目根
    // 恢复会报 `No conversation found`,先反查记录的 cwd。codex 不按目录分桶;
    // grok 虽按 cwd 分桶,但列表只捞「解码目录名全等于项目根」的会话。
    // 反查是**同步磁盘遍历**,跳转路径上照样丢后台。
    let needs_cwd = session.session_type == "claude";
    let session_id = session.id.clone();
    let store = store.clone();
    window.spawn(cx, async move |cx| {
        let cwd = if needs_cwd {
            cx.background_executor()
                .spawn(async move { mt_ai::sessions::lookup_ai_session_cwd(session_id) })
                .await
        } else {
            None
        };
        let _ = cx.update(|window, cx| {
            store.update(cx, |store, cx| {
                // 用**返回的那个 pane**,不能事后再取活动 pane:焦点还没落下去
                let Some(pane_id) =
                    store.new_terminal_with_cwd(&project_id, None, anchor, cwd.clone(), window, cx)
                else {
                    return;
                };
                store.write_to_pane(&project_id, &pane_id, &format!("{command}\r"), cx);
                // 恢复出的会话身份**当场**写回 pane,不等 hook ——
                // codex resume 不会重新上报 SessionStart
                store.set_pane_ai_session(
                    &project_id,
                    &pane_id,
                    AiSessionRef {
                        agent: Some(session.session_type.clone()),
                        session_id: session.id.clone(),
                        cwd,
                    },
                    cx,
                );
                store.focus_pane(&project_id, &pane_id, window, cx);
            });
            crate::workbench_area::activate_terminal_page(window, cx);
        });
    })
}

/// 项目是否有 WSL 会话来源:UNC 形态的 WSL 根项目,或显式配置了发行版。
///
/// (`mt_ai` 的 `parse_wsl_unc` 目前是 crate 私有,这里按前缀判一道;
/// 见交付说明的「接线需求」。)
fn has_wsl_source(path: &str, distro: Option<&str>) -> bool {
    if distro.is_some_and(|d| !d.is_empty()) {
        return true;
    }
    let lower = path.to_ascii_lowercase().replace('/', "\\");
    lower.starts_with("\\\\wsl$\\") || lower.starts_with("\\\\wsl.localhost\\")
}

/// `PaneStatus` → `StatusKind`(mt-ui 不能反向依赖 mt-app,在这里转一次)。
fn status_kind(status: PaneStatus) -> StatusKind {
    match status {
        PaneStatus::Idle => StatusKind::Idle,
        PaneStatus::AiIdle => StatusKind::AiIdle,
        PaneStatus::AiWorking => StatusKind::AiWorking,
        PaneStatus::Error => StatusKind::Error,
    }
}

fn agent_vendor(provider: &AgentProvider) -> Option<AiVendor> {
    match provider.as_str() {
        AgentProvider::CODEX => Some(AiVendor::OpenAi),
        other => AiVendor::from_session_type(other).or_else(|| AiVendor::infer(Some(other), None)),
    }
}

fn agent_activity_label(activity: AgentActivity) -> &'static str {
    match activity {
        AgentActivity::Starting => "Starting",
        AgentActivity::Working => "Working",
        AgentActivity::Blocked => "Needs you",
        AgentActivity::Waiting => "Waiting",
        AgentActivity::Done => "Done",
        AgentActivity::Failed => "Failed",
        AgentActivity::Interrupted => "Interrupted",
        AgentActivity::Exited => "Exited",
        AgentActivity::Unknown => "Unknown",
    }
}

fn agent_activity_color(activity: AgentActivity, attention: bool) -> gpui::Hsla {
    if attention || matches!(activity, AgentActivity::Blocked | AgentActivity::Failed) {
        ui::color_warning()
    } else if matches!(activity, AgentActivity::Starting | AgentActivity::Working) {
        ui::accent()
    } else if matches!(activity, AgentActivity::Done | AgentActivity::Waiting) {
        ui::color_success()
    } else {
        ui::text_muted()
    }
}

fn agent_connectivity_label(connectivity: AgentConnectivity) -> &'static str {
    match connectivity {
        AgentConnectivity::Live => "Live",
        AgentConnectivity::Stale => "Stale",
        AgentConnectivity::Disconnected => "Offline",
    }
}

fn agent_connectivity_color(connectivity: AgentConnectivity) -> gpui::Hsla {
    match connectivity {
        AgentConnectivity::Live => ui::color_success(),
        AgentConnectivity::Stale => ui::color_warning(),
        AgentConnectivity::Disconnected => ui::text_muted(),
    }
}

fn terminal_recovery_label(recovery: TerminalRecovery) -> &'static str {
    match recovery {
        TerminalRecovery::Fresh => "Fresh",
        TerminalRecovery::Reattached => "Warm reattach",
        TerminalRecovery::RestoredHistory => "Restored history",
        TerminalRecovery::Compatibility => "Compatibility",
        TerminalRecovery::Unavailable => "Unavailable",
    }
}

fn terminal_recovery_color(recovery: TerminalRecovery) -> gpui::Hsla {
    match recovery {
        TerminalRecovery::Fresh => ui::text_muted(),
        TerminalRecovery::Reattached => ui::color_success(),
        TerminalRecovery::RestoredHistory => ui::color_info(),
        TerminalRecovery::Compatibility => ui::color_warning(),
        TerminalRecovery::Unavailable => ui::color_error(),
    }
}

fn remote_probe_label(capability: RemoteAgentProbeCapability, process_count: usize) -> String {
    match capability {
        RemoteAgentProbeCapability::Unknown => "Detecting".to_string(),
        RemoteAgentProbeCapability::LinuxProc => format!("Linux probe · {process_count}"),
        RemoteAgentProbeCapability::Unsupported => "Unsupported".to_string(),
    }
}

fn agent_session_identity_matches(
    provider: &AgentProvider,
    provider_session_id: Option<&str>,
    session_type: &str,
    session_id: &str,
) -> bool {
    let Ok(session_provider) = session_type.parse::<AgentProvider>() else {
        return false;
    };
    provider == &session_provider && provider_session_id == Some(session_id)
}

fn session_agent_target<'a>(
    session_type: &str,
    session_id: &str,
    targets: &'a [AgentTargetView],
) -> Option<&'a AgentTargetView> {
    targets.iter().find(|target| {
        agent_session_identity_matches(
            &target.provider,
            target.provider_session_id.as_deref(),
            session_type,
            session_id,
        )
    })
}

/// ISO 8601 → 「刚刚 / n 分钟前 / n 小时前 / n 天前 / 月-日」。
fn format_time(iso: &str) -> String {
    let Ok(ts) = chrono::DateTime::parse_from_rfc3339(iso) else {
        return String::new();
    };
    let now = chrono::Local::now();
    let minutes = (now.timestamp() - ts.timestamp()) / 60;
    if minutes < 1 {
        return t("sessionList", "time.justNow").to_string();
    }
    if minutes < 60 {
        return tr!("sessionList", "time.minutesAgo", n = minutes);
    }
    let hours = minutes / 60;
    if hours < 24 {
        return tr!("sessionList", "time.hoursAgo", n = hours);
    }
    let days = hours / 24;
    if days < 7 {
        return tr!("sessionList", "time.daysAgo", n = days);
    }
    let local = ts.with_timezone(&chrono::Local);
    use chrono::Datelike;
    if local.year() == now.year() {
        tr!(
            "sessionList",
            "time.monthDay",
            m = local.month(),
            d = local.day()
        )
    } else {
        format!("{}/{}/{}", local.year(), local.month(), local.day())
    }
}

/// 消息头那一行的时间(ISO 8601 → 本地时钟 `月-日 时:分:秒`)。
///
/// 会话列表那边给的是「几分钟前」(找会话用),正文里要的是**绝对时刻** ——
/// 看一段对话是什么时候发生的、两条之间隔了多久,相对时间答不了。带秒是因为
/// 相邻消息常落在同一分钟内。
///
/// 时间戳解析不出来(远古记录 / 字段缺失)返回 `None`,那条就不显示时间 ——
/// 宁可少一行灰字,也不显示 `1970-01-01`。
fn format_message_time(iso: &str) -> Option<String> {
    let ts = chrono::DateTime::parse_from_rfc3339(iso.trim()).ok()?;
    Some(
        ts.with_timezone(&chrono::Local)
            .format("%m-%d %H:%M:%S")
            .to_string(),
    )
}

/// 会话正文的富文本排版。取自 [`crate::file_viewer`] 的 markdown 预览那份,
/// 按抽屉里的 12px 正文重新定基准(那边是 14px 的文档视图)。
fn preview_text_style(cx: &mut App) -> TextViewStyle {
    let mut code_block = gpui::StyleRefinement::default();
    {
        let text = code_block.text.get_or_insert_default();
        text.font_size = Some(ui::font_px(11.0).into());
        text.line_height = Some(gpui::relative(1.5).into());
    }
    TextViewStyle {
        highlight_theme: cx.theme().highlight_theme.clone(),
        is_dark: cx.theme().mode.is_dark(),
        heading_base_font_size: ui::font_px(12.0),
        // 抽屉窄,段距按文档视图的 1rem 给会把一条消息撑得很散
        paragraph_gap: gpui::rems(0.5),
        code_block,
        ..Default::default()
    }
    .heading_font_size(|level, base| match level {
        1 => base * 1.4,
        2 => base * 1.2,
        3 => base * 1.1,
        _ => base,
    })
}

/// 会话正文预览的一次加载。
struct Preview {
    /// 会话 id。只用于给每条消息的 [`TextView`] 拼稳定且**跨会话不撞**的
    /// element id —— 光按序号编的话,换一个会话看会命中上一个会话同序号那条
    /// 的缓存状态,首帧显示的是别人的正文。
    session_id: String,
    session_type: String,
    wsl_distro: Option<String>,
    title: String,
    loading: bool,
    error: Option<String>,
    messages: Vec<AiSessionMessage>,
    /// 与 `messages` 同下标的安全渲染副本；复制动作仍使用原始正文。
    rendered_messages: Vec<String>,
    /// 已铺出去的条数(见 [`PREVIEW_PAGE_SIZE`])。
    shown: usize,
    /// 可复制的 resume 命令(拼不出来则为 None)。
    command: Option<String>,
    scroll: ScrollHandle,
}

impl Preview {
    /// 「复制全文」的文本:整份对话按 `角色 · 时间` + 正文平铺。
    ///
    /// 逐条选中复制是 [`TextView`] 的能力,但它的选区**只在单个 TextView 内**
    /// (选区坐标是相对自己 bounds 算的),跨消息拖不出来 —— 整份对话只能由
    /// 这里拼。
    fn all_text(&self) -> String {
        self.messages
            .iter()
            .map(|m| {
                let role = if m.role == "user" {
                    "User"
                } else {
                    "Assistant"
                };
                match format_message_time(&m.timestamp) {
                    Some(time) => format!("{role} · {time}\n{}", m.content),
                    None => format!("{role}\n{}", m.content),
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// 平铺 / 分支树两种视图。取值与 `AppConfig::session_list_view` 的字面量同。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ViewMode {
    Flat,
    Tree,
}

impl ViewMode {
    fn key(self) -> &'static str {
        match self {
            Self::Flat => "flat",
            Self::Tree => "tree",
        }
    }

    fn toggled(self) -> Self {
        match self {
            Self::Flat => Self::Tree,
            Self::Tree => Self::Flat,
        }
    }
}

fn session_source_signature(
    project_id: &str,
    path: &str,
    wsl_distro: Option<&str>,
    source: &crate::ssh_conn::SessionSource,
) -> String {
    match source {
        crate::ssh_conn::SessionSource::Local => format!(
            "{project_id}|{path}|local|wsl:{}",
            wsl_distro.unwrap_or_default()
        ),
        crate::ssh_conn::SessionSource::BrokenRemote => {
            format!("{project_id}|{path}|ssh:broken")
        }
        crate::ssh_conn::SessionSource::Remote(connection) => format!(
            "{project_id}|{path}|ssh:{}:{:016x}",
            connection.id,
            crate::remote_ssh::connection_fingerprint(connection)
        ),
    }
}

fn session_scope_changed(
    cache_enabled: bool,
    current_worktree: Option<&WorktreeId>,
    next_worktree: Option<&WorktreeId>,
    current_path: Option<&str>,
    next_path: Option<&str>,
    current_source: Option<&str>,
    next_source: Option<&str>,
) -> bool {
    current_path != next_path
        || (cache_enabled && (current_worktree != next_worktree || current_source != next_source))
}

fn session_scope_request_matches(
    request_generation: u64,
    current_generation: u64,
    request_worktree: Option<&WorktreeId>,
    current_worktree: Option<&WorktreeId>,
    request_source: Option<&str>,
    current_source: Option<&str>,
) -> bool {
    request_generation == current_generation
        && request_worktree == current_worktree
        && request_source == current_source
}

fn loading_preview_needs_restart(preview: Option<&Preview>) -> bool {
    preview.is_some_and(|preview| preview.loading)
}

struct SessionScopeState {
    host: Vec<AiSession>,
    wsl: Vec<AiSession>,
    lineage: Vec<LineageEdge>,
    bookkept: Vec<LineageEdge>,
    display_count: usize,
    view: ViewMode,
    remote: bool,
    remote_broken: bool,
    preview: Option<Preview>,
    preview_refresh_needed: bool,
    list_scroll: ScrollHandle,
}

pub struct SessionPanel {
    store: Entity<AppStore>,
    /// 上次拉取用的 canonical worktree 路径。
    project_path: Option<String>,
    current_worktree: Option<WorktreeId>,
    source_signature: Option<String>,
    scope_cache: HashMap<WorktreeId, SessionScopeState>,
    scope_generation: u64,
    host: Vec<AiSession>,
    wsl: Vec<AiSession>,
    /// 磁盘扫描出来的分支边。树视图的数据面,平铺不消费。
    lineage: Vec<LineageEdge>,
    /// 自记账边(`AppConfig::session_lineage`)。本面板**只读**;
    /// 写入端是 pane 右键的「分支会话到新分屏」
    /// (`pane_actions::fork_pane_session` → `AppStore::consume_pending_fork`)。
    bookkept: Vec<LineageEdge>,
    loading: bool,
    wsl_loading: bool,
    display_count: usize,
    /// 平铺 / 树。**存本地态而不是每次 render 去读 config** ——
    /// 面板已经 `cx.observe(&store)`,读 config 会让每次 store 变化都重算。
    view: ViewMode,
    /// 请求序号:项目切换后旧请求(尤其是慢的 WSL)返回时不得覆盖新项目的列表。
    request_id: u64,
    /// 抽屉是否展开。旧版 `SessionList` 挂在 `RightDrawer` 里,收起时压根不挂载,
    /// 自然也不会去扫会话 —— 这里是常驻实体,只能自己记一份可见性。
    visible: bool,
    /// 关着的时候项目切过 → 打开时补拉一次。
    stale: bool,
    /// 当前项目是 SSH 远程项目(转圈提示要区分 WSL 与远程两种来源)。
    remote: bool,
    /// 断链的远程项目:列表恒空,头部给一句断链提示。
    remote_broken: bool,
    preview: Option<Preview>,
    preview_request: u64,
    preview_refresh_pending: bool,
    list_scroll: ScrollHandle,
    _tasks: Vec<Task<()>>,
}

impl SessionPanel {
    pub fn new(store: Entity<AppStore>, cx: &mut Context<Self>) -> Self {
        cx.observe(&store, |this: &mut Self, _, cx| {
            let (worktree_id, path, source_signature) = this.active_scope(cx);
            if session_scope_changed(
                orca_worktree_context_enabled(),
                this.current_worktree.as_ref(),
                worktree_id.as_ref(),
                this.project_path.as_deref(),
                path.as_deref(),
                this.source_signature.as_deref(),
                source_signature.as_deref(),
            ) {
                this.switch_scope(worktree_id, path, source_signature, cx);
                if this.visible {
                    this.refresh(false, cx);
                } else {
                    // 收着的时候不去扫:WSL 那一路要冷启动整台 VM,不该由「切了个
                    // 项目」触发(旧版收起时组件根本没挂载)
                    this.stale = true;
                }
            }
            cx.notify();
        })
        .detach();
        let view = match store.read(cx).session_list_view() {
            "tree" => ViewMode::Tree,
            _ => ViewMode::Flat,
        };
        Self {
            store,
            project_path: None,
            current_worktree: None,
            source_signature: None,
            scope_cache: HashMap::new(),
            scope_generation: 0,
            host: Vec::new(),
            wsl: Vec::new(),
            lineage: Vec::new(),
            bookkept: Vec::new(),
            loading: false,
            wsl_loading: false,
            display_count: PAGE_SIZE,
            view,
            request_id: 0,
            visible: false,
            stale: true,
            remote: false,
            remote_broken: false,
            preview: None,
            preview_request: 0,
            preview_refresh_pending: false,
            list_scroll: ScrollHandle::new(),
            _tasks: Vec::new(),
        }
    }

    fn active_scope(&self, cx: &App) -> (Option<WorktreeId>, Option<String>, Option<String>) {
        let store = self.store.read(cx);
        let Some(project) = store.active_project() else {
            return (None, None, None);
        };
        let path = store
            .canonical_worktree_path_for_project(&project.id)
            .unwrap_or(&project.path)
            .to_string();
        let source = crate::ssh_conn::session_source(project, store.ssh_connections());
        let signature = session_source_signature(
            &project.id,
            &path,
            project.wsl_sessions_distro.as_deref(),
            &source,
        );
        (
            store.active_worktree_id().cloned(),
            Some(path),
            Some(signature),
        )
    }

    fn default_view(&self, cx: &App) -> ViewMode {
        match self.store.read(cx).session_list_view() {
            "tree" => ViewMode::Tree,
            _ => ViewMode::Flat,
        }
    }

    fn save_scope(&mut self) {
        let Some(worktree_id) = self.current_worktree.clone() else {
            return;
        };
        let preview_refresh_needed =
            self.preview_refresh_pending || loading_preview_needs_restart(self.preview.as_ref());
        self.scope_cache.insert(
            worktree_id,
            SessionScopeState {
                host: std::mem::take(&mut self.host),
                wsl: std::mem::take(&mut self.wsl),
                lineage: std::mem::take(&mut self.lineage),
                bookkept: std::mem::take(&mut self.bookkept),
                display_count: self.display_count,
                view: self.view,
                remote: self.remote,
                remote_broken: self.remote_broken,
                preview: self.preview.take(),
                preview_refresh_needed,
                list_scroll: std::mem::replace(&mut self.list_scroll, ScrollHandle::new()),
            },
        );
    }

    fn restore_scope(&mut self, worktree_id: Option<&WorktreeId>, default_view: ViewMode) -> bool {
        let preview_refresh_needed = if let Some(state) =
            worktree_id.and_then(|worktree_id| self.scope_cache.remove(worktree_id))
        {
            self.host = state.host;
            self.wsl = state.wsl;
            self.lineage = state.lineage;
            self.bookkept = state.bookkept;
            self.display_count = state.display_count;
            self.view = state.view;
            self.remote = state.remote;
            self.remote_broken = state.remote_broken;
            self.preview = state.preview;
            self.list_scroll = state.list_scroll;
            state.preview_refresh_needed
        } else {
            self.host.clear();
            self.wsl.clear();
            self.lineage.clear();
            self.bookkept.clear();
            self.display_count = PAGE_SIZE;
            self.view = default_view;
            self.remote = false;
            self.remote_broken = false;
            self.preview = None;
            self.list_scroll = ScrollHandle::new();
            false
        };
        self.loading = false;
        self.wsl_loading = false;
        preview_refresh_needed
    }

    fn switch_scope(
        &mut self,
        worktree_id: Option<WorktreeId>,
        path: Option<String>,
        source_signature: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let default_view = self.default_view(cx);
        let source_changed_in_place =
            self.current_worktree == worktree_id && self.source_signature != source_signature;
        let preview_refresh_needed = if orca_worktree_context_enabled() {
            self.save_scope();
            self.restore_scope(worktree_id.as_ref(), default_view)
        } else {
            self.scope_cache.clear();
            self.restore_scope(None, default_view);
            false
        };
        self.scope_generation = self.scope_generation.wrapping_add(1);
        self.request_id = self.request_id.wrapping_add(1);
        self.preview_request = self.preview_request.wrapping_add(1);
        self._tasks.clear();
        self.current_worktree = worktree_id;
        self.project_path = path;
        self.source_signature = source_signature;
        self.stale = true;
        self.preview_refresh_pending =
            preview_refresh_needed || (source_changed_in_place && self.preview.is_some());
    }

    fn toggle_view(&mut self, cx: &mut Context<Self>) {
        self.view = self.view.toggled();
        let key = self.view.key();
        self.store
            .update(cx, |store, cx| store.set_session_list_view(key, cx));
        cx.notify();
    }

    /// 抽屉开合。第一次展开(或关着的时候项目切过)在这里补拉。
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        let (worktree_id, path, source_signature) = self.active_scope(cx);
        if session_scope_changed(
            orca_worktree_context_enabled(),
            self.current_worktree.as_ref(),
            worktree_id.as_ref(),
            self.project_path.as_deref(),
            path.as_deref(),
            self.source_signature.as_deref(),
            source_signature.as_deref(),
        ) {
            self.switch_scope(worktree_id, path, source_signature, cx);
        }
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if visible && self.stale {
            self.refresh(false, cx);
        }
    }

    /// 重拉两个来源 + 分支边。`force` 绕过 `mt_ai` 的会话缓存(手动刷新用)。
    ///
    /// **只在 `visible` 为真时被调用**(见 `new` 里的 observe 与 [`set_visible`])——
    /// 新增的分支边扫描同样挂在这道闸后面,不许绕过去直接拉。
    ///
    /// [`set_visible`]: Self::set_visible
    pub fn refresh(&mut self, force: bool, cx: &mut Context<Self>) {
        let (project, source) = {
            let store = self.store.read(cx);
            let project = store.active_project().map(|project| {
                (
                    store
                        .canonical_worktree_path_for_project(&project.id)
                        .unwrap_or(&project.path)
                        .to_string(),
                    project.wsl_sessions_distro.clone(),
                )
            });
            // **唯一的来源分流开关**(见模块注释):三条并发请求与远程那一条
            // 不会同时发出
            let source = store
                .active_project()
                .map(|p| crate::ssh_conn::session_source(p, store.ssh_connections()));
            (project, source)
        };
        // 自记账边:mini-term 自己发起的 fork 当场记下的 child→parent。
        // **必须传** —— Claude 的 CLI fork 不写磁盘指针,这些边的「分叉后第一问」
        // 标题只能由 mt-ai 拿父子文件比对补出。两个 crate 各持一份同构结构,
        // 转换是上层的活(`sessions.rs:1480-1484` 的设计)。
        let saved = self.store.read(cx).config().session_lineage.clone();
        let bookkept: Vec<mt_ai::sessions::BookkeptLineageEdge> = saved
            .iter()
            .map(|e| mt_ai::sessions::BookkeptLineageEdge {
                agent: e.agent.clone(),
                session_id: e.session_id.clone(),
                parent_session_id: e.parent_session_id.clone(),
                fork_point_uuid: e.fork_point_uuid.clone(),
            })
            .collect();
        // 同一批边留一份 `LineageEdge` 形态,给「扫描失败」时的兜底合并用
        self.bookkept = saved
            .iter()
            .map(|e| LineageEdge {
                agent: e.agent.clone(),
                session_id: e.session_id.clone(),
                parent_session_id: e.parent_session_id.clone(),
                fork_point_uuid: e.fork_point_uuid.clone(),
                branch_title: None,
            })
            .collect();
        self.request_id = self.request_id.wrapping_add(1);
        let req = self.request_id;
        let generation = self.scope_generation;
        let worktree_id = self.current_worktree.clone();
        let source_signature = self.source_signature.clone();
        let restart_preview =
            self.preview_refresh_pending || loading_preview_needs_restart(self.preview.as_ref());
        self.preview_refresh_pending = false;
        self.stale = false;
        self._tasks.clear();

        let Some((path, distro)) = project else {
            self.project_path = None;
            self.host.clear();
            self.wsl.clear();
            self.lineage.clear();
            self.bookkept.clear();
            self.preview = None;
            self.loading = false;
            self.wsl_loading = false;
            self.remote = false;
            self.remote_broken = false;
            cx.notify();
            return;
        };
        self.project_path = Some(path.clone());
        self.loading = true;
        if restart_preview && self.preview.is_some() {
            self.load_preview(cx);
        }

        // ── SSH 远程项目:**只**取远程来源 ────────────────────────
        match source {
            Some(crate::ssh_conn::SessionSource::BrokenRemote) => {
                // 连接被删:什么都取不到。**绝不退回本地扫描** —— 那会把本机
                // 同路径的会话贴到这个远程项目上(见 `ssh_conn::SessionSource`)
                self.remote = true;
                self.remote_broken = true;
                self.host.clear();
                self.wsl.clear();
                self.lineage.clear();
                self.loading = false;
                self.wsl_loading = false;
                cx.notify();
                return;
            }
            Some(crate::ssh_conn::SessionSource::Remote(conn)) => {
                self.remote = true;
                self.remote_broken = false;
                self.wsl.clear();
                self.lineage.clear();
                self.wsl_loading = false;
                let remote_path = path.clone();
                let remote_source_signature = source_signature.clone();
                self._tasks.push(cx.spawn(async move |this, cx| {
                    // [后台] SFTP 往返,秒级;`ai_sessions` 永不返 Err
                    // (失败静默降级为空表,与原版同)
                    let result =
                        cx.background_executor()
                            .spawn(async move {
                                crate::remote_ssh::ai_sessions(&conn, &remote_path, force)
                            })
                            .await;
                    let _ = this.update(cx, |this: &mut Self, cx| {
                        if !session_scope_request_matches(
                            generation,
                            this.scope_generation,
                            worktree_id.as_ref(),
                            this.current_worktree.as_ref(),
                            remote_source_signature.as_deref(),
                            this.source_signature.as_deref(),
                        ) || this.request_id != req
                        {
                            return;
                        }
                        this.host = result.unwrap_or_default();
                        this.loading = false;
                        cx.notify();
                    });
                }));
                cx.notify();
                return;
            }
            _ => {
                self.remote = false;
                self.remote_broken = false;
            }
        }

        // 宿主来源:秒出,先显示
        let host_path = path.clone();
        let host_worktree_id = worktree_id.clone();
        let host_source_signature = source_signature.clone();
        self._tasks.push(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { mt_ai::sessions::get_ai_sessions(host_path) })
                .await;
            let _ = this.update(cx, |this: &mut Self, cx| {
                if !session_scope_request_matches(
                    generation,
                    this.scope_generation,
                    host_worktree_id.as_ref(),
                    this.current_worktree.as_ref(),
                    host_source_signature.as_deref(),
                    this.source_signature.as_deref(),
                ) || this.request_id != req
                {
                    return;
                }
                this.host = result.unwrap_or_default();
                this.loading = false;
                cx.notify();
            });
        }));

        // 分支边:与会话列表**并行**拉取(只读文件头,同量级),同一个请求序号守卫;
        // 失败按无分支处理,不影响会话列表
        let lineage_path = path.clone();
        let lineage_worktree_id = worktree_id.clone();
        let lineage_source_signature = source_signature.clone();
        self._tasks.push(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    mt_ai::sessions::scan_session_lineage(lineage_path, Some(bookkept))
                })
                .await;
            let _ = this.update(cx, |this: &mut Self, cx| {
                if !session_scope_request_matches(
                    generation,
                    this.scope_generation,
                    lineage_worktree_id.as_ref(),
                    this.current_worktree.as_ref(),
                    lineage_source_signature.as_deref(),
                    this.source_signature.as_deref(),
                ) || this.request_id != req
                {
                    return;
                }
                // 扫描永远返回 Vec(内部逐文件容错),失败 = 空表 = 按无分支处理
                this.lineage = result;
                cx.notify();
            });
        }));

        // WSL 来源:并行请求,到达后合并(不阻塞宿主显示)
        if has_wsl_source(&path, distro.as_deref()) {
            self.wsl_loading = true;
            let wsl_worktree_id = worktree_id.clone();
            let wsl_source_signature = source_signature;
            self._tasks.push(cx.spawn(async move |this, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        mt_ai::sessions::get_wsl_ai_sessions(path, distro, Some(force))
                    })
                    .await;
                let _ = this.update(cx, |this: &mut Self, cx| {
                    if !session_scope_request_matches(
                        generation,
                        this.scope_generation,
                        wsl_worktree_id.as_ref(),
                        this.current_worktree.as_ref(),
                        wsl_source_signature.as_deref(),
                        this.source_signature.as_deref(),
                    ) || this.request_id != req
                    {
                        return;
                    }
                    this.wsl = result.unwrap_or_default();
                    this.wsl_loading = false;
                    cx.notify();
                });
            }));
        } else {
            self.wsl.clear();
            self.wsl_loading = false;
        }
        cx.notify();
    }

    /// 两个来源按时间戳降序混排(与后端排序口径一致:ISO 8601 字符串比较)。
    fn merged(&self) -> Vec<&AiSession> {
        let mut all: Vec<&AiSession> = self.host.iter().chain(self.wsl.iter()).collect();
        if !self.wsl.is_empty() {
            all.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        }
        all
    }

    /// 渲染用的行:`(会话, 连线前缀, 显示标题)`。
    ///
    /// **两模式共用同一套行渲染** —— 平铺模式 `prefix` 为空串、标题就是会话标题,
    /// 树只是列表长出了结构。
    ///
    /// ⚠️ 树模式的分页 `take(display_count)` 在**拍平之后**做 —— 反过来会把父
    /// 截掉、子全变成孤儿根。
    fn rows(&self) -> Vec<(AiSession, String, String)> {
        let all = self.merged();
        if self.view == ViewMode::Flat {
            return all
                .into_iter()
                .take(self.display_count)
                .map(|s| (s.clone(), String::new(), s.title.clone()))
                .collect();
        }
        // 磁盘边 + 自记账边(**磁盘优先**)。自记账那份在扫描时已被 mt-ai 补过
        // 标题并入返回值,这里再合一次是为了兜住「扫描整个失败但自记账还在」的窗口
        // (原版 `SessionList.tsx:260` 同样合两次)。
        let edges = merge_lineage_edges(self.lineage.clone(), self.bookkept.clone());
        let ids: Vec<String> = all.iter().map(|s| s.id.clone()).collect();
        let timestamps: Vec<String> = all.iter().map(|s| s.timestamp.clone()).collect();
        let roots = build_session_tree(&ids, &timestamps, &edges);
        flatten_session_tree(&roots)
            .into_iter()
            .take(self.display_count)
            .filter_map(|row| {
                let session = all.get(row.index)?;
                // fork 是**整份复制**,标题字段连同首条消息一起继承自根会话,
                // 分支之间全同名 —— 真正区分一条分支的是它岔开后干了什么
                let title = row
                    .edge
                    .and_then(|i| edges.get(i))
                    .and_then(|e| e.branch_title.clone())
                    .unwrap_or_else(|| session.title.clone());
                Some(((*session).clone(), row.prefix, title))
            })
            .collect()
    }

    /// 在当前活动 pane 里恢复会话。没有终端时退化成「开一个新的再恢复」。
    fn resume(
        &mut self,
        command: String,
        new_tab: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project_id) = self.store.read(cx).active_project_id.clone() else {
            return;
        };
        let existing = self.store.read(cx).active_pane_id(&project_id);
        self.store.update(cx, |store, cx| {
            let target = if new_tab || existing.is_none() {
                // 不能事后再 resolveActivePane:新终端的焦点还没落下去,
                // 拿到的会是用户原本待着的那个 —— 命令就敲进别人的会话了
                store.new_terminal(&project_id, None, existing.clone(), window, cx)
            } else {
                existing.clone()
            };
            let Some(pane_id) = target else { return };
            store.write_to_pane(&project_id, &pane_id, &format!("{command}\r"), cx);
            store.focus_pane(&project_id, &pane_id, window, cx);
        });
        crate::workbench_area::activate_terminal_page(window, cx);
    }

    /// 会话行的右键菜单(`SessionList.tsx:378-401`,顺序照抄):
    ///
    /// ```text
    /// 查看                        ← 恒在
    /// ──────────                  ← 仅当 canResumeHere
    /// 在当前终端恢复
    /// 在新终端标签恢复
    /// ──────────                  ← 仅当 cmd 拼得出
    /// 复制恢复命令
    /// ```
    ///
    /// `canResumeHere = cmd.is_some() && wsl_distro.is_none() && ssh_connection_id.is_none()`
    /// —— 会话来自别处时把命令敲进本机终端跑不通,只留「查看 / 复制命令」。
    fn row_menu(&self, session: &AiSession, cx: &mut Context<Self>) -> Vec<menu::MenuEntry> {
        let entity = cx.entity();
        let command = build_resume_command(&session.session_type, &session.id);
        let can_resume_here = command.is_some()
            && session.wsl_distro.is_none()
            && session.ssh_connection_id.is_none();

        let mut entries = vec![menu::item(t("sessionList", "view"), {
            let entity = entity.clone();
            let session = session.clone();
            move |_window, cx: &mut App| {
                let session = session.clone();
                entity.update(cx, |this, cx| this.open_preview(&session, cx));
            }
        })];
        if can_resume_here && let Some(cmd) = command.clone() {
            entries.push(menu::separator());
            for (label, new_tab) in [
                (t("sessionList", "resumeHere"), false),
                (t("sessionList", "resumeInNewTab"), true),
            ] {
                let entity = entity.clone();
                let cmd = cmd.clone();
                entries.push(menu::item(label, move |window, cx: &mut App| {
                    let cmd = cmd.clone();
                    entity.update(cx, |this, cx| this.resume(cmd, new_tab, window, cx));
                }));
            }
        }
        if let Some(cmd) = command {
            entries.push(menu::separator());
            entries.push(menu::item(
                t("sessionList", "copyResumeCommand"),
                move |_window, cx: &mut App| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(cmd.clone()));
                },
            ));
        }
        entries
    }

    fn render_runtime_row(
        &self,
        diagnostic: TerminalDiagnosticView,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let recovery_label = terminal_recovery_label(diagnostic.recovery);
        let recovery_color = terminal_recovery_color(diagnostic.recovery);
        let agent = diagnostic.agent.clone();
        let activity = agent
            .as_ref()
            .map(|agent| agent_activity_label(agent.activity));
        let activity_color = agent
            .as_ref()
            .map(|agent| agent_activity_color(agent.activity, agent.attention));
        let connectivity = agent.as_ref().map(|agent| agent.connectivity).or_else(|| {
            diagnostic
                .remote_agent
                .as_ref()
                .map(|probe| probe.connectivity)
        });
        let remote_probe = diagnostic
            .remote_agent
            .as_ref()
            .map(|probe| remote_probe_label(probe.capability, probe.process_count));
        let detail = diagnostic.backend_notice.clone().or_else(|| {
            diagnostic
                .remote_agent
                .as_ref()
                .and_then(|probe| probe.last_error.clone())
        });
        let tooltip = detail
            .clone()
            .unwrap_or_else(|| format!("{} · {recovery_label}", diagnostic.pane_label));
        let run_id = agent.as_ref().map(|agent| agent.run_id.clone());
        let vendor = agent
            .as_ref()
            .and_then(|agent| agent_vendor(&agent.provider));
        let pane_label = diagnostic.pane_label.clone();
        let exited = diagnostic.exited;

        div()
            .id(SharedString::from(format!(
                "session-runtime-{}-{}",
                diagnostic.project_id, diagnostic.pane_id
            )))
            .flex()
            .items_start()
            .gap(px(8.0))
            .px(px(10.0))
            .py(px(6.0))
            .text_size(ui::font_px(10.5))
            .hover(|row| row.bg(ui::border_subtle()))
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .when_some(run_id, |row, run_id| {
                row.cursor_pointer()
                    .on_click(cx.listener(move |this: &mut Self, _, window, cx| {
                        AppStore::activate_agent_run(&this.store, &run_id, window, cx);
                    }))
            })
            .child(
                div()
                    .flex_none()
                    .w(px(16.0))
                    .h(px(16.0))
                    .mt(px(1.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when_some(vendor, |icon, vendor| {
                        icon.child(
                            BrandIcon::new(Some(vendor))
                                .size(px(13.0))
                                .color(ui::text_secondary()),
                        )
                    })
                    .when(vendor.is_none(), |icon| {
                        icon.child(
                            div()
                                .w(px(6.0))
                                .h(px(6.0))
                                .rounded_full()
                                .bg(recovery_color),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(ui::text_secondary())
                                    .child(pane_label),
                            )
                            .when_some(activity.zip(activity_color), |line, (label, color)| {
                                line.child(
                                    div()
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.0))
                                        .text_color(color)
                                        .child(div().w(px(5.0)).h(px(5.0)).rounded_full().bg(color))
                                        .child(label),
                                )
                            }),
                    )
                    .child(
                        div()
                            .mt(px(2.0))
                            .flex()
                            .items_center()
                            .gap(px(5.0))
                            .text_size(ui::font_px(9.0))
                            .text_color(ui::text_muted())
                            .child(
                                div()
                                    .w(px(5.0))
                                    .h(px(5.0))
                                    .rounded_full()
                                    .bg(recovery_color),
                            )
                            .child(recovery_label)
                            .when(exited, |line| {
                                line.child("·")
                                    .child(div().text_color(ui::color_error()).child("Exited"))
                            })
                            .when_some(remote_probe, |line, label| line.child("·").child(label))
                            .child(div().flex_1())
                            .when_some(connectivity, |line, connectivity| {
                                let color = agent_connectivity_color(connectivity);
                                line.child(
                                    div()
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.0))
                                        .child(div().w(px(5.0)).h(px(5.0)).rounded_full().bg(color))
                                        .child(agent_connectivity_label(connectivity)),
                                )
                            }),
                    )
                    .when_some(detail, |body, detail| {
                        body.child(
                            div()
                                .mt(px(2.0))
                                .truncate()
                                .text_size(ui::font_px(9.0))
                                .text_color(ui::text_muted())
                                .child(detail),
                        )
                    }),
            )
            .into_any_element()
    }

    fn open_preview(&mut self, session: &AiSession, cx: &mut Context<Self>) {
        self.preview_refresh_pending = false;
        self.preview = Some(Preview {
            session_id: session.id.clone(),
            session_type: session.session_type.clone(),
            wsl_distro: session.wsl_distro.clone(),
            title: session.title.clone(),
            loading: false,
            error: None,
            messages: Vec::new(),
            rendered_messages: Vec::new(),
            shown: PREVIEW_PAGE_SIZE,
            command: build_resume_command(&session.session_type, &session.id),
            scroll: ScrollHandle::new(),
        });
        self.load_preview(cx);
        cx.notify();
    }

    fn load_preview(&mut self, cx: &mut Context<Self>) {
        self.preview_refresh_pending = false;
        let Some(project_path) = self.project_path.clone() else {
            return;
        };
        let Some(preview) = self.preview.as_mut() else {
            return;
        };
        preview.loading = true;
        preview.error = None;
        let expected_session_id = preview.session_id.clone();
        let session_type = preview.session_type.clone();
        let distro = preview.wsl_distro.clone();
        self.preview_request = self.preview_request.wrapping_add(1);
        let request = self.preview_request;
        let generation = self.scope_generation;
        let worktree_id = self.current_worktree.clone();
        let source_signature = self.source_signature.clone();
        let source = {
            let store = self.store.read(cx);
            store
                .active_project()
                .map(|project| crate::ssh_conn::session_source(project, store.ssh_connections()))
        };
        let session_id = expected_session_id.clone();
        self._tasks.push(cx.spawn(async move |this, cx| {
            // 正文可能几 MB + WSL 9P / SFTP 往返,雷打不动丢后台。
            let result = cx
                .background_executor()
                .spawn(async move {
                    let result = match source {
                        Some(crate::ssh_conn::SessionSource::Remote(connection)) => {
                            crate::remote_ssh::ai_session_content_all(
                                &connection,
                                &session_type,
                                &session_id,
                                &project_path,
                            )
                        }
                        Some(crate::ssh_conn::SessionSource::BrokenRemote) => {
                            Err("SSH connection is unavailable".to_string())
                        }
                        _ => mt_ai::sessions::get_ai_session_content(
                            session_type,
                            session_id,
                            project_path,
                            distro,
                        ),
                    };
                    result.map(|messages| {
                        let rendered_messages = messages
                            .iter()
                            .map(|message| {
                                crate::file_viewer::sanitize_session_markdown(&message.content)
                            })
                            .collect::<Vec<_>>();
                        (messages, rendered_messages)
                    })
                })
                .await;
            let _ = this.update(cx, |this: &mut Self, cx| {
                if !session_scope_request_matches(
                    generation,
                    this.scope_generation,
                    worktree_id.as_ref(),
                    this.current_worktree.as_ref(),
                    source_signature.as_deref(),
                    this.source_signature.as_deref(),
                ) || this.preview_request != request
                {
                    return;
                }
                let Some(preview) = this.preview.as_mut() else {
                    return;
                };
                if preview.session_id != expected_session_id {
                    return;
                }
                preview.loading = false;
                match result {
                    Ok((messages, rendered_messages)) => {
                        preview.messages = messages;
                        preview.rendered_messages = rendered_messages;
                    }
                    Err(err) => preview.error = Some(err),
                }
                cx.notify();
            });
        }));
    }

    /// 会话正文预览。
    ///
    /// **正文走 [`TextView`] 而不是静态文本**:GPUI 的 `div().child(文本)` 画出来
    /// 的字一个都选不中,而 gpui-component 里唯一带选区的文本渲染器就是它
    /// (`selectable(true)` + `Ctrl/Cmd+C`,与文件查看器的 markdown 预览同一个)。
    /// 顺带把会话正文按 markdown 排出来 —— 三家 agent 的回复本来就是 markdown。
    ///
    /// 选区**跨不了消息**(组件的选区坐标相对单个 TextView 自己的 bounds),
    /// 所以每条消息头上留一颗「复制」、顶上留一颗「复制全文」把整条/整份兜住。
    fn render_preview(
        &mut self,
        preview_title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(preview) = self.preview.as_ref() else {
            return div().into_any_element();
        };
        let loading = preview.loading;
        let error = preview.error.clone();
        let command = preview.command.clone();
        let session_key = preview.session_id.clone();
        let total = preview.messages.len();
        let shown = preview.shown.min(total);
        let has_messages = total > 0;
        let text_style = preview_text_style(cx);
        let preview_scroll = preview.scroll.clone();

        let mut body = div()
            .id("session-preview-body")
            .flex_1()
            .overflow_y_scroll()
            .track_scroll(&preview_scroll)
            .px(px(10.0))
            .flex()
            .flex_col()
            .gap(px(8.0));

        if loading {
            body = body.child(
                div()
                    .py(px(12.0))
                    .text_size(ui::font_px(12.0))
                    .text_color(ui::text_muted())
                    .child(t("sessionViewer", "loading")),
            );
        }
        if let Some(err) = error {
            body = body.child(
                div()
                    .py(px(12.0))
                    .text_size(ui::font_px(12.0))
                    .text_color(ui::color_error())
                    .child(err),
            );
        }
        if !loading && !has_messages {
            body = body.child(
                div()
                    .py(px(12.0))
                    .text_size(ui::font_px(12.0))
                    .text_color(ui::text_muted())
                    .child(t("sessionViewer", "emptyContent")),
            );
        }

        // 逐条引用着画,不整页 clone:终端一跑起来整窗每帧重绘(GPUI 的
        // notify 是整窗口口径),一页几十条正文每帧复制一遍就是白烧内存带宽
        for (ix, msg) in preview.messages.iter().take(shown).enumerate() {
            let is_user = msg.role == "user";
            let time = format_message_time(&msg.timestamp);
            let content = msg.content.clone();
            let rendered_content = preview.rendered_messages[ix].clone();
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .p(px(8.0))
                    .rounded(px(4.0))
                    .bg(if is_user {
                        ui::bg_overlay()
                    } else {
                        ui::bg_base()
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .text_size(ui::font_px(10.0))
                            .text_color(ui::text_muted())
                            // 旧版 `SessionViewerModal.tsx` 这两个角色名就是硬编码的
                            // 英文字面量(不进字典),照抄
                            .child(if is_user { "User" } else { "Assistant" })
                            .child(div().flex_1().when_some(time, |el, time| el.child(time)))
                            .child(
                                div()
                                    .id(SharedString::from(format!("session-msg-copy-{ix}")))
                                    .cursor_pointer()
                                    .hover(|el| el.text_color(ui::accent()))
                                    .child(t("sessionViewer", "copyMessage"))
                                    .on_click(move |_, _window, cx| {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                            content.clone(),
                                        ));
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_size(ui::font_px(12.0))
                            .text_color(ui::text_secondary())
                            .child(
                                TextView::markdown(
                                    // id 带会话 id:换会话看时不复用上一份的解析缓存
                                    SharedString::from(format!("session-msg-{session_key}-{ix}")),
                                    rendered_content,
                                    window,
                                    cx,
                                )
                                .style(text_style.clone())
                                .selectable(true),
                            ),
                    ),
            );
        }

        if shown < total {
            let remaining = total - shown;
            body = body.child(
                div()
                    .id("session-preview-more")
                    .py(px(8.0))
                    .text_size(ui::font_px(11.0))
                    .text_color(ui::text_muted())
                    .cursor_pointer()
                    .hover(|el| el.text_color(ui::accent()))
                    .child(tr!("sessionList", "loadMore", n = remaining))
                    .on_click(cx.listener(|this: &mut Self, _, _window, cx| {
                        if let Some(preview) = this.preview.as_mut() {
                            preview.shown += PREVIEW_PAGE_SIZE;
                        }
                        cx.notify();
                    })),
            );
        }

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(10.0))
                    .py(px(6.0))
                    .border_b_1()
                    .border_color(ui::border_subtle())
                    .child(
                        ui::ghost_button(
                            "session-preview-back",
                            format!("‹ {}", t("fileViewer", "back")),
                        )
                        .on_click(cx.listener(
                            |this: &mut Self, _, _window, cx| {
                                this.preview = None;
                                this.preview_refresh_pending = false;
                                this.preview_request = this.preview_request.wrapping_add(1);
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .text_size(ui::font_px(12.0))
                            .text_color(ui::text_primary())
                            .child(preview_title),
                    )
                    .when(has_messages, |el| {
                        el.child(
                            ui::ghost_button("session-copy-all", t("sessionViewer", "copyAll"))
                                .tooltip(move |window, cx| {
                                    Tooltip::new(tr!(
                                        "sessionViewer",
                                        "messageCount",
                                        count = total
                                    ))
                                    .build(window, cx)
                                })
                                .on_click(cx.listener(|this: &mut Self, _, _window, cx| {
                                    let Some(preview) = this.preview.as_ref() else {
                                        return;
                                    };
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        preview.all_text(),
                                    ));
                                })),
                        )
                    })
                    .when_some(command, |el, command| {
                        el.child(
                            ui::ghost_button(
                                "session-copy-cmd",
                                t("sessionList", "copyResumeCommand"),
                            )
                            .on_click(move |_, _window, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    command.clone(),
                                ));
                            }),
                        )
                    }),
            )
            .child(
                div().relative().flex_1().min_h_0().child(body).child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .child(
                            Scrollbar::vertical(&preview_scroll).id("session-preview-scrollbar"),
                        ),
                ),
            )
            .into_any_element()
    }
}

impl Render for SessionPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let container = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(ui::bg_surface())
            .border_l_1()
            .border_color(ui::border_default());

        if let Some(title) = self.preview.as_ref().map(|p| p.title.clone()) {
            let body = self.render_preview(title, window, cx);
            return container.child(body);
        }

        let rows = self.rows();
        let sessions: Vec<AiSession> = rows.iter().map(|(s, _, _)| s.clone()).collect();
        let worktree_context = orca_worktree_context_enabled();
        let (agent_targets, terminal_diagnostics) = if worktree_context {
            self.current_worktree
                .as_ref()
                .map(|worktree_id| {
                    let store = self.store.read(cx);
                    (
                        store.agent_target_views_for_worktree(worktree_id),
                        store.terminal_diagnostics_for_worktree(worktree_id, cx),
                    )
                })
                .unwrap_or_default()
        } else {
            (Vec::new(), Vec::new())
        };
        let session_targets: Vec<Option<AgentTargetView>> = sessions
            .iter()
            .map(|session| {
                session_agent_target(&session.session_type, &session.id, &agent_targets).cloned()
            })
            .collect();
        let runtime_rows: Vec<AnyElement> = terminal_diagnostics
            .into_iter()
            .map(|diagnostic| self.render_runtime_row(diagnostic, cx))
            .collect();
        let total = self.host.len() + self.wsl.len();
        let has_more = self.display_count < total;
        let loading = self.loading;
        let wsl_loading = self.wsl_loading;
        let has_project = self.project_path.is_some();
        let tree = self.view == ViewMode::Tree;
        let remote = self.remote;
        let remote_broken = self.remote_broken;
        // 行尾的远程来源标识:连接名(连接被删时回退 'SSH')。
        // 逐行现查连接表,规模是连接条数
        let remote_name_of = |session: &AiSession| -> Option<String> {
            let id = session.ssh_connection_id.as_deref()?;
            Some(
                self.store
                    .read(cx)
                    .ssh_connections()
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| "SSH".to_string()),
            )
        };
        // 回滚时保留旧的 pane 会话嗅探。新路径只认 AgentRuntimeRegistry
        // 的精确 run，历史记录本身不创建 live 状态。
        let legacy_live_of: Vec<Option<(String, PaneStatus)>> = if !worktree_context && tree {
            sessions
                .iter()
                .map(|session| {
                    self.store
                        .read(cx)
                        .find_live_session_pane(&session.id)
                        .map(|(project_id, _, status)| (project_id, status))
                })
                .collect()
        } else {
            vec![None; sessions.len()]
        };
        let project_name_of = |project_id: &str| -> String {
            self.store
                .read(cx)
                .projects()
                .iter()
                .find(|p| p.id == project_id)
                .map(|p| p.name.clone())
                .unwrap_or_default()
        };

        let mut list = div()
            .id("session-list")
            .flex_1()
            .px(px(6.0))
            .flex()
            .flex_col();

        if !runtime_rows.is_empty() {
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .pb(px(5.0))
                    .mb(px(3.0))
                    .border_b_1()
                    .border_color(ui::border_subtle())
                    .child(
                        div()
                            .px(px(10.0))
                            .pt(px(8.0))
                            .pb(px(3.0))
                            .text_size(ui::font_px(9.0))
                            .text_color(ui::text_muted())
                            .child("Runtime"),
                    )
                    .children(runtime_rows),
            );
        }

        if remote_broken {
            // 断链:列表恒空 + 一句明确提示(原版是静默空表,见模块注释)
            list = list.child(
                div()
                    .py(px(12.0))
                    .text_size(ui::font_px(12.0))
                    .text_color(ui::color_error())
                    .child(t("projectList", "remoteBrokenTitle")),
            );
        } else if loading && sessions.is_empty() {
            list = list.child(
                div()
                    .py(px(12.0))
                    .text_size(ui::font_px(12.0))
                    .text_color(ui::text_muted())
                    .child(t("sessionList", "loading")),
            );
        } else if sessions.is_empty() {
            list = list.child(
                div()
                    .py(px(12.0))
                    .text_size(ui::font_px(12.0))
                    .text_color(ui::text_muted())
                    .child(if has_project {
                        t("sessionList", "empty")
                    } else {
                        t("sessionList", "selectProject")
                    }),
            );
        }

        for (i, (session, prefix, display_title)) in rows.into_iter().enumerate() {
            let key = format!(
                "{}-{}-{}",
                session.session_type,
                session
                    .wsl_distro
                    .as_deref()
                    .or(session.ssh_connection_id.as_deref())
                    .unwrap_or("host"),
                session.id
            );
            let remote_badge = remote_name_of(&session);
            let time = format_time(&session.timestamp);
            // 行图标**两套口径**(`SessionList.tsx:339-342`):
            // 树模式按最新模型推厂商(claude CLI 挂 GLM / DeepSeek 中转是常见用法,
            // CLI ≠ 模型厂商);平铺模式沿用 CLI 图标。
            let vendor = if tree {
                AiVendor::for_session(&session.session_type, session.model.as_deref())
            } else {
                // 平铺的缺省是 claude(原版 `TYPE_VENDOR[...] ?? 'claude'`)
                AiVendor::from_session_type(&session.session_type).or(Some(AiVendor::Claude))
            };
            let wsl_badge = session.wsl_distro.clone();
            let agent_target = session_targets.get(i).cloned().flatten();
            let legacy_live = legacy_live_of.get(i).cloned().flatten();
            let live_project = agent_target
                .as_ref()
                .map(|target| target.project_name.clone())
                .or_else(|| {
                    legacy_live
                        .as_ref()
                        .map(|(project_id, _)| project_name_of(project_id))
                });
            let tip: SharedString = if let Some(target) = agent_target.as_ref() {
                format!(
                    "{display_title}\n{} · {} · {}",
                    target.provider,
                    agent_activity_label(target.activity),
                    agent_connectivity_label(target.connectivity)
                )
                .into()
            } else if tree {
                match &live_project {
                    Some(name) => tr!(
                        "sessionList",
                        "branchTree.runningIn",
                        project = name.clone()
                    )
                    .into(),
                    None => format!(
                        "{display_title}\n{}",
                        t("sessionList", "branchTree.clickToResume")
                    )
                    .into(),
                }
            } else {
                session.title.clone().into()
            };
            let session_for_menu = session.clone();
            let session_for_click = session.clone();
            let target_run_id = agent_target.as_ref().map(|target| target.run_id.clone());
            let has_target = target_run_id.is_some();

            list = list.child(
                div()
                    .id(SharedString::from(format!("session-row-{key}")))
                    .flex()
                    .items_start()
                    .gap(px(8.0))
                    .px(px(10.0))
                    .py(px(6.0))
                    .rounded(px(4.0))
                    .text_size(ui::font_px(12.0))
                    .hover(|el| el.bg(ui::border_subtle()))
                    .when(tree || has_target, |el| el.cursor_pointer())
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .when_some(target_run_id, |el, run_id| {
                        el.on_click(cx.listener(move |this: &mut Self, _, window, cx| {
                            AppStore::activate_agent_run(&this.store, &run_id, window, cx);
                        }))
                    })
                    .when(tree && !has_target, |el| {
                        el.on_click(cx.listener(move |this: &mut Self, _, window, cx| {
                            let task =
                                jump_to_session(&this.store, session_for_click.clone(), window, cx);
                            this._tasks.push(task);
                        }))
                    })
                    // 四项右键菜单(N 批的 `menu.rs` 已就位,行内按钮随之收回)
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this: &mut Self, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            let entries = this.row_menu(&session_for_menu, cx);
                            menu::show(event.position, entries, window, cx);
                        }),
                    )
                    // 树连线前缀:**等宽字体 + 不换行 + 不截断**,`│├└` 才对得齐
                    .when(!prefix.is_empty(), |el| {
                        el.child(
                            div()
                                .flex_none()
                                .mt(px(1.0))
                                .font_family("monospace")
                                .whitespace_nowrap()
                                .text_color(ui::text_muted())
                                .child(prefix.clone()),
                        )
                    })
                    .child(
                        div()
                            .flex_none()
                            .w(px(16.0))
                            .h(px(16.0))
                            .mt(px(1.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                BrandIcon::new(vendor)
                                    .size(px(14.0))
                                    // VectorIcon 自己画,不继承 text_color
                                    .color(ui::text_secondary()),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .when(agent_target.is_none(), |line| {
                                        line.when_some(legacy_live.as_ref(), |line, (_, status)| {
                                            line.child(
                                                StatusDot::new(status_kind(*status))
                                                    .size(px(11.0))
                                                    .color(ui::status_color(*status))
                                                    .contrast(ui::bg_surface()),
                                            )
                                        })
                                    })
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.0))
                                            .truncate()
                                            .text_color(ui::text_secondary())
                                            .child(display_title.clone()),
                                    )
                                    .when_some(agent_target.as_ref(), |line, target| {
                                        let color =
                                            agent_activity_color(target.activity, target.attention);
                                        line.child(
                                            div()
                                                .flex_none()
                                                .flex()
                                                .items_center()
                                                .gap(px(4.0))
                                                .text_size(ui::font_px(9.0))
                                                .text_color(color)
                                                .child(
                                                    div()
                                                        .w(px(5.0))
                                                        .h(px(5.0))
                                                        .rounded_full()
                                                        .bg(color),
                                                )
                                                .child(agent_activity_label(target.activity)),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .mt(px(2.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .text_color(ui::text_muted())
                                    .child(div().flex_1().min_w_0().truncate().child(
                                        match (wsl_badge, remote_badge) {
                                            (Some(distro), _) => format!(
                                                "{time} · {}·{distro}",
                                                t("sessionList", "wslBadge")
                                            ),
                                            (None, Some(name)) => format!("{time} · {name}"),
                                            (None, None) => time,
                                        },
                                    ))
                                    .when_some(agent_target.as_ref(), |line, target| {
                                        let color = agent_connectivity_color(target.connectivity);
                                        line.child(
                                            div()
                                                .flex_none()
                                                .flex()
                                                .items_center()
                                                .gap(px(4.0))
                                                .text_size(ui::font_px(9.0))
                                                .child(
                                                    div()
                                                        .w(px(5.0))
                                                        .h(px(5.0))
                                                        .rounded_full()
                                                        .bg(color),
                                                )
                                                .child(agent_connectivity_label(
                                                    target.connectivity,
                                                )),
                                        )
                                    }),
                            ),
                    ),
            );
        }

        if has_more {
            let remaining = total - self.display_count;
            list = list.child(
                div()
                    .id("session-load-more")
                    .py(px(6.0))
                    .text_size(ui::font_px(11.0))
                    .text_color(ui::text_muted())
                    .cursor_pointer()
                    .hover(|el| el.text_color(ui::accent()))
                    .on_click(cx.listener(|this: &mut Self, _, _window, cx| {
                        this.display_count += PAGE_SIZE;
                        cx.notify();
                    }))
                    .child(tr!("sessionList", "loadMore", n = remaining)),
            );
        }

        let list = list.track_scroll(&self.list_scroll).overflow_y_scroll();
        let list_shell = div().relative().flex_1().min_h_0().child(list).child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .child(Scrollbar::vertical(&self.list_scroll).id("session-list-scrollbar")),
        );

        container
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(10.0))
                    .py(px(6.0))
                    .border_b_1()
                    .border_color(ui::border_subtle())
                    .child(
                        div()
                            .flex()
                            .gap(px(6.0))
                            .items_center()
                            .child(
                                div()
                                    .text_size(ui::font_px(11.0))
                                    .text_color(ui::text_muted())
                                    .child(t("panels", "sessions")),
                            )
                            // WSL 加载中的转圈。`wsl_loading` 落回 false 时整个
                            // 元素从树上消失,保底泵随之自停
                            .when(wsl_loading, |el| {
                                el.child(
                                    div()
                                        .id("session-wsl-spinner")
                                        .flex()
                                        .items_center()
                                        .tooltip(|window, cx| {
                                            Tooltip::new(t("sessionList", "wslLoading"))
                                                .build(window, cx)
                                        })
                                        .child(ui::spinner(px(12.0), ui::text_muted())),
                                )
                            })
                            // 远程来源加载中的转圈(原版 `loading && sshConnectionId`)
                            .when(loading && remote, |el| {
                                el.child(
                                    div()
                                        .id("session-remote-spinner")
                                        .flex()
                                        .items_center()
                                        .tooltip(|window, cx| {
                                            Tooltip::new(t("sessionList", "remoteLoading"))
                                                .build(window, cx)
                                        })
                                        .child(ui::spinner(px(12.0), ui::text_muted())),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            // 平铺 | 树 切换。仅在有活动项目时渲染(原版 `:295`)。
                            // 显示的字符与 tooltip **相反** —— 文案是「切到 X 视图」。
                            .when(has_project, |el| {
                                el.child(
                                    div()
                                        .id("session-view-toggle")
                                        .px(px(4.0))
                                        .font_family("monospace")
                                        .text_size(ui::font_px(12.0))
                                        .text_color(ui::text_muted())
                                        .cursor_pointer()
                                        .hover(|el| el.text_color(ui::text_primary()))
                                        .tooltip(move |window, cx| {
                                            Tooltip::new(if tree {
                                                t("sessionList", "viewFlat")
                                            } else {
                                                t("sessionList", "viewTree")
                                            })
                                            .build(window, cx)
                                        })
                                        .on_click(cx.listener(|this: &mut Self, _, _window, cx| {
                                            this.toggle_view(cx);
                                        }))
                                        .child(if tree { "≡" } else { "⑂" }),
                                )
                            })
                            .child(
                                div()
                                    .id("session-refresh")
                                    .px(px(4.0))
                                    .text_size(ui::font_px(12.0))
                                    .text_color(ui::text_muted())
                                    .cursor_pointer()
                                    .hover(|el| el.text_color(ui::accent()))
                                    .tooltip(|window, cx| {
                                        Tooltip::new(t("sessionList", "refresh")).build(window, cx)
                                    })
                                    .on_click(cx.listener(|this: &mut Self, _, _window, cx| {
                                        this.refresh(true, cx);
                                    }))
                                    .child("↻"),
                            ),
                    ),
            )
            .child(list_shell)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worktree_id(hex: char) -> WorktreeId {
        format!("worktree-v1:{}", hex.to_string().repeat(64))
            .parse()
            .unwrap()
    }

    /// resume 命令按 agent 分派,id 过白名单。
    #[test]
    fn resume_命令按_agent_分派() {
        assert_eq!(
            build_resume_command("claude", "abc-123").as_deref(),
            Some("claude --resume abc-123")
        );
        assert_eq!(
            build_resume_command("codex", "rollout_9").as_deref(),
            Some("codex resume rollout_9")
        );
        assert_eq!(
            build_resume_command("grok", "0199-x").as_deref(),
            Some("grok --resume 0199-x")
        );
        // 未知 agent 按 claude 兜底(与旧版一致)
        assert!(build_resume_command("whatever", "id1").is_some());
    }

    /// shell 元字符一律拦下 —— 这条命令是要原样写进 PTY 的。
    #[test]
    fn 非法会话_id_拒绝拼命令() {
        for bad in [
            "a b",
            "a;rm -rf /",
            "a|b",
            "a\nb",
            "a$(x)",
            "a`x`",
            "a\"b",
            "a'b",
            "../../etc",
            "",
        ] {
            assert!(
                build_resume_command("claude", bad).is_none(),
                "应拒绝: {bad:?}"
            );
        }
        assert!(build_resume_command("claude", &"a".repeat(129)).is_none());
        assert!(build_resume_command("claude", &"a".repeat(128)).is_some());
    }

    #[test]
    fn wsl_来源判定() {
        assert!(has_wsl_source("\\\\wsl$\\Ubuntu\\home\\u", None));
        assert!(has_wsl_source("\\\\wsl.localhost\\Debian\\srv", None));
        assert!(has_wsl_source("D:\\Git\\x", Some("Ubuntu")));
        assert!(!has_wsl_source("D:\\Git\\x", None));
        assert!(!has_wsl_source("D:\\Git\\x", Some("")));
    }

    #[test]
    fn 时间戳解析不出来时不显示() {
        assert_eq!(format_time("不是时间"), "");
        assert_eq!(format_time(""), "");
    }

    /// 消息头的时间是绝对时刻(本地时区),解析不出来就整个不显示。
    #[test]
    fn 消息时间按本地时钟成串() {
        // 固定偏移写死时区,断言与运行机器的时区无关:UTC+8 的 00:30
        let got = format_message_time("2026-08-24T00:30:05+08:00").expect("应能解析");
        let expect = chrono::DateTime::parse_from_rfc3339("2026-08-24T00:30:05+08:00")
            .unwrap()
            .with_timezone(&chrono::Local)
            .format("%m-%d %H:%M:%S")
            .to_string();
        assert_eq!(got, expect);
        // 带毫秒与 Z 的形态(三家 agent 的记录里都出现过)照样认
        assert!(format_message_time("2026-08-24T00:30:05.123Z").is_some());
        assert!(format_message_time("").is_none());
        assert!(format_message_time("不是时间").is_none());
    }

    /// 「复制全文」拼的是 `角色 · 时间` + 正文,时间戳缺失时只留角色。
    #[test]
    fn 复制全文按角色与时间平铺() {
        let msg = |role: &str, content: &str, ts: &str| AiSessionMessage {
            role: role.to_string(),
            content: content.to_string(),
            timestamp: ts.to_string(),
        };
        let preview = Preview {
            session_id: "s1".into(),
            session_type: "claude".into(),
            wsl_distro: None,
            title: "t".into(),
            loading: false,
            error: None,
            messages: vec![
                msg("user", "问题", "2026-08-24T00:30:05+08:00"),
                msg("assistant", "回答", ""),
            ],
            rendered_messages: vec!["问题".into(), "回答".into()],
            shown: PREVIEW_PAGE_SIZE,
            command: None,
            scroll: ScrollHandle::new(),
        };
        let text = preview.all_text();
        assert!(text.contains("User · "), "{text}");
        assert!(text.contains("\n问题"));
        // 时间戳缺失的那条不留悬空的分隔符
        assert!(text.contains("Assistant\n回答"), "{text}");
        // 两条之间空一行
        assert!(text.contains("问题\n\nAssistant"));
    }

    /// 分页只影响铺出去多少条,「复制全文」始终是整份。
    #[test]
    fn 复制全文不受分页影响() {
        let messages: Vec<AiSessionMessage> = (0..PREVIEW_PAGE_SIZE + 5)
            .map(|i| AiSessionMessage {
                role: "user".into(),
                content: format!("第{i}条"),
                timestamp: String::new(),
            })
            .collect();
        let preview = Preview {
            session_id: "s1".into(),
            session_type: "claude".into(),
            wsl_distro: None,
            title: "t".into(),
            loading: false,
            error: None,
            rendered_messages: messages
                .iter()
                .map(|message| message.content.clone())
                .collect(),
            messages,
            shown: PREVIEW_PAGE_SIZE,
            command: None,
            scroll: ScrollHandle::new(),
        };
        let text = preview.all_text();
        assert!(text.contains(&format!("第{}条", PREVIEW_PAGE_SIZE + 4)));
    }
    #[test]
    fn session_scope_request_rejects_old_generation_and_worktree() {
        use mt_identity::{ExecutionHostId, HostInstallId, RepoId};

        let install = HostInstallId::new();
        let host = ExecutionHostId::derive("session-panel", &install);
        let repo = RepoId::derive(&host, "/repo/.git");
        let first = WorktreeId::derive(&repo, "/repo/first", None);
        let second = WorktreeId::derive(&repo, "/repo/second", None);

        assert!(session_scope_request_matches(
            7,
            7,
            Some(&first),
            Some(&first),
            Some("source-a"),
            Some("source-a"),
        ));
        assert!(!session_scope_request_matches(
            6,
            7,
            Some(&first),
            Some(&first),
            Some("source-a"),
            Some("source-a"),
        ));
        assert!(!session_scope_request_matches(
            7,
            7,
            Some(&first),
            Some(&second),
            Some("source-a"),
            Some("source-a"),
        ));
        assert!(!session_scope_request_matches(
            7,
            7,
            Some(&first),
            None,
            Some("source-a"),
            Some("source-a"),
        ));
        assert!(!session_scope_request_matches(
            7,
            7,
            Some(&first),
            Some(&first),
            Some("source-a"),
            Some("source-b"),
        ));
    }

    #[test]
    fn scope_gate_keeps_legacy_path_only_comparison() {
        let first = worktree_id('a');
        let second = worktree_id('b');
        assert!(session_scope_changed(
            true,
            Some(&first),
            Some(&second),
            Some("/repo/shared"),
            Some("/repo/shared"),
            Some("local-a"),
            Some("local-b"),
        ));
        assert!(!session_scope_changed(
            false,
            Some(&first),
            Some(&second),
            Some("/repo/shared"),
            Some("/repo/shared"),
            Some("local-a"),
            Some("local-b"),
        ));
        assert!(session_scope_changed(
            false,
            Some(&first),
            Some(&first),
            Some("/repo/a"),
            Some("/repo/b"),
            Some("local-a"),
            Some("local-a"),
        ));
    }

    #[test]
    fn loading_preview_is_restarted_after_scope_restore() {
        let mut preview = Preview {
            session_id: "session-1".into(),
            session_type: "claude".into(),
            wsl_distro: None,
            title: "Preview".into(),
            loading: false,
            error: None,
            messages: Vec::new(),
            rendered_messages: Vec::new(),
            shown: PREVIEW_PAGE_SIZE,
            command: None,
            scroll: ScrollHandle::new(),
        };
        assert!(!loading_preview_needs_restart(Some(&preview)));
        preview.loading = true;
        assert!(loading_preview_needs_restart(Some(&preview)));
        assert!(!loading_preview_needs_restart(None));
    }

    #[test]
    fn runtime_labels_keep_recovery_activity_and_connectivity_separate() {
        assert_eq!(terminal_recovery_label(TerminalRecovery::Fresh), "Fresh");
        assert_eq!(
            terminal_recovery_label(TerminalRecovery::Reattached),
            "Warm reattach"
        );
        assert_eq!(
            terminal_recovery_label(TerminalRecovery::RestoredHistory),
            "Restored history"
        );
        assert_eq!(
            terminal_recovery_label(TerminalRecovery::Compatibility),
            "Compatibility"
        );
        assert_eq!(
            terminal_recovery_label(TerminalRecovery::Unavailable),
            "Unavailable"
        );
        assert_eq!(agent_activity_label(AgentActivity::Working), "Working");
        assert_eq!(
            agent_connectivity_label(AgentConnectivity::Disconnected),
            "Offline"
        );
    }

    #[test]
    fn remote_probe_labels_cover_capability_states() {
        assert_eq!(
            remote_probe_label(RemoteAgentProbeCapability::Unknown, 0),
            "Detecting"
        );
        assert_eq!(
            remote_probe_label(RemoteAgentProbeCapability::LinuxProc, 3),
            "Linux probe · 3"
        );
        assert_eq!(
            remote_probe_label(RemoteAgentProbeCapability::Unsupported, 0),
            "Unsupported"
        );
    }

    #[test]
    fn history_only_matches_exact_provider_session_identity() {
        let provider = "claude-code".parse::<AgentProvider>().unwrap();
        assert!(agent_session_identity_matches(
            &provider,
            Some("session-1"),
            "claude",
            "session-1"
        ));
        assert!(!agent_session_identity_matches(
            &provider,
            Some("session-1"),
            "codex",
            "session-1"
        ));
        assert!(!agent_session_identity_matches(
            &provider,
            Some("session-2"),
            "claude",
            "session-1"
        ));
        assert!(!agent_session_identity_matches(
            &provider,
            None,
            "claude",
            "session-1"
        ));
    }
}
