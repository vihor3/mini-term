# Remote Agent Reconciliation Contract

## Scenario: Track remote Agent processes across SSH runtime epochs

### 1. Scope / Trigger

Use this contract when `mt-app` schedules an authenticated SSH Agent inventory,
accepts its result, recreates poll state, handles a remote-runtime gap, or
projects activity/connectivity into panes and Agent feeds. `mt-app` owns route
and scheduling facts; `mt-ssh` owns authenticated process discovery.

### 2. Signatures

```text
RemoteAgentPollRequest = {
  pty_id, project_id, project_path, generation,
  connection_id, connection_fingerprint,
  route: AgentRoute, connection_epoch
}

RemoteAgentPollState = {
  capability, connectivity, process_count, connection_epoch,
  generation, in_flight, route, had_processes, empty_successes
}
```

```rust
pub fn remote_agent_status_enabled() -> bool;
```

Rollback environment:

```text
MINI_TERM_REMOTE_AGENT_STATUS=0
```

### 3. Contracts

- A poll is eligible only when the feature is enabled, the terminal has a
  current `AgentRoute`, the route belongs to an SSH project, runtime phase is
  `Ready`, execution-host/worktree IDs match, and the latest authenticated
  connection epoch equals the runtime snapshot.
- One in-flight request owns each terminal route. Completion revalidates every
  request field plus current project, connection fingerprint, exact route,
  runtime ownership, cached-session winner, and epoch. Teardown or reuse makes
  old work inert.
- Recreating a poll state derives `had_processes` from a non-ended,
  process-attested run on the exact route. Empty-inventory hysteresis therefore
  survives poll-map eviction instead of treating the next empty result as a
  process-free baseline.
- A remote-runtime gap cannot leave an Agent `Live`. `Connecting` projects
  `Disconnected`; `CompatibilityFallback` and `RebindDeferred` project `Stale`.
  Missing runtime/connection/epoch facts may request runtime refresh, but do not
  manufacture process or completion evidence.
- A connectivity observation is accepted only when it changes connectivity or
  the known connection epoch for at least one non-ended run on the exact route.
  Repeating the same connectivity at the same epoch creates no new
  `AgentEventId`, sequence, timestamp, or unread renewal. A route with no active
  run also emits no connectivity event, and a hysteresis-suppressed inventory
  does not reserve an event sequence before it becomes eligible to apply.
- Recent PTY output plus a live matched process projects `ai-working`; a quiet
  live process projects `ai-idle`. Two successful empty Linux inventories are
  required to retire prior process evidence. The first empty result is a race
  window.
- Hook-enabled panes retain Hook authority. Without Hook, weak PTY idle/error
  cannot clear a current process-attested run while the feature is enabled.
  Unsupported/probe failures preserve activity and update only bounded
  capability, diagnostics, and connectivity.
- SSH launch injects only the exact public route and preallocated incarnation.
  PTY IDs, credentials, Hook secrets, and arbitrary user `MINITERM_*` values are
  never exported remotely.
- Exact value `0` disables route injection and remote polling. Local Hook/PTY
  perception and the existing four-state UI continue unchanged.

### 4. Validation & Error Matrix

| Condition | Required behavior |
|-----------|-------------------|
| Runtime is `Ready` and route/epoch facts match | Start or continue one exact-route poll |
| Runtime is `Connecting` | Stop the poll and mark exact active runs `Disconnected` |
| Runtime is fallback or rebind-deferred | Stop the poll and mark exact active runs `Stale` |
| Poll state was recreated after a process-attested run | Seed `had_processes=true`; require two empty successes before retirement |
| Same connectivity and same known epoch repeat | Emit no Agent event and do not renew unread state |
| Connectivity changes or a supplied epoch differs | Emit one fenced connectivity event for the exact route |
| Project, path, connection, fingerprint, route, or epoch changes | Reject the old completion |
| Capability is unsupported | Preserve activity; show unsupported capability separately |
| Probe fails | Preserve activity; publish bounded stale/disconnected diagnostics only |
| Feature value is exactly `0` | Disable remote route injection and polling |

### 5. Good / Base / Bad Cases

- Good: A disconnected poll map is recreated while the runtime registry still
  holds a process-attested run; the first empty inventory does not retire it.
- Good: A repeated runtime-gap scan at one epoch leaves the current event and
  acknowledgement watermark unchanged.
- Base: A new exact route with no prior process evidence receives an empty
  inventory and remains without an Agent run.
- Bad: Initialize every recreated poll with `had_processes=false`; that bypasses
  the two-confirmation retirement contract.
- Bad: Emit a fresh connectivity event on every timer tick; Waiting/Done rows
  would repeatedly become unread without a state change.

### 6. Tests Required

- Feature gate disables only exact zero.
- Request-fact tests independently change generation, project path, connection,
  fingerprint, route, and epoch and assert stale rejection.
- Recreated-poll tests seed process-attested registry evidence and require two
  empty confirmations; an unrelated incarnation must not seed it.
- Connectivity tests cover every non-ready runtime phase and assert no `Live`
  projection.
- Duplicate-observation tests assert no active route and same connectivity/epoch
  emit no new event, while changed connectivity or epoch is accepted.
- SSH launcher tests preserve exact route values and incarnation equality.
- Linux focused tests plus Windows MSVC checks compile scheduling and projection.

### 7. Wrong vs Correct

#### Wrong

```rust
let poll = RemoteAgentPollState::from_request(&request, false);
registry.mark_connectivity(new_event(Disconnected));
```

This forgets durable process evidence and renews activity merely because a poll
object or timer iteration was recreated.

#### Correct

```rust
let had_processes = has_process_attested_run_for_route(&registry, &request.route);
let poll = RemoteAgentPollState::from_request(&request, had_processes);
if active_route_connectivity_change_needed(&registry, &request.route, epoch, next) {
    registry.mark_connectivity(fenced_event(request.route, epoch, next));
}
```

Poll allocation consumes exact-route runtime evidence, and connectivity changes
only when at least one active exact-route run authoritatively differs.
