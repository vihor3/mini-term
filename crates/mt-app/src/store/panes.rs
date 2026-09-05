//! Flat terminal selection, single-pane lifecycle, and PTY attach/recovery.
//! Legacy panel and tree identities remain route owners, not UI groups.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{AppContext, Context, Task, Window};
use mt_config::{AiLauncher, ProjectConfig, ShellConfig};
use mt_identity::{PaneKey, TabId, TerminalIncarnationId, TerminalSessionId};
use mt_layout::ProjectWorktreeBinding;
use mt_pty::PtySpawn;
use mt_terminal_host::ErrorCode as HostErrorCode;
use mt_ui::{DwellConfig, TerminalStyle};

use crate::pane::{HostedLaunch, PaneEvent, TerminalPane, TerminalRecovery};
use crate::tree::{AiSessionRef, PaneState, PaneStatus, ProjectPanel, SplitNode};

use super::{AppStore, ProjectState, TerminalJumpTarget};
use super::identity::TerminalRoute;
use super::pure::{
    resolve_auto_resume_command, resolve_resume_cwd, resolve_scrollback,
    terminal_style_from,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalCloseRequest {
    target: TerminalJumpTarget,
    pty_id: Option<u32>,
    binding: ProjectWorktreeBinding,
    project_path: String,
    ssh_connection_id: Option<String>,
    connection_fingerprint: Option<u64>,
    // Capture deletion authority separately from same-owner navigation changes.
    source_layout: serde_json::Value,
    // Alias snapshots also fence changes already flushed by the layout debounce.
    aliases: Vec<(ProjectWorktreeBinding, serde_json::Value)>,
}

impl TerminalCloseRequest {
    fn same_owner(&self, other: &Self) -> bool {
        self.target == other.target
            && self.pty_id == other.pty_id
            && self.binding == other.binding
            && self.project_path == other.project_path
            && self.ssh_connection_id == other.ssh_connection_id
            && self.connection_fingerprint == other.connection_fingerprint
    }

    fn matches_before_dispatch(&self, current: &Self) -> bool {
        self.same_owner(current) && self.aliases == current.aliases
    }

    fn may_replace_alias_layout(&self, dirty_owner: Option<&str>) -> bool {
        // A completed flush forgets its owner. Divergent captured aliases are
        // therefore not proof that this source owns the latest saved layout.
        dirty_owner.is_none_or(|owner| owner == self.target.project_id.as_str())
            && self.aliases.iter().all(|(_, saved)| saved == &self.source_layout)
    }
}

fn terminal_close_aliases(
    target: &TerminalJumpTarget,
    bindings: &HashMap<String, ProjectWorktreeBinding>,
    states: &HashMap<String, ProjectState>,
) -> Option<Vec<(ProjectWorktreeBinding, serde_json::Value)>> {
    let mut aliases = Vec::new();
    for alias in bindings.values().filter(|alias| {
        alias.worktree_id == target.worktree_id && alias.project_id != target.project_id
    }) {
        let saved = states.get(&alias.project_id)?.saved_layout();
        aliases.push((alias.clone(), serde_json::to_value(saved).ok()?));
    }
    aliases.sort_by(|a, b| a.0.project_id.cmp(&b.0.project_id));
    Some(aliases)
}

#[derive(Debug, PartialEq, Eq)]
enum TerminalCloseCompletion {
    Stale,
    Failed(Option<HostErrorCode>),
    Conflict,
    Remove,
}

fn terminal_close_completion(
    expected: &TerminalCloseRequest,
    current: Option<&TerminalCloseRequest>,
    alias_conflict: bool,
    result: Result<(), Option<HostErrorCode>>,
) -> TerminalCloseCompletion {
    let Some(current) = current.filter(|current| expected.same_owner(current)) else {
        return TerminalCloseCompletion::Stale;
    };
    if let Err(code) = result {
        return TerminalCloseCompletion::Failed(code);
    }
    if alias_conflict || expected.aliases != current.aliases {
        return TerminalCloseCompletion::Conflict;
    }
    TerminalCloseCompletion::Remove
}

fn close_has_other_attachment(
    request: &TerminalCloseRequest,
    routes: &HashMap<u32, TerminalRoute>,
    states: &HashMap<String, ProjectState>,
) -> bool {
    let session_id = &request.target.terminal_session_id;
    routes.iter().any(|(pty_id, route)| {
        &route.terminal_session_id == session_id && Some(*pty_id) != request.pty_id
    }) || states.iter().any(|(project_id, state)| {
        state.all_panes().iter().any(|pane| {
            &pane.terminal_session_id == session_id && pane.pty_id.is_some()
                && (project_id != &request.target.project_id || pane.pane_key != request.target.pane_key)
        })
    })
}

#[derive(Default)]
pub(super) struct PendingTerminalCloses {
    requests: HashMap<TerminalSessionId, Arc<TerminalCloseRequest>>,
}

impl PendingTerminalCloses {
    pub(super) fn contains(&self, session_id: &TerminalSessionId) -> bool {
        self.requests.contains_key(session_id)
    }

    fn begin(&mut self, request: TerminalCloseRequest) -> Option<Arc<TerminalCloseRequest>> {
        let session_id = request.target.terminal_session_id.clone();
        if self.contains(&session_id) {
            return None;
        }
        let request = Arc::new(request);
        self.requests.insert(session_id, request.clone());
        Some(request)
    }

    fn finish(&mut self, request: &Arc<TerminalCloseRequest>) -> bool {
        let session_id = &request.target.terminal_session_id;
        if !self.requests.get(session_id).is_some_and(|owner| Arc::ptr_eq(owner, request)) {
            return false;
        }
        self.requests.remove(session_id);
        true
    }
}

#[derive(Debug, PartialEq, Eq)]
enum TerminalClosePlan {
    Attached(u32),
    RecordOnly,
    Hosted(TerminalIncarnationId),
    HostUnavailable,
}

fn terminal_close_plan(request: &TerminalCloseRequest, host_available: bool) -> TerminalClosePlan {
    if let Some(pty_id) = request.pty_id {
        return TerminalClosePlan::Attached(pty_id);
    }
    if request.ssh_connection_id.is_some() {
        return TerminalClosePlan::RecordOnly;
    }
    match request.target.terminal_incarnation_id.as_ref() {
        None => TerminalClosePlan::RecordOnly,
        Some(incarnation) if host_available => TerminalClosePlan::Hosted(incarnation.clone()),
        Some(_) => TerminalClosePlan::HostUnavailable,
    }
}

fn dormant_close_error(code: Option<HostErrorCode>) -> &'static str {
    match code {
        Some(HostErrorCode::IncarnationMismatch) =>
            "Terminal identity changed. The saved terminal was kept.",
        Some(HostErrorCode::SessionMissing | HostErrorCode::RecoveryUnavailable) =>
            "Terminal history could not be safely closed. The saved terminal was kept.",
        Some(HostErrorCode::ProtocolMismatch) =>
            "Terminal host version mismatch. The saved terminal was kept.",
        _ => "Terminal close was not confirmed. The saved terminal was kept; retry when the host is available.",
    }
}

impl AppStore {
    // === 终端 ===

    /// 新建一个终端 tab。
    ///
    /// - 项目还没有布局:建根叶子;
    /// - 已有布局:加进锚点 pane 所在叶子的 tab 栏并激活(锚点缺省 = 当前焦点 pane)。
    pub fn new_terminal(
        &mut self,
        project_id: &str,
        shell: Option<ShellConfig>,
        anchor_pane_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        self.new_terminal_with_cwd(project_id, shell, anchor_pane_id, None, window, cx)
    }

    /// 新建一个终端 tab 并按 AI 启动器把 agent 拉起来。
    ///
    /// 启动器是 `{名称, shell(可选), 命令}` 的具名条目(见 [`mt_config::AiLauncher`]),
    /// 桌面端与移动端**共用同一份配置**;这里是桌面端那条触发路径。
    ///
    /// 与移动端发起会话(`mobile_relay::MobileRelayBridge::try_start_session`)的区别
    /// 只在落点与善后:那边刻意用 `append_pane_background` 不抢焦点、要回执、要弹
    /// 审计 toast;桌面端是人自己点的,照常走 [`new_terminal`] 把焦点带过去,
    /// 不回执也不弹 toast。**改这里时记得看一眼那边**,两条路径共用启动器语义。
    ///
    /// [`new_terminal`]: Self::new_terminal
    pub fn new_terminal_from_launcher(
        &mut self,
        project_id: &str,
        launcher: &AiLauncher,
        anchor_pane_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        // 绑定的 shell 被删掉时退回默认 —— 总比不开好,用户在桌面看得到实情
        // (判据与移动端那条同一个 `resolve_shell`)
        let shell = self.resolve_shell(launcher.shell.as_deref())?;
        let pane_id = self.new_terminal(project_id, Some(shell), anchor_pane_id, window, cx)?;
        self.rename_pane(project_id, &pane_id, &launcher.name, cx);
        self.write_launcher_command(project_id, &pane_id, &launcher.command, cx);
        Some(pane_id)
    }

    /// 把启动器命令连同回车写进 pane。
    ///
    /// ⚠️ 必须走 [`write_to_pane`] 而不是裸 PTY 写:AI 会话身份靠**输入检测**建立,
    /// 只有「往 shell 里敲进启动命令并回车」这条路能让 pane 进入 AI 会话状态。
    /// 把 AI CLI 当成 PTY 根程序 spawn(`shell -c "claude"`)会绕开检测,拿不到
    /// 状态徽章与对话镜像 —— 这是 ADR 0002 定下的纪律,别改。
    ///
    /// PTY 内核缓冲 stdin,shell 就绪前写入不丢(与移动端发起会话同一时序)。
    /// 写不进去时**保留 pane**:用户回头能看到它卡在哪。
    ///
    /// [`write_to_pane`]: Self::write_to_pane
    fn write_launcher_command(
        &mut self,
        project_id: &str,
        pane_id: &str,
        command: &str,
        cx: &mut Context<Self>,
    ) {
        self.write_to_pane(project_id, pane_id, &format!("{command}\r"), cx);
    }

    /// 新建终端并指定启动目录。
    ///
    /// 单独一个入口是因为 `claude --resume` 只认「启动目录」对应的会话桶 ——
    /// 子目录里起的会话在项目根恢复会报 `No conversation found`
    /// (对应 `src/utils/sessionJump.ts:90-99`)。除此之外与 [`new_terminal`] 同。
    ///
    /// [`new_terminal`]: Self::new_terminal
    pub fn new_terminal_with_cwd(
        &mut self,
        project_id: &str,
        shell: Option<ShellConfig>,
        anchor_pane_id: Option<String>,
        cwd: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let project = self.project(project_id)?.clone();
        let shell = shell.or_else(|| self.resolve_shell(None))?;
        let anchor = anchor_pane_id.or_else(|| self.active_pane_id(project_id));
        let (target_panel_id, tab_id) = {
            let state = self.project_states.get(project_id)?;
            if state.panels.is_empty() {
                (None, TabId::new())
            } else {
                let valid_anchor = anchor.as_deref().filter(|id| state.pane(id).is_some());
                let target = valid_anchor
                    .and_then(|id| state.panel_id_of_pane(id))
                    .map(str::to_string)
                    .or_else(|| state.active_panel().map(|panel| panel.id.clone()))?;
                let tab_id = state
                    .panels
                    .iter()
                    .find(|panel| panel.id == target)
                    .map(|panel| panel.tab_id.clone())?;
                (Some(target), tab_id)
            }
        };
        let pane = self.spawn_pane(&project, &shell, cwd, &tab_id, window, cx)?;
        let pane_id = pane.id.clone();

        let state = self.project_states.get_mut(project_id)?;
        if state.panels.is_empty() {
            let panel = ProjectPanel::with_tab_id(tab_id, SplitNode::leaf(pane));
            state.active_panel_id = Some(panel.id.clone());
            state.panels.push(panel);
        } else {
            let target = target_panel_id?;
            let anchor = anchor.filter(|id| state.pane(id).is_some());
            let layout = &mut state.panel_mut(&target)?.layout;
            layout.append_pane(anchor.as_deref(), pane);
        }
        state.select_terminal(&pane_id);
        self.clear_preedit_of_focused(cx);
        self.after_layout_change(project_id, cx);
        self.focus_pane(project_id, &pane_id, window, cx);
        Some(pane_id)
    }

    pub(crate) fn terminal_close_request(&self, target: &TerminalJumpTarget) -> Option<TerminalCloseRequest> {
        if self.pending_terminal_closes.contains(&target.terminal_session_id) {
            return None;
        }
        self.terminal_close_snapshot(target)
    }

    fn terminal_close_snapshot(&self, target: &TerminalJumpTarget) -> Option<TerminalCloseRequest> {
        self.resolve_terminal_jump_target(target)?;
        let project = self.project(&target.project_id)?;
        let state = self.project_states.get(&target.project_id)?;
        let pane = state.pane(target.pane_key.as_str())?;
        let binding = self.project_worktree_bindings.get(&target.project_id)?.clone();
        let aliases = terminal_close_aliases(target, &self.project_worktree_bindings, &self.project_states)?;
        Some(TerminalCloseRequest {
            target: target.clone(),
            pty_id: pane.pty_id,
            binding,
            project_path: project.path.clone(),
            ssh_connection_id: project.ssh_connection_id.clone(),
            connection_fingerprint: self.remote_connection_of(&target.project_id)
                .as_ref().map(crate::remote_ssh::connection_fingerprint),
            source_layout: serde_json::to_value(state.saved_layout()).ok()?,
            aliases,
        })
    }

    fn close_has_other_attachment(&self, request: &TerminalCloseRequest) -> bool {
        close_has_other_attachment(request, &self.terminal_routes, &self.project_states)
    }

    fn report_terminal_close_error(&self, target: &TerminalJumpTarget, message: &str, cx: &mut Context<Self>) {
        let Some(project) = self.project(&target.project_id) else {
            return;
        };
        crate::toast::push_message_deduped(
            crate::notify::ToastKind::PasteError,
            target.project_id.clone(), project.name.clone(), message.to_string(), cx,
        );
    }

    fn report_terminal_close_conflict(&mut self, target: &TerminalJumpTarget, message: &str, cx: &mut Context<Self>) {
        if let Some(state) = self.project_states.get_mut(&target.project_id) {
            if let Some(pane) = state.pane_mut(target.pane_key.as_str()) {
                pane.status = PaneStatus::Error;
            }
            state.status = state.highest_status();
        }
        self.report_terminal_close_error(target, message, cx);
        cx.notify();
    }

    /// Returns whether an active selected terminal was removed and needs focus handoff.
    /// Dormant host IPC runs off the GUI thread; uncertainty never removes the record.
    pub(crate) fn close_terminal_target(
        &mut self,
        request: TerminalCloseRequest,
        cx: &mut Context<Self>,
    ) -> Task<bool> {
        if self.pending_terminal_closes.contains(&request.target.terminal_session_id)
            || self.terminal_close_snapshot(&request.target)
                .is_none_or(|current| !request.matches_before_dispatch(&current))
        {
            return Task::ready(false);
        }
        if !request.may_replace_alias_layout(
            self.layout_dirty_worktree_owners.get(&request.target.worktree_id).map(String::as_str),
        ) {
            self.report_terminal_close_conflict(&request.target,
                "Another project may own a newer layout. The terminal was kept; review the project aliases before closing it.", cx);
            return Task::ready(false);
        }
        if self.close_has_other_attachment(&request) {
            self.report_terminal_close_error(&request.target,
                "This terminal is attached through another project. The saved terminal was kept.", cx);
            return Task::ready(false);
        }
        let incarnation = match terminal_close_plan(&request, self.terminal_host.is_some()) {
            TerminalClosePlan::Attached(pty_id) => {
                self.dispose_terminal(pty_id, cx);
                return Task::ready(self.remove_closed_pane(&request.target, cx));
            }
            TerminalClosePlan::RecordOnly => {
                return Task::ready(self.remove_closed_pane(&request.target, cx));
            }
            TerminalClosePlan::HostUnavailable => {
                self.report_terminal_close_error(&request.target,
                    "Terminal host is disabled or unavailable. Re-enable it before closing this saved terminal.", cx);
                return Task::ready(false);
            }
            TerminalClosePlan::Hosted(incarnation) => incarnation,
        };
        let Some(client) = self.terminal_host.clone() else {
            return Task::ready(false);
        };
        let Some(request) = self.pending_terminal_closes.begin(request) else {
            return Task::ready(false);
        };
        cx.spawn(async move |this, cx| {
            let session_id = request.target.terminal_session_id.clone();
            let result = cx.background_executor().spawn(async move {
                client.kill(session_id, incarnation)
            }).await;
            this.update(cx, |store, cx| {
                if !store.pending_terminal_closes.finish(&request) {
                    return false;
                }
                match terminal_close_completion(
                    &request,
                    store.terminal_close_snapshot(&request.target).as_ref(),
                    store.close_has_other_attachment(&request) || !request.may_replace_alias_layout(
                        store.layout_dirty_worktree_owners.get(&request.target.worktree_id).map(String::as_str),
                    ),
                    result.map_err(|error| error.code()),
                ) {
                    TerminalCloseCompletion::Stale => false,
                    TerminalCloseCompletion::Failed(code) => {
                        store.report_terminal_close_error(&request.target, dormant_close_error(code), cx);
                        false
                    }
                    TerminalCloseCompletion::Conflict => {
                        store.report_terminal_close_conflict(&request.target,
                            "Terminal closed, but another project layout changed. The saved record was kept; review it before retrying.", cx);
                        false
                    }
                    TerminalCloseCompletion::Remove => store.remove_closed_pane(&request.target, cx),
                }
            }).unwrap_or(false)
        })
    }

    fn remove_closed_pane(&mut self, target: &TerminalJumpTarget, cx: &mut Context<Self>) -> bool {
        let project_id = target.project_id.as_str();
        let pane_id = target.pane_key.as_str();
        let was_selected = self.active_pane_id(project_id).as_deref() == Some(pane_id);
        let Some(state) = self.project_states.get_mut(project_id) else {
            return false;
        };
        state.remove_pane(pane_id);
        if self.focused_pane_id.as_deref() == Some(pane_id) {
            self.focused_pane_id = (self.active_project_id.as_deref() == Some(project_id))
                .then(|| self.active_pane_id(project_id))
                .flatten();
        }
        // Closing a background terminal must not start unrelated dormant records.
        let hydrate_neighbor = was_selected
            && self.active_project_id.as_deref() == Some(project_id)
            && self.project_states.get(project_id)
                .and_then(|state| state.selected_terminal())
                .is_some_and(|pane| pane.pty_id.is_none());
        if hydrate_neighbor {
            self.hydrate_project(project_id, cx);
        }
        self.after_layout_change(project_id, cx);
        was_selected && self.active_project_id.as_deref() == Some(project_id)
            && self.active_worktree_id() == Some(&target.worktree_id)
    }

    /// Select one terminal under its existing owner and focus its current entity.
    pub fn activate_pane(
        &mut self,
        project_id: &str,
        pane_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_pane_inner(project_id, pane_id, true, window, cx);
    }

    /// Focuses only an already-attached pane. Unlike ordinary navigation, this
    /// path cannot hydrate another dormant pane while routing a live Agent.
    pub(super) fn activate_existing_pane(
        &mut self,
        project_id: &str,
        pane_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.activate_pane_inner(project_id, pane_id, false, window, cx)
    }

    fn activate_pane_inner(
        &mut self,
        project_id: &str,
        pane_id: &str,
        hydrate: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(pane) = self
            .project_states
            .get(project_id)
            .and_then(|state| state.pane(pane_id))
        else {
            return false;
        };
        if self.pending_terminal_closes.contains(&pane.terminal_session_id) {
            return false;
        }
        let live = pane.pty_id.is_some_and(|id| self.terminals.contains_key(&id));
        if !hydrate && !live {
            return false;
        }
        self.clear_preedit_of_focused(cx);
        let Some(state) = self.project_states.get_mut(project_id) else {
            return false;
        };
        if !state.select_terminal(pane_id) {
            return false;
        }
        if hydrate && !live {
            self.hydrate_project(project_id, cx);
        }
        self.focus_pane(project_id, pane_id, window, cx);
        self.save_project_layout_soon(project_id, cx);
        true
    }

    /// Cycle the complete flat worktree inventory (Ctrl+Tab / Ctrl+Shift+Tab).
    pub fn cycle_pane(
        &mut self,
        project_id: &str,
        from_pane_id: &str,
        delta: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self
            .project_states
            .get(project_id)
            .and_then(|state| state.cycle_terminal_target(from_pane_id, delta))
            .and_then(|id| self.terminal_jump_target_for_pane(project_id, &id));
        if let Some(target) = target {
            self.focus_terminal_jump_target(&target, window, cx);
        }
    }

    /// Select the 1-based flat terminal index; out-of-range requests are inert.
    pub fn select_pane_by_index(
        &mut self,
        project_id: &str,
        _from_pane_id: &str,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self
            .project_states
            .get(project_id)
            .and_then(|state| state.terminal_at_index(index))
            .and_then(|pane| self.terminal_jump_target_for_pane(project_id, &pane.id));
        if let Some(target) = target {
            self.focus_terminal_jump_target(&target, window, cx);
        }
    }

    /// 把当前焦点 pane 的 IME 预编辑串收掉(切 tab / 关 pane 之前)。
    fn clear_preedit_of_focused(&mut self, cx: &mut Context<Self>) {
        let Some(pane_id) = self.focused_pane_id.clone() else {
            return;
        };
        let pty_id = self
            .project_states
            .values()
            .find_map(|s| s.pane(&pane_id).and_then(|p| p.pty_id));
        if let Some(entity) = pty_id.and_then(|id| self.terminals.get(&id)).cloned() {
            entity.update(cx, |pane, cx| pane.clear_preedit(cx));
        }
    }

    /// 把键盘焦点交给某个 pane。
    pub fn focus_pane(
        &mut self,
        project_id: &str,
        pane_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_project_id.as_deref() != Some(project_id)
            || self.active_pane_id(project_id).as_deref() != Some(pane_id)
        {
            return;
        }
        self.focused_pane_id = Some(pane_id.to_string());
        let pty_id = self
            .project_states
            .get(project_id)
            .and_then(|s| s.pane(pane_id))
            .and_then(|p| p.pty_id);
        if let Some(entity) = pty_id.and_then(|id| self.terminals.get(&id)) {
            entity.update(cx, |pane, _| pane.focus(window));
        }
        cx.notify();
    }

    /// Remembered worktree terminal, with a focused-pane fallback for old live state.
    pub fn active_pane_id(&self, project_id: &str) -> Option<String> {
        let state = self.project_states.get(project_id)?;
        state.selected_terminal_pane_key.as_ref().and_then(|key| state.pane(key.as_str()))
            .or_else(|| self.focused_pane_id.as_deref().and_then(|id| state.pane(id)))
            .or_else(|| state.selected_terminal())
            .map(|pane| pane.id.clone())
    }

    /// 把一段文本当作用户键入写进某个 pane。
    ///
    /// 走 `TerminalPane::write` 而不是裸 PTY 写,是为了保住 AI 输入检测那一路 ——
    /// 与用户自己敲这条命令完全同一条链路,pane 因此能正常进入 AI 会话状态
    /// (旧版 `writePtyInput` 的同一条红线)。
    pub fn write_to_pane(
        &mut self,
        project_id: &str,
        pane_id: &str,
        text: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(pty_id) = self
            .project_states
            .get(project_id)
            .and_then(|s| s.pane(pane_id))
            .and_then(|p| p.pty_id)
        else {
            return false;
        };
        let Some(entity) = self.terminals.get(&pty_id).cloned() else {
            return false;
        };
        let bytes = text.as_bytes().to_vec();
        entity.update(cx, |pane, cx| pane.write(&bytes, cx));
        true
    }

    /// 分屏分隔条拖动后的比例回写。
    pub fn set_split_sizes(
        &mut self,
        project_id: &str,
        node_id: &str,
        sizes: Vec<f64>,
        cx: &mut Context<Self>,
    ) {
        let changed = self
            .project_states
            .get_mut(project_id)
            .and_then(|s| s.node_mut(node_id))
            .map(|node| match node {
                SplitNode::Split {
                    sizes: current,
                    children,
                    ..
                } => {
                    if sizes.len() != children.len() || *current == sizes {
                        false
                    } else {
                        *current = sizes;
                        true
                    }
                }
                SplitNode::Leaf { .. } => false,
            })
            .unwrap_or(false);
        if changed {
            self.save_project_layout_soon(project_id, cx);
        }
    }

    /// Ordinary dormant activation recovers eligible records in the selected
    /// terminal's original legacy panel. Exact-live activation skips this path.
    ///
    /// # AI 自动续接
    ///
    /// 逐条搬运 `src/components/PaneGroup.tsx` 的那个 effect 与
    /// `src/utils/aiResume.ts`:
    ///
    /// 1. **起 PTY 的目录**用会话记录的 cwd —— `claude --resume` 只认「启动目录」
    ///    对应的会话桶,起于子目录的会话在项目根恢复会报 `No conversation found`;
    ///    但 **pane 自己的 cwd 优先**(那是用户显式给这个 pane 定的目录,worktree
    ///    终端靠它),会话 cwd 只在 pane 没指定时兜底;
    /// 2. 存量记录没有 cwd 时向 `mt_ai` 反查 jsonl,查到随身份写回并持久化,
    ///    下次重启免查;codex 会话不按目录分桶,不反查;
    /// 3. 写完 resume **只清 `resume_pending`、保留 `ai_session`** ——
    ///    codex resume 不会重新上报 SessionStart,身份清了第二次重启就断代;
    /// 4. 否决条件全在 [`resolve_auto_resume_command`]。
    pub fn hydrate_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        if self.defer_remote_hydration(project_id, cx) {
            return;
        }
        let Some(project) = self.project(project_id).cloned() else {
            return;
        };
        // SSH 远程项目的 PTY 是 ssh 启动器,启动初期可能停在口令交互上,预写的
        // 命令会被当口令消费;远端会话身份也不来自本机 hook(mt-ssh 尚未进
        // crates/,这里只把这条守卫先立住)。
        let remote = project.ssh_connection_id.is_some();
        // 缺省开启(`config.aiAutoResume`)
        let auto_resume = self.config.ai_auto_resume.unwrap_or(true);

        struct Pending {
            pane_id: String,
            tab_id: TabId,
            pane_key: PaneKey,
            terminal_session_id: TerminalSessionId,
            terminal_incarnation_id: Option<TerminalIncarnationId>,
            shell_name: String,
            cwd: Option<String>,
            ai_session: Option<AiSessionRef>,
            resume_pending: bool,
        }
        // Selection sets the original owner before this ordinary recovery path.
        // Retain panel-wide dormant recovery; flat rendering does not change PTY lifetime.
        let pending: Vec<Pending> = self
            .project_states
            .get(project_id)
            .and_then(|state| state.active_panel())
            .map(|panel| {
                let tab_id = panel.tab_id.clone();
                panel
                    .layout
                    .panes()
                    .into_iter()
                    // Failed or exited records are not automatically restarted.
                    .filter(|p| p.pty_id.is_none() && p.status != PaneStatus::Error)
                    .filter(|p| !self.pending_terminal_closes.contains(&p.terminal_session_id))
                    .map(|p| Pending {
                        pane_id: p.id.clone(),
                        tab_id: tab_id.clone(),
                        pane_key: p.pane_key.clone(),
                        terminal_session_id: p.terminal_session_id.clone(),
                        terminal_incarnation_id: p.terminal_incarnation_id.clone(),
                        shell_name: p.shell_name.clone(),
                        cwd: p.cwd.clone(),
                        ai_session: p.ai_session.clone(),
                        resume_pending: p.resume_pending,
                    })
                    .collect()
            })
            .unwrap_or_default();
        if pending.is_empty() {
            return;
        }

        let mut spawned_any = false;
        for item in pending {
            let Some(shell) = self.resolve_shell(Some(&item.shell_name)) else {
                // 一个 shell 都没有 —— 旧版把 pane 标成 error 而不是静默跳过
                if let Some(state) = self.project_states.get_mut(project_id)
                    && let Some(pane) = state.pane_mut(&item.pane_id)
                {
                    pane.status = PaneStatus::Error;
                }
                continue;
            };

            // 这一轮要不要续接(开关 + 标记 + 远程),决定了会不会去查会话 cwd
            let session = (auto_resume && item.resume_pending && !remote)
                .then(|| item.ai_session.clone())
                .flatten();
            let resume_cwd = session.as_ref().and_then(resolve_resume_cwd);
            // pane 自己的 cwd 优先,会话 cwd 兜底
            let start_cwd = item.cwd.clone().or_else(|| resume_cwd.clone());

            let (pty_id, incarnation_id, recovery) = self.start_pty(
                &project,
                &shell,
                start_cwd.as_deref(),
                &item.tab_id,
                &item.pane_key,
                &item.terminal_session_id,
                item.terminal_incarnation_id.as_ref(),
                cx,
            );
            spawned_any = true;
            if let Some(state) = self.project_states.get_mut(project_id)
                && let Some(pane) = state.pane_mut(&item.pane_id)
            {
                pane.pty_id = Some(pty_id);
                pane.terminal_incarnation_id = Some(incarnation_id);
                pane.resume_pending &= !recovery.is_warm_reattach();
            }

            let Some(command) = resolve_auto_resume_command(
                auto_resume,
                item.resume_pending,
                item.ai_session.as_ref(),
                remote,
            ) else {
                continue;
            };
            if recovery.is_warm_reattach() {
                continue;
            }

            // 先清标记再写命令(顺序同旧版):标记的语义是「这个 pane 还没续过」
            let mut session_patch: Option<AiSessionRef> = None;
            if let Some(state) = self.project_states.get_mut(project_id)
                && let Some(pane) = state.pane_mut(&item.pane_id)
            {
                pane.resume_pending = false;
                // 反查所得的启动目录随身份写回,下次重启直达不再查
                if let Some(cwd) = resume_cwd.as_ref()
                    && let Some(sess) = pane.ai_session.as_mut()
                    && sess.cwd.as_deref() != Some(cwd.as_str())
                {
                    sess.cwd = Some(cwd.clone());
                    session_patch = Some(sess.clone());
                }
            }
            // PTY 内核缓冲 stdin,shell 就绪前写入不丢(与移动端发起会话同一时序)。
            // 走 `write_to_pane` 而不是裸 PTY 写:AI 输入检测那一路要看得见这条命令,
            // pane 才会正常进入 AI 会话状态。
            self.write_to_pane(project_id, &item.pane_id, &format!("{command}\r"), cx);
            if recovery.is_cold_restore()
                && let Some(terminal) = self.terminals.get(&pty_id)
            {
                terminal.update(cx, |pane, cx| pane.mark_agent_resumed(cx));
            }
            if session_patch.is_some() {
                self.save_project_layout_soon(project_id, cx);
            }
        }
        if spawned_any {
            self.save_project_layout_soon(project_id, cx);
        }
        cx.notify();
    }

    /// 起 PTY 并拼出 `PaneState`.
    // 拆分前是私有方法;调用点在 `store::layout`(挂后台 pane / 新建面板),升到 `pub(super)`。
    pub(super) fn spawn_pane(
        &mut self,
        project: &ProjectConfig,
        shell: &ShellConfig,
        cwd_override: Option<String>,
        tab_id: &TabId,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<PaneState> {
        if self.defer_remote_hydration(&project.id, cx) {
            return None;
        }
        let mut pane = PaneState::new(shell.name.clone());
        let (pty_id, incarnation_id, _) = self.start_pty(
            project,
            shell,
            cwd_override.as_deref(),
            tab_id,
            &pane.pane_key,
            &pane.terminal_session_id,
            None,
            cx,
        );
        pane.pty_id = Some(pty_id);
        pane.terminal_incarnation_id = Some(incarnation_id);
        pane.cwd = cwd_override;
        Some(pane)
    }

    /// 真正起一个 PTY + 终端视图,返回 pane 编号。
    ///
    /// PTY 起不到(shell 路径没了 / 目录不存在)时不 panic 也不静默:视图里显示
    /// 错误文本,pane 照样存在,用户看得见是哪个 tab 出的问题。
    // 拆分前是私有方法;调用点在 `store::ssh::reset_pane_for_reconnect`,升到 `pub(super)`。
    // Stable routing fields stay explicit at the spawn boundary.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_pty(
        &mut self,
        project: &ProjectConfig,
        shell: &ShellConfig,
        cwd_override: Option<&str>,
        tab_id: &TabId,
        pane_key: &PaneKey,
        terminal_session_id: &TerminalSessionId,
        expected_incarnation_id: Option<&TerminalIncarnationId>,
        cx: &mut Context<Self>,
    ) -> (u32, TerminalIncarnationId, TerminalRecovery) {
        let pty_id = self.next_pty_id;
        self.next_pty_id += 1;

        let cwd = cwd_override
            .map(str::to_string)
            .unwrap_or_else(|| project.path.clone());
        let mut env = vec![
            // hook 子进程靠它关联回具体 pane(与装机版同一个变量名,不能改)
            ("MINITERM_PTY_ID".to_string(), pty_id.to_string()),
        ];
        env.extend([
            ("MINITERM_TAB_ID".to_string(), tab_id.to_string()),
            ("MINITERM_PANE_KEY".to_string(), pane_key.to_string()),
            (
                "MINITERM_TERMINAL_SESSION_ID".to_string(),
                terminal_session_id.to_string(),
            ),
        ]);
        let route_identity = self
            .project_worktree_bindings
            .get(&project.id)
            .map(|binding| {
                (
                    binding.execution_host_id.clone(),
                    binding.worktree_id.clone(),
                )
            });
        if let Some((execution_host_id, worktree_id)) = route_identity.as_ref() {
            env.push((
                "MINITERM_EXECUTION_HOST_ID".to_string(),
                execution_host_id.to_string(),
            ));
            env.push(("MINITERM_WORKTREE_ID".to_string(), worktree_id.to_string()));
        }
        let is_remote = project.ssh_connection_id.is_some();
        let remote_incarnation_id = is_remote.then(TerminalIncarnationId::new);
        let remote_terminal_route = route_identity
            .as_ref()
            .zip(remote_incarnation_id.as_ref())
            .map(
                |((execution_host_id, worktree_id), terminal_incarnation_id)| TerminalRoute {
                    execution_host_id: execution_host_id.clone(),
                    worktree_id: worktree_id.clone(),
                    tab_id: tab_id.clone(),
                    pane_key: pane_key.clone(),
                    terminal_session_id: terminal_session_id.clone(),
                    terminal_incarnation_id: terminal_incarnation_id.clone(),
                },
            );
        let remote_terminal_env = crate::ai::remote_agent_status_enabled()
            .then_some(remote_terminal_route.as_ref())
            .flatten()
            .map(|route| mt_pty::ssh::RemoteTerminalEnv {
                protocol_version: mt_ai::AGENT_RUNTIME_PROTOCOL_VERSION,
                execution_host_id: route.execution_host_id.to_string(),
                worktree_id: route.worktree_id.to_string(),
                tab_id: route.tab_id.to_string(),
                pane_key: route.pane_key.to_string(),
                terminal_session_id: route.terminal_session_id.to_string(),
                terminal_incarnation_id: route.terminal_incarnation_id.to_string(),
            });

        let hook_port = self.ai.hook_port();
        if hook_port > 0 {
            env.push(("MINITERM_HOOK_PORT".to_string(), hook_port.to_string()));
        }

        // SSH 远程分支:直接 spawn `ssh` 作 PTY 子进程(不经本地 shell,对齐 WSL
        // 启动器重写模式)。本地 cwd 用兜底目录 —— 远程目录由 ssh 的远端命令
        // `cd '<path>' 2>/dev/null; exec $SHELL -l` 进入,项目的 `path` 是远程 POSIX 路径,
        // 传给 portable-pty 只会让 ConPTY 静默退回 `$USERPROFILE`。
        //
        // AI 状态感知在这条路上走 PTY 输入/输出扫描的降级路径(输入检测作用于
        // 数据流,对远程天然可用);hook 精确状态不可用,PRD 已接受。
        //
        // 项目级环境变量对远程 pane **不注入**(装机版同款:那些变量属于本地
        // 机器,注给本地 ssh 客户端毫无意义)。
        let remote = project.ssh_connection_id.as_deref().map(|conn_id| {
            crate::remote_ssh::find_connection(&self.config.ssh_connections, conn_id).and_then(
                |conn| {
                    if let Some(route) = remote_terminal_env.as_ref() {
                        crate::remote_ssh::prepare_remote_launch_with_env(&conn, &cwd, Some(route))
                    } else {
                        crate::remote_ssh::prepare_remote_launch(&conn, &cwd)
                    }
                },
            )
        });
        let (spec, extras) = match remote {
            None => (
                PtySpawn {
                    program: shell.command.clone(),
                    args: shell.args.clone().unwrap_or_default(),
                    cwd: Some(cwd.clone()),
                    env,
                    rows: mt_pty::INITIAL_PTY_ROWS,
                    cols: mt_pty::INITIAL_PTY_COLS,
                },
                crate::pane::RemoteLaunchExtras::default(),
            ),
            Some(Ok(launch)) => (
                PtySpawn {
                    program: launch.program,
                    args: launch.args,
                    cwd: Some(mt_pty::fallback_local_cwd()),
                    env,
                    rows: mt_pty::INITIAL_PTY_ROWS,
                    cols: mt_pty::INITIAL_PTY_COLS,
                },
                crate::pane::RemoteLaunchExtras {
                    legacy_incarnation_id: remote_incarnation_id.clone(),
                    ssh_password: launch.password,
                    preflight_error: None,
                },
            ),
            Some(Err(err)) => (
                // 预检失败:不 spawn,pane 里直接显示这条错误(见 RemoteLaunchExtras)。
                // spec 的内容此时不会被用到,给一份无害的占位。
                PtySpawn {
                    program: shell.command.clone(),
                    args: Vec::new(),
                    cwd: None,
                    env,
                    rows: mt_pty::INITIAL_PTY_ROWS,
                    cols: mt_pty::INITIAL_PTY_COLS,
                },
                crate::pane::RemoteLaunchExtras {
                    legacy_incarnation_id: remote_incarnation_id.clone(),
                    ssh_password: None,
                    preflight_error: Some(err),
                },
            ),
        };
        // 项目级环境变量走 user_env —— 它会被 `MINITERM_` 前缀过滤挡一道,
        // 用户手改配置(现在是 config.db)也覆盖不掉内部协议变量。
        // 远程 pane 不注入(见上方分支注释)。
        let user_env: Vec<(String, String)> = if is_remote {
            Vec::new()
        } else {
            project
                .env_vars
                .iter()
                .filter(|v| v.enabled)
                .map(|v| (v.key.clone(), v.value.clone()))
                .collect()
        };

        let hosted = if is_remote {
            None
        } else {
            self.terminal_host
                .clone()
                .zip(
                    route_identity
                        .as_ref()
                        .map(|(_, worktree_id)| worktree_id.clone()),
                )
                .map(|(client, worktree_id)| HostedLaunch {
                    client,
                    worktree_id,
                    terminal_session_id: terminal_session_id.clone(),
                    expected_incarnation_id: expected_incarnation_id.cloned(),
                })
        };

        let style = self.terminal_style();
        let theme = self.terminal_theme.clone();
        let dwell = self.selection_dwell();
        // 回滚行数在**建终端时**就要喂进 alacritty 的 `term::Config` ——
        // 它决定 grid 的历史容量,晚一步只能靠 `set_options` 补(见 `apply_scrollback`)
        let scrollback = resolve_scrollback(self.config.terminal_scrollback as f64) as usize;
        let ai = self.ai.clone();
        let entity = cx.new(|cx| {
            TerminalPane::new(
                pty_id, spec, user_env, style, theme, dwell, scrollback, ai, extras, hosted, cx,
            )
        });
        let terminal_incarnation_id = entity.read(cx).terminal_incarnation_id().clone();
        let recovery = entity.read(cx).recovery();
        let terminal_route = route_identity.map(|(execution_host_id, worktree_id)| TerminalRoute {
            execution_host_id,
            worktree_id,
            tab_id: tab_id.clone(),
            pane_key: pane_key.clone(),
            terminal_session_id: terminal_session_id.clone(),
            terminal_incarnation_id: terminal_incarnation_id.clone(),
        });
        if let Some(route) = terminal_route.as_ref() {
            self.terminal_routes.insert(pty_id, route.clone());
        }
        self.ai.add_pane(pty_id, terminal_route.clone());

        // 子进程退出 → pane 状态 error(与旧版 pty-exit 同语义);
        // 用户键入 → 清 attention 黄灯(与旧版 clearPaneAttentionByPty 同语义)
        let expected_route = terminal_route;
        let sub = cx.subscribe(&entity, move |store, _entity, event: &PaneEvent, cx| {
            if expected_route
                .as_ref()
                .is_some_and(|route| store.terminal_routes.get(&pty_id) != Some(route))
            {
                return;
            }
            match event {
                PaneEvent::Exited(code) => store.on_pty_exit(pty_id, *code, cx),
                PaneEvent::UserInput => store.clear_pane_attention_by_pty(pty_id, cx),
                // AI 任务标记。**必须走事件而不是在 write 里直接 update store** ——
                // `write_to_pane` 是在 `store.update` 里调 `pane.write` 的,那里再去
                // `AppStore::global(cx).update` 就是同一实体的嵌套 update(gpui 直接 panic)。
                // `cx.emit` 是延后派发的,天然绕开。
                PaneEvent::AiMarks(batch) => store.add_markers(pty_id, batch.clone(), cx),
            }
        });
        self.pane_subs.insert(pty_id, sub);
        self.terminals.insert(pty_id, entity);
        (pty_id, terminal_incarnation_id, recovery)
    }

    /// 拖选停留自动复制的参数(`config.selectionAutoCopySecs`)。
    ///
    /// **缺省 1 秒**,与前端 `config.selectionAutoCopySecs ?? 1` 一字不差;填 0
    /// 就是关掉停留语义(退回「松手即复制」)。设置页改了这一项之后走
    /// [`Self::apply_selection_dwell`] 给存量终端下发 —— 与 `apply_theme` 同形。
    // 拆分前是私有方法;调用点在 `store::prefs::set_selection_auto_copy_secs`,升到 `pub(super)`。
    pub(super) fn selection_dwell(&self) -> DwellConfig {
        DwellConfig::from_secs(self.config.selection_auto_copy_secs.unwrap_or(1.0) as f32)
    }

    // 拆分前是私有方法;调用点在 `store::prefs::apply_terminal_style`,升到 `pub(super)`。
    pub(super) fn terminal_style(&self) -> TerminalStyle {
        terminal_style_from(
            self.config.terminal_font_size,
            self.config.terminal_font_family.as_deref(),
            self.config.terminal_ligatures,
        )
    }

    /// 解析要用的 shell:指定名 → `defaultShell` → 列表首项。
    pub fn resolve_shell(&self, preferred: Option<&str>) -> Option<ShellConfig> {
        let shells = &self.config.available_shells;
        preferred
            .and_then(|name| shells.iter().find(|s| s.name == name))
            .or_else(|| shells.iter().find(|s| s.name == self.config.default_shell))
            .or_else(|| shells.first())
            .cloned()
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use mt_identity::{ExecutionHostId, HostInstallId, RepoId, WorktreeId};

    use super::*;

    fn close_request() -> TerminalCloseRequest {
        let host = ExecutionHostId::derive("local", &HostInstallId::new());
        let repo = RepoId::derive(&host, "/repo/.git");
        let worktree = WorktreeId::derive(&repo, "/repo", None);
        let mut request = TerminalCloseRequest {
            target: TerminalJumpTarget {
                project_id: "owner".into(),
                execution_host_id: host.clone(),
                worktree_id: worktree.clone(),
                tab_id: TabId::new(),
                pane_key: PaneKey::new(),
                terminal_session_id: TerminalSessionId::new(),
                terminal_incarnation_id: Some(TerminalIncarnationId::new()),
            },
            pty_id: None,
            binding: ProjectWorktreeBinding {
                project_id: "owner".into(),
                execution_host_id: host,
                repo_id: repo,
                worktree_id: worktree,
                identity_source: "test".into(),
                canonical_worktree_path: Some("/repo".into()),
                identity_context: None,
            },
            project_path: "/repo".into(),
            ssh_connection_id: None,
            connection_fingerprint: None,
            source_layout: serde_json::Value::Null,
            aliases: Vec::new(),
        };
        request.source_layout = serde_json::to_value(state_for(&request).saved_layout()).unwrap();
        request
    }

    fn state_for(request: &TerminalCloseRequest) -> ProjectState {
        let mut pane = PaneState::new("saved-shell");
        pane.pane_key = request.target.pane_key.clone();
        pane.id = pane.pane_key.to_string();
        pane.terminal_session_id = request.target.terminal_session_id.clone();
        pane.terminal_incarnation_id = request.target.terminal_incarnation_id.clone();
        pane.pty_id = request.pty_id;
        let mut state = ProjectState::new();
        state.panels.push(ProjectPanel::with_tab_id(request.target.tab_id.clone(), SplitNode::leaf(pane)));
        state.select_terminal(request.target.pane_key.as_str());
        state
    }

    #[test]
    fn dormant_close_requires_host_confirmation_only_for_local_saved_incarnations() {
        let mut request = close_request();
        assert_eq!(terminal_close_plan(&request, true), TerminalClosePlan::Hosted(request.target.terminal_incarnation_id.clone().unwrap()));
        assert_eq!(terminal_close_plan(&request, false), TerminalClosePlan::HostUnavailable);
        request.project_path = r"\\wsl.localhost\Ubuntu\repo".into();
        assert!(matches!(terminal_close_plan(&request, true), TerminalClosePlan::Hosted(_)));
        assert_eq!(terminal_close_plan(&request, false), TerminalClosePlan::HostUnavailable);
        request.ssh_connection_id = Some("ssh".into());
        assert_eq!(terminal_close_plan(&request, true), TerminalClosePlan::RecordOnly);
        assert_eq!(terminal_close_plan(&request, false), TerminalClosePlan::RecordOnly);
        request.ssh_connection_id = None;
        request.target.terminal_incarnation_id = None;
        assert_eq!(terminal_close_plan(&request, false), TerminalClosePlan::RecordOnly);
        request.pty_id = Some(17);
        assert_eq!(terminal_close_plan(&request, false), TerminalClosePlan::Attached(17));
    }

    #[test]
    fn pending_close_excludes_alias_activation_reconnect_and_duplicate_close_until_its_owner_finishes() {
        let request = close_request();
        let mut pending = PendingTerminalCloses::default();
        let first = pending.begin(request.clone()).unwrap();
        assert!(pending.contains(&request.target.terminal_session_id));
        assert!(pending.begin(request.clone()).is_none());
        let mut alias = request.clone();
        alias.target.project_id = "alias".into();
        alias.target.terminal_incarnation_id = Some(TerminalIncarnationId::new());
        assert!(pending.begin(alias).is_none());
        let other = pending.begin(close_request()).unwrap();
        assert!(pending.finish(&first));
        assert!(!pending.contains(&request.target.terminal_session_id));
        let retry = pending.begin(request).unwrap();
        assert!(!pending.finish(&first), "a late completion must not release the retry");
        assert!(pending.contains(&retry.target.terminal_session_id));
        assert!(pending.finish(&other));
        assert!(pending.finish(&retry));
    }

    #[test]
    fn close_never_deletes_on_missing_exited_transport_or_protocol_failure() {
        let request = close_request();
        for code in [None, Some(HostErrorCode::SessionMissing), Some(HostErrorCode::SessionExited),
            Some(HostErrorCode::ProtocolMismatch), Some(HostErrorCode::IncarnationMismatch),
            Some(HostErrorCode::IoFailed), Some(HostErrorCode::HostBusy), Some(HostErrorCode::RecoveryUnavailable)] {
            assert_eq!(terminal_close_completion(&request, Some(&request), false, Err(code)), TerminalCloseCompletion::Failed(code));
            assert!(dormant_close_error(code).len() < 180);
        }
        assert_eq!(terminal_close_completion(&request, Some(&request), false, Ok(())), TerminalCloseCompletion::Remove);
    }

    #[test]
    fn close_completion_rejects_every_changed_route_source_and_attachment() {
        let request = close_request();
        let mutations: &[fn(&mut TerminalCloseRequest)] = &[
            |r| r.target.project_id = "different".into(),
            |r| r.target.execution_host_id = ExecutionHostId::derive("other", &HostInstallId::new()),
            |r| r.target.worktree_id = WorktreeId::derive(&r.binding.repo_id, "/different", None),
            |r| r.target.tab_id = TabId::new(),
            |r| r.target.pane_key = PaneKey::new(),
            |r| r.target.terminal_session_id = TerminalSessionId::new(),
            |r| r.target.terminal_incarnation_id = Some(TerminalIncarnationId::new()),
            |r| r.pty_id = Some(9),
            |r| r.project_path = "/different".into(),
            |r| r.binding.identity_context = Some("rebound".into()),
            |r| r.ssh_connection_id = Some("other-transport".into()),
            |r| r.connection_fingerprint = Some(5),
        ];
        for mutate in mutations {
            let mut current = request.clone();
            mutate(&mut current);
            assert_eq!(terminal_close_completion(&request, Some(&current), false, Ok(())), TerminalCloseCompletion::Stale);
        }
        assert_eq!(terminal_close_completion(&request, None, false, Ok(())), TerminalCloseCompletion::Stale);
        assert_eq!(terminal_close_completion(&request, Some(&request), true, Ok(())), TerminalCloseCompletion::Conflict);
    }

    #[test]
    fn selection_and_order_changes_during_close_preserve_the_new_selected_terminal() {
        let mut request = close_request();
        let mut state = state_for(&request);
        let neighbor = PaneState::new("neighbor");
        let neighbor_id = neighbor.id.clone();
        let neighbor_key = neighbor.pane_key.clone();
        state.panels.push(ProjectPanel::new(SplitNode::leaf(neighbor)));
        state.normalize_terminal_navigation(None);
        let mut alias_state = state_for(&request);
        alias_state.panels = state.panels.clone();
        alias_state.normalize_terminal_navigation(None);
        request.source_layout = serde_json::to_value(state.saved_layout()).unwrap();
        let mut alias_binding = request.binding.clone();
        alias_binding.project_id = "alias".into();
        let bindings = HashMap::from([
            ("owner".into(), request.binding.clone()), ("alias".into(), alias_binding),
        ]);
        let mut states = HashMap::from([
            ("owner".into(), state), ("alias".into(), alias_state),
        ]);
        request.aliases = terminal_close_aliases(&request.target, &bindings, &states).unwrap();
        assert!(request.may_replace_alias_layout(None));
        assert!(request.may_replace_alias_layout(Some("owner")));
        assert!(!request.may_replace_alias_layout(Some("alias")));
        let state = states.get_mut("owner").unwrap();
        assert!(state.select_terminal(&neighbor_id));
        assert!(state.reorder_terminal(&neighbor_key, &request.target.pane_key, false));
        let mut current = request.clone();
        current.source_layout = serde_json::to_value(state.saved_layout()).unwrap();
        current.aliases = terminal_close_aliases(&request.target, &bindings, &states).unwrap();
        assert_ne!(request.source_layout, current.source_layout);
        assert!(request.matches_before_dispatch(&current));
        assert!(request.may_replace_alias_layout(None));
        assert_eq!(terminal_close_completion(&request, Some(&current), false, Ok(())), TerminalCloseCompletion::Remove);
        assert_eq!(
            terminal_close_completion(&request, Some(&current), !request.may_replace_alias_layout(Some("alias")), Ok(())),
            TerminalCloseCompletion::Conflict
        );
        let state = states.get_mut("owner").unwrap();
        state.remove_pane(request.target.pane_key.as_str());
        assert_eq!(state.selected_terminal().unwrap().id, neighbor_id);
        assert!(state.selected_terminal().unwrap().pty_id.is_none(), "background close does not hydrate");

        states.get_mut("alias").unwrap().panels[0].custom_title = Some("changed alias".into());
        current.aliases = terminal_close_aliases(&request.target, &bindings, &states).unwrap();
        assert_eq!(terminal_close_completion(&request, Some(&current), false, Ok(())), TerminalCloseCompletion::Conflict);
    }

    #[test]
    fn preexisting_alias_append_blocks_close_before_and_after_layout_flush() {
        let mut request = close_request();
        let source = state_for(&request);
        let source_before = serde_json::to_value(source.saved_layout()).unwrap();
        let mut alias = state_for(&request);
        let mut mobile_pane = PaneState::new("mobile-shell");
        mobile_pane.pty_id = Some(83);
        mobile_pane.terminal_incarnation_id = Some(TerminalIncarnationId::new());
        let mobile_key = mobile_pane.pane_key.clone();
        assert!(alias.append_background_terminal(request.target.tab_id.clone(), mobile_pane));
        let alias_before = serde_json::to_value(alias.saved_layout()).unwrap();
        let mut alias_binding = request.binding.clone();
        alias_binding.project_id = "alias".into();
        let bindings = HashMap::from([
            ("owner".into(), request.binding.clone()), ("alias".into(), alias_binding),
        ]);
        let states = HashMap::from([("owner".into(), source), ("alias".into(), alias)]);
        request.aliases = terminal_close_aliases(&request.target, &bindings, &states).unwrap();

        // The shared source stayed dormant; its attachment fence cannot see the
        // different terminal added before close captured the other alias.
        assert!(!close_has_other_attachment(&request, &HashMap::new(), &states));
        assert!(request.matches_before_dispatch(&request));
        assert!(!request.may_replace_alias_layout(Some("alias")));
        assert!(!request.may_replace_alias_layout(None), "flush has forgotten its owner");
        assert!(!request.may_replace_alias_layout(Some("owner")), "selection cannot authorize an older inventory");
        assert_eq!(serde_json::to_value(states["owner"].saved_layout()).unwrap(), source_before);
        assert_eq!(serde_json::to_value(states["alias"].saved_layout()).unwrap(), alias_before);
        assert_eq!(states["alias"].pane(mobile_key.as_str()).unwrap().pty_id, Some(83));
        assert!(states["alias"].pane(request.target.pane_key.as_str()).unwrap().pty_id.is_none());
    }

    #[test]
    fn another_alias_attachment_blocks_dormant_close_even_with_the_same_pty_handle() {
        let mut request = close_request();
        let mut attached = request.clone();
        attached.pty_id = Some(7);
        let states = HashMap::from([("alias".into(), state_for(&attached))]);
        assert!(close_has_other_attachment(&request, &HashMap::new(), &states));
        request.pty_id = Some(7);
        assert!(close_has_other_attachment(&request, &HashMap::new(), &states));
        let states = HashMap::from([("owner".into(), state_for(&request))]);
        assert!(!close_has_other_attachment(&request, &HashMap::new(), &states));
        let route = TerminalRoute {
            execution_host_id: request.target.execution_host_id.clone(),
            worktree_id: request.target.worktree_id.clone(),
            tab_id: request.target.tab_id.clone(),
            pane_key: request.target.pane_key.clone(),
            terminal_session_id: request.target.terminal_session_id.clone(),
            terminal_incarnation_id: request.target.terminal_incarnation_id.clone().unwrap(),
        };
        assert!(close_has_other_attachment(&request, &HashMap::from([(8, route)]), &HashMap::new()));
    }
}
