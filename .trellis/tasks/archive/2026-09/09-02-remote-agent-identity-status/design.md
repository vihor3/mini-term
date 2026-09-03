# Technical Design

## Boundary

```text
local Hook / PTY fallback                    authenticated SSH runtime
          |                                             |
          v                                             v
AiBridge captures exact terminal route       bounded exact-route /proc probe
          |                                             |
          +--------------- AgentObservation -----------+
                                |
                                v
                    mt-ai AgentRuntimeRegistry
                                |
             rich run state + legacy pane projection
                                |
                                v
                            AppStore/UI
```

`mt-identity` owns opaque IDs, `mt-ai` owns provider/activity semantics and
reconciliation, `mt-ssh` owns authenticated transport and bounded remote facts,
and `mt-app` owns scheduling, route lookup, compatibility projection, and UI
notifications.

## Identity And Route

`AgentRunId` and `AgentEventId` are UUID-v4 identities from `mt-identity`.
Provider session IDs and remote `(pid, start_ticks)` values remain evidence and
lookup keys, never public run identity.

Every observation carries an `AgentRoute`:

```rust
AgentRoute {
    execution_host_id,
    worktree_id,
    tab_id,
    pane_key,
    terminal_session_id,
    terminal_incarnation_id,
}
```

The full route is the ownership boundary. `pty_id` remains a process-local
attachment lookup only. `AiBridge` stores the current route for each live PTY
and snapshots it into the event before crossing the channel.

## Runtime Model

`mt-ai::agent_runtime` defines:

- `AgentProvider`: normalized Claude, Codex, OpenCode, Pi, Grok, or validated
  extensible identifier.
- `AgentActivity`: starting, working, blocked, waiting, done, failed,
  interrupted, exited, unknown.
- `AgentConnectivity`: live, stale, disconnected.
- `AgentConfirmation`: live-confirmed or restored-unconfirmed.
- `AgentEvidence`: Hook, process-attested, PTY activity, or restored history,
  with an explicit strength ordering.
- `AgentProcessIdentity`: remote PID plus Linux start ticks.
- `AgentObservation`: route, provider/session/process, activity axes, evidence,
  optional connection epoch, and monotonic sequence.
- `AgentRuntimeState`: stable run ID plus the accepted current observation.
- `AgentRuntimeRegistry`: route/process/session indexes and fail-closed merge.

Matching order is exact process identity, exact provider session identity, then
one live same-route/provider run. Stronger evidence may correct provider or
activity. Weaker evidence can refresh connectivity but cannot overwrite a
stronger semantic activity at the same sequence horizon. Successful process
inventory ending a process-attested run marks it exited; transport failure only
changes connectivity.

The legacy projection remains one-way:

```text
working / starting / blocked -> ai-working
waiting / done               -> ai-idle
failed                        -> error
exited / interrupted / unknown without a live provider -> idle
```

Existing Hook code remains the owner of notifications and attention cause
semantics; the rich registry observes that result and does not independently
replay completion alerts.

## Local Event Capture

`AiBridge` owns a shared `pty_id -> AgentRoute` table. `add_pane` installs the
route immediately after terminal creation; `remove_pane` removes it before
purging perception state. `ChannelSink` attaches the current route to both
status and provider-session events.

`AppStore::apply_ai_event` first compares the captured route with the current
`terminal_routes` entry. A mismatch drops the event. The existing pane update,
attention, completion, and persisted provider-session behavior then runs
unchanged, followed by a rich registry observation.

## SSH Launch Attestation

SSH compatibility terminals need the incarnation before argv construction.
`RemoteLaunchExtras` therefore carries an optional preallocated legacy
incarnation. `start_legacy` uses it verbatim instead of generating a second ID.

`mt-pty::ssh` receives a fixed `RemoteTerminalRouteEnv` and builds:

```text
cd '<worktree>' 2>/dev/null;
exec env MINITERM_AGENT_PROTOCOL_VERSION='1' ... "$SHELL" -l
```

Only canonical public route values are accepted. Values use existing POSIX
single-quote escaping. `MINITERM_PTY_ID`, Hook port/token, credentials, and
user-supplied `MINITERM_*` values are not forwarded.

When `MINI_TERM_REMOTE_AGENT_STATUS=0`, the existing login command is emitted
without route variables and no polling is scheduled.

## Remote Inventory

`mt-ssh::inspect_remote_agents` opens one bounded exec channel on the current
pooled session. A fixed POSIX shell script:

1. reports `unsupported` when Linux `/proc` or required basic tools are absent;
2. scans readable `/proc/<pid>/environ` entries;
3. keeps only processes containing every exact target route variable and the
   protocol version;
4. classifies a provider from executable/initial argv internally;
5. extracts `/proc/<pid>/stat` start ticks;
6. emits at most 64 normalized `provider<TAB>pid<TAB>start_ticks` rows.

The protocol has a fixed header/footer, unique keys, strict UTF-8, a 16 KiB
transport cap, numeric bounds, duplicate rejection, and a connection epoch
added from `CachedSession`. Raw inspected values never leave the remote script.

## Polling And Fencing

`AppStore` maintains per-terminal poll state with a monotonic generation and
last successful sequence. A periodic GPUI task asks the store to schedule
eligible routes. Eligibility requires:

- feature gate enabled;
- SSH project and live terminal route;
- remote runtime phase `Ready`;
- snapshot host/worktree equal to the route;
- matching current connection ID/configuration.

A completion applies only if the route, project/connection owner facts,
generation, and current connection epoch still match. Supported inventories
reconcile all returned processes and end missing process-attested runs.
Unsupported results preserve fallback status. Errors mark eligible runs stale
or disconnected without changing their last activity.

The existing PTY tracker supplies recent-output evidence for process rows:
recent output means `working`; a live quiet process means `waiting`. Hook
observations always outrank this heuristic.

## Compatibility And Rollback

- Existing wire status strings and `PaneStatus` remain unchanged.
- Existing local Hook HTTP endpoint/payload stays unchanged.
- Existing remote runtime fallback and SSH terminal launch remain usable when
  probing fails.
- `MINI_TERM_REMOTE_AGENT_STATUS=0` disables only the new SSH attestation/probe
  path and keeps local AI perception intact.
- No schema migration is required for the runtime registry; later UI children
  consume its read-only snapshots.
