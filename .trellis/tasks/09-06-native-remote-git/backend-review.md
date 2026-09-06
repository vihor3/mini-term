# Backend Source Review

Status: bounded SOURCE REVIEW COMPLETE, backend/local-helper writes RELEASED
to main. CLI parity is separately released in `cli-parity-review.md`. No builds,
tests, fixtures, probes, formatting or whitespace checks have run locally. This
is not a CI, native acceptance or full-scope feature pass.

## Follow-Up: Fake Discard HEAD Lookup

Status: bounded fixture correction SOURCE COMPLETE; `git_backend/tests.rs`
RELEASED to main. No production code or public API changed.

Read the authorized [Windows Actions job log](https://github.com/vihor3/mini-term/actions/runs/34005933974/job/101412979902):
the Git filter reports 123 passed / 1 failed. The sole failure is
`git_backend::tests::queued_write_rejects_same_status_changed_bytes_without_dispatch`,
whose FakeHost panics on the exact literal `ls-tree -z -l --full-tree` lookup.

Source trace: `prepare_write` validates authority, captures status/index and
the working-byte hash in `Baseline::capture` / `FileGuard::capture`, then
`plan_steps` checks HEAD with `tree_entry_plan` and `parse_tree_entry` before
deciding between tracked restore and untracked leaf removal. The fake reports
all its files as untracked and an empty index, but omitted the corresponding
HEAD lookup. This is incomplete fake coverage, not a production discard defect.

- `tests.rs:110`: added only that read case. It requires a known fake worktree
  file and full equality with the production plan for fixture HEAD `OID`, then
  returns successful, complete empty bytes. The real parser consequently
  returns no tree entry. Other unexpected reads still panic; no blanket empty
  response or tracked/untracked reclassification was added.
- `tests.rs:233`: retained the exact test name, hostile newline/pathspec-like
  spelling, byte-guard change, NotDispatched, Changed, retained file count and
  released busy-slot assertions. Added explicit before/after porcelain-byte
  equality, proof the HEAD lookup was issued, retained changed file value, and
  a zero-mutation-call assertion. The counter covers both Git mutation plans
  and host leaf removal, so the test does not infer no dispatch from status alone.
- Execution still recaptures the baseline before its first dispatch. With
  unchanged status/index and changed working hash, baseline comparison must
  reject the captured intent; no write plan, lease semantics or parser changed.

This is synthetic executor evidence only, not real SSH/WSL/native Git proof.
Tests / Build / Metadata / Lint / Format / Syntax / Whitespace / Fixtures: UNRUN
for this repair. Require the exact named test and complete Windows/Linux Git
filters in Actions at the new candidate SHA; retain separate actual host gates.
No other source, Agent/CI/spec file, staging, commit or push was touched.

## Follow-Up: Backend Clippy

Status: bounded diagnostic repair SOURCE COMPLETE; `git_backend.rs` and
`git_backend/write.rs` RELEASED to main. No tests or other product files edited.

Read the exact [Actions job log](https://github.com/vihor3/mini-term/actions/runs/34005933974/job/101412980006)
through the approved `gh api` read. It reports 177 baseline warnings ignored and
28 changed-line warnings; this handoff addresses only the owned backend subset.
Main reports `37b4ef9` passed Linux/Windows application and test-target compilation;
that does not validate this subsequent Clippy repair.

- `git_backend.rs:189`: `lifetime()` has only the existing backend regression
  caller (`git_backend/tests.rs:269`), so it is now private and `#[cfg(test)]`.
  The lifetime field and production invalidation/dispatch checks are unchanged.
- `git_backend.rs:845`: removed unused `GitReadRequest::repository()` and
  `GitReadRequest::operation()` after source-wide call-site search. Retained
  request fields, `id()`/`execute()`, and all used result/outcome/prepared-write
  accessors. These are the only removed callable internal APIs; no caller edit
  is required.
- `write.rs:54`, `:75`, `:86`: preserved `GitBusy.source`, WorktreeCreated.head,
  and GitReconciliation.repository/status/branches/worktrees. Each has its own
  documented field-level `allow(dead_code)`: original owner, verified creation
  HEAD, recovery source and same-receipt status/ref/inventory facts must not be
  erased just because current UI refreshes independently or only consumes the
  typed postcondition. No module/struct blanket allowance, fake read, black_box,
  new UI behavior, or fabricated consumer was introduced.
- Six mechanical collapses: `git_backend.rs:199`; `write.rs:187`, `:324`, `:405`,
  `:1088`, `:1282`. Pattern/local-source checks still precede host inspection or
  native target normalization. Target absence still short-circuits the anchor
  probe. Exact-ID checks precede slot update/removal. The explicit review's
  existing `writes` guard and phase update's temporary WRITES guard retain their
  lock scope through mutation; no second lock, replay or automatic lease release.
  Explicit read-only reconnect and recovery-only survivor behavior are unchanged.

Source/diff review only. Build / Metadata / Tests / Fixtures / Syntax / Lint /
Format / Whitespace: UNRUN for this delta. Existing lifetime/uncertain/shared
lease and actual authenticated SSH regressions are retained, not re-executed.
Require exact-head Actions compilation, Clippy changed-line gate, formatting and
backend tests, including the existing ignored SSH fixtures. Other owners retain
all non-backend warnings. No local probes/validation, children, staging, commit
or push; Agent and all unrelated dirty paths remain untouched.

## Follow-Up: Pinned Status API

Status: compiler correction SOURCE COMPLETE; `mt-project/src/git/local.rs`
RELEASED to main. Applied on the existing Actions-formatted source; no formatter
or other local check was run. No dependency, lockfile or public API change.

- Main reports integration `7e60d31`, formatting/i18n follow-up `a566edf` not yet
  pushed, and Actions `34004657463` / Windows `101409559517` failing lib/test
  compilation with E0599 for `Status::WT_UNREADABLE`. That constant is absent
  from the [pinned git2 0.19.0 Status definition](https://github.com/rust-lang/git2-rs/blob/git2-0.19.0/src/lib.rs#L969).
- FIXED at `crates/mt-project/src/git/local.rs:125` and `:154`: use the typed
  workdir delta from `StatusEntry::index_to_workdir()` and reject
  `git2::Delta::Unreadable` before staged/unstaged mapping or clean-entry skip.
  [StatusEntry::status](https://github.com/rust-lang/git2-rs/blob/git2-0.19.0/src/status.rs#L306)
  truncates unknown bits, so testing that returned flag set cannot recover the
  missing unreadable bit. The pinned
  [DiffDelta::status mapping](https://github.com/rust-lang/git2-rs/blob/git2-0.19.0/src/diff.rs#L476)
  exposes Unreadable, and
  [libgit2 1.8.1 workdir_delta2status](https://github.com/libgit2/libgit2/blob/v1.8.1/src/libgit2/status.c#L56)
  derives the unreadable flag from exactly that delta. No guessed raw constant,
  FFI or dependency change is needed. `include_unreadable(true)` remains set;
  unreadable/unsupported working entries still fail closed, never become clean
  or actionable staged-only rows. Existing supported flag conversion is intact.
- Direct production-helper regression at `local.rs:519`:
  `git::local::tests::native_status_flags_reject_unreadable_even_after_truncation`
  rejects Unreadable with CURRENT, staged-only and conflict flags, and preserves
  accepted no-workdir, untracked, partially staged and conflict flags. This is a
  typed-boundary unit test, not executed native filesystem permission evidence.

Build / Metadata / Tests / Fixtures / Syntax / Lint / Format / Whitespace: UNRUN
locally. Require exact-head Actions Windows lib/test compilation, the focused
unit test and existing mt-project regressions, then the blocked mt-app gates.
No UI/Agent/Tasks/CI/spec or other product file was edited; no staging, commit or
push. Main may commit this correction with its formatted follow-up and rerun.

## Follow-Up: Captured Connect Epoch

Status: bounded source correction COMPLETE; `git_backend.rs` and
`git_backend/tests.rs` writes RELEASED again to main. No public API change.

- P1 FIXED, Pauli's `ui-review.md` Backend Boundary:
  `crates/mt-app/src/git_backend.rs:94` now checks the observed readiness epoch
  against the captured pin immediately after readiness returns. A differing
  `Some(epoch)` yields `GitErrorKind::Stale` before assigning a replacement pin,
  constructing `ExecutionGitHost`, or reading the canonical project anchor.
  `None` may still bootstrap; matching `Some` remains accepted. Readiness itself
  may authenticate a session; this is not a claim that it performs no transport
  work. The private `checked_readiness_epoch` helper is the production check.
- `git_backend/tests.rs:110` covers absent, matching and mismatched epochs in
  both directions through that helper. The existing actual ignored SSH fixture
  at `tests.rs:464` now reconnects after epoch replacement, rejects the original
  pinned snapshot, and requires Stale even with a non-directory file anchor
  whose SFTP probe would fail. It also accepts the freshly pinned snapshot with
  unchanged source signature. Existing None bootstrap and queued stale-write
  NotDispatched assertions remain intact; they are not rollback evidence.
- `git_backend/write.rs:591` explicit original-write reconciliation reconnect
  is unchanged, including its separate read-only epoch adoption and immutable
  write receipt. No host framework, UI, store, Files, Tasks, Agent, CI, specs,
  domain/local helper, or other owner's source was edited for this follow-up.

Actions-only, all UNRUN: compile/Clippy/format and tests at the staged head SHA,
including the exact unit filter
`git_backend::tests::readiness_epoch_bootstraps_none_but_never_replaces_a_captured_epoch`
and the existing ignored authenticated filter
`git_backend::tests::actions_fixtures::actual_loopback_ssh_git_backend_reads_writes_epoch_and_sftp_containment`.
Require one discovered/executed test for each applicable filter and normal
loopback cleanup. No workflow change is requested. No local build, metadata,
test, fixture, probe, syntax, lint, formatting, whitespace, app/CI validation,
automation, staging, commit or push was run. This resolves only the reported
backend boundary; the earlier review's remaining native/UI/Actions gates remain.

## Early Findings For Main

- P1: `git_backend/write.rs::WriteKey::of` includes the SSH configuration ID
  even though authenticated `ExecutionHostId` plus canonical common directory
  identifies the shared write authority. Two connection aliases can bypass an
  existing running or uncertain lease. FIXED: use the SSH backend
  class, not configuration ID; retain the immutable original snapshot inside
  the lease for reconciliation. No public API change. Added a production-lease
  regression across aliases, epochs, project/worktree IDs and credentials.
- Main's `UsePAM yes` loopback adjustment is accepted as source-reviewed setup,
  not execution proof. No account unlock, configuration mutation or workflow
  edit is requested from this checker. The four exact ignored SSH tests remain
  Actions-only and unrun for this backend slice.

## File Resolver API For Kepler

SOURCE AVAILABLE, tests UNRUN:

```rust
GitBackend::repository_for_file(&self, project_relative_file: &str)
    -> GitResult<(GitRepository, String)>
```

Implementation: `crates/mt-app/src/git_backend.rs:209`. This is the only public
API addition by the checker; the reviewed Turing APIs otherwise remain intact.
Input uses literal `/`-separated Git relative paths. Native Windows ambiguous
backslash/colon input is rejected; POSIX literal backslashes are preserved.

Blocking, read-only, background-worker API. The string returned with the
repository is the exact repo-relative path for normal `GitRead::WorkingDiff`.
It keeps the existing backend snapshot, lifetime and canonical project anchor.
Nested descendant repositories win even when the project itself is a repo;
missing file parents are skipped on the upward walk. Discovery stops at the
project's fifth ancestor, never scans descendants and never uses a local answer
for WSL/SSH. Literal path validation, no-follow parent checks and authority
revalidation apply. No repository, stale source, directory/special target and
escape attempts are errors. Symlink leaves retain existing literal-link diff
semantics; symlink parents are rejected.

Native authority/status resolution uses libgit2, with a `NoCommands` production
host wrapper in the authored fixture. Tests cover nearest/deleted/ancestor-limit
selection, no repository, invalid paths, lifetime invalidation, same-spelling
fake-remote/native isolation and existing working-versus-HEAD diff DTO parity.
The existing exact ignored authenticated SSH fixture also now resolves a nested
repository through this API. Kepler still owns request generation, snapshot and
publication fencing, and the FileTree wrapper edit. No UI source was changed.

Caller issue for main/Kepler, NOT FIXED in this ownership boundary:
`file_tree/mod.rs:1568` computes `row.rel` with unconditional
`.replace('\\', "/")`; `file_tree/menu.rs:331` forwards it to `open_file_diff`.
For a POSIX filename containing a literal backslash this has already changed
the target before the backend sees it. Preserve exact source-relative POSIX
bytes in the caller, and only convert native Windows separators where that is
actually the source representation. The resolver must not guess or retarget
the altered spelling. Files/UI owners need to coordinate this existing caller
limitation; no FileTree edits were made by this checker.

## Findings (fixed)

- P1, `git_backend/write.rs:669`: removed SSH connection-alias identity from
  the shared common-directory write key. `write.rs:723` exercises the actual
  production lease and `busy()` across alias, epoch, credential, project and
  linked-worktree changes, retaining the original owner's source facts.
- P2, `git/local.rs:261`: native index lookup could select a different-case
  path under `core.ignorecase`. It now selects exact entry bytes, rejects any
  conflict stage/duplicate/non-blob, and returns Missing only for no exact
  entry. Regressions at `local.rs:356` and `:367` use actual libgit2 indexes.
  This follows the pinned implementation's
  [case-dependent index lookup](https://github.com/libgit2/libgit2/blob/v1.8.1/src/libgit2/index.c#L843),
  not an assumption about the client filesystem.
- P2, `git/local.rs:23`: the common-directory metadata reader removed only one
  LF, unlike libgit2's trailing-ASCII-whitespace trim. It now matches
  [lookup_commondir](https://github.com/libgit2/libgit2/blob/v1.8.1/src/libgit2/repository.c),
  while retaining strict UTF-8, NUL rejection and byte bounds. `local.rs:323`
  authors a linked worktree and checks LF, CRLF and ASCII suffix variants.
- P2, `git_backend.rs:201`: detached repository discovery lost the old
  parenthesized seven-character HEAD label. Restored it; `tests.rs:360`
  compares the production adapter to existing local discovery.
- FileTree dependency: implemented the approved nearest-file resolver above.
  `tests.rs:206` covers synthetic nested/deleted/ceiling/no-repo/escape/lifetime
  cases. `tests.rs:310` covers native nested resolution without host commands,
  deleted-parent blobs, same-spelling fake-host isolation and complete existing
  working-versus-HEAD DTO comparison. The authenticated SSH fixture includes
  nested resolution and literal backslash/newline working-file bytes.

## Source Contracts Checked

- Read the current PRD/design/implement/check context, action matrix, domain
  and backend handoffs, package indexes and relevant identity/worktree/SSH
  contracts. Read all new backend, host, write, test, Git SSH and native-helper
  source, plus the existing execution runner, SFTP and local DTO boundaries.
- `git_backend.rs:87`, `:157`, `:337`, `:485`: captured project/root/host/backend/
  worktree/path/fingerprint/epoch, canonical root/git-dir/common-dir, lifetime
  and request identity are preserved and revalidated. Local status/branches/
  history/commit-file/tree/index reads explicitly use libgit2; local worktree
  reads keep the existing catalog/fallback contract. WSL/SSH never fall back.
- History keeps all-parent continuation plus time/topological ordering;
  commit files/diff use first parent or empty root. Working diff remains
  worktree versus HEAD, staged diff index versus HEAD. Both content sides use
  bounded lookups and the existing pure diff builder. The prior CLI review's
  literal pathspec, ref/OID, framing and truncation rules are unchanged.
- `host.rs:342`, `remote_ssh/git_ops.rs:161`: canonical parent and leaf checks,
  strict host paths, real missing versus errors, raw symlink payloads, bounded
  regular reads and explicit nonregular errors. SSH channel readiness and
  operations retain the captured authenticated epoch; dispatch/retirement
  facts survive into the outcome. No credential/environment setting was added
  to production Git execution and Tasks account choice is not consulted.
- `write.rs:99`, `:190`, `:581`: capture before confirmation; consume once;
  revalidate source, CLI/native authority, HEAD/index/status and relevant file
  guards before mutation. StageAll remains repo-wide; selected file plans
  reject directory rows; unborn unstage and tracked index+worktree discard are
  retained. Untracked removal is one nonrecursive leaf, never broad clean.
- `write.rs:216`, `:591`, `:669`: common-dir coordination survives view/epoch
  changes; a possibly dispatched worker owns reconciliation even on error.
  Known failure is distinct from uncertainty. No replay, rollback, timeout
  lease expiry or speculative success. Exact-ID review requires explicit
  acknowledgement that the original operation stopped, then original-source
  reconciliation. A removed source can yield only a recovery-only survivor.
- Worktree targets/branches/base/Force are frozen; destination argv cannot
  inject options. Main/locked/bare exclusions and fresh authoritative inventory
  gate writes. Plausible created/removed/pruned postconditions on Uncertain are
  NOT UI registration, terminal disposal or configuration cleanup authority.
- Shared loopback configuration is test-only: Actions, 127.0.0.1, unprivileged
  port, canonical root below RUNNER_TEMP, isolated client HOME, marker and
  contained key. The helper does not start sshd or mutate account/config state.
  ProcessTree changes remain visibility-only for Unix and Windows.

## Findings (not fixed)

- P2 caller spelling loss at `file_tree/mod.rs:1568` / `menu.rs:331`, described
  above. Outside the released backend ownership boundary; main must coordinate
  Files/UI correction. Backend normalization would retarget, not repair it.
- Compatibility limits in `host.rs:88`, `:367` and `write.rs:500`: WSL requires
  Python 3.10+, SSH symlink bytes require `readlink -n --`, and removing the last
  non-bare linked worktree of a bare repository is refused without a verified
  survivor. These are explicit capability failures, not proof of full approved
  parity. The last case is narrower than the old local CLI removal path and
  needs main's acceptance decision or a separately designed bare-authority
  executor; this review did not bypass the survivor guard.
- Per-file directory/gitlink/special-node operations and Force removal with
  nested untracked repositories remain explicitly refused. No recursive
  untracked discard is introduced. Illegal legacy worktree branch shorthand
  remains the separately reported limitation, with no retargeting.
- Native libgit2 inventories and SFTP read_dir materialize before adapter
  limits; native filesystem blocking is not preemptible. This is not a fully
  streaming bound. Canonical/byte/index checks are not atomic inode authority;
  same-account/external Git races and SFTP v3's no-openat limitation remain.
  There is no persistent operation journal across app restart.
- UI/store acceptance remains main/Kepler-owned: detached worker ownership,
  stale captured modal/draft preservation, dirty documents, exact PTY checks,
  host-qualified registration and destructive cleanup. The backend does not
  authorize bypasses; no UI/store source was edited here.

## Verification

- Lint / Type Check / Tests / Format / Whitespace: UNRUN for this backend delta.
  No local fixture, parser, syntax, probe, build, app validation or Git write.
- Main's prior run 33998882225 / Linux 101394109884 does not cover this slice.
  The separate CLI report preserves its actual full DTO assertion and records
  the main-reported 169 pass / 1 fail result without claiming the fix passed.
- Fake-host and production-lease tests assert ownership/routing, not actual
  SSH or WSL behavior. Libgit2 and Unix CLI fixtures assert native behavior only
  when Actions executes them. The two ignored SSH tests use real production
  pooled exec/SFTP but are still authored, unrun tests, not SSH proof.

## Remaining Actions And Native Gates

1. Main stages the isolated two-file CLI fix and obtains exact-commit Actions
   compile/Clippy/format/tests. Then run the backend/native-helper delta and
   integrated UI against its own exact head SHA, including root and sidecar
   locked graphs and Windows MSVC branches. Do not treat an earlier green job
   or a zero-test filter as current evidence.
2. Current draft Linux setup has UsePAM yes, password/kbd-interactive no,
   contained keys/HOME, early cleanup, 20-minute step and four exact tests:
   `git_backend::tests::actions_fixtures::actual_loopback_ssh_git_backend_reads_writes_epoch_and_sftp_containment`;
   `git_backend::tests::actions_fixtures::actual_loopback_ssh_dispatched_timeout_keeps_ownership_until_explicit_review`;
   `remote_ssh::tasks_accounts::tasks_account_executor_ssh_sentinels_cleanup_and_epoch_pipeline`;
   `remote_ssh::transfer::epoch_tests::actual_loopback_sftp_files_pins_mutations_and_containment`.
   Require one executed test per exact filter and successful fixture cleanup.
   No workflow edit requested here; PAM setup is source evidence only.
3. Additional production-adapter cases still need Actions coverage: isolated
   Local GIT_DIR/GIT_WORK_TREE/GIT_COMMON_DIR retarget attempts; read permission
   failures; changed destination/ignored content between confirmation and Force
   remove; actual SSH disconnect or server death during SFTP unlink and during
   worktree add/remove/prune; bounded/truncated captured output at the transport
   boundary. The authored pre-commit timeout is not arbitrary mid-transport
   proof, and queued stale-epoch NotDispatched is not rollback proof.
4. Retain the domain fixture gates for unusual names, old refs, hostile options
   and pathspecs, unborn/partial-stage/rename/conflict, root/merge history and
   old/new blob limits. No parser assertion should be weakened to obtain green.
5. Native Windows production-host execution and actual selected-distro WSL
   remain unverified; Unix native fixtures and fake WSL tests do not establish
   them. Use Actions fixtures/artifacts for Windows paths, junction/permission
   behavior, worktree targets and real WSL filesystem/Git/helper capabilities.
6. Run native UI acceptance from the matching Actions artifact for source
   switches, confirmation races, uncertain review acknowledgement, FileTree
   nested diffs, draft retention and worktree registration/PTY/document cleanup.

## Release

Checker product edits are limited to `git_backend.rs`, `git_backend/write.rs`,
`git_backend/tests.rs` and `mt-project/src/git/local.rs`. All reviewed backend,
Git SSH, ProcessTree visibility and narrow local export write slots are now
RELEASED. Main may forward the file resolver API to Kepler/Turing, stage/rerun
Actions and reuse the checker slot for R11. No other owner's changes reverted;
no app UI/store/general SSH/manifests/locks/workflows/specs/Git writes performed.
