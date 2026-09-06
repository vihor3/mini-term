# Agent Runtime Contract

## Scope

Use this contract for live agent identity and activity that may arrive from
Hook events, PTY heuristics, authenticated process inventory, or restored
history. `mt-ai` owns reconciliation semantics and remains independent of UI,
SSH transport, and process-local PTY identifiers.

## Canonical Model

```rust
pub struct AgentRoute {
    pub execution_host_id: ExecutionHostId,
    pub worktree_id: WorktreeId,
    pub tab_id: TabId,
    pub pane_key: PaneKey,
    pub terminal_session_id: TerminalSessionId,
    pub terminal_incarnation_id: TerminalIncarnationId,
}

pub enum AgentEvidence {
    RestoredHistory,
    PtyActivity,
    ProcessAttested,
    Hook,
}
```

Every accepted observation has a random `AgentEventId`, a nonzero monotonic
sequence, an exact route, and an optional authenticated connection epoch. A
live run has a random `AgentRunId`; provider session IDs and `(pid,
start_ticks)` are matching evidence, not public run identity.

## Reconciliation

- Reject duplicate event IDs, zero sequences, zero epochs, older epochs, and
  out-of-order observations for the matched run. Rejection is side-effect-free:
  validate ownership/order before accepting an epoch or disconnecting siblings.
- Retirement does not erase the route's ordering evidence. A queued weak PTY
  event older than the retained route watermark cannot create a replacement
  heuristic run merely because no active run matches. A genuinely later launch
  can create a new run on the still-live terminal route.
- Match by exact process identity or an already bound valid provider session.
  Provider equality alone cannot merge an unbound Hook with Process/PTY
  evidence. A singleton weak-PTY/process upgrade requires pre-batch uniqueness;
  inventory order must not hide independently proved processes. A different
  incarnation is a different route.
- Evidence order is `Hook > ProcessAttested > PtyActivity > RestoredHistory`.
  Weaker evidence cannot replace provider, process, confirmation, or semantic
  terminal state.
- Process inventories prove liveness, not task activity. Compatibility activity
  inputs on process observations are normalized to `Unknown`; non-ended weak
  PTY observations are likewise unknown. Repeated process inventories retain
  accepted semantics without renewing their original semantic timestamp. An
  unchanged weak liveness update advances replay/order bookkeeping but preserves
  presentation `last_event_id` and `received_at_unix_ms`; it must not renew unread
  or attention. Accepted changes to owner/provider/session/activity/connectivity/
  confirmation/evidence/epoch remain observable.
- There is no generic PTY-recency exception. PTY working/waiting cannot replace
  process-owned semantics, and weak idle/error cannot end a process-owned run.
  Run-owned title/session semantics use `observe_semantic` below; Hook evidence
  remains stronger than that separate acceptance path.
- A successful process inventory marks missing process-attested runs exited.
  Probe errors and reconnects only change connectivity; they never invent a
  semantic done/blocked result or clear last known activity.
- Hook remains authoritative for blocked/done/failed semantics and provider
  session identity.
- History observations are always restored-unconfirmed and never prove a live
  run.

## Legacy Projection

```text
starting / working / blocked -> ai-working
waiting / done               -> ai-idle
failed                        -> error
interrupted / exited / unknown -> idle
```

This is a one-way compatibility projection. Existing four-state consumers must
not become a source of stronger rich-state evidence.
Application consumers must honor `AgentApplyOutcome` before emitting legacy
status or attention effects, then project accepted state rather than the raw
observation. See the mt-app remote-agent reconciliation contract for terminal
observer teardown and process-absence hysteresis.

## Required Tests

- Provider aliases normalize; invalid provider keys fail closed.
- Route, event, epoch, and sequence fences reject stale observations.
- Process evidence upgrades one heuristic run and preserves the run ID.
- PTY working/waiting cannot classify or refresh process activity; PTY exit
  cannot end a process-attested run. Process-only activity is `Unknown` even
  when the compatibility input claimed Working or Waiting.
- A successful empty inventory ends missing processes; connectivity-only
  changes preserve activity.
- A queued older PTY event after process retirement creates no replacement run;
  a later genuine launch gets a new run identity. Execute regressions only in
  GitHub Actions, including any disposable process fixtures.

## Scenario: Source-Owned Weak Detection Episodes

### 1. Scope / Trigger

Use this boundary when input heuristics precede a first Hook or authenticated
process sample, and when the same terminal later launches another Agent. Weak
fallback replacement is separate from identity correlation or process exit.

### 2. Signatures

```text
AgentWeakEpisode: opaque source-minted Copy/Eq/Ord/Hash identity
SessionTracker::weak_detection_episode(pty_id) -> Option<AgentWeakEpisode>
SessionTracker::clear_ai_session_if_episode(pty_id, expected_episode) -> bool
StatusEmitter::emit_if_changed_with_episode(..., Option<AgentWeakEpisode>)

StatusChange, SessionIdentity, AgentObservation, AgentRuntimeState,
AgentProcessInventoryObservation -> weak_episode: Option<AgentWeakEpisode>

AgentRuntimeRegistry::fallback_supersession(run_id)
    -> Option<&AgentFallbackSupersession>
AgentRuntimeRegistry::is_superseded_weak_alias(run_id) -> bool
AgentFallbackSupersession = accepted event_id/evidence/sequence/epoch/weak_episode
```

### 3. Contracts

- Only real input detection mints an episode. Echo detection reuses its
  captured real-input episode; output recency, Hook marking and polls do not
  mint one. Clearing detection retains the latest episode to identify queued
  events; exact pane purge removes tracker state.
- Inventory-driven clearing compares the captured episode and clears under
  the existing outer `ai_sessions` mutation lock. Input mint/publication, echo
  promotion, ordinary clear and pane purge acquire that lock before auxiliary
  maps. Refuse a different published OR pending episode, including a newer
  pending input not yet recognized through echo. There is no compare-unlock-
  plain-clear sequence. None matches only no published/pending episode; a
  matching idempotent clear returns true. Retain the published episode after
  success, remove pending echo state, and never call Hook handling while holding
  the mutation lock. Rejected clearing leaves the new detection unchanged.
- Status and identity events retain the episode at emission. The app forwards
  that field rather than rereading the tracker on channel delivery. Inventory
  requests capture the episode at scheduling, not at completion.
- Accepted live Hook/process evidence may supersede a live-confirmed unbound
  PtyActivity fallback with no process and no provider session. It does not
  merge their identities or manufacture an Exited state. Episode comparisons
  prevent an older capture from invalidating a newer input episode. A genuinely
  new accepted weak episode likewise supersedes older unbound fallbacks.
- Supersession is sticky until exact route removal. `runs`, `runs_for_worktree`
  and `run` retain audit rows; matching and `active_run_for_route` exclude them.
  Every consumer must exclude them from liveness, provider projection, counts,
  titles, activation and close warnings. Never use a rendering-only dedupe.
- Hook exit, disconnect and successful empty inventory do not revive a
  superseded fallback. New sequence, receipt time, polling or reconnect cannot
  turn an old episode into a new launch. Legacy None-episode input cannot
  relaunch a fenced fallback. A genuinely later episode gets a NEW run ID.
- Do not suppress independently attested processes or already session-bound
  runs. Multiple candidates stay distinct; source-owned fallback supersession
  is not provider-wide route deduplication.
- A genuinely later different-provider episode B is not the old episode-A
  alias. A retained Hook captured at A cannot cover B merely by arriving later
  or remaining live. Keep B Unknown until covering accepted source evidence or
  real retirement. Same-provider identity ambiguity is still rejected instead
  of guessed; this is not permission for same-episode duplicates.
- Production monitoring emits compatibility liveness only for unhooked input.
  It never writes Hook state or infers Working/Waiting/exit after silence.
  Poll status/cause dedup also observes episode changes so a genuine later
  launch is visible even when its compatibility status is unchanged.

### 4. Validation & Error Matrix

| Condition | Required behavior |
| --- | --- |
| Weak input then first session Hook without PID | Separate identities; old unbound fallback becomes audit-only |
| First Hook then weak input from same episode | No additional actionable Agent |
| Stronger run exits; idle polls repeat | Do not revive fallback or retain phantom close warning |
| Queued old episode arrives with fresh sequence/epoch | Reject; no new run or side effects |
| Real later input episode arrives | New fallback ID, not reuse of old audit row |
| Old scheduled process sample completes after newer input | Cannot supersede newer weak episode |
| Old empty inventory retires A while B is pending before echo | Retire A, but conditional tracker clear refuses to erase B |
| Two independently proved same-provider processes | Keep both; no provider-only Hook matching |
| Unhooked output or long silence | No manufactured task state or Hook mutation |

### 5. Good / Base / Bad

- Good: public input, first Hook identity/status and Hook exit leave no phantom
  live Agent; a later public input launches a distinct fallback.
- Base: no semantic telemetry means Unknown activity, not automatic Working.
- Bad: hide the weak row while still counting it for title ownership and close
  warnings, or reread the latest tracker episode for an older queued event.

### 6. Tests Required

- Use public input plus production poll/emitter dedup, Hook identity/status/end,
  repeated idle, later input and queued old episodes in one lifecycle regression.
- Assert sticky audit records, accepted supersession provenance, all liveness
  projections, no mutation on rejection, and independent proved-process counts.
- Reverse Hook/input and process inventory ordering, including bound sessions,
  competing weak aliases and genuinely later input between request and reply.
- Conditional-clear tests cover matching/idempotent/None, newer recognized and
  pending input, and concurrent input/echo promotion. The app must pass the
  scheduling-time episode through its accepted-retirement consumer.
- Author tests against source APIs; cross-crate tests must not call mt-ai-only
  cfg(test) helpers. Execute them only in Actions, not the local workspace.

### 7. Wrong vs Correct

```text
Wrong: dedupe same-provider UI rows; keep both active internally
Correct: source episode -> accepted runtime supersession -> audit-only fallback
         -> consistent liveness/title/count/activation/close projections
```

## Scenario: Owned Semantic Activity

### 1. Scope / Trigger

Use this boundary for a provider-specific native title or exact provider-session
semantic observation after positive live ownership has been established. Title
decoding alone does not prove ownership, foreground status, or process liveness.
This supersedes the older generic output-recency exception above.

### 2. Signatures

```rust
pub enum AgentSemanticOwner {
    ForegroundProcess(AgentProcessIdentity),
    ProviderSession(String),
}

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

pub fn activity_from_owned_title(
    provider: &AgentProvider,
    title: &str,
) -> Option<AgentActivity>;

impl AgentRuntimeRegistry {
    pub fn observe_semantic(
        &mut self,
        observation: AgentSemanticObservation,
    ) -> AgentApplyOutcome;
    pub fn activity_freshness(
        &self,
        run_id: &AgentRunId,
        now_unix_ms: i64,
    ) -> AgentActivityFreshness;
}
```

### 3. Contracts

- Require exact run/route/provider, live confirmation, live connectivity,
  process-or-Hook evidence, and equal authenticated epoch. Process identity must
  match PID/start ticks; a provider-session owner must match an already bound
  nonempty, control-free session ID no larger than 512 bytes. This bound applies
  to semantic owner input, not a claim that ordinary observation parsing has the
  same validation. The owner must uniquely identify a live same-route/provider
  run. The caller proves foreground ownership when capturing a title, not by
  borrowing whichever process is newest at delivery time.
- A Hook-owned run rejects the weaker semantic path with `StrongerEvidence`.
  Semantic observations cannot end the process: Starting/Working/Waiting/
  Blocked/Done/Failed are task states, while Exited/Interrupted require their
  lifecycle boundary.
- `AGENT_SEMANTIC_MAX_AGE_MS` is 15,000. Original observation time must be
  nonnegative, not in the future, within the age bound, no earlier than the
  current process owner's attestation, and newer than the last accepted
  semantic time. Poll/receipt time cannot refresh an unchanged old title. Retain
  its capture epoch separately: any accepted process-owner epoch change, even
  through a process-less PTY event in the same millisecond, invalidates freshness.
- Ordinary event-ID, sequence, epoch and ended-run fences still apply before
  accepted state is projected. Same-provider unbound observations with multiple
  compatible runs are rejected rather than arbitrarily merged.
- `Unknown`, `Fresh` and `Stale` are independent activity freshness values.
  Stale semantics remain last-known data, not evidence of completion or fresh
  work. Four-state compatibility remains one-way and cannot encode Agent
  liveness by itself; consumers must retain the exact rich liveness fact.
- Native title decoding accepts only bounded provider-specific markers. It
  rejects controls and titles over 1,024 bytes. Arbitrary conversation names,
  redraws, output words and Codex task titles do not assert semantic activity.

### 4. Validation & Error Matrix

| Condition | Result |
| --- | --- |
| Live process inventory with Working input | Accept liveness with Unknown activity |
| Unchanged accepted weak inventory | Advance ordering without renewing presentation event/receipt |
| Rejected newer-epoch observation | Preserve route epoch, sibling connectivity and owner timestamps |
| Exact foreground/session owner and fresh ordered semantic event | Accept task state without lowering evidence |
| Wrong run/route/provider/process/session/epoch or not live | `UnresolvedSemanticOwner` |
| Zero PID/start ticks on ordinary observation or inventory input | `InvalidProcess` |
| Semantic event targets an ended run | `EndedRun` |
| Exact run is Hook-owned | `StrongerEvidence` |
| Semantic input tries Unknown/Exited/Interrupted | `InvalidSemanticActivity` |
| Old, future, duplicate-time or pre-owner semantic timestamp | `StaleSemanticObservation` |
| Known run with an old event sequence | `OutOfOrder` |
| Process remains but semantic age expires | Retain state with Stale freshness; no manufactured Waiting/Done |
| Provider and cwd match multiple independent runs | Do not collapse their identity |

### 5. Good / Base / Bad

- Good: an exact foreground provider title updates its run, while another
  independent process on the same terminal remains Unknown.
- Base: a valid owned process with no semantic telemetry is detected with
  Unknown activity and retains its provider identity.
- Bad: mark every process Working because the shared terminal just redrew.

### 6. Tests Required

Cover process/PTY normalization, Hook priority, independent same-provider runs,
all owner dimensions, malformed identities, stale/future/duplicate timestamps,
owner/epoch changes, accepted ordering and presentation freshness. Verify title
markers with provider-specific fixtures and negative arbitrary-title/transcript
cases. Assert original semantic time survives repeated inventory and that a
later genuine launch is not blocked by the retired weak latch. Execute all
tests in Actions; source review and authored tests are not passing evidence.
Snapshot route epochs, siblings and owner times around rejected newer events;
prove a subsequent valid event on the retained epoch is still accepted. Assert
same-millisecond epoch replacement cannot refresh an old semantic capture.

### 7. Wrong vs Correct

Wrong: `process.activity = if recent_output { Working } else { Waiting }`.

Correct: submit liveness as Unknown, then submit a separately captured exact-run
semantic observation only when its owner, provider marker and original timestamp
are proven. Project the accepted state and its freshness, not the raw hint.

## Scenario: Provider-less Hook Exit

### 1. Scope / Trigger

Use this boundary for a Hook-owned semantic exit whose event omitted provider
identity. A newer process observation on the same route is not the Hook owner.

### 2. Signatures

```rust
pub fn observe_hook_exit(
    &mut self,
    route: AgentRoute,
    event_id: AgentEventId,
    sequence: u64,
    connection_epoch: Option<u64>,
    received_at_unix_ms: i64,
) -> AgentApplyOutcome;
```

Unresolved ownership returns
`AgentApplyOutcome::Ignored(AgentObservationIgnored::UnresolvedHookOwner)`.

### 3. Contracts

- Require one current, live-confirmed, non-ended Hook run on the exact route.
- Carry that owner's provider session and process identity into the ordinary
  observation boundary. Prove uniqueness in its first applicable matching
  branch, including ended identity matches; never rely on HashMap order.
- Without process/session identity, same-provider matching must still identify
  exactly one non-ended run. Otherwise reject before any legacy side effects.
- Preserve existing event/sequence/epoch validation and generic matching.
  This boundary introduces no remote Hook transport, secrets, or protocol.

### 4. Validation Matrix

| Condition | Result |
| --- | --- |
| One exact Hook owner plus a newer independent provider | Exit only the Hook owner |
| Same provider on another process, but exact Hook identity is available | Preserve the independent process |
| Multiple Hook owners or ambiguous fallback identity | Ignore with `UnresolvedHookOwner` |
| No current Hook owner | Ignore; do not promote a process run into the exit owner |
| Exact owner but event ordering/epoch is stale | Ordinary registry validation rejects it |

### 5. Good / Base / Bad

- Good: a Hook Codex run exits while a newer Claude process remains working.
- Base: a sole exact Hook owner exits through the usual acceptance machinery.
- Bad: select `active_run_for_route()` by receipt recency to fill a missing
  Hook provider and thereby retire an unrelated process.

### 6. Tests Required

Cover different-provider and same-provider independent processes, ambiguous and
unknown owners, retained exact identity, stale ordering, and no projection on
rejection. Execute tests only in GitHub Actions; shell/no-route compatibility
remains separately covered by the application boundary.

### 7. Wrong vs Correct

Wrong: derive the provider from the newest route event, then emit a generic
Hook `Exited` observation without owner identity.

Correct: call `observe_hook_exit` for the captured exact route and honor its
`AgentApplyOutcome` before projecting accepted state into the application.

## Scenario: Immutable Hook Detection Receipts

### 1. Scope / Trigger

Use this source boundary when multiple Hook sessions share one owned terminal,
or a later unhooked launch occurs before an older Hook event is delivered.
Detection receipts are distinct from provider-session or run identity.

### 2. Signatures

```text
HookDetectionReceipt { episode: Option<AgentWeakEpisode> } // crate-private
SessionTracker::capture_hook_detection(pty_id, agent, can_cover_detected_launch)
    -> Option<HookDetectionReceipt>
SessionTracker::mark_ai_session_if_receipt(pty_id, agent, receipt) -> bool
SessionTracker::clear_ai_session_if_episode(pty_id, expected_episode) -> bool
hook_server::handle_hook_payload(&HookState, &StatusEmitter, &SessionTracker, HookPayload)
```

### 3. Contracts

- An active Hook session captures its optional receipt once. An absent receipt
  means unknown ownership; it is not a captured source with no weak episode.
  Repeated statuses, starts and provider corrections reuse the original receipt.
- Compatible detected input can be covered only by an explicit first start
  without a competing Hook lifecycle. Provider equality is a mismatch guard,
  not identity proof. An unseen mid-session Hook, conflicting provider or newer
  pending input cannot claim an existing detected launch.
- Capture, conditional marking and clearing use the outer tracker mutation lock
  shared with input/echo publication, purge and snapshots. A newer published OR
  pending episode rejects old marking/clearing without changing its state.
- Every known SessionEnd emits its exact captured session identity and receipt,
  including nonlast and clear ends. Nonlast ends may conditionally clear only
  their own receipt when no remaining lifecycle shares it. Only last non-clear
  ends remove pane Hook state. Clear preserves pane state; unknown, missing,
  duplicate or evicted session ends have no pane teardown authority.
- Source events carry captured facts through the channel unchanged. Do not
  reread tracker or current Hook identity on delivery. Internal Hook event
  ownership is serde-skipped; existing external status/payload formats remain.
- A receipt may be unknown or unchanged for a real resumed Hook-only session.
  Therefore it cannot substitute for explicit lifecycle-start authority or
  justify bypassing the ordinary runtime ended-run guard.

### 4. Validation & Error Matrix

| Condition | Required behavior |
| --- | --- |
| Repeated old Hook after recognized or pending B | Reuse A receipt; preserve B tracking |
| Unseen mid-session event beside detected input | Unknown receipt, no borrowed launch |
| Nonlast known end | Emit exact rich exit; preserve other Hook sessions |
| Last non-clear known end | Remove pane Hook state, clear only matched receipt |
| Clear end | Emit exact rich exit without pane teardown |
| Missing, duplicate, unknown or evicted end | No invented rich exit or pane teardown |
| Provider correction | Retain original receipt, never recapture current input |
| Genuine same-ID resume | Needs explicit lifecycle boundary, not receipt recency |

### 5. Good / Base / Bad Cases

- Good: an older Codex Hook ends while later Claude input waits for echo;
  Codex exits and the pending Claude launch remains eligible for detection.
- Base: a sole matching Hook exits and clears only its own detector state.
- Bad: plain-clear the tracker, then reread its current episode for the exit.

### 6. Tests Required

Exercise the actual payload handler with public tracker input, not fabricated
end observations. Cover both end orders with two Hooks, recognized and pending
later input, repeated old statuses, clear/sole/unknown/late ends, provider
correction, receipt sharing and bounded storage. Cross-crate regressions use
the real ChannelSink and production app ingestion, assert exact run retirement
and preserved later detection, and retain external serialization assertions.
Lock-participation tests cover input, echo, clear, purge, snapshots, capture and
conditional mark. All execution remains GitHub Actions-only.

### 7. Wrong vs Correct

Wrong: clear the current detector for any SessionEnd and let the UI choose a
Hook by provider or receipt recency.

Correct: retire the supplied known source session with its immutable receipt;
conditionally clear only that owned boundary and forward the exact event to
runtime ingestion before projecting accepted effects.

## Scenario: Explicit Hook Lifecycle Resume

### 1. Scope / Trigger

Use this boundary when a provider resumes the same session ID after an accepted
end. The provider session is reusable; an ended AgentRunId is not reopened.

### 2. Signatures

```text
AgentHookLifecycleId: opaque internal Copy/Eq/Ord/Hash identity
HookSessionId::lifecycle_id: Option<AgentHookLifecycleId>
SessionIdentity::hook_lifecycle: Option<HookLifecycleEvent> // serde(skip)
HookLifecycleEvent::{Started(AgentHookLifecycleId), Observed(AgentHookLifecycleId)}

AgentRuntimeRegistry::start_hook_lifecycle(AgentObservation, AgentHookLifecycleId)
    -> AgentApplyOutcome
AgentRuntimeRegistry::observe_hook_lifecycle(AgentObservation, AgentHookLifecycleId)
    -> AgentApplyOutcome
```

### 3. Contracts

- Only mt-ai source recognition mints the non-wrapping lifecycle ID. Keep one
  per bounded active Hook session; neither delivery time nor weak-input receipt
  is lifecycle identity. No provider session, wire or persisted layout changes.
- Retain `explicit_start_seen` separately. First explicit SessionStart emits
  Started before status, even if provider/session identity did not change or a
  mid-session event first recognized this lifecycle. Promotion keeps the same
  token and immutable receipt. Mid-session recognition alone cannot resume.
- Repeated active starts do not reset Working to Waiting, renew Hook age or
  remint identity/receipt. Provider corrections can still emit Observed.
- Registry bindings are private `(exact AgentRoute, lifecycle_id) -> RunId`.
  Require a bounded valid session, Hook/LiveConfirmed/Live evidence and no
  supplied process. Initial binding may retain a uniquely proved live session
  owner's process. A new explicit lifecycle after all exact owners ended gets
  a NEW RunId and preserves the old ended audit records.
- Competing bound live lifecycles and ambiguous exact owners reject; never
  retire another run to make room. Follow-on events address the bound original
  run, including provider corrections. An ended bound run rejects even a late
  exit with a newer delivery epoch. Ordinary/legacy matching stays unchanged.
- Ownership, replay, sequence, epoch and ended validation are pure preflight.
  Only accepted observations update runs, epochs, bindings, siblings, freshness,
  receipts and weak supersession. Rejected events consume no ID or binding.
  Exact route removal purges only that route's lifecycle map entries.
- ChannelSink forwards source Started/Observed/status ownership unchanged.
  Internal lifecycle fields are not serialized to compatibility consumers.
- Wire limit: a same-session end first received AFTER an explicit resume has
  no incoming generation and is indistinguishable from a new lifecycle end.
  Internal IDs fence already captured/queued events, not that missing wire fact.
  Do not invent time/provider guesses or silently extend the incoming protocol.

### 4. Validation & Error Matrix

| Condition | Required behavior |
| --- | --- |
| Explicit resume after accepted end | New RunId, same real session, old audit unchanged |
| First explicit start after mid-session recognition | Promote same token/receipt once |
| Repeated active start | No new run or start-state/age reset |
| Already bound old lifecycle event after resume | Target ended old run and reject |
| New unbound event without explicit start and ended owner | Retain ended/ambiguity rejection |
| Invalid provider/session, competing owner or stale order/epoch | Reject before every registry side effect |
| Exact route removed | Purge only its bindings and runs |
| Newly received same-sid old end lacks wire generation | Document limitation, no synthetic proof |

### 5. Good / Base / Bad Cases

- Good: resume an ended sid into a new run, then reject its old queued end
  without advancing the new run's epoch or changing another provider's input.
- Base: follow-on status updates the one bound live lifecycle.
- Bad: globally ignore ended session matches and pick the newest live row.

### 6. Tests Required

Exercise real producer -> ChannelSink -> app ingestion -> registry with sole
and nonlast resumes, Hook-only/recognized/pending input, unchanged last_session,
first explicit start after mid-session recognition, repeated starts, old queued
identity/status/end and provider corrections. Assert Done stays live, old audit
rows stay ended, and rejected higher-epoch events preserve every registry map
and remain unconsumed. Cover unique initial binding and exact-route purge.
Assert internal fields are absent from serialized status/identity events.
All compilation, fixtures, tests and native automation run only in Actions.

### 7. Wrong vs Correct

Wrong: reopen an ended run or rewrite the provider session to evade ambiguity.

Correct: source-mint one internal lifecycle, emit its first Started identity
before status and use dedicated validated lifecycle entry points. Preserve old
records and ordinary ended-run rejection.
