//! Worktree-scoped immutable projections for contextual UI surfaces.
//!
//! This module is the only UI-facing boundary that joins stable worktree,
//! terminal, and Agent identities. Callers receive display models and route
//! actions; they never reconstruct ownership from paths or runtime PTY ids.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::OsStr;

use gpui::{App, Context, Entity, Window};
use mt_ai::{
    AgentActivity, AgentActivityFreshness, AgentConfirmation, AgentConnectivity, AgentEvidence,
    AgentProvider, AgentRoute, AgentRuntimeState,
    sessions::AiSession,
};
use mt_identity::{
    AgentEventId, AgentRunId, ExecutionHostId, PaneKey, TabId, TerminalIncarnationId,
    TerminalSessionId, WorktreeId,
};
use mt_layout::ProjectWorktreeBinding;

use crate::pane::TerminalRecovery;
use crate::execution_host::{ExecutionBackendSignature, ExecutionSourceSignature};
use crate::tree::{PaneState, PaneStatus};

use super::{AppStore, ProjectState, RemoteAgentProbeCapability};

const DIAGNOSTIC_TEXT_LIMIT: usize = 512;
const RUNTIME_TITLE_LIMIT: usize = 120;

#[derive(Clone, Debug, PartialEq, Eq)]
struct RuntimeTitleOwner {
    run_id: AgentRunId,
    route: AgentRoute,
    provider: AgentProvider,
    session_id: String,
    connection_epoch: Option<u64>,
}

impl RuntimeTitleOwner {
    fn from_run(run: &AgentRuntimeState) -> Option<Self> {
        if run.activity.is_ended()
            || run.confirmation != AgentConfirmation::LiveConfirmed
            || run.evidence == AgentEvidence::RestoredHistory
        {
            return None;
        }
        Some(Self {
            run_id: run.run_id.clone(),
            route: run.route.clone(),
            provider: run.provider.clone(),
            session_id: run.provider_session_id.clone().filter(|id| !id.is_empty())?,
            connection_epoch: run.connection_epoch,
        })
    }

    fn matches(&self, run: &AgentRuntimeState) -> bool {
        Self::from_run(run).as_ref() == Some(self)
    }

    fn matches_source(&self, source: &ExecutionSourceSignature) -> bool {
        self.route.execution_host_id == source.execution_host_id
            && self.route.worktree_id == source.worktree_id
            && match &source.backend {
                ExecutionBackendSignature::Ssh { connection_epoch, .. } => {
                    connection_epoch.is_some() && *connection_epoch == self.connection_epoch
                }
                ExecutionBackendSignature::Local | ExecutionBackendSignature::Wsl { .. } => {
                    self.connection_epoch.is_none()
                }
            }
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeTitleRequest {
    project_id: String,
    source: ExecutionSourceSignature,
    owners: Vec<RuntimeTitleOwner>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RuntimeSessionTitle {
    owner: RuntimeTitleOwner,
    source: ExecutionSourceSignature,
    title: String,
}

impl RuntimeSessionTitle {
    fn for_run<'a>(
        &'a self,
        run: &AgentRuntimeState,
        source: Option<&ExecutionSourceSignature>,
    ) -> Option<&'a str> {
        (self.owner.matches(run) && source == Some(&self.source)).then_some(self.title.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RuntimeLiveTitle {
    route: AgentRoute,
    provider: AgentProvider,
    process: mt_ai::AgentProcessIdentity,
    source: ExecutionSourceSignature,
    title: String,
}

impl RuntimeLiveTitle {
    fn for_run<'a>(&'a self, run: &AgentRuntimeState, source: Option<&ExecutionSourceSignature>) -> Option<&'a str> {
        (self.route == run.route && self.provider == run.provider
            && run.process == Some(self.process) && !run.activity.is_ended()
            && source == Some(&self.source)).then_some(self.title.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentTargetView {
    pub run_id: AgentRunId,
    pub last_event_id: AgentEventId,
    pub project_id: String,
    pub project_name: String,
    pub root_project_name: String,
    pub worktree_name: String,
    pub host_label: String,
    pub pane_id: String,
    pub pane_label: String,
    pub route: AgentRoute,
    pub provider: AgentProvider,
    pub provider_session_id: Option<String>,
    pub activity: AgentActivity,
    pub activity_freshness: AgentActivityFreshness,
    pub connectivity: AgentConnectivity,
    pub connection_epoch: Option<u64>,
    pub evidence: AgentEvidence,
    pub received_at_unix_ms: i64,
    pub attention: bool,
    pub unread: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TerminalJumpTarget {
    pub project_id: String,
    pub execution_host_id: ExecutionHostId,
    pub worktree_id: WorktreeId,
    pub tab_id: TabId,
    pub pane_key: PaneKey,
    pub terminal_session_id: TerminalSessionId,
    pub terminal_incarnation_id: Option<TerminalIncarnationId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalJumpView {
    pub target: TerminalJumpTarget,
    pub project_name: String,
    pub root_project_name: String,
    pub worktree_name: String,
    pub host_label: String,
    pub panel_label: String,
    pub pane_label: String,
    pub status: PaneStatus,
    pub active: bool,
    pub dormant: bool,
    pub live: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AgentActivationReceipt {
    run_id: AgentRunId,
    event_id: AgentEventId,
    project_id: String,
    pane_id: String,
    route: AgentRoute,
}

impl AgentActivationReceipt {
    fn from_target(target: &AgentTargetView) -> Self {
        Self {
            run_id: target.run_id.clone(),
            event_id: target.last_event_id.clone(),
            project_id: target.project_id.clone(),
            pane_id: target.pane_id.clone(),
            route: target.route.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteAgentDiagnosticView {
    pub capability: RemoteAgentProbeCapability,
    pub connectivity: AgentConnectivity,
    pub process_count: usize,
    pub connection_epoch: u64,
    pub last_error: Option<String>,
    pub updated_at_unix_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalDiagnosticView {
    pub project_id: String,
    pub pane_id: String,
    pub pane_label: String,
    pub route: Option<AgentRoute>,
    pub recovery: TerminalRecovery,
    pub exited: bool,
    pub backend_notice: Option<String>,
    pub agent: Option<AgentTargetView>,
    pub remote_agent: Option<RemoteAgentDiagnosticView>,
}

/// Rollback gate for the Orca worktree-context ownership model.
///
/// Only the exact value `0` disables it. Missing and all other values keep the
/// verified path active, matching the existing shell rollout convention.
pub fn orca_worktree_context_enabled() -> bool {
    orca_worktree_context_enabled_for(
        std::env::var_os("MINI_TERM_ORCA_WORKTREE_CONTEXT").as_deref(),
    )
}

fn orca_worktree_context_enabled_for(value: Option<&OsStr>) -> bool {
    value.is_none_or(|value| value != "0")
}

fn bounded_text(value: &str) -> String {
    value.chars().take(DIAGNOSTIC_TEXT_LIMIT).collect()
}

fn usable_runtime_title(value: &str) -> Option<String> {
    let title = value
        .split_whitespace()
        .flat_map(|word| word.chars().chain(std::iter::once(' ')))
        .filter(|ch| !ch.is_control())
        .take(RUNTIME_TITLE_LIMIT)
        .collect::<String>();
    let title = title.trim_end();
    (!title.is_empty()).then(|| title.to_string())
}

fn runtime_fallback_title(label: &str, identity: &str) -> String {
    let label = usable_runtime_title(label).unwrap_or_else(|| "Terminal".into());
    let label = label.chars().take(64).collect::<String>();
    let identity = identity.strip_prefix(AgentRunId::PREFIX)
        .or_else(|| identity.strip_prefix(TerminalSessionId::PREFIX)).unwrap_or(identity);
    let identity = identity.chars().take(8).collect::<String>();
    format!("{label} [{identity}]")
}

fn runtime_display_title(
    custom_title: Option<&str>,
    session_title: Option<&str>,
    live_title: Option<&str>,
    fallback: &str,
    identity: &str,
) -> String {
    custom_title.and_then(usable_runtime_title)
        .or_else(|| session_title.and_then(usable_runtime_title))
        .or_else(|| live_title.and_then(usable_runtime_title))
        .unwrap_or_else(|| runtime_fallback_title(fallback, identity))
}

fn live_runtime_title(provider: &AgentProvider, title: &str) -> Option<String> {
    let mut title = title.trim().trim_start_matches(|ch| {
        ('\u{2800}'..='\u{28ff}').contains(&ch) || ch == '\u{2733}'
    }).trim();
    match provider.as_str() {
        AgentProvider::OPENCODE => {
            title = title.strip_prefix("\u{25a3} ").unwrap_or(title);
            title = title.strip_prefix("OC | ").unwrap_or(title);
        }
        AgentProvider::PI => {
            if let Some(label) = title.strip_prefix("\u{03c0} ") {
                title = label.trim_start_matches([':', '!', '>', '-']).trim();
            }
        }
        _ => {}
    }
    let title = usable_runtime_title(title)?;
    let lower = title.to_ascii_lowercase();
    let cwd_like = title.starts_with(['/', '\\', '~'])
        || title.as_bytes().get(1..3).is_some_and(|drive| drive == b":/" || drive == b":\\")
        || (!title.contains(char::is_whitespace)
            && (title.contains(['/', '\\']) || (title.contains('@') && title.contains(':'))));
    if cwd_like || lower == provider.as_str()
        || matches!(lower.as_str(), "claude code" | "working" | "waiting" | "done"
            | "starting" | "permission required" | "terminal" | "untitled"
            | "bash" | "zsh" | "fish" | "pwsh" | "powershell")
    {
        return None;
    }
    Some(title)
}

fn session_title_for_owner(
    owner: &RuntimeTitleOwner,
    source: &ExecutionSourceSignature,
    sessions: &[AiSession],
) -> Option<String> {
    if !owner.matches_source(source) {
        return None;
    }
    let mut matched = sessions.iter().filter(|session| {
        session.id == owner.session_id
            && session.session_type.parse::<AgentProvider>().ok().as_ref() == Some(&owner.provider)
            && match &source.backend {
                ExecutionBackendSignature::Local => {
                    session.ssh_connection_id.is_none() && session.wsl_distro.is_none()
                }
                ExecutionBackendSignature::Wsl { distro } => {
                    session.ssh_connection_id.is_none()
                        && session.wsl_distro.as_deref() == Some(distro.as_str())
                }
                ExecutionBackendSignature::Ssh { connection_id, .. } => {
                    session.ssh_connection_id.as_deref() == Some(connection_id.as_str())
                        && session.wsl_distro.is_none()
                }
            }
    });
    let title = usable_runtime_title(&matched.next()?.title)?;
    if title.eq_ignore_ascii_case("untitled")
        || matched.any(|session| usable_runtime_title(&session.title).as_ref() != Some(&title))
    {
        return None;
    }
    Some(title)
}

fn event_requires_feed_acknowledgement(activity: AgentActivity, attention: bool) -> bool {
    attention
        || matches!(
            activity,
            AgentActivity::Blocked
                | AgentActivity::Failed
                | AgentActivity::Done
                | AgentActivity::Waiting
        )
}

fn agent_event_is_unread(
    acknowledged: Option<&AgentEventId>,
    current: &AgentEventId,
    activity: AgentActivity,
    attention: bool,
) -> bool {
    event_requires_feed_acknowledgement(activity, attention) && acknowledged != Some(current)
}

fn prune_agent_acknowledgements(
    acknowledgements: &mut HashMap<AgentRunId, AgentEventId>,
    removed: &[AgentRunId],
) {
    for run_id in removed {
        acknowledgements.remove(run_id);
    }
}

fn activation_receipt_matches_target(
    receipt: &AgentActivationReceipt,
    target: &AgentTargetView,
) -> bool {
    receipt.run_id == target.run_id
        && receipt.project_id == target.project_id
        && receipt.pane_id == target.pane_id
        && receipt.route == target.route
}

fn path_leaf_label(path: &str, fallback: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|label| !label.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn activity_rank(activity: AgentActivity, attention: bool) -> u8 {
    if attention || matches!(activity, AgentActivity::Blocked | AgentActivity::Failed) {
        0
    } else if matches!(activity, AgentActivity::Starting | AgentActivity::Working) {
        1
    } else if matches!(activity, AgentActivity::Done | AgentActivity::Waiting) {
        2
    } else {
        3
    }
}

fn compare_agent_targets(left: &AgentTargetView, right: &AgentTargetView) -> Ordering {
    activity_rank(left.activity, left.attention)
        .cmp(&activity_rank(right.activity, right.attention))
        .then_with(|| right.received_at_unix_ms.cmp(&left.received_at_unix_ms))
        .then_with(|| left.provider.cmp(&right.provider))
        .then_with(|| left.run_id.cmp(&right.run_id))
}

fn route_matches_terminal(
    route: &AgentRoute,
    execution_host_id: &ExecutionHostId,
    worktree_id: &WorktreeId,
    tab_id: &TabId,
    pane_key: &PaneKey,
    terminal_session_id: &TerminalSessionId,
    terminal_incarnation_id: Option<&TerminalIncarnationId>,
) -> bool {
    &route.execution_host_id == execution_host_id
        && &route.worktree_id == worktree_id
        && &route.tab_id == tab_id
        && &route.pane_key == pane_key
        && &route.terminal_session_id == terminal_session_id
        && terminal_incarnation_id == Some(&route.terminal_incarnation_id)
}

fn exact_terminal_route<'a>(
    route: Option<&'a AgentRoute>,
    execution_host_id: &ExecutionHostId,
    worktree_id: &WorktreeId,
    tab_id: &TabId,
    pane_key: &PaneKey,
    terminal_session_id: &TerminalSessionId,
    terminal_incarnation_id: Option<&TerminalIncarnationId>,
) -> Option<&'a AgentRoute> {
    route.filter(|route| {
        route_matches_terminal(
            route,
            execution_host_id,
            worktree_id,
            tab_id,
            pane_key,
            terminal_session_id,
            terminal_incarnation_id,
        )
    })
}

fn terminal_jump_identity_matches(
    target: &TerminalJumpTarget,
    execution_host_id: &ExecutionHostId,
    worktree_id: &WorktreeId,
    tab_id: &TabId,
    pane_key: &PaneKey,
    terminal_session_id: &TerminalSessionId,
    terminal_incarnation_id: Option<&TerminalIncarnationId>,
) -> bool {
    &target.execution_host_id == execution_host_id
        && &target.worktree_id == worktree_id
        && &target.tab_id == tab_id
        && &target.pane_key == pane_key
        && &target.terminal_session_id == terminal_session_id
        && target.terminal_incarnation_id.as_ref() == terminal_incarnation_id
}

fn resolve_terminal_pane<'a>(
    binding: &ProjectWorktreeBinding,
    state: &'a ProjectState,
    target: &TerminalJumpTarget,
) -> Option<&'a PaneState> {
    if binding.project_id != target.project_id {
        return None;
    }
    let panel = state
        .panels
        .iter()
        .find(|panel| panel.tab_id == target.tab_id)?;
    let pane = panel.layout.pane(target.pane_key.as_str())?;
    terminal_jump_identity_matches(
        target,
        &binding.execution_host_id,
        &binding.worktree_id,
        &panel.tab_id,
        &pane.pane_key,
        &pane.terminal_session_id,
        pane.terminal_incarnation_id.as_ref(),
    )
    .then_some(pane)
}

fn selected_terminal_matches_route(state: &ProjectState, route: &AgentRoute) -> bool {
    state
        .active_panel()
        .is_some_and(|panel| panel.tab_id == route.tab_id)
        && state
            .selected_terminal()
            .is_some_and(|pane| pane.pane_key == route.pane_key)
}

fn preferred_agent_route_project_id<'a>(
    candidate_project_ids: impl IntoIterator<Item = &'a str>,
    active_project_id: Option<&str>,
) -> Option<&'a str> {
    candidate_project_ids.into_iter().min_by(|left, right| {
        match (
            active_project_id == Some(*left),
            active_project_id == Some(*right),
        ) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => left.cmp(right),
        }
    })
}

impl AppStore {
    pub(super) fn clear_runtime_live_title(&mut self, route: &AgentRoute) {
        self.runtime_live_titles.retain(|_, title| &title.route != route);
    }

    pub(super) fn record_runtime_live_title(
        &mut self,
        run_id: &AgentRunId,
        route: &AgentRoute,
        process: mt_ai::AgentProcessIdentity,
        source: ExecutionSourceSignature,
        title: &str,
    ) {
        let Some(run) = self.agent_runtime.run(run_id).filter(|run| {
            &run.route == route && run.process == Some(process) && !run.activity.is_ended()
                && run.confirmation == AgentConfirmation::LiveConfirmed
                && run.evidence >= AgentEvidence::ProcessAttested
        }) else { return; };
        let Some(title) = live_runtime_title(&run.provider, title) else {
            self.runtime_live_titles.remove(run_id);
            return;
        };
        self.runtime_live_titles.insert(run_id.clone(), RuntimeLiveTitle {
            route: route.clone(), provider: run.provider.clone(), process, source, title,
        });
    }

    pub(super) fn current_agent_route_for_pane(
        &self,
        project_id: &str,
        pane: &PaneState,
    ) -> Option<&AgentRoute> {
        let pty_id = pane.pty_id?;
        if !self.terminals.contains_key(&pty_id) {
            return None;
        }
        let binding = self.project_worktree_bindings.get(project_id)?;
        let panel = self.project_states.get(project_id)?.panels.iter().find(|panel| {
            panel.layout.pane(&pane.id).is_some_and(|current| current.pty_id == Some(pty_id))
        })?;
        exact_terminal_route(
            self.terminal_routes.get(&pty_id),
            &binding.execution_host_id,
            &binding.worktree_id,
            &panel.tab_id,
            &pane.pane_key,
            &pane.terminal_session_id,
            pane.terminal_incarnation_id.as_ref(),
        )
    }

    pub fn pane_has_live_agent(&self, project_id: &str, pane: &PaneState) -> bool {
        if pane.pty_id.is_none_or(|pty_id| {
            self.is_pty_exited(pty_id) || !self.terminals.contains_key(&pty_id)
        }) {
            return false;
        }
        match self.current_agent_route_for_pane(project_id, pane) {
            Some(route) if self.agent_runtime.runs().any(|run| &run.route == route) => {
                return super::ai::accepted_agent_projection(&self.agent_runtime, route, false).live;
            }
            None if pane.pty_id.is_some_and(|pty_id| self.terminal_routes.contains_key(&pty_id)) => {
                return false;
            }
            _ => {}
        }
        matches!(pane.status, PaneStatus::AiWorking | PaneStatus::AiIdle)
    }

    pub fn pane_agent_provider(&self, project_id: &str, pane: &PaneState) -> Option<String> {
        if pane.pty_id.is_none() {
            return pane.shows_ai_session(self.config.ai_auto_resume.unwrap_or(true))
                .then(|| pane.ai_agent()).flatten().map(str::to_string);
        }
        if !self.pane_has_live_agent(project_id, pane) {
            return None;
        }
        if let Some(route) = self.current_agent_route_for_pane(project_id, pane)
            && self.agent_runtime.runs().any(|run| &run.route == route)
        {
            return super::ai::accepted_agent_projection(&self.agent_runtime, route, false).provider;
        }
        pane.ai_agent().map(str::to_string)
    }

    pub(crate) fn runtime_title_request(&self, project_id: &str) -> Option<RuntimeTitleRequest> {
        let source = self.project_execution_snapshot(project_id).ok()?.source_signature();
        let owners = self.agent_runtime.runs().filter_map(|run| {
            let owner = RuntimeTitleOwner::from_run(run)?;
            if !owner.matches_source(&source)
                || run.connectivity != AgentConnectivity::Live
                || self.resolve_agent_target(&run.run_id).is_none()
            {
                return None;
            }
            Some(owner)
        }).collect();
        Some(RuntimeTitleRequest { project_id: project_id.to_string(), source, owners })
    }

    pub(crate) fn apply_runtime_session_titles(
        &mut self,
        request: &RuntimeTitleRequest,
        sessions: &[AiSession],
        cx: &mut Context<Self>,
    ) {
        let current_source = self.project_execution_snapshot(&request.project_id)
            .ok().map(|snapshot| snapshot.source_signature());
        if current_source.as_ref() != Some(&request.source) {
            return;
        }
        self.runtime_session_titles.retain(|run_id, title| {
            self.agent_runtime.run(run_id).is_some_and(|run| title.owner.matches(run))
        });
        let mut changed = false;
        for owner in &request.owners {
            if !self.agent_runtime.run(&owner.run_id).is_some_and(|run| owner.matches(run))
                || self.resolve_agent_target(&owner.run_id).is_none()
            {
                continue;
            }
            let Some(title) = session_title_for_owner(owner, &request.source, sessions) else {
                continue;
            };
            let title = RuntimeSessionTitle {
                owner: owner.clone(), source: request.source.clone(), title,
            };
            if self.runtime_session_titles.get(&owner.run_id) != Some(&title) {
                self.runtime_session_titles.insert(owner.run_id.clone(), title);
                changed = true;
            }
        }
        if changed {
            cx.notify();
        }
    }

    fn runtime_pane_label(
        &self,
        project_id: &str,
        pane: &PaneState,
        run: Option<&AgentRuntimeState>,
    ) -> String {
        let source = self.project_execution_snapshot(project_id).ok()
            .map(|snapshot| snapshot.source_signature());
        let session_title = run.and_then(|run| {
            self.runtime_session_titles.get(&run.run_id)?.for_run(run, source.as_ref())
        });
        let live_title = run.and_then(|run| {
            self.runtime_live_titles.get(&run.run_id)?.for_run(run, source.as_ref())
        });
        let label = run.map(|run| run.provider.as_str().to_string())
            .unwrap_or_else(|| self.pane_display_label(project_id, pane));
        let identity = run.map(|run| run.run_id.as_str()).unwrap_or(pane.terminal_session_id.as_str());
        runtime_display_title(pane.custom_title.as_deref(), session_title, live_title, &label, identity)
    }

    pub fn terminal_runtime_label(&self, project_id: &str, pane: &PaneState) -> String {
        let route = self.current_agent_route_for_pane(project_id, pane);
        let run = route.and_then(|route| super::ai::single_live_agent_for_route(&self.agent_runtime, route));
        self.runtime_pane_label(project_id, pane, run)
    }

    /// Canonical path from the stable binding. The configured path is only a
    /// compatibility fallback when no canonical value has been persisted yet.
    pub fn canonical_worktree_path_for_project(&self, project_id: &str) -> Option<&str> {
        self.project_worktree_bindings
            .get(project_id)
            .and_then(|binding| binding.canonical_worktree_path.as_deref())
            .or_else(|| {
                self.project(project_id)
                    .map(|project| project.path.as_str())
            })
    }

    fn root_project_name_for(&self, project_id: &str) -> String {
        let mut current_id = project_id;
        let mut name = self
            .project(project_id)
            .map(|project| project.name.clone())
            .unwrap_or_else(|| project_id.to_string());
        for _ in 0..self.config.projects.len() {
            let Some(project) = self.project(current_id) else {
                break;
            };
            name = project.name.clone();
            let Some(parent_id) = project.parent_project_id.as_deref() else {
                break;
            };
            if self.project(parent_id).is_none() {
                break;
            }
            current_id = parent_id;
        }
        name
    }

    fn resolve_agent_target(&self, run_id: &AgentRunId) -> Option<AgentTargetView> {
        if self.agent_runtime.is_superseded_weak_alias(run_id) {
            return None;
        }
        let run = self.agent_runtime.run(run_id)?;
        let mut candidates = Vec::new();
        for project in &self.config.projects {
            let Some(binding) = self.project_worktree_bindings.get(&project.id) else {
                continue;
            };
            if binding.worktree_id != run.route.worktree_id
                || binding.execution_host_id != run.route.execution_host_id
            {
                continue;
            }
            let Some(state) = self.project_states.get(&project.id) else {
                continue;
            };
            let Some(panel) = state
                .panels
                .iter()
                .find(|panel| panel.tab_id == run.route.tab_id)
            else {
                continue;
            };
            let Some(pane) = panel.layout.pane(run.route.pane_key.as_str()) else {
                continue;
            };
            if !route_matches_terminal(
                &run.route,
                &binding.execution_host_id,
                &binding.worktree_id,
                &panel.tab_id,
                &pane.pane_key,
                &pane.terminal_session_id,
                pane.terminal_incarnation_id.as_ref(),
            ) {
                continue;
            }
            let Some(pty_id) = pane.pty_id else {
                continue;
            };
            if !self.terminals.contains_key(&pty_id)
                || self.terminal_routes.get(&pty_id) != Some(&run.route)
            {
                continue;
            }

            let worktree_path = binding
                .canonical_worktree_path
                .as_deref()
                .unwrap_or(project.path.as_str());
            let host_label = self
                .project_execution_snapshot(&project.id)
                .map(|snapshot| snapshot.host_label)
                .unwrap_or_else(|_| run.route.execution_host_id.to_string());
            candidates.push(AgentTargetView {
                run_id: run.run_id.clone(),
                last_event_id: run.last_event_id.clone(),
                project_id: project.id.clone(),
                project_name: project.name.clone(),
                root_project_name: self.root_project_name_for(&project.id),
                worktree_name: path_leaf_label(worktree_path, &project.name),
                host_label,
                pane_id: pane.id.clone(),
                pane_label: self.runtime_pane_label(&project.id, pane, Some(run)),
                route: run.route.clone(),
                provider: run.provider.clone(),
                provider_session_id: run.provider_session_id.clone(),
                activity: run.activity,
                activity_freshness: self.agent_runtime.activity_freshness(
                    &run.run_id,
                    chrono::Utc::now().timestamp_millis(),
                ),
                connectivity: run.connectivity,
                connection_epoch: run.connection_epoch,
                evidence: run.evidence,
                received_at_unix_ms: run.received_at_unix_ms,
                attention: pane.attention,
                unread: agent_event_is_unread(
                    self.agent_feed_acknowledged.get(&run.run_id),
                    &run.last_event_id,
                    run.activity,
                    pane.attention,
                ),
            });
        }
        let project_id = preferred_agent_route_project_id(
            candidates
                .iter()
                .map(|candidate| candidate.project_id.as_str()),
            self.active_project_id.as_deref(),
        )?
        .to_string();
        candidates
            .into_iter()
            .find(|candidate| candidate.project_id == project_id)
    }

    pub fn agent_target_views(&self) -> Vec<AgentTargetView> {
        let mut targets: Vec<_> = self
            .agent_runtime
            .runs()
            .filter_map(|run| self.resolve_agent_target(&run.run_id))
            .collect();
        targets.sort_by(compare_agent_targets);
        targets
    }

    pub fn agent_target_views_for_worktree(
        &self,
        worktree_id: &WorktreeId,
    ) -> Vec<AgentTargetView> {
        let mut targets: Vec<_> = self
            .agent_runtime
            .runs_for_worktree(worktree_id)
            .filter_map(|run| self.resolve_agent_target(&run.run_id))
            .collect();
        targets.sort_by(compare_agent_targets);
        targets
    }

    /// Immutable terminal rows for global navigation. Saved panes remain
    /// visible before hydration, while live panes carry their expected
    /// incarnation so activation can reject a reused runtime route.
    pub fn terminal_jump_views(&self) -> Vec<TerminalJumpView> {
        self.config
            .projects
            .iter()
            .flat_map(|project| self.terminal_tab_views(&project.id))
            .collect()
    }

    /// Flat presentation for one exact project's bound worktree. This read never hydrates.
    pub fn terminal_tab_views(&self, project_id: &str) -> Vec<TerminalJumpView> {
        let mut rows = Vec::new();
        for project in self
            .config
            .projects
            .iter()
            .filter(|project| project.id == project_id)
        {
            let Some(binding) = self.project_worktree_bindings.get(&project.id) else {
                continue;
            };
            let Some(state) = self.project_states.get(&project.id) else {
                continue;
            };
            let worktree_path = binding
                .canonical_worktree_path
                .as_deref()
                .unwrap_or(project.path.as_str());
            let host_label = self
                .project_execution_snapshot(&project.id)
                .map(|snapshot| snapshot.host_label)
                .unwrap_or_else(|_| binding.execution_host_id.to_string());
            let active_pane_id = self.active_pane_id(&project.id);
            for (panel_index, panel) in state.panels.iter().enumerate() {
                let panel_label = panel
                    .custom_title
                    .clone()
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| format!("Terminal {}", panel_index + 1));
                for pane in panel.layout.panes() {
                    let target = TerminalJumpTarget {
                        project_id: project.id.clone(),
                        execution_host_id: binding.execution_host_id.clone(),
                        worktree_id: binding.worktree_id.clone(),
                        tab_id: panel.tab_id.clone(),
                        pane_key: pane.pane_key.clone(),
                        terminal_session_id: pane.terminal_session_id.clone(),
                        terminal_incarnation_id: pane.terminal_incarnation_id.clone(),
                    };
                    let live = pane.pty_id.is_some_and(|pty_id| {
                        self.terminals.contains_key(&pty_id)
                            && self.terminal_routes.get(&pty_id).is_some_and(|route| {
                                route_matches_terminal(
                                    route,
                                    &target.execution_host_id,
                                    &target.worktree_id,
                                    &target.tab_id,
                                    &target.pane_key,
                                    &target.terminal_session_id,
                                    target.terminal_incarnation_id.as_ref(),
                                )
                            })
                    });
                    let active = self.active_project_id.as_deref() == Some(project.id.as_str())
                        && self.active_worktree_id() == Some(&binding.worktree_id)
                        && state.active_panel().map(|active| &active.tab_id) == Some(&panel.tab_id)
                        && active_pane_id.as_deref() == Some(pane.id.as_str());
                    let display_status = pane.pty_id.filter(|pty_id| !self.is_pty_exited(*pty_id))
                        .and_then(|_| self.current_agent_route_for_pane(&project.id, pane))
                        .and_then(|route| super::ai::agent_display_status(
                            &self.agent_runtime, route, chrono::Utc::now().timestamp_millis(),
                        )).unwrap_or(pane.status);
                    rows.push(TerminalJumpView {
                        target,
                        project_name: project.name.clone(),
                        root_project_name: self.root_project_name_for(&project.id),
                        worktree_name: path_leaf_label(worktree_path, &project.name),
                        host_label: host_label.clone(),
                        panel_label: panel_label.clone(),
                        pane_label: self.terminal_runtime_label(&project.id, pane),
                        status: display_status,
                        active,
                        dormant: pane.pty_id.is_none(),
                        live,
                    });
                }
            }
            let order = state
                .ordered_terminal_panes()
                .iter()
                .enumerate()
                .map(|(index, pane)| (pane.pane_key.clone(), index))
                .collect::<HashMap<_, _>>();
            rows.sort_by_key(|row| {
                order
                    .get(&row.target.pane_key)
                    .copied()
                    .unwrap_or(usize::MAX)
            });
        }
        rows
    }

    pub fn terminal_jump_target_for_pane(
        &self,
        project_id: &str,
        pane_id: &str,
    ) -> Option<TerminalJumpTarget> {
        let binding = self.project_worktree_bindings.get(project_id)?;
        let state = self.project_states.get(project_id)?;
        let panel = state
            .panels
            .iter()
            .find(|panel| panel.layout.pane(pane_id).is_some())?;
        let pane = panel.layout.pane(pane_id)?;
        Some(TerminalJumpTarget {
            project_id: project_id.to_string(),
            execution_host_id: binding.execution_host_id.clone(),
            worktree_id: binding.worktree_id.clone(),
            tab_id: panel.tab_id.clone(),
            pane_key: pane.pane_key.clone(),
            terminal_session_id: pane.terminal_session_id.clone(),
            terminal_incarnation_id: pane.terminal_incarnation_id.clone(),
        })
    }

    pub(crate) fn resolve_terminal_jump_target(
        &self,
        target: &TerminalJumpTarget,
    ) -> Option<(String, bool)> {
        let binding = self.project_worktree_bindings.get(&target.project_id)?;
        let state = self.project_states.get(&target.project_id)?;
        let pane = resolve_terminal_pane(binding, state, target)?;
        let live = match pane.pty_id {
            Some(pty_id) => {
                self.terminals.contains_key(&pty_id)
                    && self.terminal_routes.get(&pty_id).is_some_and(|route| {
                        route_matches_terminal(
                            route,
                            &target.execution_host_id,
                            &target.worktree_id,
                            &target.tab_id,
                            &target.pane_key,
                            &target.terminal_session_id,
                            target.terminal_incarnation_id.as_ref(),
                        )
                    })
            }
            None => false,
        };
        pane.pty_id
            .is_none_or(|_| live)
            .then(|| (pane.id.clone(), live))
    }

    /// Reorder presentation only; both endpoints retain their exact original owner.
    pub fn reorder_terminal_tabs(
        &mut self,
        source: &TerminalJumpTarget,
        target: &TerminalJumpTarget,
        after: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if source.project_id != target.project_id
            || source.worktree_id != target.worktree_id
            || self.active_project_id.as_deref() != Some(source.project_id.as_str())
            || self.active_worktree_id() != Some(&source.worktree_id)
            || self.resolve_terminal_jump_target(source).is_none()
            || self.resolve_terminal_jump_target(target).is_none()
            || source.pane_key == target.pane_key
        {
            return false;
        }
        let Some(state) = self.project_states.get_mut(&source.project_id) else {
            return false;
        };
        if !state.reorder_terminal(&source.pane_key, &target.pane_key, after) {
            return false;
        }
        self.save_project_layout_soon(&source.project_id, cx);
        cx.notify();
        true
    }

    pub(super) fn focus_terminal_jump_target(
        &mut self,
        target: &TerminalJumpTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // Block navigation, not the identity resolution needed by close completion.
        if self
            .pending_terminal_closes
            .contains(&target.terminal_session_id)
        {
            return false;
        }
        let Some((pane_id, live)) = self.resolve_terminal_jump_target(target) else {
            return false;
        };
        self.set_active_project_without_hydration(&target.project_id, cx);
        let Some((current_pane_id, current_live)) = self.resolve_terminal_jump_target(target)
        else {
            return false;
        };
        if pane_id != current_pane_id || live != current_live {
            return false;
        }
        if live {
            if !self.activate_existing_pane(&target.project_id, &pane_id, window, cx) {
                return false;
            }
        } else {
            self.activate_pane(&target.project_id, &pane_id, window, cx);
        }
        self.active_project_id.as_deref() == Some(target.project_id.as_str())
            && self.active_worktree_id() == Some(&target.worktree_id)
            && self.focused_pane_id.as_deref() == Some(pane_id.as_str())
    }

    /// Revalidate and focus exactly one saved/live terminal. A missing or
    /// reused pane is inert; only an exact dormant pane may run its ordinary
    /// hydration path.
    pub fn activate_terminal_jump_target(
        store: &Entity<Self>,
        target: &TerminalJumpTarget,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        if !store.update(cx, |store, cx| {
            store.focus_terminal_jump_target(target, window, cx)
        }) {
            return false;
        }
        crate::workbench_area::activate_terminal_page(window, cx)
    }

    fn agent_target_is_active(&self, target: &AgentTargetView) -> bool {
        if self.active_project_id.as_deref() != Some(target.project_id.as_str())
            || self.active_worktree_id() != Some(&target.route.worktree_id)
            || self.focused_pane_id.as_deref() != Some(target.pane_id.as_str())
        {
            return false;
        }
        let Some(state) = self.project_states.get(&target.project_id) else {
            return false;
        };
        let Some(panel) = state.active_panel() else {
            return false;
        };
        if !selected_terminal_matches_route(state, &target.route) {
            return false;
        }
        let Some(pane) = panel.layout.pane(&target.pane_id) else {
            return false;
        };
        let Some(pty_id) = pane.pty_id else {
            return false;
        };
        self.terminals.contains_key(&pty_id)
            && self.terminal_routes.get(&pty_id) == Some(&target.route)
            && route_matches_terminal(
                &target.route,
                &target.route.execution_host_id,
                &target.route.worktree_id,
                &panel.tab_id,
                &pane.pane_key,
                &pane.terminal_session_id,
                pane.terminal_incarnation_id.as_ref(),
            )
    }

    pub(super) fn remove_agent_runtime_route(&mut self, route: &AgentRoute) {
        self.runtime_session_titles.retain(|_, title| &title.owner.route != route);
        self.clear_runtime_live_title(route);
        let removed: Vec<_> = self
            .agent_runtime
            .runs()
            .filter(|state| &state.route == route)
            .map(|state| state.run_id.clone())
            .collect();
        self.agent_runtime.remove_route(route);
        prune_agent_acknowledgements(&mut self.agent_feed_acknowledged, &removed);
    }

    fn focus_agent_run(
        &mut self,
        run_id: &AgentRunId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AgentActivationReceipt> {
        let target = self.resolve_agent_target(run_id)?;
        self.set_active_project_without_hydration(&target.project_id, cx);
        let current = self.resolve_agent_target(run_id)?;
        if !self.activate_existing_pane(&current.project_id, &current.pane_id, window, cx) {
            return None;
        }
        let active = self.resolve_agent_target(run_id)?;
        self.agent_target_is_active(&active)
            .then(|| AgentActivationReceipt::from_target(&active))
    }

    fn acknowledge_agent_activation(
        &mut self,
        receipt: &AgentActivationReceipt,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(current) = self.resolve_agent_target(&receipt.run_id) else {
            return false;
        };
        if !activation_receipt_matches_target(receipt, &current)
            || !self.agent_target_is_active(&current)
        {
            return false;
        }

        self.agent_feed_acknowledged
            .insert(receipt.run_id.clone(), receipt.event_id.clone());
        if current.last_event_id == receipt.event_id
            && let Some(state) = self.project_states.get_mut(&current.project_id)
            && let Some(pane) = state.pane_mut(&current.pane_id)
        {
            pane.attention = false;
        }
        cx.notify();
        true
    }

    /// Revalidates every stable identity before focus, reveals the terminal
    /// workbench, then acknowledges only the event selected by this activation.
    /// A stale run is inert and this path never hydrates or resumes a terminal.
    pub fn activate_agent_run(
        store: &Entity<Self>,
        run_id: &AgentRunId,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let Some(receipt) = store.update(cx, |store, cx| store.focus_agent_run(run_id, window, cx))
        else {
            return false;
        };
        if !crate::workbench_area::activate_terminal_page(window, cx) {
            return false;
        }
        store.update(cx, |store, cx| {
            store.acknowledge_agent_activation(&receipt, cx)
        })
    }

    pub fn terminal_diagnostics_for_worktree(
        &self,
        worktree_id: &WorktreeId,
        cx: &App,
    ) -> Vec<TerminalDiagnosticView> {
        let mut diagnostics = Vec::new();
        for project in &self.config.projects {
            let Some(binding) = self.project_worktree_bindings.get(&project.id) else {
                continue;
            };
            if &binding.worktree_id != worktree_id {
                continue;
            }
            let Some(state) = self.project_states.get(&project.id) else {
                continue;
            };
            for panel in &state.panels {
                for pane in panel.layout.panes() {
                    let pty_id = pane.pty_id;
                    let route = pty_id
                        .and_then(|pty_id| self.terminal_routes.get(&pty_id))
                        .and_then(|route| {
                            exact_terminal_route(
                                Some(route),
                                &binding.execution_host_id,
                                &binding.worktree_id,
                                &panel.tab_id,
                                &pane.pane_key,
                                &pane.terminal_session_id,
                                pane.terminal_incarnation_id.as_ref(),
                            )
                        })
                        .cloned();
                    let (recovery, backend_notice, terminal_exited) = pty_id
                        .and_then(|pty_id| self.terminals.get(&pty_id))
                        .map(|terminal| {
                            let terminal = terminal.read(cx);
                            (
                                terminal.recovery(),
                                terminal.backend_notice().map(bounded_text),
                                terminal.is_exited(),
                            )
                        })
                        .unwrap_or((TerminalRecovery::Unavailable, None, false));
                    let mut agents = self.agent_runtime.runs()
                        .filter(|run| route.as_ref() == Some(&run.route) && !run.activity.is_ended())
                        .filter_map(|run| self.resolve_agent_target(&run.run_id))
                        .collect::<Vec<_>>();
                    agents.sort_by(compare_agent_targets);
                    let remote_agent = pty_id
                        .and_then(|pty_id| self.remote_agent_polls.get(&pty_id))
                        .filter(|poll| route.as_ref() == Some(poll.route()))
                        .map(|poll| RemoteAgentDiagnosticView {
                            capability: poll.capability,
                            connectivity: poll.connectivity,
                            process_count: poll.process_count,
                            connection_epoch: poll.connection_epoch,
                            last_error: poll.last_error.as_deref().map(bounded_text),
                            updated_at_unix_ms: poll.updated_at_unix_ms,
                        });
                    let diagnostic = TerminalDiagnosticView {
                        project_id: project.id.clone(),
                        pane_id: pane.id.clone(),
                        pane_label: self.terminal_runtime_label(&project.id, pane),
                        route,
                        recovery,
                        exited: terminal_exited
                            || pty_id.is_some_and(|pty_id| self.is_pty_exited(pty_id)),
                        backend_notice,
                        agent: None,
                        remote_agent,
                    };
                    if agents.is_empty() {
                        diagnostics.push(diagnostic);
                    } else {
                        for agent in agents {
                            diagnostics.push(TerminalDiagnosticView {
                                pane_label: agent.pane_label.clone(),
                                agent: Some(agent),
                                ..diagnostic.clone()
                            });
                        }
                    }
                }
            }
        }
        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt_identity::{AgentEventId, AgentRunId, ExecutionHostId, HostInstallId, RepoId};

    fn route() -> AgentRoute {
        let install = HostInstallId::new();
        AgentRoute {
            execution_host_id: ExecutionHostId::derive("test-host", &install),
            worktree_id: WorktreeId::derive(
                &RepoId::derive(
                    &ExecutionHostId::derive("test-host", &install),
                    "/repo/.git",
                ),
                "/repo",
                None,
            ),
            tab_id: TabId::new(),
            pane_key: PaneKey::new(),
            terminal_session_id: TerminalSessionId::new(),
            terminal_incarnation_id: TerminalIncarnationId::new(),
        }
    }

    fn terminal_target(route: &AgentRoute) -> TerminalJumpTarget {
        TerminalJumpTarget {
            project_id: "project".into(),
            execution_host_id: route.execution_host_id.clone(),
            worktree_id: route.worktree_id.clone(),
            tab_id: route.tab_id.clone(),
            pane_key: route.pane_key.clone(),
            terminal_session_id: route.terminal_session_id.clone(),
            terminal_incarnation_id: Some(route.terminal_incarnation_id.clone()),
        }
    }

    #[test]
    fn exact_terminal_route_rejects_every_reused_identity_boundary() {
        let route = route();
        let matches = |execution_host_id: &ExecutionHostId,
                       worktree_id: &WorktreeId,
                       tab_id: &TabId,
                       pane_key: &PaneKey,
                       terminal_session_id: &TerminalSessionId,
                       terminal_incarnation_id: &TerminalIncarnationId| {
            route_matches_terminal(
                &route,
                execution_host_id,
                worktree_id,
                tab_id,
                pane_key,
                terminal_session_id,
                Some(terminal_incarnation_id),
            )
        };
        assert!(matches(
            &route.execution_host_id,
            &route.worktree_id,
            &route.tab_id,
            &route.pane_key,
            &route.terminal_session_id,
            &route.terminal_incarnation_id,
        ));
        let other_host = ExecutionHostId::derive("other-host", &HostInstallId::new());
        let other_worktree = WorktreeId::derive(
            &RepoId::derive(&route.execution_host_id, "/other/.git"),
            "/other",
            None,
        );
        assert!(!matches(
            &other_host,
            &route.worktree_id,
            &route.tab_id,
            &route.pane_key,
            &route.terminal_session_id,
            &route.terminal_incarnation_id,
        ));
        assert!(!matches(
            &route.execution_host_id,
            &other_worktree,
            &route.tab_id,
            &route.pane_key,
            &route.terminal_session_id,
            &route.terminal_incarnation_id,
        ));
        assert!(!matches(
            &route.execution_host_id,
            &route.worktree_id,
            &TabId::new(),
            &route.pane_key,
            &route.terminal_session_id,
            &route.terminal_incarnation_id,
        ));
        assert!(!matches(
            &route.execution_host_id,
            &route.worktree_id,
            &route.tab_id,
            &PaneKey::new(),
            &route.terminal_session_id,
            &route.terminal_incarnation_id,
        ));
        assert!(!matches(
            &route.execution_host_id,
            &route.worktree_id,
            &route.tab_id,
            &route.pane_key,
            &TerminalSessionId::new(),
            &route.terminal_incarnation_id,
        ));
        assert!(!matches(
            &route.execution_host_id,
            &route.worktree_id,
            &route.tab_id,
            &route.pane_key,
            &route.terminal_session_id,
            &TerminalIncarnationId::new(),
        ));
    }

    #[test]
    fn terminal_jump_target_rejects_each_stable_identity_mismatch() {
        let route = route();
        let target = terminal_target(&route);
        let matches = |target: &TerminalJumpTarget| {
            terminal_jump_identity_matches(
                target,
                &route.execution_host_id,
                &route.worktree_id,
                &route.tab_id,
                &route.pane_key,
                &route.terminal_session_id,
                Some(&route.terminal_incarnation_id),
            )
        };
        assert!(matches(&target));
        let mut changed = target.clone();
        changed.execution_host_id = ExecutionHostId::derive("other", &HostInstallId::new());
        assert!(!matches(&changed));
        let mut changed = target.clone();
        changed.worktree_id = WorktreeId::derive(
            &RepoId::derive(&route.execution_host_id, "/other/.git"),
            "/other",
            None,
        );
        assert!(!matches(&changed));
        let mut changed = target.clone();
        changed.tab_id = TabId::new();
        assert!(!matches(&changed));
        let mut changed = target.clone();
        changed.pane_key = PaneKey::new();
        assert!(!matches(&changed));
        let mut changed = target.clone();
        changed.terminal_session_id = TerminalSessionId::new();
        assert!(!matches(&changed));
        let mut changed = target;
        changed.terminal_incarnation_id = Some(TerminalIncarnationId::new());
        assert!(!matches(&changed));
    }

    #[test]
    fn exact_terminal_lookup_and_agent_selection_retain_hidden_legacy_owner() {
        use crate::tree::{ProjectPanel, SplitDirection, SplitNode, gen_id};
        let route = route();
        let target = terminal_target(&route);
        let binding = ProjectWorktreeBinding {
            project_id: target.project_id.clone(),
            execution_host_id: route.execution_host_id.clone(),
            repo_id: RepoId::derive(&route.execution_host_id, "/repo/.git"),
            worktree_id: route.worktree_id.clone(),
            identity_source: "test".into(),
            canonical_worktree_path: Some("/repo".into()),
            identity_context: None,
        };
        let mut source = PaneState::from_identity(
            "source",
            route.pane_key.clone(),
            route.terminal_session_id.clone(),
            Some(route.terminal_incarnation_id.clone()),
        );
        source.pty_id = Some(44);
        let other = PaneState::new("other");
        let first = ProjectPanel::new(SplitNode::leaf(PaneState::new("first-owner")));
        let owner = ProjectPanel::with_tab_id(
            route.tab_id.clone(),
            SplitNode::Split {
                id: gen_id("split"),
                direction: SplitDirection::Vertical,
                sizes: vec![40.0, 60.0],
                children: vec![SplitNode::leaf(other), SplitNode::leaf(source.clone())],
            },
        );
        let mut state = ProjectState::new();
        state.active_panel_id = Some(first.id.clone());
        state.panels = vec![first, owner];
        state.normalize_terminal_navigation(None);
        assert_eq!(
            resolve_terminal_pane(&binding, &state, &target),
            Some(&source)
        );
        assert!(!selected_terminal_matches_route(&state, &route));
        state.select_terminal(&source.id);
        assert!(selected_terminal_matches_route(&state, &route));
        assert_ne!(
            state
                .active_layout()
                .unwrap()
                .first_active_pane()
                .unwrap()
                .pane_key,
            source.pane_key
        );
        let anchor = state.all_panes()[0].pane_key.clone();
        state.reorder_terminal(&source.pane_key, &anchor, false);
        assert_eq!(
            resolve_terminal_pane(&binding, &state, &target),
            Some(&source)
        );
        assert!(selected_terminal_matches_route(&state, &route));
        let mut changed = target.clone();
        changed.project_id = "different-project".into();
        assert!(resolve_terminal_pane(&binding, &state, &changed).is_none());
        let mut changed = target.clone();
        changed.tab_id = state.panels[0].tab_id.clone();
        assert!(resolve_terminal_pane(&binding, &state, &changed).is_none());
        let mut rebound = binding.clone();
        rebound.worktree_id = WorktreeId::derive(&binding.repo_id, "/other-worktree", None);
        assert!(resolve_terminal_pane(&rebound, &state, &target).is_none());
        state.pane_mut(&source.id).unwrap().terminal_incarnation_id =
            Some(TerminalIncarnationId::new());
        assert!(resolve_terminal_pane(&binding, &state, &target).is_none());
        state.remove_pane(&source.id);
        assert!(resolve_terminal_pane(&binding, &state, &target).is_none());
        assert_eq!(state.all_panes().len(), 2);
    }

    #[test]
    fn diagnostics_accept_only_an_exact_current_terminal_route() {
        let route = route();
        assert_eq!(
            exact_terminal_route(
                Some(&route),
                &route.execution_host_id,
                &route.worktree_id,
                &route.tab_id,
                &route.pane_key,
                &route.terminal_session_id,
                Some(&route.terminal_incarnation_id),
            ),
            Some(&route)
        );
        assert!(
            exact_terminal_route(
                Some(&route),
                &route.execution_host_id,
                &route.worktree_id,
                &route.tab_id,
                &route.pane_key,
                &route.terminal_session_id,
                None,
            )
            .is_none()
        );
    }

    #[test]
    fn shared_alias_route_selection_prefers_active_exact_candidate_in_any_order() {
        for candidates in [
            vec!["project-b", "project-a"],
            vec!["project-a", "project-b"],
        ] {
            assert_eq!(
                preferred_agent_route_project_id(candidates, Some("project-b")),
                Some("project-b")
            );
        }
    }

    #[test]
    fn shared_alias_route_selection_uses_project_id_without_active_candidate() {
        for candidates in [
            vec!["project-b", "project-a"],
            vec!["project-a", "project-b"],
        ] {
            assert_eq!(
                preferred_agent_route_project_id(candidates.clone(), None),
                Some("project-a")
            );
            assert_eq!(
                preferred_agent_route_project_id(candidates, Some("other-project")),
                Some("project-a")
            );
        }
    }

    #[test]
    fn worktree_context_rollback_only_accepts_exact_zero() {
        assert!(orca_worktree_context_enabled_for(None));
        assert!(!orca_worktree_context_enabled_for(Some(OsStr::new("0"))));
        assert!(orca_worktree_context_enabled_for(Some(OsStr::new("false"))));
        assert!(orca_worktree_context_enabled_for(Some(OsStr::new("1"))));
    }

    #[test]
    fn agent_activity_order_keeps_connectivity_out_of_the_activity_axis() {
        assert_eq!(activity_rank(AgentActivity::Waiting, true), 0);
        assert_eq!(activity_rank(AgentActivity::Working, false), 1);
        assert_eq!(activity_rank(AgentActivity::Done, false), 2);
        assert_eq!(activity_rank(AgentActivity::Unknown, false), 3);
    }

    #[test]
    fn diagnostic_text_is_unicode_safe_and_bounded() {
        let text = "状".repeat(DIAGNOSTIC_TEXT_LIMIT + 10);
        assert_eq!(bounded_text(&text).chars().count(), DIAGNOSTIC_TEXT_LIMIT);
    }

    #[test]
    fn feed_acknowledgement_is_exact_and_a_later_event_is_unread_again() {
        let run = AgentRunId::new();
        let first = AgentEventId::new();
        let second = AgentEventId::new();
        let mut acknowledgements = HashMap::new();

        assert!(agent_event_is_unread(
            acknowledgements.get(&run),
            &first,
            AgentActivity::Done,
            false,
        ));
        acknowledgements.insert(run.clone(), first.clone());
        assert!(!agent_event_is_unread(
            acknowledgements.get(&run),
            &first,
            AgentActivity::Done,
            false,
        ));
        assert!(agent_event_is_unread(
            acknowledgements.get(&run),
            &second,
            AgentActivity::Waiting,
            false,
        ));
        assert!(!agent_event_is_unread(
            None,
            &second,
            AgentActivity::Working,
            false,
        ));
    }

    #[test]
    fn pruning_removes_only_orphaned_run_watermarks() {
        let removed = AgentRunId::new();
        let retained = AgentRunId::new();
        let mut acknowledgements = HashMap::from([
            (removed.clone(), AgentEventId::new()),
            (retained.clone(), AgentEventId::new()),
        ]);
        prune_agent_acknowledgements(&mut acknowledgements, std::slice::from_ref(&removed));
        assert!(!acknowledgements.contains_key(&removed));
        assert!(acknowledgements.contains_key(&retained));
    }

    #[test]
    fn worktree_leaf_label_handles_local_and_windows_spelling() {
        assert_eq!(path_leaf_label("/repo/feature", "fallback"), "feature");
        assert_eq!(
            path_leaf_label(r"C:\\repo\\feature\\", "fallback"),
            "feature"
        );
        assert_eq!(path_leaf_label("/", "fallback"), "fallback");
    }

    fn target_for_receipt(route: AgentRoute) -> AgentTargetView {
        AgentTargetView {
            run_id: AgentRunId::new(),
            last_event_id: AgentEventId::new(),
            project_id: "project".into(),
            project_name: "worktree".into(),
            root_project_name: "project".into(),
            worktree_name: "worktree".into(),
            host_label: "Local machine".into(),
            pane_id: route.pane_key.to_string(),
            pane_label: "Terminal".into(),
            route,
            provider: "codex".parse().unwrap(),
            provider_session_id: Some("session".into()),
            activity: AgentActivity::Done,
            activity_freshness: AgentActivityFreshness::Fresh,
            connectivity: AgentConnectivity::Live,
            connection_epoch: None,
            evidence: AgentEvidence::Hook,
            received_at_unix_ms: 1,
            attention: false,
            unread: true,
        }
    }

    fn title_run() -> AgentRuntimeState {
        AgentRuntimeState {
            run_id: AgentRunId::new(),
            last_event_id: AgentEventId::new(),
            route: route(),
            provider: "codex".parse().unwrap(),
            provider_session_id: Some("exact-session".into()),
            process: mt_ai::AgentProcessIdentity::new(10, 100),
            activity: AgentActivity::Unknown,
            connectivity: AgentConnectivity::Live,
            confirmation: AgentConfirmation::LiveConfirmed,
            evidence: AgentEvidence::Hook,
            connection_epoch: Some(1),
            weak_episode: None,
            last_sequence: 1,
            received_at_unix_ms: 1,
        }
    }

    fn title_source(run: &AgentRuntimeState) -> ExecutionSourceSignature {
        ExecutionSourceSignature {
            execution_host_id: run.route.execution_host_id.clone(),
            root_project_id: "project".into(),
            root_source_path: "/repo".into(),
            worktree_id: run.route.worktree_id.clone(),
            canonical_path: "/repo".into(),
            backend: ExecutionBackendSignature::Ssh {
                connection_id: "ssh".into(),
                connection_fingerprint: 42,
                connection_epoch: Some(1),
            },
        }
    }

    fn title_session(id: &str, provider: &str, title: &str) -> AiSession {
        AiSession {
            id: id.into(), session_type: provider.into(), title: title.into(),
            timestamp: "2026-09-06T00:00:00Z".into(), model: None,
            wsl_distro: None, ssh_connection_id: Some("ssh".into()),
        }
    }

    #[test]
    fn runtime_title_requires_exact_provider_session_and_execution_source() {
        let run = title_run();
        let owner = RuntimeTitleOwner::from_run(&run).unwrap();
        let source = title_source(&run);
        let sessions = vec![
            title_session("newest-unrelated", "codex", "Unrelated title"),
            title_session("exact-session", "claude", "Other provider title"),
            title_session("exact-session", "codex", "Owned conversation"),
        ];
        assert_eq!(session_title_for_owner(&owner, &source, &sessions).as_deref(),
            Some("Owned conversation"));
        assert!(session_title_for_owner(&owner, &source, &sessions[..2]).is_none());
        let mut changed_source = source.clone();
        changed_source.backend = ExecutionBackendSignature::Local;
        assert!(session_title_for_owner(&owner, &changed_source, &sessions).is_none());
        let mut ambiguous = sessions;
        ambiguous.push(title_session("exact-session", "codex", "Conflicting title"));
        assert!(session_title_for_owner(&owner, &source, &ambiguous).is_none());
    }

    #[test]
    fn runtime_title_rejects_replacement_runs_routes_sessions_and_sources() {
        let run = title_run();
        let source = title_source(&run);
        let title = RuntimeSessionTitle {
            owner: RuntimeTitleOwner::from_run(&run).unwrap(),
            source: source.clone(), title: "Owned".into(),
        };
        assert_eq!(title.for_run(&run, Some(&source)), Some("Owned"));
        let mut changed = run.clone();
        changed.run_id = AgentRunId::new();
        assert!(title.for_run(&changed, Some(&source)).is_none());
        changed = run.clone();
        changed.route.terminal_incarnation_id = TerminalIncarnationId::new();
        assert!(title.for_run(&changed, Some(&source)).is_none());
        changed = run.clone();
        changed.provider_session_id = Some("other".into());
        assert!(title.for_run(&changed, Some(&source)).is_none());
        changed.provider_session_id = None;
        assert!(RuntimeTitleOwner::from_run(&changed).is_none());
        changed = run.clone();
        changed.activity = AgentActivity::Exited;
        assert!(title.for_run(&changed, Some(&source)).is_none());
        changed = run.clone();
        changed.connection_epoch = Some(2);
        assert!(title.for_run(&changed, Some(&source)).is_none());
        assert!(title.for_run(&run, None).is_none());
        let changed_source = source.with_connection_epoch(Some(2));
        assert!(title.for_run(&run, Some(&changed_source)).is_none());
        let mut changed_source = source;
        changed_source.canonical_path = "/other".into();
        assert!(title.for_run(&run, Some(&changed_source)).is_none());
    }

    #[test]
    fn title_metadata_cannot_bind_a_retained_run_to_a_replacement_epoch() {
        let run = title_run();
        let owner = RuntimeTitleOwner::from_run(&run).unwrap();
        let source = title_source(&run);
        let sessions = [title_session("exact-session", "codex", "Owned")];
        assert_eq!(session_title_for_owner(&owner, &source, &sessions).as_deref(), Some("Owned"));
        for epoch in [None, Some(2)] {
            let changed_source = source.clone().with_connection_epoch(epoch);
            assert!(session_title_for_owner(&owner, &changed_source, &sessions).is_none());
        }
        let mut local_run = run;
        local_run.connection_epoch = None;
        let local_owner = RuntimeTitleOwner::from_run(&local_run).unwrap();
        assert!(!local_owner.matches_source(&source));
        let mut local_source = source;
        local_source.backend = ExecutionBackendSignature::Local;
        assert!(local_owner.matches_source(&local_source));
        let mut other_worktree = local_source.clone();
        other_worktree.worktree_id = route().worktree_id;
        assert!(!local_owner.matches_source(&other_worktree));
    }

    #[test]
    fn terminal_title_cardinality_ignores_superseded_alias_and_drops_ended_hook_title() {
        let run = title_run();
        let source = title_source(&run);
        let tracker = mt_ai::SessionTracker::new();
        tracker.track_input_with_line_snapshot(7, "codex\r", None);
        let episode = tracker.weak_detection_episode(7);
        let mut registry = mt_ai::AgentRuntimeRegistry::default();
        let weak = mt_ai::AgentObservation {
            event_id: AgentEventId::new(), route: run.route.clone(), sequence: 1,
            connection_epoch: Some(1), weak_episode: episode, provider: run.provider.clone(),
            provider_session_id: None, process: None, activity: AgentActivity::Unknown,
            connectivity: AgentConnectivity::Live, confirmation: AgentConfirmation::LiveConfirmed,
            evidence: AgentEvidence::PtyActivity, received_at_unix_ms: 1,
        };
        let mt_ai::AgentApplyOutcome::Applied { run_id: alias, .. } = registry.observe(weak) else { panic!("weak rejected"); };
        registry.observe(mt_ai::AgentObservation {
            event_id: AgentEventId::new(), route: run.route.clone(), sequence: 2,
            connection_epoch: Some(1), weak_episode: episode, provider: run.provider.clone(),
            provider_session_id: run.provider_session_id.clone(), process: run.process,
            activity: AgentActivity::Done, connectivity: AgentConnectivity::Live,
            confirmation: AgentConfirmation::LiveConfirmed, evidence: AgentEvidence::Hook,
            received_at_unix_ms: 2,
        });
        assert!(registry.is_superseded_weak_alias(&alias));
        assert_eq!(registry.runs().count(), 2);
        let selected = super::super::ai::single_live_agent_for_route(&registry, &run.route).unwrap();
        let owner = RuntimeTitleOwner::from_run(selected).unwrap();
        let title = session_title_for_owner(&owner, &source, &[title_session("exact-session", "codex", "Exact title")]);
        assert_eq!(runtime_display_title(None, title.as_deref(), None, "shell", selected.run_id.as_str()), "Exact title");
        registry.observe_hook_exit(run.route.clone(), AgentEventId::new(), 3, Some(1), 3);
        assert!(super::super::ai::single_live_agent_for_route(&registry, &run.route).is_none());
        assert!(!super::super::ai::accepted_agent_projection(&registry, &run.route, false).live);
        assert!(registry.is_superseded_weak_alias(&alias));
        assert!(!registry.run(&alias).unwrap().activity.is_ended());
        let fallback = runtime_display_title(None, None, None, "shell", run.route.terminal_session_id.as_str());
        assert!(fallback.starts_with("shell ["));
        assert!(!fallback.contains("Exact title"));
    }

    #[test]
    fn runtime_title_prefers_manual_names_and_bounds_distinguishable_fallbacks() {
        assert_eq!(runtime_display_title(Some("  Manual\nname  "), Some("Session"), Some("Live"), "ssh", "terminal1"),
            "Manual name");
        assert_eq!(runtime_display_title(Some(" \n"), Some("Session"), Some("Live"), "ssh", "terminal1"),
            "Session");
        assert_eq!(runtime_display_title(None, None, Some("Live"), "ssh", "terminal1"), "Live");
        let first_id = format!("{}11111111-0000-4000-8000-000000000000", TerminalSessionId::PREFIX);
        let second_id = format!("{}22222222-0000-4000-8000-000000000000", TerminalSessionId::PREFIX);
        let first = runtime_display_title(None, None, None, "duplicate-ssh", &first_id);
        let second = runtime_display_title(None, None, None, "duplicate-ssh", &second_id);
        assert_ne!(first, second);
        assert!(first.contains("11111111"));
        assert_eq!(usable_runtime_title(&"x".repeat(200)).unwrap().len(), RUNTIME_TITLE_LIMIT);
        assert!(!usable_runtime_title("a\u{1b}b").unwrap().contains('\u{1b}'));
    }

    #[test]
    fn live_runtime_titles_exclude_provider_status_and_cwd_labels() {
        let claude = "claude".parse().unwrap();
        assert!(live_runtime_title(&claude, "\u{280b} Claude Code").is_none());
        assert!(live_runtime_title(&claude, "\u{2733} /repo").is_none());
        assert!(live_runtime_title(&claude, "root@host:~").is_none());
        assert_eq!(live_runtime_title(&claude, "\u{2733} Fix ownership").as_deref(), Some("Fix ownership"));
        let opencode = "opencode".parse().unwrap();
        assert_eq!(live_runtime_title(&opencode, "\u{280b} OC | Review changes").as_deref(), Some("Review changes"));
        let pi = "pi".parse().unwrap();
        assert!(live_runtime_title(&pi, "\u{03c0} > /repo").is_none());
        assert_eq!(live_runtime_title(&pi, "\u{03c0} : Fix tests").as_deref(), Some("Fix tests"));
    }

    #[test]
    fn acknowledged_waiting_and_done_are_not_renewed_by_unchanged_process_inventory() {
        for activity in [AgentActivity::Waiting, AgentActivity::Blocked, AgentActivity::Done, AgentActivity::Failed] {
            let run = title_run();
            let mut registry = mt_ai::AgentRuntimeRegistry::default();
            let outcome = registry.observe(mt_ai::AgentObservation {
                event_id: run.last_event_id.clone(), route: run.route.clone(),
                sequence: 1, connection_epoch: Some(1), provider: run.provider.clone(),
                weak_episode: None,
                provider_session_id: run.provider_session_id.clone(), process: run.process,
                activity, connectivity: AgentConnectivity::Live,
                confirmation: AgentConfirmation::LiveConfirmed, evidence: AgentEvidence::Hook,
                received_at_unix_ms: 100,
            });
            let mt_ai::AgentApplyOutcome::Applied { run_id, .. } = outcome else { panic!("Hook rejected"); };
            let acknowledged = registry.run(&run_id).unwrap().last_event_id.clone();
            for sequence in 2..=4 {
                registry.apply_process_inventory(mt_ai::AgentProcessInventoryObservation {
                    event_id: AgentEventId::new(), route: run.route.clone(), sequence,
                    connection_epoch: 1,
                    weak_episode: None,
                    processes: vec![mt_ai::AgentProcessObservation {
                        provider: run.provider.clone(), process: run.process.unwrap(),
                        activity: AgentActivity::Unknown,
                    }],
                    received_at_unix_ms: 100 + sequence as i64 * 2_000,
                }).unwrap();
                let current = registry.run(&run_id).unwrap();
                assert_eq!(current.activity, activity);
                assert_eq!(current.last_event_id, acknowledged);
                assert_eq!(current.received_at_unix_ms, 100);
                let projection = super::super::ai::accepted_agent_projection(&registry, &run.route, false);
                assert!(!agent_event_is_unread(Some(&acknowledged), &current.last_event_id, activity, projection.attention));
                assert!(projection.live);
            }
        }
    }

    #[test]
    fn activation_receipt_fences_route_but_allows_a_later_event_to_stay_unread() {
        let target = target_for_receipt(route());
        let receipt = AgentActivationReceipt::from_target(&target);
        assert!(activation_receipt_matches_target(&receipt, &target));

        let mut later_event = target.clone();
        later_event.last_event_id = AgentEventId::new();
        assert!(activation_receipt_matches_target(&receipt, &later_event));

        let mut reused_route = target;
        reused_route.route.terminal_incarnation_id = TerminalIncarnationId::new();
        assert!(!activation_receipt_matches_target(&receipt, &reused_route));
    }
}
