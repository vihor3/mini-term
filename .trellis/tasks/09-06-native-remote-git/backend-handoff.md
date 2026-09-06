# Git Backend Handoff

Source implementation complete and RELEASED for independent backend review.
The API below is implemented, not proposed. All backend/local-helper tests,
fixtures, formatting and builds remain UNRUN and Actions-only. Main's earlier
committed CLI Actions results do not verify this uncommitted adapter slice.

## Actual UI API

All types are in `crate::git_backend`; blocking methods must run in a detached
background worker that owns its request until completion, not on the UI thread.

```rust
GitLifetime::{new, invalidate, is_valid}
GitBackend::connect(ProjectExecutionSnapshot, GitLifetime) -> GitResult<GitBackend>
GitBackend::{snapshot, lifetime, matches_snapshot}
GitBackend::discover(&self) -> GitResult<Vec<GitRepository>>
GitRepository::{backend, authority, info, matches_snapshot}
GitRepository::request(&self, GitRead) -> GitResult<GitReadRequest>
GitReadRequest::{id, repository, operation}
GitReadRequest::execute(self) -> GitResult<GitReadResult>
GitReadResult::{id, repository}
GitReadResult::is_current(&self, current: &GitRepository, latest_request: u64) -> bool
// GitReadResult.value: GitReadValue
GitRepository::prepare_write(&self, GitWrite) -> GitResult<PreparedGitWrite>
PreparedGitWrite::{id, repository, operation}
PreparedGitWrite::execute(self) -> GitWriteOutcome
GitRepository::busy(&self) -> Option<GitBusy>
GitRepository::review_uncertain(&self, operation_id: u64, UncertainReview)
    -> GitResult<GitReconciliation>
```

`GitRead`: `Status`, `Branches`, `History { before: Option<ObjectId>,
branch: Option<GitRef>, limit: usize }`, `CommitFiles { commit: ObjectId }`,
`WorkingDiff { path, old_path, staged }`, `CommitDiff { commit, path, old_path }`,
`Worktrees`. Paths are exact `String`; old paths are `Option<String>`.
`GitReadValue`: `Status(cli::RepositoryStatus)`, `Branches(Vec<BranchInfo>)`,
`History(Vec<GitCommitInfo>)`, `CommitFiles(Vec<CommitFileInfo>)`,
`Diff(GitDiffResult)`, `Worktrees(GitWorktrees { scan, entries })`.
Status retains `untracked_directories` as non-file capability information.
Select a history branch with `GitRef::from_branch`; never pass shorthand to
revision commands. `before` continues from all parents, with consumer dedupe.

`GitWrite`: `Stage { paths }`, `Unstage { paths }`, `StageAll`, `UnstageAll`,
`Discard { paths }`, `Commit { message }`, `Pull`, `Push`,
`WorktreeAdd { target, branch: GitRef, create_branch, base: Option<ObjectId> }`,
`WorktreeRemove { target, force }`, `WorktreePrune`. Capture prepared requests
BEFORE confirmation; consume once after confirmation. No branch checkout.

`GitWriteOutcome` fields: `operation_id`, `state` (`NotDispatched`, `Completed`,
`Uncertain`), `error`, `reconciliation`, `reconciliation_error`, `lease_retained`;
methods `repository`, `operation`, `succeeded`, `is_current(current, latest_id)`.
`GitReconciliation` fields: original-source refreshed `repository`, `status`,
`branches`, `worktrees`, `postcondition`. Postconditions are `Refreshed`,
`WorktreeCreated { authority, branch, head }`, `WorktreeRemoved { path }`,
`WorktreesPruned { paths }`. Uncertain outcomes never authorize registration or
destructive finalization, even if a read observed a plausible postcondition.

`GitBusy`: operation ID, phase (`Validating`, `Running`, `Reconciling`,
`Uncertain`), full source signature, project ID and repository authority.
All views/linked trees sharing the host/common directory share this lock.
`UncertainReview::UserConfirmedOriginalOperationStopped` is an explicit user
acknowledgement, NOT an automatic timer/retry. Review re-reads the original
source, and can only release that exact quarantined operation ID.

For publication, check the UI generation, `matches_snapshot` against the active
snapshot, and result `is_current`. Retain the backend's observed SSH epoch from
`snapshot()`; do not compare it to a permanently unobserved `None` epoch. On
source/dialog disposal call `invalidate`, but do not drop a dispatched worker
or release its lease. Errors carry `GitErrorKind` and a bounded message.

## Shared Helper Completed

`execution_host::ProcessTree` and its `configure`, `attach`, `terminate`
methods are now `pub(crate)` in both Unix and Windows branches. No behavior,
output-reader, environment API or ordinary Tasks executor change. Copernicus
can import this helper for its private credential-pipe runner.

## Local Read Compatibility

Main approved the narrow `mt_project::git::local` helper module, now implemented
and consumed. Public helpers: `authority(&Path) -> RepositoryAuthority`,
`head(&Path) -> HeadState`, `status(&Path) -> RepositoryStatus`,
`branches(&Path) -> Vec<BranchInfo>`, `resolve_ref(&Path, &GitRef) -> ObjectId`,
`parents(&Path, &ObjectId) -> Vec<ObjectId>`,
`history(&Path, &[ObjectId], limit) -> Vec<GitCommitInfo>`,
`commit_files(&Path, &ObjectId) -> Vec<CommitFileInfo>`,
`content(&Path, &BlobLookup) -> BlobContent::{Missing, Bytes(Vec<u8>), TooLarge}`,
and `validate_native_repo_path`. All are fallible `anyhow::Result` APIs.

Local read authority/status/refs/history/commit files/blobs no longer require
system Git. Existing libgit2 status projection helpers, history ordering and
commit-file semantics are retained, with strict paths and directory separation.
The same pure diff builder consumes bounded native working bytes and bounded
libgit2 tree/index blobs. ODB header size/type is checked before loading blobs.
Native Windows paths are never passed through a POSIX normalizer. Local
worktree reads retain the existing catalog's authoritative/fallback distinction;
mutations always demand a fresh authoritative CLI inventory. Local writes also
compare CLI authority to libgit2 authority before dispatch, so inherited Git
environment overrides cannot redirect a mutation to another repository.

The pinned libgit2 version has no common-dir accessor: the helper starts at
libgit2's actual repo.path and reads the bounded `commondir` metadata file,
with only genuine absence on a non-linked repository meaning the same git-dir.
Permission/malformed/invalid-UTF8 errors never mean a default common directory.

## Actions Loopback SSH Fixture

Test-only shared read-only helper now exists:
`remote_ssh::loopback_ssh_fixture() -> Result<(SshConnection, PathBuf), String>`.
Pauli/Tasks can reuse it without another fixture framework. It accepts ONLY
`127.0.0.1`, an unprivileged port, key under an isolated `RUNNER_TEMP` directory,
`GITHUB_ACTIONS=true`, and client `HOME=<fixture>/client-home`. It does not start
sshd, change environment or create keys. Main owns lifecycle/setup in Actions.

Auth transport tests (authored, UNRUN, ignored by default, Linux):
`git_backend::tests::actions_fixtures::actual_loopback_ssh_git_backend_reads_writes_epoch_and_sftp_containment`.
This exercises the production pooled authenticated SSH exec, SFTP bytes and
containment, domain argv/parsers, stage/unstage/discard/commit, pull/push to a
private bare fixture, worktree add/remove/prune, and stale epoch refusal.
`git_backend::tests::actions_fixtures::actual_loopback_ssh_dispatched_timeout_keeps_ownership_until_explicit_review`
uses a private failing pre-commit hook, a test-only shortened command deadline,
and the fixture Git PID to require process exit before acknowledging review.
It asserts retained ownership after timeout and exact-ID explicit release.
Fake-host tests are NOT authenticated SSH evidence.

Source-reviewed main's `.github/workflows/ci.yml` loopback step: both exact Git
filters exist, alongside the Tasks fixture. Its `MT_TEST_SSH_ROOT`, `_PORT`,
`_USER`, `_KEY`, marker bytes, isolated client HOME, preserved Cargo/rustup
locations, loopback sshd/SFTP, early cleanup trap, five-minute command deadlines
and fifteen-minute step deadline match this helper. No workflow edit made here.
Fixture bin-first PATH and synthetic GH variables do not change the adapter's
production argv/environment behavior. One setup risk to check in Actions:
`UsePAM no` may reject a password-locked runner account before public-key auth;
main owns the fixture-account/PAM decision. No authenticated result is claimed.

No user authorized_keys/config are changed; both keys and known_hosts stay in
the disposable directory. OpenSSH `SetEnv` overrides the session HOME, keeping
Git host config isolated ([official sshd_config](https://man.openbsd.org/sshd_config.5#SetEnv)).
Only public keys/config may be retained as debug artifacts; do not upload the
fixture directory or private keys. Main must still arrange normal native app
build dependencies and an outer Actions job timeout.

## Lifetime Contract

- `GitLifetime`: cloneable source/dialog lifetime; `invalidate()` prevents
  queued work and stale publication. It never releases a dispatched write.
- `GitBackend::connect(snapshot, lifetime)`: blocking background-only read
  readiness. May establish a fresh SSH epoch, returning an immutable pinned
  backend. Never retags an existing mutation to a replacement connection.
- `GitBackend::discover()`: bounded root/ancestor (five parents), otherwise
  descendant (five levels, existing exclusions) discovery. Returns verified
  `GitRepository` handles, not sibling worktree injection.
- `GitBackend::matches_snapshot(current)`: exact project/source/host/worktree/
  fingerprint/epoch comparison for publication, in addition to the UI's own
  request/generation fence.
- `GitRepository::request(GitRead) -> GitReadRequest`; the request captures its
  ID, source, canonical repository identity and complete read parameters.
  `GitReadRequest::execute()` returns an owned result whose `is_current` checks
  the latest request ID, current repository and source lifetime before publish.
- Reads cover status, refs, parent-based history, commit files, working/index
  and historical diffs, and worktree inventory, using existing DTOs.
- `GitRepository::prepare_write(GitWrite) -> PreparedGitWrite`: blocking
  background-only preflight, freezing source, intent, HEAD/index/status and
  affected-file facts. Capture this BEFORE destructive confirmation.
- `PreparedGitWrite::execute(self) -> GitWriteOutcome`: one dispatch attempt,
  exact-source revalidation, a source-owned lease and original-source
  reconciliation after possible effects. Consume the prepared request once.
- `GitRepository::busy()` exposes the common-directory coordinator; a view
  switch, another linked worktree, or SSH epoch replacement cannot bypass it.
- Uncertain exec/file deletion retains a quarantined lease, not a spinner-only
  flag. The explicit review API above checks phase and exact operation ID.
  Read-only reconciliation does not prove a remote process
  has stopped and never automatically replays or rolls back a write.

## Ownership And Dependencies

The initial scope covered new `git_backend` modules, new `remote_ssh/git_ops.rs`,
and narrow wiring. Main subsequently approved the new local domain helper and
the isolated CLI history-body parity correction below. No Git UI edits. Local
read DTO behavior remains explicitly dispatched to libgit2 helpers;
WSL/SSH always execute on their captured host. The ordinary Tasks executor is
not changed. Its bounded local process runner can be reused conservatively:
generic errors cannot be treated as proof that a write never started.

Remote directory discovery consumes Raman's existing epoch-pinned
`browse_directory(RemoteProjectContext, path)` output and skip symlink children.
The shared `ensure_operation_session` helper is already `pub(super)`, so no
project_ops edit is needed. SFTP provides raw bounded regular-file reads and
exact NoSuchFile classification; Git-specific leaf containment checks must
preserve colon/backslash/newline names (the existing editor name validator is
intentionally stricter and cannot round-trip all Git filenames).

The host adapters capture raw index inventory and read-only working-file digests
for stale destructive confirmation checks, not only XY status labels. These
stay narrowly in the adapter. `hash-object --no-filters` has no `-w`, so it does
not write objects or invoke input filters ([official Git documentation](https://git-scm.com/docs/git-hash-object)).
No dependency manifest changes were made.

UI/store owners still own dialog lifetime, dirty-document/PTY checks, project
registration/activation and authoritative cleanup. Creation/removal results
must carry verified postconditions separately from uncertain outcomes.

## Source Removal And Errors

Removing the source worktree runs from a preverified surviving worktree sharing
the captured common directory. If removal destroys the original source path,
reconciliation uses that captured inventory's surviving worktree, retaining the
original mutation owner. This recovery-only repository handle cannot create
new requests/writes or match an active snapshot. Its facts and verified removal
postcondition are for UI/store finalization only, not an implicit context switch.
Native creation/removal targets are canonicalized before the prepared request
is returned; show/confirm `PreparedGitWrite::operation()`'s captured target.

`GitErrorKind`: Invalid, Unavailable, Unsupported, Permission, Stale, Busy,
Changed, Limit, Command. Messages are capped at 4 KiB. Raw Git stderr is not
rendered or retained in public write outcomes. SSH/SFTP failures preserve their
nonsecret diagnostic text; their available transport API does not provide a
structured permission subtype, so those use Unavailable rather than guessing
from localized text. Missing/unsupported commands, truncated/malformed output,
offline/permission failures cannot produce a clean repository.

Each command obeys domain stdout/stderr/deadline bounds. Discovery and each
confirmation scan have a 120-second budget checked between bounded operations,
20,000-entry bounds and 4 MiB baseline limits. Working reads cap at 1 MiB plus
one byte; both diff blob sides check size first. SFTP opening/read operations
are bounded, close is best effort for two seconds. Existing SFTP `read_dir`
materializes a directory before its count limit can be checked: this adapter
does not claim a streaming memory bound for that shared API. Native libgit2
status/index APIs likewise materialize their own inventory before adapter
result limits; native filesystem/kernel blocking is not preemptible here.

## Changed Paths

- `crates/mt-app/src/git_backend.rs`: source/read API, discovery, local/host dispatch.
- `crates/mt-app/src/git_backend/host.rs`: bounded raw Local/WSL/SSH adapter.
- `crates/mt-app/src/git_backend/write.rs`: all intents, baselines, coordinator, reconciliation.
- `crates/mt-app/src/git_backend/tests.rs`: deterministic and disposable fixtures.
- `crates/mt-app/src/remote_ssh/git_ops.rs`: epoch-pinned Git/SFTP helpers and shared test fixture configuration.
- `crates/mt-app/src/execution_host.rs`: only ProcessTree type/method visibility.
- `crates/mt-app/src/main.rs`: only `mod git_backend;` from this slice.
- `crates/mt-app/src/remote_ssh/mod.rs`: only Git module declaration/glob export from this slice.
- `crates/mt-project/src/git.rs`: only the new `pub mod local;` declaration/docs from this slice.
- `crates/mt-project/src/git/local.rs`: narrow native libgit2 helpers and focused tests.
- `crates/mt-project/src/git/cli/parse.rs`, `cli/tests.rs`: separately stageable body-parity fix below.
- `.trellis/tasks/09-06-native-remote-git/backend-handoff.md`: this handoff.

Other owners' edits in shared wiring files were preserved. No manifest,
lockfile, store, UI, Files helper, workflow, specification or Git mutation.

## Tests Authored, All UNRUN

`git_backend::tests`:
- `source_lifetime_and_request_id_fence_aba_publication`
- `queued_write_rejects_same_status_changed_bytes_without_dispatch`
- `queued_source_cancellation_and_changed_common_directory_prevent_dispatch`
- `uncertain_write_outlives_view_and_only_its_id_can_release_quarantine`
- `known_nonzero_reconciles_original_source_and_releases_its_lease`
- `malformed_capture_is_an_error_not_a_clean_repository`
- `bounded_capture_rejects_both_streams_and_unconfirmed_completion`

`git_backend::write::tests`:
- `confirmation_index_framing_rejects_empty_duplicate_mixed_and_partial_records`

`git_backend::tests::actions_fixtures` (Unix, private disposable repositories):
- `actual_native_read_adapter_never_requires_a_host_git_command`
- `actual_local_unborn_literal_stage_unstage_discard_and_diff_parity`
- `actual_local_deleted_binary_limits_and_symlink_parent_are_explicit`
- `actual_worktree_postconditions_and_discovery_do_not_inject_linked_siblings`
- `actual_removing_the_source_worktree_uses_a_verified_survivor_for_reconciliation`
- The two ignored authenticated Linux loopback fixtures listed above.

`mt_project::git::local::tests` (libgit2-only setup and reads):
- `native_authority_unborn_and_qualified_history_need_no_git_process`
- `native_blob_bounds_are_checked_for_old_and_index_objects`
- `native_status_rejects_non_utf8_and_keeps_embedded_directories_separate`

## Isolated CLI Parity Fix

Main reported Actions 33998882225: the committed CLI history parity fixture
compared `root body\n` against libgit2's `root body`. The standalone fix touches
ONLY `git/cli/parse.rs` and `git/cli/tests.rs`, so main can stage those without
the adapter/local-helper slice. It trims body boundary ASCII whitespace after
strict framing/UTF-8 validation, retaining interior whitespace and non-ASCII
whitespace, matching [libgit2 commit_body](https://github.com/libgit2/libgit2/blob/v1.8.1/src/libgit2/commit.c#L619).
The full serialized DTO parity assertion is unchanged; two explicit trailing-LF
expectations are updated to the intended existing local semantics.

Added UNRUN regressions:
- `git::cli::tests::history_body_matches_libgit2_ascii_boundary_whitespace_semantics`
- `git::cli::tests::actions_fixtures::actual_history_body_boundary_whitespace_matches_existing_local_dtos`

## Limits And Pending UI Work

No source implementation is waiting on another owner. Independent backend +
new-local-helper review and Actions compilation/format/tests remain required.
Native Windows runtime and actual WSL transport are not covered by the Unix
fixtures. Real SSH fixtures exist but have not run, including timeout cleanup.
Discovery depth/exclusion permutations, permission/replacement races and SSH
server death during SFTP unlink need broader runner coverage.

WSL filesystem inspection requires Python 3.10+ in that selected distribution;
SSH symlink payload reads require `readlink -n --`. Missing helpers are explicit
capability failures, never local fallback. Per-file actions refuse directories,
gitlinks/submodules requiring directory operations, and nonregular nodes.
Untracked embedded directories remain read-only rows; StageAll retains its
explicit repository-wide Git behavior. Forced removal captures tracked,
untracked and ignored file guards but refuses nested untracked repositories and
nonregular directory entries. Removing the last non-bare linked tree of a bare
repository is explicitly unsupported without a surviving non-bare executor.
Legacy branch names that cannot be used safely as worktree shorthand retain
the domain's explicit capability error; qualified history reads still work.

SFTP does not supply atomic directory-fd Git execution/unlink. Canonical parent
and leaf-type checks, byte/index baselines, pinned epoch and postchecks reject
observed replacements, but a same-account concurrent filesystem writer can
race between operations. This is not an atomic transaction or an inode lock.
After timeout/transport loss even a plausible read postcondition cannot prove
a remote process stopped; its coordinator lease remains quarantined. Local
executor cleanup errors/timeouts are treated conservatively the same way.
There is no automatic retry, rollback, expiry release or persistent-across-app-
restart operation journal in this slice.

Kepler/UI and store owners still wire all views/actions, branch history filters,
directory capability rendering, dialog confirmations, generation/lifetime
invalidation, commit-draft retention, source-owned refresh, worktree registration,
PTY/dirty-document safeguards and destructive finalization. Backend outcomes
must not be treated as permission to bypass those existing contracts.
