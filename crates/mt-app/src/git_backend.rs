//! Source-owned Git operations. All blocking methods belong on a background
//! executor; UI generations still decide which owned result may be published.

mod host;
mod write;

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use mt_project::git::cli::{
    self, BlobLookup, DiffContent, GitCommand, GitRef, ObjectId, RepositoryAuthority,
    RepositoryStatus,
};
use mt_project::git::{
    self, BranchInfo, CommitFileInfo, GitCommitInfo, GitDiffResult, GitRepoInfo, WorktreeInfo,
};
use mt_project::worktree::{
    self, WorktreeFact, WorktreePathState, WorktreeScan, WorktreeScanSource,
};

use crate::execution_host::{
    self, ExecutionBackend, ExecutionSourceSignature, ProjectExecutionSnapshot,
};
use crate::remote_ssh;

use host::{ExecutionGitHost, Host, NodeKind};
pub use write::{
    GitPostcondition, GitWrite, GitWriteOutcome, GitWritePhase, GitWriteState, PreparedGitWrite,
    UncertainReview,
};

pub type GitResult<T> = Result<T, GitError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitErrorKind {
    Invalid,
    Unavailable,
    Unsupported,
    Permission,
    Stale,
    Busy,
    Changed,
    Limit,
    Command,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitError {
    pub kind: GitErrorKind,
    pub message: String,
}

impl GitError {
    fn new(kind: GitErrorKind, message: impl Into<String>) -> Self {
        let mut message = message.into();
        if message.len() > 4096 {
            let mut end = 4096;
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
        }
        Self { kind, message }
    }
    fn invalid(message: impl Into<String>) -> Self {
        Self::new(GitErrorKind::Invalid, message)
    }
    fn unavailable(message: impl Into<String>) -> Self {
        Self::new(GitErrorKind::Unavailable, message)
    }
    fn stale() -> Self {
        Self::new(GitErrorKind::Stale, "Git source authority changed")
    }
    fn changed() -> Self {
        Self::new(
            GitErrorKind::Changed,
            "Git state changed since this operation was prepared",
        )
    }
    fn domain(error: impl fmt::Display) -> Self {
        Self::new(GitErrorKind::Command, error.to_string())
    }
    fn io(error: std::io::Error) -> Self {
        Self::new(
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                GitErrorKind::Permission
            } else {
                GitErrorKind::Unavailable
            },
            error.to_string(),
        )
    }
}

impl fmt::Display for GitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl std::error::Error for GitError {}

/// Invalidating a view/source prevents queued dispatch, but cannot cancel or
/// release an operation which may already have affected its original source.
#[derive(Clone)]
pub struct GitLifetime(Arc<AtomicBool>);

impl Default for GitLifetime {
    fn default() -> Self {
        Self::new()
    }
}
impl GitLifetime {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
    pub fn invalidate(&self) {
        self.0.store(false, Ordering::Release);
    }
    pub fn is_valid(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Clone)]
pub struct GitBackend {
    host: Arc<dyn Host>,
    lifetime: GitLifetime,
    anchor: String,
    probe_anchor: String,
    recovery_only: bool,
}

impl GitBackend {
    /// Read readiness may establish an absent epoch, never replace a captured
    /// one. Use the returned snapshot for subsequent publication fences.
    pub fn connect(
        mut snapshot: ProjectExecutionSnapshot,
        lifetime: GitLifetime,
    ) -> GitResult<Self> {
        if !lifetime.is_valid() {
            return Err(GitError::stale());
        }
        if snapshot.project_id.is_empty() || snapshot.root_project_id.is_empty() {
            return Err(GitError::invalid("Git source project identity is missing"));
        }
        if let ExecutionBackend::Ssh {
            connection,
            connection_fingerprint,
            connection_epoch,
        } = &mut snapshot.backend
        {
            let observed = remote_ssh::git_connection_epoch(connection, *connection_fingerprint)
                .map_err(GitError::unavailable)?;
            *connection_epoch = Some(Self::checked_readiness_epoch(*connection_epoch, observed)?);
        }
        let host = Arc::new(ExecutionGitHost(snapshot));
        let anchor = host.canonical_directory(&host.snapshot().canonical_path)?;
        let probe_anchor = host.snapshot().canonical_path.clone();
        let backend = Self {
            host,
            lifetime,
            anchor,
            probe_anchor,
            recovery_only: false,
        };
        backend.validate_path(&backend.snapshot().canonical_path)?;
        backend.check()?;
        Ok(backend)
    }

    fn checked_readiness_epoch(captured: Option<u64>, observed: u64) -> GitResult<u64> {
        if captured.is_some_and(|epoch| epoch != observed) {
            return Err(GitError::stale());
        }
        Ok(observed)
    }

    pub fn snapshot(&self) -> &ProjectExecutionSnapshot {
        self.host.snapshot()
    }
    pub fn lifetime(&self) -> GitLifetime {
        self.lifetime.clone()
    }
    pub fn matches_snapshot(&self, current: &ProjectExecutionSnapshot) -> bool {
        !self.recovery_only
            && self.snapshot().project_id == current.project_id
            && self.snapshot().source_signature() == current.source_signature()
            && self.check().is_ok()
    }

    fn check(&self) -> GitResult<()> {
        if !self.lifetime.is_valid() {
            return Err(GitError::stale());
        }
        if let ExecutionBackend::Ssh {
            connection,
            connection_fingerprint,
            connection_epoch,
        } = &self.snapshot().backend
        {
            if connection_epoch.is_none()
                || remote_ssh::connection_fingerprint(connection) != *connection_fingerprint
                || remote_ssh::current_connection_epoch(&connection.id) != *connection_epoch
            {
                return Err(GitError::stale());
            }
        }
        Ok(())
    }

    fn local(&self) -> bool {
        matches!(&self.snapshot().backend, ExecutionBackend::Local)
    }

    fn validate_path(&self, path: &str) -> GitResult<()> {
        if path.len() > cli::MAX_PATH_BYTES || path.contains('\0') || path.is_empty() {
            return Err(GitError::invalid("Invalid Git host path"));
        }
        if self.local() {
            if !Path::new(path).is_absolute() {
                return Err(GitError::invalid("Git native path is not absolute"));
            }
        } else if execution_host::normalize_absolute_posix_path(path).map_err(GitError::invalid)?
            != path
        {
            return Err(GitError::invalid(
                "Git host path is not canonical POSIX spelling",
            ));
        }
        Ok(())
    }

    fn join(&self, root: &str, relative: &str) -> GitResult<String> {
        if self.local() {
            Path::new(root)
                .join(relative)
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| GitError::invalid("Git path is not UTF-8"))
        } else {
            Ok(remote_ssh::join_posix(root, relative))
        }
    }

    fn parent(&self, path: &str) -> Option<String> {
        if self.local() {
            Path::new(path).parent()?.to_str().map(str::to_owned)
        } else if path == "/" {
            None
        } else {
            Some(
                path.rsplit_once('/')
                    .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
                    .unwrap_or("/")
                    .to_owned(),
            )
        }
    }

    fn command(&self, cwd: &str, plan: &GitCommand) -> GitResult<Vec<u8>> {
        self.check()?;
        let bytes = self.host.run(cwd, plan).checked(plan)?;
        self.check()?;
        Ok(bytes)
    }

    fn authority_at(&self, path: &str) -> GitResult<RepositoryAuthority> {
        self.check()?;
        if self.host.canonical_directory(&self.probe_anchor)? != self.anchor {
            return Err(GitError::stale());
        }
        if self.local() {
            let authority = git::local::authority(Path::new(path)).map_err(GitError::domain)?;
            self.check()?;
            return Ok(authority);
        }
        self.authority_cli_at(path)
    }

    fn authority_cli_at(&self, path: &str) -> GitResult<RepositoryAuthority> {
        let plans = cli::repository_authority_plans();
        let root = self.command(path, &plans[0])?;
        let git_dir = self.command(path, &plans[1])?;
        let common_dir = self.command(path, &plans[2])?;
        let authority = if self.local() {
            // Native Windows Git emits drive paths; the POSIX domain parser
            // intentionally refuses to interpret them as remote authority.
            RepositoryAuthority {
                worktree_root: self.host.canonical_directory(single_path(&root)?)?,
                git_dir: self.host.canonical_directory(single_path(&git_dir)?)?,
                common_dir: self.host.canonical_directory(single_path(&common_dir)?)?,
            }
        } else {
            cli::parse_repository_authority(&root, &git_dir, &common_dir)
                .map_err(GitError::domain)?
        };
        for directory in [
            &authority.worktree_root,
            &authority.git_dir,
            &authority.common_dir,
        ] {
            self.validate_path(directory)?;
            if self.host.canonical_directory(directory)? != *directory {
                return Err(GitError::stale());
            }
            self.check()?;
        }
        Ok(authority)
    }

    fn repository_at(&self, path: &str) -> GitResult<GitRepository> {
        let authority = self.authority_at(path)?;
        if authority.worktree_root != path {
            return Err(GitError::stale());
        }
        let name = if self.local() {
            Path::new(path)
                .file_name()
                .and_then(|part| part.to_str())
                .unwrap_or(path)
        } else {
            path.rsplit('/')
                .next()
                .filter(|part| !part.is_empty())
                .unwrap_or(path)
        };
        let mut repository = GitRepository {
            backend: self.clone(),
            info: GitRepoInfo {
                name: name.to_owned(),
                path: PathBuf::from(path),
                current_branch: None,
                is_worktree: authority.is_linked_worktree(),
            },
            authority,
        };
        let head = repository.status_read()?.head;
        repository.info.current_branch = head
            .branch
            .map(|branch| branch.short_name().to_owned())
            .or_else(|| head.oid.map(|oid| format!("({})", &oid.as_str()[..7])));
        repository.validate()?;
        Ok(repository)
    }

    /// Resolve FileTree's literal project-relative file on this captured host.
    /// A nested repository wins; absent file parents do not hide its repository.
    pub fn repository_for_file(
        &self,
        project_relative_file: &str,
    ) -> GitResult<(GitRepository, String)> {
        if self.recovery_only {
            return Err(GitError::stale());
        }
        self.check()?;
        if self.local() {
            git::local::validate_native_repo_path(project_relative_file)
                .map_err(GitError::domain)?;
        } else {
            cli::validate_repo_path(project_relative_file).map_err(GitError::domain)?;
        }
        let start = Instant::now();
        if self.host.canonical_directory(&self.probe_anchor)? != self.anchor {
            return Err(GitError::stale());
        }
        let file = self.join(&self.anchor, project_relative_file)?;
        self.validate_path(&file)?;
        if matches!(
            self.host.file_kind(&self.anchor, project_relative_file)?,
            Some(NodeKind::Directory | NodeKind::Other)
        ) {
            return Err(GitError::invalid("Git diff target is not a file"));
        }
        let mut ceiling = self.anchor.clone();
        for _ in 0..5 {
            let Some(parent) = self.parent(&ceiling) else {
                break;
            };
            ceiling = parent;
        }
        let mut current = self.parent(&file).ok_or_else(GitError::stale)?;
        let mut relative = project_relative_file
            .rsplit('/')
            .next()
            .ok_or_else(GitError::stale)?
            .to_owned();
        loop {
            budget(start)?;
            self.check()?;
            match self.host.kind(&current)? {
                Some(NodeKind::Directory) => {
                    if self.host.canonical_directory(&current)? != current {
                        return Err(GitError::stale());
                    }
                    match self.host.kind(&self.join(&current, ".git")?)? {
                        Some(NodeKind::File | NodeKind::Directory) => {
                            let repository = self.repository_at(&current)?;
                            if self.local() {
                                git::local::validate_native_repo_path(&relative)
                                    .map_err(GitError::domain)?;
                            } else {
                                cli::validate_repo_path(&relative).map_err(GitError::domain)?;
                            }
                            if matches!(
                                self.host.file_kind(&current, &relative)?,
                                Some(NodeKind::Directory | NodeKind::Other)
                            ) {
                                return Err(GitError::invalid("Git diff target is not a file"));
                            }
                            repository.validate()?;
                            budget(start)?;
                            return Ok((repository, relative));
                        }
                        Some(_) => {
                            return Err(GitError::invalid(
                                "Git marker is not a regular file or directory",
                            ));
                        }
                        None => {}
                    }
                }
                Some(_) => {
                    return Err(GitError::invalid("Git file parent is not a real directory"));
                }
                None => {}
            }
            if current == ceiling {
                break;
            }
            let name = if self.local() {
                Path::new(&current)
                    .file_name()
                    .and_then(|name| name.to_str())
            } else {
                current.rsplit('/').next()
            };
            let name = name
                .filter(|name| !name.is_empty())
                .ok_or_else(GitError::stale)?;
            relative = format!("{name}/{relative}");
            if relative.len() > cli::MAX_PATH_BYTES {
                return Err(GitError::new(
                    GitErrorKind::Limit,
                    "Git relative file path exceeds its limit",
                ));
            }
            current = self.parent(&current).ok_or_else(GitError::stale)?;
        }
        self.check()?;
        Err(GitError::unavailable(
            "No Git repository contains this file within the project discovery boundary",
        ))
    }

    /// Root/at most five ancestors, otherwise descendants at most five levels.
    /// Linked siblings are inventory, never automatically project repositories.
    pub fn discover(&self) -> GitResult<Vec<GitRepository>> {
        if self.recovery_only {
            return Err(GitError::stale());
        }
        self.check()?;
        let start = Instant::now();
        let anchor = self
            .host
            .canonical_directory(&self.snapshot().canonical_path)?;
        if anchor != self.anchor {
            return Err(GitError::stale());
        }
        self.validate_path(&anchor)?;
        if !self.local() {
            self.command(&anchor, &host::auxiliary("git", &["--version"], 4096))?;
        }
        let mut current = anchor.clone();
        for _ in 0..=5 {
            budget(start)?;
            self.check()?;
            match self.host.kind(&self.join(&current, ".git")?)? {
                Some(NodeKind::File | NodeKind::Directory) => {
                    return Ok(vec![self.repository_at(&current)?]);
                }
                Some(_) => {
                    return Err(GitError::invalid(
                        "Git marker is not a regular file or directory",
                    ));
                }
                None => {}
            }
            let Some(parent) = self.parent(&current) else {
                break;
            };
            current = parent;
        }
        let mut stack = vec![(anchor, 0_usize)];
        let mut seen = HashSet::new();
        let mut repositories = Vec::new();
        while let Some((directory, depth)) = stack.pop() {
            budget(start)?;
            self.check()?;
            if !seen.insert(directory.clone()) {
                continue;
            }
            if seen.len() > cli::MAX_RECORDS {
                return Err(GitError::new(
                    GitErrorKind::Limit,
                    "Git discovery exceeded its directory limit",
                ));
            }
            if depth > 0 {
                match self.host.kind(&self.join(&directory, ".git")?)? {
                    Some(NodeKind::File | NodeKind::Directory) => {
                        repositories.push(self.repository_at(&directory)?);
                        continue;
                    }
                    Some(_) => {
                        return Err(GitError::invalid(
                            "Git marker is not a regular file or directory",
                        ));
                    }
                    None => {}
                }
            }
            if depth == 5 {
                continue;
            }
            for child in self.host.directories(&directory)? {
                self.validate_path(&child)?;
                let name = if self.local() {
                    Path::new(&child)
                        .file_name()
                        .and_then(|part| part.to_str())
                        .unwrap_or("")
                } else {
                    child.rsplit('/').next().unwrap_or("")
                };
                if [
                    ".git",
                    "node_modules",
                    "target",
                    ".next",
                    "dist",
                    "__pycache__",
                    ".superpowers",
                ]
                .contains(&name)
                {
                    continue;
                }
                if self.parent(&child).as_deref() != Some(directory.as_str()) {
                    return Err(GitError::stale());
                }
                stack.push((child, depth + 1));
                if stack.len() + seen.len() > cli::MAX_RECORDS {
                    return Err(GitError::new(
                        GitErrorKind::Limit,
                        "Git discovery exceeded its directory limit",
                    ));
                }
            }
        }
        self.check()?;
        repositories.sort_by(|a, b| a.authority.worktree_root.cmp(&b.authority.worktree_root));
        Ok(repositories)
    }
}

#[derive(Clone)]
pub struct GitRepository {
    backend: GitBackend,
    authority: RepositoryAuthority,
    info: GitRepoInfo,
}

impl GitRepository {
    pub fn backend(&self) -> &GitBackend {
        &self.backend
    }
    pub fn authority(&self) -> &RepositoryAuthority {
        &self.authority
    }
    pub fn info(&self) -> &GitRepoInfo {
        &self.info
    }
    pub fn matches_snapshot(&self, current: &ProjectExecutionSnapshot) -> bool {
        self.backend.matches_snapshot(current)
    }
    pub fn request(&self, operation: GitRead) -> GitResult<GitReadRequest> {
        if self.backend.recovery_only {
            return Err(GitError::stale());
        }
        self.backend.check()?;
        Ok(GitReadRequest {
            id: next_id()?,
            repository: self.clone(),
            operation,
        })
    }

    fn same_source(&self, other: &Self) -> bool {
        self.authority == other.authority
            && self.backend.snapshot().project_id == other.backend.snapshot().project_id
            && self.backend.snapshot().source_signature()
                == other.backend.snapshot().source_signature()
            && Arc::ptr_eq(&self.backend.lifetime.0, &other.backend.lifetime.0)
    }
    fn validate(&self) -> GitResult<()> {
        if self.backend.authority_at(&self.authority.worktree_root)? != self.authority {
            return Err(GitError::stale());
        }
        self.backend.check()
    }
    fn command(&self, plan: &GitCommand) -> GitResult<Vec<u8>> {
        self.backend.command(&self.authority.worktree_root, plan)
    }
    fn status_cli(&self) -> GitResult<RepositoryStatus> {
        cli::parse_status(&self.command(&cli::status_plan())?).map_err(GitError::domain)
    }
    fn status_read(&self) -> GitResult<RepositoryStatus> {
        if self.backend.local() {
            git::local::status(Path::new(&self.authority.worktree_root)).map_err(GitError::domain)
        } else {
            self.status_cli()
        }
    }
    fn branches_cli(&self, status: &RepositoryStatus) -> GitResult<Vec<BranchInfo>> {
        cli::parse_branches(
            &self.command(&cli::branches_plan())?,
            status.head.oid.as_ref(),
        )
        .map_err(GitError::domain)
    }
    fn parents(&self, commit: &ObjectId) -> GitResult<Vec<ObjectId>> {
        if self.backend.local() {
            return git::local::parents(Path::new(&self.authority.worktree_root), commit)
                .map_err(GitError::domain);
        }
        cli::parse_commit_parents(&self.command(&cli::commit_parents_plan(commit))?, commit)
            .map_err(GitError::domain)
    }
    fn resolve(&self, reference: &GitRef) -> GitResult<ObjectId> {
        if self.backend.local() {
            return git::local::resolve_ref(Path::new(&self.authority.worktree_root), reference)
                .map_err(GitError::domain);
        }
        cli::parse_object_id(&self.command(&cli::resolve_ref_plan(reference))?)
            .map_err(GitError::domain)
    }

    fn content(&self, lookup: &BlobLookup) -> GitResult<OwnedContent> {
        if self.backend.local() && !matches!(lookup, BlobLookup::Worktree { .. }) {
            return git::local::content(Path::new(&self.authority.worktree_root), lookup)
                .map(|content| match content {
                    git::local::BlobContent::Missing => OwnedContent::Missing,
                    git::local::BlobContent::Bytes(bytes) => OwnedContent::Bytes(bytes),
                    git::local::BlobContent::TooLarge => OwnedContent::TooLarge,
                })
                .map_err(GitError::domain);
        }
        let oid = match lookup {
            BlobLookup::Empty => return Ok(OwnedContent::Missing),
            BlobLookup::Worktree { path } => {
                self.backend.check()?;
                let content = self
                    .backend
                    .host
                    .read_file(&self.authority.worktree_root, path)?;
                self.backend.check()?;
                return Ok(content);
            }
            BlobLookup::Tree { commit, path } => {
                let plan = cli::tree_entry_plan(commit, path).map_err(GitError::domain)?;
                let Some(entry) =
                    cli::parse_tree_entry(&self.command(&plan)?, path).map_err(GitError::domain)?
                else {
                    return Ok(OwnedContent::Missing);
                };
                if entry.kind != cli::EntryKind::Blob {
                    return Err(GitError::new(
                        GitErrorKind::Unsupported,
                        "Git entry is not a blob",
                    ));
                }
                if entry
                    .size
                    .is_some_and(|size| size > cli::MAX_BLOB_BYTES as u64)
                {
                    return Ok(OwnedContent::TooLarge);
                }
                entry.oid
            }
            BlobLookup::Index { path } => {
                let plan = cli::index_entry_plan(path).map_err(GitError::domain)?;
                let Some(entry) = cli::parse_index_entry(&self.command(&plan)?, path)
                    .map_err(GitError::domain)?
                else {
                    return Ok(OwnedContent::Missing);
                };
                entry.oid
            }
        };
        let size = cli::parse_blob_size(&self.command(&cli::blob_size_plan(&oid))?)
            .map_err(GitError::domain)?;
        if size > cli::MAX_BLOB_BYTES as u64 {
            return Ok(OwnedContent::TooLarge);
        }
        let bytes = self.command(&cli::blob_plan(&oid))?;
        if bytes.len() as u64 != size {
            return Err(GitError::invalid(
                "Git blob size does not match its bounded capture",
            ));
        }
        Ok(OwnedContent::Bytes(bytes))
    }

    fn diff(&self, plan: &cli::DiffPlan) -> GitResult<GitDiffResult> {
        let old = self.content(&plan.old)?;
        let new = self.content(&plan.new)?;
        Ok(cli::build_diff(old.borrow(), new.borrow()))
    }

    fn worktrees(&self) -> GitResult<GitWorktrees> {
        let bytes = self.command(&cli::worktree_list_plan())?;
        let mut facts = if self.backend.local() && cfg!(windows) {
            if !bytes.ends_with(b"\0\0") {
                return Err(GitError::invalid("Incomplete Git worktree inventory"));
            }
            let facts = worktree::parse_porcelain(worktree::WorktreePorcelainMode::Nul, &bytes)
                .map_err(GitError::domain)?;
            if facts.len() > cli::MAX_WORKTREES {
                return Err(GitError::new(GitErrorKind::Limit, "Too many Git worktrees"));
            }
            let mut width = None;
            for fact in &facts {
                self.backend.validate_path(fact_path(fact)?)?;
                if fact.is_bare {
                    if fact.head.is_some() || fact.branch_ref.is_some() || fact.is_detached {
                        return Err(GitError::invalid("Bare worktree contains checkout fields"));
                    }
                } else if fact.head.is_none() {
                    return Err(GitError::invalid("Worktree is missing HEAD"));
                }
                if let Some(head) = &fact.head {
                    if ![40, 64].contains(&head.len())
                        || !head
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                        || width.is_some_and(|width| width != head.len())
                    {
                        return Err(GitError::invalid("Invalid worktree HEAD object format"));
                    }
                    width = Some(head.len());
                }
                if let Some(branch) = &fact.branch_ref {
                    GitRef::parse(branch).map_err(GitError::domain)?;
                }
            }
            facts
        } else {
            cli::parse_worktrees(&bytes).map_err(GitError::domain)?
        };
        for fact in &mut facts {
            self.backend.check()?;
            if self.backend.local() && cfg!(windows) {
                fact.path = PathBuf::from(canonical_native_inventory_path(
                    &self.backend,
                    fact_path(fact)?,
                )?);
            }
            fact.path_state = match self.backend.host.kind(fact_path(fact)?)? {
                None => WorktreePathState::Missing,
                Some(NodeKind::Directory) => WorktreePathState::Present,
                Some(_) => WorktreePathState::Unknown,
            };
        }
        let scan = WorktreeScan {
            generation: 0,
            source: WorktreeScanSource::PorcelainZ,
            authoritative: true,
            worktrees: facts,
            warning: None,
        };
        let entries = if self.backend.local() {
            git::project_worktree_scan(&scan)
        } else {
            cli::project_worktrees(&scan.worktrees).map_err(GitError::domain)?
        };
        Ok(GitWorktrees { scan, entries })
    }
}

#[derive(Clone, Debug)]
pub enum GitRead {
    Status,
    Branches,
    /// Continue from ALL parents of `before`; caller deduplicates graph pages.
    History {
        before: Option<ObjectId>,
        branch: Option<GitRef>,
        limit: usize,
    },
    CommitFiles {
        commit: ObjectId,
    },
    WorkingDiff {
        path: String,
        old_path: Option<String>,
        staged: bool,
    },
    CommitDiff {
        commit: ObjectId,
        path: String,
        old_path: Option<String>,
    },
    Worktrees,
}

#[derive(Clone, Debug)]
pub struct GitWorktrees {
    pub scan: WorktreeScan,
    pub entries: Vec<WorktreeInfo>,
}

#[derive(Clone, Debug)]
pub enum GitReadValue {
    Status(RepositoryStatus),
    Branches(Vec<BranchInfo>),
    History(Vec<GitCommitInfo>),
    CommitFiles(Vec<CommitFileInfo>),
    Diff(GitDiffResult),
    Worktrees(GitWorktrees),
}

#[derive(Clone)]
pub struct GitReadRequest {
    id: u64,
    repository: GitRepository,
    operation: GitRead,
}

impl GitReadRequest {
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn repository(&self) -> &GitRepository {
        &self.repository
    }
    pub fn operation(&self) -> &GitRead {
        &self.operation
    }
    pub fn execute(self) -> GitResult<GitReadResult> {
        let repository = &self.repository;
        repository.validate()?;
        let local = repository.backend.local();
        let path = Path::new(&repository.authority.worktree_root);
        let value = match &self.operation {
            GitRead::Status => GitReadValue::Status(repository.status_read()?),
            GitRead::Branches => {
                let branches = if local {
                    git::local::branches(path).map_err(GitError::domain)?
                } else {
                    repository.branches_cli(&repository.status_cli()?)?
                };
                GitReadValue::Branches(branches)
            }
            GitRead::History {
                before,
                branch,
                limit,
            } => {
                let tips = if let Some(before) = before {
                    repository.parents(before)?
                } else if let Some(branch) = branch {
                    vec![repository.resolve(branch)?]
                } else {
                    repository.status_read()?.head.oid.into_iter().collect()
                };
                let commits = if local {
                    git::local::history(path, &tips, *limit).map_err(GitError::domain)?
                } else if let Some(plan) = cli::log_plan(&tips, *limit).map_err(GitError::domain)? {
                    cli::parse_log(&repository.command(&plan)?).map_err(GitError::domain)?
                } else {
                    Vec::new()
                };
                GitReadValue::History(commits)
            }
            GitRead::CommitFiles { commit } => {
                let files = if local {
                    git::local::commit_files(path, commit).map_err(GitError::domain)?
                } else {
                    let parents = repository.parents(commit)?;
                    cli::parse_commit_files(
                        &repository.command(&cli::commit_files_plan(commit, parents.first()))?,
                    )
                    .map_err(GitError::domain)?
                };
                GitReadValue::CommitFiles(files)
            }
            GitRead::WorkingDiff {
                path,
                old_path,
                staged,
            } => {
                let status = repository.status_read()?;
                let before = status_key(&status)?;
                let plan = cli::working_diff_plan(
                    status.head.oid.as_ref(),
                    path,
                    old_path.as_deref(),
                    *staged,
                )
                .map_err(GitError::domain)?;
                let diff = repository.diff(&plan)?;
                if status_key(&repository.status_read()?)? != before {
                    return Err(GitError::changed());
                }
                GitReadValue::Diff(diff)
            }
            GitRead::CommitDiff {
                commit,
                path,
                old_path,
            } => {
                let parents = repository.parents(commit)?;
                let plan =
                    cli::commit_diff_plan(commit, parents.first(), path, old_path.as_deref())
                        .map_err(GitError::domain)?;
                GitReadValue::Diff(repository.diff(&plan)?)
            }
            GitRead::Worktrees => {
                let inventory = if local {
                    let scan = worktree::scan(path).map_err(GitError::domain)?;
                    if scan.worktrees.len() > cli::MAX_WORKTREES {
                        return Err(GitError::new(GitErrorKind::Limit, "Too many Git worktrees"));
                    }
                    for fact in &scan.worktrees {
                        repository.backend.validate_path(fact_path(fact)?)?;
                    }
                    GitWorktrees {
                        entries: git::project_worktree_scan(&scan),
                        scan,
                    }
                } else {
                    repository.worktrees()?
                };
                GitReadValue::Worktrees(inventory)
            }
        };
        repository.validate()?;
        Ok(GitReadResult {
            request: self,
            value,
        })
    }
}

pub struct GitReadResult {
    request: GitReadRequest,
    pub value: GitReadValue,
}

impl GitReadResult {
    pub fn id(&self) -> u64 {
        self.request.id
    }
    pub fn repository(&self) -> &GitRepository {
        &self.request.repository
    }
    pub fn is_current(&self, current: &GitRepository, latest_request: u64) -> bool {
        self.id() == latest_request
            && self.repository().same_source(current)
            && current.backend.check().is_ok()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum OwnedContent {
    Missing,
    Bytes(Vec<u8>),
    TooLarge,
}
impl OwnedContent {
    fn bounded(bytes: Vec<u8>) -> Self {
        if bytes.len() > cli::MAX_BLOB_BYTES {
            Self::TooLarge
        } else {
            Self::Bytes(bytes)
        }
    }
    fn borrow(&self) -> DiffContent<'_> {
        match self {
            Self::Missing => DiffContent::Missing,
            Self::Bytes(bytes) => DiffContent::Bytes(bytes),
            Self::TooLarge => DiffContent::TooLarge,
        }
    }
}

fn single_path(bytes: &[u8]) -> GitResult<&str> {
    let bytes = bytes
        .strip_suffix(b"\n")
        .ok_or_else(|| GitError::invalid("Incomplete Git authority path"))?;
    if bytes.is_empty() || bytes.len() > cli::MAX_PATH_BYTES || bytes.contains(&0) {
        return Err(GitError::invalid("Invalid Git authority path"));
    }
    std::str::from_utf8(bytes).map_err(GitError::domain)
}

fn fact_path(fact: &WorktreeFact) -> GitResult<&str> {
    fact.path
        .to_str()
        .ok_or_else(|| GitError::invalid("Git worktree path is not UTF-8"))
}

fn canonical_native_inventory_path(backend: &GitBackend, path: &str) -> GitResult<String> {
    let mut parent = PathBuf::from(path);
    let mut suffix = Vec::new();
    for _ in 0..=64 {
        let text = parent
            .to_str()
            .ok_or_else(|| GitError::invalid("Git native path is not UTF-8"))?;
        match backend.host.kind(text)? {
            Some(NodeKind::Directory) => {
                let mut canonical = PathBuf::from(backend.host.canonical_directory(text)?);
                for part in suffix.into_iter().rev() {
                    canonical.push(part);
                }
                return canonical
                    .to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| GitError::invalid("Git native path is not UTF-8"));
            }
            Some(_) => return Ok(path.to_owned()),
            None => {}
        }
        suffix.push(parent.file_name().ok_or_else(GitError::stale)?.to_owned());
        if !parent.pop() {
            return Err(GitError::stale());
        }
    }
    Err(GitError::new(
        GitErrorKind::Limit,
        "Git native missing path exceeds its ancestor limit",
    ))
}

fn status_key(status: &RepositoryStatus) -> GitResult<(cli::HeadState, Vec<u8>)> {
    Ok((
        status.head.clone(),
        serde_json::to_vec(&(&status.changes, &status.untracked_directories))
            .map_err(GitError::domain)?,
    ))
}

fn next_id() -> GitResult<u64> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .map_err(|_| GitError::new(GitErrorKind::Limit, "Git request identity exhausted"))
}

fn budget(start: Instant) -> GitResult<()> {
    if start.elapsed() > Duration::from_secs(120) {
        Err(GitError::new(
            GitErrorKind::Limit,
            "Git read exceeded its total time budget",
        ))
    } else {
        Ok(())
    }
}
