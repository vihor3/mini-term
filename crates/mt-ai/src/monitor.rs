//! 状态判定与 500ms 轮询(原 `src-tauri/src/process_monitor.rs`)。
//!
//! 与原实现唯一的结构差异:`pty-status-change` 不再走 Tauri `emit`,改由注入的
//! [`StatusSink`] 承接。**去重表原样保留**——它防的是迟到 hook 事件推错状态后
//! monitor 的纠正被吞掉,与传输层无关(见 [`StatusEmitter`] 的文档)。

use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// 去重表被 hook HTTP 线程、500ms 轮询线程与 GPUI 主线程共同发射,
// 与 tracker/hook_server 那批表同一条命脉:用 parking_lot 免掉锁中毒。
use parking_lot::Mutex;

use crate::hook_server::{HookSessionId, HookState};
use crate::agent_runtime::{AgentHookLifecycleId, AgentWeakEpisode};
use crate::tracker::SessionTracker;

/// 一次状态变化。字段与原 `PtyStatusChangePayload` 完全一致
/// (serde camelCase 保留:移动端中转会把它原样转发给 PWA)。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusChange {
    pub pty_id: u32,
    pub status: String,
    /// 状态成因：hook 直推时是（归一化后的）hook 事件名（`Stop` /
    /// `PermissionRequest` / `SessionEnd` …，见 hook_server::event_cause），
    /// monitor 轮询算出的变化为 None。多个事件都落到 ai-idle，但只有 `Stop`
    /// 是「任务做完了」，UI 据此决定播报完成与托盘绿灯；黄灯认
    /// `PermissionRequest`/`Elicitation`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
    /// 会话内 AI 命令名("claude"/"codex"/…),品牌图标兜底用
    /// (hook 未启用时 aiSession 不会上报,这是 agent 的唯一来源);
    /// None = 非 AI 状态或来源未知。不参与去重(同一会话内恒定)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Captured at the source, never inferred from delivery time or a poll.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weak_episode: Option<AgentWeakEpisode>,
    /// Internal source identity, never part of the serialized compatibility event.
    #[serde(skip)]
    pub hook_session: Option<HookSessionId>,
}

/// hook 上报的会话身份变化(原 `pty-ai-session` 事件)。
///
/// 上层据此持久化「退出时该 pane 正跑着哪个 AI 会话」,重启后可 `--resume` 续接。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionIdentity {
    pub pty_id: u32,
    pub agent: Option<String>,
    pub session_id: String,
    /// 会话启动目录:claude --resume 只认该目录的会话桶,随身份一起持久化,
    /// 重启续接时 PTY 直接以它为 cwd。None = hook 给的 cwd 已不存在。
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weak_episode: Option<AgentWeakEpisode>,
    /// Source-captured start authority; never serialized to legacy consumers.
    #[serde(skip)]
    pub hook_lifecycle: Option<HookLifecycleEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookLifecycleEvent {
    Started(AgentHookLifecycleId),
    Observed(AgentHookLifecycleId),
}

/// 状态变化的去处。上层(GPUI 壳 / 移动端中转)实现它把变化落到自己的模型上。
///
/// 只关心状态的调用方可以直接塞一个闭包:`Arc::new(|c: StatusChange| …)`,
/// 见下方对 `Fn(StatusChange)` 的 blanket impl。
pub trait StatusSink: Send + Sync {
    fn status_changed(&self, change: StatusChange);

    /// hook 上报的会话身份变化(新 pane / 换会话 / agent 修正)。
    /// 默认 no-op —— 只有需要做会话续接持久化的上层才实现。
    fn session_identified(&self, _identity: SessionIdentity) {}
}

impl<F> StatusSink for F
where
    F: Fn(StatusChange) + Send + Sync,
{
    fn status_changed(&self, change: StatusChange) {
        self(change)
    }
}

/// 状态变化的统一发射器：monitor 轮询与 hook server 直推
/// 共用同一份"上次发出的状态"去重表。
///
/// 此前两个发射源各自为政（hook 直推不更新 monitor 的 prev_statuses）：
/// AI 退出后迟到的 Stop hook 把 UI 直推回 ai-idle，而 monitor 自己算出的
/// 纠正值 "idle" 与它的 prev 相同被去重吞掉，UI 就永久停在 ai-idle。
/// 比较、记录、发射收在同一把锁内，保证两个发射源的事件顺序一致。
#[derive(Clone)]
pub struct StatusEmitter {
    /// Previous status/cause and source episode, shared by all producers.
    prev: Arc<Mutex<HashMap<u32, EmittedStatus>>>,
    sink: Arc<dyn StatusSink>,
}

struct EmittedStatus {
    status: String,
    cause: Option<String>,
    weak_episode: Option<AgentWeakEpisode>,
    hook_session: Option<HookSessionId>,
}

impl StatusEmitter {
    pub fn new(sink: Arc<dyn StatusSink>) -> Self {
        Self {
            prev: Arc::new(Mutex::new(HashMap::new())),
            sink,
        }
    }

    /// 与上次发出的状态不同才发射。
    /// cause 规则:
    /// - 状态变化 → 总是发射(cause 取本次值,None/非 attention 类事件会清掉
    ///   UI 的 attention 标注,这正是「用户批准后下一个事件自然熄灭黄灯」的路径);
    /// - 状态相同 + cause=None → **跳过**:monitor 每 500ms 以无成因方式重发
    ///   hook 状态,若放行会在黄灯点亮后 500ms 内把 attention 抹掉
    ///   (黄灯闪一下就被蓝色顶掉的根因);
    /// - 状态相同 + cause 为 attention 类事件(见 `hook_server::is_attention_cause`)
    ///   → **总是发射**:黄灯的清除发生在 UI 侧(用户键入即批准),这里的去重表
    ///   感知不到;若按相同 cause 去重,同一轮内第二次授权请求会被吞掉;
    /// - 状态相同 + 其他 cause 与上次相同 → 跳过;变化 → 发射(如
    ///   PermissionRequest → Stop)。
    pub fn emit_if_changed(
        &self,
        pty_id: u32,
        status: &str,
        cause: Option<&str>,
        agent: Option<String>,
    ) {
        self.emit_if_changed_with_episode(pty_id, status, cause, agent, None);
    }

    /// Episode changes represent source input detection, not poll activity.
    pub fn emit_if_changed_with_episode(
        &self,
        pty_id: u32,
        status: &str,
        cause: Option<&str>,
        agent: Option<String>,
        weak_episode: Option<AgentWeakEpisode>,
    ) {
        self.emit_change(StatusChange {
            pty_id,
            status: status.to_string(),
            cause: cause.map(str::to_string),
            agent,
            weak_episode,
            hook_session: None,
        });
    }

    pub(crate) fn emit_hook_status(&self, change: StatusChange) {
        self.emit_change(change);
    }

    fn emit_change(&self, change: StatusChange) {
        let pty_id = change.pty_id;
        let status = change.status.as_str();
        let cause = change.cause.as_deref();
        let weak_episode = change.weak_episode;
        let mut prev = self.prev.lock();
        if let Some(previous) = prev.get(&pty_id) {
            let same_owner = change.hook_session.as_ref()
                .is_none_or(|owner| previous.hook_session.as_ref() == Some(owner));
            if previous.status == status && same_owner {
                match cause {
                    None if weak_episode.is_none() || weak_episode == previous.weak_episode => return,
                    Some(c) if crate::hook_server::is_attention_cause(c) => {}
                    Some(c) if previous.cause.as_deref() == Some(c)
                        && (weak_episode.is_none() || weak_episode == previous.weak_episode) => return,
                    _ => {}
                }
            }
        }
        let remembered_episode = weak_episode.or_else(|| prev.get(&pty_id).and_then(|previous| previous.weak_episode));
        let remembered_session = change.hook_session.clone()
            .or_else(|| prev.get(&pty_id).and_then(|previous| previous.hook_session.clone()));
        let remembered_cause = match (cause, prev.get(&pty_id)) {
            (None, Some(previous)) if previous.status == status => {
                previous.cause.clone()
            }
            _ => cause.map(str::to_string),
        };
        prev.insert(pty_id, EmittedStatus {
            status: status.to_string(),
            cause: remembered_cause,
            weak_episode: remembered_episode,
            hook_session: remembered_session,
        });
        self.sink.status_changed(change);
    }

    /// 会话身份变化的直通口(不参与状态去重):hook server 记录到新会话身份时调。
    pub fn notify_session_identity(&self, identity: SessionIdentity) {
        self.sink.session_identified(identity);
    }

    #[cfg(test)]
    pub(crate) fn last_cause(&self, pty_id: u32) -> Option<String> {
        self.prev
            .lock()
            .get(&pty_id)
            .and_then(|previous| previous.cause.clone())
    }

    /// 清掉已不存在的 pty 的去重记录
    pub fn retain(&self, alive: &[u32]) {
        self.prev.lock().retain(|id, _| alive.contains(id));
    }
}

/// 单个 pty 的状态判定（monitor 每轮对每个 pty 调一次）。
///
/// **本函数**里 hook 一旦启用即为绝对权威：状态完全由 `last_hook_status` 决定，
/// PTY 输出活跃度不参与判定，因此同一份 hook 状态连续多轮必然算出同一个值。
///
/// 这里曾有一条兜底——"hook 停在 ai-working 但连续 AI_ACTIVE_TIMEOUT 无输出即
/// 视为 ai-idle"（后又给 API 重试加了 retry_hold 豁免）。它是**无记忆**的：每轮
/// 500ms 重算，降级结果不落盘，hook_status 本身仍是 ai-working。于是只要
/// hook_status 卡住（Stop 丢失，或 Stop 之后又收到同会话迟到的 PostToolUse），
/// AI 空闲期每一次零星伪输出（TUI 定时重绘等）都会把状态抬回 ai-working、
/// 3 秒后再落回 ai-idle，形成以伪输出间隔为周期的脉冲（实测 20~50s 一轮），
/// 每个下降沿被 UI 当成一次"任务完成"反复播报。v0.9.3 起整条兜底从本函数删除
/// （retry_hold 机制随之失去消费者一并移除）。
///
/// Silence never writes back to Hook state. Missing semantic telemetry remains
/// missing evidence, not a fabricated completion, input wait, or process exit.
///
/// 退出的唯一权威信号是 SessionEnd hook（hook_server 处理：清状态 + 直推 idle）。
/// 这里**不能**根据输入检测（is_ai_session）把 hook 状态拆掉降级 idle：
/// 输入检测会漏判启动（别名/包装脚本）、误判退出（任务运行中双击 Ctrl+C 只是
/// 打断并不退出），曾经的 "ai-idle && !is_ai_session → idle" 兜底会把这类
/// 误差放大成 pane 整个会话期永久显示 idle。
///
/// Without Hook, `ai-idle` plus agent identity is only a compatibility liveness
/// signal. The runtime normalizes it to Unknown, not semantic Waiting. This
/// input-detection path does not depend on whether the Hook server is running.
pub fn resolve_status(hook_state: &HookState, tracker: &SessionTracker, pty_id: u32) -> String {
    status_from_detection(hook_state, pty_id, tracker.is_ai_session(pty_id))
}

fn status_from_detection(hook_state: &HookState, pty_id: u32, detected: bool) -> String {
    if hook_state.is_hook_enabled(pty_id) {
        hook_state
            .get_status(pty_id)
            .unwrap_or_else(|| "idle".to_string())
    } else if detected {
        "ai-idle".to_string()
    } else {
        "idle".to_string()
    }
}

fn poll_panes(
    hook_state: &HookState,
    tracker: &SessionTracker,
    emitter: &StatusEmitter,
    pty_ids: &[u32],
) {
    for pty_id in pty_ids {
        let (detected_agent, weak_episode) = tracker.weak_session_snapshot(*pty_id);
        let status = status_from_detection(hook_state, *pty_id, detected_agent.is_some());
        let agent = if status.starts_with("ai-") {
            detected_agent
        } else {
            None
        };
        emitter.emit_if_changed_with_episode(*pty_id, &status, None, agent, weak_episode);
    }
    emitter.retain(pty_ids);
}

/// 活着的 pane id 列表。原实现直接问 `PtyManager::get_pty_ids()`;本 crate 不认识
/// PTY,由上层注入(mt-ai 不依赖 mt-pty 是迁移契约里的硬约束)。
pub type PaneListFn = Box<dyn Fn() -> Vec<u32> + Send>;

pub fn start_monitor(
    tracker: SessionTracker,
    hook_state: HookState,
    emitter: StatusEmitter,
    live_panes: PaneListFn,
) {
    thread::spawn(move || {
        loop {
            let pty_ids = live_panes();

            poll_panes(&hook_state, &tracker, &emitter, &pty_ids);

            let sleep_ms = if pty_ids.is_empty() { 2000 } else { 500 };
            thread::sleep(Duration::from_millis(sleep_ms));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归测试（2026-07-31 tab 显示 idle 而非 ai-* 的 bug）：claude 任务
    /// 运行中快速连按两次 Ctrl+C 打断（claude 只是中断当前任务回到提示符，
    /// 并未退出），输入检测按"双击 Ctrl+C 退出"误清 AI 会话标记；随后 hook
    /// 上报 ai-idle。修复前 monitor 因 !is_ai_session 把 hook 状态整体拆除
    /// 并降级为 idle——hook 状态必须保持权威。
    #[test]
    fn double_ctrlc_interrupt_keeps_hook_ai_idle() {
        let hooks = HookState::new();
        let mgr = SessionTracker::new();

        mgr.track_input(1, "claude\r"); // 用户启动 claude
        hooks.update(1, "ai-working".to_string()); // UserPromptSubmit：任务运行中
        mgr.track_input(1, "\x03"); // 双击 Ctrl+C 打断任务
        mgr.track_input(1, "\x03"); // （claude 未退出，仅回到提示符）
        hooks.update(1, "ai-idle".to_string()); // 打断后 claude 上报 ai-idle

        assert_eq!(resolve_status(&hooks, &mgr, 1), "ai-idle");
    }

    /// 回归测试（AI 完成通知每 20~50s 重复播报的 bug）：hook 卡在 ai-working
    /// 且无 PTY 输出时，**不再**按输出超时降级为 ai-idle。
    ///
    /// 旧行为在这里返回 ai-idle，而 hook_status 仍是 ai-working、降级结果不落盘；
    /// 于是 AI 空闲期的零星伪输出会把状态抬回 ai-working，3 秒后再落回 ai-idle，
    /// 每个下降沿被 UI 当成一次"任务完成"。现在 hook 是绝对权威，代价是 Stop
    /// 丢失时徽章残留 ai-working（详见 `resolve_status` 文档）。
    #[test]
    fn stuck_ai_working_is_not_degraded_by_output_timeout() {
        let hooks = HookState::new();
        let mgr = SessionTracker::new();

        mgr.track_input(1, "claude\r");
        hooks.update(1, "ai-working".to_string());
        mgr.track_input(1, "\x03");
        mgr.track_input(1, "\x03");
        // 无后续 hook 事件、无 PTY 输出（has_recent_output 为 false）

        assert_eq!(resolve_status(&hooks, &mgr, 1), "ai-working");
    }

    /// 同一 bug 的另一面：monitor 每 500ms 重算一次，只要没有新 hook 事件，
    /// 连续多轮必须给出同一个值——状态不再随输出活跃度上下摆动，也就没有
    /// 供 UI 误判为"完成"的下降沿。
    #[test]
    fn hook_status_is_stable_across_polls() {
        let hooks = HookState::new();
        let mgr = SessionTracker::new();

        mgr.track_input(1, "claude\r");
        hooks.update(1, "ai-working".to_string());

        let polls: Vec<String> = (0..5).map(|_| resolve_status(&hooks, &mgr, 1)).collect();
        assert!(
            polls.iter().all(|s| s == "ai-working"),
            "hook 未更新时状态应恒定，实测 {:?}",
            polls
        );
    }

    /// 启动方式漏检（别名/包装脚本，输入检测从未标记 is_ai_session）时，
    /// hook 状态照常生效，不因 !is_ai_session 被降级。
    #[test]
    fn alias_start_without_input_detection_keeps_hook_status() {
        let hooks = HookState::new();
        let mgr = SessionTracker::new();

        hooks.update(1, "ai-idle".to_string()); // hook 正常上报，但输入检测漏了启动

        assert_eq!(resolve_status(&hooks, &mgr, 1), "ai-idle");
    }

    /// 对照组：输入检测与 hook 一致（未误判退出）时，ai-idle 正常保持。
    #[test]
    fn hook_ai_idle_stays_when_input_detection_agrees() {
        let hooks = HookState::new();
        let mgr = SessionTracker::new();

        mgr.track_input(1, "claude\r");
        hooks.update(1, "ai-idle".to_string());

        assert_eq!(resolve_status(&hooks, &mgr, 1), "ai-idle");
    }

    /// 无 hook 的 pane（WSL/SSH/hook 关闭）维持轮询逻辑：
    /// 输入检测在会话中 + 无近期输出 → ai-idle；不在会话中 → idle。
    #[test]
    fn fallback_path_without_hook_unchanged() {
        let hooks = HookState::new();
        let mgr = SessionTracker::new();

        assert_eq!(resolve_status(&hooks, &mgr, 1), "idle");
        mgr.track_input(1, "claude\r");
        assert_eq!(resolve_status(&hooks, &mgr, 1), "ai-idle");
    }

    /// 回归测试（PR #43 评审）：hook server 未运行**不得**让 AI 感知整体归零。
    /// `hook_enabled` 默认是 false，曾经的 `if !server_running { return "idle" }`
    /// 让全新安装与从没进过设置页的存量用户一次性失去全部 AI 徽章、完成通知与
    /// 托盘灯；WSL/SSH/opencode/pi 这些永远拿不到 hook 上报的 pane 更是彻底没
    /// 出路。判定只看 pane 自己有没有 hook（is_hook_enabled），与 server 无关。
    #[test]
    fn no_hook_server_still_falls_back_to_polling() {
        let hooks = HookState::new();
        let mgr = SessionTracker::new();

        // server 从未启动 → hook_state 里没有该 pty 的任何记录
        mgr.track_input(1, "claude\r"); // 只有输入检测标记了 AI 会话
        assert!(!hooks.is_hook_enabled(1));

        assert_eq!(resolve_status(&hooks, &mgr, 1), "ai-idle");
    }

    #[test]
    fn production_poll_preserves_silent_hooks_even_after_weak_exit_input() {
        assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
        let hooks = HookState::new();
        let tracker = SessionTracker::new();
        let seen = Arc::new(Mutex::new(Vec::<StatusChange>::new()));
        let sink = seen.clone();
        let emitter = StatusEmitter::new(Arc::new(move |change: StatusChange| {
            sink.lock().push(change);
        }));
        let states = [
            (1, "ai-working", "PreToolUse"),
            (2, "ai-working", "UserPromptSubmit"),
            (3, "ai-working", "PermissionRequest"),
            (4, "ai-idle", "Stop"),
            (5, "error", "StopFailure"),
        ];
        for (pty_id, status, cause) in states {
            tracker.track_input_with_line_snapshot(pty_id, "claude\r", None);
            hooks.update(pty_id, status.to_string());
            emitter.emit_if_changed_with_episode(pty_id, status, Some(cause), Some("claude".into()),
                tracker.weak_detection_episode(pty_id));
        }
        tracker.track_input_with_line_snapshot(2, "\x03", None);
        tracker.track_input_with_line_snapshot(2, "\x03", None);
        assert!(!tracker.is_ai_session(2));
        // Cross the removed ten-second stall window using the real production
        // poll helper, not a test-only copy of its decision logic.
        thread::sleep(Duration::from_secs(11));
        for _ in 0..4 {
            poll_panes(&hooks, &tracker, &emitter, &[1, 2, 3, 4, 5]);
        }
        assert_eq!(seen.lock().len(), states.len());
        for (pty_id, status, cause) in states {
            assert_eq!(hooks.get_status(pty_id).as_deref(), Some(status));
            assert_eq!(emitter.last_cause(pty_id).as_deref(), Some(cause));
            assert!(hooks.status_age(pty_id).unwrap() >= Duration::from_secs(10));
        }
        hooks.update(1, "ai-idle".into());
        emitter.emit_if_changed_with_episode(1, "ai-idle", Some("Stop"), Some("claude".into()),
            tracker.weak_detection_episode(1));
        hooks.remove(2);
        emitter.emit_if_changed_with_episode(2, "idle", Some("SessionEnd"), None,
            tracker.weak_detection_episode(2));
        poll_panes(&hooks, &tracker, &emitter, &[1, 2, 3, 4, 5]);
        let changes = seen.lock();
        assert_eq!(changes.len(), states.len() + 2);
        assert_eq!(changes[states.len()].cause.as_deref(), Some("Stop"));
        assert_eq!(changes[states.len() + 1].cause.as_deref(), Some("SessionEnd"));
        assert!(changes.iter().all(|change| !matches!(change.cause.as_deref(), Some("Stall" | "StallExit"))));
    }

    #[test]
    fn production_poll_keeps_unhooked_input_liveness_independent_of_output() {
        let hooks = HookState::new();
        let tracker = SessionTracker::new();
        let seen = Arc::new(Mutex::new(Vec::<StatusChange>::new()));
        let sink = seen.clone();
        let emitter = StatusEmitter::new(Arc::new(move |change: StatusChange| {
            sink.lock().push(change);
        }));
        tracker.track_input_with_line_snapshot(1, "codex\r", None);
        poll_panes(&hooks, &tracker, &emitter, &[1]);
        for output in ["shell output", "\x1b[2Jredraw", "done", "working"] {
            tracker.note_output(1, output);
            poll_panes(&hooks, &tracker, &emitter, &[1]);
        }
        let changes = seen.lock();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].status, "ai-idle");
        assert_eq!(changes[0].agent.as_deref(), Some("codex"));
        assert_eq!(changes[0].cause, None);
        assert_eq!(changes[0].weak_episode, tracker.weak_detection_episode(1));
        assert!(tracker.is_ai_session(1));
        assert!(!hooks.is_hook_enabled(1));
    }

    #[test]
    fn changed_input_episode_does_not_clear_authoritative_hook_cause() {
        let tracker = SessionTracker::new();
        let hooks = HookState::new();
        let emitter = StatusEmitter::new(Arc::new(|_: StatusChange| {}));
        tracker.track_input_with_line_snapshot(1, "claude\r", None);
        hooks.update(1, "ai-working".into());
        emitter.emit_if_changed_with_episode(1, "ai-working", Some("PermissionRequest"), Some("claude".into()),
            tracker.weak_detection_episode(1));
        tracker.clear_ai_session(1);
        tracker.track_input_with_line_snapshot(1, "claude\r", None);
        poll_panes(&hooks, &tracker, &emitter, &[1]);
        assert_eq!(emitter.last_cause(1).as_deref(), Some("PermissionRequest"));
        assert_eq!(hooks.get_status(1).as_deref(), Some("ai-working"));
    }

    #[test]
    fn production_input_hook_exit_dedup_and_later_episode_do_not_resurrect_fallback() {
        use crate::{AgentActivity, AgentApplyOutcome, AgentConfirmation, AgentConnectivity,
            AgentEvidence, AgentObservation, AgentObservationIgnored, AgentRoute, AgentRuntimeRegistry};
        use mt_identity::{AgentEventId, ExecutionHostId, HostInstallId, PaneKey, RepoId,
            TabId, TerminalIncarnationId, TerminalSessionId, WorktreeId};

        struct RuntimeSink {
            route: AgentRoute,
            state: Arc<Mutex<(AgentRuntimeRegistry, u64)>>,
        }
        impl StatusSink for RuntimeSink {
            fn status_changed(&self, change: StatusChange) {
                let mut state = self.state.lock();
                state.1 += 1;
                let sequence = state.1;
                let activity = crate::activity_from_legacy_status(&change.status, change.cause.as_deref()).unwrap();
                let hook = change.cause.is_some();
                let outcome = if hook && activity.is_ended() && change.agent.is_none() {
                    state.0.observe_hook_exit(self.route.clone(), AgentEventId::new(), sequence, None, 100)
                } else {
                    let provider = change.agent.as_deref().map(|agent| agent.parse().unwrap())
                        .or_else(|| state.0.active_run_for_route(&self.route).map(|run| run.provider.clone()))
                        .expect("an emitted AI/lifecycle observation has an owner");
                    state.0.observe(AgentObservation {
                        event_id: AgentEventId::new(), route: self.route.clone(), sequence,
                        connection_epoch: None, provider, provider_session_id: None, process: None,
                        weak_episode: change.weak_episode, activity,
                        connectivity: AgentConnectivity::Live, confirmation: AgentConfirmation::LiveConfirmed,
                        evidence: if hook { AgentEvidence::Hook } else { AgentEvidence::PtyActivity },
                        received_at_unix_ms: 100,
                    })
                };
                assert!(matches!(&outcome, AgentApplyOutcome::Applied { .. }), "{outcome:?}");
            }

            fn session_identified(&self, identity: SessionIdentity) {
                let mut state = self.state.lock();
                state.1 += 1;
                let sequence = state.1;
                let outcome = state.0.observe(AgentObservation {
                    event_id: AgentEventId::new(), route: self.route.clone(), sequence,
                    connection_epoch: None, provider: identity.agent.unwrap().parse().unwrap(),
                    provider_session_id: Some(identity.session_id), process: None,
                    weak_episode: identity.weak_episode, activity: AgentActivity::Unknown,
                    connectivity: AgentConnectivity::Live, confirmation: AgentConfirmation::LiveConfirmed,
                    evidence: AgentEvidence::Hook, received_at_unix_ms: 100,
                });
                assert!(matches!(outcome, AgentApplyOutcome::Applied { created: true, .. }));
            }
        }

        let host = ExecutionHostId::derive("local", &HostInstallId::new());
        let route = AgentRoute {
            worktree_id: WorktreeId::derive(&RepoId::derive(&host, "/repo/.git"), "/repo", None),
            execution_host_id: host, tab_id: TabId::new(), pane_key: PaneKey::new(),
            terminal_session_id: TerminalSessionId::new(), terminal_incarnation_id: TerminalIncarnationId::new(),
        };
        let state = Arc::new(Mutex::new((AgentRuntimeRegistry::default(), 0)));
        let emitter = StatusEmitter::new(Arc::new(RuntimeSink { route: route.clone(), state: state.clone() }));
        let tracker = SessionTracker::new();
        let hooks = HookState::new();
        tracker.track_input_with_line_snapshot(1, "claude\r", None);
        let old_episode = tracker.weak_detection_episode(1).unwrap();
        poll_panes(&hooks, &tracker, &emitter, &[1]);
        let weak = state.lock().0.active_run_for_route(&route).unwrap().clone();
        assert_eq!(weak.weak_episode, Some(old_episode));
        assert_eq!(weak.activity, AgentActivity::Unknown);
        emitter.notify_session_identity(SessionIdentity {
            pty_id: 1, agent: Some("claude".into()), session_id: "session".into(), cwd: None,
            weak_episode: tracker.weak_detection_episode(1),
            hook_lifecycle: None,
        });
        {
            let state = state.lock();
            assert_eq!(state.0.runs().count(), 2);
            assert!(state.0.is_superseded_weak_alias(&weak.run_id));
            assert_eq!(state.0.fallback_supersession(&weak.run_id).unwrap().sequence, 2);
            assert_eq!(state.0.active_run_for_route(&route).unwrap().evidence, AgentEvidence::Hook);
        }
        hooks.update(1, "ai-working".into());
        tracker.mark_ai_session(1, "claude");
        emitter.emit_if_changed_with_episode(1, "ai-working", Some("UserPromptSubmit"), Some("claude".into()),
            tracker.weak_detection_episode(1));
        hooks.remove(1);
        tracker.clear_ai_session(1);
        emitter.emit_if_changed_with_episode(1, "idle", Some("SessionEnd"), None,
            tracker.weak_detection_episode(1));
        for _ in 0..4 { poll_panes(&hooks, &tracker, &emitter, &[1]); }
        {
            let state = state.lock();
            assert_eq!(state.1, 4, "Hook exit already emitted idle; monitor must dedup");
            assert_eq!(state.0.run(&weak.run_id), Some(&weak));
            assert!(state.0.is_superseded_weak_alias(&weak.run_id));
            assert!(state.0.active_run_for_route(&route).is_none());
        }
        tracker.track_input_with_line_snapshot(1, "claude\r", None);
        assert!(tracker.weak_detection_episode(1).unwrap() > old_episode);
        poll_panes(&hooks, &tracker, &emitter, &[1]);
        let later = state.lock().0.active_run_for_route(&route).unwrap().clone();
        assert_ne!(later.run_id, weak.run_id);
        assert_eq!(later.activity, AgentActivity::Unknown);
        for epoch in [None, Some(99)] {
            let mut state = state.lock();
            let outcome = state.0.observe(AgentObservation {
                event_id: AgentEventId::new(), route: route.clone(), sequence: 100,
                connection_epoch: epoch, provider: weak.provider.clone(), provider_session_id: None,
                process: None, weak_episode: Some(old_episode), activity: AgentActivity::Working,
                connectivity: AgentConnectivity::Live, confirmation: AgentConfirmation::LiveConfirmed,
                evidence: AgentEvidence::PtyActivity, received_at_unix_ms: 99_000,
            });
            assert_eq!(outcome, AgentApplyOutcome::Ignored(AgentObservationIgnored::SupersededWeakEpisode));
            assert_eq!(state.0.run(&later.run_id), Some(&later));
            assert!(state.0.is_superseded_weak_alias(&weak.run_id));
        }
        for _ in 0..3 { poll_panes(&hooks, &tracker, &emitter, &[1]); }
        assert_eq!(state.lock().1, 5);
        tracker.track_input_with_line_snapshot(1, "/exit\r", None);
        poll_panes(&hooks, &tracker, &emitter, &[1]);
        assert!(state.lock().0.active_run_for_route(&route).is_none());
    }

    // ---- StatusSink 注入(替代原 Tauri emit) ----

    /// 去重表的语义必须原样保留:状态相同 + 无成因的重发被吞、attention 成因
    /// 每次都放行、状态一变就放行。这条与传输层无关——它防的是迟到 hook 事件
    /// 推错状态后 monitor 的纠正被吞掉。
    #[test]
    fn sink_receives_deduplicated_changes() {
        let seen: Arc<Mutex<Vec<(String, Option<String>)>>> = Arc::new(Mutex::new(Vec::new()));
        let sink_seen = seen.clone();
        let emitter = StatusEmitter::new(Arc::new(move |c: StatusChange| {
            sink_seen.lock().push((c.status, c.cause));
        }));

        emitter.emit_if_changed(1, "ai-working", None, None);
        emitter.emit_if_changed(1, "ai-working", None, None); // 同状态无成因 → 吞
        emitter.emit_if_changed(1, "ai-working", Some("PermissionRequest"), None);
        emitter.emit_if_changed(1, "ai-working", Some("PermissionRequest"), None); // attention → 放行
        emitter.emit_if_changed(1, "ai-working", Some("PreToolUse"), None);
        emitter.emit_if_changed(1, "ai-working", Some("PreToolUse"), None); // 同成因 → 吞
        emitter.emit_if_changed(1, "ai-idle", Some("Stop"), None);

        let got = seen.lock().clone();
        assert_eq!(
            got,
            vec![
                ("ai-working".to_string(), None),
                ("ai-working".to_string(), Some("PermissionRequest".into())),
                ("ai-working".to_string(), Some("PermissionRequest".into())),
                ("ai-working".to_string(), Some("PreToolUse".into())),
                ("ai-idle".to_string(), Some("Stop".into())),
            ]
        );
    }

    /// pane 关闭后去重记录要清掉,否则同 id 复用时新状态会被旧记录吞掉。
    #[test]
    fn retain_drops_dead_panes() {
        let emitter = StatusEmitter::new(Arc::new(|_: StatusChange| {}));
        emitter.emit_if_changed(1, "ai-idle", Some("Stop"), None);
        assert_eq!(emitter.last_cause(1).as_deref(), Some("Stop"));
        emitter.retain(&[2]);
        assert_eq!(emitter.last_cause(1), None);
    }
}
