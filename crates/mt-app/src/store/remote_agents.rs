//! Generation-fenced remote agent inventory scheduling and projection.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui::Context;
use mt_ai::{
    AgentActivity, AgentConnectivity, AgentConnectivityObservation, AgentEvidence,
    AgentObservationIgnored, AgentProcessIdentity, AgentProcessInventoryObservation,
    AgentProcessObservation, AgentProvider, AgentRoute, AgentRuntimeRegistry, SessionTracker,
};
use mt_identity::AgentEventId;
use mt_ssh::{RemoteAgentCapability, RemoteAgentInventory, RemoteAgentRoute};

use crate::remote_ssh::{RemoteAgentInventoryError, connection_fingerprint};
use crate::tree::PaneStatus;

use super::AppStore;
use super::ai::accepted_agent_projection;
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

struct RemoteAgentRuntimeGap {
    project_id: String,
    route: AgentRoute,
    connection_id: String,
    fallback_epoch: Option<u64>,
    connectivity: AgentConnectivity,
    refresh_runtime: bool,
}

impl RemoteAgentPollState {
    pub(super) fn route(&self) -> &AgentRoute {
        &self.route
    }

    fn from_request(request: &RemoteAgentPollRequest, had_processes: bool) -> Self {
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
            had_processes,
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

fn has_process_attested_run_for_route(registry: &AgentRuntimeRegistry, route: &AgentRoute) -> bool {
    registry.runs().any(|state| {
        &state.route == route
            && state.evidence == AgentEvidence::ProcessAttested
            && state.process.is_some()
            && !state.activity.is_ended()
    })
}

fn active_route_connection_epoch(
    registry: &AgentRuntimeRegistry,
    route: &AgentRoute,
) -> Option<u64> {
    registry
        .runs()
        .filter(|state| &state.route == route && !state.activity.is_ended())
        .filter_map(|state| state.connection_epoch)
        .max()
}

fn active_route_connectivity_change_needed(
    registry: &AgentRuntimeRegistry,
    route: &AgentRoute,
    connection_epoch: Option<u64>,
    connectivity: AgentConnectivity,
) -> bool {
    registry
        .runs()
        .filter(|state| &state.route == route && !state.activity.is_ended())
        .any(|state| {
            state.connectivity != connectivity
                || connection_epoch.is_some_and(|epoch| state.connection_epoch != Some(epoch))
        })
}

fn non_ready_runtime_connectivity(phase: RemoteRuntimePhase) -> Option<AgentConnectivity> {
    match phase {
        RemoteRuntimePhase::Ready => None,
        RemoteRuntimePhase::Connecting => Some(AgentConnectivity::Disconnected),
        RemoteRuntimePhase::CompatibilityFallback | RemoteRuntimePhase::RebindDeferred => {
            Some(AgentConnectivity::Stale)
        }
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

fn apply_inventory_and_retire_tracking(
    registry: &mut AgentRuntimeRegistry,
    tracker: &SessionTracker,
    pty_id: u32,
    hook_enabled: bool,
    inventory: AgentProcessInventoryObservation,
) -> Result<(), AgentObservationIgnored> {
    let route = inventory.route.clone();
    let event_id = inventory.event_id.clone();
    registry.apply_process_inventory(inventory)?;
    let retired = registry.runs().any(|state| {
        state.route == route
            && state.last_event_id == event_id
            && state.evidence == AgentEvidence::ProcessAttested
            && state.activity == AgentActivity::Exited
    });
    let has_live_owner = registry.runs().any(|state| {
        state.route == route
            && !state.activity.is_ended()
            && state.evidence != AgentEvidence::RestoredHistory
    });
    if retired && !has_live_owner && !hook_enabled {
        tracker.clear_ai_session(pty_id);
    }
    Ok(())
}

pub(super) fn retire_terminal_polling(
    pty_id: u32,
    exited_ptys: &mut HashSet<u32>,
    polls: &mut HashMap<u32, RemoteAgentPollState>,
) {
    exited_ptys.insert(pty_id);
    polls.remove(&pty_id);
}

impl AppStore {
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
        let exited_ptys = &self.exited_ptys;
        self.remote_agent_polls.retain(|pty_id, state| {
            !exited_ptys.contains(pty_id) && terminal_routes.get(pty_id) == Some(&state.route)
        });

        let mut candidates = Vec::new();
        let mut runtime_gaps = Vec::new();
        for (pty_id, route) in self.terminal_routes.iter() {
            if self.exited_ptys.contains(pty_id) {
                continue;
            }
            let Some((project_id, _)) = find_pane_of_pty(&self.project_states, *pty_id) else {
                continue;
            };
            let Some(project) = self.project(&project_id) else {
                continue;
            };
            let Some(connection_id) = project.ssh_connection_id.as_deref() else {
                continue;
            };
            let route_epoch = active_route_connection_epoch(&self.agent_runtime, route);
            let Some(connection) = self.remote_connection_of(&project_id) else {
                runtime_gaps.push(RemoteAgentRuntimeGap {
                    project_id,
                    route: route.clone(),
                    connection_id: connection_id.to_string(),
                    fallback_epoch: route_epoch,
                    connectivity: AgentConnectivity::Disconnected,
                    refresh_runtime: false,
                });
                continue;
            };
            let Some(runtime) = self.remote_runtime_projects.get(&project_id) else {
                runtime_gaps.push(RemoteAgentRuntimeGap {
                    project_id,
                    route: route.clone(),
                    connection_id: connection_id.to_string(),
                    fallback_epoch: route_epoch,
                    connectivity: AgentConnectivity::Disconnected,
                    refresh_runtime: true,
                });
                continue;
            };
            if let Some(connectivity) = non_ready_runtime_connectivity(runtime.phase) {
                runtime_gaps.push(RemoteAgentRuntimeGap {
                    project_id,
                    route: route.clone(),
                    connection_id: connection_id.to_string(),
                    fallback_epoch: runtime
                        .snapshot
                        .as_ref()
                        .map(|snapshot| snapshot.identity.connection_epoch)
                        .or(route_epoch),
                    connectivity,
                    refresh_runtime: false,
                });
                continue;
            }
            let Some(snapshot) = runtime.snapshot.as_ref() else {
                runtime_gaps.push(RemoteAgentRuntimeGap {
                    project_id,
                    route: route.clone(),
                    connection_id: connection_id.to_string(),
                    fallback_epoch: route_epoch,
                    connectivity: AgentConnectivity::Stale,
                    refresh_runtime: true,
                });
                continue;
            };
            if snapshot.identity.execution_host_id != route.execution_host_id
                || snapshot.worktree_id != route.worktree_id
            {
                runtime_gaps.push(RemoteAgentRuntimeGap {
                    project_id,
                    route: route.clone(),
                    connection_id: connection_id.to_string(),
                    fallback_epoch: Some(snapshot.identity.connection_epoch),
                    connectivity: AgentConnectivity::Stale,
                    refresh_runtime: false,
                });
                continue;
            }
            let expected_epoch = snapshot.identity.connection_epoch;
            if crate::remote_ssh::current_connection_epoch(connection_id) != Some(expected_epoch) {
                runtime_gaps.push(RemoteAgentRuntimeGap {
                    project_id,
                    route: route.clone(),
                    connection_id: connection_id.to_string(),
                    fallback_epoch: Some(expected_epoch),
                    connectivity: AgentConnectivity::Disconnected,
                    refresh_runtime: true,
                });
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

        let notify_runtime_gaps = !runtime_gaps.is_empty();
        let mut refreshed = HashSet::new();
        let now = chrono::Utc::now().timestamp_millis();
        for gap in runtime_gaps {
            let observed_epoch = crate::remote_ssh::current_connection_epoch(&gap.connection_id)
                .or(gap.fallback_epoch);
            for state in self
                .remote_agent_polls
                .values_mut()
                .filter(|state| state.route == gap.route)
            {
                state.connectivity = gap.connectivity;
                state.in_flight = false;
                state.updated_at_unix_ms = Some(now);
                if let Some(epoch) = observed_epoch {
                    state.connection_epoch = epoch;
                }
            }
            self.mark_agent_connectivity(gap.route, observed_epoch, gap.connectivity, now);
            if gap.refresh_runtime && refreshed.insert(gap.project_id.clone()) {
                self.refresh_remote_runtime_for_agents(&gap.project_id, cx);
            }
        }
        if notify_runtime_gaps {
            cx.notify();
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
            let had_processes =
                has_process_attested_run_for_route(&self.agent_runtime, &request.route);
            self.remote_agent_polls
                .entry(request.pty_id)
                .and_modify(|state| state.begin(&request))
                .or_insert_with(|| RemoteAgentPollState::from_request(&request, had_processes));

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
        if self.exited_ptys.contains(&request.pty_id) {
            return false;
        }
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

        if should_apply {
            let Some(sequence) = self.ai.next_event_sequence() else {
                if let Some(state) = self.remote_agent_polls.get_mut(&request.pty_id) {
                    state.last_error = Some("agent event sequence space exhausted".into());
                    state.connectivity = AgentConnectivity::Stale;
                }
                return;
            };
            let is_empty = processes.is_empty();
            if let Err(error) = apply_inventory_and_retire_tracking(
                &mut self.agent_runtime,
                self.ai.perception().tracker(),
                request.pty_id,
                self.ai.perception().hooks().is_hook_enabled(request.pty_id),
                AgentProcessInventoryObservation {
                    event_id: AgentEventId::new(),
                    route: request.route.clone(),
                    sequence,
                    connection_epoch: inventory.connection_epoch,
                    processes,
                    received_at_unix_ms: now,
                },
            ) {
                if let Some(state) = self.remote_agent_polls.get_mut(&request.pty_id) {
                    state.last_error = Some(format!("agent inventory was rejected: {error:?}"));
                    state.connectivity = AgentConnectivity::Stale;
                    state.had_processes |=
                        has_process_attested_run_for_route(&self.agent_runtime, &request.route);
                }
                return;
            }
            if is_empty && let Some(state) = self.remote_agent_polls.get_mut(&request.pty_id) {
                state.had_processes =
                    has_process_attested_run_for_route(&self.agent_runtime, &request.route);
            }
            self.update_remote_agent_projection(request.pty_id, &request.route, cx);
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

    pub(super) fn mark_agent_connectivity(
        &mut self,
        route: AgentRoute,
        connection_epoch: Option<u64>,
        connectivity: AgentConnectivity,
        now: i64,
    ) {
        let connection_epoch =
            connection_epoch.or_else(|| active_route_connection_epoch(&self.agent_runtime, &route));
        if !active_route_connectivity_change_needed(
            &self.agent_runtime,
            &route,
            connection_epoch,
            connectivity,
        ) {
            return;
        }
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
        route: &AgentRoute,
        cx: &mut Context<Self>,
    ) {
        if self.exited_ptys.contains(&pty_id) || self.terminal_routes.get(&pty_id) != Some(route) {
            return;
        }
        let retained_attention = find_pane_of_pty(&self.project_states, pty_id)
            .and_then(|(owner, pane)| self.project_states.get(&owner)?.pane(&pane))
            .is_some_and(|pane| pane.attention);
        let projection = accepted_agent_projection(&self.agent_runtime, route, retained_attention);
        crate::git_watch::set_ai_pane(
            pty_id,
            matches!(
                projection.status,
                PaneStatus::AiWorking | PaneStatus::AiIdle
            ),
        );
        for state in self.project_states.values_mut() {
            let updated = state
                .layouts_mut()
                .any(|layout| projection.apply_to_layout(layout, pty_id));
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
        let state = RemoteAgentPollState::from_request(&request, false);
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
    fn recreated_poll_uses_exact_process_attested_registry_evidence() {
        let request = request();
        let process = AgentProcessIdentity::new(10, 20).unwrap();
        let mut registry = AgentRuntimeRegistry::default();
        registry
            .apply_process_inventory(AgentProcessInventoryObservation {
                event_id: AgentEventId::new(),
                route: request.route.clone(),
                sequence: 1,
                connection_epoch: request.connection_epoch,
                processes: vec![AgentProcessObservation {
                    provider: AgentProvider::CODEX.parse().unwrap(),
                    process,
                    activity: AgentActivity::Working,
                }],
                received_at_unix_ms: 1,
            })
            .unwrap();

        assert!(has_process_attested_run_for_route(
            &registry,
            &request.route
        ));
        assert_eq!(
            active_route_connection_epoch(&registry, &request.route),
            Some(request.connection_epoch)
        );
        let mut other_route = request.route.clone();
        other_route.terminal_incarnation_id = TerminalIncarnationId::new();
        assert!(!has_process_attested_run_for_route(&registry, &other_route));
        assert_eq!(active_route_connection_epoch(&registry, &other_route), None);

        let changed = registry
            .mark_connectivity(AgentConnectivityObservation {
                event_id: AgentEventId::new(),
                route: request.route.clone(),
                sequence: 2,
                connection_epoch: active_route_connection_epoch(&registry, &request.route),
                connectivity: AgentConnectivity::Disconnected,
                received_at_unix_ms: 2,
            })
            .unwrap();
        assert_eq!(changed, 1);
        assert_eq!(
            registry
                .active_run_for_route(&request.route)
                .unwrap()
                .connectivity,
            AgentConnectivity::Disconnected
        );

        let mut recreated = RemoteAgentPollState::from_request(
            &request,
            has_process_attested_run_for_route(&registry, &request.route),
        );
        assert!(!should_apply_process_inventory(
            &mut recreated.had_processes,
            &mut recreated.empty_successes,
            0,
        ));
        assert!(should_apply_process_inventory(
            &mut recreated.had_processes,
            &mut recreated.empty_successes,
            0,
        ));
    }

    fn inventory(
        request: &RemoteAgentPollRequest,
        sequence: u64,
        processes: Vec<AgentProcessObservation>,
    ) -> AgentProcessInventoryObservation {
        AgentProcessInventoryObservation {
            event_id: AgentEventId::new(),
            route: request.route.clone(),
            sequence,
            connection_epoch: request.connection_epoch,
            processes,
            received_at_unix_ms: sequence as i64,
        }
    }

    fn process(pid: u32) -> AgentProcessObservation {
        AgentProcessObservation {
            provider: AgentProvider::CODEX.parse().unwrap(),
            process: AgentProcessIdentity::new(pid, 100).unwrap(),
            activity: AgentActivity::Working,
        }
    }

    #[test]
    fn confirmed_retirement_clears_latch_without_blocking_a_new_launch() {
        let request = request();
        let mut registry = AgentRuntimeRegistry::default();
        let tracker = SessionTracker::new();
        tracker.track_input_with_line_snapshot(request.pty_id, "codex\r", None);
        apply_inventory_and_retire_tracking(
            &mut registry,
            &tracker,
            request.pty_id,
            false,
            inventory(&request, 1, vec![process(10)]),
        )
        .unwrap();
        let old_run = registry
            .active_run_for_route(&request.route)
            .unwrap()
            .run_id
            .clone();
        let mut poll = RemoteAgentPollState::from_request(
            &request,
            has_process_attested_run_for_route(&registry, &request.route),
        );
        assert!(!should_apply_process_inventory(
            &mut poll.had_processes,
            &mut poll.empty_successes,
            0,
        ));
        assert!(tracker.is_ai_session(request.pty_id));
        assert_eq!(
            accepted_agent_projection(&registry, &request.route, false).status,
            PaneStatus::AiWorking
        );
        assert!(should_apply_process_inventory(
            &mut poll.had_processes,
            &mut poll.empty_successes,
            0,
        ));
        apply_inventory_and_retire_tracking(
            &mut registry,
            &tracker,
            request.pty_id,
            false,
            inventory(&request, 3, vec![]),
        )
        .unwrap();
        tracker.note_output(request.pty_id, "ordinary shell output\r\n$ codex\r\n");
        assert!(!tracker.is_ai_session(request.pty_id));
        assert_eq!(
            mt_ai::monitor::resolve_status(&mt_ai::HookState::new(), &tracker, request.pty_id),
            "idle"
        );
        assert_eq!(
            accepted_agent_projection(&registry, &request.route, false).status,
            PaneStatus::Idle
        );
        tracker.track_input_with_line_snapshot(request.pty_id, "codex\r", None);
        assert!(tracker.is_ai_session(request.pty_id));
        apply_inventory_and_retire_tracking(
            &mut registry,
            &tracker,
            request.pty_id,
            false,
            inventory(&request, 4, vec![process(20)]),
        )
        .unwrap();
        assert_ne!(
            registry
                .active_run_for_route(&request.route)
                .unwrap()
                .run_id,
            old_run
        );
    }

    #[test]
    fn empty_inventory_does_not_clear_never_attested_or_other_live_runs() {
        let request = request();
        let mut registry = AgentRuntimeRegistry::default();
        let tracker = SessionTracker::new();
        tracker.mark_ai_session(request.pty_id, "codex");
        apply_inventory_and_retire_tracking(
            &mut registry,
            &tracker,
            request.pty_id,
            false,
            inventory(&request, 1, vec![]),
        )
        .unwrap();
        assert!(tracker.is_ai_session(request.pty_id));
        apply_inventory_and_retire_tracking(
            &mut registry,
            &tracker,
            request.pty_id,
            false,
            inventory(&request, 2, vec![process(10), process(20)]),
        )
        .unwrap();
        apply_inventory_and_retire_tracking(
            &mut registry,
            &tracker,
            request.pty_id,
            false,
            inventory(&request, 3, vec![process(20)]),
        )
        .unwrap();
        assert!(tracker.is_ai_session(request.pty_id));
        assert!(has_process_attested_run_for_route(
            &registry,
            &request.route
        ));

        registry.observe(mt_ai::AgentObservation {
            event_id: AgentEventId::new(),
            route: request.route.clone(),
            sequence: 4,
            connection_epoch: Some(request.connection_epoch),
            provider: AgentProvider::CODEX.parse().unwrap(),
            provider_session_id: Some("hook-session".into()),
            process: Some(process(20).process),
            activity: AgentActivity::Blocked,
            connectivity: AgentConnectivity::Live,
            confirmation: mt_ai::AgentConfirmation::LiveConfirmed,
            evidence: AgentEvidence::Hook,
            received_at_unix_ms: 4,
        });
        apply_inventory_and_retire_tracking(
            &mut registry,
            &tracker,
            request.pty_id,
            false,
            inventory(&request, 5, vec![]),
        )
        .unwrap();
        assert!(tracker.is_ai_session(request.pty_id));
        let projection = accepted_agent_projection(&registry, &request.route, true);
        assert_eq!(projection.status, PaneStatus::AiWorking);
        assert!(projection.attention);
        assert_eq!(projection.evidence, AgentEvidence::Hook);
    }

    #[test]
    fn ignored_inventory_items_project_accepted_hook_state() {
        let request = request();
        let mut registry = AgentRuntimeRegistry::default();
        let tracker = SessionTracker::new();
        let mut other = process(20);
        other.provider = "claude".parse().unwrap();
        apply_inventory_and_retire_tracking(
            &mut registry,
            &tracker,
            request.pty_id,
            false,
            inventory(&request, 9, vec![process(10), other.clone()]),
        )
        .unwrap();
        registry.observe(mt_ai::AgentObservation {
            event_id: AgentEventId::new(),
            route: request.route.clone(),
            sequence: 11,
            connection_epoch: Some(request.connection_epoch),
            provider: AgentProvider::CODEX.parse().unwrap(),
            provider_session_id: Some("hook-session".into()),
            process: Some(process(10).process),
            activity: AgentActivity::Done,
            connectivity: AgentConnectivity::Live,
            confirmation: mt_ai::AgentConfirmation::LiveConfirmed,
            evidence: AgentEvidence::Hook,
            received_at_unix_ms: 11,
        });
        apply_inventory_and_retire_tracking(
            &mut registry,
            &tracker,
            request.pty_id,
            false,
            inventory(&request, 10, vec![process(10), other]),
        )
        .unwrap();
        let hook = registry
            .runs()
            .find(|run| run.evidence == AgentEvidence::Hook)
            .unwrap();
        assert_eq!(hook.last_sequence, 11);
        assert_eq!(hook.activity, AgentActivity::Done);
        assert_eq!(registry.runs().count(), 2);
        let projection = accepted_agent_projection(&registry, &request.route, false);
        assert_eq!(projection.status, PaneStatus::AiWorking);
        assert_eq!(projection.provider, None);
    }

    #[test]
    fn retired_terminal_polling_fences_completion_and_preserves_other_owners() {
        let request = request();
        let mut registry = AgentRuntimeRegistry::default();
        registry
            .apply_process_inventory(inventory(&request, 1, vec![process(10)]))
            .unwrap();
        let mut other = request.clone();
        other.pty_id += 1;
        other.route.terminal_incarnation_id = mt_identity::TerminalIncarnationId::new();
        let mut polls = HashMap::from([
            (
                request.pty_id,
                RemoteAgentPollState::from_request(&request, true),
            ),
            (
                other.pty_id,
                RemoteAgentPollState::from_request(&other, false),
            ),
        ]);
        let mut exited_ptys = HashSet::new();
        retire_terminal_polling(request.pty_id, &mut exited_ptys, &mut polls);
        retire_terminal_polling(request.pty_id, &mut exited_ptys, &mut polls);
        assert_eq!(exited_ptys, HashSet::from([request.pty_id]));
        assert_eq!(polls.len(), 1);
        assert!(polls.get(&other.pty_id).unwrap().owns(&other));
        registry
            .mark_connectivity(AgentConnectivityObservation {
                event_id: AgentEventId::new(),
                route: request.route.clone(),
                sequence: 2,
                connection_epoch: active_route_connection_epoch(&registry, &request.route),
                connectivity: AgentConnectivity::Disconnected,
                received_at_unix_ms: 2,
            })
            .unwrap();
        assert!(!request_facts_match(
            &request,
            polls.get(&request.pty_id),
            Some((
                request.project_path.as_str(),
                request.connection_id.as_str()
            )),
            Some(request.connection_fingerprint),
            Some(&request.route),
            Some((&request.route, request.connection_epoch)),
        ));
        let run = registry.active_run_for_route(&request.route).unwrap();
        assert_eq!(run.activity, AgentActivity::Working);
        assert_eq!(run.connectivity, AgentConnectivity::Disconnected);
        assert!(!active_route_connectivity_change_needed(
            &registry,
            &request.route,
            Some(request.connection_epoch),
            AgentConnectivity::Disconnected,
        ));
    }

    #[test]
    fn connectivity_change_requires_an_active_route_and_changed_state() {
        let request = request();
        let process = AgentProcessIdentity::new(10, 20).unwrap();
        let mut registry = AgentRuntimeRegistry::default();
        assert!(!active_route_connectivity_change_needed(
            &registry,
            &request.route,
            Some(request.connection_epoch),
            AgentConnectivity::Disconnected,
        ));
        registry
            .apply_process_inventory(AgentProcessInventoryObservation {
                event_id: AgentEventId::new(),
                route: request.route.clone(),
                sequence: 1,
                connection_epoch: request.connection_epoch,
                processes: vec![AgentProcessObservation {
                    provider: AgentProvider::CODEX.parse().unwrap(),
                    process,
                    activity: AgentActivity::Waiting,
                }],
                received_at_unix_ms: 1,
            })
            .unwrap();

        assert!(active_route_connectivity_change_needed(
            &registry,
            &request.route,
            Some(request.connection_epoch),
            AgentConnectivity::Disconnected,
        ));
        registry
            .mark_connectivity(AgentConnectivityObservation {
                event_id: AgentEventId::new(),
                route: request.route.clone(),
                sequence: 2,
                connection_epoch: Some(request.connection_epoch),
                connectivity: AgentConnectivity::Disconnected,
                received_at_unix_ms: 2,
            })
            .unwrap();
        assert!(!active_route_connectivity_change_needed(
            &registry,
            &request.route,
            Some(request.connection_epoch),
            AgentConnectivity::Disconnected,
        ));
        assert!(active_route_connectivity_change_needed(
            &registry,
            &request.route,
            Some(request.connection_epoch + 1),
            AgentConnectivity::Disconnected,
        ));
        assert!(active_route_connectivity_change_needed(
            &registry,
            &request.route,
            Some(request.connection_epoch),
            AgentConnectivity::Live,
        ));
    }

    #[test]
    fn non_ready_runtime_never_leaves_agent_connectivity_live() {
        assert_eq!(
            non_ready_runtime_connectivity(RemoteRuntimePhase::Ready),
            None
        );
        assert_eq!(
            non_ready_runtime_connectivity(RemoteRuntimePhase::Connecting),
            Some(AgentConnectivity::Disconnected)
        );
        assert_eq!(
            non_ready_runtime_connectivity(RemoteRuntimePhase::CompatibilityFallback),
            Some(AgentConnectivity::Stale)
        );
        assert_eq!(
            non_ready_runtime_connectivity(RemoteRuntimePhase::RebindDeferred),
            Some(AgentConnectivity::Stale)
        );
    }

    #[test]
    fn error_summary_is_bounded_by_characters() {
        let source = "界".repeat(ERROR_SUMMARY_CHARS + 10);
        let summary = bounded_error(&source);
        assert_eq!(summary.chars().count(), ERROR_SUMMARY_CHARS + 3);
        assert!(summary.ends_with("..."));
    }
}
