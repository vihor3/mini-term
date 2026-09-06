# Four-UI Review

Status: four-module SOURCE REVIEW COMPLETE; four-UI writes RELEASED to main.
Remote enablement is now source-complete and RELEASED; the final addendum
supersedes the earlier guard, backend and locale holds recorded below.
Reviewer: same trellis-check; Kepler closed. This is not an integrated feature,
build, test, transport or native acceptance pass. Backend request below is open.
Files/onboarding remain RELEASED. Worktree/cleanup belongs to Bohr.
The remote guard remains until Bohr completes and main authorizes removal.

## Findings (fixed)

- P1, `git_changes.rs:254`: Changes row/menu authority omitted the producing
  status request. An old menu
  survives a same-source refresh and can prepare a mutation from new status.
  Added a captured action owner with the producing request, repository authority
  and current-status gate. Row/group/menu/diff/commit-button/key callbacks reject
  refresh, failure, source change and ABA. Write completion retains its separate
  operation owner so a status refresh cannot strand its spinner or reassign it.
- P1, `git_panel.rs:225`, `git_changes.rs:243`: rediscovery could retain a path
  while replacing its Git/common-directory authority without invalidating the
  parent generation. That now disposes old source requests and menu callbacks.
  Changes completion/cache refresh also compares captured repository authority.
  Pull/push/review buttons and repository/branch/context menus retain their
  rendered scope/request instead of silently adopting a newer selection.
- P2, `git_diff.rs:555`: source invalidation also invalidated the token required
  by the visible close button. Two lifetimes now separate source reads from
  dialog disposal. Reconnect/config invalidation is checked on source events,
  rendering, selection, dispatch and publication. Both close paths invalidate
  this instance before removal; a stale old handler cannot close a successor.
  Empty commit diffs no longer mask source errors as No Changes. Diff request
  overflow invalidates reads instead of wrapping back to an old request.
- P1 caller boundary, `git_panel.rs:670`: readiness checked a captured SSH epoch
  only after discovery. It now rejects replacement readiness before following
  discovery reads and checks active source before publishing errors too.
  Connect's internal anchor read is separately NOT FIXED below.
- P2, `git_panel.rs:616`, `git_history.rs:326`: late write reconciliation after
  A-B-A called history reload, resetting the
  newly restored scroll/page depth. Only exact-owner completion may reset it;
  stale/shared-source completion now requests an in-place read refresh.
  Recreated repository authority also gets a newly owned sync-clear timer;
  Loading never schedules that timer, and an earlier sync error clears when a
  new owned sync starts. No completion clears a newer operation's state.
- P1, `git_panel.rs:256`: WSL Open In Terminal used unchecked POSIX-to-UNC conversion,
  allowing literal backslashes to select another directory. The Git caller
  now consumes the released browser's checked DirectoryLocation::host_path;
  an unrepresentable path reports an error, while Worktree Management remains
  available. No shared model/host/store API or implementation changed.

## Coordination

### Bohr: Owned Close Pattern Available Now

No shared signature change. `host_ui::dialog_title(kind, title, GitLifetime)`
remains unchanged; its one token cannot represent both source invalidation and
visible-instance disposal. Current Worktree `actions::manager_title` can mirror
the now-authored private `git_diff.rs::DiffLifetime` without shared-file edits:

```rust
#[derive(Clone, Default)]
struct DiffLifetime {
    view: GitLifetime,
    read: GitLifetime,
}
// Source/epoch invalidation: read.invalidate(); view remains valid.
// Dispose/on_close/Drop: view.invalidate(); read.invalidate().
// Visible close callback captures BOTH tokens from this modal instance:
if lifetime.view.is_valid() && overlay::is_top(overlay::key(kind)) {
    lifetime.close(); // invalidates both BEFORE programmatic removal
    prompt::close_guarded(kind, window, cx);
}
```

For Worktree, retain its existing `lifetime` as the source/queued-dispatch token
and add a distinct cloned `view_lifetime` (or disposed flag) to the modal.
Source watcher/epoch failure invalidates only existing source/child tokens.
Manager close, every user on_close, and Drop invalidate the instance token too.
The close callback must test its captured instance token, NOT `is_live` or the
source token. A nested overlay leaves both tokens untouched; a disposed old
handler cannot close a successor of the same kind. Programmatic close does not
run Dialog::on_close, so dispose before close_guarded. Preserve detached write
workers and backend lease ownership. Diff uses this pattern in both dialogs.

Production-helper regression authored (UNRUN):
`git_diff::tests::stale_diff_source_remains_closable_without_closing_a_successor`.
No Worktree files edited; Bohr owns its mirror and regressions.

### Backend Boundary

Main confirmed this boundary and assigned the narrow backend/tests fix to
resumed McClintock, preserving explicit recovery-read epoch bootstrap. No
backend edit is needed or authorized from this four-UI reviewer. Main also
confirmed the private DiffLifetime pattern was relayed to Bohr; no shared
signature change is pending.

- P1, backend-owner request: `git_backend.rs::GitBackend::connect` (lines 93-97)
  overwrites a captured `Some(epoch)` with `git_connection_epoch`, then calls
  `host.canonical_directory` before any UI post-connect check. Our caller now
  rejects replacement epochs before discovery; the same guard already exists
  in Diff/host_ui. None can prevent connect's own replacement-session anchor
  read. Recommend backend refusal immediately after acquiring an epoch when
  captured Some differs, before canonical-directory I/O; preserve absent-epoch
  readiness bootstrap. No backend edit made; this boundary is NOT FIXED here.
- No shared signature changes requested. Preserve host_ui APIs
  consumed by Worktree Management and do not edit backend/store/Files/spec/CI.

## Source Contracts Reviewed

- Loaded current check.jsonl entries, PRD/design/implement, backend review, final
  UI handoff, package indexes/quality and shared guides, and updated Git-host,
  worktree, onboarding and execution/workbench identity contracts.
- Traced the four modules and host_ui through captured backend request/results,
  prepared writes, original-source reconciliation and disposal. Discard prepares
  exact paths before confirmation and consumes that prepared value once; no
  confirmation recaptures a new backend or widens paths. Stage All remains
  repository-wide. Detached workers and uncertain leases remain backend-owned.
- Commit text clears only for the successful submitted draft revision. Source
  caches retain full signature/epoch and worktree identity; explicit retained
  draft recovery is not read authority. History retains branch-as-filter,
  all-parent continuation, deduplication and independent worktree scroll/depth.
- FileTree's existing wrapper validates its captured project root, calls the
  nearest-file repository resolver and uses the exact returned repository path.
  It retains staged/working semantics, rename old paths and literal POSIX text;
  it never routes WSL/SSH to local Git. The released Files row.rel correction
  remains untouched. Commit history double-click and legacy entrypoints remain.
- Exact-ID uncertain review retains the stopped-operation acknowledgement and
  original backend reconciliation. No new write replay, rollback claim,
  registration/cleanup authority, credential source or transport was added.

## Changed Paths / API

- `crates/mt-app/src/git_panel.rs`
- `crates/mt-app/src/git_panel/source_tests.rs`
- `crates/mt-app/src/git_changes.rs`
- `crates/mt-app/src/git_history.rs`
- `crates/mt-app/src/git_diff.rs`
- This review report.

`git_panel/host_ui.rs` and all shared signatures remain unchanged. The only
visibility change is `GitHistoryContent::refresh` to `pub(crate)` for the owned
panel's in-place reconciliation. The checked browser conversion is consumed,
not modified. No locale key delta (0); no registry/count/generated-dict edits.

## Tests Authored (UNRUN)

- `git_changes::tests::status_refresh_revokes_old_row_menus_without_reassigning_write_completion`
- `git_panel::source_tests::rediscovery_at_the_same_path_revokes_replaced_repository_authority`
- `git_panel::source_tests::detached_completion_refreshes_without_resetting_a_returned_history_view`
- `git_panel::source_tests::repository_terminal_cwd_never_reinterprets_posix_names_as_wsl_separators`
- `git_history::tests::history_refresh_retains_depth_but_explicit_reload_resets_to_first_page`
- `git_diff::tests::stale_diff_source_remains_closable_without_closing_a_successor`
- `git_diff::tests::diff_file_selection_a_b_a_keeps_only_the_latest_request`

The existing sync ABA regression now invokes the production predicate used by
click/completion/timer handling. New tests exercise production predicates and
path conversion, not source-text assertions or local filesystem fixtures.
Actions filters: `git_panel::source_tests`, `git_changes::tests`,
`git_history::tests`, `git_diff::tests`. No new SSH fixture/setup is needed here;
the separately owned authenticated backend fixtures remain the transport gate.

## Findings (not fixed) / Verification

- Backend pinned-readiness anchor read is McClintock-owned as described above; no
  claim that a UI post-connect predicate fully fixes that end-to-end boundary.
- Remote enablement remains held for Bohr and explicit main authorization.
  Worktree/cleanup and the mirrored manager-close fix are not this source gate.
- Literal English Git error/review/draft UI text still needs main's combined
  locale integration decision. No shared locale scope was assumed.
- Lint: UNRUN. TypeCheck/build: UNRUN. Tests/fixtures: UNRUN. Format/whitespace/
  metadata/codegen/probes/app/automated UI: UNRUN. No local automation, child
  agents, Git writes, staging or reversion was performed.
- Native narrow/high-DPI/long-path sizing, final-row wheel/thumb scrolling,
  keyboard and nested-overlay close behavior, draft restoration, cancellation,
  reconnect/uncertain review and real Local/Windows/WSL/SSH actions require
  matching-SHA Actions artifacts and native acceptance. Source inspection and
  authored pure tests do not establish those results.

## Authorized Remote Enablement

Status: narrow enablement SOURCE COMPLETE; `git_panel.rs` writes RELEASED to
main for combined staging. Main explicitly authorized this after Bohr's final
Worktree routing gate and McClintock's before-anchor epoch correction released.

- Read `worktree-review.md` Reachable Action Pass and the final handoff's
  Completed Action Paths. Inventory/branches, creation, destination browsing,
  Open/Add/Switch, Remove/Force, Prune and uncertain review retain captured host
  routing. Local libgit2 fallback is a Local-only read capability, not a remote
  fallback or mutation authority. No reachable local-only remote action remains
  in that released matrix or the previously released four-UI routing pass.
- Read the released backend follow-up and the changed connect boundary:
  differing captured Some(epoch) is rejected before assigning the new pin or
  canonical-anchor I/O. Explicit recovery-read epoch adoption remains separate.
  The earlier backend finding is resolved by its owner, not by this UI edit.
- Exact code edit: deleted private `GitPanel::is_remote_project` and the
  `GitPanel::render` remoteNotSupported early-return block. There was no remaining
  remote-only loading guard; `load_repos` already connects/discovers through the
  captured GitBackend. Local, WSL and SSH now reach the same host-aware UI path.
- Kept no-project selection, source/request/epoch/authority checks, individual
  capability/error/uncertain states and Worktree Management's captured
  `git_worktree::open_repository` action unchanged. No backend, store, Worktree,
  Files, Tasks, host_ui, test, locale, workflow or generated file was edited.
- No new test-only predicate or source-text guard assertion was introduced for
  deleting the obsolete branch. The existing focused production-helper tests
  listed above, backend epoch regression and actual SSH fixtures remain the
  Actions gates. No tests or other verification were run for this delta.
- Main approved retaining the existing literal-English Git diagnostic/review/
  draft style. Existing translated labels are unchanged; Git locale key delta
  remains zero, and no broad localization work is outstanding for this slice.

Changed in this enablement follow-up only: `crates/mt-app/src/git_panel.rs` and
this report. Lint/type-check/build/tests/format/metadata/whitespace/codegen/
fixtures/probes/app/automated UI remain UNRUN and Actions-only. No child agents
or Git writes. Source release is not transport or native acceptance; main owns
exact-SHA Actions and matching artifact verification.

## Compiler Integration Follow-Up

Status: bounded SOURCE CORRECTION COMPLETE; the five source paths below and
this report are RELEASED to main. Input was main's actual Linux/Windows failure
report for HEAD `4660367`, run `34005271807`, jobs `101411287139` and
`101411287151`. This is not a claim that the next Actions compilation passes.

- `crates/mt-ssh/src/lib.rs`: re-export the existing public
  `sftp::SftpBoundedFileRead` at the crate root used by Git SSH consumers. Read
  the existing enum/variants/method signatures first; `sftp.rs` is unchanged.
  No new type, transport behavior or consumer rewrite was introduced.
- `crates/mt-app/src/project_onboarding/view.rs`: import the existing
  `picker_open_is_current` into its test module. Retain the correct helper name,
  production implementation and all seven assertions; do not substitute the
  similarly named request helper suggested by the compiler.
- `crates/mt-app/src/git_backend.rs`: remove only the unused binary-module
  `GitBusy` and `GitReconciliation` re-exports. Their definitions and returned
  values/signatures in `write.rs` remain unchanged; no source consumer names
  these re-exports.
- `crates/mt-app/src/remote_ssh/dirs.rs`: remove the unused `remote_home` import.
  Authenticated canonical-home resolution, epoch checks and fixtures unchanged.
- `crates/mt-app/src/store/git_worktree_cleanup.rs`: remove only the reported
  unused `AppContext` trait import. Cleanup ownership/mutations unchanged.

No new tests are needed for these name-resolution/import corrections. Existing
focused regression to execute in Actions:
`project_onboarding::view::tests::browser_open_button_retains_its_form_context_before_allocating_a_request`.
Both platform app/test builds must be rerun to establish that the missing root
export and all seven missing test-name errors are resolved. Source inspection
and main's prior failing logs are not passing compiler evidence for this patch.

Lint/type-check/build/tests/format/metadata/whitespace/codegen/fixtures/probes/
syntax/app verification: UNRUN locally, Actions-only. No child agents, staging,
commits, shared index edits, Agent core/probe edits or changes outside the five
listed source files and this report. No source policy or mutation behavior changed.

## Clippy Repair Coordination

Status: 16/16 assigned nonbackend diagnostics corrected in source; SOURCE
COMPLETE and RELEASED to main. Both narrow sibling-file scope requests were
authorized and completed. Read the actual log for run
`34005933974`, Linux job `101412980006` at HEAD `37b4ef9`; backend diagnostics
remain exclusively McClintock-owned. No local Clippy/check execution.

Authorized final followup:

- `crates/mt-app/src/remote_ssh/tests.rs`: changed the sole `download_conflicts`
  assertion to production `download_conflicts_for_files`, preserving the same
  `/remote/C:evil.exe` unsafe-name input and error expectation in
  `remote_ssh::tests::local_download_targets_stay_inside_root`. Test UNRUN.
- `crates/mt-app/src/remote_ssh/transfer.rs`: removed only the now-unused
  `download_conflicts` wrapper and its comment. Pinned production entrypoints,
  `download_conflicts_for_files`, shared policy helpers and fixtures unchanged.
- `crates/mt-app/src/remote_ssh/mod.rs`: changed only the retired listing doc
  link to `list_directory_at_epoch`. No module declarations, exports or runtime
  changes.
- `.trellis/tasks/09-06-native-remote-git/ui-review.md`: recorded this release.
  These four paths are the exact changed paths for the final authorized followup.

All other listed wrappers have no production, fixture or rollback callers.
The unused file_ops parent/self helper's isolated test is superseded by the
existing production `file_tree::tests::creation_and_drop_share_directory_file_parent_and_blank_targets`
and POSIX-parent tests. Existing pinned implementation and cleanup helpers stay.

Completed source edits (all 16 assigned nonbackend diagnostics):

- Removed unused `entry_target_directory` and its isolated obsolete test from
  `file_ops.rs`; retained the existing production FileTree target regressions.
- Removed unused `host_ui::dialog_title` and its now-unused UI imports. Current
  Diff/Worktree instance-owned close controls and all live host_ui APIs remain.
- Removed unused unpinned wrappers: `delete_entry`; `list_directory`,
  `list_directory_for`, `create_entry`, `rename_entry`; `copy_entry_keep_both`,
  `upload_conflicts`, `upload_paths`, `download_conflicts`, `download_entries`.
  All `*_at_epoch` bodies,
  provenance, shared implementation helpers, staged commit/rollback/cleanup,
  upload_paste behavior and actual SSH fixtures remain unchanged. The sole
  obsolete download-preflight test caller now exercises the production helper.
- Applied the exact Clippy style corrections to `git_panel.rs` StatusLoaded,
  `github_tasks.rs` visibility short-circuit, `github_tasks/service.rs` Option
  early return, and the live Change arm in `remote_directory_picker.rs`.
  Condition order and side effects are unchanged; no Tasks redesign.

No assigned nonbackend source finding or scope request remains open. No
dead-code suppression, fake calls, black_box or unpinned production caller was
added. No backend or Agent edits. This source release is not a passing Clippy
or compilation result; execution remains Actions-only.

Changed paths for the full nonbackend Clippy pass:

- `crates/mt-app/src/file_ops.rs`
- `crates/mt-app/src/git_panel.rs`
- `crates/mt-app/src/git_panel/host_ui.rs`
- `crates/mt-app/src/github_tasks.rs`
- `crates/mt-app/src/github_tasks/service.rs`
- `crates/mt-app/src/remote_directory_picker.rs`
- `crates/mt-app/src/remote_ssh/delete.rs`
- `crates/mt-app/src/remote_ssh/dirs.rs`
- `crates/mt-app/src/remote_ssh/transfer.rs`
- `crates/mt-app/src/remote_ssh/tests.rs`
- `crates/mt-app/src/remote_ssh/mod.rs`
- `.trellis/tasks/09-06-native-remote-git/ui-review.md`

Verification: only the authorized Actions-log read and source/Git inspection.
Local lint/build/test/format/metadata/whitespace/syntax/probe/fixture/app checks
are UNRUN. Existing Linux/Windows compile success at `37b4ef9` predates this
patch; the next exact-SHA Actions run must establish compilation and Clippy.
No child agents, staging, commits or changes to shared index/Agent files.
