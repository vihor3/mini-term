# Git UI Integration Handoff

## Four-Module Source Release

SOURCE-COMPLETE AND RELEASED to main for independent review: `git_panel.rs`,
`git_changes.rs`, `git_history.rs`, `git_diff.rs`, their shared `host_ui` child
and focused tests. This is a source handoff, not a compile, CI or full R14 pass.
No four-module backend API blocker remains. The released
`GitBackend::repository_for_file(&str)` is consumed by the legacy FileTree diff
wrapper. Its pinned epoch is checked immediately after connection, before
repository resolution or status/diff reads.

The remote unsupported guard is INTENTIONALLY STILL PRESENT in
`GitPanel::render` / `is_remote_project`. Main requested it remain until Mill's
Worktree Management handoff confirms every reachable body is host-aware. Main
or the independent checker must remove it after that confirmation; this
four-module release does not wait for Mill and does not disable/remove the
Worktree Management feature. No additional `git_worktree.rs` edits followed
the ownership transfer recorded below.

### Changed Files

- `crates/mt-app/src/git_panel.rs`: source/worktree/generation-qualified panel
  state; detached backend connect/discovery/branch/sync/review dispatch;
  common-directory busy/uncertain presentation; exact sync completion/timer
  ownership; host-owned context actions and `open_repository` integration.
- `crates/mt-app/src/git_panel/host_ui.rs` (new): shared source/epoch and
  repository predicates, background-only exact repository resolver, outcome
  diagnostics, and instance-owned dialog close handling. APIs below are stable
  for Mill; no changes to his source are required by this release.
- `crates/mt-app/src/git_panel/source_tests.rs` (new): seven focused source,
  cache, sync-owner, HEAD-label and explicit draft-recovery regressions.
- `crates/mt-app/src/git_changes.rs`: host-routed status and all existing
  stage/unstage/discard/commit actions, prepared exact-target discard
  confirmation, shared write exclusion, revision-owned commit completion,
  retained drafts, source-qualified caches, original-source reconciliation,
  non-file untracked-directory presentation, and three new focused tests.
- `crates/mt-app/src/git_history.rs`: host-routed branch-filtered history and
  commit-file reads, all-parent continuation and real merge/dedupe helper,
  source/request fences and restored-depth refresh; four new focused tests
  plus the existing pagination regression now exercises the production helper.
- `crates/mt-app/src/git_diff.rs`: host-owned working-versus-HEAD,
  index-versus-HEAD and commit diffs, retained legacy entry points, backend
  nearest-file resolution, independently owned dialog lifetime/epoch/request
  fences, and three new focused tests.
- `.trellis/tasks/09-06-native-remote-git/ui-handoff.md`: integration contract,
  scope transfer, resolved backend request and this release inventory.

The earlier partial `git_worktree.rs` patch was transferred intact to Mill;
it is not part of this continuing four-module write release. Store, backend,
execution host, FileTree, layout/navigation, Tasks, main, i18n, workflows,
specs and Git index/commits were not edited by this UI owner.

### Preserved Behavior And Review Focus

The branch selector remains a history filter, never checkout. Reads use the
backend's libgit2 local adapter, with no new local CLI fallback. Working diff
is worktree versus HEAD; staged diff is index versus HEAD. Tracked discard
retains its staged-content effect and confirmation, while Stage All is an
explicit repo-wide operation. Untracked directories are counts/capability
information, not actionable file rows. Commit keeps its existing no-extra-
confirmation semantics and only clears the exact successful submitted draft.

Remote read caches require an equal full source, including epoch; Loading
state is never restored. User-authored text from a replaced epoch is retained
separately. An explicit `Restore draft` command is available only for the same
project/worktree/configuration/repository authority and an empty live draft.
This does not adopt old remote data or replay a write. Newer text/revisions,
other projects and replacement repository authority are never cleared or
overwritten by late results.

Blocking connect/discover/read/prepare/execute/review calls are dispatched on
detached background workers. Source/dialog disposal invalidates queued work;
an already dispatched write retains its backend worker/coordinator lease and
original-source reconciliation. Uncertain operations stay quarantined with
explicit original-operation review, without automatic retry or unlock.

### Authored Tests And Remaining Gates

Seventeen new focused pure tests are authored: seven in panel/source_tests,
three in changes, four in history, and three in diff. Existing pagination
coverage additionally uses the actual merge helper. These assert source and
epoch equality, A-B-A generation/operation ownership, draft revision retention,
qualified remote branch filtering, last-commit continuation, restored page
limits, dialog disposal/request supersession, literal POSIX root spelling and
WSL distribution isolation. They are not actual GPUI race or SSH acceptance
fixtures and none has been run by this owner.

- Main: independent source review and matching-head Actions compilation,
  formatting, Clippy and focused/full tests. Formatting has not been run;
  any resulting format-only patch must come from Actions, not local tooling.
- Main/Mill: finish Worktree Management and guarded store cleanup; publish
  reachable-action coverage before removing the remote panel guard. The store
  request below is transferred to Mill, not a pending four-module API request.
- Pauli: the legacy FileTree caller's literal-backslash spelling fix, as
  assigned by main from backend-review.md. No duplicate FileTree edit here.
- Main: matching Actions native/SSH/WSL acceptance for source-switch/dialog
  races, drafts, uncertain review, nested FileTree diffs and integrated
  worktree registration/document/terminal cleanup. Backend fixture limitations
  in backend-review.md remain applicable and are not waived by this release.
- Main/i18n owner: new bounded status/error/review/recovery text currently uses
  English literals; translation changes are outside this ownership boundary.

No local builds, metadata, tests, fixtures, syntax checks, lint, formatting,
generation, whitespace checks, app runs or SSH probes were performed. No Git
writes. All execution evidence remains outstanding; no passing CI claim.

## Scope Release To Main

`git_worktree.rs` is RELEASED to the dedicated worktree implementer following
main's split request. No further edits to that file will be made by this UI
implementer. The continuing write scope is `git_panel.rs`, `git_changes.rs`,
`git_history.rs`, `git_diff.rs`, `git_panel/host_ui.rs` and focused private tests.

The final pre-interruption patch DID touch `git_worktree.rs`. Preserve it and
continue from its source: imports of backend/source/picker registration types;
modal `snapshot`, `expected_repository`, backend/repository map, lifetime,
request fields, `is_live`/`repository` helpers, source observer and Drop; legacy
`open` now snapshots its explicit project at invocation and delegates to
`open_host`; a preliminary `open_repository` helper and guarded close handling
were added. The old load/branches/create/remove/prune and action bodies are NOT
migrated. In particular the import changed from `open_guarded` to
`open_guarded_with_close` while the old remove dialog still calls the former.
This is incomplete integration source, not a compile-ready worktree handoff.
The store API request below belongs to the new worktree/store owner.

## Git-To-Worktree API Agreement

Main's subsequent dispatch retained the existing scaffold name. This supersedes
the earlier `open_with_repository(store, ...)` proposal. The panel now calls:

```rust
pub(crate) fn open_repository(
    repository: GitRepository,
    on_changed: impl Fn(&mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
);
```

This is selected-repository mode (the old panel call had `discover_repos=false`).
`repository.backend().snapshot()` is the captured project/root/host/worktree and
observed epoch; `repository.authority()` is the exact selected repository.
Neither a new project ID nor a path should override those captured facts.
Worktree owner preserves `open(repo_path, discover_repos, project_id,
on_changed, window, cx)` and all public legacy path/branch helpers for external
project_list/orca_sidebar callers. The dedicated implementer owns
`git_worktree.rs` plus its new narrow store guard module/visibility changes.
The four-module UI owner has not touched `git_worktree.rs` after release.

`git_panel::host_ui` is `pub(crate)` and may be consumed without edits by the
worktree owner. Available functions: `snapshot_current(snapshot, store, cx)`,
`repository_current(repository, store, cx)`, `active_repository(...)`,
`resolve_repository(snapshot, path, lifetime)` (blocking background-only,
selects exact discovery path), `outcome_error(&GitWriteOutcome)`, and
`dialog_title(kind, title, lifetime)` (invalidates on its programmatic close).
Added `read_source_matches(captured_snapshot, observed_snapshot)` without
changing existing signatures: it allows the first observed SSH epoch only
when the captured epoch was `None`; an already pinned epoch must match exactly.
Dialog connection publication and pre-repository source observers should use
this instead of unconditionally ignoring epoch differences. It also checks
project ID and every source-signature field. Main should relay this addition
to the worktree owner so its initial dialog connect cannot retag a pinned source.
These are UI-owned; request changes through main. Source availability is not
passing CI evidence. Remote guard remains until worktree owner confirms that
all reachable bodies have host-aware dispatch.

## Resolved Backend File Resolver Request

RESOLVED: backend-review.md now publishes the implemented API below, including
authored native and authenticated-SSH fixtures. The preserved FileTree wrapper
consumes it. The design history below explains why this boundary was needed;
there is no outstanding resolver request or backend edit owned by this UI
implementer.

FileTree's preserved `open_file_diff(store, project_root, relative_file, ...)`
is not always a repository-root API: the original local implementation resolves
the containing/descendant repository and converts the file to repo-relative
spelling. Further reading of `git.rs::discover_repo_for` found a nested Git
repository must win even when the project root is itself a repository, so the
initial canonical-anchor getter proposal alone is insufficient. Please provide
this narrow read-only backend method (supersedes that getter-only request):

```rust
GitBackend::repository_for_file(&self, project_relative_file: &str)
    -> GitResult<(GitRepository, String)> // nearest repo + repo-relative file
```

It validates the literal relative file path, starts at the file's nearest
existing parent (deleted parent directories are possible), searches toward the
project anchor and at most its five allowed ancestors, and returns verified
repository authority from the same backend/lifetime. Descendant nested repos
must beat the project-root repo. No descendant bulk discovery is needed.
Local keeps native/libgit2 semantics without CLI; SSH/WSL use captured host
path semantics. The existing backend canonical anchor is the starting authority,
not the client OS. The four-module owner will call this only in the preserved
FileTree wrapper, then use normal backend reads with the returned path. No
backend source has been edited by this owner. Main approved and relayed this
to the backend checker, whose released source is now consumed by the legacy
wrapper. Pauli owns the upstream FileTree literal-path correction.

## Ownership And Design

UI implementer now owns only `git_panel.rs`, `git_changes.rs`, `git_history.rs`,
`git_diff.rs`, their private child modules and focused tests. Worktree ownership
was transferred as recorded above.
Main owns CI, specs, staging and commits. All execution verification is UNRUN
and Actions-only; no local automated commands or Git writes are authorized.

The original behavior gap was path-only local Git dispatch in the panel and
its reachable children. The four-module integration now carries
`GitRepository` from a background `GitBackend::connect` and discovery through
status, branches, history, diff and mutations, preserving presentation and
public legacy helpers. Mill owns the remaining worktree integration.
Source identity includes the captured project snapshot and observed SSH epoch;
UI generation/request/parameters and dialog lifetime independently fence it.
Worktree caches retain stable presentation only for an equal full source.
Suspended operations become refresh-needed, never restored Loading.

All blocking backend work runs in detached background workers. Invalidation
prevents queued dispatch without dropping a dispatched worker. Prepared writes
are captured before destructive confirmation and consumed once. The backend's
shared common-directory coordinator owns exclusion and uncertainty; UI never
replays or automatically releases a quarantined operation. Partial/late effects
reconcile only their original source and cannot clear another draft or spinner.

## Call Sites And Store Boundary

- Preserve `git_diff::open_file_diff` for FileTree and
  `git_worktree::open` for project_list/orca_sidebar. Add explicit repository
  entry points for Git UI callers. Legacy wrappers must capture a concrete
  project at invocation and resolve through its execution host, never local
  fallback. A path alone is not remote authority.
- Retain public worktree path/branch helpers unchanged for external consumers;
  remote code uses host-aware path semantics instead.
- Worktree browsing consumes `DirectoryPickerOptions` on the captured host.
  Open/Add/Switch use `ProjectLocationKey` and central `ChildWorktree`
  registration with the captured root, not later active-project lookup.
- Creation registers only after a verified normal completion and while the
  exact modal is live. Uncertain completion cannot authorize registration.
- Removal requires dirty-document and exact terminal snapshot validation.
  Existing store APIs are being inspected for an atomic, target-owned cleanup
  boundary. Any missing owner API will be specified here for main; the UI will
  not bypass that boundary with broad `dispose_project_terminals` or path-only
  config deletion.
- Prune consumes authoritative backend absence postconditions only. Offline or
  incomplete inventory is not cleanup authority, and manual visibility remains
  separate from deletion.

## Status

Four-module source implementation is released as detailed at the top. The
remote guard remains until Mill confirms every reachable worktree child is
routed. No compilation, test, format, syntax, whitespace, application or SSH
verification has run. Main can begin independent review without waiting for
the separate worktree/store release.

## Store API Request (Transferred To Mill)

Main assigned this request and destructive worktree finalization to the
dedicated worktree/store implementer. This section is the retained contract,
not a four-module implementation blocker. See worktree-handoff.md for its
current implementation status.

Source review found `terminal_close_request`/`close_terminal_target` already
retain exact attachment/alias ownership, but their request type is private to
`store::panes`. `dispose_project_terminals` and `remove_project` are broad,
project-ID-only calls and cannot serve as late Git cleanup authority.
Please supply/re-export the following narrow store boundary (names used by UI):

```rust
pub(crate) struct GitWorktreeRemovalGuard; // Clone, opaque retained owner

fn project_ids_for_location(&self, location: &ProjectLocationKey) -> Vec<String>;
fn prepare_git_worktree_removal(
    &self, location: &ProjectLocationKey, cx: &App,
) -> Result<GitWorktreeRemovalGuard, String>;
fn git_worktree_removal_is_current(
    &self, guard: &GitWorktreeRemovalGuard, cx: &App,
) -> bool;
fn close_git_worktree_terminals(
    &mut self, guard: GitWorktreeRemovalGuard, cx: &mut Context<Self>,
) -> Task<Result<GitWorktreeRemovalGuard, String>>;
fn finish_git_worktree_removal(
    &mut self, guard: GitWorktreeRemovalGuard, cx: &mut Context<Self>,
) -> Result<(), String>;
```

Capture host-qualified location, every matching configured alias, exact
configuration/binding and terminal-close snapshots, and dirty-document refusal.
The read-only lookup must use the registration boundary's authority rules,
including SSH canonical aliases and Local/WSL distinction, not client path
normalization for SSH. Revalidation must reject newly created aliases,
replacement terminals/PTYs, changed bindings/configuration, or dirty documents.
The close task consumes the captured terminal requests and returns an updated
guard only after those exact targets are proven closed and no new ones exist.
It must not release/cancel an already dispatched terminal-close worker.
Finalization checks the updated guard and an empty exact terminal inventory
before calling normal project removal. Failed/stale close retains configuration.

UI will capture the guard before confirmation, revalidate before terminal close
and Git dispatch, and finalize only a normal verified `WorktreeRemoved` result
for the same live dialog/source. Prune can call prepare/finalize without the
close task only for backend-proven missing paths; finalization must refuse any
remaining terminal record. Neither API interprets Git uncertainty or guesses
remote absence. No store file has been edited by the UI owner.
