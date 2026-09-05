# Host-Neutral Git CLI Contract

## 1. Scope / Trigger

Use `mt_project::git::cli` for execution-host Git panel plans and strict decoders.
This domain owns argv, machine formats, literal path/ref/object validation and
shared diff semantics. It does not execute processes or prove host authority.
Existing local libgit2 entry points remain separate compatibility behavior.

## 2. Signatures

```rust
pub struct GitCommand {
    pub program: &'static str,
    pub args: Vec<String>,
    pub timeout: Duration,
    pub stdout_limit: usize,
    pub stderr_limit: usize,
    pub effect: CommandEffect,
}

pub enum CommandEffect { ReadOnly, Mutation }

pub struct RepositoryAuthority {
    pub worktree_root: String,
    pub git_dir: String,
    pub common_dir: String,
}

pub struct RepositoryStatus {
    pub head: HeadState,
    pub changes: Vec<ChangeFileStatus>,
    pub untracked_directories: Vec<String>,
}

GitCommand::checked_stdout(CapturedOutput<'_>) -> Result<&[u8]>;
repository_authority_plans() -> [GitCommand; 3];
parse_repository_authority(root, git_dir, common_dir) -> Result<RepositoryAuthority>;
status_plan() -> GitCommand;
parse_status(bytes) -> Result<RepositoryStatus>;
log_plan(tips: &[ObjectId], limit: usize) -> Result<Option<GitCommand>>;
```

`CapturedOutput` includes both byte streams, optional exit status, timeout and
both truncation flags. `ObjectId` and `GitRef` are validated private newtypes.
Other paired plans/decoders cover refs, commit parents/files, tree/index lookup,
blob size/content, diff inputs, stage/unstage/discard, commit/sync and worktrees.

## 3. Contracts

- Execute structured argv in the captured repository root. Only the transport
  boundary quotes SSH arguments; shell quoting does not validate Git pathspecs
  or revision syntax. A plan is not a write lease or dispatch receipt.
- Call `checked_stdout` before parsing. Require exit zero, complete status and
  streams within both caps. Truncation, timeout, missing status or nonzero exit
  is not a clean/empty repository. These checks do not bound streaming capture;
  the runner must enforce the same limits while draining output and cleaning up.
- Current limits: list bytes 4 MiB, records 20,000, history page 100 commits,
  worktrees 1,024, path/ref bytes 16 KiB, argument bytes 128 KiB. Blob limits
  reuse `MAX_DIFF_BYTES`; inspect object size and cap both sides of every diff.
- Repository authority uses three independent path outputs, each with one final
  LF, not one newline-split tuple. Preserve embedded newlines and exact POSIX
  spelling. Native Windows paths need the explicit local adapter, not this
  remote POSIX parser. The host must revalidate one source/epoch/repository
  across every authority probe.
- Require full nonzero lowercase SHA-1/SHA-256 object identities where public
  object IDs are expected. Zero/unborn markers are accepted only at the paired
  protocol field. A whole capture cannot mix object widths.
- Existing full `refs/heads/` and `refs/remotes/` names are not user branch
  shorthand. Preserve qualified legal legacy names through reads and resolve
  to `ObjectId` before history operations. New-branch/worktree shorthand remains
  stricter; reject unsupported names explicitly instead of retargeting HEAD.
- File paths are exact relative UTF-8, without NUL, empty/dot/dot-dot/.git
  components or normalization. Colon, backslash, glob text and newlines are
  literal filenames. File plans use `--literal-pathspecs` before the subcommand
  and `--` before paths. Do not put this global option on commit/sync/worktree
  plans: it changes inherited Git behavior inside user hooks.
- Porcelain v2 preserves separate index/worktree status, rename source,
  conflicts, submodules, unborn and detached HEAD. Unborn has no object but
  retains branch identity; detached has an object and no branch.
- Valid untracked directory records such as `nested/` are separate exact
  `untracked_directories`, never ordinary file rows. Preserve the trailing slash,
  combined record bounds and duplicate rejection across file/directory entries.
  They do not authorize file diff/stage/discard or recursive deletion. Explicit
  Stage All keeps its existing repository-wide Git effect.
- History uses bounded byte-length/NUL framing and all-parent continuation,
  not line parsing, naive offsets or a first-parent walk. Consumers retain their
  frontier and deduplicate graph pages. Branch choice is a view filter.
- Exact tree/index lookup returns missing only for successful complete empty
  output. Conflicts and non-blob entries are errors, not empty files. Working
  diff compares HEAD to working bytes; staged diff compares HEAD to index.
  Commit diff uses the first parent or a proven root's empty tree.
- Discard returns a tracked restore plan or a nonrecursive untracked-leaf
  removal intent. The adapter must recheck trackedness, HEAD, index, file type,
  parent containment and captured confirmation before dispatch. Never replace
  this with a repository-wide reset/clean.
- Worktree captures require complete NUL framing, HEAD or explicit bare, valid
  object/ref fields, and retained locked/prunable facts. Use the POSIX projection
  for remote paths even on Windows. Plans alone do not prove main-worktree
  exclusion, current branch ownership, destination ownership or registration.
- Host configuration, hooks, signing and Git credentials remain unchanged.
  Tasks account selection supplies no auth environment to this domain. After a
  possible mutation, adapters preserve uncertainty and reconcile the original
  source without automatic replay, rollback claims or view-owned lock release.

## 4. Validation & Error Matrix

| Input / condition | Required result |
| --- | --- |
| Incomplete, truncated, timed-out or failed capture | Error; retain last-known UI data as unavailable |
| Mixed object formats or invalid machine fields | Reject the whole capture |
| Legal existing ref invalid as new shorthand | Reads remain usable; unsupported mutation is explicit |
| Modified parent plus untracked embedded repository | Preserve file changes and separate directory metadata |
| File plan receives trailing-slash directory | Reject; never normalize into a file target |
| Tree/index lookup failed or is conflicted | Error, not missing |
| Missing/binary/oversized diff side | Use explicit shared diff result semantics |
| Empty history frontier | No command and no more commits |
| Worktree creation reply lost or branch changed | Adapter reconciliation; no automatic registration/replay |

## 5. Good / Base / Bad

- Good: stage a literal `:(glob)*.txt` selection while a commit hook retains its
  ordinary Git pathspec semantics.
- Base: a valid empty/unborn repository reports no file changes without inventing
  a HEAD commit; its explicit branch remains available.
- Bad: parse any failure as an empty list, strip an untracked directory slash,
  or treat a shell-safe branch string as proof of exact mutation authority.

## 6. Tests Required

Actions runs pure parser/plan tests plus disposable real-Git fixtures for
hostile filenames, partial staging, unborn/root/conflict/rename, byte limits,
binary diffs, SHA-256, qualified legacy refs, hook pathspec inheritance,
clock-skewed merges/all-parent continuation, nested repositories and worktree/
bare-remote sync operations. Test actual paired plans, not invented captures only.
Host-adapter bounded capture, source/epoch ABA, cleanup/uncertainty/concurrency
and native SSH/UI acceptance are separate required gates. Domain tests using
fixture `Command::output` do not establish production streaming or SSH safety.
All authored tests remain unverified until their exact-SHA Actions results.

## 7. Wrong vs Correct

Wrong: run `git diff` for both index and working comparisons, parse lines, and
apply shared `--literal-pathspecs` to every Git child including commit hooks.

Correct: use the paired byte-safe plans/parsers and explicit old/new blob
sources; scope literal pathspec behavior to file operations, and leave source
authority, dispatch, cleanup and host behavior to the coordinated adapter.
