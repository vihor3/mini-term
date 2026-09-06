//! Generation-fenced remote agent inventory scheduling and projection.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use gpui::Context;
use mt_ai::{
    AgentActivity, AgentApplyOutcome, AgentConfirmation, AgentConnectivity,
    AgentConnectivityObservation, AgentEvidence, AgentObservationIgnored, AgentProcessIdentity,
    AgentProcessInventoryObservation, AgentProcessObservation, AgentProvider, AgentRoute,
    AgentRuntimeRegistry, AgentSemanticObservation, AgentSemanticOwner, AgentWeakEpisode,
    SessionTracker,
};
use mt_identity::{AgentEventId, AgentRunId};
use mt_ssh::{RemoteAgentCapability, RemoteAgentInventory, RemoteAgentRoute};

use crate::execution_host::{ExecutionBackendSignature, ExecutionSourceSignature};
use crate::pane::{CapturedCodexScreen, CodexScreenOwner, PtyCodexScreen};
use crate::remote_ssh::{RemoteAgentInventoryError, connection_fingerprint};

use super::AppStore;
use super::ai::accepted_agent_projection;
use super::pure::find_pane_of_pty;
use super::remote_runtime::{RemoteRuntimePhase, allocate_generation};

pub const REMOTE_AGENT_POLL_INTERVAL: Duration = Duration::from_secs(2);
const EMPTY_INVENTORY_CONFIRMATIONS: u8 = 2;
const ERROR_SUMMARY_CHARS: usize = 512;
const FOREGROUND_SAMPLE_MAX_AGE_MS: i64 = 4_000;

#[derive(Clone, Debug)]
struct PendingAgentTitle {
    run_id: AgentRunId,
    provider: AgentProvider,
    process: AgentProcessIdentity,
    source: ExecutionSourceSignature,
    title: String,
    observed_at_unix_ms: i64,
}

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
    foreground: Option<mt_ssh::RemoteAgentProcess>,
    foreground_observed_at_unix_ms: Option<i64>,
    pending_title: Option<PendingAgentTitle>,
    codex_screen: Option<Arc<parking_lot::Mutex<PtyCodexScreen>>>,
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
    requested_at_unix_ms: i64,
    weak_episode: Option<AgentWeakEpisode>,
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
            foreground: None,
            foreground_observed_at_unix_ms: None,
            pending_title: None,
            codex_screen: None,
        }
    }

    fn begin(&mut self, request: &RemoteAgentPollRequest) {
        if self.route != request.route
            || self.connection_epoch != request.connection_epoch
            || self.project_path != request.project_path
            || self.connection_fingerprint != request.connection_fingerprint
            || self.connection_id != request.connection_id
        {
            self.clear_title_owner();
        }
        self.generation = request.generation;
        self.in_flight = true;
        self.project_id.clone_from(&request.project_id);
        self.project_path.clone_from(&request.project_path);
        self.connection_id.clone_from(&request.connection_id);
        self.connection_fingerprint = request.connection_fingerprint;
        self.route.clone_from(&request.route);
        self.connection_epoch = request.connection_epoch;
    }

    fn clear_title_owner(&mut self) {
        self.foreground = None;
        self.foreground_observed_at_unix_ms = None;
        self.pending_title = None;
        if let Some(screen) = &self.codex_screen {
            screen.lock().bind_owner(None);
        }
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
        .filter(|state| !registry.is_superseded_weak_alias(&state.run_id))
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
        .filter(|state| !registry.is_superseded_weak_alias(&state.run_id))
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
    let weak_episode = inventory.weak_episode;
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
            && !registry.is_superseded_weak_alias(&state.run_id)
    });
    if retired && !has_live_owner && !hook_enabled {
        tracker.clear_ai_session_if_episode(pty_id, weak_episode);
    }
    Ok(())
}

pub(super) fn retire_terminal_polling(
    pty_id: u32,
    exited_ptys: &mut HashSet<u32>,
    polls: &mut HashMap<u32, RemoteAgentPollState>,
) {
    exited_ptys.insert(pty_id);
    if let Some(mut poll) = polls.remove(&pty_id) {
        poll.clear_title_owner();
    }
}

fn registered_agent_routes<'a>(
    routes: &'a HashMap<u32, AgentRoute>,
    exited: &'a HashSet<u32>,
) -> impl Iterator<Item = (&'a u32, &'a AgentRoute)> {
    routes.iter().filter(|(pty_id, _)| !exited.contains(pty_id))
}

fn process_observations(
    processes: &[mt_ssh::RemoteAgentProcess],
) -> Result<Vec<AgentProcessObservation>, String> {
    processes
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
                activity: AgentActivity::Unknown,
            })
        })
        .collect()
}

fn unique_foreground(
    processes: &[mt_ssh::RemoteAgentProcess],
) -> Option<mt_ssh::RemoteAgentProcess> {
    let mut foreground = processes.iter().filter(|process| process.foreground);
    let process = *foreground.next()?;
    foreground.next().is_none().then_some(process)
}

fn pending_title_owner(
    registry: &AgentRuntimeRegistry,
    poll: &RemoteAgentPollState,
    source: ExecutionSourceSignature,
    title: &str,
    observed_at_unix_ms: i64,
    now: i64,
) -> Option<PendingAgentTitle> {
    if title.is_empty() || title.len() > 1024 || title.chars().any(char::is_control) {
        return None;
    }
    let owner = foreground_semantic_owner(registry, poll, source, observed_at_unix_ms, now)?;
    Some(PendingAgentTitle {
        run_id: owner.run_id,
        provider: owner.provider,
        process: owner.process,
        source: owner.source,
        title: title.to_string(),
        observed_at_unix_ms,
    })
}

fn foreground_semantic_owner(
    registry: &AgentRuntimeRegistry,
    poll: &RemoteAgentPollState,
    source: ExecutionSourceSignature,
    observed_at_unix_ms: i64,
    now: i64,
) -> Option<CodexScreenOwner> {
    let sampled_at = poll.foreground_observed_at_unix_ms?;
    if !(0..=FOREGROUND_SAMPLE_MAX_AGE_MS).contains(&observed_at_unix_ms.saturating_sub(sampled_at))
        || !(0..=mt_ai::AGENT_SEMANTIC_MAX_AGE_MS)
            .contains(&now.saturating_sub(observed_at_unix_ms))
        || poll.capability != RemoteAgentProbeCapability::LinuxProc
        || poll.connectivity != AgentConnectivity::Live
        || poll.last_error.is_some()
        || source.execution_host_id != poll.route.execution_host_id
        || source.worktree_id != poll.route.worktree_id
        || !matches!(&source.backend, ExecutionBackendSignature::Ssh {
            connection_id, connection_fingerprint, connection_epoch,
        } if connection_id == &poll.connection_id
            && *connection_fingerprint == poll.connection_fingerprint
            && *connection_epoch == Some(poll.connection_epoch))
    {
        return None;
    }
    let foreground = poll.foreground?;
    if !foreground.foreground {
        return None;
    }
    let process = AgentProcessIdentity::new(foreground.pid, foreground.start_ticks)?;
    let provider: AgentProvider = foreground.provider.as_str().parse().ok()?;
    let mut runs = registry.runs().filter(|run| {
        run.route == poll.route
            && run.provider == provider
            && run.process == Some(process)
            && !run.activity.is_ended()
            && run.confirmation == AgentConfirmation::LiveConfirmed
            && run.evidence >= AgentEvidence::ProcessAttested
            && run.connectivity == AgentConnectivity::Live
            && run.connection_epoch == Some(poll.connection_epoch)
            && !registry.is_superseded_weak_alias(&run.run_id)
    });
    let run_id = runs.next()?.run_id.clone();
    if runs.next().is_some() {
        return None;
    }
    Some(CodexScreenOwner {
        route: poll.route.clone(),
        run_id,
        provider,
        process,
        source,
        sampled_at_unix_ms: sampled_at,
    })
}

fn confirmed_title_owner(
    foreground: Option<mt_ssh::RemoteAgentProcess>,
    pending: &PendingAgentTitle,
    now: i64,
) -> bool {
    foreground.is_some_and(|foreground| {
        AgentProcessIdentity::new(foreground.pid, foreground.start_ticks) == Some(pending.process)
            && foreground.provider.as_str() == pending.provider.as_str()
            && foreground.foreground
    }) && (0..=mt_ai::AGENT_SEMANTIC_MAX_AGE_MS)
        .contains(&now.saturating_sub(pending.observed_at_unix_ms))
}

fn take_title_after_sample(
    pending: &mut Option<PendingAgentTitle>,
    foreground: Option<mt_ssh::RemoteAgentProcess>,
    requested_at_unix_ms: i64,
    now: i64,
) -> Option<PendingAgentTitle> {
    let title = pending.as_ref()?;
    if !confirmed_title_owner(foreground, title, now) {
        *pending = None;
        return None;
    }
    // A late reply may contain a snapshot taken before the title. Keep the
    // original capture until a request scheduled strictly after it confirms it.
    if requested_at_unix_ms <= title.observed_at_unix_ms {
        return None;
    }
    pending.take()
}

fn apply_codex_screen(
    registry: &mut AgentRuntimeRegistry,
    screen: &mut PtyCodexScreen,
    owner: Option<CodexScreenOwner>,
    request: &RemoteAgentPollRequest,
    sequence: u64,
    now: i64,
) -> Option<AgentApplyOutcome> {
    let owner = owner.filter(|owner| {
        owner.route == request.route
            && matches!(&owner.source.backend, ExecutionBackendSignature::Ssh {
                connection_id, connection_fingerprint, connection_epoch,
            } if connection_id == &request.connection_id
                && *connection_fingerprint == request.connection_fingerprint
                && *connection_epoch == Some(request.connection_epoch))
    });
    screen.bind_owner(owner.clone());
    let CapturedCodexScreen {
        owner,
        evidence,
        observed_at_unix_ms,
        ..
    } = screen.take_confirmed(owner.as_ref(), request.requested_at_unix_ms, now)?;
    Some(registry.observe_semantic(AgentSemanticObservation {
        event_id: AgentEventId::new(),
        run_id: owner.run_id,
        route: owner.route,
        provider: owner.provider,
        owner: AgentSemanticOwner::ForegroundProcess(owner.process),
        sequence,
        connection_epoch: Some(request.connection_epoch),
        activity: evidence.activity,
        observed_at_unix_ms,
        received_at_unix_ms: now,
    }))
}

impl AppStore {
    pub(super) fn observe_terminal_title(
        &mut self,
        pty_id: u32,
        route: &AgentRoute,
        title: &str,
        observed_at_unix_ms: i64,
        cx: &mut Context<Self>,
    ) {
        if self.exited_ptys.contains(&pty_id) || self.terminal_routes.get(&pty_id) != Some(route) {
            return;
        }
        if title.is_empty() {
            self.clear_runtime_live_title(route);
            if let Some(poll) = self.remote_agent_polls.get_mut(&pty_id) {
                poll.pending_title = None;
            }
            cx.notify();
            return;
        }
        let Some(poll) = self
            .remote_agent_polls
            .get(&pty_id)
            .filter(|poll| &poll.route == route)
        else {
            return;
        };
        if self
            .project(&poll.project_id)
            .is_none_or(|project| project.path != poll.project_path)
            || crate::remote_ssh::current_connection_epoch(&poll.connection_id)
                != Some(poll.connection_epoch)
        {
            return;
        }
        let Ok(snapshot) = self.project_execution_snapshot(&poll.project_id) else {
            return;
        };
        let pending = pending_title_owner(
            &self.agent_runtime,
            poll,
            snapshot.source_signature(),
            title,
            observed_at_unix_ms,
            chrono::Utc::now().timestamp_millis(),
        );
        if let Some(poll) = self.remote_agent_polls.get_mut(&pty_id) {
            poll.pending_title = pending;
        }
    }

    pub(super) fn invalidate_remote_agent_connection(&mut self, connection_id: &str) {
        self.remote_agent_polls.retain(|_, state| {
            let keep = state.connection_id != connection_id;
            if !keep {
                state.clear_title_owner();
            }
            keep
        });
    }

    pub(super) fn remove_remote_agent_project(&mut self, project_id: &str) {
        self.remote_agent_polls.retain(|_, state| {
            let keep = state.project_id != project_id;
            if !keep {
                state.clear_title_owner();
            }
            keep
        });
    }

    pub(super) fn remove_remote_agent_terminal(&mut self, pty_id: u32) {
        if let Some(mut poll) = self.remote_agent_polls.remove(&pty_id) {
            poll.clear_title_owner();
        }
    }

    pub fn poll_remote_agents(&mut self, cx: &mut Context<Self>) {
        if !crate::ai::remote_agent_status_enabled() {
            for poll in self.remote_agent_polls.values_mut() {
                poll.clear_title_owner();
            }
            self.remote_agent_polls.clear();
            return;
        }

        let terminal_routes = &self.terminal_routes;
        let exited_ptys = &self.exited_ptys;
        self.remote_agent_polls.retain(|pty_id, state| {
            let keep = !exited_ptys.contains(pty_id)
                && terminal_routes.get(pty_id) == Some(&state.route);
            if !keep {
                state.clear_title_owner();
            }
            keep
        });

        let mut candidates = Vec::new();
        let mut runtime_gaps = Vec::new();
        for (pty_id, route) in registered_agent_routes(&self.terminal_routes, &self.exited_ptys) {
            if !self.terminals.contains_key(pty_id) {
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
                state.clear_title_owner();
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
                    state.clear_title_owner();
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
                requested_at_unix_ms: chrono::Utc::now().timestamp_millis(),
                weak_episode: self
                    .ai
                    .perception()
                    .tracker()
                    .weak_detection_episode(candidate.pty_id),
            };
            let had_processes =
                has_process_attested_run_for_route(&self.agent_runtime, &request.route);
            self.remote_agent_polls
                .entry(request.pty_id)
                .and_modify(|state| state.begin(&request))
                .or_insert_with(|| RemoteAgentPollState::from_request(&request, had_processes));
            if let Some(poll) = self.remote_agent_polls.get_mut(&request.pty_id) {
                poll.codex_screen = self
                    .terminals
                    .get(&request.pty_id)
                    .and_then(|terminal| terminal.read(cx).codex_screen());
            }

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
                    state.clear_title_owner();
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
        let processes = process_observations(&inventory.processes);
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

        let foreground = unique_foreground(&inventory.processes);
        let should_apply = if let Some(state) = self.remote_agent_polls.get_mut(&request.pty_id) {
            state.foreground = None;
            state.foreground_observed_at_unix_ms = None;
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
                    state.clear_title_owner();
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
                    weak_episode: request.weak_episode,
                    processes,
                    received_at_unix_ms: now,
                },
            ) {
                if let Some(state) = self.remote_agent_polls.get_mut(&request.pty_id) {
                    state.clear_title_owner();
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
            let pending_title =
                self.remote_agent_polls
                    .get_mut(&request.pty_id)
                    .and_then(|state| {
                        state.foreground = foreground;
                        state.foreground_observed_at_unix_ms = foreground.map(|_| now);
                        take_title_after_sample(
                            &mut state.pending_title,
                            foreground,
                            request.requested_at_unix_ms,
                            now,
                        )
                    });
            if let Some(pending) = pending_title {
                self.apply_confirmed_title(&request, foreground, pending, now);
            }
            self.confirm_codex_screen(&request, now);
            self.update_remote_agent_projection(request.pty_id, &request.route, cx);
        } else {
            if let Some(state) = self.remote_agent_polls.get_mut(&request.pty_id) {
                state.clear_title_owner();
            }
            self.mark_agent_connectivity(
                request.route,
                Some(inventory.connection_epoch),
                AgentConnectivity::Live,
                now,
            );
        }
        cx.notify();
    }

    fn confirm_codex_screen(&mut self, request: &RemoteAgentPollRequest, now: i64) {
        let Some(poll) = self.remote_agent_polls.get(&request.pty_id) else {
            return;
        };
        let Some(screen) = poll.codex_screen.clone() else {
            return;
        };
        let owner = self
            .project_execution_snapshot(&request.project_id)
            .ok()
            .and_then(|snapshot| {
                foreground_semantic_owner(
                    &self.agent_runtime,
                    poll,
                    snapshot.source_signature(),
                    now,
                    now,
                )
            });
        // Allocate outside the reader lock. Acceptance is synchronous while the
        // buffer is locked, so already observed contradictions cannot race it.
        let Some(sequence) = self.ai.next_event_sequence() else {
            screen.lock().bind_owner(None);
            return;
        };
        let _ = apply_codex_screen(
            &mut self.agent_runtime,
            &mut screen.lock(),
            owner,
            request,
            sequence,
            now,
        );
    }

    fn apply_confirmed_title(
        &mut self,
        request: &RemoteAgentPollRequest,
        foreground: Option<mt_ssh::RemoteAgentProcess>,
        pending: PendingAgentTitle,
        now: i64,
    ) {
        if !confirmed_title_owner(foreground, &pending, now)
            || self
                .project_execution_snapshot(&request.project_id)
                .ok()
                .is_none_or(|snapshot| snapshot.source_signature() != pending.source)
        {
            return;
        }
        if let Some(activity) = mt_ai::activity_from_owned_title(&pending.provider, &pending.title)
        {
            let Some(sequence) = self.ai.next_event_sequence() else {
                return;
            };
            let outcome = self
                .agent_runtime
                .observe_semantic(AgentSemanticObservation {
                    event_id: AgentEventId::new(),
                    run_id: pending.run_id.clone(),
                    route: request.route.clone(),
                    provider: pending.provider.clone(),
                    owner: AgentSemanticOwner::ForegroundProcess(pending.process),
                    sequence,
                    connection_epoch: Some(request.connection_epoch),
                    activity,
                    observed_at_unix_ms: pending.observed_at_unix_ms,
                    received_at_unix_ms: now,
                });
            if !matches!(
                outcome,
                AgentApplyOutcome::Applied { .. }
                    | AgentApplyOutcome::Ignored(AgentObservationIgnored::StrongerEvidence)
            ) {
                return;
            }
        }
        self.record_runtime_live_title(
            &pending.run_id,
            &request.route,
            pending.process,
            pending.source,
            &pending.title,
        );
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
            state.clear_title_owner();
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
        crate::git_watch::set_ai_pane(pty_id, projection.live);
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
    use crate::tree::PaneStatus;
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
            requested_at_unix_ms: 0,
            weak_episode: None,
        }
    }

    #[test]
    fn monitoring_keeps_registered_background_routes_and_excludes_exited_owners() {
        let foreground = route();
        let background = route();
        let exited_route = route();
        let external_route = route();
        let routes = HashMap::from([
            (1, foreground.clone()),
            (2, background.clone()),
            (3, exited_route),
        ]);
        let exited = HashSet::from([3]);
        let monitored = registered_agent_routes(&routes, &exited)
            .map(|(pty_id, route)| (*pty_id, route))
            .collect::<HashMap<_, _>>();
        assert_eq!(monitored.len(), 2);
        assert_eq!(monitored.get(&1), Some(&&foreground));
        assert_eq!(monitored.get(&2), Some(&&background));
        assert!(!monitored.values().any(|route| **route == external_route));
    }

    #[test]
    fn process_inventory_carries_liveness_not_shared_terminal_activity() {
        let processes = vec![
            mt_ssh::RemoteAgentProcess {
                provider: mt_ssh::RemoteAgentProvider::Codex,
                pid: 10,
                start_ticks: 100,
                foreground: true,
            },
            mt_ssh::RemoteAgentProcess {
                provider: mt_ssh::RemoteAgentProvider::Claude,
                pid: 20,
                start_ticks: 200,
                foreground: false,
            },
        ];
        let tracker = SessionTracker::new();
        tracker.track_input_with_line_snapshot(7, "codex\r", None);
        for output in ["shell output\r\n", "\u{1b}[2J", "waiting for input\r\n"] {
            tracker.note_output(7, output);
            let observations = process_observations(&processes).unwrap();
            assert_eq!(observations.len(), 2);
            assert!(
                observations
                    .iter()
                    .all(|process| process.activity == AgentActivity::Unknown)
            );
            assert_eq!(observations[0].process.pid, 10);
            assert_eq!(observations[1].process.pid, 20);
        }
    }

    fn title_fixture() -> (
        RemoteAgentPollRequest,
        RemoteAgentPollState,
        AgentRuntimeRegistry,
        ExecutionSourceSignature,
    ) {
        let request = request();
        let mut poll = RemoteAgentPollState::from_request(&request, true);
        poll.capability = RemoteAgentProbeCapability::LinuxProc;
        poll.foreground = Some(mt_ssh::RemoteAgentProcess {
            provider: mt_ssh::RemoteAgentProvider::Pi,
            pid: 10,
            start_ticks: 100,
            foreground: true,
        });
        poll.foreground_observed_at_unix_ms = Some(100);
        let mut registry = AgentRuntimeRegistry::default();
        let mut observed = process(10);
        observed.provider = "pi".parse().unwrap();
        observed.process = AgentProcessIdentity::new(10, 100).unwrap();
        let mut inventory = inventory(&request, 1, vec![observed]);
        inventory.received_at_unix_ms = 100;
        registry.apply_process_inventory(inventory).unwrap();
        let source = ExecutionSourceSignature {
            execution_host_id: request.route.execution_host_id.clone(),
            root_project_id: request.project_id.clone(),
            root_source_path: request.project_path.clone(),
            worktree_id: request.route.worktree_id.clone(),
            canonical_path: request.project_path.clone(),
            backend: ExecutionBackendSignature::Ssh {
                connection_id: request.connection_id.clone(),
                connection_fingerprint: request.connection_fingerprint,
                connection_epoch: Some(request.connection_epoch),
            },
        };
        (request, poll, registry, source)
    }

    fn codex_fixture() -> (
        RemoteAgentPollRequest,
        RemoteAgentPollState,
        AgentRuntimeRegistry,
        ExecutionSourceSignature,
        mt_terminal::TerminalEmulator,
        PtyCodexScreen,
    ) {
        let (request, mut poll, _, source) = title_fixture();
        poll.foreground.as_mut().unwrap().provider = mt_ssh::RemoteAgentProvider::Codex;
        let mut registry = AgentRuntimeRegistry::default();
        let processes = process_observations(&[poll.foreground.unwrap()]).unwrap();
        let mut observed = inventory(&request, 1, processes);
        observed.received_at_unix_ms = 100;
        registry.apply_process_inventory(observed).unwrap();
        let mut screen = PtyCodexScreen::default();
        screen.bind_owner(foreground_semantic_owner(&registry, &poll, source.clone(), 100, 100));
        (request, poll, registry, source,
            mt_terminal::TerminalEmulator::new(mt_terminal::TermSize::new(100, 30)), screen)
    }

    fn codex_working(seconds: u32) -> Vec<u8> {
        crate::pane::codex_test_frame(&format!("Working ({seconds}s \u{2022} esc to interrupt)"))
    }

    fn confirm_codex_fixture(
        request: &RemoteAgentPollRequest,
        poll: &mut RemoteAgentPollState,
        registry: &mut AgentRuntimeRegistry,
        source: &ExecutionSourceSignature,
        screen: &mut PtyCodexScreen,
        sequence: u64,
        now: i64,
    ) -> Option<AgentApplyOutcome> {
        let mut observed = inventory(request, sequence,
            process_observations(&poll.foreground.into_iter().collect::<Vec<_>>()).unwrap());
        observed.received_at_unix_ms = now;
        registry.apply_process_inventory(observed).unwrap();
        poll.foreground_observed_at_unix_ms = poll.foreground.map(|_| now);
        let owner = foreground_semantic_owner(registry, poll, source.clone(), now, now);
        apply_codex_screen(registry, screen, owner, request, sequence + 1, now)
    }

    #[test]
    fn codex_native_frame_reaches_bracketed_registry_and_accepted_projection() {
        let (mut request, mut poll, mut registry, source, emulator, mut screen) = codex_fixture();
        let run_id = registry.active_run_for_route(&request.route).unwrap().run_id.clone();
        screen.observe_output(&emulator, &codex_working(2), 110);
        request.requested_at_unix_ms = 109;
        assert!(confirm_codex_fixture(&request, &mut poll, &mut registry, &source, &mut screen, 2, 120).is_none());
        assert_eq!(registry.run(&run_id).unwrap().activity, AgentActivity::Unknown);
        request.requested_at_unix_ms = 121;
        assert!(matches!(confirm_codex_fixture(&request, &mut poll, &mut registry, &source, &mut screen, 4, 130), Some(AgentApplyOutcome::Applied { .. })));
        assert_eq!(registry.run(&run_id).unwrap().activity, AgentActivity::Working);
        let projection = accepted_agent_projection(&registry, &request.route, false);
        assert!(projection.live);
        assert_eq!(projection.status, PaneStatus::AiWorking);
        assert_eq!(registry.activity_freshness(&run_id, 130), mt_ai::AgentActivityFreshness::Fresh);
        // Neither an idle composer nor later liveness can manufacture Waiting/Done.
        screen.observe_output(&emulator, &crate::pane::codex_test_frame(""), 140);
        request.requested_at_unix_ms = 150;
        assert!(confirm_codex_fixture(&request, &mut poll, &mut registry, &source, &mut screen, 6, 160).is_none());
        assert_eq!(registry.run(&run_id).unwrap().activity, AgentActivity::Working);
        assert_eq!(registry.activity_freshness(&run_id, 15_111), mt_ai::AgentActivityFreshness::Stale);
    }

    #[test]
    fn codex_counter_redraws_keep_earliest_pending_capture_without_starvation() {
        let (mut request, mut poll, mut registry, source, emulator, mut screen) = codex_fixture();
        screen.observe_output(&emulator, &codex_working(2), 110);
        request.requested_at_unix_ms = 111;
        for seconds in 3..50 {
            screen.observe_output(&emulator, &codex_working(seconds), 110 + i64::from(seconds));
        }
        assert!(matches!(confirm_codex_fixture(&request, &mut poll, &mut registry, &source, &mut screen, 2, 200), Some(AgentApplyOutcome::Applied { .. })));
        let run_id = registry.active_run_for_route(&request.route).unwrap().run_id.clone();
        assert_eq!(registry.activity_freshness(&run_id, 15_111), mt_ai::AgentActivityFreshness::Stale);
        screen.observe_output(&emulator, &codex_working(49), 210);
        screen.observe_output(&emulator, b"\x1b]2;unrelated title\x07\x1b[5n", 220);
        request.requested_at_unix_ms = 230;
        assert!(confirm_codex_fixture(&request, &mut poll, &mut registry, &source, &mut screen, 4, 240).is_none());
        screen.observe_output(&emulator, &codex_working(50), 250);
        request.requested_at_unix_ms = 251;
        assert!(matches!(confirm_codex_fixture(&request, &mut poll, &mut registry, &source, &mut screen, 6, 260), Some(AgentApplyOutcome::Applied { .. })));
    }

    #[test]
    fn codex_split_controls_footer_and_spinner_do_not_renew_retained_working() {
        let (mut request, mut poll, mut registry, source, emulator, mut screen) = codex_fixture();
        screen.bind_owner(None);
        screen.observe_output(&emulator, &codex_working(2), 90);
        screen.bind_owner(foreground_semantic_owner(&registry, &poll, source.clone(), 100, 100));
        screen.observe_output(&emulator, b"\x1b[", 110);
        screen.observe_output(&emulator, b"0m", 111);
        request.requested_at_unix_ms = 112;
        assert!(confirm_codex_fixture(&request, &mut poll, &mut registry, &source, &mut screen, 2, 120).is_none());
        let frame = codex_working(3);
        for (index, byte) in frame.iter().enumerate() {
            screen.observe_output(&emulator, &[*byte], 130 + index as i64);
        }
        request.requested_at_unix_ms = 500;
        assert!(matches!(confirm_codex_fixture(&request, &mut poll, &mut registry, &source, &mut screen, 4, 510), Some(AgentApplyOutcome::Applied { .. })));
        let run_id = registry.active_run_for_route(&request.route).unwrap().run_id.clone();
        let accepted = registry.run(&run_id).unwrap().clone();
        screen.observe_output(&emulator, &crate::pane::codex_test_frame("\u{25e6} Working (3s \u{2022} esc to interrupt)"), 520);
        screen.observe_output(&emulator, "\x1b[5;1Hgpt-6-astra high \u{00b7} ~/another\x1b[K\x1b[3;3H".as_bytes(), 530);
        screen.observe_output(&emulator, b"\x1b[", 540);
        screen.observe_output(&emulator, b"0m", 550);
        request.requested_at_unix_ms = 560;
        assert!(confirm_codex_fixture(&request, &mut poll, &mut registry, &source, &mut screen, 6, 570).is_none());
        assert_eq!(registry.run(&run_id).unwrap().last_event_id, accepted.last_event_id);
        assert_eq!(registry.run(&run_id).unwrap().received_at_unix_ms, accepted.received_at_unix_ms);
        assert_eq!(registry.activity_freshness(&run_id, 15_500), mt_ai::AgentActivityFreshness::Stale);
    }

    #[test]
    fn codex_cursor_only_validity_changes_do_not_refresh_retained_marker_cells() {
        for (prepare, reveal) in [
            (b"\x1b[?25l".as_slice(), b"\x1b[?25h".as_slice()),
            (b"\x1b[1;1H".as_slice(), b"\x1b[3;3H".as_slice()),
            (b"\x1b[29;1H".as_slice(), b"\x1b[3;3H".as_slice()),
        ] {
            let (mut request, mut poll, mut registry, source, emulator, mut screen) =
                codex_fixture();
            screen.bind_owner(None);
            screen.observe_output(&emulator, &codex_working(2), 90);
            screen.observe_output(&emulator, prepare, 91);
            screen.bind_owner(foreground_semantic_owner(
                &registry, &poll, source.clone(), 100, 100,
            ));
            screen.observe_output(&emulator, reveal, 110);
            request.requested_at_unix_ms = 111;
            assert!(confirm_codex_fixture(
                &request, &mut poll, &mut registry, &source, &mut screen, 2, 120,
            ).is_none());
            let run_id = registry.active_run_for_route(&request.route).unwrap().run_id.clone();
            assert_eq!(registry.run(&run_id).unwrap().activity, AgentActivity::Unknown);
            assert_eq!(registry.activity_freshness(&run_id, 120), mt_ai::AgentActivityFreshness::Unknown);
            screen.observe_output(&emulator, &codex_working(3), 130);
            request.requested_at_unix_ms = 131;
            assert!(matches!(confirm_codex_fixture(
                &request, &mut poll, &mut registry, &source, &mut screen, 4, 140,
            ), Some(AgentApplyOutcome::Applied { .. })));
            assert_eq!(registry.run(&run_id).unwrap().activity, AgentActivity::Working);
        }
    }

    #[test]
    fn codex_owner_loss_invalidates_unfinished_frame_before_same_run_rebind() {
        let (mut request, mut poll, mut registry, source, emulator, mut screen) = codex_fixture();
        let frame = codex_working(2);
        screen.observe_output(&emulator, &frame[..8], 110);
        screen.bind_owner(None);
        screen.observe_output(&emulator, &frame[8..], 120);
        request.requested_at_unix_ms = 121;
        assert!(confirm_codex_fixture(
            &request, &mut poll, &mut registry, &source, &mut screen, 2, 130,
        ).is_none());
        screen.observe_output(&emulator, b"\x1b[", 140);
        screen.observe_output(&emulator, b"0m", 141);
        request.requested_at_unix_ms = 142;
        assert!(confirm_codex_fixture(
            &request, &mut poll, &mut registry, &source, &mut screen, 4, 150,
        ).is_none());
        let run_id = registry.active_run_for_route(&request.route).unwrap().run_id.clone();
        assert_eq!(registry.run(&run_id).unwrap().activity, AgentActivity::Unknown);
        screen.observe_output(&emulator, &codex_working(3), 160);
        request.requested_at_unix_ms = 161;
        assert!(matches!(confirm_codex_fixture(
            &request, &mut poll, &mut registry, &source, &mut screen, 6, 170,
        ), Some(AgentApplyOutcome::Applied { .. })));
        assert_eq!(registry.run(&run_id).unwrap().activity, AgentActivity::Working);
    }

    #[test]
    fn codex_partial_counter_keeps_earliest_capture_but_cannot_confirm_mid_frame() {
        let (mut request, mut poll, mut registry, source, emulator, mut screen) = codex_fixture();
        screen.observe_output(&emulator, &codex_working(2), 110);
        request.requested_at_unix_ms = 111;
        let frame = codex_working(3);
        screen.observe_output(&emulator, &frame[..frame.len() - 8], 120);
        assert!(confirm_codex_fixture(&request, &mut poll, &mut registry, &source, &mut screen, 2, 130).is_none());
        screen.observe_output(&emulator, &frame[frame.len() - 8..], 140);
        request.requested_at_unix_ms = 141;
        assert!(matches!(confirm_codex_fixture(&request, &mut poll, &mut registry, &source, &mut screen, 4, 150), Some(AgentApplyOutcome::Applied { .. })));
        let run_id = registry.active_run_for_route(&request.route).unwrap().run_id.clone();
        assert_eq!(registry.activity_freshness(&run_id, 15_111), mt_ai::AgentActivityFreshness::Stale);
    }

    #[test]
    fn codex_absence_partial_frames_and_older_output_cannot_publish_obsolete_working() {
        for contradiction in [crate::pane::codex_test_frame(""), crate::pane::codex_test_frame("Please approve"), b"\x1b[2J".to_vec()] {
            let (mut request, mut poll, mut registry, source, emulator, mut screen) = codex_fixture();
            screen.observe_output(&emulator, &codex_working(2), 110);
            screen.observe_output(&emulator, &contradiction, 120);
            request.requested_at_unix_ms = 111;
            assert!(confirm_codex_fixture(&request, &mut poll, &mut registry, &source, &mut screen, 2, 130).is_none());
            assert_eq!(registry.active_run_for_route(&request.route).unwrap().activity, AgentActivity::Unknown);
        }
        let (mut request, mut poll, mut registry, source, emulator, mut screen) = codex_fixture();
        screen.observe_output(&emulator, &codex_working(2), 110);
        screen.observe_output(&emulator, b"\x1b[?2026h\x1b[2J", 120);
        request.requested_at_unix_ms = 111;
        assert!(confirm_codex_fixture(&request, &mut poll, &mut registry, &source, &mut screen, 2, 130).is_none());
        screen.observe_output(&emulator, b"\x1b[?2026l", 140);
        request.requested_at_unix_ms = 141;
        assert!(confirm_codex_fixture(&request, &mut poll, &mut registry, &source, &mut screen, 4, 150).is_none());
        screen.observe_output(&emulator, &codex_working(3), 160);
        screen.observe_output(&emulator, &codex_working(4), 159);
        request.requested_at_unix_ms = 161;
        assert!(confirm_codex_fixture(&request, &mut poll, &mut registry, &source, &mut screen, 6, 170).is_none());
    }

    #[test]
    fn codex_pre_attestation_retained_grid_restore_and_scroll_are_not_new_evidence() {
        let (mut request, mut poll, mut registry, source, emulator, mut screen) = codex_fixture();
        screen.bind_owner(None);
        screen.observe_output(&emulator, &codex_working(2), 90);
        let snapshot = emulator.snapshot().unwrap();
        screen.bind_owner(foreground_semantic_owner(&registry, &poll, source.clone(), 100, 100));
        screen.observe_output(&emulator, b"\x1b[5n", 110);
        emulator.set_scrollback(500);
        emulator.restore_snapshot(&snapshot).unwrap();
        emulator.with_term_mut(|term| term.scroll_display(mt_terminal::alacritty_terminal::grid::Scroll::Top));
        screen.observe_output(&emulator, b"\x1b]2;Working\x07", 120);
        request.requested_at_unix_ms = 121;
        assert!(confirm_codex_fixture(&request, &mut poll, &mut registry, &source, &mut screen, 2, 130).is_none());
        screen.observe_output(&emulator, &codex_working(3), 140);
        request.requested_at_unix_ms = 141;
        assert!(matches!(confirm_codex_fixture(&request, &mut poll, &mut registry, &source, &mut screen, 4, 150), Some(AgentApplyOutcome::Applied { .. })));
    }

    #[test]
    fn codex_captures_reject_replaced_foreground_route_source_epoch_and_run() {
        for field in 0..8 {
            let (request, poll, registry, source, emulator, mut screen) = codex_fixture();
            screen.observe_output(&emulator, &codex_working(2), 110);
            let mut owner = foreground_semantic_owner(&registry, &poll, source, 120, 120).unwrap();
            match field {
                0 => owner.process.pid += 1,
                1 => owner.process.start_ticks += 1,
                2 => owner.route.terminal_incarnation_id = TerminalIncarnationId::new(),
                3 => owner.source = owner.source.with_connection_epoch(Some(12)),
                4 => owner.run_id = AgentRunId::new(),
                5 => owner.provider = "claude".parse().unwrap(),
                6 => owner.source.canonical_path = "/other".into(),
                _ => owner.route.worktree_id = route().worktree_id,
            }
            assert!(screen.take_confirmed(Some(&owner), 111, 130).is_none());
            assert_eq!(registry.active_run_for_route(&request.route).unwrap().activity, AgentActivity::Unknown);
        }
    }

    #[test]
    fn codex_owned_screen_never_overrides_hook_or_updates_independent_runs() {
        let (mut request, mut poll, mut registry, source, emulator, mut screen) = codex_fixture();
        let foreground = poll.foreground.unwrap();
        let background = mt_ssh::RemoteAgentProcess { pid: 20, start_ticks: 200, foreground: false, ..foreground };
        let mut observed = inventory(&request, 2, process_observations(&[foreground, background]).unwrap());
        observed.received_at_unix_ms = 101;
        registry.apply_process_inventory(observed).unwrap();
        let run_id = registry.runs().find(|run| run.process.unwrap().pid == 10).unwrap().run_id.clone();
        screen.observe_output(&emulator, &codex_working(2), 110);
        request.requested_at_unix_ms = 111;
        let owner = foreground_semantic_owner(&registry, &poll, source.clone(), 120, 120);
        assert!(matches!(apply_codex_screen(&mut registry, &mut screen, owner, &request, 3, 120), Some(AgentApplyOutcome::Applied { .. })));
        assert_eq!(registry.runs().find(|run| run.process.unwrap().pid == 20).unwrap().activity, AgentActivity::Unknown);
        assert!(matches!(registry.observe(mt_ai::AgentObservation {
            event_id: AgentEventId::new(), route: request.route.clone(), provider: "codex".parse().unwrap(),
            provider_session_id: Some("exact-hook".into()), process: Some(AgentProcessIdentity::new(10, 100).unwrap()),
            weak_episode: None, activity: AgentActivity::Waiting, connectivity: AgentConnectivity::Live,
            confirmation: AgentConfirmation::LiveConfirmed, evidence: AgentEvidence::Hook,
            sequence: 4, connection_epoch: Some(request.connection_epoch), received_at_unix_ms: 130,
        }), AgentApplyOutcome::Applied { .. }));
        screen.observe_output(&emulator, &codex_working(3), 140);
        request.requested_at_unix_ms = 141;
        let owner = foreground_semantic_owner(&registry, &poll, source.clone(), 150, 150);
        assert_eq!(apply_codex_screen(&mut registry, &mut screen, owner, &request, 5, 150), Some(AgentApplyOutcome::Ignored(AgentObservationIgnored::StrongerEvidence)));
        assert_eq!(registry.run(&run_id).unwrap().activity, AgentActivity::Waiting);
        assert_eq!(accepted_agent_projection(&registry, &request.route, false).status, PaneStatus::AiIdle);
        poll.foreground = Some(background);
        assert!(foreground_semantic_owner(&registry, &poll, source, 150, 150).is_none());
        assert!(unique_foreground(&[foreground, mt_ssh::RemoteAgentProcess { foreground: true, ..background }]).is_none());
    }

    #[test]
    fn title_semantics_require_fresh_bracketed_foreground_ownership() {
        let (request, poll, mut registry, source) = title_fixture();
        let title = "\u{03c0} : owned task";
        let pending =
            pending_title_owner(&registry, &poll, source.clone(), title, 110, 120).unwrap();
        assert!(confirmed_title_owner(poll.foreground, &pending, 200));
        let mut changed = poll.foreground.unwrap();
        changed.start_ticks += 1;
        assert!(!confirmed_title_owner(Some(changed), &pending, 200));
        assert!(!confirmed_title_owner(None, &pending, 200));
        assert!(!confirmed_title_owner(poll.foreground, &pending, 20_000));
        assert!(unique_foreground(&[poll.foreground.unwrap(), changed]).is_none());
        assert!(pending_title_owner(&registry, &poll, source.clone(), title, 99, 120).is_none());
        assert!(pending_title_owner(&registry, &poll, source.clone(), title, 110, 109).is_none());
        assert!(
            pending_title_owner(&registry, &poll, source.clone(), title, 110, 20_000).is_none()
        );
        assert!(
            pending_title_owner(
                &registry,
                &poll,
                source.with_connection_epoch(Some(12)),
                title,
                110,
                120
            )
            .is_none()
        );
        let before = registry.run(&pending.run_id).unwrap();
        assert_eq!(before.activity, AgentActivity::Unknown);
        let outcome = registry.observe_semantic(AgentSemanticObservation {
            event_id: AgentEventId::new(),
            run_id: pending.run_id.clone(),
            route: request.route,
            provider: pending.provider.clone(),
            owner: AgentSemanticOwner::ForegroundProcess(pending.process),
            sequence: 2,
            connection_epoch: Some(request.connection_epoch),
            activity: mt_ai::activity_from_owned_title(&pending.provider, &pending.title).unwrap(),
            observed_at_unix_ms: pending.observed_at_unix_ms,
            received_at_unix_ms: 200,
        });
        assert!(matches!(outcome, AgentApplyOutcome::Applied { .. }));
        assert_eq!(
            registry.run(&pending.run_id).unwrap().activity,
            AgentActivity::Working
        );
        assert_eq!(
            registry.activity_freshness(&pending.run_id, 20_000),
            mt_ai::AgentActivityFreshness::Stale
        );
    }

    #[test]
    fn epoch_change_discards_pending_title_and_previous_foreground_sample() {
        let (mut request, mut poll, registry, source) = title_fixture();
        poll.pending_title = pending_title_owner(&registry, &poll, source, "owned", 110, 120);
        assert!(poll.pending_title.is_some());
        request.connection_epoch += 1;
        poll.begin(&request);
        assert!(poll.pending_title.is_none());
        assert!(poll.foreground.is_none());
        assert!(poll.foreground_observed_at_unix_ms.is_none());
    }

    #[test]
    fn title_confirmation_requires_a_poll_scheduled_after_original_capture() {
        let (_, poll, registry, source) = title_fixture();
        let mut pending = pending_title_owner(&registry, &poll, source, "owned", 110, 120);
        assert!(take_title_after_sample(&mut pending, poll.foreground, 100, 200).is_none());
        assert_eq!(pending.as_ref().unwrap().observed_at_unix_ms, 110);
        assert!(take_title_after_sample(&mut pending, poll.foreground, 110, 210).is_none());
        let confirmed = take_title_after_sample(&mut pending, poll.foreground, 211, 300).unwrap();
        assert_eq!(confirmed.observed_at_unix_ms, 110);
        assert!(pending.is_none());
    }

    #[test]
    fn pending_title_is_discarded_on_changed_owner_or_expired_capture() {
        let (_, poll, registry, source) = title_fixture();
        for (foreground, now) in [(None, 200), (poll.foreground, 20_000)] {
            let mut pending =
                pending_title_owner(&registry, &poll, source.clone(), "owned", 110, 120);
            assert!(take_title_after_sample(&mut pending, foreground, 100, now).is_none());
            assert!(pending.is_none());
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
                weak_episode: request.weak_episode,
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
            weak_episode: request.weak_episode,
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
        let mut request = request();
        let mut registry = AgentRuntimeRegistry::default();
        let tracker = SessionTracker::new();
        tracker.track_input_with_line_snapshot(request.pty_id, "codex\r", None);
        request.weak_episode = tracker.weak_detection_episode(request.pty_id);
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
            PaneStatus::Idle
        );
        assert!(accepted_agent_projection(&registry, &request.route, false).live);
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
        request.weak_episode = tracker.weak_detection_episode(request.pty_id);
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

    fn retirement_preserves_later_input(pending_echo: bool) {
        let mut request = request();
        let mut registry = AgentRuntimeRegistry::default();
        let tracker = SessionTracker::new();
        tracker.track_input_with_line_snapshot(request.pty_id, "codex\r", None);
        request.weak_episode = tracker.weak_detection_episode(request.pty_id);
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
        let mut poll = RemoteAgentPollState::from_request(&request, true);
        assert!(!should_apply_process_inventory(
            &mut poll.had_processes,
            &mut poll.empty_successes,
            0
        ));

        tracker.track_input_with_line_snapshot(request.pty_id, "\x04", None);
        tracker.track_input_with_line_snapshot(
            request.pty_id,
            if pending_echo {
                "launcher\r"
            } else {
                "codex\r"
            },
            None,
        );
        if pending_echo {
            assert_eq!(
                tracker.weak_detection_episode(request.pty_id),
                request.weak_episode
            );
            assert!(!tracker.is_ai_session(request.pty_id));
        } else {
            assert!(tracker.weak_detection_episode(request.pty_id) > request.weak_episode);
        }
        assert!(should_apply_process_inventory(
            &mut poll.had_processes,
            &mut poll.empty_successes,
            0
        ));
        apply_inventory_and_retire_tracking(
            &mut registry,
            &tracker,
            request.pty_id,
            false,
            inventory(&request, 3, vec![]),
        )
        .unwrap();
        assert_eq!(
            registry.run(&old_run).unwrap().activity,
            AgentActivity::Exited
        );
        if pending_echo {
            tracker.note_output(request.pty_id, "PS D:\\project> codex\r\n");
        }
        let next_episode = tracker.weak_detection_episode(request.pty_id);
        assert!(next_episode > request.weak_episode);
        assert_eq!(
            tracker.ai_session_agent(request.pty_id).as_deref(),
            Some("codex")
        );
        let outcome = registry.observe(mt_ai::AgentObservation {
            event_id: AgentEventId::new(),
            route: request.route.clone(),
            sequence: 4,
            connection_epoch: Some(request.connection_epoch),
            weak_episode: next_episode,
            provider: tracker
                .ai_session_agent(request.pty_id)
                .unwrap()
                .parse()
                .unwrap(),
            provider_session_id: None,
            process: None,
            activity: AgentActivity::Unknown,
            connectivity: AgentConnectivity::Live,
            confirmation: AgentConfirmation::LiveConfirmed,
            evidence: AgentEvidence::PtyActivity,
            received_at_unix_ms: 4,
        });
        let AgentApplyOutcome::Applied {
            run_id,
            created: true,
        } = outcome
        else {
            panic!("later input rejected");
        };
        assert_ne!(run_id, old_run);
        let projection = accepted_agent_projection(&registry, &request.route, false);
        assert!(projection.live);
        assert_eq!(projection.provider.as_deref(), Some("codex"));
        assert_eq!(projection.status, PaneStatus::Idle);
    }

    #[test]
    fn old_inventory_retirement_preserves_a_later_recognized_input() {
        retirement_preserves_later_input(false);
    }

    #[test]
    fn old_inventory_retirement_preserves_pending_input_before_echo() {
        retirement_preserves_later_input(true);
    }

    #[test]
    fn retirement_rejection_and_hook_guard_preserve_a_matching_source_latch() {
        for hook_enabled in [false, true] {
            let mut request = request();
            let mut registry = AgentRuntimeRegistry::default();
            let tracker = SessionTracker::new();
            tracker.track_input_with_line_snapshot(request.pty_id, "codex\r", None);
            request.weak_episode = tracker.weak_detection_episode(request.pty_id);
            let started = tracker.ai_session_started_at(request.pty_id);
            apply_inventory_and_retire_tracking(
                &mut registry,
                &tracker,
                request.pty_id,
                false,
                inventory(&request, 1, vec![process(10)]),
            )
            .unwrap();
            let before = registry
                .active_run_for_route(&request.route)
                .unwrap()
                .clone();
            let result = apply_inventory_and_retire_tracking(
                &mut registry,
                &tracker,
                request.pty_id,
                hook_enabled,
                inventory(&request, if hook_enabled { 2 } else { 1 }, vec![]),
            );
            if hook_enabled {
                result.unwrap();
                assert_eq!(
                    registry.run(&before.run_id).unwrap().activity,
                    AgentActivity::Exited
                );
            } else {
                assert_eq!(result, Err(AgentObservationIgnored::OutOfOrder));
                assert_eq!(registry.run(&before.run_id), Some(&before));
            }
            assert_eq!(
                tracker.ai_session_agent(request.pty_id).as_deref(),
                Some("codex")
            );
            assert_eq!(
                tracker.weak_detection_episode(request.pty_id),
                request.weak_episode
            );
            assert_eq!(tracker.ai_session_started_at(request.pty_id), started);
        }
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
            weak_episode: request.weak_episode,
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
    fn rejected_stale_inventory_preserves_accepted_hook_state() {
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
            weak_episode: request.weak_episode,
            provider: AgentProvider::CODEX.parse().unwrap(),
            provider_session_id: Some("hook-session".into()),
            process: Some(process(10).process),
            activity: AgentActivity::Done,
            connectivity: AgentConnectivity::Live,
            confirmation: mt_ai::AgentConfirmation::LiveConfirmed,
            evidence: AgentEvidence::Hook,
            received_at_unix_ms: 11,
        });
        let before = registry.runs().cloned().collect::<Vec<_>>();
        let outcome = apply_inventory_and_retire_tracking(
            &mut registry,
            &tracker,
            request.pty_id,
            false,
            inventory(&request, 10, vec![process(10), other]),
        );
        assert_eq!(outcome, Err(AgentObservationIgnored::OutOfOrder));
        for run in &before {
            assert_eq!(registry.run(&run.run_id), Some(run));
        }
        let hook = registry
            .runs()
            .find(|run| run.evidence == AgentEvidence::Hook)
            .unwrap();
        assert_eq!(hook.last_sequence, 11);
        assert_eq!(hook.activity, AgentActivity::Done);
        assert_eq!(registry.runs().count(), 2);
        let projection = accepted_agent_projection(&registry, &request.route, false);
        assert_eq!(projection.status, PaneStatus::AiIdle);
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
        assert_eq!(run.activity, AgentActivity::Unknown);
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
                weak_episode: request.weak_episode,
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
