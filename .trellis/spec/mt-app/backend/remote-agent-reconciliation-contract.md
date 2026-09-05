# Remote Agent Reconciliation Contract

## Scenario: Track remote Agent processes across SSH runtime epochs

### 1. Scope / Trigger

Use this contract when `mt-app` schedules an authenticated SSH Agent inventory,
accepts its result, recreates poll state, handles a remote-runtime gap or
natural PTY exit, or projects activity/connectivity into panes and Agent feeds.
`mt-app` owns route
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
  live process projects `ai-idle`, subject to the registry's accepted state and
  stronger evidence. Presence plus recency is not provider-semantic completion
  telemetry. Two successful empty Linux inventories are required to retire
  prior process evidence. The first empty result is a race window.
- For routed Agent observations, call registry acceptance before changing
  legacy pane/project status, attention, Git-watcher flags, or completion
  notifications. `AgentApplyOutcome::Ignored` permits none of those effects;
  `Applied` projects the accepted registry state, not the weak incoming value.
  An ordinary non-Agent shell observation is a separate compatibility path,
  not an ignored Agent event. Inventory success likewise projects current
  accepted state because individual observations may have been rejected.
- A Hook exit with no provider cannot borrow the newest route provider from a
  process-attested run. Resolve a unique current Hook owner, retain its exact
  session/process identity, and reject ambiguous ownership before side effects.
  Never let generic same-provider fallback redirect an exact Hook exit to a
  different run. Ordinary non-Agent shell fallback is unaffected.
- Accepted confirmed absence may clear the exact route's contradicted weak
  tracker latch only after the last process-attested run retires and no
  stronger live Hook/run remains. Keep the terminal registration and two-empty
  hysteresis. An empty inventory for a never-attested heuristic session, a
  probe failure, or a reconnect cannot trigger blanket tracker expiry.
- Natural PTY exit retires that incarnation's local observation sources and
  poll eligibility, making queued events and inventory completions inert.
  Cleanup is idempotent with explicit close/detach. Retain remote last-known
  semantic activity with disconnected/stale connectivity: a transport exit
  does not prove that its remote process exited. Preserve warm-attach identity.
- Hook-enabled panes retain Hook authority. Without Hook, weak PTY idle/error
  cannot clear a current process-attested run while the feature is enabled.
  Unsupported/probe failures preserve activity and update only bounded
  capability, diagnostics, and connectivity.
- SSH launch injects only the exact public route and preallocated incarnation.
  PTY IDs, credentials, Hook secrets, and arbitrary user `MINITERM_*` values are
  never exported remotely.
- Exact value `0` disables route injection and remote polling. Local Hook/PTY
  perception and the existing four-state UI continue unchanged.
- Sidebar indicators separate semantic activity, connectivity, and attention.
  Only live Starting/Working may animate, and attention/error takes priority.
  Waiting/Blocked/Done/error are steady. Stale/disconnected Working retains its
  activity but has no live-work spinner. Catalog refresh/warnings cannot create
  Agent activity; no rich evidence uses ordinary legacy fallback. Keep stable
  indicator geometry and the existing one-way four-state compatibility map.

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
| PTY Working sequence 10 arrives after accepted Waiting sequence 11 | Reject without legacy/attention/completion changes |
| Weaker observation is accepted beneath Hook state | Project the retained Hook semantics, not the incoming status |
| Independent accepted runs share a route | Aggregate all runs; evidence precedence is not route-wide suppression |
| Provider-less Hook exit follows another provider's process observation | Retire only the exact Hook owner or reject unresolved ownership |
| Last attested run retires after confirmed absence | Clear only contradicted weak tracking; later shell output cannot resurrect it |
| Natural PTY exit precedes a queued observer/poll completion | Ignore old work; remote activity is retained as disconnected/stale |
| Working run loses connectivity | Preserve semantic activity without live spinner |
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
- Bad: Set `pane.status` before observing the rich event; registry rejection
  cannot undo already-emitted attention, notifications, or project aggregation.

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
- Accepted-projection tests cover stale sequence/epoch, Hook priority, ordinary
  shell fallback, multiple exact-route processes, provider-less Hook exits,
  ambiguous owner rejection, and mixed panes with legacy fallback.
- Retirement tests cover two-empty confirmation, stronger-owner preservation,
  shell output after retirement, a later genuine launch, natural PTY exit,
  delayed event/poll rejection, and idempotent teardown.
- Presentation tests cover live work, steady waiting/approval/completion/error,
  offline/stale work, no evidence, and catalog-progress independence.
- All checks and disposable fixture execution run only in GitHub Actions.
  Record native startup cadence separately using a matching Actions artifact;
  static source review and compiler success are not runtime reproduction.

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
