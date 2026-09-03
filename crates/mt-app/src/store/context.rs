//! Worktree-scoped immutable projections for contextual UI surfaces.
//!
//! This module is the only UI-facing boundary that joins stable worktree,
//! terminal, and Agent identities. Callers receive display models and route
//! actions; they never reconstruct ownership from paths or runtime PTY ids.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::OsStr;

use gpui::{App, Context, Entity, Window};
use mt_ai::{AgentActivity, AgentConnectivity, AgentEvidence, AgentProvider, AgentRoute};
use mt_identity::{
    AgentEventId, AgentRunId, ExecutionHostId, PaneKey, TabId, TerminalIncarnationId,
    TerminalSessionId, WorktreeId,
};

use crate::pane::TerminalRecovery;

use super::{AppStore, RemoteAgentProbeCapability};

const DIAGNOSTIC_TEXT_LIMIT: usize = 512;

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
    pub connectivity: AgentConnectivity,
    pub evidence: AgentEvidence,
    pub received_at_unix_ms: i64,
    pub attention: bool,
    pub unread: bool,
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
                pane_label: self.pane_display_label(&project.id, pane),
                route: run.route.clone(),
                provider: run.provider.clone(),
                provider_session_id: run.provider_session_id.clone(),
                activity: run.activity,
                connectivity: run.connectivity,
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
        if panel.tab_id != target.route.tab_id
            || panel
                .layout
                .first_active_pane()
                .map(|pane| pane.id.as_str())
                != Some(target.pane_id.as_str())
        {
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
                    let agent = route
                        .as_ref()
                        .and_then(|route| self.agent_runtime.active_run_for_route(route))
                        .and_then(|run| self.resolve_agent_target(&run.run_id));
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
                    diagnostics.push(TerminalDiagnosticView {
                        project_id: project.id.clone(),
                        pane_id: pane.id.clone(),
                        pane_label: self.pane_display_label(&project.id, pane),
                        route,
                        recovery,
                        exited: terminal_exited
                            || pty_id.is_some_and(|pty_id| self.is_pty_exited(pty_id)),
                        backend_notice,
                        agent,
                        remote_agent,
                    });
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
            connectivity: AgentConnectivity::Live,
            evidence: AgentEvidence::Hook,
            received_at_unix_ms: 1,
            attention: false,
            unread: true,
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
