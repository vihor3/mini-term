//! Stable agent-run identity and host-neutral live-state reconciliation.
//!
//! This module deliberately knows neither PTYs nor GPUI. Callers translate
//! local Hook/PTY events and authenticated remote inventory into observations;
//! the registry owns deduplication, ordering, evidence precedence, and the
//! compatibility projection consumed by the existing pane UI.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::str::FromStr;

use mt_identity::{
    AgentEventId, AgentRunId, ExecutionHostId, PaneKey, TabId, TerminalIncarnationId,
    TerminalSessionId, WorktreeId,
};
use serde::{Deserialize, Deserializer, Serialize, de};

pub const AGENT_RUNTIME_PROTOCOL_VERSION: u32 = 1;
const SEEN_EVENT_CAP: usize = 4096;
const MAX_PROVIDER_LEN: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseAgentProviderError;

impl fmt::Display for ParseAgentProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid normalized agent provider")
    }
}

impl std::error::Error for ParseAgentProviderError {}

/// Normalized, extensible provider identity.
///
/// Known aliases collapse to the five provider keys understood by mini-term.
/// Unknown providers are accepted only as bounded lowercase ASCII identifiers
/// so plugins can extend the vocabulary without turning display text into an
/// identity key.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct AgentProvider(String);

impl AgentProvider {
    pub const CLAUDE: &'static str = "claude";
    pub const CODEX: &'static str = "codex";
    pub const OPENCODE: &'static str = "opencode";
    pub const PI: &'static str = "pi";
    pub const GROK: &'static str = "grok";

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_known(&self) -> bool {
        matches!(
            self.as_str(),
            Self::CLAUDE | Self::CODEX | Self::OPENCODE | Self::PI | Self::GROK
        )
    }
}

impl fmt::Display for AgentProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for AgentProvider {
    type Err = ParseAgentProviderError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let lowercase = value.trim().to_ascii_lowercase();
        let normalized = match lowercase.as_str() {
            "claude" | "claude-code" | "anthropic" => Self::CLAUDE,
            "codex" | "codex-cli" | "openai-codex" => Self::CODEX,
            "opencode" | "open-code" => Self::OPENCODE,
            "pi" | "pi-agent" => Self::PI,
            "grok" | "grok-cli" => Self::GROK,
            other if valid_provider_key(other) => other,
            _ => return Err(ParseAgentProviderError),
        };
        Ok(Self(normalized.to_string()))
    }
}

impl<'de> Deserialize<'de> for AgentProvider {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

fn valid_provider_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_PROVIDER_LEN
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().copied().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRoute {
    pub execution_host_id: ExecutionHostId,
    pub worktree_id: WorktreeId,
    pub tab_id: TabId,
    pub pane_key: PaneKey,
    pub terminal_session_id: TerminalSessionId,
    pub terminal_incarnation_id: TerminalIncarnationId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentActivity {
    Starting,
    Working,
    Blocked,
    Waiting,
    Done,
    Failed,
    Interrupted,
    Exited,
    Unknown,
}

impl AgentActivity {
    pub fn is_ended(self) -> bool {
        matches!(self, Self::Interrupted | Self::Exited)
    }

    pub fn legacy_status(self) -> &'static str {
        match self {
            Self::Starting | Self::Working | Self::Blocked => "ai-working",
            Self::Waiting | Self::Done => "ai-idle",
            Self::Failed => "error",
            Self::Interrupted | Self::Exited | Self::Unknown => "idle",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentConnectivity {
    Live,
    Stale,
    Disconnected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentConfirmation {
    LiveConfirmed,
    RestoredUnconfirmed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEvidence {
    RestoredHistory,
    PtyActivity,
    ProcessAttested,
    Hook,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProcessIdentity {
    pub pid: u32,
    pub start_ticks: u64,
}

impl AgentProcessIdentity {
    pub fn new(pid: u32, start_ticks: u64) -> Option<Self> {
        (pid > 0 && start_ticks > 0).then_some(Self { pid, start_ticks })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentObservation {
    pub event_id: AgentEventId,
    pub route: AgentRoute,
    pub sequence: u64,
    pub connection_epoch: Option<u64>,
    pub provider: AgentProvider,
    pub provider_session_id: Option<String>,
    pub process: Option<AgentProcessIdentity>,
    pub activity: AgentActivity,
    pub connectivity: AgentConnectivity,
    pub confirmation: AgentConfirmation,
    pub evidence: AgentEvidence,
    pub received_at_unix_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentProcessObservation {
    pub provider: AgentProvider,
    pub process: AgentProcessIdentity,
    pub activity: AgentActivity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentProcessInventoryObservation {
    pub event_id: AgentEventId,
    pub route: AgentRoute,
    pub sequence: u64,
    pub connection_epoch: u64,
    pub processes: Vec<AgentProcessObservation>,
    pub received_at_unix_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentConnectivityObservation {
    pub event_id: AgentEventId,
    pub route: AgentRoute,
    pub sequence: u64,
    pub connection_epoch: Option<u64>,
    pub connectivity: AgentConnectivity,
    pub received_at_unix_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentRuntimeState {
    pub run_id: AgentRunId,
    pub last_event_id: AgentEventId,
    pub route: AgentRoute,
    pub provider: AgentProvider,
    pub provider_session_id: Option<String>,
    pub process: Option<AgentProcessIdentity>,
    pub activity: AgentActivity,
    pub connectivity: AgentConnectivity,
    pub confirmation: AgentConfirmation,
    pub evidence: AgentEvidence,
    pub connection_epoch: Option<u64>,
    pub last_sequence: u64,
    pub received_at_unix_ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentObservationIgnored {
    DuplicateEvent,
    InvalidSequence,
    InvalidConnectionEpoch,
    StaleConnectionEpoch,
    OutOfOrder,
    EndedRun,
    DuplicateProcess,
    UnresolvedHookOwner,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentApplyOutcome {
    Applied { run_id: AgentRunId, created: bool },
    Ignored(AgentObservationIgnored),
}

#[derive(Default)]
pub struct AgentRuntimeRegistry {
    runs: HashMap<AgentRunId, AgentRuntimeState>,
    latest_epoch_by_route: HashMap<AgentRoute, u64>,
    seen_event_ids: HashSet<AgentEventId>,
    seen_event_order: VecDeque<AgentEventId>,
}

impl AgentRuntimeRegistry {
    pub fn observe(&mut self, observation: AgentObservation) -> AgentApplyOutcome {
        if let Err(reason) = self.validate_event(
            &observation.event_id,
            &observation.route,
            observation.sequence,
            observation.connection_epoch,
        ) {
            return AgentApplyOutcome::Ignored(reason);
        }

        let matched = self.find_run(&observation);
        // After retirement a queued, unbound PTY event must not create a new
        // heuristic run. A later real launch still has a newer sequence.
        if matched.is_none()
            && observation.evidence == AgentEvidence::PtyActivity
            && self.runs.values().any(|state| {
                state.route == observation.route
                    && !is_newer(
                        state.connection_epoch,
                        state.last_sequence,
                        observation.connection_epoch,
                        observation.sequence,
                    )
            })
        {
            return AgentApplyOutcome::Ignored(AgentObservationIgnored::OutOfOrder);
        }
        if let Some(run_id) = matched {
            let state = self.runs.get_mut(&run_id).expect("matched run exists");
            if state.activity.is_ended() && !observation.activity.is_ended() {
                return AgentApplyOutcome::Ignored(AgentObservationIgnored::EndedRun);
            }
            if !is_newer(
                state.connection_epoch,
                state.last_sequence,
                observation.connection_epoch,
                observation.sequence,
            ) {
                return AgentApplyOutcome::Ignored(AgentObservationIgnored::OutOfOrder);
            }

            apply_observation(state, &observation);
            self.remember_event(observation.event_id);
            return AgentApplyOutcome::Applied {
                run_id,
                created: false,
            };
        }

        let run_id = AgentRunId::new();
        self.runs.insert(
            run_id.clone(),
            AgentRuntimeState {
                run_id: run_id.clone(),
                last_event_id: observation.event_id.clone(),
                route: observation.route,
                provider: observation.provider,
                provider_session_id: nonempty(observation.provider_session_id),
                process: observation.process,
                activity: observation.activity,
                connectivity: observation.connectivity,
                confirmation: observation.confirmation,
                evidence: observation.evidence,
                connection_epoch: observation.connection_epoch,
                last_sequence: observation.sequence,
                received_at_unix_ms: observation.received_at_unix_ms,
            },
        );
        self.remember_event(observation.event_id);
        AgentApplyOutcome::Applied {
            run_id,
            created: true,
        }
    }

    pub fn apply_process_inventory(
        &mut self,
        inventory: AgentProcessInventoryObservation,
    ) -> Result<Vec<AgentRunId>, AgentObservationIgnored> {
        let mut unique = HashSet::new();
        if inventory
            .processes
            .iter()
            .any(|process| !unique.insert(process.process))
        {
            return Err(AgentObservationIgnored::DuplicateProcess);
        }

        self.validate_event(
            &inventory.event_id,
            &inventory.route,
            inventory.sequence,
            Some(inventory.connection_epoch),
        )?;

        let observed_processes = unique;
        let mut applied = Vec::with_capacity(inventory.processes.len());
        for process in &inventory.processes {
            let observation = AgentObservation {
                event_id: inventory.event_id.clone(),
                route: inventory.route.clone(),
                sequence: inventory.sequence,
                connection_epoch: Some(inventory.connection_epoch),
                provider: process.provider.clone(),
                provider_session_id: None,
                process: Some(process.process),
                activity: process.activity,
                connectivity: AgentConnectivity::Live,
                confirmation: AgentConfirmation::LiveConfirmed,
                evidence: AgentEvidence::ProcessAttested,
                received_at_unix_ms: inventory.received_at_unix_ms,
            };
            let run_id = match self.find_run(&observation) {
                Some(run_id) => {
                    let state = self.runs.get_mut(&run_id).expect("matched run exists");
                    if state.activity.is_ended()
                        || !is_newer(
                            state.connection_epoch,
                            state.last_sequence,
                            observation.connection_epoch,
                            observation.sequence,
                        )
                    {
                        continue;
                    }
                    apply_observation(state, &observation);
                    run_id
                }
                None => {
                    let run_id = AgentRunId::new();
                    self.runs.insert(
                        run_id.clone(),
                        AgentRuntimeState {
                            run_id: run_id.clone(),
                            last_event_id: inventory.event_id.clone(),
                            route: inventory.route.clone(),
                            provider: process.provider.clone(),
                            provider_session_id: None,
                            process: Some(process.process),
                            activity: process.activity,
                            connectivity: AgentConnectivity::Live,
                            confirmation: AgentConfirmation::LiveConfirmed,
                            evidence: AgentEvidence::ProcessAttested,
                            connection_epoch: Some(inventory.connection_epoch),
                            last_sequence: inventory.sequence,
                            received_at_unix_ms: inventory.received_at_unix_ms,
                        },
                    );
                    run_id
                }
            };
            applied.push(run_id);
        }

        for state in self.runs.values_mut() {
            if state.route == inventory.route
                && state.evidence == AgentEvidence::ProcessAttested
                && !state.activity.is_ended()
                && state
                    .process
                    .is_some_and(|process| !observed_processes.contains(&process))
                && is_newer(
                    state.connection_epoch,
                    state.last_sequence,
                    Some(inventory.connection_epoch),
                    inventory.sequence,
                )
            {
                state.last_event_id = inventory.event_id.clone();
                state.activity = AgentActivity::Exited;
                state.connectivity = AgentConnectivity::Live;
                state.connection_epoch = Some(inventory.connection_epoch);
                state.last_sequence = inventory.sequence;
                state.received_at_unix_ms = inventory.received_at_unix_ms;
            }
        }

        self.remember_event(inventory.event_id);
        Ok(applied)
    }

    pub fn mark_connectivity(
        &mut self,
        observation: AgentConnectivityObservation,
    ) -> Result<usize, AgentObservationIgnored> {
        self.validate_event(
            &observation.event_id,
            &observation.route,
            observation.sequence,
            observation.connection_epoch,
        )?;
        let mut changed = 0;
        for state in self.runs.values_mut() {
            if state.route == observation.route
                && !state.activity.is_ended()
                && is_newer(
                    state.connection_epoch,
                    state.last_sequence,
                    observation.connection_epoch,
                    observation.sequence,
                )
            {
                state.last_event_id = observation.event_id.clone();
                state.connectivity = observation.connectivity;
                state.connection_epoch = observation.connection_epoch.or(state.connection_epoch);
                state.last_sequence = observation.sequence;
                state.received_at_unix_ms = observation.received_at_unix_ms;
                changed += 1;
            }
        }
        self.remember_event(observation.event_id);
        Ok(changed)
    }

    pub fn runs(&self) -> impl Iterator<Item = &AgentRuntimeState> {
        self.runs.values()
    }

    pub fn runs_for_worktree(
        &self,
        worktree_id: &WorktreeId,
    ) -> impl Iterator<Item = &AgentRuntimeState> {
        self.runs
            .values()
            .filter(move |state| &state.route.worktree_id == worktree_id)
    }

    pub fn run(&self, run_id: &AgentRunId) -> Option<&AgentRuntimeState> {
        self.runs.get(run_id)
    }

    pub fn active_run_for_route(&self, route: &AgentRoute) -> Option<&AgentRuntimeState> {
        self.runs
            .values()
            .filter(|state| &state.route == route && !state.activity.is_ended())
            .max_by_key(|state| (state.received_at_unix_ms, state.last_sequence))
    }

    /// Apply a provider-less Hook exit only when existing identity proves its
    /// unique current owner. Ordinary observation matching is unchanged.
    pub fn observe_hook_exit(
        &mut self,
        route: AgentRoute,
        event_id: AgentEventId,
        sequence: u64,
        connection_epoch: Option<u64>,
        received_at_unix_ms: i64,
    ) -> AgentApplyOutcome {
        let Some(owner) = self.exact_hook_exit_owner(&route) else {
            return AgentApplyOutcome::Ignored(AgentObservationIgnored::UnresolvedHookOwner);
        };
        let observation = AgentObservation {
            event_id,
            route,
            sequence,
            connection_epoch,
            provider: owner.provider.clone(),
            provider_session_id: owner.provider_session_id.clone(),
            process: owner.process,
            activity: AgentActivity::Exited,
            connectivity: AgentConnectivity::Live,
            confirmation: AgentConfirmation::LiveConfirmed,
            evidence: AgentEvidence::Hook,
            received_at_unix_ms,
        };
        self.observe(observation)
    }

    fn exact_hook_exit_owner(&self, route: &AgentRoute) -> Option<&AgentRuntimeState> {
        let same_route = || self.runs.values().filter(|state| &state.route == route);
        let mut owners = same_route().filter(|state| {
            state.evidence == AgentEvidence::Hook
                && state.confirmation == AgentConfirmation::LiveConfirmed
                && !state.activity.is_ended()
        });
        let owner = owners.next()?;
        if owners.next().is_some() {
            return None;
        }
        // Prove uniqueness in find_run's first applicable matching branch,
        // including ended identity matches. Never rely on HashMap order.
        let matches = same_route()
            .filter(|state| {
                if let Some(process) = owner.process {
                    state.process == Some(process)
                } else if let Some(session_id) = owner.provider_session_id.as_deref() {
                    state.provider == owner.provider
                        && state.provider_session_id.as_deref() == Some(session_id)
                } else {
                    state.provider == owner.provider && !state.activity.is_ended()
                }
            })
            .take(2)
            .count();
        (matches == 1).then_some(owner)
    }

    pub fn remove_route(&mut self, route: &AgentRoute) {
        self.runs.retain(|_, state| &state.route != route);
        self.latest_epoch_by_route.remove(route);
    }

    fn validate_event(
        &mut self,
        event_id: &AgentEventId,
        route: &AgentRoute,
        sequence: u64,
        connection_epoch: Option<u64>,
    ) -> Result<(), AgentObservationIgnored> {
        if self.seen_event_ids.contains(event_id) {
            return Err(AgentObservationIgnored::DuplicateEvent);
        }
        if sequence == 0 {
            return Err(AgentObservationIgnored::InvalidSequence);
        }
        if connection_epoch == Some(0) {
            return Err(AgentObservationIgnored::InvalidConnectionEpoch);
        }
        if let Some(epoch) = connection_epoch {
            let latest = self.latest_epoch_by_route.get(route).copied().unwrap_or(0);
            if epoch < latest {
                return Err(AgentObservationIgnored::StaleConnectionEpoch);
            }
            if epoch > latest {
                self.latest_epoch_by_route.insert(route.clone(), epoch);
                for state in self.runs.values_mut() {
                    if &state.route == route
                        && state.connection_epoch.is_some_and(|prior| prior < epoch)
                        && !state.activity.is_ended()
                    {
                        state.connectivity = AgentConnectivity::Disconnected;
                    }
                }
            }
        }
        Ok(())
    }

    fn find_run(&self, observation: &AgentObservation) -> Option<AgentRunId> {
        let same_route = || {
            self.runs
                .values()
                .filter(|state| state.route == observation.route)
        };

        if let Some(process) = observation.process
            && let Some(state) = same_route().find(|state| state.process == Some(process))
        {
            return Some(state.run_id.clone());
        }
        if let Some(session_id) = observation.provider_session_id.as_deref()
            && let Some(state) = same_route().find(|state| {
                state.provider_session_id.as_deref() == Some(session_id)
                    && state.provider == observation.provider
            })
        {
            return Some(state.run_id.clone());
        }
        if let Some(state) = same_route().find(|state| {
            !state.activity.is_ended()
                && state.provider == observation.provider
                && (observation.process.is_none() || state.process.is_none())
        }) {
            return Some(state.run_id.clone());
        }

        let weaker = same_route()
            .filter(|state| !state.activity.is_ended() && state.evidence < observation.evidence)
            .collect::<Vec<_>>();
        (weaker.len() == 1).then(|| weaker[0].run_id.clone())
    }

    fn remember_event(&mut self, event_id: AgentEventId) {
        if self.seen_event_ids.insert(event_id.clone()) {
            self.seen_event_order.push_back(event_id);
        }
        while self.seen_event_order.len() > SEEN_EVENT_CAP {
            if let Some(expired) = self.seen_event_order.pop_front() {
                self.seen_event_ids.remove(&expired);
            }
        }
    }
}

fn apply_observation(state: &mut AgentRuntimeState, observation: &AgentObservation) {
    state.last_event_id = observation.event_id.clone();
    let may_refresh_process_activity = observation.evidence == AgentEvidence::PtyActivity
        && state.evidence == AgentEvidence::ProcessAttested
        && !state.activity.is_ended()
        && matches!(
            observation.activity,
            AgentActivity::Working | AgentActivity::Waiting
        );
    if observation.evidence >= state.evidence {
        state.provider = observation.provider.clone();
        state.activity = observation.activity;
        state.confirmation = observation.confirmation;
        if observation.provider_session_id.is_some() {
            state.provider_session_id = nonempty(observation.provider_session_id.clone());
        }
        if observation.process.is_some() {
            state.process = observation.process;
        }
    } else if may_refresh_process_activity {
        state.activity = observation.activity;
    }
    state.connectivity = observation.connectivity;
    state.evidence = state.evidence.max(observation.evidence);
    state.connection_epoch = observation.connection_epoch.or(state.connection_epoch);
    state.last_sequence = observation.sequence;
    state.received_at_unix_ms = observation.received_at_unix_ms;
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn is_newer(
    state_epoch: Option<u64>,
    state_sequence: u64,
    observation_epoch: Option<u64>,
    observation_sequence: u64,
) -> bool {
    match (state_epoch, observation_epoch) {
        (Some(state), Some(observed)) if observed != state => observed > state,
        (None, Some(_)) => true,
        (Some(_), None) => false,
        _ => observation_sequence > state_sequence,
    }
}

pub fn activity_from_legacy_status(status: &str, cause: Option<&str>) -> Option<AgentActivity> {
    match status {
        "ai-working" if cause.is_some_and(crate::hook_server::is_attention_cause) => {
            Some(AgentActivity::Blocked)
        }
        "ai-working" => Some(AgentActivity::Working),
        "ai-idle" if cause == Some("Stop") => Some(AgentActivity::Done),
        "ai-idle" => Some(AgentActivity::Waiting),
        "error" => Some(AgentActivity::Failed),
        "idle" => Some(AgentActivity::Exited),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt_identity::{HostInstallId, RepoId};

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

    fn observation(route: AgentRoute, sequence: u64, evidence: AgentEvidence) -> AgentObservation {
        AgentObservation {
            event_id: AgentEventId::new(),
            route,
            sequence,
            connection_epoch: None,
            provider: "claude-code".parse().unwrap(),
            provider_session_id: None,
            process: None,
            activity: AgentActivity::Working,
            connectivity: AgentConnectivity::Live,
            confirmation: AgentConfirmation::LiveConfirmed,
            evidence,
            received_at_unix_ms: 1,
        }
    }

    #[test]
    fn hook_exit_ends_only_its_owner_beside_a_newer_process() {
        for provider in ["claude", "codex"] {
            let route = route();
            let mut registry = AgentRuntimeRegistry::default();
            let mut hook = observation(route.clone(), 1, AgentEvidence::Hook);
            hook.provider = "codex".parse().unwrap();
            hook.provider_session_id = Some("hook-session".into());
            hook.process = AgentProcessIdentity::new(42, 100);
            let AgentApplyOutcome::Applied { run_id: owner, .. } = registry.observe(hook) else {
                panic!("Hook owner should be created");
            };
            let mut process = observation(route.clone(), 2, AgentEvidence::ProcessAttested);
            process.provider = provider.parse().unwrap();
            process.process = AgentProcessIdentity::new(43, 200);
            process.received_at_unix_ms = 2;
            let AgentApplyOutcome::Applied {
                run_id: peer,
                created: true,
            } = registry.observe(process)
            else {
                panic!("independent process should be created");
            };
            let before = registry.run(&peer).unwrap().clone();
            assert_eq!(registry.active_run_for_route(&route).unwrap().run_id, peer);
            assert_eq!(
                registry.observe_hook_exit(route, AgentEventId::new(), 3, None, 3),
                AgentApplyOutcome::Applied {
                    run_id: owner.clone(),
                    created: false
                }
            );
            let ended = registry.run(&owner).unwrap();
            assert_eq!(ended.activity, AgentActivity::Exited);
            assert_eq!(ended.provider_session_id.as_deref(), Some("hook-session"));
            assert_eq!(ended.process, AgentProcessIdentity::new(42, 100));
            assert_eq!(registry.run(&peer), Some(&before));
            assert_eq!(registry.runs().count(), 2);
        }
    }

    #[test]
    fn unbound_hook_exit_rejects_ambiguous_fallback_but_uses_exact_session() {
        for (session_id, peer_provider, accepted) in [
            (None, "claude", true),
            (None, "codex", false),
            (Some("hook-session"), "codex", true),
        ] {
            let route = route();
            let mut registry = AgentRuntimeRegistry::default();
            let mut hook = observation(route.clone(), 1, AgentEvidence::Hook);
            hook.provider = "codex".parse().unwrap();
            hook.provider_session_id = session_id.map(str::to_string);
            let AgentApplyOutcome::Applied { run_id: owner, .. } = registry.observe(hook) else {
                panic!("Hook owner should be created");
            };
            let mut process = observation(route.clone(), 2, AgentEvidence::ProcessAttested);
            process.process = AgentProcessIdentity::new(43, 200);
            let AgentApplyOutcome::Applied {
                run_id: peer,
                created: true,
            } = registry.observe(process.clone())
            else {
                panic!("independent process should be created");
            };
            // A provider correction must not make an unbound Hook exit choose
            // whichever same-provider run happens to occur first in the map.
            process.event_id = AgentEventId::new();
            process.sequence = 3;
            process.provider = peer_provider.parse().unwrap();
            registry.observe(process);
            let before = registry.run(&peer).unwrap().clone();
            let outcome = registry.observe_hook_exit(route, AgentEventId::new(), 4, None, 4);
            if accepted {
                assert_eq!(
                    outcome,
                    AgentApplyOutcome::Applied {
                        run_id: owner.clone(),
                        created: false
                    }
                );
                assert_eq!(
                    registry.run(&owner).unwrap().activity,
                    AgentActivity::Exited
                );
                assert_eq!(
                    registry.run(&owner).unwrap().provider_session_id.as_deref(),
                    session_id
                );
            } else {
                assert_eq!(
                    outcome,
                    AgentApplyOutcome::Ignored(AgentObservationIgnored::UnresolvedHookOwner)
                );
                assert_eq!(
                    registry.run(&owner).unwrap().activity,
                    AgentActivity::Working
                );
            }
            assert_eq!(registry.run(&peer), Some(&before));
        }
    }

    #[test]
    fn exact_hook_exit_keeps_existing_event_epoch_and_sequence_fences() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let mut hook = observation(route.clone(), 10, AgentEvidence::Hook);
        hook.connection_epoch = Some(2);
        let event_id = hook.event_id.clone();
        let AgentApplyOutcome::Applied { run_id, .. } = registry.observe(hook) else {
            panic!("Hook owner should be created");
        };
        let before = registry.run(&run_id).unwrap().clone();
        for (event_id, sequence, epoch, reason) in [
            (
                event_id,
                11,
                Some(2),
                AgentObservationIgnored::DuplicateEvent,
            ),
            (
                AgentEventId::new(),
                0,
                Some(2),
                AgentObservationIgnored::InvalidSequence,
            ),
            (
                AgentEventId::new(),
                11,
                Some(0),
                AgentObservationIgnored::InvalidConnectionEpoch,
            ),
            (
                AgentEventId::new(),
                11,
                Some(1),
                AgentObservationIgnored::StaleConnectionEpoch,
            ),
            (
                AgentEventId::new(),
                9,
                Some(2),
                AgentObservationIgnored::OutOfOrder,
            ),
        ] {
            assert_eq!(
                registry.observe_hook_exit(route.clone(), event_id, sequence, epoch, 11),
                AgentApplyOutcome::Ignored(reason)
            );
            assert_eq!(registry.run(&run_id), Some(&before));
        }
        let mut other_route = route;
        other_route.terminal_incarnation_id = TerminalIncarnationId::new();
        assert_eq!(
            registry.observe_hook_exit(other_route, AgentEventId::new(), 11, Some(2), 11),
            AgentApplyOutcome::Ignored(AgentObservationIgnored::UnresolvedHookOwner)
        );
        assert_eq!(registry.run(&run_id), Some(&before));
    }

    #[test]
    fn provider_aliases_normalize_and_invalid_keys_fail() {
        assert_eq!(
            "Claude-Code".parse::<AgentProvider>().unwrap().as_str(),
            AgentProvider::CLAUDE
        );
        assert_eq!(
            "open-code".parse::<AgentProvider>().unwrap().as_str(),
            AgentProvider::OPENCODE
        );
        assert!("bad provider".parse::<AgentProvider>().is_err());
        assert!("-bad".parse::<AgentProvider>().is_err());
        assert!(
            "a".repeat(MAX_PROVIDER_LEN + 1)
                .parse::<AgentProvider>()
                .is_err()
        );
        let provider: AgentProvider = serde_json::from_str("\"Codex-CLI\"").unwrap();
        assert_eq!(provider.as_str(), AgentProvider::CODEX);
        assert_eq!(serde_json::to_string(&provider).unwrap(), "\"codex\"");
    }

    #[test]
    fn stronger_process_evidence_upgrades_one_heuristic_run() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let first = registry.observe(observation(route.clone(), 1, AgentEvidence::PtyActivity));
        let AgentApplyOutcome::Applied {
            run_id,
            created: true,
        } = first
        else {
            panic!("expected new run");
        };

        let process = AgentProcessIdentity::new(42, 99).unwrap();
        let result = registry
            .apply_process_inventory(AgentProcessInventoryObservation {
                event_id: AgentEventId::new(),
                route,
                sequence: 2,
                connection_epoch: 7,
                processes: vec![AgentProcessObservation {
                    provider: "claude".parse().unwrap(),
                    process,
                    activity: AgentActivity::Waiting,
                }],
                received_at_unix_ms: 2,
            })
            .unwrap();
        assert_eq!(result, vec![run_id.clone()]);
        let state = registry.run(&run_id).unwrap();
        assert_eq!(state.process, Some(process));
        assert_eq!(state.evidence, AgentEvidence::ProcessAttested);
        assert_eq!(state.activity, AgentActivity::Waiting);
    }

    #[test]
    fn retired_route_rejects_queued_pty_but_accepts_new_launch() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let first = registry
            .apply_process_inventory(AgentProcessInventoryObservation {
                event_id: AgentEventId::new(),
                route: route.clone(),
                sequence: 9,
                connection_epoch: 1,
                processes: vec![AgentProcessObservation {
                    provider: "claude".parse().unwrap(),
                    process: AgentProcessIdentity::new(42, 99).unwrap(),
                    activity: AgentActivity::Working,
                }],
                received_at_unix_ms: 9,
            })
            .unwrap();
        registry
            .apply_process_inventory(AgentProcessInventoryObservation {
                event_id: AgentEventId::new(),
                route: route.clone(),
                sequence: 11,
                connection_epoch: 1,
                processes: vec![],
                received_at_unix_ms: 11,
            })
            .unwrap();
        let mut delayed = observation(route.clone(), 10, AgentEvidence::PtyActivity);
        delayed.connection_epoch = Some(1);
        assert_eq!(
            registry.observe(delayed),
            AgentApplyOutcome::Ignored(AgentObservationIgnored::OutOfOrder)
        );
        assert!(registry.active_run_for_route(&route).is_none());
        let mut launch = observation(route, 12, AgentEvidence::PtyActivity);
        launch.connection_epoch = Some(1);
        let AgentApplyOutcome::Applied { run_id, created } = registry.observe(launch) else {
            panic!("new launch should be accepted");
        };
        assert!(created);
        assert_ne!(run_id, first[0]);
    }

    #[test]
    fn pty_activity_refreshes_but_cannot_end_process_attested_run() {
        let route = route();
        let process = AgentProcessIdentity::new(42, 99).unwrap();
        let mut registry = AgentRuntimeRegistry::default();
        let run_id = registry
            .apply_process_inventory(AgentProcessInventoryObservation {
                event_id: AgentEventId::new(),
                route: route.clone(),
                sequence: 1,
                connection_epoch: 7,
                processes: vec![AgentProcessObservation {
                    provider: "claude".parse().unwrap(),
                    process,
                    activity: AgentActivity::Waiting,
                }],
                received_at_unix_ms: 1,
            })
            .unwrap()
            .pop()
            .unwrap();

        let mut working = observation(route.clone(), 2, AgentEvidence::PtyActivity);
        working.connection_epoch = Some(7);
        assert!(matches!(
            registry.observe(working),
            AgentApplyOutcome::Applied { created: false, .. }
        ));
        assert_eq!(
            registry.run(&run_id).unwrap().activity,
            AgentActivity::Working
        );

        let mut exited = observation(route, 3, AgentEvidence::PtyActivity);
        exited.connection_epoch = Some(7);
        exited.activity = AgentActivity::Exited;
        assert!(matches!(
            registry.observe(exited),
            AgentApplyOutcome::Applied { created: false, .. }
        ));
        let state = registry.run(&run_id).unwrap();
        assert_eq!(state.activity, AgentActivity::Working);
        assert_eq!(state.evidence, AgentEvidence::ProcessAttested);
        assert_eq!(state.process, Some(process));
    }

    #[test]
    fn route_epoch_sequence_and_event_fences_fail_closed() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let mut first = observation(route.clone(), 3, AgentEvidence::ProcessAttested);
        first.connection_epoch = Some(9);
        first.process = AgentProcessIdentity::new(7, 11);
        let event_id = first.event_id.clone();
        let AgentApplyOutcome::Applied { run_id, .. } = registry.observe(first.clone()) else {
            panic!("expected apply");
        };

        assert_eq!(
            registry.observe(first),
            AgentApplyOutcome::Ignored(AgentObservationIgnored::DuplicateEvent)
        );
        let mut older_sequence = observation(route.clone(), 3, AgentEvidence::ProcessAttested);
        older_sequence.connection_epoch = Some(9);
        older_sequence.process = AgentProcessIdentity::new(7, 11);
        assert_eq!(
            registry.observe(older_sequence),
            AgentApplyOutcome::Ignored(AgentObservationIgnored::OutOfOrder)
        );
        let mut old_epoch = observation(route.clone(), 99, AgentEvidence::ProcessAttested);
        old_epoch.connection_epoch = Some(8);
        old_epoch.process = AgentProcessIdentity::new(7, 11);
        assert_eq!(
            registry.observe(old_epoch),
            AgentApplyOutcome::Ignored(AgentObservationIgnored::StaleConnectionEpoch)
        );

        let mut wrong_route = route.clone();
        wrong_route.terminal_incarnation_id = TerminalIncarnationId::new();
        let mut different = observation(wrong_route, 4, AgentEvidence::ProcessAttested);
        different.connection_epoch = Some(9);
        different.process = AgentProcessIdentity::new(7, 11);
        let AgentApplyOutcome::Applied {
            run_id: different_run,
            created: true,
        } = registry.observe(different)
        else {
            panic!("different incarnation must create a separate run");
        };
        assert_ne!(run_id, different_run);
        assert!(registry.seen_event_ids.contains(&event_id));
    }

    #[test]
    fn ended_run_rejects_replay_but_new_process_creates_new_run() {
        let route = route();
        let process = AgentProcessIdentity::new(10, 20).unwrap();
        let mut registry = AgentRuntimeRegistry::default();
        registry
            .apply_process_inventory(AgentProcessInventoryObservation {
                event_id: AgentEventId::new(),
                route: route.clone(),
                sequence: 1,
                connection_epoch: 1,
                processes: vec![AgentProcessObservation {
                    provider: "codex".parse().unwrap(),
                    process,
                    activity: AgentActivity::Working,
                }],
                received_at_unix_ms: 1,
            })
            .unwrap();
        registry
            .apply_process_inventory(AgentProcessInventoryObservation {
                event_id: AgentEventId::new(),
                route: route.clone(),
                sequence: 2,
                connection_epoch: 1,
                processes: Vec::new(),
                received_at_unix_ms: 2,
            })
            .unwrap();

        let replay = AgentObservation {
            event_id: AgentEventId::new(),
            route: route.clone(),
            sequence: 3,
            connection_epoch: Some(1),
            provider: "codex".parse().unwrap(),
            provider_session_id: None,
            process: Some(process),
            activity: AgentActivity::Working,
            connectivity: AgentConnectivity::Live,
            confirmation: AgentConfirmation::LiveConfirmed,
            evidence: AgentEvidence::ProcessAttested,
            received_at_unix_ms: 3,
        };
        assert_eq!(
            registry.observe(replay),
            AgentApplyOutcome::Ignored(AgentObservationIgnored::EndedRun)
        );

        let new_process = AgentProcessIdentity::new(11, 30).unwrap();
        let result = registry
            .apply_process_inventory(AgentProcessInventoryObservation {
                event_id: AgentEventId::new(),
                route,
                sequence: 4,
                connection_epoch: 1,
                processes: vec![AgentProcessObservation {
                    provider: "codex".parse().unwrap(),
                    process: new_process,
                    activity: AgentActivity::Working,
                }],
                received_at_unix_ms: 4,
            })
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(registry.run(&result[0]).unwrap().process, Some(new_process));
    }

    #[test]
    fn connectivity_changes_do_not_rewrite_activity() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let AgentApplyOutcome::Applied { run_id, .. } =
            registry.observe(observation(route.clone(), 1, AgentEvidence::PtyActivity))
        else {
            panic!("expected run");
        };
        assert_eq!(
            registry
                .mark_connectivity(AgentConnectivityObservation {
                    event_id: AgentEventId::new(),
                    route,
                    sequence: 2,
                    connection_epoch: None,
                    connectivity: AgentConnectivity::Disconnected,
                    received_at_unix_ms: 2,
                })
                .unwrap(),
            1
        );
        let state = registry.run(&run_id).unwrap();
        assert_eq!(state.activity, AgentActivity::Working);
        assert_eq!(state.connectivity, AgentConnectivity::Disconnected);
    }

    #[test]
    fn legacy_projection_keeps_existing_status_vocabulary() {
        assert_eq!(
            activity_from_legacy_status("ai-working", Some("PermissionRequest")),
            Some(AgentActivity::Blocked)
        );
        assert_eq!(
            activity_from_legacy_status("ai-idle", Some("Stop")),
            Some(AgentActivity::Done)
        );
        assert_eq!(AgentActivity::Working.legacy_status(), "ai-working");
        assert_eq!(AgentActivity::Waiting.legacy_status(), "ai-idle");
        assert_eq!(AgentActivity::Failed.legacy_status(), "error");
        assert_eq!(AgentActivity::Exited.legacy_status(), "idle");
    }
}
