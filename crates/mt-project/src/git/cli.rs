//! Host-neutral Git CLI plans. Execute in the captured repository root, using
//! structured argv; only the host transport may quote arguments for a shell.
//!
//! A plan is not execution authority. The caller owns repository/epoch checks,
//! discovery bounds, cancellation, write exclusion and uncertain-effect recovery.
//! Always call [`GitCommand::checked_stdout`] before parsing a captured result.

use std::time::Duration;

use anyhow::{Result, bail, ensure};

use super::{BranchInfo, ChangeFileStatus, GitDiffResult};

mod parse;
pub use parse::{
    parse_blob_size, parse_branches, parse_commit_files, parse_commit_parents,
    parse_index_entry, parse_log, parse_object_id, parse_repository_authority,
    parse_status, parse_tree_entry, parse_worktrees,
};

pub const MAX_BLOB_BYTES: usize = super::MAX_DIFF_BYTES;
pub const MAX_LIST_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_RECORDS: usize = 20_000;
pub const MAX_LOG_COMMITS: usize = 100;
pub const MAX_WORKTREES: usize = 1024;
pub const MAX_PATH_BYTES: usize = 16 * 1024;
const MAX_ARG_BYTES: usize = 128 * 1024;
const DIAGNOSTIC_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandEffect {
    ReadOnly,
    Mutation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommand {
    pub program: &'static str,
    pub args: Vec<String>,
    pub timeout: Duration,
    pub stdout_limit: usize,
    pub stderr_limit: usize,
    pub effect: CommandEffect,
}

/// Transport facts only. Dispatch uncertainty must remain in the host adapter,
/// including when a mutation exits nonzero or this capture cannot be parsed.
#[derive(Debug, Clone, Copy)]
pub struct CapturedOutput<'a> {
    pub stdout: &'a [u8],
    pub stderr: &'a [u8],
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

impl GitCommand {
    pub fn checked_stdout<'a>(&self, output: CapturedOutput<'a>) -> Result<&'a [u8]> {
        ensure!(!output.timed_out, "Git command timed out");
        ensure!(
            !output.stdout_truncated && !output.stderr_truncated,
            "Git command output was truncated"
        );
        ensure!(
            output.stdout.len() <= self.stdout_limit
                && output.stderr.len() <= self.stderr_limit,
            "Git command output exceeded its limit"
        );
        match output.exit_code {
            Some(0) => Ok(output.stdout),
            Some(code) => bail!("Git command failed with exit status {code}"),
            None => bail!("Git command has no confirmed exit status"),
        }
    }
}

fn command(args: &[&str], effect: CommandEffect, seconds: u64, limit: usize) -> GitCommand {
    let mut argv = vec!["--no-pager".to_string()];
    if effect == CommandEffect::ReadOnly {
        argv.push("--no-optional-locks".to_string());
    }
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    GitCommand {
        program: "git",
        args: argv,
        timeout: Duration::from_secs(seconds),
        stdout_limit: limit,
        stderr_limit: DIAGNOSTIC_BYTES,
        effect,
    }
}

fn read(args: &[&str]) -> GitCommand {
    command(args, CommandEffect::ReadOnly, 30, MAX_LIST_BYTES)
}

fn write(args: &[&str], seconds: u64) -> GitCommand {
    command(args, CommandEffect::Mutation, seconds, DIAGNOSTIC_BYTES)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectId(String);

impl ObjectId {
    pub fn parse(value: &str) -> Result<Self> {
        validate_oid(value, false)?;
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_oid(value: &str, allow_zero: bool) -> Result<()> {
    ensure!(
        matches!(value.len(), 40 | 64)
            && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && (allow_zero || value.bytes().any(|b| b != b'0')),
        "Expected a full nonzero lowercase Git object ID"
    );
    Ok(())
}

/// Fully qualified branch refs, never revision expressions. An existing ref
/// need not be valid branch-creation shorthand; keep it qualified when resolving.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GitRef(String);

impl GitRef {
    pub fn parse(value: &str) -> Result<Self> {
        let name = value
            .strip_prefix("refs/heads/")
            .or_else(|| value.strip_prefix("refs/remotes/"))
            .ok_or_else(|| anyhow::anyhow!("Expected a fully qualified branch ref"))?;
        ensure!(
            !name.is_empty()
                && !name.ends_with('.')
                && !name.contains("..")
                && !name.contains("@{")
                && value.len() <= MAX_PATH_BYTES
                && !name.bytes().any(|b| b <= b' ' || b == 0x7f || b"~^:?*[\\".contains(&b))
                && name.split('/').all(|part| {
                    !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock")
                }),
            "Invalid Git branch ref"
        );
        Ok(Self(value.to_string()))
    }

    /// Validate user-entered branch shorthand without expanding revision aliases.
    pub fn local(name: &str) -> Result<Self> {
        validate_branch_shorthand(name)?;
        Self::parse(&format!("refs/heads/{name}"))
    }

    pub fn remote(name: &str) -> Result<Self> {
        Self::parse(&format!("refs/remotes/{name}"))
    }

    pub fn from_branch(branch: &BranchInfo) -> Result<Self> {
        let namespace = if branch.is_remote { "refs/remotes/" } else { "refs/heads/" };
        Self::parse(&format!("{namespace}{}", branch.name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_remote(&self) -> bool {
        self.0.starts_with("refs/remotes/")
    }

    pub fn short_name(&self) -> &str {
        self.0
            .strip_prefix("refs/heads/")
            .or_else(|| self.0.strip_prefix("refs/remotes/"))
            .expect("validated branch namespace")
    }
}

fn validate_branch_shorthand(name: &str) -> Result<()> {
    ensure!(
        !name.starts_with('-') && name != "HEAD" && name != "@",
        "Git ref cannot be used as literal branch shorthand"
    );
    Ok(())
}

/// POSIX repository-relative spelling, without normalization or pathspec magic.
/// Backslash, colon, glob characters and newlines are valid literal filenames.
pub fn validate_repo_path(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty()
            && path.len() <= MAX_PATH_BYTES
            && !path.contains('\0')
            && path.split('/').all(|part| {
                !part.is_empty() && part != "." && part != ".." && part != ".git"
            }),
        "Expected an exact repository-relative file path"
    );
    Ok(())
}

fn validate_absolute_path(path: &str) -> Result<()> {
    ensure!(
        path.starts_with('/')
            && path.len() <= MAX_PATH_BYTES
            && !path.contains('\0')
            && (path == "/"
                || path[1..].split('/').all(|part| {
                    !part.is_empty() && part != "." && part != ".."
                })),
        "Expected an absolute canonical POSIX path"
    );
    Ok(())
}

fn with_paths(mut plan: GitCommand, paths: &[String]) -> Result<GitCommand> {
    ensure!(!paths.is_empty(), "An exact file selection is required");
    ensure!(paths.len() <= MAX_RECORDS, "Too many Git path arguments");
    let mut bytes = 0usize;
    for path in paths {
        validate_repo_path(path)?;
        bytes = bytes.saturating_add(path.len() + 1);
    }
    ensure!(bytes <= MAX_ARG_BYTES, "Git path arguments exceed their limit");
    // This global option is inherited by child Git processes. Only path-targeted
    // plans need it; commit/sync/worktree hooks must keep normal pathspec rules.
    plan.args.insert(0, "--literal-pathspecs".to_string());
    plan.args.push("--".to_string());
    plan.args.extend_from_slice(paths);
    Ok(plan)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryAuthority {
    pub worktree_root: String,
    pub git_dir: String,
    pub common_dir: String,
}

impl RepositoryAuthority {
    pub fn is_linked_worktree(&self) -> bool {
        self.git_dir != self.common_dir
    }
}

/// Independent outputs avoid ambiguous newline-delimited path tuples. The
/// adapter must pin and revalidate one repository authority across all three.
pub fn repository_authority_plans() -> [GitCommand; 3] {
    ["--show-toplevel", "--absolute-git-dir", "--git-common-dir"].map(|field| {
        command(
            &["rev-parse", "--path-format=absolute", field],
            CommandEffect::ReadOnly,
            30,
            MAX_PATH_BYTES + 1,
        )
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadState {
    /// None is an explicitly reported unborn HEAD, not a failed resolution.
    pub oid: Option<ObjectId>,
    /// None means detached; an unborn HEAD always has a branch ref.
    pub branch: Option<GitRef>,
}

#[derive(Debug, Clone)]
pub struct RepositoryStatus {
    pub head: HeadState,
    pub changes: Vec<ChangeFileStatus>,
    /// Exact reported directory paths, including their trailing slash. These
    /// are not regular-file change rows or file-only mutation/diff targets.
    pub untracked_directories: Vec<String>,
}

pub fn status_plan() -> GitCommand {
    read(&[
        "status",
        "--porcelain=v2",
        "--branch",
        "-z",
        "--untracked-files=all",
        "--ignored=no",
        "--ignore-submodules=none",
        "--no-ahead-behind",
        "--find-renames=50%",
    ])
}

pub fn branches_plan() -> GitCommand {
    read(&[
        "for-each-ref",
        "--sort=refname",
        "--format=%(refname)%00%(objectname)%00%(symref)%00",
        "refs/heads/",
        "refs/remotes/",
    ])
}

pub fn resolve_ref_plan(reference: &GitRef) -> GitCommand {
    let mut plan = read(&["rev-parse", "--verify", "--end-of-options"]);
    plan.args.push(format!("{}^{{commit}}", reference.as_str()));
    plan
}

/// Initial tips are the selected branch's object ID or the captured HEAD.
/// Continuation tips are ALL parents of the last commit, never that commit or
/// an offset. The consumer owns cross-page graph deduplication.
pub fn log_plan(tips: &[ObjectId], limit: usize) -> Result<Option<GitCommand>> {
    ensure!((1..=MAX_LOG_COMMITS).contains(&limit), "Invalid Git log page size");
    ensure!(tips.len() <= MAX_LOG_COMMITS, "Too many Git log tips");
    if tips.is_empty() {
        return Ok(None);
    }
    let mut plan = read(&[
        "log",
        "--date-order",
        "--no-patch",
        "--no-decorate",
        "--no-notes",
        "--no-show-signature",
        "--encoding=UTF-8",
        "--log-size",
        "-z",
        "--format=tformat:%H%x00%P%x00%an%x00%ct%x00%s%x00%b",
    ]);
    plan.args.push(format!("--max-count={limit}"));
    plan.args.extend(tips.iter().map(|tip| tip.as_str().to_string()));
    plan.args.push("--".to_string());
    Ok(Some(plan))
}

pub fn commit_parents_plan(commit: &ObjectId) -> GitCommand {
    read(&["rev-list", "--parents", "--max-count=1", commit.as_str(), "--"])
}

/// Supply the selected commit's FIRST parent, or None only for a verified root.
pub fn commit_files_plan(commit: &ObjectId, first_parent: Option<&ObjectId>) -> GitCommand {
    let mut plan = read(&[
        "diff-tree",
        "--no-commit-id",
        "--name-status",
        "-r",
        "-z",
        "--no-ext-diff",
        "--no-textconv",
        "--find-renames=50%",
        "--ignore-submodules=none",
    ]);
    if let Some(parent) = first_parent {
        plan.args.push(parent.as_str().to_string());
    } else {
        plan.args.push("--root".to_string());
    }
    plan.args.push(commit.as_str().to_string());
    plan.args.push("--".to_string());
    plan
}

/// A non-blob (e.g. a gitlink) is explicit, never interpreted as empty text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Blob,
    Tree,
    Commit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub oid: ObjectId,
    pub mode: u32,
    pub kind: EntryKind,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub oid: ObjectId,
    pub mode: u32,
}

pub fn tree_entry_plan(commit: &ObjectId, path: &str) -> Result<GitCommand> {
    with_paths(
        read(&["ls-tree", "-z", "-l", "--full-tree", commit.as_str()]),
        &[path.to_string()],
    )
}

pub fn index_entry_plan(path: &str) -> Result<GitCommand> {
    with_paths(
        read(&["ls-files", "--stage", "--full-name", "-z"]),
        &[path.to_string()],
    )
}

pub fn blob_size_plan(oid: &ObjectId) -> GitCommand {
    command(&["cat-file", "-s", oid.as_str()], CommandEffect::ReadOnly, 30, 32)
}

/// Read size first (ls-tree or cat-file -s). An oversized object must produce
/// DiffContent::TooLarge without fetching its contents. Reads are still capped.
pub fn blob_plan(oid: &ObjectId) -> GitCommand {
    command(
        &["cat-file", "blob", oid.as_str()],
        CommandEffect::ReadOnly,
        30,
        MAX_BLOB_BYTES,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobLookup {
    Empty,
    Tree { commit: ObjectId, path: String },
    Index { path: String },
    /// Bounded host filesystem read, not a local filesystem or Git diff call.
    Worktree { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffPlan {
    pub old: BlobLookup,
    pub new: BlobLookup,
}

fn tree_lookup(commit: Option<&ObjectId>, path: &str) -> BlobLookup {
    match commit {
        Some(commit) => BlobLookup::Tree { commit: commit.clone(), path: path.to_string() },
        None => BlobLookup::Empty,
    }
}

pub fn working_diff_plan(
    head: Option<&ObjectId>,
    path: &str,
    old_path: Option<&str>,
    staged: bool,
) -> Result<DiffPlan> {
    validate_repo_path(path)?;
    validate_repo_path(old_path.unwrap_or(path))?;
    Ok(DiffPlan {
        old: tree_lookup(head, old_path.unwrap_or(path)),
        new: if staged {
            BlobLookup::Index { path: path.to_string() }
        } else {
            BlobLookup::Worktree { path: path.to_string() }
        },
    })
}

pub fn commit_diff_plan(
    commit: &ObjectId,
    first_parent: Option<&ObjectId>,
    path: &str,
    old_path: Option<&str>,
) -> Result<DiffPlan> {
    validate_repo_path(path)?;
    validate_repo_path(old_path.unwrap_or(path))?;
    Ok(DiffPlan {
        old: tree_lookup(first_parent, old_path.unwrap_or(path)),
        new: tree_lookup(Some(commit), path),
    })
}

#[derive(Debug, Clone, Copy)]
pub enum DiffContent<'a> {
    /// Only proven absence (or the empty side of an unborn/root commit).
    Missing,
    Bytes(&'a [u8]),
    TooLarge,
}

fn diff_bytes(content: DiffContent<'_>) -> Option<&[u8]> {
    match content {
        DiffContent::Missing => Some(&[][..]),
        DiffContent::Bytes(bytes) if bytes.len() <= MAX_BLOB_BYTES => Some(bytes),
        DiffContent::Bytes(_) | DiffContent::TooLarge => None,
    }
}

pub fn build_diff(old: DiffContent<'_>, new: DiffContent<'_>) -> GitDiffResult {
    let (Some(old), Some(new)) = (diff_bytes(old), diff_bytes(new)) else {
        return super::too_large_diff_result();
    };
    if old.contains(&0) || new.contains(&0) {
        return super::binary_diff_result();
    }
    match (std::str::from_utf8(old), std::str::from_utf8(new)) {
        (Ok(old), Ok(new)) => super::diff_two_texts(old.to_string(), new.to_string()),
        _ => super::binary_diff_result(),
    }
}

pub fn stage_plan(paths: &[String]) -> Result<GitCommand> {
    with_paths(write(&["add", "--all"], 30), paths)
}

pub fn stage_all_plan() -> GitCommand {
    write(&["--literal-pathspecs", "add", "--all", "--", "."], 30)
}

pub fn unstage_plan(head: Option<&ObjectId>, paths: &[String]) -> Result<GitCommand> {
    let plan = match head {
        Some(head) => write(&["reset", "--quiet", head.as_str()], 30),
        None => write(&["rm", "--cached", "--force", "--ignore-unmatch"], 30),
    };
    with_paths(plan, paths)
}

pub fn unstage_all_plan(head: Option<&ObjectId>) -> GitCommand {
    match head {
        // Preserve mixed-reset merge-state cleanup without requesting a rewind
        // to an old object ID. The caller still revalidates its captured HEAD.
        Some(_) => write(&["reset", "--mixed", "HEAD", "--"], 30),
        None => write(&["read-tree", "--empty"], 30),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscardPlan {
    Git(GitCommand),
    /// The host must remove this exact leaf nonrecursively, without following
    /// leaf symlinks; revalidate parent containment and current untracked status.
    RemoveUntrackedFile { path: String },
}

/// `tracked` must come from a current exact index/status probe, not its display
/// label: untracked files in an unborn repository are displayed as Added.
pub fn discard_plan(path: &str, head: Option<&ObjectId>, tracked: bool) -> Result<DiscardPlan> {
    validate_repo_path(path)?;
    if !tracked {
        return Ok(DiscardPlan::RemoveUntrackedFile { path: path.to_string() });
    }
    let command = match head {
        Some(head) => write(
            &["restore", "--source", head.as_str(), "--staged", "--worktree"],
            30,
        ),
        None => write(&["rm", "--force"], 30),
    };
    Ok(DiscardPlan::Git(with_paths(command, &[path.to_string()])?))
}

pub fn commit_plan(message: &str) -> Result<GitCommand> {
    ensure!(
        !message.trim().is_empty() && !message.contains('\0') && message.len() <= 64 * 1024,
        "Invalid or oversized Git commit message"
    );
    Ok(write(&["commit", "-m", message], 60))
}

pub fn pull_plan() -> GitCommand {
    write(&["pull"], 30)
}

pub fn push_plan() -> GitCommand {
    write(&["push"], 30)
}

pub fn worktree_list_plan() -> GitCommand {
    read(&["worktree", "list", "--porcelain", "-z"])
}

/// Reuse the legacy DTO projection, retaining POSIX paths on every client OS.
/// Native display_path/file_name would otherwise trim or split backslashes on
/// Windows. Keep authoritative facts separately for catalog/cleanup decisions.
pub fn project_worktrees(facts: &[crate::worktree::WorktreeFact]) -> Result<Vec<super::WorktreeInfo>> {
    ensure!(facts.len() <= MAX_WORKTREES, "Too many Git worktrees");
    facts.iter().map(|fact| {
        let path = fact.path.to_str()
            .ok_or_else(|| anyhow::anyhow!("Git worktree path is not valid UTF-8"))?;
        validate_absolute_path(path)?;
        let mut info = super::project_worktree_fact(fact);
        info.path = path.to_string();
        info.name = path.rsplit('/').next().filter(|name| !name.is_empty())
            .unwrap_or("main").to_string();
        Ok(info)
    }).collect()
}

pub fn worktree_add_plan(
    target: &str,
    branch: &GitRef,
    create_branch: bool,
    base: Option<&ObjectId>,
) -> Result<GitCommand> {
    validate_absolute_path(target)?;
    ensure!(target != "/", "Cannot create a worktree at the filesystem root");
    ensure!(!branch.is_remote(), "A worktree must use a local branch");
    validate_branch_shorthand(branch.short_name())?;
    ensure!(create_branch || base.is_none(), "Existing branch cannot have a new base");
    let mut plan = write(&["worktree", "add"], 120);
    if create_branch {
        plan.args.extend(["-b".to_string(), branch.short_name().to_string()]);
    }
    plan.args.extend(["--".to_string(), target.to_string()]);
    if create_branch {
        if let Some(base) = base {
            plan.args.push(base.as_str().to_string());
        }
    } else {
        plan.args.push(branch.short_name().to_string());
    }
    Ok(plan)
}

pub fn worktree_remove_plan(target: &str, main_worktree: &str, force: bool) -> Result<GitCommand> {
    validate_absolute_path(target)?;
    validate_absolute_path(main_worktree)?;
    ensure!(target != "/" && target != main_worktree, "Cannot remove the main worktree");
    let mut plan = write(&["worktree", "remove"], 60);
    if force {
        plan.args.push("--force".to_string());
    }
    plan.args.extend(["--".to_string(), target.to_string()]);
    Ok(plan)
}

pub fn worktree_prune_plan() -> GitCommand {
    write(&["worktree", "prune"], 30)
}

#[cfg(test)]
mod tests;
