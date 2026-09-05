# Remote Git Domain Review

Status: bounded source review and approved directory-status followup complete;
domain writes RELEASED. All automated verification UNRUN. This is not the full
feature quality/acceptance gate.

## Early Adapter Notice For Main To Forward To Turing

- Approved API addition: `cli::RepositoryStatus.untracked_directories: Vec<String>`
  retains exact reported relative directory spelling including one trailing
  slash. These entries are NOT `ChangeFileStatus` rows or file-action targets.
  Existing local/global DTOs and function signatures stay unchanged. Main will
  relay the new field to Turing/UI; this review does not edit app callers.
  Include directories when projecting dirty/untracked presence, but never
  convert them into actionable regular-file rows by removing the slash.
- The write boundary is
  `crates/mt-project/src/git/cli.rs`, its `cli/{parse,tests}.rs` children, and
  this review. Existing local implementations are left intact.
- Fixed: `GitRef::parse` conflated fully qualified existing ref syntax with new
  branch shorthand restrictions. A legal plumbing-created `refs/heads/-legacy`
  made the entire refs/worktree capture fail. Inventory parsing and
  `from_branch` now retain exact full refs; user branch creation
  and worktree shorthand still require their stricter checks. See
  [Git's distinction between ref and branch syntax](https://git-scm.com/docs/git-check-ref-format).
- Do not pass an inventory `GitRef`'s short name directly to revision commands.
  Continue using `resolve_ref_plan` and resolved `ObjectId` history tips. Worktree
  add has different shorthand semantics and must revalidate current branch
  existence/ownership immediately before dispatch; a pure plan is not that proof.
- Confirmed and fixed: the common command builder exported literal pathspec
  behavior into commit/sync/worktree hooks. `--literal-pathspecs` is now present
  only on file-path plans and Stage All, still before the Git subcommand. Do not
  re-add it globally in the adapter. It is an inherited environment setting, not
  only argument escaping. See [Git global options](https://git-scm.com/docs/git).
- Malformed captures mixing SHA-1/SHA-256 IDs now fail across a whole status,
  branch, log, or worktree response, including unborn HEAD. No DTO changes.
- Fixed under main's approved API decision: `? nested/` is now preserved in
  `untracked_directories`. Exactly one trailing slash is required, the full
  reported spelling shares the normal path byte/component bounds, and file/
  directory records share duplicate detection and the 20,000-entry limit.
  File-only stage/diff/discard still reject directory paths; explicit Stage All
  remains repository-wide. No recursive untracked discard was added.

## Findings (fixed)

### Hook Environment Leakage

- File: `crates/mt-project/src/git/cli.rs:78`, `:220`, `:502`.
- Issue: the common builder enabled `--literal-pathspecs` for commit, pull,
  push and worktree operations. Git exports this setting to child processes,
  changing pathspec behavior inside otherwise unchanged user hooks.
- Fix: apply the global option only to file-path plans and Stage All, before
  the subcommand. Commit/sync/worktree plans inherit normal host behavior.
- Regressions: `literal_pathspecs_stay_global_without_changing_unrelated_hooks`
  and `actual_commit_hook_keeps_its_own_git_pathspec_semantics`.
- Evidence: [Git option implementation](https://github.com/git/git/blob/v2.45.0/git.c#L257)
  and [Git environment contract](https://git-scm.com/docs/git).

### Existing Ref Compatibility

- File: `crates/mt-project/src/git/cli.rs:132`, `:153`, `:162`, `:585`;
  `crates/mt-project/src/git/cli/parse.rs:136`.
- Issue: a legal full ref with a shorthand such as `-legacy`, `HEAD` or `@`
  invalidated the entire enumeration; the 16 KiB check omitted the namespace.
- Fix: validate existing fully qualified names separately from user branch
  shorthand, preserve exact refs in status/DTO round trips, enforce the full
  ref byte bound, and retain a shorthand check at worktree add. Full refs are
  only resolved by the existing qualified revision plan, never shell text.
- Regressions: `existing_refs_are_not_reinterpreted_as_branch_creation_input`
  and `actual_existing_refs_remain_qualified_through_read_plans`.
- Evidence: [Git ref vs branch validation](https://git-scm.com/docs/git-check-ref-format).

### Malformed Capture Acceptance

- File: `crates/mt-project/src/git/cli/parse.rs:110`, `:383`, `:412`, `:441`.
- Issue: unborn status, refs, log and worktree lists could mix SHA-1/SHA-256
  records. Tree metadata accepted invalid spacing outside its padded size
  column. Tree/index parsing allocated vectors for arbitrary metadata field
  counts. A terminated worktree row without HEAD or bare was accepted.
- Fix: track one object width per capture; split only the bounded expected
  metadata fields; allow padding only for tree size; require HEAD on non-bare
  worktrees and reject checkout fields on bare rows. Valid unborn/zero-HEAD,
  detached, locked, prunable and POSIX path behavior remains supported. The
  existing shared worktree parser is unchanged.
- Regressions: `repository_captures_reject_mixed_object_formats_even_without_head`,
  `exact_lookup_absence_requires_complete_success_and_single_matching_path`,
  `worktree_records_require_head_or_bare_without_erasing_prunable_rows`, plus
  the actual SHA-256 fixture.
- Evidence: [tree format](https://git-scm.com/docs/git-ls-tree),
  [index format](https://git-scm.com/docs/git-ls-files), and
  [worktree record emission](https://github.com/git/git/blob/v2.45.0/builtin/worktree.c#L872).

### Untracked Directory Status (Approved Followup)

- File: `crates/mt-project/src/git/cli.rs:272`;
  `crates/mt-project/src/git/cli/parse.rs:136`.
- Issue: valid `? nested/` output for an embedded untracked repository rejected
  the whole parent status capture and blocked unrelated modified file rows.
- Fix: main explicitly approved the new transport-free status field
  `pub untracked_directories: Vec<String>`. The parser retains the reported
  relative spelling including its single trailing slash and keeps ordinary
  changes intact. Files/directories share one duplicate-key set and record
  limit. A directory and file at the same relative name cannot both appear.
  Unborn HEAD, ignored exclusions and invalid UTF-8 failure remain intact.
- No `ChangeFileStatus` or existing local/global DTO changed. File-only
  stage/unstage/diff/discard continue rejecting trailing-slash entries; no
  recursive deletion intent exists. Explicit Stage All still stages the whole
  repository, including Git's normal embedded-repository/gitlink behavior.
- Regressions: `status_preserves_untracked_directories_outside_actionable_file_rows`,
  `status_rejects_malformed_and_duplicate_directory_paths`,
  `status_counts_files_and_directories_against_one_record_limit`, and
  `actual_nested_repository_status_retains_modified_parent_and_explicit_stage_all`.
  The actual fixture uses production status/stage plans with a committed nested
  repository and a modified parent file. All tests remain UNRUN.
- Evidence: [Git directory walk](https://github.com/git/git/blob/v2.45.0/dir.c#L1805)
  and [status collection](https://github.com/git/git/blob/v2.45.0/wt-status.c#L722).

## Findings (not fixed)

1. Legacy refs remain readable but cannot all be used as worktree shorthand:
   `cli.rs:585` still explicitly rejects `-legacy`, `HEAD`, and `@`. This is a
   retained fail-closed limitation, not a new mutation API. Do not substitute a
   full ref blindly: Git 2.45's
   [branch-vs-detached worktree logic](https://github.com/git/git/blob/v2.45.0/builtin/worktree.c#L406)
   uses branch shorthand and has different resolution rules. Extending this
   needs a deliberate exact-branch creation protocol and caller postcondition
   checks. Display an owned capability error meanwhile; never retarget HEAD.

2. Runner/authority guarantees remain outside this slice. `cli.rs:59` checks
   a completed capture, not streaming allocation, dispatch provenance or
   cancellation. `tests.rs:592` uses `Command::output`, and `tests.rs:652` uses
   a simple filesystem fixture reader, not production remote I/O. These tests
   cannot prove bounded capture, pipe draining, process cleanup, remote leaf
   type/containment, authenticated SSH, or atomic repository authority. No
   adapter/SSH files were edited. Main/Turing must implement and verify these
   contracts in their owned layers; fake/captured-output tests are not native
   or real-SSH evidence.

## API Audit And Runner Obligations

- `GitCommand`, `CapturedOutput`, `ObjectId`, `GitRef`, path validators and all
  plan builders remain transport-free; production code adds no process,
  filesystem, GPUI, SSH or credential dependency. No manifests/locks changed.
- Authority probes are three separately bounded single-path/LF outputs.
  Embedded path newlines survive intact. The adapter must pin the same host,
  epoch and repository across probes and revalidate canonical root/git/common
  directory identities, including configured paths that are not repo roots.
- Porcelain v2 preserves partial index/worktree changes, rename source paths,
  conflicts, untracked/ignored policy and explicit unborn/detached state.
  Untracked directory records remain separate from regular-file changes and
  must contribute to the UI's untracked/dirty projection without file actions.
  Invalid UTF-8 and incomplete output fail closed. Keep last-known data marked
  unavailable on failure, not an empty/clean repository.
- Ref selection resolves only the viewed history tip. `log_plan` retains
  `--date-order`, all parent tips, explicit empty-tip termination and length
  framing. The consumer still owns cross-page deduplication and its saved
  frontier. No naive offset or first-parent-only replacement was introduced.
  [Git ordering](https://git-scm.com/docs/git-log) matches the intended
  child-before-parent, timestamp-ordered walk; the new skew fixture compares
  production plans with the existing local DTO implementation.
- Commit files/diffs use the verified first parent, or None only for a proven
  root. Tree/index lookup checks exact names and successful empty inventory;
  conflict and non-blob states cannot mean missing. Read size before blobs,
  bound both sides while capturing, and reuse `build_diff` for HEAD-vs-working,
  HEAD-vs-index and first-parent-vs-commit comparisons. Working symlinks,
  deletion vs permission failure and parent replacement need host I/O checks.
- Stage/unstage use exact literal selections; All retains repository-wide
  behavior. Unborn unstage only changes the index. Tracked discard restores
  index and worktree; untracked discard is a nonrecursive leaf intent. The
  runner must recheck HEAD/status, trackedness, exact path type and parent
  containment after confirmation and before each step. Literal pathspecs do
  not prevent a selected file from becoming a directory.
- Worktree add keeps validated local branch input, absolute destination and
  option separation; removal keeps main/root exclusions and explicit Force.
  Main/linked identity and branch ownership require current inventory, not
  string equality alone. Existing-branch disappearance can trigger Git's own
  DWIM behavior: verify exact branch/HEAD/root/common-directory postconditions
  before registration. Prune cannot authorize config cleanup without current
  authoritative remote absence. No UI dialog/registration/PTY behavior was
  reviewed or changed here.
- A plan is not a repository transaction. Preserve source-owned write locks,
  exact request/dialog/epoch fences, drafts, and uncertain effects. Never
  replay a possibly dispatched mutation or claim rollback of hooks; reconcile
  original-source status, refs and inventory even after nonzero/late results.

## Tests And Remaining Gates

- Source now contains 35 tests: 23 pure tests and 12 Unix disposable real-Git
  fixtures. Thirteen tests were added in this review/followup; existing tests
  were also extended. All are UNRUN, including the original 22.
- New real-Git fixtures cover qualified legacy refs, commit-hook pathspec
  behavior, full SHA-256 status/history/blob reads, and clock-skewed merge
  ordering/all-parent continuation, and directory status beside modified parent
  files with unchanged explicit Stage All behavior. Existing fixtures cover hostile literal
  filenames, partial staging, unborn/reset/discard, root/rename/conflict and
  binary/size cases, worktree create/remove/prune, and local bare-remote sync.
- Main must run the existing Actions workflows for the exact product commit:
  compilation/type checks, Clippy/lint, changed-line rustfmt and whitespace,
  these domain tests/fixtures, full relevant regressions, and Windows checks.
  Apply Actions-generated formatting/diagnostic patches without local tools.
- Adapter gates still need real bounded capture (both streams, truncation at
  record boundaries, timeouts/descendant pipes), missing Git/capabilities,
  not-dispatched vs uncertain effects, concurrent writes and external locks,
  same-path Local/SSH isolation, project/repo A-B-A, epoch replacement,
  close/reopen/cancel, late results, working symlink/deletion/permission cases,
  and exact worktree branch/registration/Force/dirty-document/PTY/prune checks.
- Require Actions run URL/ID, exact `headSha`, job conclusions and matching
  artifact identity. Windows/native and authenticated remote acceptance use
  those Actions-produced artifacts. Local bare-remote fixtures, fake executor
  assertions and compiler success do not prove authenticated SSH/native parity.
- Main owns spec sync after evidence: document the paired parsers/limits,
  existing-ref vs branch-shorthand distinction, pathspec hook inheritance,
  approved separate directory-status contract and host-runner obligations in the mt-project/app
  contracts. No `.trellis/spec/` file was edited in this review.

## Verification

- Lint: UNRUN, Actions-only.
- TypeCheck: UNRUN, Actions-only.
- Tests: UNRUN, Actions-only; authored cases are not passing evidence.
- Formatting/whitespace/generators/build/package: UNRUN, Actions-only.
- Performed only source/spec/handoff reading, primary Git documentation/source
  review, read-only Git status/diff inspection, and scoped `apply_patch` edits.
- No Cargo metadata/build/test/lint/format, parser fixture/script, automated
  whitespace check, local/remote Git probe or mutation, SSH, app launch, CI
  dispatch, staging, commit, push, workflow or spec edit was performed.
- No SSH/native result or exact-commit Actions evidence is claimed.
