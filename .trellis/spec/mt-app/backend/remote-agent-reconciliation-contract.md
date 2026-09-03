# Remote Agent Reconciliation Contract

## Scope

Use this contract for scheduling authenticated SSH agent probes, accepting
their results, and projecting rich state into the existing pane UI. `mt-app`
owns project/terminal ownership facts and GPUI scheduling; it does not parse
`/proc` or redefine agent semantics.

## Eligibility

A poll is eligible only when all of these are true:

- `MINI_TERM_REMOTE_AGENT_STATUS` is not exactly `0`;
- the terminal has a current `AgentRoute` and belongs to an SSH project;
- the project remote runtime is `Ready`;
- runtime execution-host/worktree IDs equal the terminal route;
- the latest authenticated connection epoch equals the runtime snapshot.

SSH launch preallocates the terminal incarnation before argv construction and
injects the exact public route through `mt-pty`. The launched terminal, route
table, Hook capture, and remote process probe must all use that same
incarnation. PTY IDs, credentials, Hook secrets, and arbitrary user
`MINITERM_*` values are never exported remotely.

## Scheduling And Fences

- Keep one in-flight request per terminal route. Every request captures a
  nonzero process-monotonic generation, project ID/path, connection ID and
  configuration fingerprint, exact route, and connection epoch.
- Apply completion only while every captured fact still matches and the
  project runtime remains authoritative. Project removal, terminal reuse,
  connection edits, and reconnects invalidate old work.
- The SSH facade also verifies that the exact `Arc<CachedSession>` is still the
  pool winner and that its epoch remains current.
- A changed/missing current epoch marks runs disconnected and refreshes remote
  runtime identity before another probe.
- Terminal, project, and connection teardown clear their poll ownership state.
  Explicit terminal close also removes that route's live runtime entry.

## Projection

- Recent PTY output plus a live matched process projects `ai-working`; a quiet
  live process projects `ai-idle`.
- Two consecutive successful empty Linux inventories are required before
  clearing a previously observed process. The first empty result is treated as
  a race window.
- Hook-enabled panes retain existing Hook status, attention, notification, and
  provider-session behavior.
- Without Hook, weak PTY `idle/error` cannot clear a current
  process-attested run while the feature is enabled. It may still refresh
  `working/waiting` through the rich registry.
- Unsupported capability and probe/protocol failures preserve the last pane
  activity. Failures update only bounded diagnostics and stale/disconnected
  connectivity.
- The probe never infers blocked, done, or permission-required semantics.

## Rollback

```text
MINI_TERM_REMOTE_AGENT_STATUS=0
```

Exact zero disables route environment injection and remote polling while local
Hook/PTY perception and the existing four-state UI continue unchanged.

## Required Tests

- Feature gate disables only exact zero.
- Captured Hook/PTY events reject reused PTY routes and old incarnations.
- Request facts independently reject changed generation, path, connection,
  fingerprint, route, and epoch.
- Empty inventory requires two confirmations after a live process.
- SSH launcher escaping preserves exact route values and incarnation equality.
- Linux and Windows builds compile the scheduling, facade, and projection path.
