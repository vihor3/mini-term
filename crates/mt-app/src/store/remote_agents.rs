//! Generation-fenced remote agent inventory scheduling and projection.

use std::collections::HashSet;
use std::time::Duration;

use gpui::Context;
use mt_ai::{
    AgentActivity, AgentConnectivity, AgentConnectivityObservation, AgentProcessIdentity,
    AgentProcessInventoryObservation, AgentProcessObservation, AgentProvider, AgentRoute,
};
use mt_identity::AgentEventId;
use mt_ssh::{RemoteAgentCapability, RemoteAgentInventory, RemoteAgentRoute};

use crate::remote_ssh::{RemoteAgentInventoryError, connection_fingerprint};
use crate::tree::PaneStatus;

use super::AppStore;
use super::pure::find_pane_of_pty;
use super::remote_runtime::{RemoteRuntimePhase, allocate_generation};

pub const REMOTE_AGENT_POLL_INTERVAL: Duration = Duration::from_secs(2);
const REMOTE_AGENT_ACTIVITY_WINDOW: Duration = Duration::from_secs(3);
const EMPTY_INVENTORY_CONFIRMATIONS: u8 = 2;
const ERROR_SUMMARY_CHARS: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteAgentProbeCapability {
    Unknown,
    LinuxProc,
    Unsupported,
}

#[derive(Clone, Debug)]
pub struct RemoteAgentPollState {
    pub capability: RemoteAgentProbeCapability,
    pub connectivity: AgentConnectivity,
    pub process_count: usize,
    pub connection_epoch: u64,
    pub last_error: Option<String>,
    pub updated_at_unix_ms: Option<i64>,
    generation: u64,
    in_flight: bool,
    project_id: String,
    project_path: String,
    connection_id: String,
    connection_fingerprint: u64,
    route: AgentRoute,
    had_processes: bool,
    empty_successes: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RemoteAgentPollRequest {
    pty_id: u32,
    project_id: String,
    project_path: String,
    generation: u64,
    connection_id: String,
    connection_fingerprint: u64,
    route: AgentRoute,
    connection_epoch: u64,
}

#[derive(Clone)]
struct RemoteAgentCandidate {
    pty_id: u32,
    project_id: String,
    project_path: String,
    connection: mt_config::SshConnection,
    connection_fingerprint: u64,
    route: AgentRoute,
    connection_epoch: u64,
}

impl RemoteAgentPollState {
    pub(super) fn route(&self) -> &AgentRoute {
        &self.route
    }

    fn from_request(request: &RemoteAgentPollRequest) -> Self {
        Self {
            capability: RemoteAgentProbeCapability::Unknown,
            connectivity: AgentConnectivity::Live,
            process_count: 0,
            connection_epoch: request.connection_epoch,
            last_error: None,
            updated_at_unix_ms: None,
            generation: request.generation,
            in_flight: true,
            project_id: request.project_id.clone(),
            project_path: request.project_path.clone(),
            connection_id: request.connection_id.clone(),
            connection_fingerprint: request.connection_fingerprint,
            route: request.route.clone(),
            had_processes: false,
            empty_successes: 0,
        }
    }

    fn begin(&mut self, request: &RemoteAgentPollRequest) {
        self.generation = request.generation;
        self.in_flight = true;
        self.project_id.clone_from(&request.project_id);
        self.project_path.clone_from(&request.project_path);
        self.connection_id.clone_from(&request.connection_id);
        self.connection_fingerprint = request.connection_fingerprint;
        self.route.clone_from(&request.route);
        self.connection_epoch = request.connection_epoch;
    }

    fn owns(&self, request: &RemoteAgentPollRequest) -> bool {
        self.generation == request.generation
            && self.in_flight
            && self.project_id == request.project_id
            && self.project_path == request.project_path
            && self.connection_id == request.connection_id
            && self.connection_fingerprint == request.connection_fingerprint
            && self.route == request.route
            && self.connection_epoch == request.connection_epoch
    }
}

fn request_facts_match(
    request: &RemoteAgentPollRequest,
    state: Option<&RemoteAgentPollState>,
    current_project: Option<(&str, &str)>,
    current_connection_fingerprint: Option<u64>,
    current_route: Option<&AgentRoute>,
    runtime_owner: Option<(&AgentRoute, u64)>,
) -> bool {
    state.is_some_and(|state| state.owns(request))
        && current_project
            == Some((
                request.project_path.as_str(),
                request.connection_id.as_str(),
            ))
        && current_connection_fingerprint == Some(request.connection_fingerprint)
        && current_route == Some(&request.route)
        && runtime_owner == Some((&request.route, request.connection_epoch))
}

fn should_apply_process_inventory(
    had_processes: &mut bool,
    empty_successes: &mut u8,
    process_count: usize,
) -> bool {
    if process_count > 0 {
        *had_processes = true;
        *empty_successes = 0;
        return true;
    }
    if !*had_processes {
        *empty_successes = 0;
        return false;
    }
    *empty_successes = empty_successes.saturating_add(1);
    if *empty_successes < EMPTY_INVENTORY_CONFIRMATIONS {
        return false;
    }
    *had_processes = false;
    *empty_successes = 0;
    true
}

impl AppStore {
    pub fn remote_agent_poll_state(&self, pty_id: u32) -> Option<&RemoteAgentPollState> {
        self.remote_agent_polls.get(&pty_id)
    }

    pub(super) fn invalidate_remote_agent_connection(&mut self, connection_id: &str) {
        self.remote_agent_polls
            .retain(|_, state| state.connection_id != connection_id);
    }

    pub(super) fn remove_remote_agent_project(&mut self, project_id: &str) {
        self.remote_agent_polls
            .retain(|_, state| state.project_id != project_id);
    }

    pub(super) fn remove_remote_agent_terminal(&mut self, pty_id: u32) {
        self.remote_agent_polls.remove(&pty_id);
    }

    pub fn poll_remote_agents(&mut self, cx: &mut Context<Self>) {
        if !crate::ai::remote_agent_status_enabled() {
            self.remote_agent_polls.clear();
            return;
        }

        let terminal_routes = &self.terminal_routes;
        self.remote_agent_polls
            .retain(|pty_id, state| terminal_routes.get(pty_id) == Some(&state.route));

        let mut candidates = Vec::new();
        let mut refresh_runtime = Vec::new();
        for (pty_id, route) in self.terminal_routes.iter() {
            let Some((project_id, _)) = find_pane_of_pty(&self.project_states, *pty_id) else {
                continue;
            };
            let Some(project) = self.project(&project_id) else {
                continue;
            };
            let Some(connection_id) = project.ssh_connection_id.as_deref() else {
                continue;
            };
            let Some(connection) = self.remote_connection_of(&project_id) else {
                continue;
            };
            let Some(runtime) = self.remote_runtime_projects.get(&project_id) else {
                continue;
            };
            if runtime.phase != RemoteRuntimePhase::Ready {
                continue;
            }
            let Some(snapshot) = runtime.snapshot.as_ref() else {
                continue;
            };
            if snapshot.identity.execution_host_id != route.execution_host_id
                || snapshot.worktree_id != route.worktree_id
            {
                continue;
            }
            let expected_epoch = snapshot.identity.connection_epoch;
            if crate::remote_ssh::current_connection_epoch(connection_id) != Some(expected_epoch) {
                refresh_runtime.push((
                    project_id,
                    route.clone(),
                    expected_epoch,
                    connection_id.to_string(),
                ));
                continue;
            }
            candidates.push(RemoteAgentCandidate {
                pty_id: *pty_id,
                project_id,
                project_path: project.path.clone(),
                connection_fingerprint: connection_fingerprint(&connection),
                connection,
                route: route.clone(),
                connection_epoch: expected_epoch,
            });
        }

        let mut refreshed = HashSet::new();
        for (project_id, route, expected_epoch, connection_id) in refresh_runtime {
            let observed_epoch = crate::remote_ssh::current_connection_epoch(&connection_id)
                .or(Some(expected_epoch));
            self.mark_agent_connectivity(
                route,
                observed_epoch,
                AgentConnectivity::Disconnected,
                chrono::Utc::now().timestamp_millis(),
            );
            if refreshed.insert(project_id.clone()) {
                self.refresh_remote_runtime_for_agents(&project_id, cx);
            }
        }

        for candidate in candidates {
            let already_running =
                self.remote_agent_polls
                    .get(&candidate.pty_id)
                    .is_some_and(|state| {
                        state.in_flight
                            && state.project_id == candidate.project_id
                            && state.project_path == candidate.project_path
                            && state.connection_id == candidate.connection.id
                            && state.connection_fingerprint == candidate.connection_fingerprint
                            && state.route == candidate.route
                            && state.connection_epoch == candidate.connection_epoch
                    });
            if already_running {
                continue;
            }
            let Some(generation) = allocate_generation(&mut self.next_remote_agent_generation)
            else {
                if let Some(state) = self.remote_agent_polls.get_mut(&candidate.pty_id) {
                    state.last_error =
                        Some("remote agent request generation space exhausted".into());
                    state.connectivity = AgentConnectivity::Stale;
                    state.in_flight = false;
                }
                continue;
            };
            let request = RemoteAgentPollRequest {
                pty_id: candidate.pty_id,
                project_id: candidate.project_id,
                project_path: candidate.project_path,
                generation,
                connection_id: candidate.connection.id.clone(),
                connection_fingerprint: candidate.connection_fingerprint,
                route: candidate.route,
                connection_epoch: candidate.connection_epoch,
            };
            self.remote_agent_polls
                .entry(request.pty_id)
                .and_modify(|state| state.begin(&request))
                .or_insert_with(|| RemoteAgentPollState::from_request(&request));

            let task_request = request.clone();
            let connection = candidate.connection;
            let remote_route = RemoteAgentRoute {
                protocol_version: mt_ai::AGENT_RUNTIME_PROTOCOL_VERSION,
                execution_host_id: request.route.execution_host_id.clone(),
                worktree_id: request.route.worktree_id.clone(),
                tab_id: request.route.tab_id.clone(),
                pane_key: request.route.pane_key.clone(),
                terminal_session_id: request.route.terminal_session_id.clone(),
                terminal_incarnation_id: request.route.terminal_incarnation_id.clone(),
            };
            cx.spawn(async move |this, cx| {
                let outcome = cx
                    .background_executor()
                    .spawn(async move {
                        crate::remote_ssh::remote_agent_inventory(&connection, &remote_route)
                    })
                    .await;
                let _ = this.update(cx, |store, cx| {
                    store.finish_remote_agent_poll(task_request, outcome, cx)
                });
            })
            .detach();
        }
    }

    fn remote_agent_request_is_current(&self, request: &RemoteAgentPollRequest) -> bool {
        let current_project = self.project(&request.project_id).and_then(|project| {
            project
                .ssh_connection_id
                .as_deref()
                .map(|connection_id| (project.path.as_str(), connection_id))
        });
        let current_connection = self.remote_connection_of(&request.project_id);
        let runtime_owner = self
            .remote_runtime_projects
            .get(&request.project_id)
            .filter(|runtime| runtime.phase == RemoteRuntimePhase::Ready)
            .and_then(|runtime| runtime.snapshot.as_ref())
            .filter(|snapshot| {
                snapshot.identity.execution_host_id == request.route.execution_host_id
                    && snapshot.worktree_id == request.route.worktree_id
            })
            .map(|snapshot| (&request.route, snapshot.identity.connection_epoch));
        request_facts_match(
            request,
            self.remote_agent_polls.get(&request.pty_id),
            current_project,
            current_connection.as_ref().map(connection_fingerprint),
            self.terminal_routes.get(&request.pty_id),
            runtime_owner,
        )
    }

    fn finish_remote_agent_poll(
        &mut self,
        request: RemoteAgentPollRequest,
        outcome: Result<RemoteAgentInventory, RemoteAgentInventoryError>,
        cx: &mut Context<Self>,
    ) {
        if !self.remote_agent_request_is_current(&request) {
            return;
        }
        if let Some(state) = self.remote_agent_polls.get_mut(&request.pty_id) {
            state.in_flight = false;
        }
        let now = chrono::Utc::now().timestamp_millis();

        let inventory = match outcome {
            Ok(inventory)
                if inventory.connection_epoch == request.connection_epoch
                    && crate::remote_ssh::current_connection_epoch(&request.connection_id)
                        == Some(inventory.connection_epoch) =>
            {
                inventory
            }
            Ok(inventory) => {
                self.finish_remote_agent_error(
                    &request,
                    RemoteAgentInventoryError {
                        message: "remote agent result was superseded by a newer SSH connection"
                            .into(),
                        disconnected: true,
                    },
                    crate::remote_ssh::current_connection_epoch(&request.connection_id)
                        .or(Some(inventory.connection_epoch)),
                    now,
                    cx,
                );
                self.refresh_remote_runtime_for_agents(&request.project_id, cx);
                return;
            }
            Err(error) => {
                let epoch = crate::remote_ssh::current_connection_epoch(&request.connection_id)
                    .or(Some(request.connection_epoch));
                let should_refresh = error.disconnected && epoch != Some(request.connection_epoch);
                self.finish_remote_agent_error(&request, error, epoch, now, cx);
                if should_refresh {
                    self.refresh_remote_runtime_for_agents(&request.project_id, cx);
                }
                return;
            }
        };

        match inventory.capability {
            RemoteAgentCapability::Unsupported => {
                if let Some(state) = self.remote_agent_polls.get_mut(&request.pty_id) {
                    state.capability = RemoteAgentProbeCapability::Unsupported;
                    state.connectivity = AgentConnectivity::Live;
                    state.process_count = 0;
                    state.connection_epoch = inventory.connection_epoch;
                    state.last_error = None;
                    state.updated_at_unix_ms = Some(now);
                }
                self.mark_agent_connectivity(
                    request.route,
                    Some(inventory.connection_epoch),
                    AgentConnectivity::Live,
                    now,
                );
                cx.notify();
            }
            RemoteAgentCapability::LinuxProc => {
                self.finish_supported_inventory(request, inventory, now, cx);
            }
        }
    }

    fn finish_supported_inventory(
        &mut self,
        request: RemoteAgentPollRequest,
        inventory: RemoteAgentInventory,
        now: i64,
        cx: &mut Context<Self>,
    ) {
        let activity = if self
            .ai
            .perception()
            .tracker()
            .has_recent_output(request.pty_id, REMOTE_AGENT_ACTIVITY_WINDOW)
        {
            AgentActivity::Working
        } else {
            AgentActivity::Waiting
        };
        let processes = inventory
            .processes
            .iter()
            .map(|process| {
                let provider: AgentProvider = process
                    .provider
                    .as_str()
                    .parse()
                    .map_err(|_| "remote agent provider normalization failed".to_string())?;
                let process_identity = AgentProcessIdentity::new(process.pid, process.start_ticks)
                    .ok_or_else(|| "remote agent process identity was invalid".to_string())?;
                Ok(AgentProcessObservation {
                    provider,
                    process: process_identity,
                    activity,
                })
            })
            .collect::<Result<Vec<_>, String>>();
        let processes = match processes {
            Ok(processes) => processes,
            Err(error) => {
                self.finish_remote_agent_error(
                    &request,
                    RemoteAgentInventoryError {
                        message: error,
                        disconnected: false,
                    },
                    Some(inventory.connection_epoch),
                    now,
                    cx,
                );
                return;
            }
        };

        let should_apply = if let Some(state) = self.remote_agent_polls.get_mut(&request.pty_id) {
            state.capability = RemoteAgentProbeCapability::LinuxProc;
            state.connectivity = AgentConnectivity::Live;
            state.process_count = processes.len();
            state.connection_epoch = inventory.connection_epoch;
            state.last_error = None;
            state.updated_at_unix_ms = Some(now);
            should_apply_process_inventory(
                &mut state.had_processes,
                &mut state.empty_successes,
                processes.len(),
            )
        } else {
            false
        };

        let Some(sequence) = self.ai.next_event_sequence() else {
            if let Some(state) = self.remote_agent_polls.get_mut(&request.pty_id) {
                state.last_error = Some("agent event sequence space exhausted".into());
                state.connectivity = AgentConnectivity::Stale;
            }
            return;
        };
        if should_apply {
            let provider = processes.last().map(|process| process.provider.clone());
            let is_empty = processes.is_empty();
            if let Err(error) =
                self.agent_runtime
                    .apply_process_inventory(AgentProcessInventoryObservation {
                        event_id: AgentEventId::new(),
                        route: request.route.clone(),
                        sequence,
                        connection_epoch: inventory.connection_epoch,
                        processes,
                        received_at_unix_ms: now,
                    })
            {
                if let Some(state) = self.remote_agent_polls.get_mut(&request.pty_id) {
                    state.last_error = Some(format!("agent inventory was rejected: {error:?}"));
                    state.connectivity = AgentConnectivity::Stale;
                }
                return;
            }
            if is_empty {
                self.update_remote_agent_projection(request.pty_id, PaneStatus::Idle, None, cx);
            } else {
                let status = PaneStatus::from_str(activity.legacy_status())
                    .expect("agent activity has a legacy projection");
                self.update_remote_agent_projection(
                    request.pty_id,
                    status,
                    provider.as_ref().map(AgentProvider::as_str),
                    cx,
                );
            }
        } else {
            self.mark_agent_connectivity(
                request.route,
                Some(inventory.connection_epoch),
                AgentConnectivity::Live,
                now,
            );
        }
        cx.notify();
    }

    fn finish_remote_agent_error(
        &mut self,
        request: &RemoteAgentPollRequest,
        error: RemoteAgentInventoryError,
        connection_epoch: Option<u64>,
        now: i64,
        cx: &mut Context<Self>,
    ) {
        let connectivity = if error.disconnected {
            AgentConnectivity::Disconnected
        } else {
            AgentConnectivity::Stale
        };
        if let Some(state) = self.remote_agent_polls.get_mut(&request.pty_id) {
            state.connectivity = connectivity;
            state.last_error = Some(bounded_error(&error.message));
            state.updated_at_unix_ms = Some(now);
            if let Some(epoch) = connection_epoch {
                state.connection_epoch = epoch;
            }
        }
        self.mark_agent_connectivity(request.route.clone(), connection_epoch, connectivity, now);
        cx.notify();
    }

    fn mark_agent_connectivity(
        &mut self,
        route: AgentRoute,
        connection_epoch: Option<u64>,
        connectivity: AgentConnectivity,
        now: i64,
    ) {
        let Some(sequence) = self.ai.next_event_sequence() else {
            return;
        };
        let _ = self
            .agent_runtime
            .mark_connectivity(AgentConnectivityObservation {
                event_id: AgentEventId::new(),
                route,
                sequence,
                connection_epoch,
                connectivity,
                received_at_unix_ms: now,
            });
    }

    fn update_remote_agent_projection(
        &mut self,
        pty_id: u32,
        status: PaneStatus,
        provider: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        if self.ai.perception().hooks().is_hook_enabled(pty_id) {
            return;
        }
        crate::git_watch::set_ai_pane(
            pty_id,
            matches!(status, PaneStatus::AiWorking | PaneStatus::AiIdle),
        );
        for state in self.project_states.values_mut() {
            let updated = state
                .layouts_mut()
                .any(|layout| layout.update_status_by_pty(pty_id, status, false, provider));
            if updated {
                state.status = state.highest_status();
                cx.notify();
                break;
            }
        }
    }
}

fn bounded_error(message: &str) -> String {
    let mut chars = message.chars();
    let bounded = chars.by_ref().take(ERROR_SUMMARY_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{bounded}...")
    } else {
        bounded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt_identity::{
        ExecutionHostId, HostInstallId, PaneKey, RepoId, TabId, TerminalIncarnationId,
        TerminalSessionId, WorktreeId,
    };

    fn route() -> AgentRoute {
        let host = ExecutionHostId::derive("local", &HostInstallId::new());
        let repo = RepoId::derive(&host, "/repo/.git");
        AgentRoute {
            execution_host_id: host,
            worktree_id: WorktreeId::derive(&repo, "/repo", None),
            tab_id: TabId::new(),
            pane_key: PaneKey::new(),
            terminal_session_id: TerminalSessionId::new(),
            terminal_incarnation_id: TerminalIncarnationId::new(),
        }
    }

    fn request() -> RemoteAgentPollRequest {
        RemoteAgentPollRequest {
            pty_id: 7,
            project_id: "project".into(),
            project_path: "/repo".into(),
            generation: 3,
            connection_id: "connection".into(),
            connection_fingerprint: 9,
            route: route(),
            connection_epoch: 11,
        }
    }

    #[test]
    fn request_fence_rejects_each_changed_owner_fact() {
        let request = request();
        let state = RemoteAgentPollState::from_request(&request);
        assert!(request_facts_match(
            &request,
            Some(&state),
            Some(("/repo", "connection")),
            Some(9),
            Some(&request.route),
            Some((&request.route, 11)),
        ));
        let mut changed_state = state.clone();
        changed_state.generation += 1;
        assert!(!request_facts_match(
            &request,
            Some(&changed_state),
            Some(("/repo", "connection")),
            Some(9),
            Some(&request.route),
            Some((&request.route, 11)),
        ));
        assert!(!request_facts_match(
            &request,
            Some(&state),
            Some(("/other", "connection")),
            Some(9),
            Some(&request.route),
            Some((&request.route, 11)),
        ));
        assert!(!request_facts_match(
            &request,
            Some(&state),
            Some(("/repo", "other")),
            Some(9),
            Some(&request.route),
            Some((&request.route, 11)),
        ));
        assert!(!request_facts_match(
            &request,
            Some(&state),
            Some(("/repo", "connection")),
            Some(10),
            Some(&request.route),
            Some((&request.route, 11)),
        ));
        let mut other_route = request.route.clone();
        other_route.terminal_incarnation_id = TerminalIncarnationId::new();
        assert!(!request_facts_match(
            &request,
            Some(&state),
            Some(("/repo", "connection")),
            Some(9),
            Some(&other_route),
            Some((&request.route, 11)),
        ));
        assert!(!request_facts_match(
            &request,
            Some(&state),
            Some(("/repo", "connection")),
            Some(9),
            Some(&request.route),
            Some((&request.route, 12)),
        ));
    }

    #[test]
    fn empty_inventory_needs_two_confirmations_after_a_live_process() {
        let mut had = false;
        let mut misses = 0;
        assert!(!should_apply_process_inventory(&mut had, &mut misses, 0));
        assert!(should_apply_process_inventory(&mut had, &mut misses, 1));
        assert!(had);
        assert!(!should_apply_process_inventory(&mut had, &mut misses, 0));
        assert_eq!(misses, 1);
        assert!(should_apply_process_inventory(&mut had, &mut misses, 0));
        assert!(!had);
        assert_eq!(misses, 0);
    }

    #[test]
    fn error_summary_is_bounded_by_characters() {
        let source = "界".repeat(ERROR_SUMMARY_CHARS + 10);
        let summary = bounded_error(&source);
        assert_eq!(summary.chars().count(), ERROR_SUMMARY_CHARS + 3);
        assert!(summary.ends_with("..."));
    }
}
