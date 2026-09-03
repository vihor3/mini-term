# Technical Design

## Differential Audit Model

The audit treats `0bc6f28..c644ae9` as a causal boundary, not merely a convenient
file list. Reviewers may inspect current neighboring code and archived task contracts,
but each finding must point to an introduced line, changed ownership boundary, or new
interaction caused by this range. This prevents legacy defects from expanding scope.

The current tree is the execution target. The historical diff explains causality; current
source determines whether the issue still exists after later commits in the range.

## Review Domains

### 1. Catalog, Identity, Persistence, And Workbench

Owns `mt-project`, `mt-identity`, `mt-layout`, `mt-config`, and the identity/layout/
persistence/document portions of `mt-app`. Review stable derivation, migration salvage,
dual-write ordering, collision/rebind behavior, preview buckets, and stale callback fences.

### 2. Terminal Host, History, And PTY Lifecycle

Owns `mt-terminal-host`, `mt-terminal`, `mt-pty`, and terminal-facing `mt-app` paths.
Review IPC framing, current-user permissions, process ownership, attach/detach/kill,
incarnation fencing, replay ordering, history corruption, shutdown, timeout, and resource
cleanup.

### 3. Remote Runtime, Agent Identity, And GitHub Tasks

Owns `mt-ssh`, `mt-ai::agent_runtime`, `mt-github`, execution-host routing, remote runtime
state, remote Agent polling, and GitHub Tasks. Review authentication authority, host-key
continuity, connection epochs, bounded exec/SFTP, retry classification, shell quoting,
secret handling, account/repository generations, and process identity.

### 4. Orca UI, Context Sidebar, And Global Agent Feed

Owns the presentation/event-routing portions of `main`, `orca_sidebar`, `workbench_area`,
file/Git/session panels, context store, overlays, and global activity feed. Review exact
worktree scope, rapid navigation, focus restoration, overlay lifetime, preview promotion,
selection persistence, empty/error states, and stale UI completions.

### 5. GitHub Actions, Sidecars, And Windows Release

Owns GitHub Actions workflows, legacy Docker harness files in the review range, staging
scripts, NSIS manifest, Cargo workspace wiring, and sidecar lock reproducibility. Review
locked dependency graphs, Action cache/build ownership, PE architecture/resources, payload
completeness, failure cleanup, and artifact upload. All executable validation and packaging
runs in GitHub Actions; the local machine performs no compile/test/lint/stage/package work.

Read-only reviewers may overlap on cross-layer call paths. Fix ownership is assigned only
after triage so two workers never edit the same file concurrently.

## Finding Contract

Each candidate is recorded with:

```text
ID / severity / domain
introduced evidence: commit + file:line or diff hunk
violated invariant and user-visible impact
reproduction or deterministic proof
recommended minimal fix
disposition: confirmed / duplicate / baseline / not-a-bug / residual
validation added
```

Severity prioritizes P0 data-loss/security/identity corruption, P1 crash or wrong-host/
wrong-worktree behavior, P2 boundedness/recovery/leak/portability defects, and P3 local
maintainability issues with a concrete failure-prevention benefit.

## Triage And Fix Flow

1. Generate the immutable scope inventory.
2. Run five parallel read-only review passes with archived contracts and domain specs.
3. Consolidate candidates and independently inspect every P0-P2 claim in the main session.
4. Reject baseline-only observations and duplicates before any edit.
5. Create focused failing tests or deterministic fixtures.
6. Apply minimal fixes in disjoint file batches; avoid architecture changes unless the
   existing contract cannot be made correct locally.
7. Re-run domain checks, then cross-layer and release gates.

## Robustness Principles

- Identity is never inferred from display labels, indexes, raw paths, or connection IDs.
- Async completion must carry every owner/generation fact required to reject stale work.
- A timeout does not imply an operation did not start; fallback and retry require explicit
  protocol state.
- Detach preserves a live process; close/kill destroys it; cold restore creates a new
  incarnation and cannot masquerade as warm reattach.
- Persisted data is additive, bounded, checksummed where needed, salvageable, and never
  overwritten by an older compatibility projection.
- Remote commands are bounded and quoted structurally; credentials and provider payloads
  do not enter logs, persisted history, or UI diagnostics.
- Rollback gates switch ownership/presentation without deleting newer state.

## Validation Ladder

Every executable rung runs in GitHub Actions, never on the local workstation:

- Changed-line formatting and static workflow/script checks.
- Focused unit/regression tests per finding, including fault/cancellation/corruption paths.
- Full locked workspace tests, check, and Clippy.
- Independent sidecar `--locked` metadata/check/test.
- Windows MSVC checks for affected root and sidecar payloads.
- Conditional Windows rebuild, NSIS extraction, PE/resource/marker checks, exact payload
  hashes, and uploaded Action artifact.

## Rollback

Fixes are committed in small domain batches. A batch must preserve existing environment
rollback controls and persisted formats. If a fix unexpectedly changes approved behavior,
revert that batch without reverting the original Orca implementation or unrelated work.
