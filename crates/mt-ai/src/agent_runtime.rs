//! Stable agent-run identity and host-neutral live-state reconciliation.
//!
//! This module deliberately knows neither PTYs nor GPUI. Callers translate
//! local Hook/PTY events and authenticated remote inventory into observations;
//! the registry owns deduplication, ordering, evidence precedence, and the
//! compatibility projection consumed by the existing pane UI.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use mt_identity::{
    AgentEventId, AgentRunId, ExecutionHostId, PaneKey, TabId, TerminalIncarnationId,
    TerminalSessionId, WorktreeId,
};
use serde::{Deserialize, Deserializer, Serialize, de};

pub const AGENT_RUNTIME_PROTOCOL_VERSION: u32 = 1;
const SEEN_EVENT_CAP: usize = 4096;
const MAX_PROVIDER_LEN: usize = 32;
pub const AGENT_SEMANTIC_MAX_AGE_MS: i64 = 15_000;

/// Process-local source identity for one real-input detection episode. Only
/// the tracker mints it; transport and polling retain the captured value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct AgentWeakEpisode(u64);

impl AgentWeakEpisode {
    pub(crate) fn next() -> Option<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| value.checked_add(1))
            .ok().map(|previous| Self(previous + 1))
    }
}

/// Internal source identity for one recognized Hook lifecycle, independent of
/// the provider's reusable session ID and the optional weak-input receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AgentHookLifecycleId(u64);

impl AgentHookLifecycleId {
    pub(crate) fn next() -> Option<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| value.checked_add(1))
            .ok().map(|previous| Self(previous + 1))
    }
}

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
    pub weak_episode: Option<AgentWeakEpisode>,
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
    /// Compatibility input only. Inventory proves liveness, not task activity.
    pub activity: AgentActivity,
}

/// The caller must capture foreground ownership with the title, not borrow the
/// newest process on a route. Session observations require an already bound ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentSemanticOwner {
    ForegroundProcess(AgentProcessIdentity),
    ProviderSession(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSemanticObservation {
    pub event_id: AgentEventId,
    pub run_id: AgentRunId,
    pub route: AgentRoute,
    pub provider: AgentProvider,
    pub owner: AgentSemanticOwner,
    pub sequence: u64,
    pub connection_epoch: Option<u64>,
    pub activity: AgentActivity,
    pub observed_at_unix_ms: i64,
    pub received_at_unix_ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentActivityFreshness {
    Unknown,
    Fresh,
    Stale,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentProcessInventoryObservation {
    pub event_id: AgentEventId,
    pub route: AgentRoute,
    pub sequence: u64,
    pub connection_epoch: u64,
    pub weak_episode: Option<AgentWeakEpisode>,
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
    pub weak_episode: Option<AgentWeakEpisode>,
    pub activity: AgentActivity,
    pub connectivity: AgentConnectivity,
    pub confirmation: AgentConfirmation,
    pub evidence: AgentEvidence,
    pub connection_epoch: Option<u64>,
    pub last_sequence: u64,
    pub received_at_unix_ms: i64,
}

/// Stronger exact-route evidence or a newer real-input episode invalidated an
/// unbound PTY fallback. This is neither a process exit nor a run identity link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentFallbackSupersession {
    pub event_id: AgentEventId,
    pub evidence: AgentEvidence,
    pub sequence: u64,
    pub connection_epoch: Option<u64>,
    /// Episode captured by the invalidating source, not the delivery thread.
    pub weak_episode: Option<AgentWeakEpisode>,
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
    InvalidProcess,
    UnresolvedHookOwner,
    UnresolvedSemanticOwner,
    StaleSemanticObservation,
    InvalidSemanticActivity,
    StrongerEvidence,
    AmbiguousRun,
    SupersededWeakEpisode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentApplyOutcome {
    Applied { run_id: AgentRunId, created: bool },
    Ignored(AgentObservationIgnored),
}

#[derive(Default)]
pub struct AgentRuntimeRegistry {
    runs: HashMap<AgentRunId, AgentRuntimeState>,
    hook_lifecycles: HashMap<(AgentRoute, AgentHookLifecycleId), AgentRunId>,
    fallback_supersessions: HashMap<AgentRunId, AgentFallbackSupersession>,
    superseded_weak_episodes: HashMap<AgentRoute, AgentWeakEpisode>,
    latest_weak_episodes: HashMap<AgentRoute, AgentWeakEpisode>,
    legacy_weak_fences: HashSet<AgentRoute>,
    latest_epoch_by_route: HashMap<AgentRoute, u64>,
    seen_event_ids: HashSet<AgentEventId>,
    seen_event_order: VecDeque<AgentEventId>,
    semantic_observations: HashMap<AgentRunId, (i64, Option<u64>)>,
    process_owner_since: HashMap<AgentRunId, i64>,
}

impl AgentRuntimeRegistry {
    pub fn observe(&mut self, mut observation: AgentObservation) -> AgentApplyOutcome {
        if observation.process.is_some_and(|process| {
            AgentProcessIdentity::new(process.pid, process.start_ticks).is_none()
        }) {
            return AgentApplyOutcome::Ignored(AgentObservationIgnored::InvalidProcess);
        }
        if observation.evidence == AgentEvidence::ProcessAttested
            || (observation.evidence == AgentEvidence::PtyActivity
                && !observation.activity.is_ended())
        {
            observation.activity = AgentActivity::Unknown;
        }
        if let Err(reason) = self.validate_event(
            &observation.event_id,
            &observation.route,
            observation.sequence,
            observation.connection_epoch,
        ) {
            return AgentApplyOutcome::Ignored(reason);
        }
        let unbound_weak = is_unbound_weak_observation(&observation);
        if unbound_weak && observation.weak_episode.is_some_and(|episode| {
            self.superseded_weak_episodes.get(&observation.route).is_some_and(|fence| episode <= *fence)
                || self.latest_weak_episodes.get(&observation.route).is_some_and(|latest| episode < *latest)
        }) {
            return AgentApplyOutcome::Ignored(AgentObservationIgnored::SupersededWeakEpisode);
        }

        if observation.process.is_none()
            && observation.provider_session_id.as_deref().is_some_and(|session| {
                self.runs.values().filter(|state| {
                    state.route == observation.route
                        && state.provider == observation.provider
                        && state.provider_session_id.as_deref() == Some(session)
                }).take(2).count() > 1
            })
        {
            return AgentApplyOutcome::Ignored(AgentObservationIgnored::AmbiguousRun);
        }
        let matched = self.find_run(&observation, true);
        if matched.is_none()
            && observation.process.is_none()
            && nonempty(observation.provider_session_id.clone()).is_none()
            && self.runs.values().any(|state| {
                state.route == observation.route
                    && state.provider == observation.provider
                    && !state.activity.is_ended()
                    && !self.fallback_supersessions.contains_key(&state.run_id)
                    && !(unbound_weak && observation.weak_episode.is_some()
                        && is_unbound_weak_state(state)
                        && state.weak_episode < observation.weak_episode)
                    && !(observation.evidence == AgentEvidence::Hook
                        && observation.weak_episode.is_some()
                        && is_unbound_weak_state(state)
                        && state.weak_episode <= observation.weak_episode)
            })
        {
            return AgentApplyOutcome::Ignored(AgentObservationIgnored::AmbiguousRun);
        }
        if unbound_weak && observation.weak_episode.is_none()
            && matched.is_none()
            && self.legacy_weak_fences.contains(&observation.route)
        {
            return AgentApplyOutcome::Ignored(AgentObservationIgnored::SupersededWeakEpisode);
        }
        // After retirement a queued, unbound PTY event must not create a new
        // heuristic run. Ordering supplements source episode validation; a
        // newer poll sequence alone is never proof of a new launch.
        if matched.is_none()
            && matches!(observation.evidence, AgentEvidence::PtyActivity | AgentEvidence::ProcessAttested)
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
        if let Some(run_id) = &matched {
            let state = self.runs.get(run_id).expect("matched run exists");
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
        }
        self.apply_validated_observation(observation, matched)
    }

    /// Only a source-recognized start may establish a new run after an accepted
    /// end of the same provider session. Repeated starts retain their binding.
    pub fn start_hook_lifecycle(
        &mut self,
        observation: AgentObservation,
        lifecycle_id: AgentHookLifecycleId,
    ) -> AgentApplyOutcome {
        self.apply_hook_lifecycle(observation, lifecycle_id, true)
    }

    /// Follow-on events retain their source lifecycle, including queued ends.
    /// First recognition without a start keeps ordinary ended-run rejection.
    pub fn observe_hook_lifecycle(
        &mut self,
        observation: AgentObservation,
        lifecycle_id: AgentHookLifecycleId,
    ) -> AgentApplyOutcome {
        self.apply_hook_lifecycle(observation, lifecycle_id, false)
    }

    fn apply_hook_lifecycle(
        &mut self,
        observation: AgentObservation,
        lifecycle_id: AgentHookLifecycleId,
        explicit_start: bool,
    ) -> AgentApplyOutcome {
        let matched = match self.hook_lifecycle_target(&observation, lifecycle_id, explicit_start) {
            Ok(matched) => matched,
            Err(reason) => return AgentApplyOutcome::Ignored(reason),
        };
        let key = (observation.route.clone(), lifecycle_id);
        // Ownership, lifecycle and ordering validation above is read-only. No
        // binding, route epoch, sibling or supersession changes precede it.
        let outcome = self.apply_validated_observation(observation, matched);
        if let AgentApplyOutcome::Applied { run_id, .. } = &outcome {
            self.hook_lifecycles.insert(key, run_id.clone());
        }
        outcome
    }

    fn hook_lifecycle_target(
        &self,
        observation: &AgentObservation,
        lifecycle_id: AgentHookLifecycleId,
        explicit_start: bool,
    ) -> Result<Option<AgentRunId>, AgentObservationIgnored> {
        let valid_session = observation.provider_session_id.as_deref().is_some_and(|session| {
            !session.trim().is_empty() && session.len() <= 512 && !session.chars().any(char::is_control)
        });
        if !valid_session || observation.process.is_some()
            || observation.evidence != AgentEvidence::Hook
            || observation.confirmation != AgentConfirmation::LiveConfirmed
            || observation.connectivity != AgentConnectivity::Live
            || (explicit_start && observation.activity.is_ended())
        {
            return Err(AgentObservationIgnored::UnresolvedHookOwner);
        }
        self.validate_event(&observation.event_id, &observation.route,
            observation.sequence, observation.connection_epoch)?;

        let key = (observation.route.clone(), lifecycle_id);
        if let Some(run_id) = self.hook_lifecycles.get(&key) {
            let state = self.runs.get(run_id).expect("lifecycle run exists");
            if state.provider_session_id != observation.provider_session_id {
                return Err(AgentObservationIgnored::UnresolvedHookOwner);
            }
            if state.activity.is_ended() {
                return Err(AgentObservationIgnored::EndedRun);
            }
            if !is_newer(state.connection_epoch, state.last_sequence,
                observation.connection_epoch, observation.sequence)
            {
                return Err(AgentObservationIgnored::OutOfOrder);
            }
            // The source token also carries genuine provider corrections; the
            // provider name alone never selects a different run.
            return Ok(Some(run_id.clone()));
        }
        if observation.activity.is_ended() {
            return Err(AgentObservationIgnored::UnresolvedHookOwner);
        }

        let exact: Vec<_> = self.runs.values().filter(|state| {
            state.route == observation.route && bound_provider_session_matches(state, observation)
        }).collect();
        if explicit_start && !exact.is_empty() && exact.iter().all(|state| state.activity.is_ended()) {
            if exact.iter().any(|state| !is_newer(state.connection_epoch, state.last_sequence,
                observation.connection_epoch, observation.sequence))
            {
                return Err(AgentObservationIgnored::OutOfOrder);
            }
            return Ok(None);
        }
        if exact.len() > 1 {
            return Err(AgentObservationIgnored::AmbiguousRun);
        }
        let matched = self.find_run(observation, true);
        if let Some(run_id) = &matched {
            let state = self.runs.get(run_id).expect("matched run exists");
            if state.activity.is_ended() {
                return Err(AgentObservationIgnored::EndedRun);
            }
            if self.hook_lifecycles.values().any(|owner| owner == run_id) {
                return Err(AgentObservationIgnored::UnresolvedHookOwner);
            }
            if !is_newer(state.connection_epoch, state.last_sequence,
                observation.connection_epoch, observation.sequence)
            {
                return Err(AgentObservationIgnored::OutOfOrder);
            }
        }
        Ok(matched)
    }

    fn apply_validated_observation(
        &mut self,
        observation: AgentObservation,
        matched: Option<AgentRunId>,
    ) -> AgentApplyOutcome {
        if let Some(run_id) = matched {
            self.accept_epoch(&observation.route, observation.connection_epoch);
            let state = self.runs.get_mut(&run_id).expect("matched run exists");
            if process_owner_changed(state, &observation) {
                self.process_owner_since
                    .insert(run_id.clone(), observation.received_at_unix_ms);
            }
            apply_observation(state, &observation);
            self.supersede_unbound_fallbacks(&observation);
            self.remember_event(observation.event_id);
            return AgentApplyOutcome::Applied {
                run_id,
                created: false,
            };
        }

        self.accept_epoch(&observation.route, observation.connection_epoch);
        self.supersede_unbound_fallbacks(&observation);
        let run_id = AgentRunId::new();
        if observation.process.is_some() {
            self.process_owner_since
                .insert(run_id.clone(), observation.received_at_unix_ms);
        }
        self.runs.insert(
            run_id.clone(),
            AgentRuntimeState {
                run_id: run_id.clone(),
                last_event_id: observation.event_id.clone(),
                route: observation.route,
                provider: observation.provider,
                provider_session_id: nonempty(observation.provider_session_id),
                process: observation.process,
                weak_episode: observation.weak_episode,
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
        if inventory.processes.iter().any(|process| {
            AgentProcessIdentity::new(process.process.pid, process.process.start_ticks).is_none()
        }) {
            return Err(AgentObservationIgnored::InvalidProcess);
        }
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
        if self.runs.values().any(|state| {
            state.route == inventory.route
                && !is_newer(
                    state.connection_epoch,
                    state.last_sequence,
                    Some(inventory.connection_epoch),
                    inventory.sequence,
                )
        }) {
            return Err(AgentObservationIgnored::OutOfOrder);
        }

        let mut provider_counts = HashMap::new();
        let observations: Vec<_> = inventory.processes.iter().map(|process| {
            *provider_counts.entry(process.provider.clone()).or_insert(0usize) += 1;
            AgentObservation {
                event_id: inventory.event_id.clone(),
                route: inventory.route.clone(),
                sequence: inventory.sequence,
                connection_epoch: Some(inventory.connection_epoch),
                provider: process.provider.clone(),
                provider_session_id: None,
                process: Some(process.process),
                weak_episode: inventory.weak_episode,
                activity: AgentActivity::Unknown,
                connectivity: AgentConnectivity::Live,
                confirmation: AgentConfirmation::LiveConfirmed,
                evidence: AgentEvidence::ProcessAttested,
                received_at_unix_ms: inventory.received_at_unix_ms,
            }
        }).collect();
        // Resolve every candidate against pre-batch facts. One weak alias must
        // never be assigned to whichever independent process happens to be first.
        let matches: Vec<_> = observations.iter().map(|observation| {
            self.find_run(observation, provider_counts[&observation.provider] == 1)
        }).collect();

        self.accept_epoch(&inventory.route, Some(inventory.connection_epoch));
        let observed_processes = unique;
        let mut applied = Vec::with_capacity(inventory.processes.len());
        let mut accepted_source = None;
        for (observation, matched) in observations.iter().zip(matches) {
            let run_id = match matched {
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
                    if process_owner_changed(state, observation) {
                        self.process_owner_since
                            .insert(run_id.clone(), observation.received_at_unix_ms);
                    }
                    apply_observation(state, observation);
                    run_id
                }
                None => {
                    let run_id = AgentRunId::new();
                    self.process_owner_since
                        .insert(run_id.clone(), inventory.received_at_unix_ms);
                    self.runs.insert(
                        run_id.clone(),
                        AgentRuntimeState {
                            run_id: run_id.clone(),
                            last_event_id: inventory.event_id.clone(),
                            route: inventory.route.clone(),
                            provider: observation.provider.clone(),
                            provider_session_id: None,
                            process: observation.process,
                            weak_episode: observation.weak_episode,
                            activity: AgentActivity::Unknown,
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
            accepted_source = Some(observation);
        }
        // Finish every preflight identity upgrade before invalidating leftover
        // aliases, including when different providers occur in one inventory.
        if let Some(observation) = accepted_source {
            self.supersede_unbound_fallbacks(observation);
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

    /// Accept task semantics only for an exact, currently attested live owner.
    /// Receipt/polling time must never refresh the original semantic timestamp.
    pub fn observe_semantic(&mut self, observation: AgentSemanticObservation) -> AgentApplyOutcome {
        let Some(state) = self.runs.get(&observation.run_id) else {
            return AgentApplyOutcome::Ignored(AgentObservationIgnored::UnresolvedSemanticOwner);
        };
        let owns_identity = match &observation.owner {
            AgentSemanticOwner::ForegroundProcess(process) => state.process == Some(*process),
            AgentSemanticOwner::ProviderSession(session) => {
                !session.trim().is_empty()
                    && session.len() <= 512
                    && !session.chars().any(char::is_control)
                    && state.provider_session_id.as_ref() == Some(session)
            }
        };
        let unique_owner = self.runs.values().filter(|candidate| {
            candidate.route == observation.route
                && candidate.provider == observation.provider
                && !candidate.activity.is_ended()
                && match &observation.owner {
                    AgentSemanticOwner::ForegroundProcess(process) => candidate.process == Some(*process),
                    AgentSemanticOwner::ProviderSession(session) => candidate.provider_session_id.as_ref() == Some(session),
                }
        }).take(2).count() == 1;
        if state.route != observation.route
            || state.provider != observation.provider
            || !owns_identity
            || state.confirmation != AgentConfirmation::LiveConfirmed
            || state.evidence < AgentEvidence::ProcessAttested
            || state.connectivity != AgentConnectivity::Live
            || state.connection_epoch != observation.connection_epoch
        {
            return AgentApplyOutcome::Ignored(AgentObservationIgnored::UnresolvedSemanticOwner);
        }
        if state.activity.is_ended() {
            return AgentApplyOutcome::Ignored(AgentObservationIgnored::EndedRun);
        }
        if !unique_owner {
            return AgentApplyOutcome::Ignored(AgentObservationIgnored::UnresolvedSemanticOwner);
        }
        if state.evidence == AgentEvidence::Hook {
            return AgentApplyOutcome::Ignored(AgentObservationIgnored::StrongerEvidence);
        }
        if !matches!(
            observation.activity,
            AgentActivity::Starting
                | AgentActivity::Working
                | AgentActivity::Waiting
                | AgentActivity::Blocked
                | AgentActivity::Done
                | AgentActivity::Failed
        ) {
            return AgentApplyOutcome::Ignored(AgentObservationIgnored::InvalidSemanticActivity);
        }
        if !semantic_timestamp_is_fresh(
            observation.observed_at_unix_ms,
            observation.received_at_unix_ms,
        ) || self
            .process_owner_since
            .get(&observation.run_id)
            .is_none_or(|since| observation.observed_at_unix_ms < *since)
            || self
            .semantic_observations
            .get(&observation.run_id)
            .is_some_and(|(prior, _)| observation.observed_at_unix_ms <= *prior)
        {
            return AgentApplyOutcome::Ignored(AgentObservationIgnored::StaleSemanticObservation);
        }
        if !is_newer(
            state.connection_epoch,
            state.last_sequence,
            observation.connection_epoch,
            observation.sequence,
        ) {
            return AgentApplyOutcome::Ignored(AgentObservationIgnored::OutOfOrder);
        }
        if let Err(reason) = self.validate_event(
            &observation.event_id,
            &observation.route,
            observation.sequence,
            observation.connection_epoch,
        ) {
            return AgentApplyOutcome::Ignored(reason);
        }
        self.accept_epoch(&observation.route, observation.connection_epoch);
        let state = self.runs.get_mut(&observation.run_id).expect("owner exists");
        state.activity = observation.activity;
        state.last_event_id = observation.event_id.clone();
        state.last_sequence = observation.sequence;
        state.received_at_unix_ms = observation.received_at_unix_ms;
        self.semantic_observations.insert(
            observation.run_id.clone(),
            (observation.observed_at_unix_ms, observation.connection_epoch),
        );
        self.remember_event(observation.event_id);
        AgentApplyOutcome::Applied {
            run_id: observation.run_id,
            created: false,
        }
    }

    pub fn activity_freshness(&self, run_id: &AgentRunId, now_unix_ms: i64) -> AgentActivityFreshness {
        let Some(state) = self.runs.get(run_id) else {
            return AgentActivityFreshness::Unknown;
        };
        if state.activity == AgentActivity::Unknown {
            return AgentActivityFreshness::Unknown;
        }
        if state.evidence == AgentEvidence::Hook || state.activity.is_ended() {
            return AgentActivityFreshness::Fresh;
        }
        match self.semantic_observations.get(run_id) {
            Some((observed, epoch))
                if semantic_timestamp_is_fresh(*observed, now_unix_ms)
                    && *epoch == state.connection_epoch
                    && self.process_owner_since.get(run_id).is_some_and(|since| observed >= since) => {
                AgentActivityFreshness::Fresh
            }
            Some(_) => AgentActivityFreshness::Stale,
            None => AgentActivityFreshness::Unknown,
        }
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
        self.accept_epoch(&observation.route, observation.connection_epoch);
        let mut changed = 0;
        for state in self.runs.values_mut() {
            if state.route == observation.route
                && !state.activity.is_ended()
                && !self.fallback_supersessions.contains_key(&state.run_id)
                && is_newer(
                    state.connection_epoch,
                    state.last_sequence,
                    observation.connection_epoch,
                    observation.sequence,
                )
            {
                if observation.connection_epoch.is_some()
                    && state.connection_epoch != observation.connection_epoch
                {
                    self.process_owner_since
                        .insert(state.run_id.clone(), observation.received_at_unix_ms);
                }
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

    /// Retained after the stronger run ends. All liveness and Agent projection
    /// consumers must exclude these audit-only fallback records.
    pub fn fallback_supersession(
        &self,
        run_id: &AgentRunId,
    ) -> Option<&AgentFallbackSupersession> {
        self.fallback_supersessions.get(run_id)
    }

    pub fn is_superseded_weak_alias(&self, run_id: &AgentRunId) -> bool {
        self.fallback_supersessions.contains_key(run_id)
    }

    pub fn active_run_for_route(&self, route: &AgentRoute) -> Option<&AgentRuntimeState> {
        self.runs
            .values()
            .filter(|state| {
                &state.route == route && !state.activity.is_ended()
                    && !self.fallback_supersessions.contains_key(&state.run_id)
            })
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
            weak_episode: owner.weak_episode,
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
                    state.provider == owner.provider
                        && state.evidence == AgentEvidence::Hook
                        && !state.activity.is_ended()
                }
            })
            .take(2)
            .count();
        (matches == 1).then_some(owner)
    }

    pub fn remove_route(&mut self, route: &AgentRoute) {
        self.runs.retain(|_, state| &state.route != route);
        self.hook_lifecycles.retain(|(owner, _), _| owner != route);
        self.fallback_supersessions
            .retain(|run_id, _| self.runs.contains_key(run_id));
        self.semantic_observations
            .retain(|run_id, _| self.runs.contains_key(run_id));
        self.process_owner_since
            .retain(|run_id, _| self.runs.contains_key(run_id));
        self.latest_epoch_by_route.remove(route);
        self.superseded_weak_episodes.remove(route);
        self.latest_weak_episodes.remove(route);
        self.legacy_weak_fences.remove(route);
    }

    fn supersede_unbound_fallbacks(&mut self, observation: &AgentObservation) {
        let strong_source = observation.evidence == AgentEvidence::Hook
            || (observation.evidence == AgentEvidence::ProcessAttested && observation.process.is_some());
        let new_weak_episode = is_unbound_weak_observation(observation)
            && observation.weak_episode.is_some();
        if observation.confirmation != AgentConfirmation::LiveConfirmed
            || observation.connectivity != AgentConnectivity::Live
            || !(strong_source || new_weak_episode)
        {
            return;
        }
        if new_weak_episode {
            let episode = observation.weak_episode.expect("source episode exists");
            self.latest_weak_episodes.entry(observation.route.clone())
                .and_modify(|latest| *latest = (*latest).max(episode)).or_insert(episode);
            if observation.activity.is_ended() {
                self.superseded_weak_episodes.entry(observation.route.clone())
                    .and_modify(|fence| *fence = (*fence).max(episode)).or_insert(episode);
                self.legacy_weak_fences.insert(observation.route.clone());
            }
        }
        if observation.activity.is_ended() {
            return;
        }
        if strong_source {
            self.legacy_weak_fences.insert(observation.route.clone());
            if let Some(episode) = observation.weak_episode {
                self.superseded_weak_episodes.entry(observation.route.clone())
                    .and_modify(|fence| *fence = (*fence).max(episode)).or_insert(episode);
            }
        }
        for state in self.runs.values() {
            if state.route == observation.route
                && is_unbound_weak_state(state)
                && state.confirmation == AgentConfirmation::LiveConfirmed
                && !state.activity.is_ended()
                && match (state.weak_episode, observation.weak_episode) {
                    (Some(prior), Some(source)) => prior < source || (strong_source && prior == source),
                    (None, Some(_)) => true,
                    (None, None) => is_newer(state.connection_epoch, state.last_sequence,
                        observation.connection_epoch, observation.sequence),
                    (Some(_), None) => false,
                }
            {
                self.legacy_weak_fences.insert(state.route.clone());
                if let Some(episode) = state.weak_episode {
                    self.superseded_weak_episodes.entry(state.route.clone())
                        .and_modify(|fence| *fence = (*fence).max(episode)).or_insert(episode);
                }
                self.fallback_supersessions.entry(state.run_id.clone()).or_insert_with(|| {
                    AgentFallbackSupersession {
                        event_id: observation.event_id.clone(),
                        evidence: observation.evidence,
                        sequence: observation.sequence,
                        connection_epoch: observation.connection_epoch,
                        weak_episode: observation.weak_episode,
                    }
                });
            }
        }
    }

    fn validate_event(
        &self,
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
        }
        Ok(())
    }

    // Epoch changes affect sibling runs too, so commit them only after every
    // ownership and ordering check has accepted the observation.
    fn accept_epoch(&mut self, route: &AgentRoute, connection_epoch: Option<u64>) {
        let Some(epoch) = connection_epoch else {
            return;
        };
        if epoch <= self.latest_epoch_by_route.get(route).copied().unwrap_or(0) {
            return;
        }
        self.latest_epoch_by_route.insert(route.clone(), epoch);
        for state in self.runs.values_mut() {
            if &state.route == route
                && state.connection_epoch.is_some_and(|prior| prior < epoch)
                && !state.activity.is_ended()
                && !self.fallback_supersessions.contains_key(&state.run_id)
            {
                state.connectivity = AgentConnectivity::Disconnected;
            }
        }
    }

    fn find_run(
        &self,
        observation: &AgentObservation,
        allow_weak_process_upgrade: bool,
    ) -> Option<AgentRunId> {
        let same_route = || {
            self.runs
                .values()
                .filter(|state| {
                    state.route == observation.route
                        && !self.fallback_supersessions.contains_key(&state.run_id)
                })
        };

        if let Some(process) = observation.process
            && let Some(state) = same_route().find(|state| state.process == Some(process))
        {
            return Some(state.run_id.clone());
        }
        if let Some(state) = same_route().find(|state| {
                bound_provider_session_matches(state, observation)
                    && (observation.process.is_none()
                        || state.process.is_none()
                        || state.process == observation.process)
            })
        {
            return Some(state.run_id.clone());
        }
        let process_upgrade = observation.evidence == AgentEvidence::ProcessAttested
            && observation.process.is_some();
        if process_upgrade
            && (!allow_weak_process_upgrade || same_route().any(|state| {
                state.provider == observation.provider
                    && !state.activity.is_ended()
                    && state.process.is_some()
                    && state.process != observation.process
            }))
        {
            return None;
        }
        let mut compatible = same_route().filter(|state| {
            !state.activity.is_ended()
                && state.provider == observation.provider
                && ((state.evidence == AgentEvidence::Hook)
                    == (observation.evidence == AgentEvidence::Hook))
                && (!process_upgrade
                    || (state.evidence == AgentEvidence::PtyActivity
                        && state.confirmation == AgentConfirmation::LiveConfirmed
                        && state.process.is_none()
                        && (state.weak_episode.is_none() || state.weak_episode == observation.weak_episode)))
                && !(is_unbound_weak_observation(observation)
                    && (observation.weak_episode.is_some() || is_unbound_weak_state(state))
                    && state.weak_episode != observation.weak_episode)
                && (observation.process.is_none() || state.process.is_none())
                && (observation.provider_session_id.is_none()
                    || state.provider_session_id.is_none()
                    || observation.provider_session_id == state.provider_session_id)
        });
        let first = compatible.next()?;
        compatible.next().is_none().then(|| first.run_id.clone())
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
    let before = state.clone();
    let attests_hook_process = attests_bound_hook_process(state, observation);
    if observation.evidence >= state.evidence {
        if observation.evidence != AgentEvidence::ProcessAttested
            || state.evidence < AgentEvidence::ProcessAttested
            || state.provider != observation.provider
        {
            state.activity = observation.activity;
        }
        state.provider = observation.provider.clone();
        state.confirmation = observation.confirmation;
        if observation.provider_session_id.is_some() {
            state.provider_session_id = nonempty(observation.provider_session_id.clone());
        }
        if observation.process.is_some() {
            state.process = observation.process;
        }
        if observation.weak_episode.is_some() {
            state.weak_episode = state.weak_episode.max(observation.weak_episode);
        }
    } else if attests_hook_process {
        state.process = observation.process;
    }
    state.connectivity = observation.connectivity;
    state.evidence = state.evidence.max(observation.evidence);
    state.connection_epoch = observation.connection_epoch.or(state.connection_epoch);
    state.last_sequence = observation.sequence;
    // Ordering advances for accepted liveness probes, but an unchanged poll is
    // not a new semantic/attention event and must not renew unread or recency.
    let liveness_only = observation.evidence <= AgentEvidence::ProcessAttested
        && state.provider == before.provider
        && state.provider_session_id == before.provider_session_id
        && state.process == before.process
        && state.activity == before.activity
        && state.connectivity == before.connectivity
        && state.confirmation == before.confirmation
        && state.evidence == before.evidence
        && state.connection_epoch == before.connection_epoch;
    if !liveness_only {
        state.last_event_id = observation.event_id.clone();
        state.received_at_unix_ms = observation.received_at_unix_ms;
    }
}

fn process_owner_changed(state: &AgentRuntimeState, observation: &AgentObservation) -> bool {
    attests_bound_hook_process(state, observation)
        || (state.process.is_some() || observation.process.is_some())
        && ((observation.connection_epoch.is_some()
            && state.connection_epoch != observation.connection_epoch)
            || (observation.evidence >= state.evidence
                && ((observation.process.is_some() && state.process != observation.process)
                    || state.provider != observation.provider)))
}

fn is_unbound_weak_observation(observation: &AgentObservation) -> bool {
    observation.evidence == AgentEvidence::PtyActivity
        && observation.process.is_none()
        && observation.provider_session_id.as_deref().is_none_or(|session| session.trim().is_empty())
}

fn is_unbound_weak_state(state: &AgentRuntimeState) -> bool {
    state.evidence == AgentEvidence::PtyActivity
        && state.process.is_none()
        && state.provider_session_id.is_none()
}

fn bound_provider_session_matches(state: &AgentRuntimeState, observation: &AgentObservation) -> bool {
    state.provider == observation.provider
        && observation.provider_session_id.as_deref().is_some_and(|session| {
            !session.trim().is_empty()
                && session.len() <= 512
                && !session.chars().any(char::is_control)
                && state.provider_session_id.as_deref() == Some(session)
        })
}

fn attests_bound_hook_process(state: &AgentRuntimeState, observation: &AgentObservation) -> bool {
    state.evidence == AgentEvidence::Hook
        && observation.evidence == AgentEvidence::ProcessAttested
        && state.process.is_none()
        && observation.process.is_some()
        && bound_provider_session_matches(state, observation)
}

fn semantic_timestamp_is_fresh(observed: i64, received: i64) -> bool {
    observed >= 0 && (0..=AGENT_SEMANTIC_MAX_AGE_MS).contains(&received.saturating_sub(observed))
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
            weak_episode: None,
            activity: AgentActivity::Working,
            connectivity: AgentConnectivity::Live,
            confirmation: AgentConfirmation::LiveConfirmed,
            evidence,
            received_at_unix_ms: 1,
        }
    }

    fn assert_lifecycle_rejection_is_pure(
        registry: &mut AgentRuntimeRegistry,
        observation: AgentObservation,
        lifecycle_id: AgentHookLifecycleId,
        explicit_start: bool,
        reason: AgentObservationIgnored,
    ) {
        let snapshot = |registry: &AgentRuntimeRegistry| (
            registry.runs.clone(), registry.hook_lifecycles.clone(),
            registry.fallback_supersessions.clone(), registry.superseded_weak_episodes.clone(),
            registry.latest_weak_episodes.clone(), registry.legacy_weak_fences.clone(),
            registry.latest_epoch_by_route.clone(), registry.seen_event_ids.clone(),
            registry.seen_event_order.clone(), registry.semantic_observations.clone(),
            registry.process_owner_since.clone(),
        );
        let before = snapshot(registry);
        let outcome = if explicit_start {
            registry.start_hook_lifecycle(observation, lifecycle_id)
        } else {
            registry.observe_hook_lifecycle(observation, lifecycle_id)
        };
        assert_eq!(outcome, AgentApplyOutcome::Ignored(reason));
        assert_eq!(snapshot(registry), before);
    }

    #[test]
    fn hook_lifecycle_resume_preserves_audit_and_ordinary_ended_guards() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let mut start = observation(route.clone(), 1, AgentEvidence::Hook);
        start.provider_session_id = Some("same-session".into());
        let first_lifecycle = AgentHookLifecycleId::next().unwrap();
        let AgentApplyOutcome::Applied { run_id: first, created: true } = registry.start_hook_lifecycle(start.clone(), first_lifecycle) else {
            panic!("initial lifecycle must be accepted");
        };
        assert_lifecycle_rejection_is_pure(&mut registry, start.clone(), first_lifecycle, true, AgentObservationIgnored::DuplicateEvent);
        let mut repeat = start.clone();
        repeat.event_id = AgentEventId::new();
        repeat.sequence = 2;
        assert_eq!(registry.start_hook_lifecycle(repeat.clone(), first_lifecycle), AgentApplyOutcome::Applied {
            run_id: first.clone(), created: false,
        });
        let mut end = repeat.clone();
        end.event_id = AgentEventId::new();
        end.sequence = 3;
        end.activity = AgentActivity::Exited;
        assert!(matches!(registry.observe_hook_lifecycle(end.clone(), first_lifecycle), AgentApplyOutcome::Applied { .. }));
        let ended = registry.run(&first).unwrap().clone();

        let mut resumed = start.clone();
        resumed.event_id = AgentEventId::new();
        resumed.sequence = 4;
        let second_lifecycle = AgentHookLifecycleId::next().unwrap();
        assert_lifecycle_rejection_is_pure(&mut registry, resumed.clone(), second_lifecycle, false, AgentObservationIgnored::EndedRun);
        let mut stale_start = resumed.clone();
        stale_start.sequence = 3;
        assert_lifecycle_rejection_is_pure(&mut registry, stale_start, second_lifecycle, true, AgentObservationIgnored::OutOfOrder);
        assert_eq!(registry.observe(resumed.clone()), AgentApplyOutcome::Ignored(AgentObservationIgnored::EndedRun));
        let AgentApplyOutcome::Applied { run_id: second, created: true } = registry.start_hook_lifecycle(resumed.clone(), second_lifecycle) else {
            panic!("only explicit new lifecycle authority permits same-ID resume");
        };
        assert_ne!(second, first);
        let mut competing = resumed.clone();
        competing.event_id = AgentEventId::new();
        competing.sequence = 99;
        competing.connection_epoch = Some(99);
        assert_lifecycle_rejection_is_pure(&mut registry, competing, AgentHookLifecycleId::next().unwrap(), true, AgentObservationIgnored::AmbiguousRun);
        for activity in [AgentActivity::Working, AgentActivity::Exited] {
            let mut queued = resumed.clone();
            queued.event_id = AgentEventId::new();
            queued.sequence = 99;
            queued.connection_epoch = Some(99);
            queued.activity = activity;
            assert_lifecycle_rejection_is_pure(&mut registry, queued, first_lifecycle, false, AgentObservationIgnored::EndedRun);
        }
        let mut ordinary = resumed.clone();
        ordinary.event_id = AgentEventId::new();
        ordinary.sequence = 5;
        assert_eq!(registry.observe(ordinary.clone()), AgentApplyOutcome::Ignored(AgentObservationIgnored::AmbiguousRun));
        ordinary.activity = AgentActivity::Done;
        assert_eq!(registry.observe_hook_lifecycle(ordinary.clone(), second_lifecycle), AgentApplyOutcome::Applied {
            run_id: second.clone(), created: false,
        });
        assert!(!registry.run(&second).unwrap().activity.is_ended());
        end.event_id = AgentEventId::new();
        end.sequence = 6;
        assert!(matches!(registry.observe_hook_lifecycle(end, second_lifecycle), AgentApplyOutcome::Applied { .. }));
        resumed.event_id = AgentEventId::new();
        resumed.sequence = 7;
        let third_lifecycle = AgentHookLifecycleId::next().unwrap();
        assert!(matches!(registry.start_hook_lifecycle(resumed, third_lifecycle), AgentApplyOutcome::Applied { created: true, .. }));
        assert_eq!(registry.runs.len(), 3);
        assert_eq!(registry.hook_lifecycles.len(), 3);
        assert_eq!(registry.run(&first), Some(&ended));
        assert_eq!(registry.runs().filter(|run| !run.activity.is_ended()).count(), 1);
        assert!(registry.runs().all(|run| run.provider_session_id.as_deref() == Some("same-session")));
    }

    #[test]
    fn hook_lifecycle_rejections_preserve_epochs_bindings_and_supersession() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let tracker = crate::SessionTracker::new();
        tracker.track_input_with_line_snapshot(1, "claude\r", None);
        let mut weak = observation(route.clone(), 1, AgentEvidence::PtyActivity);
        weak.connection_epoch = Some(2);
        weak.weak_episode = tracker.weak_detection_episode(1);
        registry.observe(weak.clone());
        let mut start = observation(route.clone(), 2, AgentEvidence::Hook);
        start.connection_epoch = Some(2);
        start.provider_session_id = Some("owned".into());
        start.weak_episode = weak.weak_episode;
        let owner = AgentHookLifecycleId::next().unwrap();
        assert!(matches!(registry.start_hook_lifecycle(start.clone(), owner), AgentApplyOutcome::Applied { .. }));
        let mut peer = observation(route.clone(), 3, AgentEvidence::ProcessAttested);
        peer.provider = "codex".parse().unwrap();
        peer.process = AgentProcessIdentity::new(40, 100);
        peer.connection_epoch = Some(2);
        registry.observe(peer);
        let other = AgentHookLifecycleId::next().unwrap();
        let mut candidate = start.clone();
        candidate.event_id = AgentEventId::new();
        candidate.sequence = 99;
        candidate.connection_epoch = Some(99);
        for explicit in [false, true] {
            assert_lifecycle_rejection_is_pure(&mut registry, candidate.clone(), other, explicit, AgentObservationIgnored::UnresolvedHookOwner);
        }
        let mut mismatched = candidate.clone();
        mismatched.provider_session_id = Some("wrong-owner".into());
        assert_lifecycle_rejection_is_pure(&mut registry, mismatched, owner, false, AgentObservationIgnored::UnresolvedHookOwner);
        let mut duplicate = candidate.clone();
        duplicate.event_id = start.event_id.clone();
        assert_lifecycle_rejection_is_pure(&mut registry, duplicate, owner, true, AgentObservationIgnored::DuplicateEvent);
        let mut zero_sequence = candidate.clone();
        zero_sequence.sequence = 0;
        assert_lifecycle_rejection_is_pure(&mut registry, zero_sequence, owner, true, AgentObservationIgnored::InvalidSequence);
        let mut zero_epoch = candidate.clone();
        zero_epoch.connection_epoch = Some(0);
        assert_lifecycle_rejection_is_pure(&mut registry, zero_epoch, owner, true, AgentObservationIgnored::InvalidConnectionEpoch);
        let mut stale_epoch = candidate.clone();
        stale_epoch.connection_epoch = Some(1);
        assert_lifecycle_rejection_is_pure(&mut registry, stale_epoch, owner, true, AgentObservationIgnored::StaleConnectionEpoch);
        let mut old_sequence = candidate.clone();
        old_sequence.connection_epoch = Some(2);
        old_sequence.sequence = 1;
        assert_lifecycle_rejection_is_pure(&mut registry, old_sequence, owner, true, AgentObservationIgnored::OutOfOrder);
        let mut unknown_exit = candidate.clone();
        unknown_exit.provider_session_id = Some("unknown".into());
        unknown_exit.activity = AgentActivity::Exited;
        assert_lifecycle_rejection_is_pure(&mut registry, unknown_exit, other, false, AgentObservationIgnored::UnresolvedHookOwner);
        let mut ended_start = candidate.clone();
        ended_start.activity = AgentActivity::Exited;
        assert_lifecycle_rejection_is_pure(&mut registry, ended_start, owner, true, AgentObservationIgnored::UnresolvedHookOwner);
        let mut invalid_session = candidate.clone();
        invalid_session.provider_session_id = Some("bad\nsession".into());
        assert_lifecycle_rejection_is_pure(&mut registry, invalid_session, owner, true, AgentObservationIgnored::UnresolvedHookOwner);
        candidate.connection_epoch = Some(2);
        candidate.sequence = 4;
        assert!(matches!(registry.observe_hook_lifecycle(candidate, owner), AgentApplyOutcome::Applied { created: false, .. }));
    }

    #[test]
    fn hook_lifecycle_initial_binding_requires_unique_proof_and_purges_exact_route() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let mut process = observation(route.clone(), 1, AgentEvidence::ProcessAttested);
        process.provider_session_id = Some("attested".into());
        process.process = AgentProcessIdentity::new(41, 100);
        let AgentApplyOutcome::Applied { run_id: attested, .. } = registry.observe(process.clone()) else {
            panic!("process observation must be accepted");
        };
        let mut hook = observation(route.clone(), 2, AgentEvidence::Hook);
        hook.provider_session_id = Some("attested".into());
        let lifecycle_id = AgentHookLifecycleId::next().unwrap();
        assert_eq!(registry.start_hook_lifecycle(hook.clone(), lifecycle_id), AgentApplyOutcome::Applied {
            run_id: attested.clone(), created: false,
        });
        assert_eq!(registry.run(&attested).unwrap().process, process.process);
        hook.event_id = AgentEventId::new();
        hook.sequence = 3;
        hook.provider = "codex".parse().unwrap();
        assert_eq!(registry.observe_hook_lifecycle(hook.clone(), lifecycle_id), AgentApplyOutcome::Applied {
            run_id: attested.clone(), created: false,
        });
        assert_eq!(registry.run(&attested).unwrap().provider.as_str(), "codex");
        let mut other_route = route.clone();
        other_route.terminal_incarnation_id = TerminalIncarnationId::new();
        let mut other = hook.clone();
        other.event_id = AgentEventId::new();
        other.route = other_route.clone();
        let other_lifecycle = AgentHookLifecycleId::next().unwrap();
        assert!(matches!(registry.start_hook_lifecycle(other, other_lifecycle), AgentApplyOutcome::Applied { created: true, .. }));
        registry.remove_route(&route);
        assert_eq!(registry.hook_lifecycles.len(), 1);
        assert!(registry.hook_lifecycles.contains_key(&(other_route, other_lifecycle)));
        assert!(registry.run(&attested).is_none());

        let mut ambiguous = AgentRuntimeRegistry::default();
        ambiguous.observe(process.clone());
        process.event_id = AgentEventId::new();
        process.sequence = 2;
        process.process = AgentProcessIdentity::new(42, 101);
        ambiguous.observe(process);
        hook.event_id = AgentEventId::new();
        hook.provider = "claude".parse().unwrap();
        hook.sequence = 99;
        hook.connection_epoch = Some(99);
        assert_lifecycle_rejection_is_pure(&mut ambiguous, hook, lifecycle_id, true, AgentObservationIgnored::AmbiguousRun);
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
    fn unbound_hook_exit_does_not_borrow_process_peers() {
        for (session_id, peer_provider) in [
            (None, "claude"),
            (None, "codex"),
            (Some("hook-session"), "codex"),
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
            assert_eq!(
                outcome,
                AgentApplyOutcome::Applied {
                    run_id: owner.clone(),
                    created: false
                }
            );
            assert_eq!(registry.run(&owner).unwrap().activity, AgentActivity::Exited);
            assert_eq!(registry.run(&owner).unwrap().provider_session_id.as_deref(), session_id);
            assert_eq!(registry.run(&peer), Some(&before));
        }
    }

    #[test]
    fn hook_exit_does_not_choose_between_multiple_hook_owners() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        for (sequence, session) in [(1, "hook-a"), (2, "hook-b")] {
            let mut hook = observation(route.clone(), sequence, AgentEvidence::Hook);
            hook.connection_epoch = Some(7);
            hook.provider_session_id = Some(session.into());
            assert!(matches!(registry.observe(hook), AgentApplyOutcome::Applied { created: true, .. }));
        }
        let before: Vec<_> = registry.runs().cloned().collect();
        assert_eq!(
            registry.observe_hook_exit(route, AgentEventId::new(), 3, Some(8), 200),
            AgentApplyOutcome::Ignored(AgentObservationIgnored::UnresolvedHookOwner)
        );
        for run in &before {
            assert_eq!(registry.run(&run.run_id), Some(run));
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
                weak_episode: None,
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
        assert_eq!(state.activity, AgentActivity::Unknown);
    }

    #[test]
    fn source_episode_singleton_upgrade_remains_a_proved_unsuppressed_run() {
        let route = route();
        let tracker = crate::SessionTracker::new();
        tracker.track_input_with_line_snapshot(1, "claude\r", None);
        let episode = tracker.weak_detection_episode(1);
        let mut registry = AgentRuntimeRegistry::default();
        let mut weak = observation(route.clone(), 1, AgentEvidence::PtyActivity);
        weak.weak_episode = episode;
        let AgentApplyOutcome::Applied { run_id, .. } = registry.observe(weak.clone()) else {
            panic!("input fallback");
        };
        let mut inventory = inventory_for(&route, 2, &[AgentProcessIdentity::new(42, 99).unwrap()]);
        inventory.weak_episode = episode;
        assert_eq!(registry.apply_process_inventory(inventory).unwrap(), vec![run_id.clone()]);
        assert!(!registry.is_superseded_weak_alias(&run_id));
        assert_eq!(registry.run(&run_id).unwrap().evidence, AgentEvidence::ProcessAttested);
        weak.event_id = AgentEventId::new();
        weak.sequence = 3;
        weak.connection_epoch = Some(7);
        assert_eq!(registry.observe(weak), AgentApplyOutcome::Ignored(AgentObservationIgnored::SupersededWeakEpisode));
        assert_eq!(registry.active_run_for_route(&route).unwrap().run_id, run_id);
    }

    fn inventory_for(
        route: &AgentRoute,
        sequence: u64,
        processes: &[AgentProcessIdentity],
    ) -> AgentProcessInventoryObservation {
        AgentProcessInventoryObservation {
            event_id: AgentEventId::new(),
            route: route.clone(),
            sequence,
            connection_epoch: 7,
            weak_episode: None,
            processes: processes.iter().map(|process| AgentProcessObservation {
                provider: "claude".parse().unwrap(),
                process: *process,
                activity: AgentActivity::Working,
            }).collect(),
            received_at_unix_ms: 100 + sequence as i64,
        }
    }

    #[test]
    fn unbound_hook_cannot_swallow_inventory_processes_in_either_order() {
        let p1 = AgentProcessIdentity::new(42, 99).unwrap();
        let p2 = AgentProcessIdentity::new(43, 100).unwrap();
        for processes in [[p1, p2], [p2, p1]] {
            for session in [None, Some("hook-session")] {
                let route = route();
                let mut registry = AgentRuntimeRegistry::default();
                let mut hook = observation(route.clone(), 1, AgentEvidence::Hook);
                hook.connection_epoch = Some(7);
                hook.provider_session_id = session.map(str::to_string);
                let AgentApplyOutcome::Applied { run_id: hook_id, .. } = registry.observe(hook) else {
                    panic!("expected Hook owner");
                };
                let before = registry.run(&hook_id).unwrap().clone();
                let ids = registry.apply_process_inventory(inventory_for(&route, 2, &processes)).unwrap();
                assert_eq!(ids.len(), 2);
                assert_ne!(ids[0], ids[1]);
                assert!(!ids.contains(&hook_id));
                for (id, process) in ids.iter().zip(processes) {
                    let run = registry.run(id).unwrap();
                    assert_eq!(run.process, Some(process));
                    assert_eq!(run.activity, AgentActivity::Unknown);
                    assert_eq!(run.evidence, AgentEvidence::ProcessAttested);
                }
                let reversed = [processes[1], processes[0]];
                let again = registry.apply_process_inventory(inventory_for(&route, 3, &reversed)).unwrap();
                assert_eq!(again, vec![ids[1].clone(), ids[0].clone()]);
                assert_eq!(registry.run(&hook_id), Some(&before));
                assert!(registry.fallback_supersession(&hook_id).is_none());
                assert_eq!(registry.runs().count(), 3);
            }
        }
    }

    #[test]
    fn ambiguous_pty_alias_never_takes_the_first_process_of_a_batch() {
        let p1 = AgentProcessIdentity::new(42, 99).unwrap();
        let p2 = AgentProcessIdentity::new(43, 100).unwrap();
        for processes in [[p1, p2], [p2, p1]] {
            let route = route();
            let mut registry = AgentRuntimeRegistry::default();
            let AgentApplyOutcome::Applied { run_id: weak, .. } = registry.observe(
                observation(route.clone(), 1, AgentEvidence::PtyActivity),
            ) else {
                panic!("expected weak alias");
            };
            let before = registry.run(&weak).unwrap().clone();
            let ids = registry.apply_process_inventory(inventory_for(&route, 2, &processes)).unwrap();
            assert_eq!(ids.len(), 2);
            assert!(!ids.contains(&weak));
            assert_ne!(ids[0], ids[1]);
            assert_eq!(registry.run(&weak), Some(&before));
            let supersession = registry.fallback_supersession(&weak).unwrap().clone();
            assert_eq!(supersession.evidence, AgentEvidence::ProcessAttested);
            assert_eq!(supersession.sequence, 2);
            for (id, process) in ids.iter().zip(processes) {
                assert_eq!(registry.run(id).unwrap().process, Some(process));
                assert!(registry.fallback_supersession(id).is_none());
            }
            registry.apply_process_inventory(inventory_for(&route, 3, &[p1])).unwrap();
            let mut later = observation(route, 4, AgentEvidence::ProcessAttested);
            later.connection_epoch = Some(7);
            later.process = AgentProcessIdentity::new(44, 101);
            let AgentApplyOutcome::Applied { run_id, created: true } = registry.observe(later) else {
                panic!("known process plus new process is not a singleton upgrade");
            };
            assert_ne!(run_id, weak);
            assert_eq!(registry.run(&weak), Some(&before));
            assert_eq!(registry.fallback_supersession(&weak), Some(&supersession));
        }
    }

    #[test]
    fn singleton_process_does_not_choose_between_multiple_weak_aliases() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let mut aliases = Vec::new();
        for (sequence, session) in [(1, "weak-a"), (2, "weak-b")] {
            let mut weak = observation(route.clone(), sequence, AgentEvidence::PtyActivity);
            weak.provider_session_id = Some(session.into());
            let AgentApplyOutcome::Applied { run_id, created: true } = registry.observe(weak) else {
                panic!("expected distinct weak session aliases");
            };
            aliases.push(registry.run(&run_id).unwrap().clone());
        }
        let process = AgentProcessIdentity::new(42, 99).unwrap();
        let ids = registry.apply_process_inventory(inventory_for(&route, 3, &[process])).unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(registry.run(&ids[0]).unwrap().process, Some(process));
        for alias in &aliases {
            assert_ne!(ids[0], alias.run_id);
            assert_eq!(registry.run(&alias.run_id), Some(alias));
            assert!(registry.fallback_supersession(&alias.run_id).is_none());
        }
    }

    #[test]
    fn direct_process_observations_preserve_hook_and_singleton_pty_boundaries() {
        let p1 = AgentProcessIdentity::new(42, 99).unwrap();
        let p2 = AgentProcessIdentity::new(43, 100).unwrap();
        for evidence in [AgentEvidence::Hook, AgentEvidence::PtyActivity] {
            for processes in [[p1, p2], [p2, p1]] {
                let route = route();
                let mut registry = AgentRuntimeRegistry::default();
                let mut initial = observation(route.clone(), 1, evidence);
                initial.connection_epoch = Some(7);
                let AgentApplyOutcome::Applied { run_id: initial_id, .. } = registry.observe(initial) else {
                    panic!("expected initial owner");
                };
                let before = registry.run(&initial_id).unwrap().clone();
                for (index, process) in processes.into_iter().enumerate() {
                    let mut event = observation(route.clone(), index as u64 + 2, AgentEvidence::ProcessAttested);
                    event.connection_epoch = Some(7);
                    event.process = Some(process);
                    let AgentApplyOutcome::Applied { run_id, created } = registry.observe(event) else {
                        panic!("proved process must be retained");
                    };
                    let upgrade = evidence == AgentEvidence::PtyActivity && index == 0;
                    assert_eq!(run_id == initial_id, upgrade);
                    assert_eq!(created, !upgrade);
                    assert_eq!(registry.run(&run_id).unwrap().process, Some(process));
                }
                if evidence == AgentEvidence::Hook {
                    assert_eq!(registry.run(&initial_id), Some(&before));
                    assert_eq!(registry.runs().count(), 3);
                } else {
                    assert_eq!(registry.runs().count(), 2);
                }
            }
        }
    }

    #[test]
    fn cross_evidence_hook_matching_requires_already_proved_identity() {
        let (mut registry, process_id, process_route) = attested_registry();
        let before = registry.run(&process_id).unwrap().clone();
        let mut unbound = observation(process_route, 2, AgentEvidence::Hook);
        unbound.connection_epoch = Some(7);
        assert_eq!(registry.observe(unbound.clone()), AgentApplyOutcome::Ignored(AgentObservationIgnored::AmbiguousRun));
        assert_eq!(registry.run(&process_id), Some(&before));
        unbound.process = before.process;
        assert_eq!(registry.observe(unbound), AgentApplyOutcome::Applied { run_id: process_id, created: false });

        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let mut hook = observation(route.clone(), 1, AgentEvidence::Hook);
        hook.connection_epoch = Some(7);
        hook.provider_session_id = Some("proved-session".into());
        hook.activity = AgentActivity::Blocked;
        let AgentApplyOutcome::Applied { run_id, .. } = registry.observe(hook) else {
            panic!("expected Hook session");
        };
        let mut process = observation(route, 2, AgentEvidence::ProcessAttested);
        process.connection_epoch = Some(7);
        process.provider_session_id = Some("proved-session".into());
        process.process = AgentProcessIdentity::new(42, 99);
        assert_eq!(registry.observe(process), AgentApplyOutcome::Applied { run_id: run_id.clone(), created: false });
        let run = registry.run(&run_id).unwrap();
        assert_eq!(run.process, AgentProcessIdentity::new(42, 99));
        assert_eq!(run.evidence, AgentEvidence::Hook);
        assert_eq!(run.activity, AgentActivity::Blocked);
        assert_eq!(registry.runs().count(), 1);
    }

    #[test]
    fn first_session_hook_and_local_input_preserve_order_specific_aliases() {
        for hook_first in [false, true] {
            let route = route();
            let tracker = crate::SessionTracker::new();
            tracker.track_input_with_line_snapshot(1, "claude\r", None);
            let mut weak = observation(route.clone(), 1, AgentEvidence::PtyActivity);
            weak.provider = tracker.ai_session_agent(1).unwrap().parse().unwrap();
            weak.weak_episode = tracker.weak_detection_episode(1);
            let mut hook = observation(route.clone(), 1, AgentEvidence::Hook);
            hook.provider_session_id = Some("first-session".into());
            hook.weak_episode = tracker.weak_detection_episode(1);
            let mut registry = AgentRuntimeRegistry::default();
            let first = if hook_first { hook.clone() } else { weak.clone() };
            let AgentApplyOutcome::Applied { run_id: first_id, created: true } = registry.observe(first) else {
                panic!("expected first launch observation");
            };
            let before = registry.run(&first_id).unwrap().clone();
            let mut second = if hook_first { weak } else { hook };
            second.sequence = 2;
            let outcome = registry.observe(second);
            let hook_id = if hook_first {
                assert_eq!(outcome, AgentApplyOutcome::Ignored(AgentObservationIgnored::SupersededWeakEpisode));
                assert_eq!(registry.runs().count(), 1);
                first_id.clone()
            } else {
                let AgentApplyOutcome::Applied { run_id, created: true } = outcome else {
                    panic!("first Hook session cannot invent a weak alias binding");
                };
                assert_ne!(run_id, first_id);
                assert_eq!(registry.runs().count(), 2);
                assert_eq!(registry.run(&first_id).unwrap().activity, AgentActivity::Unknown);
                run_id
            };
            assert_eq!(registry.run(&first_id), Some(&before));
            assert_eq!(registry.fallback_supersession(&first_id).is_some(), !hook_first);
            let hook = registry.run(&hook_id).unwrap();
            assert_eq!(hook.provider_session_id.as_deref(), Some("first-session"));
            assert_eq!(hook.process, None);
            assert_eq!(hook.evidence, AgentEvidence::Hook);
            assert_eq!(hook.activity, AgentActivity::Working);
            assert_eq!(
                registry.observe_hook_exit(route, AgentEventId::new(), 3, None, 103),
                AgentApplyOutcome::Applied { run_id: hook_id.clone(), created: false }
            );
            assert_eq!(registry.run(&hook_id).unwrap().activity, AgentActivity::Exited);
            assert!(registry.active_run_for_route(&before.route).is_none());
            if !hook_first {
                // Supersession stays explicit; no process exit is invented.
                assert_eq!(registry.run(&first_id), Some(&before));
                assert!(registry.fallback_supersession(&first_id).is_some());
            }
        }
    }

    #[test]
    fn already_bound_weak_session_can_upgrade_to_hook_without_an_alias() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let mut weak = observation(route.clone(), 1, AgentEvidence::PtyActivity);
        weak.provider_session_id = Some("proved-session".into());
        let AgentApplyOutcome::Applied { run_id, .. } = registry.observe(weak) else {
            panic!("expected weak session");
        };
        let mut hook = observation(route, 2, AgentEvidence::Hook);
        hook.provider_session_id = Some("proved-session".into());
        assert_eq!(registry.observe(hook), AgentApplyOutcome::Applied { run_id: run_id.clone(), created: false });
        assert_eq!(registry.runs().count(), 1);
        assert_eq!(registry.run(&run_id).unwrap().evidence, AgentEvidence::Hook);
        assert_eq!(registry.run(&run_id).unwrap().activity, AgentActivity::Working);
        assert!(registry.fallback_supersession(&run_id).is_none());
    }

    #[test]
    fn status_only_hook_supersedes_its_captured_fallback_without_identity_merge() {
        let route = route();
        let tracker = crate::SessionTracker::new();
        tracker.track_input_with_line_snapshot(1, "claude\r", None);
        let episode = tracker.weak_detection_episode(1);
        let mut registry = AgentRuntimeRegistry::default();
        let mut weak = observation(route.clone(), 1, AgentEvidence::PtyActivity);
        weak.weak_episode = episode;
        let AgentApplyOutcome::Applied { run_id: weak_id, .. } = registry.observe(weak) else {
            panic!("input fallback");
        };
        let mut hook = observation(route.clone(), 2, AgentEvidence::Hook);
        hook.weak_episode = episode;
        let AgentApplyOutcome::Applied { run_id: hook_id, created: true } = registry.observe(hook) else {
            panic!("source Hook creates a distinct run without a guessed weak binding");
        };
        assert_ne!(weak_id, hook_id);
        assert!(registry.is_superseded_weak_alias(&weak_id));
        assert_eq!(registry.run(&hook_id).unwrap().provider_session_id, None);
        assert_eq!(registry.run(&hook_id).unwrap().process, None);
        registry.observe_hook_exit(route.clone(), AgentEventId::new(), 3, None, 100);
        assert!(registry.active_run_for_route(&route).is_none());
        assert_eq!(registry.run(&weak_id).unwrap().activity, AgentActivity::Unknown);
    }

    #[test]
    fn supersession_requires_newer_accepted_complete_route_evidence() {
        for changed_field in 0..8 {
            let route = route();
            let mut registry = AgentRuntimeRegistry::default();
            let AgentApplyOutcome::Applied { run_id: weak, .. } = registry.observe(
                observation(route.clone(), 10, AgentEvidence::PtyActivity),
            ) else {
                panic!("expected fallback");
            };
            let before = registry.run(&weak).unwrap().clone();
            let mut hook = observation(route.clone(), 11, AgentEvidence::Hook);
            hook.provider_session_id = Some("session".into());
            // Supersession is exact-route evidence, not provider correlation.
            hook.provider = "codex".parse().unwrap();
            match changed_field {
                0 => hook.route.execution_host_id = ExecutionHostId::derive("other", &HostInstallId::new()),
                1 => hook.route.worktree_id = WorktreeId::derive(&RepoId::derive(&route.execution_host_id, "/other/.git"), "/other", None),
                2 => hook.route.tab_id = TabId::new(),
                3 => hook.route.pane_key = PaneKey::new(),
                4 => hook.route.terminal_session_id = TerminalSessionId::new(),
                5 => hook.route.terminal_incarnation_id = TerminalIncarnationId::new(),
                6 => hook.sequence = 9,
                7 => hook.sequence = 0,
                _ => unreachable!(),
            }
            registry.observe(hook);
            assert!(registry.fallback_supersession(&weak).is_none());
            assert_eq!(registry.run(&weak), Some(&before));
            let mut accepted = observation(route.clone(), 12, AgentEvidence::Hook);
            accepted.provider = "codex".parse().unwrap();
            accepted.provider_session_id = Some("accepted".into());
            let event_id = accepted.event_id.clone();
            assert!(matches!(registry.observe(accepted), AgentApplyOutcome::Applied { .. }));
            assert_eq!(registry.fallback_supersession(&weak), Some(&AgentFallbackSupersession {
                event_id, evidence: AgentEvidence::Hook, sequence: 12, connection_epoch: None,
                weak_episode: None,
            }));
            assert_eq!(registry.run(&weak), Some(&before));
            registry.remove_route(&route);
            assert!(registry.fallback_supersession(&weak).is_none());
        }
    }

    #[test]
    fn supersession_survives_process_retirement_without_hiding_bound_aliases() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let mut weak_ids = Vec::new();
        for (index, binding) in [None, Some("session"), Some("process")].into_iter().enumerate() {
            let mut weak = observation(route.clone(), index as u64 + 1, AgentEvidence::PtyActivity);
            if binding == Some("session") {
                weak.provider_session_id = Some("weak-session".into());
                weak.provider = "codex".parse().unwrap();
            } else if binding == Some("process") {
                weak.process = AgentProcessIdentity::new(50, 150);
                weak.provider = "pi".parse().unwrap();
            }
            let AgentApplyOutcome::Applied { run_id, created: true } = registry.observe(weak) else {
                panic!("expected separate fallback or bound alias");
            };
            weak_ids.push(run_id);
        }
        let p1 = AgentProcessIdentity::new(42, 99).unwrap();
        let p2 = AgentProcessIdentity::new(43, 100).unwrap();
        let ids = registry.apply_process_inventory(inventory_for(&route, 4, &[p1, p2])).unwrap();
        assert_eq!(ids.len(), 2);
        let supersession = registry.fallback_supersession(&weak_ids[0]).unwrap().clone();
        for id in weak_ids[1..].iter().chain(ids.iter()) {
            assert!(registry.fallback_supersession(id).is_none());
        }
        registry.apply_process_inventory(inventory_for(&route, 5, &[])).unwrap();
        assert_eq!(registry.fallback_supersession(&weak_ids[0]), Some(&supersession));
        assert_eq!(registry.run(&weak_ids[0]).unwrap().activity, AgentActivity::Unknown);
        for id in &ids {
            assert_eq!(registry.run(id).unwrap().activity, AgentActivity::Exited);
            assert!(registry.fallback_supersession(id).is_none());
        }
        registry.mark_connectivity(AgentConnectivityObservation {
            event_id: AgentEventId::new(), route, sequence: 6, connection_epoch: Some(8),
            connectivity: AgentConnectivity::Disconnected, received_at_unix_ms: 500,
        }).unwrap();
        assert_eq!(registry.fallback_supersession(&weak_ids[0]), Some(&supersession));
        assert_eq!(registry.run(&weak_ids[0]).unwrap().last_sequence, 1);
    }

    #[test]
    fn batch_supersession_cannot_hide_a_preflight_singleton_upgrade() {
        for reverse in [false, true] {
            let route = route();
            let mut registry = AgentRuntimeRegistry::default();
            let AgentApplyOutcome::Applied { run_id: weak, .. } = registry.observe(
                observation(route.clone(), 1, AgentEvidence::PtyActivity),
            ) else {
                panic!("expected fallback");
            };
            let mut inventory = inventory_for(&route, 2, &[
                AgentProcessIdentity::new(42, 99).unwrap(),
                AgentProcessIdentity::new(43, 100).unwrap(),
            ]);
            inventory.processes[1].provider = "codex".parse().unwrap();
            if reverse { inventory.processes.reverse(); }
            let ids = registry.apply_process_inventory(inventory).unwrap();
            assert_eq!(ids.len(), 2);
            assert!(ids.contains(&weak));
            assert_eq!(registry.run(&weak).unwrap().process, AgentProcessIdentity::new(42, 99));
            assert!(ids.iter().all(|id| registry.fallback_supersession(id).is_none()));
        }
    }

    #[test]
    fn inventory_episode_is_captured_before_delivery_and_not_borrowed_from_new_input() {
        let route = route();
        let tracker = crate::SessionTracker::new();
        tracker.track_input_with_line_snapshot(1, "claude\r", None);
        let captured = tracker.weak_detection_episode(1);
        let mut registry = AgentRuntimeRegistry::default();
        let mut initial = observation(route.clone(), 1, AgentEvidence::PtyActivity);
        initial.weak_episode = captured;
        let AgentApplyOutcome::Applied { run_id: old, .. } = registry.observe(initial) else {
            panic!("first episode");
        };
        tracker.clear_ai_session(1);
        tracker.track_input_with_line_snapshot(1, "claude\r", None);
        let current = tracker.weak_detection_episode(1);
        let mut next = observation(route.clone(), 2, AgentEvidence::PtyActivity);
        next.weak_episode = current;
        let AgentApplyOutcome::Applied { run_id: new, created: true } = registry.observe(next) else {
            panic!("new episode must not reuse the old weak ID");
        };
        assert!(registry.is_superseded_weak_alias(&old));
        assert!(!registry.is_superseded_weak_alias(&new));
        let mut inventory = inventory_for(&route, 3, &[
            AgentProcessIdentity::new(42, 99).unwrap(), AgentProcessIdentity::new(43, 100).unwrap(),
        ]);
        inventory.weak_episode = captured;
        let ids = registry.apply_process_inventory(inventory.clone()).unwrap();
        assert_eq!(ids.len(), 2);
        assert!(!registry.is_superseded_weak_alias(&new), "an older capture cannot invalidate new input");
        assert!(!ids.contains(&new));
        inventory.event_id = AgentEventId::new();
        inventory.sequence = 4;
        inventory.weak_episode = current;
        assert_eq!(registry.apply_process_inventory(inventory).unwrap(), ids);
        assert!(registry.is_superseded_weak_alias(&new));
        assert!(ids.iter().all(|id| !registry.is_superseded_weak_alias(id)));
    }

    #[test]
    fn retired_route_rejects_queued_pty_but_accepts_new_launch() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let tracker = crate::SessionTracker::new();
        tracker.track_input_with_line_snapshot(1, "claude\r", None);
        let old_episode = tracker.weak_detection_episode(1);
        let first = registry
            .apply_process_inventory(AgentProcessInventoryObservation {
                event_id: AgentEventId::new(),
                route: route.clone(),
                sequence: 9,
                connection_epoch: 1,
                weak_episode: old_episode,
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
                weak_episode: None,
                processes: vec![],
                received_at_unix_ms: 11,
            })
            .unwrap();
        let mut delayed = observation(route.clone(), 10, AgentEvidence::PtyActivity);
        delayed.connection_epoch = Some(1);
        delayed.weak_episode = old_episode;
        assert_eq!(
            registry.observe(delayed),
            AgentApplyOutcome::Ignored(AgentObservationIgnored::SupersededWeakEpisode)
        );
        assert!(registry.active_run_for_route(&route).is_none());
        let mut launch = observation(route, 12, AgentEvidence::PtyActivity);
        launch.connection_epoch = Some(1);
        tracker.clear_ai_session(1);
        tracker.track_input_with_line_snapshot(1, "claude\r", None);
        launch.weak_episode = tracker.weak_detection_episode(1);
        let AgentApplyOutcome::Applied { run_id, created } = registry.observe(launch) else {
            panic!("new launch should be accepted");
        };
        assert!(created);
        assert_ne!(run_id, first[0]);
    }

    #[test]
    fn newer_input_episode_fences_unseen_older_and_closed_same_episode() {
        let tracker = crate::SessionTracker::new();
        tracker.track_input_with_line_snapshot(1, "claude\r", None);
        let unseen_old = tracker.weak_detection_episode(1).unwrap();
        tracker.clear_ai_session(1);
        tracker.track_input_with_line_snapshot(1, "claude\r", None);
        let latest = tracker.weak_detection_episode(1).unwrap();
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let mut weak = observation(route.clone(), 1, AgentEvidence::PtyActivity);
        weak.weak_episode = Some(latest);
        let AgentApplyOutcome::Applied { run_id, .. } = registry.observe(weak.clone()) else {
            panic!("real input episode");
        };
        for ended in [false, true] {
            if ended {
                let mut exit = weak.clone();
                exit.event_id = AgentEventId::new();
                exit.sequence = 2;
                exit.activity = AgentActivity::Exited;
                assert!(matches!(registry.observe(exit), AgentApplyOutcome::Applied { .. }));
            }
            for episode in [Some(unseen_old), ended.then_some(latest)] {
                let Some(episode) = episode else { continue; };
                let mut late = weak.clone();
                late.event_id = AgentEventId::new();
                late.sequence = 99;
                late.connection_epoch = Some(99);
                late.weak_episode = Some(episode);
                assert_eq!(registry.observe(late), AgentApplyOutcome::Ignored(AgentObservationIgnored::SupersededWeakEpisode));
                assert_eq!(registry.runs().count(), 1);
                assert_eq!(registry.run(&run_id).unwrap().connection_epoch, None);
            }
        }
    }

    #[test]
    fn pty_activity_neither_classifies_nor_ends_process_attested_run() {
        let route = route();
        let process = AgentProcessIdentity::new(42, 99).unwrap();
        let mut registry = AgentRuntimeRegistry::default();
        let run_id = registry
            .apply_process_inventory(AgentProcessInventoryObservation {
                event_id: AgentEventId::new(),
                route: route.clone(),
                sequence: 1,
                connection_epoch: 7,
                weak_episode: None,
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
            AgentActivity::Unknown
        );

        let mut exited = observation(route, 3, AgentEvidence::PtyActivity);
        exited.connection_epoch = Some(7);
        exited.activity = AgentActivity::Exited;
        assert!(matches!(
            registry.observe(exited),
            AgentApplyOutcome::Applied { created: false, .. }
        ));
        let state = registry.run(&run_id).unwrap();
        assert_eq!(state.activity, AgentActivity::Unknown);
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
                weak_episode: None,
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
                weak_episode: None,
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
            weak_episode: None,
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
                weak_episode: None,
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
        assert_eq!(state.activity, AgentActivity::Unknown);
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

    fn attested_registry() -> (AgentRuntimeRegistry, AgentRunId, AgentRoute) {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let mut process = observation(route.clone(), 1, AgentEvidence::ProcessAttested);
        process.process = AgentProcessIdentity::new(42, 99);
        process.connection_epoch = Some(7);
        process.received_at_unix_ms = 100;
        let AgentApplyOutcome::Applied { run_id, .. } = registry.observe(process) else {
            panic!("expected process");
        };
        (registry, run_id, route)
    }

    fn semantic(state: &AgentRuntimeState, sequence: u64, activity: AgentActivity) -> AgentSemanticObservation {
        AgentSemanticObservation {
            event_id: AgentEventId::new(),
            run_id: state.run_id.clone(),
            route: state.route.clone(),
            provider: state.provider.clone(),
            owner: AgentSemanticOwner::ForegroundProcess(state.process.unwrap()),
            sequence,
            connection_epoch: state.connection_epoch,
            activity,
            observed_at_unix_ms: 100 + sequence as i64,
            received_at_unix_ms: 100 + sequence as i64,
        }
    }

    #[test]
    fn inventory_and_output_cannot_manufacture_task_semantics() {
        let (mut registry, run_id, route) = attested_registry();
        let tracker = crate::SessionTracker::new();
        for output in ["shell output", "\x1b[2J\x1b[H", "done", "permission requested"] {
            tracker.note_output(1, output);
            assert!(!tracker.is_ai_session(1));
        }
        tracker.track_input_with_line_snapshot(1, "claude\r", None);
        assert!(tracker.is_ai_session(1));
        tracker.note_output(1, "unrelated shell output");
        let weak = AgentObservation {
            connection_epoch: Some(7),
            ..observation(route.clone(), 2, AgentEvidence::PtyActivity)
        };
        registry.observe(weak);
        for (sequence, claimed) in [
            (3, AgentActivity::Working),
            (4, AgentActivity::Waiting),
            (5, AgentActivity::Done),
            (6, AgentActivity::Blocked),
        ] {
            registry.apply_process_inventory(AgentProcessInventoryObservation {
                event_id: AgentEventId::new(), route: route.clone(), sequence,
                connection_epoch: 7,
                weak_episode: None,
                processes: vec![AgentProcessObservation {
                    provider: "claude".parse().unwrap(),
                    process: AgentProcessIdentity::new(42, 99).unwrap(), activity: claimed,
                }],
                received_at_unix_ms: 200,
            }).unwrap();
            assert_eq!(registry.run(&run_id).unwrap().activity, AgentActivity::Unknown);
            assert_eq!(registry.activity_freshness(&run_id, 200), AgentActivityFreshness::Unknown);
        }
        assert_eq!(registry.runs().count(), 1);
    }

    #[test]
    fn exact_semantics_survive_inventory_weak_output_and_expiry() {
        let tracker = crate::SessionTracker::new();
        tracker.track_input_with_line_snapshot(1, "claude\r", None);
        for activity in [AgentActivity::Working, AgentActivity::Waiting, AgentActivity::Blocked,
            AgentActivity::Done, AgentActivity::Failed] {
            let (mut registry, run_id, route) = attested_registry();
            let observed = semantic(registry.run(&run_id).unwrap(), 2, activity);
            assert!(matches!(registry.observe_semantic(observed.clone()), AgentApplyOutcome::Applied { created: false, .. }));
            registry.apply_process_inventory(AgentProcessInventoryObservation {
                event_id: AgentEventId::new(), route: route.clone(), sequence: 3,
                connection_epoch: 7,
                weak_episode: tracker.weak_detection_episode(1),
                processes: vec![AgentProcessObservation {
                    provider: "claude".parse().unwrap(),
                    process: AgentProcessIdentity::new(42, 99).unwrap(), activity: AgentActivity::Working,
                }], received_at_unix_ms: 500,
            }).unwrap();
            let weak = AgentObservation {
                connection_epoch: Some(7),
                ..observation(route, 4, AgentEvidence::PtyActivity)
            };
            registry.observe(weak);
            assert_eq!(registry.run(&run_id).unwrap().activity, activity);
            assert_eq!(registry.run(&run_id).unwrap().last_event_id, observed.event_id);
            assert_eq!(registry.run(&run_id).unwrap().received_at_unix_ms, observed.received_at_unix_ms);
            assert_eq!(registry.run(&run_id).unwrap().last_sequence, 4);
            assert_eq!(registry.activity_freshness(&run_id, 500), AgentActivityFreshness::Fresh);
            assert_eq!(registry.activity_freshness(&run_id, 20_000), AgentActivityFreshness::Stale);
            assert_eq!(registry.run(&run_id).unwrap().connectivity, AgentConnectivity::Live);
            let before = registry.run(&run_id).unwrap().clone();
            let repeated = AgentSemanticObservation { event_id: AgentEventId::new(), sequence: 5, received_at_unix_ms: 600, ..observed };
            assert_eq!(registry.observe_semantic(repeated), AgentApplyOutcome::Ignored(AgentObservationIgnored::StaleSemanticObservation));
            assert_eq!(registry.run(&run_id), Some(&before));
        }
    }

    #[test]
    fn semantic_identity_order_and_freshness_fences_fail_closed() {
        let (mut registry, run_id, _) = attested_registry();
        let before = registry.run(&run_id).unwrap().clone();
        for field in 0..14 {
            let mut event = semantic(&before, 2, AgentActivity::Waiting);
            match field {
                0 => event.route.terminal_incarnation_id = TerminalIncarnationId::new(),
                1 => event.route.pane_key = PaneKey::new(),
                2 => event.route.tab_id = TabId::new(),
                3 => event.route.terminal_session_id = TerminalSessionId::new(),
                4 => event.route.execution_host_id = ExecutionHostId::derive("other", &HostInstallId::new()),
                5 => event.route.worktree_id = WorktreeId::derive(&RepoId::derive(&event.route.execution_host_id, "/other/.git"), "/other", None),
                6 => event.provider = "codex".parse().unwrap(),
                7 => event.owner = AgentSemanticOwner::ForegroundProcess(AgentProcessIdentity::new(42, 100).unwrap()),
                8 => event.owner = AgentSemanticOwner::ProviderSession("unbound-session".into()),
                9 => event.connection_epoch = Some(8),
                10 => event.sequence = 1,
                11 => event.observed_at_unix_ms = 99,
                12 => event.received_at_unix_ms = 50_000,
                13 => event.observed_at_unix_ms = 103,
                _ => unreachable!(),
            }
            assert!(matches!(registry.observe_semantic(event), AgentApplyOutcome::Ignored(_)), "field {field}");
            assert_eq!(registry.run(&run_id), Some(&before));
        }
        let mut bound = observation(before.route.clone(), 2, AgentEvidence::ProcessAttested);
        bound.process = before.process;
        bound.provider_session_id = Some("exact-session".into());
        bound.connection_epoch = before.connection_epoch;
        registry.observe(bound);
        let mut event = semantic(registry.run(&run_id).unwrap(), 3, AgentActivity::Blocked);
        event.owner = AgentSemanticOwner::ProviderSession("exact-session".into());
        assert!(matches!(registry.observe_semantic(event), AgentApplyOutcome::Applied { created: false, .. }));
    }

    #[test]
    fn independent_processes_and_sessions_do_not_borrow_unbound_observations() {
        let (mut registry, first, route) = attested_registry();
        let mut independent = observation(route.clone(), 2, AgentEvidence::ProcessAttested);
        independent.process = AgentProcessIdentity::new(43, 100);
        independent.connection_epoch = Some(7);
        assert!(matches!(registry.observe(independent), AgentApplyOutcome::Applied { created: true, .. }));
        let before = registry.run(&first).unwrap().clone();
        let weak = AgentObservation {
            connection_epoch: Some(7),
            ..observation(route.clone(), 3, AgentEvidence::PtyActivity)
        };
        assert_eq!(registry.observe(weak), AgentApplyOutcome::Ignored(AgentObservationIgnored::AmbiguousRun));
        assert_eq!(registry.run(&first), Some(&before));
        let mut other_provider = observation(route, 4, AgentEvidence::Hook);
        other_provider.provider = "codex".parse().unwrap();
        other_provider.connection_epoch = Some(7);
        assert!(matches!(registry.observe(other_provider), AgentApplyOutcome::Applied { created: true, .. }));
        assert_eq!(registry.runs().count(), 3);
    }

    #[test]
    fn hooks_override_owned_semantics_but_never_the_other_way_round() {
        let (mut registry, run_id, route) = attested_registry();
        let event = semantic(registry.run(&run_id).unwrap(), 2, AgentActivity::Working);
        registry.observe_semantic(event);
        let mut hook = observation(route, 3, AgentEvidence::Hook);
        hook.process = AgentProcessIdentity::new(42, 99);
        hook.connection_epoch = Some(7);
        hook.activity = AgentActivity::Blocked;
        registry.observe(hook);
        let before = registry.run(&run_id).unwrap().clone();
        let event = semantic(&before, 4, AgentActivity::Done);
        assert_eq!(registry.observe_semantic(event), AgentApplyOutcome::Ignored(AgentObservationIgnored::StrongerEvidence));
        assert_eq!(registry.run(&run_id), Some(&before));
        assert_eq!(registry.activity_freshness(&run_id, 50_000), AgentActivityFreshness::Fresh);
    }

    #[test]
    fn old_inventory_cannot_create_a_process_after_retirement() {
        let (mut registry, _, route) = attested_registry();
        let empty = AgentProcessInventoryObservation {
            event_id: AgentEventId::new(), route: route.clone(), sequence: 3,
            connection_epoch: 7, weak_episode: None, processes: vec![], received_at_unix_ms: 200,
        };
        registry.apply_process_inventory(empty.clone()).unwrap();
        let mut delayed = empty;
        delayed.event_id = AgentEventId::new();
        delayed.sequence = 2;
        delayed.processes.push(AgentProcessObservation {
            provider: "claude".parse().unwrap(), process: AgentProcessIdentity::new(43, 200).unwrap(),
            activity: AgentActivity::Working,
        });
        assert_eq!(registry.apply_process_inventory(delayed.clone()), Err(AgentObservationIgnored::OutOfOrder));
        assert!(registry.active_run_for_route(&route).is_none());
        delayed.sequence = 4;
        delayed.event_id = AgentEventId::new();
        assert_eq!(registry.apply_process_inventory(delayed).unwrap().len(), 1);
        assert_eq!(registry.active_run_for_route(&route).unwrap().activity, AgentActivity::Unknown);
    }

    #[test]
    fn reconnect_and_owner_epoch_changes_do_not_revalidate_old_titles() {
        let (mut registry, run_id, route) = attested_registry();
        let old = semantic(registry.run(&run_id).unwrap(), 2, AgentActivity::Waiting);
        registry.observe_semantic(old.clone());
        registry.mark_connectivity(AgentConnectivityObservation {
            event_id: AgentEventId::new(), route: route.clone(), sequence: 3,
            connection_epoch: Some(8), connectivity: AgentConnectivity::Disconnected,
            received_at_unix_ms: 500,
        }).unwrap();
        assert_eq!(registry.run(&run_id).unwrap().activity, AgentActivity::Waiting);
        registry.apply_process_inventory(AgentProcessInventoryObservation {
            event_id: AgentEventId::new(), route, sequence: 4, connection_epoch: 8, weak_episode: None,
            processes: vec![AgentProcessObservation {
                provider: "claude".parse().unwrap(), process: AgentProcessIdentity::new(42, 99).unwrap(),
                activity: AgentActivity::Working,
            }], received_at_unix_ms: 600,
        }).unwrap();
        assert_eq!(registry.activity_freshness(&run_id, 600), AgentActivityFreshness::Stale);
        let stale = AgentSemanticObservation {
            sequence: 5, connection_epoch: Some(8), received_at_unix_ms: 600,
            observed_at_unix_ms: 400, ..old
        };
        let before = registry.run(&run_id).unwrap().clone();
        assert_eq!(registry.observe_semantic(stale), AgentApplyOutcome::Ignored(AgentObservationIgnored::StaleSemanticObservation));
        assert_eq!(registry.run(&run_id), Some(&before));
        let fresh = AgentSemanticObservation {
            observed_at_unix_ms: 601, received_at_unix_ms: 601,
            ..semantic(&before, 5, AgentActivity::Working)
        };
        assert!(matches!(registry.observe_semantic(fresh), AgentApplyOutcome::Applied { .. }));
        assert_eq!(registry.activity_freshness(&run_id, 602), AgentActivityFreshness::Fresh);
    }

    #[test]
    fn weak_new_epoch_invalidates_semantics_without_borrowing_old_capture_time() {
        let (mut registry, run_id, route) = attested_registry();
        registry.observe_semantic(semantic(registry.run(&run_id).unwrap(), 2, AgentActivity::Waiting));
        let mut weak = observation(route.clone(), 3, AgentEvidence::PtyActivity);
        weak.connection_epoch = Some(8);
        weak.received_at_unix_ms = 500;
        let reconnect_event = weak.event_id.clone();
        assert!(matches!(registry.observe(weak), AgentApplyOutcome::Applied { created: false, .. }));
        let before = registry.run(&run_id).unwrap().clone();
        assert_eq!(before.activity, AgentActivity::Waiting);
        assert_eq!(before.last_event_id, reconnect_event);
        assert_eq!(registry.activity_freshness(&run_id, 500), AgentActivityFreshness::Stale);
        let stale = AgentSemanticObservation {
            observed_at_unix_ms: 400,
            received_at_unix_ms: 501,
            ..semantic(&before, 4, AgentActivity::Working)
        };
        assert_eq!(registry.observe_semantic(stale), AgentApplyOutcome::Ignored(AgentObservationIgnored::StaleSemanticObservation));
        assert_eq!(registry.run(&run_id), Some(&before));
        let mut resumed = observation(route, 4, AgentEvidence::ProcessAttested);
        resumed.connection_epoch = Some(8);
        resumed.process = before.process;
        resumed.received_at_unix_ms = 600;
        assert!(matches!(registry.observe(resumed), AgentApplyOutcome::Applied { created: false, .. }));
        assert_eq!(registry.run(&run_id).unwrap().last_event_id, reconnect_event);
        assert_eq!(registry.run(&run_id).unwrap().received_at_unix_ms, 500);
        assert_eq!(registry.activity_freshness(&run_id, 600), AgentActivityFreshness::Stale);
    }

    #[test]
    fn same_millisecond_epoch_change_cannot_keep_an_old_title_fresh() {
        let (mut registry, run_id, route) = attested_registry();
        let capture = semantic(registry.run(&run_id).unwrap(), 2, AgentActivity::Waiting);
        let captured_at = capture.observed_at_unix_ms;
        registry.observe_semantic(capture);
        let mut weak = observation(route, 3, AgentEvidence::PtyActivity);
        weak.connection_epoch = Some(8);
        weak.received_at_unix_ms = captured_at;
        assert!(matches!(registry.observe(weak), AgentApplyOutcome::Applied { created: false, .. }));
        assert_eq!(registry.run(&run_id).unwrap().activity, AgentActivity::Waiting);
        assert_eq!(registry.activity_freshness(&run_id, captured_at), AgentActivityFreshness::Stale);
    }

    #[test]
    fn changed_liveness_facts_and_hook_semantics_still_publish_new_events() {
        let (mut registry, run_id, route) = attested_registry();
        registry.observe_semantic(semantic(registry.run(&run_id).unwrap(), 2, AgentActivity::Done));
        registry.mark_connectivity(AgentConnectivityObservation {
            event_id: AgentEventId::new(), route: route.clone(), sequence: 3,
            connection_epoch: Some(7), connectivity: AgentConnectivity::Disconnected,
            received_at_unix_ms: 200,
        }).unwrap();
        let mut live = observation(route.clone(), 4, AgentEvidence::ProcessAttested);
        live.process = registry.run(&run_id).unwrap().process;
        live.connection_epoch = Some(7);
        live.received_at_unix_ms = 300;
        let event_id = live.event_id.clone();
        registry.observe(live);
        let state = registry.run(&run_id).unwrap();
        assert_eq!(state.last_event_id, event_id);
        assert_eq!(state.received_at_unix_ms, 300);
        assert_eq!(state.activity, AgentActivity::Done);
        let process = state.process;
        for sequence in [5, 6] {
            let mut hook = observation(route.clone(), sequence, AgentEvidence::Hook);
            hook.process = process;
            hook.connection_epoch = Some(7);
            hook.activity = AgentActivity::Done;
            hook.received_at_unix_ms = 300 + sequence as i64;
            let event_id = hook.event_id.clone();
            registry.observe(hook);
            assert_eq!(registry.run(&run_id).unwrap().last_event_id, event_id);
        }
    }

    #[test]
    fn duplicate_session_identity_cannot_select_an_arbitrary_run() {
        let route = route();
        let mut registry = AgentRuntimeRegistry::default();
        let mut runs = Vec::new();
        for (sequence, pid) in [(1, 42), (2, 43)] {
            let mut process = observation(route.clone(), sequence, AgentEvidence::ProcessAttested);
            process.process = AgentProcessIdentity::new(pid, 99);
            process.provider_session_id = Some("shared-session".into());
            let AgentApplyOutcome::Applied { run_id, created: true } = registry.observe(process) else {
                panic!("independent identity");
            };
            runs.push(run_id);
        }
        let before = registry.run(&runs[0]).unwrap().clone();
        let mut event = semantic(&before, 3, AgentActivity::Done);
        event.owner = AgentSemanticOwner::ProviderSession("shared-session".into());
        assert_eq!(registry.observe_semantic(event), AgentApplyOutcome::Ignored(AgentObservationIgnored::UnresolvedSemanticOwner));
        let mut hook = observation(route, 3, AgentEvidence::Hook);
        hook.provider_session_id = Some("shared-session".into());
        assert_eq!(registry.observe(hook), AgentApplyOutcome::Ignored(AgentObservationIgnored::AmbiguousRun));
        assert_eq!(registry.run(&runs[0]), Some(&before));
        assert_eq!(registry.runs().count(), 2);
    }

    #[test]
    fn rejected_new_epoch_observations_have_no_route_side_effects() {
        let (mut registry, first, route) = attested_registry();
        let mut peer = observation(route.clone(), 2, AgentEvidence::ProcessAttested);
        peer.process = AgentProcessIdentity::new(43, 100);
        peer.connection_epoch = Some(7);
        assert!(matches!(registry.observe(peer), AgentApplyOutcome::Applied { created: true, .. }));
        let before = registry.runs.clone();
        let epochs_before = registry.latest_epoch_by_route.clone();
        let owners_before = registry.process_owner_since.clone();
        let mut ambiguous = observation(route.clone(), 1, AgentEvidence::Hook);
        ambiguous.connection_epoch = Some(8);
        assert_eq!(
            registry.observe(ambiguous),
            AgentApplyOutcome::Ignored(AgentObservationIgnored::AmbiguousRun)
        );
        assert_eq!(registry.runs, before);
        assert_eq!(registry.latest_epoch_by_route, epochs_before);
        assert_eq!(registry.process_owner_since, owners_before);

        let mut valid = observation(route.clone(), 3, AgentEvidence::ProcessAttested);
        valid.process = before[&first].process;
        valid.connection_epoch = Some(7);
        assert!(matches!(registry.observe(valid), AgentApplyOutcome::Applied { created: false, .. }));

        let mut exit = observation(route.clone(), 4, AgentEvidence::Hook);
        exit.process = before[&first].process;
        exit.connection_epoch = Some(7);
        exit.activity = AgentActivity::Exited;
        assert!(matches!(registry.observe(exit), AgentApplyOutcome::Applied { created: false, .. }));
        let before = registry.runs.clone();
        let mut replay = observation(route.clone(), 1, AgentEvidence::ProcessAttested);
        replay.process = before[&first].process;
        replay.connection_epoch = Some(8);
        assert_eq!(registry.observe(replay), AgentApplyOutcome::Ignored(AgentObservationIgnored::EndedRun));
        assert_eq!(registry.runs, before);
        assert_eq!(registry.latest_epoch_by_route, epochs_before);
    }

    #[test]
    fn semantic_rejections_do_not_consume_event_identity_or_mutate_state() {
        let (mut registry, run_id, _) = attested_registry();
        let before = registry.run(&run_id).unwrap().clone();
        for activity in [AgentActivity::Unknown, AgentActivity::Exited, AgentActivity::Interrupted] {
            let rejected = semantic(&before, 2, activity);
            let event_id = rejected.event_id.clone();
            assert_eq!(registry.observe_semantic(rejected), AgentApplyOutcome::Ignored(AgentObservationIgnored::InvalidSemanticActivity));
            assert!(!registry.seen_event_ids.contains(&event_id));
            assert_eq!(registry.run(&run_id), Some(&before));
        }
        for field in 0..5 {
            let mut rejected = semantic(&before, 2, AgentActivity::Waiting);
            match field {
                0 => rejected.run_id = AgentRunId::new(),
                1 => rejected.owner = AgentSemanticOwner::ForegroundProcess(AgentProcessIdentity::new(43, 99).unwrap()),
                2 => rejected.sequence = 0,
                3 => rejected.connection_epoch = None,
                4 => rejected.observed_at_unix_ms = -1,
                _ => unreachable!(),
            }
            let event_id = rejected.event_id.clone();
            assert!(matches!(registry.observe_semantic(rejected), AgentApplyOutcome::Ignored(_)));
            assert!(!registry.seen_event_ids.contains(&event_id));
            assert_eq!(registry.run(&run_id), Some(&before));
        }
        let accepted = semantic(&before, 2, AgentActivity::Waiting);
        assert!(matches!(registry.observe_semantic(accepted.clone()), AgentApplyOutcome::Applied { created: false, .. }));
        let before = registry.run(&run_id).unwrap().clone();
        let duplicate = AgentSemanticObservation {
            sequence: 3,
            observed_at_unix_ms: 103,
            received_at_unix_ms: 103,
            ..accepted
        };
        assert_eq!(registry.observe_semantic(duplicate), AgentApplyOutcome::Ignored(AgentObservationIgnored::DuplicateEvent));
        assert_eq!(registry.run(&run_id), Some(&before));
    }

    #[test]
    fn local_input_launch_is_live_unknown_and_preserves_public_tracker_api() {
        let tracker = crate::SessionTracker::new();
        tracker.track_input_with_line_snapshot(1, "codex\r", None);
        let mut registry = AgentRuntimeRegistry::default();
        let mut launch = observation(route(), 1, AgentEvidence::PtyActivity);
        launch.provider = tracker.ai_session_agent(1).unwrap().parse().unwrap();
        let AgentApplyOutcome::Applied { run_id, created: true } = registry.observe(launch) else {
            panic!("input launch must retain weak liveness");
        };
        let run = registry.run(&run_id).unwrap();
        assert_eq!(run.activity, AgentActivity::Unknown);
        assert_eq!(run.confirmation, AgentConfirmation::LiveConfirmed);
        assert_eq!(run.connectivity, AgentConnectivity::Live);
        assert_eq!(run.evidence, AgentEvidence::PtyActivity);
        assert_eq!(run.provider.as_str(), "codex");
    }

    #[test]
    fn invalid_process_and_ended_semantic_observations_are_rejected() {
        let (mut registry, run_id, route) = attested_registry();
        for process in [
            AgentProcessIdentity { pid: 0, start_ticks: 99 },
            AgentProcessIdentity { pid: 42, start_ticks: 0 },
        ] {
            let mut invalid = observation(route.clone(), 2, AgentEvidence::ProcessAttested);
            invalid.process = Some(process);
            invalid.connection_epoch = Some(7);
            assert_eq!(registry.observe(invalid), AgentApplyOutcome::Ignored(AgentObservationIgnored::InvalidProcess));
        }
        let event = semantic(registry.run(&run_id).unwrap(), 3, AgentActivity::Done);
        registry.apply_process_inventory(AgentProcessInventoryObservation {
            event_id: AgentEventId::new(), route, sequence: 2, connection_epoch: 7, weak_episode: None,
            processes: vec![], received_at_unix_ms: 102,
        }).unwrap();
        assert_eq!(registry.observe_semantic(event), AgentApplyOutcome::Ignored(AgentObservationIgnored::EndedRun));
        assert_eq!(registry.run(&run_id).unwrap().activity, AgentActivity::Exited);
    }
}
