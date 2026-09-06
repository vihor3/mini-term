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
  route: AgentRoute, connection_epoch, requested_at_unix_ms, weak_episode
}

RemoteAgentPollState = {
  capability, connectivity, process_count, connection_epoch,
  generation, in_flight, route, had_processes, empty_successes
}
```

```rust
pub fn remote_agent_status_enabled() -> bool;
```

The live Codex producer uses `TerminalEmulator::advance_with_current_screen`
for before/after current-grid facts under the existing emulator lock order.
`PtyCodexScreen::bind_owner`, `observe_output` and `take_confirmed` retain the
reader's run/route/process/source and original evidence time. Foreground poll
confirmation then submits `AgentSemanticObservation`; render/title code never
parses transcript text into activity.

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
- Poll only registered Mini-Term terminal routes, including background projects
  and worktrees. Provider/cwd matches on arbitrary device processes are not
  ownership. Status/session events forward their source-owned weak episode;
  inventory captures it when scheduled. Never reread newer input state at reply.
- Exclude runtime-superseded weak aliases from every liveness, legacy provider,
  feed/sidebar/Runtime, title-cardinality, activation and close-warning path.
  Retained audit rows are not independently live. Exact proved runs remain
  distinct; row suppression alone cannot repair lifecycle duplication.
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
- Process-only inventory supplies Unknown activity. Generic PTY output recency
  cannot infer Working or Waiting and cannot replace accepted task semantics.
  Exact-run semantic observations use the mt-ai owned semantic boundary with
  original event time and foreground/session ownership; Hook remains strongest.
  Two successful empty Linux inventories are required to retire prior process
  evidence. The first empty result is a race window; unsupported managed-root
  capability, including legacy SSH terminals, is not an empty success.
- Title semantics require an actual terminal title event with exact route,
  foreground PID/start ticks/provider and original capture time. Retain a pending
  title until a confirming inventory was SCHEDULED strictly after capture; a
  late pre-capture reply is not confirmation. Owner change or age expiry rejects
  it. Never restamp retained titles on polls or treat restored/replayed titles
  as new semantic observations.
- Codex current-screen semantics use actual parsed PTY output, a bounded native
  status/composer capture and the same foreground/source/route/epoch fences.
  Keep screen evidence separate from display titles. Confirm with an inventory
  scheduled strictly after capture, not a pre-capture request returning late.
  Repeated same-state counter updates must not keep replacing the earliest
  pending capture and starve confirmation; absent or contradictory newer screen
  evidence must prevent publication of an obsolete pending Working sample.
  Failed ownership/epoch confirmation never retimestamps retained screen data.
- Runtime display titles are separate from activity. Exact owned history title
  metadata carries run/session/route/source/epoch and cannot bind to a replacement
  connection. An ambiguous history match does not choose the first run. All
  activity labels use the shared freshness-qualified projection, not raw Working
  text after semantic freshness expires.
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
- Source-produced Hook statuses and ends retain their internal exact session
  identity through ChannelSink into `record_runtime_status_change`. A known
  nonlast end must retire that specific rich run too; it cannot disappear into
  pane-only state. Exact ends require an existing unique live-confirmed Hook
  owner before registry/epoch side effects. Only legacy ownerless events use
  `observe_hook_exit`. Forward the source receipt unchanged; never fill it from
  current tracker state on delivery. See the mt-ai immutable receipt contract.
- `SessionIdentity::hook_lifecycle::Started` dispatches to
  `start_hook_lifecycle`; Observed and lifecycle-bound statuses/ends dispatch
  to `observe_hook_lifecycle`. Source emits first Started before status; the
  app never infers it from delivery-time Hook state or a reusable session ID.
  Legacy None fields keep legacy matching. Invalid internal provider/session
  or Ignored outcomes cannot reach persistence, pending-fork, attention or
  legacy projection. Explicit resume creates a new RunId and keeps old audit
  rows; queued old lifecycle events cannot target the resumed run.
- Accepted confirmed absence may clear the exact route's contradicted weak
  tracker latch only after the last process-attested run retires and no
  stronger live Hook/run remains. Keep the terminal registration and two-empty
  hysteresis. An empty inventory for a never-attested heuristic session, a
  probe failure, or a reconnect cannot trigger blanket tracker expiry.
  Pass that inventory's scheduling-time episode to
  `SessionTracker::clear_ai_session_if_episode`; do not reread it at receipt or
  call unconditional clear. The tracker atomically refuses newer published or
  pending input. A retired old process and surviving later detection are valid
  independent facts; rejecting the clear must not resurrect the old process.
- Natural PTY exit retires that incarnation's local observation sources and
  poll eligibility, making queued events and inventory completions inert.
  Cleanup is idempotent with explicit close/detach. Retain remote last-known
  semantic activity with disconnected/stale connectivity: a transport exit
  does not prove that its remote process exited. Preserve warm-attach identity.
- Hook-enabled panes retain Hook authority. Without Hook, weak PTY idle/error
  cannot clear a current process-attested run while the feature is enabled.
  Unsupported/probe failures preserve activity and update only bounded
  capability, diagnostics, and connectivity.
- SSH launch injects the exact public route and preallocated incarnation, and
  captures the remote managed login root's public PID/start ticks/TTY before exec.
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
| Weak audit alias was superseded by accepted source evidence | Exclude it from every actionable projection, even after stronger exit |
| Pre-capture inventory returns after a title | Keep pending title; require a strictly later scheduled sample |
| Title owner/session/source or epoch changed | Reject attachment; retained metadata is not replacement evidence |
| Provider-less Hook exit follows another provider's process observation | Retire only the exact Hook owner or reject unresolved ownership |
| Last attested run retires after confirmed absence | Clear only contradicted weak tracking; later shell output cannot resurrect it |
| New recognized or pending input follows the empty request | Preserve its episode/detection while retiring the old process |
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
- Exercise old empty completion after newer recognized input and before newer
  pending-input echo publication. Both call the production conditional-clear
  consumer; assert new detection survives and the old attested run stays retired.
- Cross-crate tracker tests use the public
  `track_input_with_line_snapshot(pty_id, data, None)` input API.
  `SessionTracker::track_input` is an mt-ai-local `#[cfg(test)]` helper and
  is unavailable when mt-ai is compiled as mt-app's dependency. Do not expose
  a production compatibility helper just to make consumer tests compile.
- Public-input lifecycle tests cover weak detection -> first Hook identity ->
  Working -> provider-less SessionEnd -> deduped idle -> later launch, with
  no phantom feed/provider/title/close ownership and no old-episode revival.
- Title tests vary capture/scheduling/completion order, PID/start ticks, provider,
  route, epoch and freshness, and reject pre-capture in-flight confirmation.
- Screen tests use actual VT frames before the production owner/registry path;
  cover fast counter updates, absent/changed markers before delayed confirmation,
  scrollback, replay, chunked redraw, multiple runs and stronger Hook semantics.
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
