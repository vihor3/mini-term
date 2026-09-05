# Remote Git Execution Plan

Inherit parent approval, Actions-only, and Trellis dispatch gates. Execute after
the Files child supplies the host-aware browser, and before Tasks account work
touches shared execution-host code. All operations in this document are future
implementation/Actions plans, not authorized live Git commands on user projects.

## Implementation Order

- [ ] Read the source action matrix and host/context/mutation contracts. Map
  every reachable Git child/dialog call before removing the remote guard.
- [ ] Add transport-free plans/parsers and narrowly expose reusable DTO/diff
  functions; preserve explicit Local/WSL/SSH dispatch and literal path semantics.
- [ ] Implement bounded read and epoch-pinned mutation adapters with explicit
  uncertain outcomes and a source-owned conflicting-write coordinator.
- [ ] Route repository/ref/status/history/diff loads with full request authority,
  truthful loading/stale/error state, and independent worktree caches.
- [ ] Route stage/unstage/discard/commit/pull/push; preserve confirmations,
  host Git configuration, drafts, and partial/late-effect reconciliation.
- [ ] Route every Worktree Management action through host-aware inventory,
  destination browsing, registration, and exact destructive target validation.
- [ ] Enable the remote panel only after all reachable operations are host-aware;
  request main's Trellis check and update affected specs after evidence agrees.

## Actions-Only Cases

- Disposable repositories covering empty/unborn, tracked/untracked/deleted,
  ignored, partially staged, conflict, rename, unusual filename/pathspec input,
  detached HEAD, merge history, root commit, binary and oversized diffs.
- Same-path Local/SSH isolation and exact-host child/dialog dispatch; no local
  filesystem/repository probe for a remote worktree or sibling destination.
- Branch filtering without checkout; parent-based history pagination; staged
  versus unstaged diff semantics; exact stage-all/discard behavior.
- Project/repository A-to-B-to-A, connection replacement before/after dispatch,
  dialog close/reopen, stale confirmation, timeout and uncertain effects.
- Conflicting operations across Changes/Sync/Worktree Management; external locks,
  changed HEAD/files, partial failures, last-known data, and draft retention.
- Host-qualified worktree create/add/switch/remove/prune, force confirmation,
  dirty documents, offline inventory, and no collateral terminal/config deletion.

## Acceptance and Risky Files

`git_panel`, `git_changes`, `git_history`, `git_diff`, `git_worktree`, shared Git
domain helpers, execution-host/remote mutation plumbing, and registration owners.
Use disposable runner fixtures only, never the user's repositories for automated
mutation tests. Remote parity requires Actions and matching artifact acceptance;
compiler success alone does not validate an actual authenticated SSH workflow.
