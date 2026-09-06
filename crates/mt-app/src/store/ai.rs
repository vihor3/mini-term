//! AI 感知相关的 `AppStore` 方法:AI 任务标记(⚑)、AI 事件落地、通知 / 待办、
//! 会话分支自记账。
//!
//! 从 `store.rs` 原样搬来的几段(`// === AI 任务标记 ===` / `// === AI 事件 ===` /
//! `// === 通知 / 待办 ===` / `// === 会话分支自记账 ===`),段注释随代码走,
//! 逻辑一行未改。终端回收(`dispose_terminal` 一族)跟着标记段一起来 ——
//! 它做的正是「清 AI 感知痕迹」,原文件里也紧挨着标记段。

use gpui::Context;
use mt_ai::{
    AgentActivity, AgentApplyOutcome, AgentConfirmation, AgentConnectivity, AgentEvidence,
    AgentObservation, AgentProvider, AgentRuntimeRegistry,
};
use mt_identity::AgentEventId;

use crate::ai::AiEvent;
use crate::markers::{self, AiMarker, MarkerBatch};
use crate::notify::{NotifyPrefs, PaneRef, StatusTransition};
use crate::tree::{AiSessionRef, PaneStatus, SplitNode};

use super::identity::TerminalRoute;
use super::pure::{
    AiProjects, DoneScope, PendingFork, TitleBarLight, collect_ai_projects,
    compute_title_bar_light, find_pane_of_pty, push_lineage_edge, resolve_fork_edge,
};
use super::remote_runtime::RemoteRuntimePhase;
use super::{AppStore, PendingAlert};

fn captured_route_matches(
    captured: Option<&TerminalRoute>,
    current: Option<&TerminalRoute>,
    terminal_exited: bool,
) -> bool {
    if terminal_exited {
        return false;
    }
    match (captured, current) {
        (Some(captured), Some(current)) => captured == current,
        (None, None) => true,
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AgentPaneProjection {
    pub status: PaneStatus,
    pub live: bool,
    pub attention: bool,
    pub provider: Option<String>,
    pub evidence: AgentEvidence,
}

impl AgentPaneProjection {
    pub(super) fn apply_to_layout(&self, layout: &mut SplitNode, pty_id: u32) -> bool {
        if !self.live {
            return layout.update_status_by_pty(
                pty_id,
                self.status,
                self.attention,
                self.provider.as_deref(),
            );
        }
        let Some(pane) = layout.pane_by_pty_mut(pty_id) else {
            return false;
        };
        // Unknown maps to legacy Idle, but does not mean the Agent exited.
        pane.status = self.status;
        pane.attention = self.attention;
        pane.detected_agent.clone_from(&self.provider);
        true
    }
}

pub(super) fn live_agent_runs_for_route<'a>(
    registry: &'a AgentRuntimeRegistry,
    route: &'a TerminalRoute,
) -> impl Iterator<Item = &'a mt_ai::AgentRuntimeState> {
    registry.runs().filter(move |state| {
        &state.route == route
            && !state.activity.is_ended()
            && state.confirmation == AgentConfirmation::LiveConfirmed
            && state.evidence != AgentEvidence::RestoredHistory
            && !registry.is_superseded_weak_alias(&state.run_id)
    })
}

pub(super) fn single_live_agent_for_route<'a>(
    registry: &'a AgentRuntimeRegistry,
    route: &'a TerminalRoute,
) -> Option<&'a mt_ai::AgentRuntimeState> {
    let mut runs = live_agent_runs_for_route(registry, route);
    let run = runs.next()?;
    runs.next().is_none().then_some(run)
}

pub(super) fn accepted_agent_projection(
    registry: &AgentRuntimeRegistry,
    route: &TerminalRoute,
    retained_attention: bool,
) -> AgentPaneProjection {
    let active = || live_agent_runs_for_route(registry, route);
    let evidence = active()
        .map(|state| state.evidence)
        .max()
        .unwrap_or(AgentEvidence::RestoredHistory);
    let mut projection = AgentPaneProjection {
        status: PaneStatus::Idle,
        live: false,
        attention: evidence == AgentEvidence::Hook && retained_attention,
        provider: None,
        evidence,
    };
    let mut providers = std::collections::HashSet::new();
    // The registry has already reconciled evidence within each run. Independent
    // accepted runs all contribute; receipt order must not pick a pane owner.
    for state in active() {
        projection.live = true;
        let status = PaneStatus::from_str(state.activity.legacy_status())
            .expect("agent activity has a legacy projection");
        if status.priority() > projection.status.priority() {
            projection.status = status;
        }
        projection.attention |= state.activity == AgentActivity::Blocked;
        providers.insert(state.provider.as_str());
    }
    if providers.len() == 1 {
        projection.provider = providers.into_iter().next().map(str::to_string);
    }
    projection
}

pub(super) fn agent_display_status(
    registry: &AgentRuntimeRegistry,
    route: &TerminalRoute,
    now_unix_ms: i64,
) -> Option<PaneStatus> {
    live_agent_runs_for_route(registry, route).map(|run| {
        if matches!(run.activity, AgentActivity::Starting | AgentActivity::Working)
            && (run.connectivity != AgentConnectivity::Live
                || registry.activity_freshness(&run.run_id, now_unix_ms)
                    != mt_ai::AgentActivityFreshness::Fresh)
        {
            PaneStatus::Idle
        } else {
            PaneStatus::from_str(run.activity.legacy_status())
                .expect("agent activity has a legacy projection")
        }
    }).max_by_key(|status| status.priority())
}

fn project_status_observation(
    registry: &AgentRuntimeRegistry,
    route: Option<&TerminalRoute>,
    outcome: Option<AgentApplyOutcome>,
    change: &mt_ai::StatusChange,
    old_attention: bool,
    hook_event: bool,
) -> Option<AgentPaneProjection> {
    let incoming_attention = change
        .cause
        .as_deref()
        .is_some_and(mt_ai::is_attention_cause);
    match outcome {
        Some(AgentApplyOutcome::Ignored(_)) => None,
        Some(AgentApplyOutcome::Applied { .. }) => Some(accepted_agent_projection(
            registry,
            route?,
            if hook_event {
                incoming_attention
            } else {
                old_attention
            },
        )),
        None => Some(AgentPaneProjection {
            status: PaneStatus::from_str(&change.status)?,
            live: matches!(change.status.as_str(), "ai-working" | "ai-idle"),
            attention: incoming_attention,
            provider: change.agent.clone(),
            evidence: AgentEvidence::PtyActivity,
        }),
    }
}

fn activity_for_session_identity(
    registry: &AgentRuntimeRegistry,
    route: &TerminalRoute,
    provider: &AgentProvider,
    session_id: &str,
) -> AgentActivity {
    let eligible = |run: &&mt_ai::AgentRuntimeState| {
        &run.route == route && &run.provider == provider && !run.activity.is_ended()
            && run.confirmation == AgentConfirmation::LiveConfirmed
            && !registry.is_superseded_weak_alias(&run.run_id)
    };
    let unique_activity = |mut runs: Vec<&mt_ai::AgentRuntimeState>| {
        if runs.len() == 1 { runs.pop().map(|run| run.activity) } else { None }
    };
    let exact = registry.runs().filter(eligible)
        .filter(|run| run.provider_session_id.as_deref() == Some(session_id)).collect::<Vec<_>>();
    if !exact.is_empty() {
        return unique_activity(exact).unwrap_or(AgentActivity::Unknown);
    }
    unique_activity(registry.runs().filter(eligible)
        .filter(|run| run.evidence == AgentEvidence::Hook && run.provider_session_id.is_none())
        .collect())
        .unwrap_or(AgentActivity::Unknown)
}

fn is_hook_observation(cause: Option<&str>) -> bool {
    cause.is_some_and(|cause| !matches!(cause, "Stall" | "StallExit"))
}

fn unique_route_provider(registry: &AgentRuntimeRegistry, route: &TerminalRoute) -> Option<AgentProvider> {
    let mut providers = registry.runs()
        .filter(|run| &run.route == route && !run.activity.is_ended()
            && !registry.is_superseded_weak_alias(&run.run_id))
        .map(|run| &run.provider);
    let provider = providers.next()?;
    providers.all(|other| other == provider).then(|| provider.clone())
}

pub(crate) fn record_runtime_status_change(
    registry: &mut AgentRuntimeRegistry,
    route: TerminalRoute,
    event_id: AgentEventId,
    sequence: u64,
    connection_epoch: Option<u64>,
    received_at_unix_ms: i64,
    change: &mt_ai::StatusChange,
) -> Option<AgentApplyOutcome> {
    let activity = mt_ai::activity_from_legacy_status(&change.status, change.cause.as_deref())?;
    let hook_event = is_hook_observation(change.cause.as_deref());
    let exact_session = change.hook_session.as_ref().filter(|_| hook_event);
    if let Some(session) = exact_session {
        let provider = session.agent.as_deref().unwrap_or("claude").parse::<AgentProvider>().ok();
        let valid_session = !session.session_id.trim().is_empty()
            && session.session_id.len() <= 512
            && !session.session_id.chars().any(char::is_control);
        let Some(provider) = provider.filter(|_| valid_session) else {
            return Some(AgentApplyOutcome::Ignored(mt_ai::AgentObservationIgnored::UnresolvedHookOwner));
        };
        // An exact end cannot create a new ended run or attach to an unbound
        // same-provider Hook. Only the already recognized session may end.
        if session.lifecycle_id.is_none() && activity.is_ended() && registry.runs().filter(|run| {
            run.route == route && run.provider == provider
                && run.provider_session_id.as_deref() == Some(session.session_id.as_str())
                && run.evidence == AgentEvidence::Hook && !run.activity.is_ended()
                && run.confirmation == AgentConfirmation::LiveConfirmed
        }).take(2).count() != 1 {
            return Some(AgentApplyOutcome::Ignored(mt_ai::AgentObservationIgnored::UnresolvedHookOwner));
        }
        let observation = AgentObservation {
            event_id, route, sequence, connection_epoch, received_at_unix_ms,
            weak_episode: change.weak_episode,
            provider,
            provider_session_id: Some(session.session_id.clone()),
            process: None,
            activity,
            connectivity: AgentConnectivity::Live,
            confirmation: AgentConfirmation::LiveConfirmed,
            evidence: AgentEvidence::Hook,
        };
        return Some(match session.lifecycle_id {
            Some(lifecycle_id) => registry.observe_hook_lifecycle(observation, lifecycle_id),
            None => registry.observe(observation),
        });
    }
    if change.agent.is_none() && hook_event && activity.is_ended() {
        return Some(registry.observe_hook_exit(
            route, event_id, sequence, connection_epoch, received_at_unix_ms,
        ));
    }
    let provider = change.agent.as_deref()
        .and_then(|provider| provider.parse::<AgentProvider>().ok())
        .or_else(|| unique_route_provider(registry, &route));
    let Some(provider) = provider else {
        return registry.runs().any(|run| run.route == route && !run.activity.is_ended()
            && !registry.is_superseded_weak_alias(&run.run_id))
            .then_some(AgentApplyOutcome::Ignored(mt_ai::AgentObservationIgnored::AmbiguousRun));
    };
    if !hook_event && registry.runs().filter(|run| {
        run.route == route && run.provider == provider && !run.activity.is_ended()
            && run.evidence >= AgentEvidence::ProcessAttested
            && !registry.is_superseded_weak_alias(&run.run_id)
    }).take(2).count() > 1 {
        return Some(AgentApplyOutcome::Ignored(mt_ai::AgentObservationIgnored::AmbiguousRun));
    }
    // Queued monitor events stay weak even if Hook enabled before delivery.
    Some(registry.observe(AgentObservation {
        event_id, route, sequence, connection_epoch, received_at_unix_ms,
        weak_episode: change.weak_episode,
        provider,
        provider_session_id: None,
        process: None,
        activity,
        connectivity: AgentConnectivity::Live,
        confirmation: AgentConfirmation::LiveConfirmed,
        evidence: if hook_event { AgentEvidence::Hook } else { AgentEvidence::PtyActivity },
    }))
}

fn record_runtime_session_identity(
    registry: &mut AgentRuntimeRegistry,
    route: TerminalRoute,
    event_id: AgentEventId,
    sequence: u64,
    connection_epoch: Option<u64>,
    received_at_unix_ms: i64,
    identity: &mt_ai::SessionIdentity,
) -> AgentApplyOutcome {
    let provider = match identity.agent.as_deref().unwrap_or("claude").parse::<AgentProvider>() {
        Ok(provider) => provider,
        Err(_) if identity.hook_lifecycle.is_some() => {
            return AgentApplyOutcome::Ignored(mt_ai::AgentObservationIgnored::UnresolvedHookOwner);
        }
        Err(_) => AgentProvider::CLAUDE.parse().expect("known provider"),
    };
    let activity = activity_for_session_identity(registry, &route, &provider, &identity.session_id);
    let observation = AgentObservation {
        event_id, route, sequence, connection_epoch, received_at_unix_ms,
        weak_episode: identity.weak_episode,
        provider,
        provider_session_id: Some(identity.session_id.clone()),
        process: None,
        activity,
        connectivity: AgentConnectivity::Live,
        confirmation: AgentConfirmation::LiveConfirmed,
        evidence: AgentEvidence::Hook,
    };
    match identity.hook_lifecycle {
        Some(mt_ai::HookLifecycleEvent::Started(id)) => registry.start_hook_lifecycle(observation, id),
        Some(mt_ai::HookLifecycleEvent::Observed(id)) => registry.observe_hook_lifecycle(observation, id),
        None => registry.observe(observation),
    }
}

impl AppStore {
    // === AI 任务标记(⚑)===

    /// 某个 pane 的标记列表(没有就是空)。对应 `store.ts:1225` 的 `getMarkersForPty`。
    ///
    /// ⚠️ 这是**内部全量**,含正文还没验明正身的候选条目。给用户看的一律走
    /// [`Self::visible_markers_for_pty`] —— 见 [`crate::markers::AiMarker::confirmed`]。
    pub fn markers_for_pty(&self, pty_id: u32) -> &[AiMarker] {
        self.markers_by_pty
            .get(&pty_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// 能给用户看的那些(`⚑ N` 的计数与下拉列表**共用这一个口**)。
    pub fn visible_markers_for_pty(&self, pty_id: u32) -> Vec<AiMarker> {
        markers::visible(self.markers_for_pty(pty_id))
            .cloned()
            .collect()
    }

    /// 落一批标记(pane 在 [`crate::pane::TerminalPane::write`] 里当场取好锚点后发来)。
    ///
    /// 节奏照抄 `useAiSubmitMarker.ts:20-23`:**追加之后立刻剪一遍枝**,
    /// 不在渲染路径上剪(见 [`crate::markers`] 的模块注释)。
    // 拆分前是私有方法;调用点在 `store::panes` 的 PTY 事件订阅里,升到 `pub(super)`。
    pub(super) fn add_markers(&mut self, pty_id: u32, batch: MarkerBatch, cx: &mut Context<Self>) {
        if batch.submits.is_empty() {
            return;
        }
        // 先把旧条目收拾一遍(锚点行已经不是原来那行的降级或删、挂着的补锚),再追加
        // 新的 —— 「⚑ N」不能一直挂着已经跳不对的条目。
        // 新条目要在这之后 push:它的指纹刚取,自己校验自己没有意义。
        self.refresh_markers(pty_id, cx);
        let list = self.markers_by_pty.entry(pty_id).or_default();
        for submit in batch.submits {
            markers::push_marker(list, pty_id, submit, batch.anchor);
        }
        markers::prune(list, batch.history, batch.max_scrollback);
        // 过滤后为空则连键一起删(`store.ts:1219` 的同一处置)
        let empty = list.is_empty();
        if empty {
            self.markers_by_pty.remove(&pty_id);
            self.marker_cursor.remove(&pty_id);
        }
        cx.notify();
    }

    /// 收拾一遍某个 pane 的标记:**失效的处置 + 挂着的补锚**,返回「列表变过没有」。
    ///
    /// 三件事按这个顺序,少一步或者换个顺序都不对:
    ///
    /// 1. [`markers::prune`] —— scrollback 装满,整份作废(算术锚点从此不可信);
    /// 2. [`markers::prune_stale`] —— 校验已定锚的那些:锚点行已经不是原来那行的,
    ///    键入的降级回挂起、猜来的删(分流理由见 [`crate::markers`] 模块注释);
    /// 3. [`markers::relocate_pending`] —— 给挂着的(含上一步刚降级的)补锚。
    ///    **必须排在校验之后**:刚补上的指纹是从同一份 grid 读的,当轮自校必过、
    ///    白跑;而这个顺序让降级的条目当轮就能回扫找回,不用灰一拍等下一次。
    ///
    /// 跑的时机:新增标记时、跳转前、下拉打开时 —— **一律不在渲染路径上**
    /// (见 [`crate::markers`] 模块注释)。
    fn refresh_markers(&mut self, pty_id: u32, cx: &mut Context<Self>) -> bool {
        let Some(entity) = self.terminals.get(&pty_id).cloned() else {
            return false;
        };
        let pane = entity.read(cx);
        let (history, max) = pane.scrollback_state();
        // alt screen 期间读的是备用 grid,校验会把整份标记误杀、回扫也扫不到主屏
        let probe_ok = pane.can_probe_lines();
        let (bottom, viewport) = pane.scan_bounds();
        let Some(list) = self.markers_by_pty.get_mut(&pty_id) else {
            return false;
        };
        let mut changed = markers::prune(list, history, max);
        if probe_ok {
            changed |= markers::prune_stale(list, |anchor| pane.line_fingerprint(anchor));
            changed |= markers::relocate_pending(list, bottom, viewport, |row| pane.line_text(row));
        }
        let empty = list.is_empty();
        if empty {
            self.markers_by_pty.remove(&pty_id);
            self.marker_cursor.remove(&pty_id);
        }
        changed
    }

    /// 打开「⚑」下拉之前收拾一遍 —— 用户要看的这一眼必须是最新的:AI 刚把排队的
    /// 那条处理掉的话,这次补锚就能让它从「灰的、点不动」变回可跳。
    pub fn refresh_markers_for_pty(&mut self, pty_id: u32, cx: &mut Context<Self>) {
        if self.refresh_markers(pty_id, cx) {
            cx.notify();
        }
    }

    /// 整份丢掉(`store.ts:1205-1211` 的 `clearMarkersForPty`)。游标一并清 ——
    /// 原版那份游标从不清理,这里顺手修掉。
    fn clear_markers_for_pty(&mut self, pty_id: u32) {
        self.markers_by_pty.remove(&pty_id);
        self.marker_cursor.remove(&pty_id);
    }

    /// 跳到某一条标记:滚到视口顶部 + 闪 300ms,并把游标推到它身上。
    ///
    /// 浮层点击与 Ctrl+Shift+↑/↓ **走的是同一条路**(原版 `useMarkerHotkeys.ts:56`
    /// 与 `MarkerList.tsx:36-39` 调的都是 `scrollToMarker`),**不关任何东西**。
    ///
    /// 返回「这一下真的跳了没有」:跳不动的三种情形(pane 没了 / 标记还挂着没定位 /
    /// pane 正在 alt screen 里)都是 `false`,调用方据此**不推游标、不关浮层**。
    pub fn jump_to_marker(&mut self, pty_id: u32, marker_id: &str, cx: &mut Context<Self>) -> bool {
        let Some(entity) = self.terminals.get(&pty_id).cloned() else {
            return false;
        };
        // 跳之前先收拾一遍:挂着的趁机补锚(点的可能正是刚被 AI 处理掉的那条),
        // 锚点已经不可信的宁可什么都不做,也不能跳到错的行上 —— 见
        // [`Self::refresh_markers`] 与 [`crate::markers`] 模块注释
        if self.refresh_markers(pty_id, cx) {
            cx.notify();
        }
        let Some(anchor) = self
            .markers_for_pty(pty_id)
            .iter()
            .find(|m| m.id == marker_id)
            // 还挂着的跳不了:那条消息还没上屏,没有目标行可跳。**静默不动**,
            // 与「列表空 / 到头」同一个处置(`useMarkerHotkeys.ts:39`、`:50`)
            .and_then(|m| m.anchor.settled())
        else {
            return false;
        };
        // 跳不动(pane 正在 alt screen 里)就不推游标 —— 连按方向键不该空走格子
        if entity.update(cx, |pane, cx| pane.scroll_to_marker(anchor, cx)) {
            self.marker_cursor.insert(pty_id, marker_id.to_string());
            return true;
        }
        false
    }

    /// Ctrl+Shift+↑ / ↓。`dir = -1` 上一条、`+1` 下一条,**非环形**。
    ///
    /// 目标 pane 的解析与其它全局动作同口径:焦点 pane → 布局里第一个激活 pane
    /// ([`Self::active_pane_id`],原版是 `focusedPtyIdFromDom()` → `resolveActivePane`)。
    /// 列表空 / 到头都是静默不动,不弹任何提示(`useMarkerHotkeys.ts:39`、`:50`)。
    pub fn step_marker(&mut self, dir: i32, cx: &mut Context<Self>) {
        let Some(project_id) = self.active_project_id.clone() else {
            return;
        };
        let Some(pty_id) = self.active_pane_id(&project_id).and_then(|pane_id| {
            self.project_states
                .get(&project_id)
                .and_then(|s| s.pane(&pane_id))
                .and_then(|p| p.pty_id)
        }) else {
            return;
        };
        // 先收拾一遍再挑目标:否则刚被 AI 处理掉的那条还挂着「跳不了」的旧状态,
        // 这一下会白白跳过它
        self.refresh_markers_for_pty(pty_id, cx);
        let mut cursor = self.marker_cursor.get(&pty_id).and_then(|id| {
            self.markers_for_pty(pty_id)
                .iter()
                .position(|m| &m.id == id)
        });
        let len = self.markers_for_pty(pty_id).len();
        // 还挂着的条目跳不动,连按时要**跨过去**继续找下一条 —— 停在它身上的话
        // 游标不会推进,再按一次还是它,方向键就卡死了
        let target = loop {
            let Some(next) = markers::next_index(cursor, len, dir) else {
                return;
            };
            match self.markers_for_pty(pty_id).get(next) {
                Some(marker) if marker.anchor.settled().is_some() => break marker.id.clone(),
                Some(_) => cursor = Some(next),
                None => return,
            }
        };
        self.jump_to_marker(pty_id, &target, cx);
    }

    /// 回收一个终端:kill 子进程 + 清 AI 感知痕迹 + 摘掉视图与订阅。
    // 拆分前是私有方法;调用点散在 `projects` / `panes` / `ssh` / `layout`,升到 `pub(super)`。
    pub(super) fn dispose_terminal(&mut self, pty_id: u32, cx: &mut Context<Self>) {
        self.release_terminal(pty_id, true, cx);
    }

    /// Drops only the GUI attachment for hosted terminals. This is used when a
    /// project registration disappears while its worktree session remains live.
    pub(super) fn detach_terminal(&mut self, pty_id: u32, cx: &mut Context<Self>) {
        self.release_terminal(pty_id, false, cx);
    }

    fn release_terminal(&mut self, pty_id: u32, kill: bool, cx: &mut Context<Self>) {
        // 对应 `terminalCache.ts:546` 的 `aiPtyIds.delete(ptyId)` ——
        // 不摘的话新 PTY 复用同一个编号时会被误当成 AI pane(嗅探静默失效)
        crate::git_watch::forget_pane(pty_id);
        // 关 pane / 关整组 / 项目移除三条路的唯一汇合点,标记与游标在这里一并回收
        // (原版分散在 `setProjectLayout` 的 ptyId 集合比对、`disposePane`、
        // `removeProject` 三处,漏一处就是「pty id 复用后接手了上一任的标记」)
        self.clear_markers_for_pty(pty_id);
        // 分支登记同理:留着会让复用同一编号的新 PTY 认领上一任的 fork 登记
        self.clear_pending_fork(pty_id);
        // 退出登记同理:留着会让复用同一编号的新 PTY 一开就顶着「已断开」遮罩
        self.exited_ptys.remove(&pty_id);
        self.remove_remote_agent_terminal(pty_id);
        if kill && let Some(route) = self.terminal_routes.get(&pty_id).cloned() {
            self.remove_agent_runtime_route(&route);
        }
        self.terminal_routes.remove(&pty_id);
        if let Some(entity) = self.terminals.remove(&pty_id) {
            // 组合中关 pane:先把预编辑收掉,免得 IME 还挂在一个即将消失的
            // 输入宿主上(marked range 不收回,下一次按键会被 IME 永久劫持)
            entity.update(cx, |pane, cx| {
                pane.clear_preedit(cx);
                if kill {
                    pane.shutdown();
                } else {
                    pane.detach();
                }
            });
        }
        self.pane_subs.remove(&pty_id);
    }

    // 拆分前是私有方法;调用点在 `store::panes::split_pane_with_cwd`,升到 `pub(super)`。
    pub(super) fn pty_in_any_layout(&self, pty_id: u32) -> bool {
        self.project_states
            .values()
            .flat_map(|s| s.layouts())
            .any(|l| l.pane_by_pty(pty_id).is_some())
    }

    /// 子进程退出:pane 落 `error`。
    ///
    /// 旧版就是这个语义(`pty-exit` → `updatePaneStatusByPty('error')`):pane 不
    /// 自动关闭,用户主动 `exit` 与异常断开不做区分,画面留在原地可回看。
    // 拆分前是私有方法;调用点在 `store::panes` 的 PTY 事件订阅里,升到 `pub(super)`。
    pub(super) fn on_pty_exit(&mut self, pty_id: u32, code: Option<u32>, cx: &mut Context<Self>) {
        if let Some(code) = code
            && code != 0
        {
            eprintln!("[store] pane {pty_id} 子进程退出,退出码 {code}");
        }
        // fork 命令没能起起会话就退了 —— 这条登记不该等到下一个进程头上
        // (原版把 `clearPendingFork` 挂在 `pty-exit` 监听里,同一时机)
        self.clear_pending_fork(pty_id);
        // 原版 `App.tsx:359` 的 `markPtyExited`:与状态落 error 同一时机
        super::remote_agents::retire_terminal_polling(
            pty_id,
            &mut self.exited_ptys,
            &mut self.remote_agent_polls,
        );
        self.ai.remove_pane(pty_id);
        crate::git_watch::forget_pane(pty_id);
        // Keep the route for last-known views and explicit close. Transport loss
        // does not prove that the remote Agent process ended.
        if let Some(route) = self.terminal_routes.get(&pty_id).cloned() {
            self.mark_agent_connectivity(
                route,
                None,
                AgentConnectivity::Disconnected,
                chrono::Utc::now().timestamp_millis(),
            );
        }
        let mut touched: Option<String> = None;
        for (pid, state) in self.project_states.iter_mut() {
            let hit = state
                .layouts_mut()
                .any(|layout| layout.update_status_by_pty(pty_id, PaneStatus::Error, false, None));
            if hit {
                state.status = state.highest_status();
                touched = Some(pid.clone());
                break;
            }
        }
        if touched.is_some() {
            cx.notify();
        }
    }

    // === AI 事件 ===

    fn current_agent_connection_epoch(
        &self,
        project_id: &str,
        route: &TerminalRoute,
    ) -> Option<u64> {
        let project = self.project(project_id)?;
        project.ssh_connection_id.as_ref()?;
        let state = self.remote_runtime_projects.get(project_id)?;
        if state.phase != RemoteRuntimePhase::Ready {
            return None;
        }
        let snapshot = state.snapshot.as_ref()?;
        (snapshot.identity.execution_host_id == route.execution_host_id
            && snapshot.worktree_id == route.worktree_id)
            .then_some(snapshot.identity.connection_epoch)
    }

    fn observe_agent_status(
        &mut self,
        project_id: &str,
        route: Option<TerminalRoute>,
        event_id: AgentEventId,
        sequence: u64,
        change: &mt_ai::StatusChange,
    ) -> Option<AgentApplyOutcome> {
        let route = route?;
        let connection_epoch = self.current_agent_connection_epoch(project_id, &route);
        record_runtime_status_change(
            &mut self.agent_runtime, route, event_id, sequence, connection_epoch,
            chrono::Utc::now().timestamp_millis(), change,
        )
    }

    fn observe_agent_session(
        &mut self,
        project_id: Option<&str>,
        route: Option<TerminalRoute>,
        event_id: AgentEventId,
        sequence: u64,
        identity: &mt_ai::SessionIdentity,
    ) -> Option<AgentApplyOutcome> {
        let route = route?;
        let connection_epoch = project_id
            .and_then(|project_id| self.current_agent_connection_epoch(project_id, &route));
        Some(record_runtime_session_identity(
            &mut self.agent_runtime, route, event_id, sequence, connection_epoch,
            chrono::Utc::now().timestamp_millis(), identity,
        ))
    }

    /// 后台线程送上来的 AI 事件(见 `ai.rs` 的接线图)。
    ///
    /// 返回值是要执行的提醒动作(提示音 / 任务栏闪烁 / toast),由调用方在持有
    /// `Window` 的地方兑现 —— 见 [`PendingAlert`]。
    pub fn apply_ai_event(
        &mut self,
        event: AiEvent,
        cx: &mut Context<Self>,
    ) -> Option<PendingAlert> {
        match event {
            AiEvent::Status {
                change,
                route,
                event_id,
                sequence,
            } => {
                if !captured_route_matches(
                    route.as_ref(),
                    self.terminal_routes.get(&change.pty_id),
                    self.exited_ptys.contains(&change.pty_id),
                ) {
                    return None;
                }
                let status = PaneStatus::from_str(&change.status)?;
                let (owner, pane_id) = find_pane_of_pty(&self.project_states, change.pty_id)?;
                let pane = self.project_states.get(&owner)?.pane(&pane_id)?;
                let old_status = pane.status;
                let old_attention = pane.attention;
                let hook_event = is_hook_observation(change.cause.as_deref());
                let incoming_attention = change
                    .cause
                    .as_deref()
                    .is_some_and(mt_ai::is_attention_cause);
                let outcome =
                    self.observe_agent_status(&owner, route.clone(), event_id, sequence, &change);
                let agent_observation = outcome.is_some();
                let projection = project_status_observation(
                    &self.agent_runtime,
                    route.as_ref(),
                    outcome,
                    &change,
                    old_attention,
                    hook_event,
                )?;
                let projected_status = projection.status;
                let notify_transition = (hook_event || projection.evidence != AgentEvidence::Hook)
                    && projected_status == status
                    && projection.attention == incoming_attention;
                // Unknown task activity still belongs to a live Agent, not shell Git output.
                crate::git_watch::set_ai_pane(change.pty_id, projection.live);
                if let Some(state) = self.project_states.get_mut(&owner) {
                    state.layouts_mut().any(|layout| {
                        if agent_observation {
                            return projection.apply_to_layout(layout, change.pty_id);
                        }
                        layout.update_status_by_pty(
                            change.pty_id,
                            projected_status,
                            projection.attention,
                            projection.provider.as_deref(),
                        )
                    });
                    state.status = state.highest_status();
                }
                let project_active = self.active_project_id.as_deref() == Some(owner.as_str());

                let plan = if notify_transition {
                    self.done.apply(
                        &StatusTransition {
                            pane_id: &pane_id,
                            old_status,
                            new_status: projected_status,
                            old_attention,
                            cause: change.cause.as_deref(),
                            window_focused: self.window_focused,
                            project_active,
                        },
                        &self.notify_prefs(),
                    )
                } else {
                    crate::notify::AlertPlan::default()
                };
                if plan.mark_needs_attention
                    && let Some(state) = self.project_states.get_mut(&owner)
                {
                    state.needs_attention = true;
                }
                cx.notify();

                if plan.is_empty() {
                    return None;
                }
                Some(PendingAlert {
                    plan,
                    project_name: self
                        .project(&owner)
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| owner.clone()),
                    project_id: owner,
                    sound_path: self.config.ai_completion_sound_path.clone(),
                })
            }
            AiEvent::Session {
                identity,
                route,
                event_id,
                sequence,
            } => {
                if !captured_route_matches(
                    route.as_ref(),
                    self.terminal_routes.get(&identity.pty_id),
                    self.exited_ptys.contains(&identity.pty_id),
                ) {
                    return None;
                }
                let current_owner =
                    find_pane_of_pty(&self.project_states, identity.pty_id).map(|(owner, _)| owner);
                if matches!(
                    self.observe_agent_session(
                        current_owner.as_deref(),
                        route,
                        event_id,
                        sequence,
                        &identity,
                    ),
                    Some(AgentApplyOutcome::Ignored(_))
                ) {
                    return None;
                }
                let mut owner: Option<String> = None;
                let session = AiSessionRef {
                    agent: identity.agent.clone(),
                    session_id: identity.session_id.clone(),
                    cwd: identity.cwd.clone(),
                };
                for (pid, state) in self.project_states.iter_mut() {
                    if let Some(pane) = state.pane_by_pty_mut(identity.pty_id) {
                        pane.ai_session = Some(session.clone());
                        owner = Some(pid.clone());
                        break;
                    }
                }
                // 会话身份随布局落盘 —— 重启后据此续接
                if let Some(owner) = owner.as_deref() {
                    self.save_project_layout_soon(owner, cx);
                    cx.notify();
                }
                // 分支自记账:这个 pane 是 fork 出来的话,新身份到手即落边。
                // **必须在这里**而不是等 pane 变 ai-working —— 身份只上报一次,
                // 错过就再没有第二次机会把 child→parent 记下来。
                self.consume_pending_fork(identity.pty_id, &session, cx);
                None
            }
        }
    }
    fn notify_prefs(&self) -> NotifyPrefs {
        NotifyPrefs {
            sound: self.config.ai_completion_sound,
            flash: self.config.ai_completion_taskbar_flash,
            popup: self.config.ai_completion_popup,
            attention_notify: self.config.ai_attention_notify,
        }
    }

    // === 通知 / 待办 ===

    /// 主窗口聚焦状态(旧版 `setWindowFocused`)。聚焦时完成的任务不计未读。
    ///
    /// **聚焦即已读**:旧版 `App.tsx` 的 `onFocusChanged` 里 `focused` 一到就
    /// `clearUnreadDone()` —— 人已经回到窗口前了,绿灯必须熄,否则它会一直亮到
    /// 下次手动点掉为止。少了这一句「未读完成」就成了只增不减的计数。
    pub fn set_window_focused(&mut self, focused: bool, cx: &mut Context<Self>) {
        if self.window_focused == focused {
            return;
        }
        self.window_focused = focused;
        if focused {
            self.done.clear_unread();
        }
        cx.notify();
    }

    /// 主窗口是否聚焦。托盘的闪烁策略要看它(聚焦不闪),而托盘的推送发生在
    /// store 观察者里、手上没有 `Window`,只能从这里读。
    pub fn window_focused(&self) -> bool {
        self.window_focused
    }

    /// 未读完成数(旧版托盘绿灯的计数,这里给壳内徽章用)。
    pub fn unread_done_count(&self) -> usize {
        self.done.unread_count()
    }

    /// 全局 AI 状态(边条上那颗徽标点)。逐条对照 `ActivityBar.tsx` 的 `globalStatus`:
    /// 取所有项目里优先级最高的一档,**`error` 先压成 `idle`** —— 某个 shell
    /// `exit 1` 不该让整条边栏亮红点,那会盖住真正在跑的 AI。
    pub fn global_ai_status(&self) -> PaneStatus {
        let mut highest = PaneStatus::Idle;
        for state in self.project_states.values() {
            let status = match state.status {
                PaneStatus::Error => PaneStatus::Idle,
                other => other,
            };
            if status.priority() > highest.priority() {
                highest = status;
            }
        }
        highest
    }

    /// 全部(或某个项目的)pane 的一份只读快照。
    ///
    /// 三处聚合(挑待办 / 按项目聚合 / 标题栏状态灯)都从这一份出发,免得各写
    /// 一遍「跳过还没有 layout 的项目」这类边角。
    ///
    /// ⚠️ **顺序不确定**:`project_states` 是 `HashMap`,遍历顺序每次都可能不同。
    /// 消费方要么与顺序无关(取最高档),要么自己排序(见 [`collect_ai_projects`])。
    fn pane_refs(&self, only_project: Option<&str>) -> Vec<PaneRef<'_>> {
        self.project_states
            .iter()
            .filter(|(pid, _)| only_project.is_none_or(|only| only == pid.as_str()))
            .flat_map(|(pid, state)| {
                state.all_panes().into_iter().map(move |p| PaneRef {
                    project_id: pid.as_str(),
                    pane_id: p.id.as_str(),
                    status: p.status,
                    attention: p.attention,
                })
            })
            .collect()
    }

    /// 「进入 AI agent 的项目」按项目聚合(`store.ts::collectAiProjects` 等价物)。
    ///
    /// 标题栏的项目切换胶囊与托盘菜单(T 批)共用这一份,唯一的差别是 done 判据
    /// 从哪来 —— 见 [`DoneScope`]。
    pub fn ai_projects(&self, scope: DoneScope) -> AiProjects {
        self.ai_projects_of(&self.pane_refs(None), scope)
    }

    /// [`Self::ai_projects`] 的「pane 快照已经在手上」版本。
    ///
    /// 拆出来只为一件事:标题栏那一帧要把**同一份**快照喂给两个聚合器
    /// (见 [`Self::title_bar_snapshot`]),不该为此扫两遍全部 pane。
    fn ai_projects_of(&self, panes: &[PaneRef<'_>], scope: DoneScope) -> AiProjects {
        let projects = self.config.projects.as_slice();
        match scope {
            DoneScope::All => {
                let order = self.done.order();
                collect_ai_projects(panes.iter().copied(), projects, |id| order.contains_key(id))
            }
            DoneScope::Unread => collect_ai_projects(panes.iter().copied(), projects, |id| {
                self.done.is_unread(id)
            }),
        }
    }

    /// 标题栏一帧要的两件事:那颗全局状态灯(`TitleBar.tsx::computeLight`)+
    /// 项目切换胶囊的下拉列表。
    ///
    /// ⚠️ 状态灯与边条徽标的 [`AppStore::global_ai_status`] **口径不同**:边条把
    /// `error` 压成 `idle`(一个 `exit 1` 的 shell 不该盖住真在跑的 AI),标题栏灯
    /// 反过来把 `error` 列为最高一档,另外还多一个 `done` 档。两处不可互相复用。
    ///
    /// # 为什么合成一个方法
    ///
    /// 拆成两个 getter 就要各扫一遍 `pane_refs(None)`(全项目 flat_map + collect
    /// 一个 Vec),而标题栏**每帧都要**:它挂了 `window_control_area`,套不了
    /// view 级缓存(理由见 `main.rs::cached_panel` 与标题栏挂载点的注释),
    /// 所以那两遍是真的每帧各来一次。
    ///
    /// 两条结果的 done 判据都取 [`DoneScope::All`](`aiDoneOrder`,不看窗口焦点),
    /// 与标题栏那两处消费点原本的口径逐字一致 —— 托盘用的是
    /// [`DoneScope::Unread`],**不能**并进来。
    ///
    /// # 为什么不做脏标记缓存
    ///
    /// 评估后判为不划算:两条结果的输入横跨 `project_states`(30 处 `&mut`
    /// 触点)、`config.projects`(22 处)与 `done` 账本(11 处),没有任何一个
    /// 收口函数覆盖得住全部失效点。漏一处的后果是**状态灯从此不更新**,
    /// 比多扫一遍 pane 严重得多。合成一次遍历省下的,正好是能确定省下的那一半。
    pub fn title_bar_snapshot(&self) -> (TitleBarLight, AiProjects) {
        let panes = self.pane_refs(None);
        let order = self.done.order();
        let light = compute_title_bar_light(panes.iter().copied(), |id| order.contains_key(id));
        (light, self.ai_projects_of(&panes, DoneScope::All))
    }

    pub fn is_pane_unread_done(&self, pane_id: &str) -> bool {
        self.done.is_unread(pane_id)
    }

    pub fn clear_unread_done(&mut self, cx: &mut Context<Self>) {
        self.done.clear_unread();
        cx.notify();
    }

    /// 「下一件该我做的事」在哪个 pane。`only_project` 限定项目内挑。
    pub fn next_attention_target(&self, only_project: Option<&str>) -> Option<(String, String)> {
        crate::notify::pick_attention_target(self.pane_refs(only_project), self.done.order())
    }

    /// 按 `session_id` 跨**全部项目**找「在跑」的 pane。对应
    /// `src/utils/sessionJump.ts::findLiveSessionPane`。
    ///
    /// Rich routes require the exact live provider/session owner. Legacy-only
    /// panes retain the four-state fallback; historical pane metadata is not
    /// enough to identify a replacement process with unknown activity.
    pub fn find_live_session_pane(&self, session_id: &str) -> Option<(String, String, PaneStatus)> {
        let mut exact = self.agent_target_views().into_iter().filter(|target| {
            target.provider_session_id.as_deref() == Some(session_id)
                && self.project_states.get(&target.project_id)
                    .and_then(|state| state.pane(&target.pane_id))
                    .is_some_and(|pane| self.pane_has_live_agent(&target.project_id, pane))
                && !target.activity.is_ended()
        });
        if let Some(target) = exact.next() {
            if exact.next().is_some() {
                return None;
            }
            let status = self.project_states.get(&target.project_id)?.pane(&target.pane_id)?.status;
            return Some((target.project_id, target.pane_id, status));
        }
        let mut found = None;
        for (project_id, state) in self.project_states.iter() {
            for pane in state.all_panes() {
                if !pane.ai_session.as_ref().is_some_and(|s| s.session_id == session_id) {
                    continue;
                }
                if !self.pane_has_live_agent(project_id, pane) {
                    continue;
                }
                let has_rich_route = self.current_agent_route_for_pane(project_id, pane)
                    .is_some_and(|route| self.agent_runtime.runs().any(|run| &run.route == route));
                if !has_rich_route {
                    if found.is_some() {
                        return None;
                    }
                    found = Some((project_id.clone(), pane.id.clone(), pane.status));
                }
            }
        }
        found
    }

    /// 把恢复出来的会话身份**当场**写回 pane(对应 `setPaneAiSessionByPty`)。
    ///
    /// 不能干等 hook:codex resume 不会重新上报 SessionStart,新 pane 会永远
    /// 拿不到身份,右键的分支入口随之消失(claude 会上报同 id 幂等覆盖)。
    /// 身份随布局持久化,重启自动续接顺带受益。
    pub fn set_pane_ai_session(
        &mut self,
        project_id: &str,
        pane_id: &str,
        session: AiSessionRef,
        cx: &mut Context<Self>,
    ) {
        let mut pty_id = None;
        if let Some(state) = self.project_states.get_mut(project_id)
            && let Some(pane) = state.pane_mut(pane_id)
        {
            pane.ai_session = Some(session.clone());
            // 身份是自己写进去的,不是「待续接」——别让下次启动再敲一遍命令
            pane.resume_pending = false;
            pty_id = pane.pty_id;
            self.save_project_layout_soon(project_id, cx);
            cx.notify();
        }
        // 与 hook 上报那条路同一个消费点(原版两条都走 `setPaneAiSessionByPty`)。
        // 走到这里的多半是 resume/跳转,没有登记 → 空操作。
        if let Some(pty_id) = pty_id {
            self.consume_pending_fork(pty_id, &session, cx);
        }
    }

    // === 会话分支自记账 ===
    //
    // 设计: `docs/plans/2026-08-14-session-branch-tree-design.md`。
    // mini-term 自己发起的 fork 在新 pane 的 PTY 上登记「等新会话身份」,hook 上报
    // 新 id 时落成 child→parent 边写进 `config.session_lineage`。磁盘扫描
    // (`scan_session_lineage`)是权威且合并时优先,这里只兜两件事:文件尚未落盘的
    // 窗口期,以及 **Claude 的 CLI fork 压根不写磁盘指针**(`forkedFrom` 只有
    // `/branch` 路径写)——那种边只存在于自记账。

    /// 登记一次 fork:`pty_id` 上跑起来的下一个会话身份是 `parent_session_id` 的孩子。
    pub fn register_pending_fork(&mut self, pty_id: u32, agent: &str, parent_session_id: &str) {
        self.pending_forks.insert(
            pty_id,
            PendingFork {
                agent: agent.to_ascii_lowercase(),
                parent_session_id: parent_session_id.to_string(),
            },
        );
    }

    /// 丢掉一个 PTY 的登记(子进程退出 / 终端回收)。
    ///
    /// 不清的话:fork 命令没起成会话,这条登记会一直挂着,等 pty id 被复用之后
    /// 认领**下一个进程**的会话身份,凭空造出一条假分支边(原版 `clearPendingFork`
    /// 挂在 `pty-exit` 上是同一条理由)。
    pub fn clear_pending_fork(&mut self, pty_id: u32) {
        self.pending_forks.remove(&pty_id);
    }

    /// 消费**一次性**的 fork 登记。判据是纯函数 [`resolve_fork_edge`];
    /// 无论落不落边,登记都当场作废(agent 不符 = fork 失败后起了别家)。
    fn consume_pending_fork(
        &mut self,
        pty_id: u32,
        session: &AiSessionRef,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending_forks.remove(&pty_id) else {
            return;
        };
        let Some(edge) = resolve_fork_edge(&pending, session) else {
            return;
        };
        if push_lineage_edge(&mut self.config.session_lineage, edge) {
            self.save_config_soon(cx);
        }
    }
}

#[cfg(test)]
mod route_tests {
    use super::*;
    use mt_identity::{
        ExecutionHostId, HostInstallId, PaneKey, RepoId, TabId, TerminalIncarnationId,
        TerminalSessionId, WorktreeId,
    };

    fn route() -> TerminalRoute {
        let host = ExecutionHostId::derive("local", &HostInstallId::new());
        let repo = RepoId::derive(&host, "/repo/.git");
        TerminalRoute {
            execution_host_id: host,
            worktree_id: WorktreeId::derive(&repo, "/repo", None),
            tab_id: TabId::new(),
            pane_key: PaneKey::new(),
            terminal_session_id: TerminalSessionId::new(),
            terminal_incarnation_id: TerminalIncarnationId::new(),
        }
    }

    #[test]
    fn captured_route_rejects_reused_pty_and_missing_identity() {
        let captured = route();
        let mut current = captured.clone();
        assert!(captured_route_matches(
            Some(&captured),
            Some(&current),
            false
        ));
        assert!(!captured_route_matches(
            Some(&captured),
            Some(&current),
            true
        ));
        current.terminal_incarnation_id = TerminalIncarnationId::new();
        assert!(!captured_route_matches(
            Some(&captured),
            Some(&current),
            false
        ));
        assert!(!captured_route_matches(Some(&captured), None, false));
        assert!(!captured_route_matches(None, Some(&current), false));
        assert!(captured_route_matches(None, None, false));
        assert!(!captured_route_matches(None, None, true));
    }

    fn status_observation(
        route: TerminalRoute,
        sequence: u64,
        activity: AgentActivity,
        evidence: AgentEvidence,
    ) -> AgentObservation {
        AgentObservation {
            event_id: AgentEventId::new(),
            route,
            sequence,
            connection_epoch: Some(1),
            weak_episode: None,
            provider: AgentProvider::CODEX.parse().unwrap(),
            provider_session_id: None,
            process: None,
            activity,
            connectivity: AgentConnectivity::Live,
            confirmation: AgentConfirmation::LiveConfirmed,
            evidence,
            received_at_unix_ms: sequence as i64,
        }
    }

    fn status_change(status: &str) -> mt_ai::StatusChange {
        mt_ai::StatusChange {
            pty_id: 7,
            status: status.into(),
            cause: None,
            agent: Some("codex".into()),
            weak_episode: None,
            hook_session: None,
        }
    }

    fn source_hook(perception: &mt_ai::AiPerception, event: &str, sid: Option<&str>) {
        let payload = serde_json::from_value(serde_json::json!({
            "pty_id": 7, "event": event, "agent": "codex", "session_id": sid,
        })).unwrap();
        mt_ai::hook_server::handle_hook_payload(
            perception.hooks(), perception.emitter(), perception.tracker(), payload,
        );
    }

    fn apply_source_channel(
        registry: &mut AgentRuntimeRegistry,
        receiver: &mut futures::channel::mpsc::UnboundedReceiver<AiEvent>,
    ) -> Vec<(mt_ai::StatusChange, AgentApplyOutcome)> {
        let mut statuses = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            let outcome = apply_source_event(registry, &event, None);
            match event {
                AiEvent::Status { change, .. } => {
                    statuses.push((change, outcome));
                }
                AiEvent::Session { .. } => {
                    assert!(matches!(outcome, AgentApplyOutcome::Applied { .. }));
                }
            }
        }
        statuses
    }

    fn apply_source_event(
        registry: &mut AgentRuntimeRegistry,
        event: &AiEvent,
        epoch: Option<u64>,
    ) -> AgentApplyOutcome {
        match event {
            AiEvent::Status { change, route, event_id, sequence } => record_runtime_status_change(
                registry, route.clone().unwrap(), event_id.clone(), *sequence, epoch,
                *sequence as i64, change,
            ).expect("producer status is supported"),
            AiEvent::Session { identity, route, event_id, sequence } => record_runtime_session_identity(
                registry, route.clone().unwrap(), event_id.clone(), *sequence, epoch,
                *sequence as i64, identity,
            ),
        }
    }

    fn source_weak_input(perception: &mt_ai::AiPerception) {
        perception.emitter().emit_if_changed_with_episode(
            7, "ai-idle", None, perception.tracker().ai_session_agent(7),
            perception.tracker().weak_detection_episode(7),
        );
    }

    #[test]
    fn producer_bridge_same_id_resume_creates_new_run_for_sole_and_nonlast_sessions() {
        for sibling in [false, true] {
            for detected in [false, true] {
                let route = route();
                let (perception, mut receiver) = crate::ai::hook_channel_fixture(7, route.clone());
                let mut registry = AgentRuntimeRegistry::default();
                if detected {
                    perception.observe_input(7, b"codex\r");
                    source_weak_input(&perception);
                    apply_source_channel(&mut registry, &mut receiver);
                }
                if sibling {
                    source_hook(&perception, "SessionStart", Some("sibling"));
                    source_hook(&perception, "UserPromptSubmit", Some("sibling"));
                    apply_source_channel(&mut registry, &mut receiver);
                }
                source_hook(&perception, "SessionStart", Some("resumed"));
                source_hook(&perception, "UserPromptSubmit", Some("resumed"));
                apply_source_channel(&mut registry, &mut receiver);
                let old = registry.runs().find(|run| run.provider_session_id.as_deref() == Some("resumed"))
                    .unwrap().run_id.clone();
                let old_identity = perception.hooks().session_of(7).unwrap();
                source_hook(&perception, "SessionEnd", Some("resumed"));
                apply_source_channel(&mut registry, &mut receiver);
                let ended = registry.run(&old).unwrap().clone();
                assert_eq!(ended.activity, AgentActivity::Exited);
                let sibling_before = registry.runs().find(|run| run.provider_session_id.as_deref() == Some("sibling")).cloned();
                if sibling {
                    assert_eq!(perception.hooks().session_of(7), Some(old_identity.clone()));
                }
                source_hook(&perception, "SessionStart", Some("resumed"));
                let first = receiver.try_recv().unwrap();
                let AiEvent::Session { identity, sequence: first_sequence, .. } = &first else {
                    panic!("resumed Started identity must precede its status");
                };
                let Some(mt_ai::HookLifecycleEvent::Started(lifecycle_id)) = identity.hook_lifecycle else {
                    panic!("explicit resumed start must carry source authority");
                };
                assert_ne!(Some(lifecycle_id), old_identity.lifecycle_id);
                assert_eq!(identity.session_id, "resumed");
                let AgentApplyOutcome::Applied { run_id: resumed, created: true } = apply_source_event(&mut registry, &first, None) else {
                    panic!("same-ID resume must create a new run");
                };
                assert_ne!(resumed, old);
                let status = receiver.try_recv().unwrap();
                let AiEvent::Status { change, sequence, .. } = &status else { panic!("missing start status"); };
                assert!(*sequence > *first_sequence);
                assert_eq!(change.hook_session.as_ref().unwrap().lifecycle_id, Some(lifecycle_id));
                assert_eq!(apply_source_event(&mut registry, &status, None), AgentApplyOutcome::Applied {
                    run_id: resumed.clone(), created: false,
                });
                let before_duplicate = registry.run(&resumed).unwrap().clone();
                source_hook(&perception, "SessionStart", Some("resumed"));
                assert!(receiver.try_recv().is_err(), "duplicate active start must reuse dedup and identity");
                assert_eq!(registry.run(&resumed), Some(&before_duplicate));
                source_hook(&perception, "UserPromptSubmit", Some("resumed"));
                apply_source_channel(&mut registry, &mut receiver);
                assert_eq!(registry.run(&resumed).unwrap().activity, AgentActivity::Working);
                let working = registry.run(&resumed).unwrap().clone();
                source_hook(&perception, "SessionStart", Some("resumed"));
                assert!(receiver.try_recv().is_err());
                assert_eq!(registry.run(&resumed), Some(&working));
                source_hook(&perception, "Stop", Some("resumed"));
                apply_source_channel(&mut registry, &mut receiver);
                assert_eq!(registry.run(&resumed).unwrap().activity, AgentActivity::Done);
                assert_eq!(live_agent_runs_for_route(&registry, &route).count(), if sibling { 2 } else { 1 });
                source_hook(&perception, "SessionEnd", Some("resumed"));
                apply_source_channel(&mut registry, &mut receiver);
                assert_eq!(registry.run(&resumed).unwrap().activity, AgentActivity::Exited);
                assert_eq!(registry.run(&old), Some(&ended));
                assert_eq!(live_agent_runs_for_route(&registry, &route).count(), usize::from(sibling));
                if let Some(sibling) = sibling_before {
                    assert_eq!(registry.run(&sibling.run_id), Some(&sibling));
                }
            }
        }
    }

    #[test]
    fn producer_bridge_queued_old_lifecycle_events_cannot_mutate_a_resumed_run() {
        let route = route();
        let (perception, mut receiver) = crate::ai::hook_channel_fixture(7, route.clone());
        let mut registry = AgentRuntimeRegistry::default();
        source_hook(&perception, "SessionStart", Some("same"));
        apply_source_channel(&mut registry, &mut receiver);
        source_hook(&perception, "SessionStart", Some("sibling"));
        apply_source_channel(&mut registry, &mut receiver);
        source_hook(&perception, "UserPromptSubmit", Some("same"));
        let queued_identity = receiver.try_recv().unwrap();
        assert!(matches!(queued_identity, AiEvent::Session { .. }));
        let queued_work = receiver.try_recv().unwrap();
        source_hook(&perception, "SessionEnd", Some("same"));
        let ended = receiver.try_recv().unwrap();
        assert!(matches!(apply_source_event(&mut registry, &ended, None), AgentApplyOutcome::Applied { .. }));
        source_hook(&perception, "SessionStart", Some("same"));
        source_hook(&perception, "UserPromptSubmit", Some("same"));
        apply_source_channel(&mut registry, &mut receiver);
        let before: Vec<_> = registry.runs().cloned().collect();
        for event in [&queued_identity, &queued_work] {
            assert_eq!(apply_source_event(&mut registry, event, Some(99)),
                AgentApplyOutcome::Ignored(mt_ai::AgentObservationIgnored::EndedRun));
        }
        assert_eq!(apply_source_event(&mut registry, &ended, Some(99)),
            AgentApplyOutcome::Ignored(mt_ai::AgentObservationIgnored::DuplicateEvent));
        // Keep the actual producer's old end identity while testing a fresh
        // delivery envelope: ownership must reject it independently of replay.
        let AiEvent::Status { change, .. } = &ended else { panic!("missing exact end"); };
        let outcome = record_runtime_status_change(&mut registry, route.clone(), AgentEventId::new(),
            99, Some(99), 99, change).unwrap();
        assert_eq!(outcome, AgentApplyOutcome::Ignored(mt_ai::AgentObservationIgnored::EndedRun));
        assert!(project_status_observation(&registry, Some(&route), Some(outcome), change, true, true).is_none());
        for run in before { assert_eq!(registry.run(&run.run_id), Some(&run)); }
        source_hook(&perception, "Stop", Some("same"));
        let current = apply_source_channel(&mut registry, &mut receiver);
        assert!(matches!(current[0].1, AgentApplyOutcome::Applied { .. }), "rejected epoch must not fence current events");
        assert_eq!(live_agent_runs_for_route(&registry, &route)
            .find(|run| run.provider_session_id.as_deref() == Some("same")).unwrap().activity, AgentActivity::Done);
    }

    #[test]
    fn producer_bridge_rejected_start_does_not_bind_or_consume_its_event() {
        let route = route();
        let (perception, mut receiver) = crate::ai::hook_channel_fixture(7, route.clone());
        let mut registry = AgentRuntimeRegistry::default();
        source_hook(&perception, "SessionStart", Some("same"));
        apply_source_channel(&mut registry, &mut receiver);
        source_hook(&perception, "SessionEnd", Some("same"));
        let queued_end = receiver.try_recv().unwrap();
        source_hook(&perception, "SessionStart", Some("same"));
        let early_start = receiver.try_recv().unwrap();
        let early_status = receiver.try_recv().unwrap();
        let before: Vec<_> = registry.runs().cloned().collect();
        for event in [&early_start, &early_status] {
            assert_eq!(apply_source_event(&mut registry, event, Some(99)),
                AgentApplyOutcome::Ignored(mt_ai::AgentObservationIgnored::UnresolvedHookOwner));
        }
        for run in before { assert_eq!(registry.run(&run.run_id), Some(&run)); }
        assert!(matches!(apply_source_event(&mut registry, &queued_end, None), AgentApplyOutcome::Applied { .. }));
        assert!(matches!(apply_source_event(&mut registry, &early_start, None), AgentApplyOutcome::Applied { created: true, .. }));
        assert!(matches!(apply_source_event(&mut registry, &early_status, None), AgentApplyOutcome::Applied { created: false, .. }));
        assert_eq!(live_agent_runs_for_route(&registry, &route).count(), 1);
    }

    #[test]
    fn producer_bridge_first_explicit_start_after_mid_session_recognition_can_resume() {
        let route = route();
        let (perception, mut receiver) = crate::ai::hook_channel_fixture(7, route.clone());
        let mut registry = AgentRuntimeRegistry::default();
        source_hook(&perception, "SessionStart", Some("same"));
        source_hook(&perception, "SessionEnd", Some("same"));
        apply_source_channel(&mut registry, &mut receiver);
        let ended = registry.runs().next().unwrap().clone();
        // Real later source lifecycles evict the bounded sid tombstone. A
        // subsequent mid-session event still has no rich restart authority.
        for index in 0..8 {
            let sid = format!("intervening-{index}");
            source_hook(&perception, "SessionStart", Some(&sid));
            source_hook(&perception, "SessionEnd", Some(&sid));
            apply_source_channel(&mut registry, &mut receiver);
        }
        assert!(!perception.hooks().is_session_ended(7, "same"));
        perception.observe_input(7, b"launcher\r");
        source_hook(&perception, "UserPromptSubmit", Some("same"));
        let observed = receiver.try_recv().unwrap();
        let AiEvent::Session { identity, .. } = &observed else { panic!("missing mid-session identity"); };
        let Some(mt_ai::HookLifecycleEvent::Observed(lifecycle_id)) = identity.hook_lifecycle else {
            panic!("mid-session recognition is not Started");
        };
        assert_eq!(identity.weak_episode, None);
        let status = receiver.try_recv().unwrap();
        for event in [&observed, &status] {
            assert_eq!(apply_source_event(&mut registry, event, None),
                AgentApplyOutcome::Ignored(mt_ai::AgentObservationIgnored::EndedRun));
        }
        assert_eq!(live_agent_runs_for_route(&registry, &route).count(), 0);
        perception.observe_output(7, b"PS D:\\project> claude\r\n");
        let later_episode = perception.tracker().weak_detection_episode(7);
        assert!(later_episode.is_some());
        source_hook(&perception, "SessionStart", Some("same"));
        let started = receiver.try_recv().unwrap();
        let AiEvent::Session { identity, .. } = &started else { panic!("first explicit start must emit identity"); };
        assert_eq!(identity.hook_lifecycle, Some(mt_ai::HookLifecycleEvent::Started(lifecycle_id)));
        assert_eq!(identity.weak_episode, None, "start authority cannot recapture the receipt");
        assert!(matches!(apply_source_event(&mut registry, &started, None), AgentApplyOutcome::Applied { created: true, .. }));
        apply_source_channel(&mut registry, &mut receiver);
        source_hook(&perception, "SessionStart", Some("same"));
        assert!(receiver.try_recv().is_err());
        assert_eq!(registry.run(&ended.run_id), Some(&ended));
        assert_eq!(live_agent_runs_for_route(&registry, &route).count(), 1);
        source_hook(&perception, "SessionEnd", Some("same"));
        apply_source_channel(&mut registry, &mut receiver);
        assert_eq!(perception.tracker().weak_detection_episode(7), later_episode);
        assert_eq!(perception.tracker().ai_session_agent(7).as_deref(), Some("claude"));
    }

    #[test]
    fn producer_bridge_resumed_hook_never_borrows_later_provider_input() {
        for pending in [false, true] {
            let route = route();
            let (perception, mut receiver) = crate::ai::hook_channel_fixture(7, route.clone());
            let mut registry = AgentRuntimeRegistry::default();
            perception.observe_input(7, b"codex\r");
            let original = perception.tracker().weak_detection_episode(7);
            source_weak_input(&perception);
            apply_source_channel(&mut registry, &mut receiver);
            let alias = registry.runs().next().unwrap().run_id.clone();
            source_hook(&perception, "SessionStart", Some("same"));
            apply_source_channel(&mut registry, &mut receiver);
            perception.observe_input(7, b"\x04");
            perception.observe_input(7, if pending { b"launcher\r" } else { b"claude\r" });
            let newer_run = if pending {
                None
            } else {
                source_weak_input(&perception);
                apply_source_channel(&mut registry, &mut receiver);
                registry.runs().find(|run| run.provider.as_str() == "claude").cloned()
            };
            for event in ["UserPromptSubmit", "SessionEnd", "SessionStart", "UserPromptSubmit", "SessionEnd"] {
                source_hook(&perception, event, Some("same"));
                let emitted = apply_source_channel(&mut registry, &mut receiver);
                assert!(emitted.iter().all(|(_, result)| matches!(result, AgentApplyOutcome::Applied { .. })));
                assert_eq!(perception.tracker().is_ai_session(7), !pending);
                if let Some(newer) = &newer_run {
                    assert_eq!(registry.run(&newer.run_id), Some(newer));
                    assert!(!registry.is_superseded_weak_alias(&newer.run_id));
                }
            }
            perception.observe_output(7, b"PS D:\\project> claude\r\n");
            let newer = perception.tracker().weak_detection_episode(7);
            assert!(newer > original);
            assert_eq!(perception.tracker().ai_session_agent(7).as_deref(), Some("claude"));
            source_weak_input(&perception);
            apply_source_channel(&mut registry, &mut receiver);
            assert!(registry.is_superseded_weak_alias(&alias));
            let live = live_agent_runs_for_route(&registry, &route).collect::<Vec<_>>();
            assert_eq!(live.len(), 1);
            assert_eq!(live[0].provider.as_str(), "claude");
            assert_eq!(live[0].weak_episode, newer);
            assert_eq!(live[0].activity, AgentActivity::Unknown);
        }
    }

    #[test]
    fn producer_bridge_exact_nonlast_and_last_hook_exits_preserve_later_input() {
        for pending in [false, true] {
            for inner_first in [false, true] {
                let route = route();
                let (perception, mut receiver) = crate::ai::hook_channel_fixture(7, route.clone());
                let mut registry = AgentRuntimeRegistry::default();
                perception.observe_input(7, b"codex\r");
                let original = perception.tracker().weak_detection_episode(7);
                source_weak_input(&perception);
                apply_source_channel(&mut registry, &mut receiver);
                let alias = registry.runs().next().unwrap().run_id.clone();
                for sid in ["outer", "inner"] {
                    source_hook(&perception, "SessionStart", Some(sid));
                    source_hook(&perception, "UserPromptSubmit", Some(sid));
                    let emitted = apply_source_channel(&mut registry, &mut receiver);
                    assert_eq!(emitted.len(), 2, "same status/cause must not dedup a different Hook sid");
                    assert!(emitted.iter().all(|(_, outcome)| matches!(outcome, AgentApplyOutcome::Applied { .. })));
                }
                assert!(registry.is_superseded_weak_alias(&alias));
                assert_eq!(registry.runs().filter(|run| run.evidence == AgentEvidence::Hook
                    && run.activity == AgentActivity::Working).count(), 2);
                perception.observe_input(7, b"\x04");
                perception.observe_input(7, if pending { b"launcher\r" } else { b"claude\r" });
                let newer = if pending {
                    None
                } else {
                    source_weak_input(&perception);
                    apply_source_channel(&mut registry, &mut receiver);
                    Some(registry.runs().find(|run| run.provider.as_str() == "claude").unwrap().clone())
                };
                let endings = if inner_first { ["inner", "outer"] } else { ["outer", "inner"] };
                for (index, sid) in endings.into_iter().enumerate() {
                    // Same-sid repeated status after B must retain its source receipt.
                    source_hook(&perception, "UserPromptSubmit", Some(sid));
                    let repeated = apply_source_channel(&mut registry, &mut receiver);
                    assert!(repeated.iter().all(|(change, _)| change.weak_episode == if sid == "outer" { original } else { None }));
                    source_hook(&perception, "SessionEnd", Some(sid));
                    let ended = apply_source_channel(&mut registry, &mut receiver);
                    assert_eq!(ended.len(), 1);
                    let (change, outcome) = &ended[0];
                    assert!(matches!(outcome, AgentApplyOutcome::Applied { .. }));
                    assert_eq!(change.hook_session.as_ref().unwrap().session_id, sid);
                    assert_eq!(change.weak_episode, if sid == "outer" { original } else { None });
                    assert_eq!(registry.runs().find(|run| run.provider_session_id.as_deref() == Some(sid)).unwrap().activity, AgentActivity::Exited);
                    assert_eq!(registry.runs().filter(|run| run.evidence == AgentEvidence::Hook && !run.activity.is_ended()).count(), 1 - index);
                    assert_eq!(perception.hooks().is_hook_enabled(7), index == 0);
                    if let Some(newer) = &newer {
                        assert_eq!(registry.run(&newer.run_id), Some(newer));
                        assert!(!registry.is_superseded_weak_alias(&newer.run_id));
                        assert_eq!(perception.tracker().ai_session_agent(7).as_deref(), Some("claude"));
                    } else {
                        assert!(!perception.tracker().is_ai_session(7));
                    }
                }
                perception.observe_output(7, b"PS D:\\project> claude\r\n");
                assert_eq!(perception.tracker().ai_session_agent(7).as_deref(), Some("claude"));
                assert!(perception.tracker().weak_detection_episode(7) > original);
                source_weak_input(&perception);
                apply_source_channel(&mut registry, &mut receiver);
                let projection = accepted_agent_projection(&registry, &route, false);
                assert!(projection.live);
                assert_eq!(projection.status, PaneStatus::Idle);
                assert_eq!(projection.provider.as_deref(), Some("claude"));
                assert_eq!(live_agent_runs_for_route(&registry, &route).count(), 1);
                assert_eq!(live_agent_runs_for_route(&registry, &route).next().unwrap().activity, AgentActivity::Unknown);
                for sid in [None, Some("never-seen"), Some("inner"), Some("outer")] {
                    source_hook(&perception, "SessionEnd", sid);
                }
                assert!(apply_source_channel(&mut registry, &mut receiver).is_empty());
                assert_eq!(live_agent_runs_for_route(&registry, &route).count(), 1);
            }
        }
    }

    #[test]
    fn exact_hook_exit_requires_existing_unique_session_before_side_effects() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let mut known = status_observation(route.clone(), 1, AgentActivity::Working, AgentEvidence::Hook);
        known.provider_session_id = Some("known".into());
        registry.observe(known);
        let before: Vec<_> = registry.runs().cloned().collect();
        let mut change = status_change("idle");
        change.cause = Some("SessionEnd".into());
        change.agent = None;
        change.hook_session = Some(mt_ai::hook_server::HookSessionId {
            agent: Some("codex".into()), session_id: "unknown".into(),
            lifecycle_id: None,
        });
        assert_eq!(record_runtime_status_change(
            &mut registry, route.clone(), AgentEventId::new(), 99, Some(99), 99, &change,
        ), Some(AgentApplyOutcome::Ignored(mt_ai::AgentObservationIgnored::UnresolvedHookOwner)));
        assert_eq!(registry.runs().count(), before.len());
        for run in before {
            assert_eq!(registry.run(&run.run_id), Some(&run));
        }
        change.hook_session.as_mut().unwrap().session_id = "known".into();
        assert!(matches!(record_runtime_status_change(
            &mut registry, route, AgentEventId::new(), 2, Some(1), 2, &change,
        ), Some(AgentApplyOutcome::Applied { .. })));
    }

    #[test]
    fn producer_bridge_all_hook_exits_allow_new_input_without_old_alias_resurrection() {
        for inner_first in [false, true] {
            let route = route();
            let (perception, mut receiver) = crate::ai::hook_channel_fixture(7, route.clone());
            let mut registry = AgentRuntimeRegistry::default();
            perception.observe_input(7, b"codex\r");
            let original = perception.tracker().weak_detection_episode(7);
            source_weak_input(&perception);
            apply_source_channel(&mut registry, &mut receiver);
            let alias = registry.runs().next().unwrap().run_id.clone();
            for sid in ["outer", "inner"] {
                source_hook(&perception, "SessionStart", Some(sid));
                source_hook(&perception, "UserPromptSubmit", Some(sid));
            }
            apply_source_channel(&mut registry, &mut receiver);
            assert!(registry.is_superseded_weak_alias(&alias));
            for sid in if inner_first { ["inner", "outer"] } else { ["outer", "inner"] } {
                source_hook(&perception, "SessionEnd", Some(sid));
                let statuses = apply_source_channel(&mut registry, &mut receiver);
                assert_eq!(statuses.len(), 1);
                assert!(matches!(statuses[0].1, AgentApplyOutcome::Applied { .. }));
            }
            assert!(!perception.tracker().is_ai_session(7));
            assert_eq!(live_agent_runs_for_route(&registry, &route).count(), 0);
            for _ in 0..3 {
                perception.emitter().emit_if_changed_with_episode(7, "idle", None, None, original);
            }
            assert!(apply_source_channel(&mut registry, &mut receiver).is_empty());
            perception.observe_input(7, b"claude\r");
            source_weak_input(&perception);
            let launched = apply_source_channel(&mut registry, &mut receiver);
            assert_eq!(launched.len(), 1);
            assert!(matches!(launched[0].1, AgentApplyOutcome::Applied { created: true, .. }));
            let newer = live_agent_runs_for_route(&registry, &route).next().unwrap().clone();
            assert_eq!(newer.activity, AgentActivity::Unknown);
            assert!(newer.weak_episode > original);
            assert_ne!(newer.run_id, alias);
            perception.emitter().emit_if_changed_with_episode(7, "ai-idle", None, Some("codex".into()), original);
            let queued = apply_source_channel(&mut registry, &mut receiver);
            assert_eq!(queued.len(), 1);
            assert_eq!(queued[0].1, AgentApplyOutcome::Ignored(mt_ai::AgentObservationIgnored::SupersededWeakEpisode));
            assert_eq!(registry.run(&newer.run_id), Some(&newer));
            assert_eq!(live_agent_runs_for_route(&registry, &route).count(), 1);
        }
    }

    #[test]
    fn providerless_hook_exit_projects_the_surviving_process_not_the_old_hook() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let mut hook = status_observation(
            route.clone(),
            1,
            AgentActivity::Blocked,
            AgentEvidence::Hook,
        );
        hook.provider_session_id = Some("hook-session".into());
        registry.observe(hook);
        let mut process = status_observation(
            route.clone(),
            2,
            AgentActivity::Working,
            AgentEvidence::ProcessAttested,
        );
        process.provider = "claude".parse().unwrap();
        process.process = mt_ai::AgentProcessIdentity::new(20, 200);
        registry.observe(process);
        let outcome = registry.observe_hook_exit(route.clone(), AgentEventId::new(), 3, Some(1), 3);
        let change = mt_ai::StatusChange {
            pty_id: 7,
            status: "idle".into(),
            cause: Some("SessionEnd".into()),
            agent: None,
            weak_episode: None,
            hook_session: None,
        };
        let projection =
            project_status_observation(&registry, Some(&route), Some(outcome), &change, true, true)
                .unwrap();
        assert_eq!(projection.status, PaneStatus::Idle);
        assert!(projection.live);
        assert_eq!(projection.provider.as_deref(), Some("claude"));
        assert!(!projection.attention);
        assert!(registry.runs().any(|run| {
            run.provider.as_str() == "codex" && run.activity == AgentActivity::Exited
        }));
    }

    #[test]
    fn unknown_or_multiple_hook_owners_have_no_projection() {
        for owner_count in [0, 2] {
            let route = route();
            let mut registry = AgentRuntimeRegistry::default();
            for pid in 1..=owner_count {
                let mut hook = status_observation(
                    route.clone(),
                    pid as u64,
                    AgentActivity::Blocked,
                    AgentEvidence::Hook,
                );
                hook.process = mt_ai::AgentProcessIdentity::new(pid, 100);
                registry.observe(hook);
            }
            let mut process = status_observation(
                route.clone(),
                3,
                AgentActivity::Working,
                AgentEvidence::ProcessAttested,
            );
            process.provider = "claude".parse().unwrap();
            process.process = mt_ai::AgentProcessIdentity::new(20, 200);
            registry.observe(process);
            let before: Vec<_> = registry.runs().cloned().collect();
            let outcome =
                registry.observe_hook_exit(route.clone(), AgentEventId::new(), 4, Some(1), 4);
            assert_eq!(
                outcome,
                AgentApplyOutcome::Ignored(mt_ai::AgentObservationIgnored::UnresolvedHookOwner)
            );
            let change = mt_ai::StatusChange {
                pty_id: 7,
                status: "idle".into(),
                cause: Some("SessionEnd".into()),
                agent: None,
                weak_episode: None,
                hook_session: None,
            };
            assert!(
                project_status_observation(
                    &registry,
                    Some(&route),
                    Some(outcome),
                    &change,
                    true,
                    true,
                )
                .is_none()
            );
            for run in before {
                assert_eq!(registry.run(&run.run_id), Some(&run));
            }
        }
    }

    #[test]
    fn delayed_pty_working_after_process_inventory_has_no_projection() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        registry
            .apply_process_inventory(mt_ai::AgentProcessInventoryObservation {
                event_id: AgentEventId::new(),
                route: route.clone(),
                sequence: 11,
                connection_epoch: 1,
                weak_episode: None,
                processes: vec![mt_ai::AgentProcessObservation {
                    provider: AgentProvider::CODEX.parse().unwrap(),
                    process: mt_ai::AgentProcessIdentity::new(10, 20).unwrap(),
                    activity: AgentActivity::Waiting,
                }],
                received_at_unix_ms: 11,
            })
            .unwrap();
        let before = accepted_agent_projection(&registry, &route, false);
        let outcome = registry.observe(status_observation(
            route.clone(),
            10,
            AgentActivity::Working,
            AgentEvidence::PtyActivity,
        ));
        assert_eq!(
            outcome,
            AgentApplyOutcome::Ignored(mt_ai::AgentObservationIgnored::OutOfOrder)
        );
        // The caller must have a projection before touching status, attention,
        // git-watcher flags, or DoneTracker.
        assert!(
            project_status_observation(
                &registry,
                Some(&route),
                Some(outcome),
                &status_change("ai-working"),
                false,
                false,
            )
            .is_none()
        );
        assert_eq!(accepted_agent_projection(&registry, &route, false), before);
        assert_eq!(before.status, PaneStatus::Idle);
        assert!(before.live);
    }

    #[test]
    fn accepted_weak_status_preserves_hook_semantics_and_attention() {
        for (activity, status, attention) in [
            (AgentActivity::Blocked, PaneStatus::AiWorking, true),
            (AgentActivity::Waiting, PaneStatus::AiIdle, true),
            (AgentActivity::Done, PaneStatus::AiIdle, false),
            (AgentActivity::Failed, PaneStatus::Error, false),
        ] {
            let route = route();
            let mut registry = AgentRuntimeRegistry::default();
            let process = mt_ai::AgentProcessIdentity::new(42, 99);
            let mut hook = status_observation(
                route.clone(),
                1,
                activity,
                AgentEvidence::Hook,
            );
            hook.process = process;
            registry.observe(hook);
            let mut weak = status_observation(
                route.clone(),
                2,
                AgentActivity::Working,
                AgentEvidence::PtyActivity,
            );
            weak.process = process;
            let outcome = registry.observe(weak);
            let projection = project_status_observation(
                &registry,
                Some(&route),
                Some(outcome),
                &status_change("ai-working"),
                attention,
                false,
            )
            .unwrap();
            assert_eq!(projection.status, status);
            assert_eq!(projection.attention, attention);
            assert_eq!(projection.evidence, AgentEvidence::Hook);
        }
    }

    #[test]
    fn ordinary_shell_and_unrouted_status_keep_legacy_fallback() {
        let registry = AgentRuntimeRegistry::default();
        let route = route();
        for current_route in [None, Some(&route)] {
            for status in ["idle", "error", "ai-working", "ai-idle"] {
                let mut change = status_change(status);
                change.agent = None;
                let projection = project_status_observation(
                    &registry,
                    current_route,
                    None,
                    &change,
                    false,
                    false,
                )
                .unwrap();
                assert_eq!(projection.status, PaneStatus::from_str(status).unwrap());
            }
        }
        let change = mt_ai::StatusChange {
            pty_id: 7,
            status: "idle".into(),
            cause: Some("SessionEnd".into()),
            agent: None,
            weak_episode: None,
            hook_session: None,
        };
        assert_eq!(
            project_status_observation(&registry, None, None, &change, true, true)
                .unwrap()
                .status,
            PaneStatus::Idle
        );
    }

    #[test]
    fn route_projection_aggregates_processes_without_arbitrary_provider() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        for (pid, provider, activity) in [
            (10, "codex", AgentActivity::Working),
            (20, "claude", AgentActivity::Waiting),
        ] {
            let mut event = status_observation(
                route.clone(),
                pid as u64,
                activity,
                AgentEvidence::ProcessAttested,
            );
            event.process = mt_ai::AgentProcessIdentity::new(pid, 100);
            event.provider = provider.parse().unwrap();
            registry.observe(event);
        }
        let projection = accepted_agent_projection(&registry, &route, false);
        assert_eq!(projection.status, PaneStatus::Idle);
        assert!(projection.live);
        assert_eq!(projection.provider, None);
        assert!(!projection.attention);

        let mut other = route.clone();
        other.terminal_incarnation_id = TerminalIncarnationId::new();
        assert_eq!(
            accepted_agent_projection(&registry, &other, false).status,
            PaneStatus::Idle
        );
    }

    #[test]
    fn route_projection_keeps_independent_process_liveness_beside_hook_state() {
        for (activity, status, attention) in [
            (AgentActivity::Done, PaneStatus::AiIdle, false),
            (AgentActivity::Waiting, PaneStatus::AiIdle, false),
            (AgentActivity::Blocked, PaneStatus::AiWorking, true),
            (AgentActivity::Failed, PaneStatus::Error, false),
        ] {
            let route = route();
            let mut registry = AgentRuntimeRegistry::default();
            registry
                .apply_process_inventory(mt_ai::AgentProcessInventoryObservation {
                    event_id: AgentEventId::new(),
                    route: route.clone(),
                    sequence: 1,
                    connection_epoch: 1,
                    weak_episode: None,
                    processes: vec![
                        mt_ai::AgentProcessObservation {
                            provider: "codex".parse().unwrap(),
                            process: mt_ai::AgentProcessIdentity::new(10, 100).unwrap(),
                            activity: AgentActivity::Working,
                        },
                        mt_ai::AgentProcessObservation {
                            provider: "claude".parse().unwrap(),
                            process: mt_ai::AgentProcessIdentity::new(20, 200).unwrap(),
                            activity: AgentActivity::Working,
                        },
                    ],
                    received_at_unix_ms: 1,
                })
                .unwrap();
            let mut hook = status_observation(route.clone(), 2, activity, AgentEvidence::Hook);
            hook.process = mt_ai::AgentProcessIdentity::new(10, 100);
            assert!(matches!(
                registry.observe(hook),
                AgentApplyOutcome::Applied { .. }
            ));
            let projection = accepted_agent_projection(&registry, &route, false);
            assert_eq!(projection.status, status);
            assert_eq!(projection.attention, attention);
            assert_eq!(projection.provider, None);
            assert_eq!(projection.evidence, AgentEvidence::Hook);
            assert_eq!(registry.runs().count(), 2);
        }
    }

    #[test]
    fn accepted_ambiguous_provider_clears_only_inferred_legacy_identity() {
        let mut pane = crate::tree::PaneState::new("shell");
        pane.pty_id = Some(7);
        pane.detected_agent = Some("codex".into());
        let session = AiSessionRef {
            agent: Some("codex".into()),
            session_id: "hook-session".into(),
            cwd: None,
        };
        pane.ai_session = Some(session.clone());
        let mut layout = SplitNode::leaf(pane);
        let projection = AgentPaneProjection {
            status: PaneStatus::AiWorking,
            live: true,
            attention: false,
            provider: None,
            evidence: AgentEvidence::ProcessAttested,
        };
        assert!(layout.update_status_by_pty(7, PaneStatus::AiWorking, false, None));
        assert_eq!(
            layout.pane_by_pty(7).unwrap().detected_agent.as_deref(),
            Some("codex")
        );
        assert!(!projection.apply_to_layout(&mut layout, 8));
        assert!(projection.apply_to_layout(&mut layout, 7));
        let pane = layout.pane_by_pty(7).unwrap();
        assert_eq!(pane.detected_agent, None);
        assert_eq!(pane.ai_session, Some(session));
        assert_eq!(pane.status, PaneStatus::AiWorking);
    }

    #[test]
    fn unknown_activity_retains_live_provider_and_session_without_legacy_work() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        registry.observe(status_observation(
            route.clone(), 1, AgentActivity::Unknown, AgentEvidence::Hook,
        ));
        let mut pane = crate::tree::PaneState::new("shell");
        pane.pty_id = Some(7);
        let session = AiSessionRef {
            agent: Some("codex".into()), session_id: "exact".into(), cwd: None,
        };
        pane.ai_session = Some(session.clone());
        let mut layout = SplitNode::leaf(pane);
        let projection = accepted_agent_projection(&registry, &route, false);
        assert!(projection.live);
        assert_eq!(projection.status, PaneStatus::Idle);
        assert!(projection.apply_to_layout(&mut layout, 7));
        let pane = layout.pane_by_pty(7).unwrap();
        assert_eq!(pane.status, PaneStatus::Idle);
        assert_eq!(pane.detected_agent.as_deref(), Some("codex"));
        assert_eq!(pane.ai_session, Some(session));
        assert!(!pane.attention);
        registry.remove_route(&route);
        let projection = accepted_agent_projection(&registry, &route, false);
        assert!(!projection.live);
        assert!(projection.apply_to_layout(&mut layout, 7));
        assert!(layout.pane_by_pty(7).unwrap().ai_session.is_none());
    }

    #[test]
    fn terminal_display_stops_stale_work_without_losing_agent_liveness() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let process = mt_ai::AgentProcessIdentity::new(42, 99).unwrap();
        let mut observation = status_observation(
            route.clone(), 1, AgentActivity::Working, AgentEvidence::ProcessAttested,
        );
        observation.process = Some(process);
        let AgentApplyOutcome::Applied { run_id, .. } = registry.observe(observation)
        else { panic!("process rejected"); };
        assert_eq!(agent_display_status(&registry, &route, 1), Some(PaneStatus::Idle));
        assert!(matches!(registry.observe_semantic(mt_ai::AgentSemanticObservation {
            event_id: AgentEventId::new(), run_id, route: route.clone(),
            provider: "codex".parse().unwrap(),
            owner: mt_ai::AgentSemanticOwner::ForegroundProcess(process),
            sequence: 2, connection_epoch: Some(1), activity: AgentActivity::Working,
            observed_at_unix_ms: 2, received_at_unix_ms: 2,
        }), AgentApplyOutcome::Applied { .. }));
        assert_eq!(agent_display_status(&registry, &route, 2), Some(PaneStatus::AiWorking));
        let expired = 3 + mt_ai::AGENT_SEMANTIC_MAX_AGE_MS;
        assert_eq!(agent_display_status(&registry, &route, expired), Some(PaneStatus::Idle));
        assert!(accepted_agent_projection(&registry, &route, false).live);

        let mut hook = status_observation(route.clone(), 3, AgentActivity::Working, AgentEvidence::Hook);
        hook.process = Some(process);
        registry.observe(hook);
        assert_eq!(agent_display_status(&registry, &route, expired), Some(PaneStatus::AiWorking));
        registry.mark_connectivity(mt_ai::AgentConnectivityObservation {
            event_id: AgentEventId::new(), route: route.clone(), sequence: 4,
            connection_epoch: Some(1), connectivity: AgentConnectivity::Disconnected,
            received_at_unix_ms: 4,
        }).unwrap();
        assert_eq!(agent_display_status(&registry, &route, expired), Some(PaneStatus::Idle));
        assert!(accepted_agent_projection(&registry, &route, false).live);
    }

    #[test]
    fn session_identity_never_borrows_newest_other_provider_or_session_activity() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let provider: AgentProvider = "codex".parse().unwrap();
        assert_eq!(activity_for_session_identity(&registry, &route, &provider, "new"),
            AgentActivity::Unknown);
        let mut owned = status_observation(route.clone(), 1, AgentActivity::Waiting, AgentEvidence::Hook);
        owned.provider_session_id = Some("owned".into());
        registry.observe(owned);
        let mut other = status_observation(route.clone(), 2, AgentActivity::Working, AgentEvidence::Hook);
        other.provider = "claude".parse().unwrap();
        other.provider_session_id = Some("newest-other".into());
        registry.observe(other);
        assert_eq!(activity_for_session_identity(&registry, &route, &provider, "owned"),
            AgentActivity::Waiting);
        assert_eq!(activity_for_session_identity(&registry, &route, &provider, "new"),
            AgentActivity::Unknown);
    }

    #[test]
    fn session_identity_does_not_promote_unbound_process_semantics_into_hook_authority() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let process = mt_ai::AgentProcessIdentity::new(42, 99).unwrap();
        let mut observed = status_observation(
            route.clone(), 1, AgentActivity::Unknown, AgentEvidence::ProcessAttested,
        );
        observed.process = Some(process);
        let AgentApplyOutcome::Applied { run_id, .. } = registry.observe(observed)
        else { panic!("process rejected"); };
        let provider: AgentProvider = "codex".parse().unwrap();
        assert!(matches!(registry.observe_semantic(mt_ai::AgentSemanticObservation {
            event_id: AgentEventId::new(), run_id: run_id.clone(), route: route.clone(),
            provider: provider.clone(), owner: mt_ai::AgentSemanticOwner::ForegroundProcess(process),
            sequence: 2, connection_epoch: Some(1), activity: AgentActivity::Working,
            observed_at_unix_ms: 2, received_at_unix_ms: 2,
        }), AgentApplyOutcome::Applied { .. }));

        let mut session = status_observation(
            route.clone(), 3,
            activity_for_session_identity(&registry, &route, &provider, "new-session"),
            AgentEvidence::Hook,
        );
        assert_eq!(session.activity, AgentActivity::Unknown);
        session.provider_session_id = Some("new-session".into());
        assert!(matches!(registry.observe(session), AgentApplyOutcome::Applied { created: true, .. }));
        assert_eq!(registry.runs().count(), 2);
        assert_eq!(registry.activity_freshness(&run_id, 20_000), mt_ai::AgentActivityFreshness::Stale);
        assert_eq!(agent_display_status(&registry, &route, 20_000), Some(PaneStatus::Idle));
    }

    #[test]
    fn session_identity_can_retain_a_unique_unbound_hook_activity() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        registry.observe(status_observation(
            route.clone(), 1, AgentActivity::Waiting, AgentEvidence::Hook,
        ));
        assert_eq!(activity_for_session_identity(&registry, &route, &"codex".parse().unwrap(), "new"),
            AgentActivity::Waiting);
    }

    #[test]
    fn unbound_hook_and_two_same_provider_processes_keep_independent_app_liveness() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        registry.observe(status_observation(
            route.clone(), 1, AgentActivity::Waiting, AgentEvidence::Hook,
        ));
        registry.apply_process_inventory(mt_ai::AgentProcessInventoryObservation {
            event_id: AgentEventId::new(), route: route.clone(), sequence: 2,
            connection_epoch: 1, received_at_unix_ms: 2,
            weak_episode: None,
            processes: [42, 43].into_iter().map(|pid| mt_ai::AgentProcessObservation {
                provider: "codex".parse().unwrap(),
                process: mt_ai::AgentProcessIdentity::new(pid, 99).unwrap(),
                activity: AgentActivity::Unknown,
            }).collect(),
        }).unwrap();
        assert_eq!(registry.runs().count(), 3);
        assert_eq!(registry.runs().filter(|run| run.process.is_some()).count(), 2);
        let projection = accepted_agent_projection(&registry, &route, false);
        assert!(projection.live);
        assert_eq!(projection.status, PaneStatus::AiIdle);
        assert_eq!(projection.provider.as_deref(), Some("codex"));
        assert!(!projection.attention);
    }

    #[test]
    fn monitor_silence_is_not_hook_evidence_and_provider_fallback_is_unambiguous() {
        for cause in [None, Some("Stall"), Some("StallExit")] {
            assert!(!is_hook_observation(cause));
        }
        for cause in ["Stop", "PermissionRequest", "SessionEnd", "Interrupt"] {
            assert!(is_hook_observation(Some(cause)));
        }
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        registry.observe(status_observation(route.clone(), 1, AgentActivity::Working, AgentEvidence::Hook));
        assert_eq!(unique_route_provider(&registry, &route).unwrap().as_str(), "codex");
        let outcome = registry.observe(status_observation(route.clone(), 2, AgentActivity::Exited, AgentEvidence::PtyActivity));
        assert_eq!(outcome, AgentApplyOutcome::Ignored(mt_ai::AgentObservationIgnored::AmbiguousRun));
        assert!(project_status_observation(
            &registry, Some(&route), Some(outcome), &status_change("idle"), false, false,
        ).is_none());
        assert_eq!(accepted_agent_projection(&registry, &route, false).status, PaneStatus::AiWorking);
        let mut other = status_observation(route.clone(), 3, AgentActivity::Waiting, AgentEvidence::Hook);
        other.provider = "claude".parse().unwrap();
        registry.observe(other);
        assert!(unique_route_provider(&registry, &route).is_none());
    }

    #[test]
    fn weak_fallback_projection_is_sticky_after_hook_end_but_not_a_later_launch() {
        for hook_first in [false, true] {
            let route = route();
            let tracker = mt_ai::SessionTracker::new();
            tracker.track_input_with_line_snapshot(7, "codex\r", None);
            let episode = tracker.weak_detection_episode(7).unwrap();
            let mut registry = AgentRuntimeRegistry::default();
            let mut weak = status_observation(route.clone(), 1, AgentActivity::Unknown, AgentEvidence::PtyActivity);
            weak.connection_epoch = None;
            weak.weak_episode = Some(episode);
            let mut hook = status_observation(route.clone(), 2, AgentActivity::Done, AgentEvidence::Hook);
            hook.connection_epoch = None;
            hook.weak_episode = Some(episode);
            hook.provider_session_id = Some("exact-session".into());
            let alias = if hook_first {
                hook.sequence = 1;
                registry.observe(hook);
                weak.sequence = 2;
                assert_eq!(registry.observe(weak.clone()), AgentApplyOutcome::Ignored(mt_ai::AgentObservationIgnored::SupersededWeakEpisode));
                None
            } else {
                let AgentApplyOutcome::Applied { run_id, .. } = registry.observe(weak.clone()) else { panic!("weak rejected"); };
                assert!(accepted_agent_projection(&registry, &route, false).live);
                assert_eq!(single_live_agent_for_route(&registry, &route).unwrap().run_id, run_id);
                registry.observe(hook);
                Some(run_id)
            };
            assert_eq!(live_agent_runs_for_route(&registry, &route).count(), 1);
            let hook_id = single_live_agent_for_route(&registry, &route).unwrap().run_id.clone();
            let projection = accepted_agent_projection(&registry, &route, false);
            assert!(projection.live, "Done is not a process exit");
            assert_eq!(projection.status, PaneStatus::AiIdle);
            assert_eq!(projection.provider.as_deref(), Some("codex"));
            assert_eq!(agent_display_status(&registry, &route, 3), Some(PaneStatus::AiIdle));
            if let Some(alias) = alias.as_ref() {
                assert!(registry.is_superseded_weak_alias(alias));
                assert!(!registry.run(alias).unwrap().activity.is_ended());
            }

            tracker.clear_ai_session(7);
            assert!(matches!(registry.observe_hook_exit(route.clone(), AgentEventId::new(), 3, None, 3), AgentApplyOutcome::Applied { .. }));
            for sequence in 4..=6 {
                weak.event_id = AgentEventId::new();
                weak.sequence = sequence;
                weak.received_at_unix_ms = sequence as i64 * 10_000;
                let outcome = registry.observe(weak.clone());
                assert_eq!(outcome, AgentApplyOutcome::Ignored(mt_ai::AgentObservationIgnored::SupersededWeakEpisode));
                assert!(project_status_observation(&registry, Some(&route), Some(outcome), &status_change("ai-idle"), false, false).is_none());
            }
            let projection = accepted_agent_projection(&registry, &route, true);
            assert!(!projection.live);
            assert!(!projection.attention);
            assert_eq!(projection.provider, None);
            assert_eq!(unique_route_provider(&registry, &route), None);
            assert_eq!(agent_display_status(&registry, &route, 60_000), None);
            assert!(single_live_agent_for_route(&registry, &route).is_none());
            let mut pane = crate::tree::PaneState::new("shell");
            pane.pty_id = Some(7);
            pane.detected_agent = Some("codex".into());
            pane.ai_session = Some(AiSessionRef { agent: Some("codex".into()), session_id: "exact-session".into(), cwd: None });
            let mut layout = SplitNode::leaf(pane);
            assert!(projection.apply_to_layout(&mut layout, 7));
            assert_eq!(layout.pane_by_pty(7).unwrap().status, PaneStatus::Idle);
            assert!(layout.pane_by_pty(7).unwrap().ai_session.is_none());
            assert!(layout.pane_by_pty(7).unwrap().detected_agent.is_none());

            tracker.track_input_with_line_snapshot(7, "codex\r", None);
            let next_episode = tracker.weak_detection_episode(7).unwrap();
            assert!(next_episode > episode);
            weak.event_id = AgentEventId::new();
            weak.sequence = 7;
            weak.weak_episode = Some(next_episode);
            let AgentApplyOutcome::Applied { run_id: next_id, created: true } = registry.observe(weak) else { panic!("later launch rejected"); };
            assert_ne!(next_id, hook_id);
            if let Some(alias) = alias {
                assert_ne!(next_id, alias);
                assert!(registry.is_superseded_weak_alias(&alias));
                assert!(!registry.run(&alias).unwrap().activity.is_ended());
            }
            assert_eq!(live_agent_runs_for_route(&registry, &route).count(), 1);
            assert_eq!(single_live_agent_for_route(&registry, &route).unwrap().run_id, next_id);
            let projection = accepted_agent_projection(&registry, &route, false);
            assert!(projection.live);
            assert_eq!(projection.status, PaneStatus::Idle);
            assert_eq!(projection.provider.as_deref(), Some("codex"));
        }
    }

    #[test]
    fn weak_supersession_never_hides_proved_same_provider_runs_or_another_route() {
        let route = route();
        let mut other_route = route.clone();
        other_route.terminal_incarnation_id = TerminalIncarnationId::new();
        let tracker = mt_ai::SessionTracker::new();
        tracker.track_input_with_line_snapshot(7, "codex\r", None);
        let episode = tracker.weak_detection_episode(7);
        let mut registry = AgentRuntimeRegistry::default();
        for route in [route.clone(), other_route.clone()] {
            let mut weak = status_observation(route, 1, AgentActivity::Unknown, AgentEvidence::PtyActivity);
            weak.weak_episode = episode;
            registry.observe(weak);
        }
        let mut hook = status_observation(route.clone(), 2, AgentActivity::Waiting, AgentEvidence::Hook);
        hook.weak_episode = episode;
        hook.provider_session_id = Some("session".into());
        registry.observe(hook);
        registry.apply_process_inventory(mt_ai::AgentProcessInventoryObservation {
            event_id: AgentEventId::new(), route: route.clone(), sequence: 3,
            connection_epoch: 1, weak_episode: episode, received_at_unix_ms: 3,
            processes: [10, 20].into_iter().map(|pid| mt_ai::AgentProcessObservation {
                provider: "codex".parse().unwrap(), process: mt_ai::AgentProcessIdentity::new(pid, 100).unwrap(),
                activity: AgentActivity::Unknown,
            }).collect(),
        }).unwrap();
        assert_eq!(registry.runs().count(), 5);
        assert_eq!(live_agent_runs_for_route(&registry, &route).count(), 3);
        assert_eq!(live_agent_runs_for_route(&registry, &other_route).count(), 1);
        assert!(single_live_agent_for_route(&registry, &route).is_none());
        assert!(registry.runs().filter(|run| run.process.is_some() || run.evidence == AgentEvidence::Hook)
            .all(|run| !registry.is_superseded_weak_alias(&run.run_id)));
        registry.observe_hook_exit(route.clone(), AgentEventId::new(), 4, Some(1), 4);
        assert_eq!(live_agent_runs_for_route(&registry, &route).count(), 2);
        assert!(accepted_agent_projection(&registry, &route, false).live);
        registry.apply_process_inventory(mt_ai::AgentProcessInventoryObservation {
            event_id: AgentEventId::new(), route: route.clone(), sequence: 5,
            connection_epoch: 1, weak_episode: episode, received_at_unix_ms: 5, processes: vec![],
        }).unwrap();
        assert!(!accepted_agent_projection(&registry, &route, false).live);
        assert_eq!(accepted_agent_projection(&registry, &route, false).provider, None);
        assert!(accepted_agent_projection(&registry, &other_route, false).live);
    }

    #[test]
    fn later_different_provider_episode_survives_older_hook_capture_and_retirement() {
        let route = route();
        let tracker = mt_ai::SessionTracker::new();
        tracker.track_input_with_line_snapshot(7, "codex\r", None);
        let first_episode = tracker.weak_detection_episode(7).unwrap();
        let mut registry = AgentRuntimeRegistry::default();
        let mut weak = status_observation(route.clone(), 1, AgentActivity::Unknown, AgentEvidence::PtyActivity);
        weak.connection_epoch = None;
        weak.weak_episode = Some(first_episode);
        let AgentApplyOutcome::Applied { run_id: old_alias, .. } = registry.observe(weak.clone()) else { panic!("first weak input rejected"); };
        let mut hook = status_observation(route.clone(), 2, AgentActivity::Working, AgentEvidence::Hook);
        hook.connection_epoch = None;
        hook.weak_episode = Some(first_episode);
        hook.provider_session_id = Some("codex-session".into());
        let AgentApplyOutcome::Applied { run_id: hook_id, .. } = registry.observe(hook.clone()) else { panic!("Hook rejected"); };
        assert!(registry.is_superseded_weak_alias(&old_alias));
        assert_eq!(live_agent_runs_for_route(&registry, &route).count(), 1);

        let mut same_episode = weak.clone();
        same_episode.event_id = AgentEventId::new();
        same_episode.sequence = 3;
        same_episode.provider = "claude".parse().unwrap();
        assert_eq!(registry.observe(same_episode), AgentApplyOutcome::Ignored(mt_ai::AgentObservationIgnored::SupersededWeakEpisode));

        tracker.track_input_with_line_snapshot(7, "\x04", None);
        tracker.track_input_with_line_snapshot(7, "claude\r", None);
        let later_episode = tracker.weak_detection_episode(7).unwrap();
        assert!(later_episode > first_episode);
        weak.event_id = AgentEventId::new();
        weak.sequence = 4;
        weak.received_at_unix_ms = 4;
        weak.weak_episode = Some(later_episode);
        weak.provider = tracker.ai_session_agent(7).unwrap().parse().unwrap();
        let AgentApplyOutcome::Applied { run_id: later_id, created: true } = registry.observe(weak) else { panic!("later detection rejected"); };
        let later = registry.run(&later_id).unwrap().clone();
        assert_eq!(later.activity, AgentActivity::Unknown);
        assert!(!registry.is_superseded_weak_alias(&later_id));
        assert_eq!(live_agent_runs_for_route(&registry, &route).count(), 2);
        assert_eq!(accepted_agent_projection(&registry, &route, false).provider, None);

        hook.event_id = AgentEventId::new();
        hook.sequence = 5;
        hook.received_at_unix_ms = 50_000;
        assert!(matches!(registry.observe(hook), AgentApplyOutcome::Applied { .. }));
        assert_eq!(registry.run(&later_id), Some(&later));
        assert!(!registry.is_superseded_weak_alias(&later_id));
        registry.observe_hook_exit(route.clone(), AgentEventId::new(), 6, None, 50_001);
        assert_eq!(registry.run(&hook_id).unwrap().activity, AgentActivity::Exited);
        assert_eq!(registry.run(&later_id), Some(&later));
        let projection = accepted_agent_projection(&registry, &route, false);
        assert!(projection.live);
        assert_eq!(projection.status, PaneStatus::Idle);
        assert_eq!(projection.provider.as_deref(), Some("claude"));
        assert_eq!(single_live_agent_for_route(&registry, &route).unwrap().run_id, later_id);

        let mut covering = status_observation(route.clone(), 7, AgentActivity::Done, AgentEvidence::Hook);
        covering.connection_epoch = None;
        covering.provider = "claude".parse().unwrap();
        covering.weak_episode = Some(later_episode);
        covering.provider_session_id = Some("claude-session".into());
        covering.received_at_unix_ms = 50_002;
        assert!(matches!(registry.observe(covering), AgentApplyOutcome::Applied { .. }));
        assert!(registry.is_superseded_weak_alias(&later_id));
        assert_eq!(registry.run(&later_id), Some(&later));
        assert_eq!(live_agent_runs_for_route(&registry, &route).count(), 1);
        assert_eq!(accepted_agent_projection(&registry, &route, false).status, PaneStatus::AiIdle);
    }
}
