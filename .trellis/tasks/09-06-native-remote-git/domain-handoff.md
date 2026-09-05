# Remote Git Domain Handoff

Domain source implementation is complete, with verification UNRUN. The early
contract above the detailed signatures below was written before implementation
to unblock the app owner. Main owns integration, Actions, generated formatting
patches, specs and acceptance; this handoff is not passing CI evidence.

## Boundary

- Public module: `mt_project::git::cli`, implemented under `src/git/`.
- `GitCommand { program, args, timeout, stdout_limit, stderr_limit, effect }`
  converts directly to the app's `CommandPlan::new(command.program,
  command.args.clone())`. No cwd, transport, SSH, GUI, process execution, or
  credentials live in this API. Run in the captured repository root.
- `CapturedOutput` and `GitCommand::checked_stdout` reject timeout, missing
  status, either truncated stream, over-limit output, and nonzero exit before
  parsing. They deliberately do not classify mutation dispatch uncertainty.
- Public validated `ObjectId` (full SHA-1/SHA-256) and `GitRef` (fully qualified
  local/remote branch only); paths remain exact UTF-8 POSIX spelling. Invalid
  UTF-8 filenames fail the entire capture, never produce lossy mutation paths.

## Implemented API Families

- `repository_authority_plans` / `parse_repository_authority`: three separate
  root/git-dir/common-dir probes so embedded newlines cannot split authority
  fields. Discovery depth, exclusions, root classification and host authority
  remain runner responsibilities.
- `status_plan` / `parse_status`: porcelain v2, branch HEAD/unborn state and
  existing `Vec<ChangeFileStatus>`, separate index/worktree/conflict status.
- `branches_plan` / `parse_branches`, `resolve_ref_plan` / `parse_object_id`:
  existing `BranchInfo`; selected branch resolves only a history tip.
- `log_plan` / `parse_log`: existing `GitCommitInfo`; provide every parent of
  the last commit as continuation tips. Empty tips produce no command, not an
  implicit HEAD walk. Consumer retains graph/page deduplication. The format is
  Git's `--log-size` byte-length frame around six NUL-separated fields, with
  `-z` record termination; never split a capture on newlines or lossily decode it.
- `commit_parents_plan` / `parse_commit_parents`, `commit_files_plan` /
  `parse_commit_files`: first parent, or root empty-tree diff; rename old path.
- `tree_entry_plan` / `parse_tree_entry`, `index_entry_plan` /
  `parse_index_entry`, `blob_size_plan` / `parse_blob_size`, `blob_plan`:
  bounded exact object lookup; only successful empty tree/index inventory
  proves a missing blob. Conflict stages and non-blob entries are explicit.
- `working_diff_plan`, `commit_diff_plan`: `DiffPlan` with old/new
  `BlobLookup::{Empty, Tree, Index, Worktree}`. Working diffs use HEAD versus
  working bytes, staged diffs HEAD versus index. `build_diff(DiffContent,
  DiffContent)` bounds both sides and delegates to the existing pure builder.
- `stage_plan`, `stage_all_plan`, `unstage_plan`, `unstage_all_plan`:
  literal paths, deletions, repository-wide All, explicit unborn HEAD.
- `discard_plan`: tracked restore affects both index and worktree; untracked
  deletion is an explicit nonrecursive host file-removal intent. The runner
  must recheck current status/HEAD, exact path type/identity and confirmation.
- `commit_plan`, `pull_plan`, `push_plan`: host configuration, hooks and
  credentials unchanged; bounded waits and no retries after possible dispatch.
- `worktree_list_plan` / `parse_worktrees` / `project_worktrees`: delegate to
  the existing POSIX porcelain parser, adding complete `NUL NUL` framing,
  bounds and path/ref/OID validation. `project_worktrees` reuses the legacy
  `WorktreeInfo` projection but keeps POSIX names/paths on Windows. Do not use
  native `project_worktree_scan` to round-trip a remote mutation target;
  `worktree_add_plan`, `worktree_remove_plan`, `worktree_prune_plan`: exact
  destinations, validated branches/base OIDs, explicit Force. Catalog,
  dialog/registration ownership, dirty documents and PTY safety stay in app.

## Adapter Requirements

Preserve raw stdout bytes and completeness flags through execution. Keep the
captured epoch, canonical root/common directory and write coordinator outside
this domain layer. Revalidate between reads and before each write; a plan is
not atomic authority or an automatic retry policy. Pathspec literalness does
not prove that a file has not been replaced by a directory or symlink parent.

All verification is UNRUN and Actions-only. No dependency/API from another
crate is required, and no manifest changes are planned.

## Exact Public API

All fallible functions below return `anyhow::Result`; `GitCommand` has no runner.
Argument/output details are intentionally small enough to adapt without adding
an `mt-github` dependency to `mt-project`.

```rust
// mt_project::git::cli
ObjectId::parse(value: &str) -> Result<ObjectId>
ObjectId::as_str(&self) -> &str
GitRef::parse(value: &str) -> Result<GitRef> // refs/heads/... or refs/remotes/...
GitRef::local(name: &str) -> Result<GitRef>
GitRef::remote(name: &str) -> Result<GitRef>
GitRef::from_branch(branch: &BranchInfo) -> Result<GitRef>
GitRef::{as_str, short_name}(&self) -> &str
GitRef::is_remote(&self) -> bool
validate_repo_path(path: &str) -> Result<()>

GitCommand::checked_stdout<'a>(&self, output: CapturedOutput<'a>) -> Result<&'a [u8]>
// CapturedOutput: stdout/stderr: &'a [u8], exit_code: Option<i32>,
// timed_out, stdout_truncated, stderr_truncated: bool
// GitCommand: program: &'static str, args: Vec<String>, timeout: Duration,
// stdout_limit, stderr_limit: usize, effect: CommandEffect::{ReadOnly, Mutation}

repository_authority_plans() -> [GitCommand; 3] // root, git-dir, common-dir order
parse_repository_authority(root: &[u8], git_dir: &[u8], common_dir: &[u8])
    -> Result<RepositoryAuthority>
// RepositoryAuthority: worktree_root, git_dir, common_dir: String
RepositoryAuthority::is_linked_worktree(&self) -> bool

status_plan() -> GitCommand
parse_status(bytes: &[u8]) -> Result<RepositoryStatus>
// RepositoryStatus: head: HeadState, changes: Vec<ChangeFileStatus>
// HeadState: oid: Option<ObjectId>, branch: Option<GitRef>
// oid=None is explicit unborn; branch=None is detached, never both None.
branches_plan() -> GitCommand
parse_branches(bytes: &[u8], head: Option<&ObjectId>) -> Result<Vec<BranchInfo>>
resolve_ref_plan(reference: &GitRef) -> GitCommand
parse_object_id(bytes: &[u8]) -> Result<ObjectId>

log_plan(tips: &[ObjectId], limit: usize) -> Result<Option<GitCommand>>
parse_log(bytes: &[u8]) -> Result<Vec<GitCommitInfo>>
commit_parents_plan(commit: &ObjectId) -> GitCommand
parse_commit_parents(bytes: &[u8], commit: &ObjectId) -> Result<Vec<ObjectId>>
commit_files_plan(commit: &ObjectId, first_parent: Option<&ObjectId>) -> GitCommand
parse_commit_files(bytes: &[u8]) -> Result<Vec<CommitFileInfo>>

tree_entry_plan(commit: &ObjectId, path: &str) -> Result<GitCommand>
parse_tree_entry(bytes: &[u8], expected_path: &str) -> Result<Option<TreeEntry>>
// TreeEntry: oid: ObjectId, mode: u32, kind: EntryKind, size: Option<u64>
// EntryKind::{Blob, Tree, Commit}; non-blob size is None, NOT zero.
index_entry_plan(path: &str) -> Result<GitCommand>
parse_index_entry(bytes: &[u8], expected_path: &str) -> Result<Option<IndexEntry>>
// IndexEntry: oid: ObjectId, mode: u32; conflict/non-blob is an error.
blob_size_plan(oid: &ObjectId) -> GitCommand
parse_blob_size(bytes: &[u8]) -> Result<u64>
blob_plan(oid: &ObjectId) -> GitCommand

working_diff_plan(head: Option<&ObjectId>, path: &str, old_path: Option<&str>, staged: bool)
    -> Result<DiffPlan>
commit_diff_plan(commit: &ObjectId, first_parent: Option<&ObjectId>, path: &str,
    old_path: Option<&str>) -> Result<DiffPlan>
// DiffPlan: old, new: BlobLookup::{Empty, Tree { commit, path },
// Index { path }, Worktree { path }}
build_diff(old: DiffContent<'_>, new: DiffContent<'_>) -> GitDiffResult
// DiffContent::{Missing, Bytes(&[u8]), TooLarge}

stage_plan(paths: &[String]) -> Result<GitCommand>
stage_all_plan() -> GitCommand
unstage_plan(head: Option<&ObjectId>, paths: &[String]) -> Result<GitCommand>
unstage_all_plan(head: Option<&ObjectId>) -> GitCommand
discard_plan(path: &str, head: Option<&ObjectId>, tracked: bool) -> Result<DiscardPlan>
// DiscardPlan::{Git(GitCommand), RemoveUntrackedFile { path: String }}
commit_plan(message: &str) -> Result<GitCommand>
pull_plan() -> GitCommand
push_plan() -> GitCommand

worktree_list_plan() -> GitCommand
parse_worktrees(bytes: &[u8]) -> Result<Vec<WorktreeFact>>
project_worktrees(facts: &[WorktreeFact]) -> Result<Vec<WorktreeInfo>>
worktree_add_plan(target: &str, branch: &GitRef, create_branch: bool,
    base: Option<&ObjectId>) -> Result<GitCommand>
worktree_remove_plan(target: &str, main_worktree: &str, force: bool) -> Result<GitCommand>
worktree_prune_plan() -> GitCommand
```

## Integration Sequence And Limits

1. Read root/git-dir/common-dir with the three authority plans on one captured
   host/epoch. Their output is one raw path plus its terminal LF each. They
   reject bare/no-repository failures, invalid UTF-8 and noncanonical POSIX
   spellings. Discovery depth, exclusions, path replacement checks and
   containing-root classification still belong to the host adapter.
2. Read status for captured HEAD, including unborn/detached. Resolve a viewed
   branch from `BranchInfo.commit_hash` or `resolve_ref_plan`; do not let the
   view branch affect commit, pull or push. For pagination parse every saved
   `parent_hashes` entry with `ObjectId::parse`, pass all tips to `log_plan`, and
   retain the existing consumer's deduplication.
3. For a selected historical commit, read/verify its parents (or use the
   captured parsed commit DTO). Only the first parent goes to commit-file and
   commit-diff plans; `None` means a VERIFIED root, not a failed parent read.
4. Resolve a tree lookup with `tree_entry_plan`; empty successful inventory
   means Missing. Require `EntryKind::Blob`; Tree/Commit is explicitly not text.
   An index lookup similarly returns a stage-zero blob or an error for conflict
   stages/gitlinks. Use tree size or `blob_size_plan` BEFORE `blob_plan`.
5. Do not fetch a side larger than `MAX_BLOB_BYTES`; supply `TooLarge` to
   `build_diff`. Both sides are checked again by the builder. Working bytes
   require the host filesystem service, including deletion/permission/symlink
   distinctions. A missing side must have affirmative absence evidence, never
   an arbitrary SSH, permission, unsupported-command or decoding error.
6. Stage All stages the whole repository, including tracked deletions and
   excluding ignored files. Unstage All uses `reset --mixed HEAD --`, preserving
   local merge-state cleanup, or `read-tree --empty` for a proven unborn HEAD.
   Per-file unstage only resets the named index paths. Tracked discard restores
   HEAD into both index and worktree; unborn tracked additions use nonrecursive
   `git rm --force`. Untracked discard delegates exact nonrecursive leaf removal
   to the host service. Determine `tracked` from index/status facts, NOT Added
   display labels: an unborn untracked file is also displayed as Added.
7. Ref validation does not replace live branch existence/ownership checks.
   Worktree creation requires a local branch; resolve any selected remote base
   to an ObjectId first. Keep exact canonical inventory as authority; the DTO
   projection is presentation, not remote absence evidence. Nonzero/uncertain
   worktree mutations require reconciliation before registration or cleanup.

Limits are explicit failures: 4 MiB list captures, 20,000 file/ref records,
100 commits per page and 100 parent tips, 1,024 worktrees, 16 KiB path/ref names,
128 KiB aggregate file argv, 64 KiB commit messages and diagnostics, and 1 MiB
per diff side. Reads/index writes/sync/prune use 30 seconds, commit/removal use
60 seconds, and worktree creation uses 120 seconds. Runners must enforce these
while capturing, not just after allocation, and close/null stdin. Unknown
status record/code, bad frame, invalid UTF-8, and unsupported mode are errors.
Git capability failures are errors, never an empty repository or local fallback.

All CLI pathspec commands use `--literal-pathspecs` and option separators.
That does NOT establish host path containment or stop a previously selected
file from becoming a directory; execution must recheck type/identity/parents.
No plan overrides hooks, signing, credentials, user configuration or Tasks
account, and no new branch-checkout UI operation was added.

## Changed Paths

- `crates/mt-project/src/git.rs`: only the `pub mod cli` declaration/docs.
- `crates/mt-project/src/git/cli.rs`: pure plans, validation, bounded capture,
  existing-diff adapter, POSIX worktree DTO projection.
- `crates/mt-project/src/git/cli/parse.rs`: paired strict machine parsers; reuses
  the existing worktree parser rather than defining another porcelain format.
- `crates/mt-project/src/git/cli/tests.rs`: pure tests and disposable Unix
  runner fixtures. Process/filesystem work exists only inside this test module.
- `.trellis/tasks/09-06-native-remote-git/domain-handoff.md`: this report.

No other product crate, dependency manifest/lockfile, spec, workflow or task
metadata was edited. Existing local implementations and their tests are intact.

## Tests Written, All UNRUN

Pure tests under `mt_project::git::cli::tests`:

- `object_ids_and_branch_refs_reject_revision_and_option_injection`
- `path_plans_preserve_literal_bytes_and_reject_escape_or_empty_selection`
- `capture_rejects_even_record_boundary_truncation_and_unconfirmed_exit`
- `authority_paths_are_independently_framed_and_never_normalized_lossily`
- `status_preserves_partial_index_rename_conflict_untracked_and_ignored_semantics`
- `status_distinguishes_unborn_detached_sha256_and_intent_to_add`
- `status_rejects_malformed_truncated_non_utf8_and_duplicate_records`
- `branch_parser_retains_local_head_parity_and_excludes_remote_head`
- `history_has_length_framing_all_parents_and_no_implicit_head_at_end`
- `history_rejects_partial_payloads_and_embedded_record_injection`
- `commit_files_and_exact_object_lookups_preserve_paths_and_missing_sides`
- `diff_plans_compare_both_modes_to_head_and_commit_to_first_parent`
- `byte_diff_reuses_text_builder_and_bounds_both_sides`
- `mutations_have_explicit_unborn_discard_and_host_configuration_semantics`
- `worktree_plans_reuse_posix_inventory_and_exclude_main_removal`

Disposable Git fixtures under `git::cli::tests::actions_fixtures` (`cfg(unix)`):

- `actual_plans_preserve_unborn_hostile_filenames_and_index_only_unstage`
- `actual_partial_status_diff_discard_and_index_lock_match_safety_contract`
- `actual_history_filters_without_checkout_and_continues_from_both_merge_parents`
- `actual_rename_binary_oversized_and_conflicted_entries_are_explicit`
- `actual_worktree_authority_create_remove_force_and_prune_plans`
- `actual_pull_push_plans_use_fixture_host_git_configuration`
- `actual_non_utf8_paths_and_symlink_blobs_do_not_alias_files`

Fixtures execute the actual public argv and parse the captured bytes in private
temporary directories with isolated Git identity/config. They compare ordinary
status, staged/working/renamed diffs, and branch-filtered history to existing
local implementations. Pull/push use only a disposable local bare remote, not
the user's repositories or a live SSH host.

## Verification And Gaps

Only source/spec reading, official Git documentation/source review, and
read-only Git status/diff inspection were performed. No Cargo metadata/build,
test, fixture, formatter, lint, whitespace check, generator, probe, app launch,
SSH execution, Git mutation, staging, commit, push or workflow dispatch ran.
Main must request the exact-commit Actions gate and apply any generated rustfmt
or diagnostic patches. The source has not been compiled.

Remaining coverage belongs to integration/Actions: real authenticated SSH
capture and epoch replacement, timeout and uncertain dispatch, source write
coordination, A-to-B-to-A/dialog ownership, host working-file read/removal
semantics, remote Git capability failures, dirty-document/PTY registration
guards, authoritative prune cleanup and matching artifact acceptance. Unix
fixtures do not validate Windows process transport; the platform-independent
POSIX projection test must also compile/run in the Windows gate. Real SHA-256
repositories, sparse/partial clones, graft/replace refs, unusual encodings and
external mutation races are not fully covered by the authored fixtures.

## Primary Format References

- [Git status porcelain v2](https://git-scm.com/docs/git-status)
- [Git log formatting and log-size](https://git-scm.com/docs/git-log)
- [Git log size/terminator implementation](https://github.com/git/git/blob/v2.45.0/log-tree.c)
- [Git ref formatting](https://git-scm.com/docs/git-for-each-ref)
- [Git ref validation](https://git-scm.com/docs/git-check-ref-format)
- [Git canonical path/revision parsing](https://git-scm.com/docs/git-rev-parse)
- [Git tree records](https://git-scm.com/docs/git-ls-tree)
- [Git index records](https://git-scm.com/docs/git-ls-files)
- [Git commit-tree differences](https://git-scm.com/docs/git-diff-tree)
- [Git restore](https://git-scm.com/docs/git-restore)
- [Git literal pathspecs](https://git-scm.com/docs/git)
- [Git worktree](https://git-scm.com/docs/git-worktree)
