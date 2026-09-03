# Differential Findings Ledger

Review range: `0bc6f28..c644ae9`. Line evidence refers to the target tree unless
otherwise stated. Every item below intersects an introduced Orca task hunk or a
task-owned audit remediation added to validate that range.

Current tally: 29 confirmed findings (13 P1, 16 P2). Twenty-eight fixes are
implemented; `TERM-08` remains a bounded residual risk. Executable verification is
owned exclusively by GitHub Actions. Final CI run `33799663655` and Windows package
run `33799663753` passed for head `c3194a683264dbc5e448c6945c71097f2b4f2e22`.
Run `33795981213` remains intermediate evidence for the preceding task state only.

## Identity, Layout, And Catalog

### ID-01 / P1 / Persisted remote authority is downgraded on restart

- Evidence: `0386e6b`, extended by `40714bf`; `store/identity.rs:69-109` always
  resolves configured SSH paths provisionally before considering an existing binding.
- Invariant/impact: an authenticated remote host binding must survive a restart as the
  strongest known identity; replacing it with a connection-ID-derived provisional ID can
  route the project to a different workbench and layout.
- Proof: focused resolver regression using a persisted authoritative SSH binding.
- Disposition: fix implemented with focused regression coverage; verified by final GitHub Actions CI run `33799663655`.

### ID-02 / P1 / Cold authoritative rebind can retain the wrong runtime layout

- Evidence: `40714bf`; `store/identity.rs:403-442` restores the destination layout only
  when the current state's panels are empty.
- Invariant/impact: a project with no live terminal or open document must atomically adopt
  the authoritative worktree layout, even if provisional startup hydration populated panels.
- Proof: cold provisional-to-authoritative rebind regression with non-empty hydrated state.
- Disposition: fix implemented with focused regression coverage; verified by final GitHub Actions CI run `33799663655`.

### ID-03 / P1 / Shared-worktree reconciliation depends on input order

- Evidence: `0386e6b`; `mt-layout/src/lib.rs:430-475` reconciles aliases sequentially and
  selects/migrates the destination row while earlier aliases can change the candidate seen
  by later aliases.
- Invariant/impact: aliases of one `WorktreeId` must converge on one deterministic layout,
  independent of configuration order.
- Proof: reverse-order reconciliation fixture with conflicting legacy candidates.
- Disposition: fix implemented with focused regression coverage; verified by final GitHub Actions CI run `33799663655`.

### ID-04 / P1 / Shared-worktree aliases can overwrite or delete live shared state

- Evidence: `0386e6b`; `store/layout.rs:395-408` schedules writes per project while
  `store/projects.rs:344-348` flushes a removed alias before deleting it.
- Invariant/impact: aliases sharing one `WorktreeId` must share one runtime owner; a stale
  alias save/removal must not overwrite the active alias's layout row.
- Proof: alias save/remove regression against a common worktree row.
- Disposition: fix implemented with focused regression coverage; verified by final GitHub Actions CI run `33799663655`.

### ID-05 / P1 / Persisted SSH authority lacks endpoint and path provenance

- Evidence: `0386e6b`/`40714bf`; an authoritative persisted binding recorded the resolved
  host/worktree IDs but not which configured SSH endpoint and source path established it.
- Invariant/impact: persisted authority may be reused only when the public connection tuple
  `(connection id, host, normalized port, user, normalized configured path)` still matches;
  passwords, key material, and other secrets must never enter the provenance record.
- Proof: configured-alias preservation, changed-endpoint rejection, legacy-null-context,
  and public-only serialization regressions in `store/identity.rs`.
- Disposition: fixed with nullable schema-v3 `identity_context`, exact provenance matching,
  compatibility reads, and authenticated canonical-path preservation. Final GitHub Actions
  CI run `33799663655` passed.

### CAT-01 / P2 / Worktree Git output and timeout cleanup are unbounded

- Evidence: `3f386f2`; `mt-project/src/worktree/catalog.rs:51-115` used unbounded
  `read_to_end`, killed only the direct child, and joined readers after timeout.
- Invariant/impact: catalog scans must bound memory and wall time; descendants inheriting
  stdout/stderr cannot keep the caller blocked after timeout.
- Proof: an over-limit output fixture plus a shell that leaves a child holding both pipes.
- Disposition: fix implemented with 16 MiB per-stream capture, an end-to-end
  deadline, process-group cleanup, and bounded reader joins. Regression coverage is
  committed; verified by final GitHub Actions CI run `33799663655`.

## Terminal Host, PTY, And History

### TERM-01 / P1 / Exit can overtake final output and attach can miss exit

- Evidence: `8e8a7dd`, exposed by the callback-order stabilization in `25430a0`;
  `server.rs:191-233` records output and exit from independent callbacks while
  `server.rs:285-315` validates exit separately from stream subscription.
- Invariant/impact: every attached client and persisted history must observe all output in
  sequence before exactly one exit event; an attach cannot land between exit validation and
  subscription.
- Proof: callback inversion and attach/exit race regressions.
- Disposition: fix implemented with focused regression coverage; verified by final GitHub Actions CI run `33799663655`.

### TERM-02 / P1 / Restore fencing compares dynamic descriptors and misses exited conflicts

- Evidence: `c89250e`; `mt-app/src/pane.rs:394-414` compares the full descriptor after
  attach, including sequence bounds that legitimately change, while restore only identity-
  checks selected live-session paths.
- Invariant/impact: stable identity fields fence restore; dynamic output cannot invalidate a
  healthy restore, and an exited newer incarnation cannot be replaced by stale history.
- Proof: output-between-restore-and-attach plus exited-incarnation conflict fixtures.
- Disposition: fix implemented with focused regression coverage; verified by final GitHub Actions CI run `33799663655`.

### TERM-03 / P1 / Disconnected and partially-created sessions retain unsafe capabilities

- Evidence: `8e8a7dd`; `mt-app/src/pane.rs:644-650` marks a disconnected view read-only but
  retains its writable transport, and `client.rs:254-266` leaves the created process alive
  when attach fails.
- Invariant/impact: a disconnected pane cannot enqueue writes; create-and-attach is cleanup-
  atomic from the caller's perspective.
- Proof: disconnect-write and create-success/attach-failure regressions.
- Disposition: fix implemented with focused regression coverage; verified by final GitHub Actions CI run `33799663655`.

### TERM-04 / P2 / IPC reads, writes, and command queues are not fully bounded

- Evidence: `8e8a7dd`/`c89250e`; `client.rs:232`, `client.rs:711-733`, and
  `server.rs:634-639,658-663,713-731` use an unbounded command channel, unbounded JSONL
  accumulation, and writes without deadlines.
- Invariant/impact: a local peer cannot grow memory without limit or pin a runtime task by
  withholding a newline/read; all request phases share one finite budget.
- Proof: oversized-frame, stalled-reader, and queue-capacity fixtures.
- Disposition: fix implemented with focused regression coverage; verified by final GitHub Actions CI run `33799663655`.

### TERM-05 / P2 / Natural exit cannot be explicitly purged

- Evidence: `8e8a7dd`; `server.rs:577-593` calls `session.kill`, which rejects an already
  exited session before registry/history deletion.
- Invariant/impact: explicit close after natural exit must validate incarnation and remove
  registry/history even when there is no native process left to kill.
- Proof: natural-exit then close regression.
- Disposition: fix implemented with focused regression coverage; verified by final GitHub Actions CI run `33799663655`.

### TERM-06 / P2 / History recovery can truncate corruption and race its size bound

- Evidence: `c89250e`; `history.rs:532-553` treats any short non-magic suffix as a torn frame,
  and limited file reads check metadata before an unbounded read.
- Invariant/impact: only a valid frame prefix may classify a tail as torn, and the bytes
  actually read must remain bounded if the file changes after metadata inspection.
- Proof: short garbage-tail and concurrent-growth fixtures.
- Disposition: fix implemented with focused regression coverage; verified by final GitHub Actions CI run `33799663655`.

### TERM-07 / P2 / Spawned host child can become a zombie

- Evidence: `8e8a7dd`; the client spawns `mt-terminal-host`, drops `Child`, and relies on
  idle exit without a waiter.
- Invariant/impact: every spawned OS child is eventually reaped.
- Proof: fake host process exit/reaper fixture or deterministic waiter inspection.
- Disposition: fix implemented with focused regression coverage; verified by final GitHub Actions CI run `33799663655`.

### TERM-08 / P2 / Synchronous GPUI-side host RPC can stall interaction

- Evidence: `8e8a7dd`/`c89250e`; `mt-app/src/pane.rs:537-555` enters terminal client calls
  during pane construction, while client `block_on` paths include connect/hello/request work.
- Invariant/impact: terminal launch should not block the UI thread for transport timeouts.
- Proof: deterministic stalled-host construction path.
- Disposition: bounded residual accepted for this differential audit. Host RPC and
  cleanup paths now have finite budgets, but making pane construction fully asynchronous
  requires a broader cancellation/orphan-ownership redesign. That redesign is deferred
  rather than introduced without proof. Final GitHub Actions CI run `33799663655`
  verifies the bounded current path and its regression coverage.

## Remote Runtime, Agents, And GitHub

### REMOTE-01 / P1 / GitHub repository cache is root-path scoped, not worktree scoped

- Evidence: `5fb072e`; `execution_host.rs:65-70` signs only the root source path and
  `github_tasks.rs:748-765` reuses repository state by that key.
- Invariant/impact: linked worktrees with different worktree-local remotes must not share the
  wrong GitHub repository/account/task cache.
- Proof: two worktrees under one root with distinct remote discovery results.
- Disposition: fix implemented with focused regression coverage; verified by final GitHub Actions CI run `33799663655`.

### REMOTE-02 / P2 / SSH identity edits invalidate but do not restart runtime discovery

- Evidence: `40714bf`; `store/ssh.rs:46-66` removes affected runtime/poll state through
  `remote_runtime.rs:100-111` without scheduling fresh requests.
- Invariant/impact: projects using an edited connection must leave stale authority and begin
  fresh runtime discovery without requiring a manual retry or app restart.
- Proof: connection-edit fixture asserting new generation/request ownership.
- Disposition: fix implemented with focused regression coverage; verified by final GitHub Actions CI run `33799663655`.

### REMOTE-03 / P2 / Recreated poll state loses empty-inventory retirement history

- Evidence: `9e08319`; `store/remote_agents.rs:80-97,142-163` resets `had_processes`, so a
  recreated poll receiving empty success never applies retirement to an existing attested run.
- Invariant/impact: empty-confirmation hysteresis belongs to the route/run lifecycle, not one
  ephemeral poll-state allocation.
- Proof: process-attested run, poll-state recreation, then confirmed empty inventories.
- Disposition: fix implemented with focused regression coverage; verified by final GitHub Actions CI run `33799663655`.

### REMOTE-04 / P2 / Local/WSL command timeout can hang on descendant-held pipes

- Evidence: `5fb072e`; `execution_host.rs:287-364` kills the direct process then joins pipe
  readers, allowing descendants to retain handles indefinitely.
- Invariant/impact: command timeout owns and terminates the complete process tree on Unix and
  Windows, with bounded output-reader cleanup.
- Proof: Unix process-group and Windows Job Object/equivalent fixtures.
- Disposition: fix implemented with cross-platform regression coverage; verified by
  final GitHub Actions CI run `33799663655`.

### REMOTE-05 / P2 / Repeated connectivity observations create synthetic Agent events

- Evidence: the introduced remote reconciliation path called `mark_connectivity` with a new
  `AgentEventId` when no active run existed, and successful inventory handling reserved a
  global sequence before empty-inventory hysteresis determined that no observation applied.
- Invariant/impact: an unchanged transport observation must not advance event watermarks,
  recreate unread/attention state, or reorder the global Agent feed.
- Proof: `connectivity_change_requires_an_active_route_and_changed_state` plus changed-epoch,
  changed-connectivity, and empty-inventory hysteresis controls.
- Disposition: fixed by requiring at least one changed active exact-route run before emitting
  connectivity and by allocating inventory sequences only after hysteresis accepts the
  observation. Verified by final GitHub Actions CI run `33799663655`.

## Orca UI

### UI-01 / P1 / FileTree early return and async ownership omit WorktreeId

- Evidence: `572c832`; `file_tree/mod.rs:488-539,718-723` originally compared source
  signature/project generation but not the exact worktree identity.
- Invariant/impact: a same-signature rebind cannot retain or accept stale directory results
  from another worktree.
- Proof: worktree-only rebind and stale completion regressions.
- Disposition: fix implemented with focused regression coverage; verified by final GitHub Actions CI run `33799663655`.

### UI-02 / P1 / Same-path aliases route catalog rows to the global first project

- Evidence: `5188188`; `orca_sidebar.rs:249-267` selected the first configured local project
  by path for every catalog projection.
- Invariant/impact: a row matching the current project or one of its child worktrees routes
  to that exact configuration, independent of global configuration order.
- Proof: main, linked, and current-parent-child same-path alias fixtures.
- Disposition: fix implemented for main, linked, and child rows with focused regression
  coverage; verified by final GitHub Actions CI run `33799663655`.

### UI-03 / P2 / Shared-worktree Agent rows are duplicated and can route to an alias

- Evidence: `572c832`/`e1eacca`; `orca_sidebar.rs:1178-1227` queried the same worktree run
  list once per project alias and generated duplicate element IDs.
- Invariant/impact: one `AgentRunId` renders once under its exact `project_id`, preserving its
  exact route.
- Proof: shared-worktree aliases with duplicate run records.
- Disposition: fix implemented with focused regression coverage; verified by final GitHub Actions CI run `33799663655`.

### UI-04 / P2 / Same-WorktreeId alias switching clears presentation state

- Evidence: `572c832`; `file_tree/mod.rs:527-539` entered the clear branch when the project
  signature changed but `scope_changed` was false.
- Invariant/impact: aliases of the same worktree preserve selection, expansion, warnings, and
  scroll state while active requests/watchers are re-owned.
- Proof: same-worktree alias state-swap fixture.
- Disposition: fix implemented with focused regression coverage; verified by final GitHub Actions CI run `33799663655`.

### UI-05 / P1 / Shared-worktree Agent activation depends on project iteration order

- Evidence: the introduced global Agent target projection resolved one shared-worktree run
  through the first matching project alias returned by configuration iteration.
- Invariant/impact: activation must prefer the active exact project when it is a valid
  candidate and otherwise choose a deterministic project ID, independent of configuration
  order; the receipt must retain that exact route owner.
- Proof: active-alias and reversed-order deterministic selection regressions in
  `store/context.rs`.
- Disposition: fixed with explicit candidate ranking and exact activation receipts.
  Final GitHub Actions CI run `33799663655` passed.

## GitHub Actions, Staging, And Release

### REL-01 / P1 / Root and sidecar lockfiles are not enforced by owned gates

- Evidence: `0848513` plus the later lock drift repaired by `c644ae9`; owned
  helper/release commands omitted `--locked`.
- Invariant/impact: GitHub Actions CI and release gates must fail rather than silently
  resolve a dependency graph different from committed root or sidecar lockfiles.
- Proof: workflow command assertions and locked metadata/check/test/build gates for both
  workspaces.
- Disposition: fix implemented in Actions workflows and staging commands; verified by
  final GitHub Actions CI run `33799663655`.

### REL-02 / P2 / The legacy local Docker harness and staging ownership disagree

- Evidence: `0848513` and `8e8a7dd`; the introduced local Docker harness exported one
  build root while `stage-sidecars.mjs` originally read and wrote repository-relative paths.
  The Linux cache then mapped the `sidecars` workspace to `sidecars/target` relative to that
  workspace, selecting an unused nested target instead of the actual sidecar build directory.
- Invariant/impact: staging has one explicit job-owned build/stage authority, and executable
  CI or packaging cannot silently fall back to a workstation-owned Docker path.
- Proof: staging-plan path tests plus static Actions ownership and cache-root assertions.
- Disposition: explicit build/stage/cache roots are implemented, and the task-added local
  Docker CI harness is retired under the 2026-09-03 Actions-only policy. Final GitHub
  Actions CI run `33799663655` passed.

### REL-03 / P2 / Windows dev staging relinks the live terminal host executable

- Evidence: `8e8a7dd`; the new root-helper build targeted the same `target/debug` directory
  from which a running `mt-terminal-host.exe` may be executing.
- Invariant/impact: dev builds link into an isolated directory and only copy into the live
  stage; a locked destination preserves the old runnable artifact without corrupting it.
- Proof: isolated-plan and copy-failure regressions.
- Disposition: fix implemented with isolated link namespaces and strict release copy
  behavior; verified by final Windows package run `33799663753`.

### REL-04 / P2 / Wrong-architecture helpers can pollute a valid release stage

- Evidence: the introduced staging flow copied each helper as it completed and only later
  validated the assembled payload, so one wrong-machine executable could overwrite a valid
  staged file before the failure was reported.
- Invariant/impact: all built Windows executables must be non-empty x64 PE files before the
  first copy; a release failure removes every task-owned staged helper and portable-ConPTY
  directory so stale files cannot be packaged.
- Proof: wrong-machine-before-copy, stale-stage cleanup, and complete-stage verification
  regressions in `tests/stageSidecars.test.cjs`.
- Disposition: fixed with pre-copy build validation and failure cleanup. Final Windows
  package run `33799663753` passed.

### REL-05 / P2 / Task-owned artifact uploads use a deprecated action runtime

- Evidence: `4474c21` introduced `actions/upload-artifact@v4` in the CI workflow
  (current task tree `.github/workflows/ci.yml:109`), and `54a9620` introduced the same
  pin in the Windows package workflow (current task tree
  `.github/workflows/windows-package.yml:143`). GitHub Actions run `33795981213`
  completed successfully but warned that the action's deprecated Node.js 20 runtime was
  being forced onto Node.js 24.
- Invariant/impact: task-owned validation and release workflows must use an action that
  runs natively on the supported runner runtime; relying on compatibility forcing leaves
  diagnostic patches and verified installer evidence exposed to a future runner cutoff.
- Proof: the runner warning establishes that `@v4` uses the deprecated runtime, and static
  inspection locates the two task-owned pins. For this review, the supplied upstream facts
  identify `v7.0.1` as the current release; its action metadata declares `node24`, and its
  `archive` input defaults to `true`. The REL-05 diff leaves both multi-file `path` blocks
  and their existing `name`, `if-no-files-found`, and `retention-days` inputs unchanged,
  while matching assertions in `tests/stageSidecars.test.cjs` now require `@v7`.
- Disposition: fix implemented by upgrading both task-owned uploads and their static
  assertions to `actions/upload-artifact@v7`. Final package run `33799663753` uploaded
  artifact `9911452864` successfully through the Node 24 action runtime, with no Node.js
  20 deprecation or runtime-forcing warning. Run `33795981213` remains intermediate
  evidence only.

## Rejected Or Out Of Scope

- Pre-existing large typed layout JSON records: baseline behavior, not introduced here.
- POSIX path normalization behavior flagged by one reviewer: matches the active path contract.
- Watcher overlap suspicion: no deterministic introduced failure was established.
- Unlocked commands in the baseline `.github/workflows/ci.yml` were not an original
  differential finding because that file was unchanged in `0bc6f28..c644ae9`. The user's
  explicit 2026-09-03 Actions-only constraint made strengthening those gates an authorized
  task requirement rather than a retroactive baseline finding.
