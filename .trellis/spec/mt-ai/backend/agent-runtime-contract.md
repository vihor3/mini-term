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
  out-of-order observations for the matched run.
- Retirement does not erase the route's ordering evidence. A queued weak PTY
  event older than the retained route watermark cannot create a replacement
  heuristic run merely because no active run matches. A genuinely later launch
  can create a new run on the still-live terminal route.
- Match by exact process identity, then exact provider session identity, then a
  single compatible live run on the same route. A different incarnation is a
  different route.
- Evidence order is `Hook > ProcessAttested > PtyActivity > RestoredHistory`.
  Weaker evidence cannot replace provider, process, confirmation, or semantic
  terminal state.
- PTY activity is the one bounded exception: `working` or `waiting` may refresh
  the activity of a live process-attested run without lowering its evidence.
  PTY `idle` or `error` cannot end that run.
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
- PTY working/waiting refreshes process activity while PTY exit cannot end it.
- A successful empty inventory ends missing processes; connectivity-only
  changes preserve activity.
- A queued older PTY event after process retirement creates no replacement run;
  a later genuine launch gets a new run identity. Execute regressions only in
  GitHub Actions, including any disposable process fixtures.

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
