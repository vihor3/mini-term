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

## Required Tests

- Provider aliases normalize; invalid provider keys fail closed.
- Route, event, epoch, and sequence fences reject stale observations.
- Process evidence upgrades one heuristic run and preserves the run ID.
- PTY working/waiting refreshes process activity while PTY exit cannot end it.
- A successful empty inventory ends missing processes; connectivity-only
  changes preserve activity.
