use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::LazyLock;

use parking_lot::Mutex;

use super::host::{Attempt, Dispatch};
use super::*;

#[derive(Clone, Debug)]
pub enum GitWrite {
    Stage {
        paths: Vec<String>,
    },
    Unstage {
        paths: Vec<String>,
    },
    StageAll,
    UnstageAll,
    Discard {
        paths: Vec<String>,
    },
    Commit {
        message: String,
    },
    Pull,
    Push,
    WorktreeAdd {
        target: String,
        branch: GitRef,
        create_branch: bool,
        base: Option<ObjectId>,
    },
    WorktreeRemove {
        target: String,
        force: bool,
    },
    WorktreePrune,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitWritePhase {
    Validating,
    Running,
    Reconciling,
    Uncertain,
}

#[derive(Clone, Debug)]
pub struct GitBusy {
    pub operation_id: u64,
    pub phase: GitWritePhase,
    pub source: ExecutionSourceSignature,
    pub project_id: String,
    pub repository: RepositoryAuthority,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitWriteState {
    NotDispatched,
    Completed,
    Uncertain,
}

#[derive(Clone, Debug)]
pub enum GitPostcondition {
    /// Read-only facts refreshed; no registration/destruction inference.
    Refreshed,
    WorktreeCreated {
        authority: RepositoryAuthority,
        branch: GitRef,
        head: Option<ObjectId>,
    },
    WorktreeRemoved {
        path: String,
    },
    WorktreesPruned {
        paths: Vec<String>,
    },
}

#[derive(Clone)]
pub struct GitReconciliation {
    pub repository: GitRepository,
    pub status: RepositoryStatus,
    pub branches: Vec<BranchInfo>,
    pub worktrees: GitWorktrees,
    pub postcondition: GitPostcondition,
}

pub struct GitWriteOutcome {
    pub operation_id: u64,
    pub state: GitWriteState,
    pub error: Option<GitError>,
    pub reconciliation: Option<GitReconciliation>,
    pub reconciliation_error: Option<GitError>,
    pub lease_retained: bool,
    repository: GitRepository,
    operation: GitWrite,
}

impl GitWriteOutcome {
    pub fn repository(&self) -> &GitRepository {
        &self.repository
    }
    pub fn operation(&self) -> &GitWrite {
        &self.operation
    }
    pub fn succeeded(&self) -> bool {
        self.state == GitWriteState::Completed
            && self.error.is_none()
            && self.reconciliation_error.is_none()
    }
    pub fn is_current(&self, current: &GitRepository, latest_operation: u64) -> bool {
        self.operation_id == latest_operation
            && self.repository.same_source(current)
            && current.backend.check().is_ok()
    }
}

/// A refreshed inventory cannot establish that a timed-out host process has
/// stopped. The UI must obtain this explicit acknowledgement before release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UncertainReview {
    UserConfirmedOriginalOperationStopped,
}

pub struct PreparedGitWrite {
    id: u64,
    repository: GitRepository,
    operation: GitWrite,
    baseline: Baseline,
    steps: Vec<Step>,
}

impl PreparedGitWrite {
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn repository(&self) -> &GitRepository {
        &self.repository
    }
    pub fn operation(&self) -> &GitWrite {
        &self.operation
    }

    /// Consume once. Once dispatch is possible, view lifetime invalidation
    /// stops later steps but leaves this worker owning reconciliation/lease.
    pub fn execute(self) -> GitWriteOutcome {
        let mut outcome = GitWriteOutcome {
            operation_id: self.id,
            state: GitWriteState::NotDispatched,
            error: None,
            reconciliation: None,
            reconciliation_error: None,
            lease_retained: false,
            repository: self.repository.clone(),
            operation: self.operation.clone(),
        };
        let mut lease = match Lease::acquire(&self) {
            Ok(lease) => lease,
            Err(error) => {
                outcome.error = Some(error);
                return outcome;
            }
        };
        let mut uncertain = false;
        let mut effects = false;
        for (index, step) in self.steps.iter().enumerate() {
            let validation = validate_write_authority(&self.repository).and_then(|()| {
                if index == 0
                    && Baseline::capture(&self.repository, &self.operation)? != self.baseline
                {
                    return Err(GitError::changed());
                }
                if let Step::WorktreeRemove { executor, .. } = step {
                    if self
                        .repository
                        .backend
                        .authority_cli_at(&executor.worktree_root)?
                        != *executor
                    {
                        return Err(GitError::stale());
                    }
                }
                if let Some(path) = step.leaf() {
                    let expected = self
                        .baseline
                        .files
                        .get(path)
                        .ok_or_else(GitError::changed)?;
                    if &FileGuard::capture(&self.repository, path, false)? != expected {
                        return Err(GitError::changed());
                    }
                    if self.repository.status_cli()?.head != self.baseline.head {
                        return Err(GitError::changed());
                    }
                    let current_index = self.repository.command(&host::auxiliary(
                        "git",
                        &["ls-files", "--stage", "-z"],
                        cli::MAX_LIST_BYTES,
                    ))?;
                    if index_paths(&current_index)?.contains(path) {
                        return Err(GitError::changed());
                    }
                }
                Ok(())
            });
            if let Err(error) = validation {
                outcome.error = Some(error);
                break;
            }
            // Set before entering host code: unwinding/worker loss cannot
            // release a lease after a syscall or exec may have taken effect.
            lease.dispatched = true;
            lease.phase(GitWritePhase::Running);
            let (attempt, plan) = match step {
                Step::Git(plan) => (
                    self.repository
                        .backend
                        .host
                        .run(&self.repository.authority.worktree_root, plan),
                    Some(plan),
                ),
                Step::WorktreeRemove { plan, executor } => (
                    self.repository
                        .backend
                        .host
                        .run(&executor.worktree_root, plan),
                    Some(plan),
                ),
                Step::Remove(path) => (
                    self.repository
                        .backend
                        .host
                        .remove_file(&self.repository.authority.worktree_root, path),
                    None,
                ),
            };
            let dispatch = attempt.dispatch;
            if dispatch != Dispatch::NotDispatched {
                effects = true;
            }
            uncertain |= dispatch == Dispatch::Uncertain;
            let result = match plan {
                Some(plan) => attempt.checked(plan).map(|_| ()),
                None => removal_result(attempt),
            };
            if let Err(error) = result {
                outcome.error = Some(error);
                break;
            }
        }
        if !effects {
            lease.dispatched = false;
            lease.release();
            return outcome;
        }
        lease.phase(GitWritePhase::Reconciling);
        if self.repository.backend.local()
            && matches!(
                self.operation,
                GitWrite::WorktreeAdd { .. }
                    | GitWrite::WorktreeRemove { .. }
                    | GitWrite::WorktreePrune
            )
        {
            git::invalidate_repo_cache();
            worktree::invalidate(Path::new(&self.repository.authority.worktree_root));
        }
        match reconcile(
            &self.repository,
            &self.operation,
            &self.baseline,
            outcome.error.is_none(),
        ) {
            Ok(reconciliation) => outcome.reconciliation = Some(reconciliation),
            Err(error) => outcome.reconciliation_error = Some(error),
        }
        // Known exit + failed reconciliation is also quarantined. Another
        // read must not present a speculative worktree registration as fact.
        if uncertain || outcome.reconciliation_error.is_some() {
            outcome.state = GitWriteState::Uncertain;
            outcome.lease_retained = true;
            lease.phase(GitWritePhase::Uncertain);
        } else {
            outcome.state = GitWriteState::Completed;
            lease.release();
        }
        outcome
    }

    #[cfg(test)]
    pub(super) fn fixture_timeout(&mut self, timeout: Duration) {
        for step in &mut self.steps {
            match step {
                Step::Git(command) | Step::WorktreeRemove { plan: command, .. } => {
                    command.timeout = command.timeout.min(timeout)
                }
                Step::Remove(_) => {}
            }
        }
    }
}

impl GitRepository {
    /// Capture before confirmation. Confirmation may not recompute or widen
    /// the selected paths, force flag, branch, base commit or source authority.
    pub fn prepare_write(&self, mut operation: GitWrite) -> GitResult<PreparedGitWrite> {
        if self.backend.recovery_only {
            return Err(GitError::stale());
        }
        self.backend.check()?;
        if self.backend.local() {
            if let GitWrite::WorktreeAdd { target, .. } | GitWrite::WorktreeRemove { target, .. } =
                &mut operation
            {
                self.backend.validate_path(target)?;
                let path = Path::new(target);
                if path.components().any(|component| {
                    matches!(
                        component,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )
                }) {
                    return Err(GitError::invalid(
                        "Worktree target must not contain dot components",
                    ));
                }
                let parent = path
                    .parent()
                    .and_then(Path::to_str)
                    .ok_or_else(GitError::stale)?;
                let leaf = path.file_name().ok_or_else(GitError::stale)?;
                let canonical =
                    PathBuf::from(self.backend.host.canonical_directory(parent)?).join(leaf);
                *target = canonical
                    .to_str()
                    .ok_or_else(|| GitError::invalid("Git native target is not UTF-8"))?
                    .to_owned();
            }
        }
        if self.busy().is_some() {
            return Err(GitError::new(
                GitErrorKind::Busy,
                "A conflicting Git operation owns this repository",
            ));
        }
        self.validate()?;
        let baseline = Baseline::capture(self, &operation)?;
        let steps = plan_steps(self, &operation, &baseline)?;
        self.validate()?;
        Ok(PreparedGitWrite {
            id: next_id()?,
            repository: self.clone(),
            operation,
            baseline,
            steps,
        })
    }

    pub fn busy(&self) -> Option<GitBusy> {
        WRITES
            .lock()
            .get(&WriteKey::of(self))
            .map(|slot| slot.busy.clone())
    }

    /// Read-only review against the ORIGINAL repository. No implicit replay,
    /// rollback, config cleanup or release of another operation's lease.
    pub fn review_uncertain(
        &self,
        operation_id: u64,
        _: UncertainReview,
    ) -> GitResult<GitReconciliation> {
        let key = WriteKey::of(self);
        let (original, operation, baseline) = {
            let mut writes = WRITES.lock();
            let slot = writes.get_mut(&key).ok_or_else(GitError::stale)?;
            if slot.busy.operation_id != operation_id || slot.busy.phase != GitWritePhase::Uncertain
            {
                return Err(GitError::stale());
            }
            if slot.repository.authority.common_dir != self.authority.common_dir {
                return Err(GitError::stale());
            }
            slot.busy.phase = GitWritePhase::Reconciling;
            (
                slot.repository.clone(),
                slot.operation.clone(),
                slot.baseline.clone(),
            )
        };
        let result = reconcile(&original, &operation, &baseline, false);
        let mut writes = WRITES.lock();
        if let Some(slot) = writes.get_mut(&key) {
            if slot.busy.operation_id == operation_id {
                if result.is_ok() {
                    writes.remove(&key);
                } else {
                    slot.busy.phase = GitWritePhase::Uncertain;
                }
            }
        }
        result
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Baseline {
    status_bytes: Vec<u8>,
    head: cli::HeadState,
    index: Vec<u8>,
    tracked: BTreeSet<String>,
    files: BTreeMap<String, FileGuard>,
    inventory: Option<Vec<WorktreeFact>>,
    target: Option<TargetGuard>,
    branch_tip: Option<ObjectId>,
}

impl Baseline {
    fn capture(repository: &GitRepository, operation: &GitWrite) -> GitResult<Self> {
        let start = Instant::now();
        let status_bytes = repository.command(&cli::status_plan())?;
        let status = cli::parse_status(&status_bytes).map_err(GitError::domain)?;
        let index = repository.command(&host::auxiliary(
            "git",
            &["ls-files", "--stage", "-z"],
            cli::MAX_LIST_BYTES,
        ))?;
        let tracked = index_paths(&index)?;
        let mut paths = BTreeSet::new();
        match operation {
            GitWrite::Stage { paths: selected }
            | GitWrite::Unstage { paths: selected }
            | GitWrite::Discard { paths: selected } => {
                cli::stage_plan(selected).map_err(GitError::domain)?;
                for path in selected {
                    if !status.changes.iter().any(|change| {
                        change.path == *path || change.old_path.as_ref() == Some(path)
                    }) {
                        return Err(GitError::changed());
                    }
                    paths.insert(path.clone());
                    if let Some(old) = status
                        .changes
                        .iter()
                        .find(|change| change.path == *path)
                        .and_then(|change| change.old_path.clone())
                    {
                        // Rename source is captured and planned with its new
                        // name, never reconstructed from a display label.
                        paths.insert(old);
                    }
                }
            }
            GitWrite::StageAll | GitWrite::Pull => {
                for change in &status.changes {
                    paths.insert(change.path.clone());
                    paths.extend(change.old_path.clone());
                }
            }
            _ => {}
        }
        let allow_directory = matches!(operation, GitWrite::StageAll | GitWrite::Pull);
        let mut files = BTreeMap::new();
        let mut guard_bytes = 0;
        for path in paths {
            budget(start)?;
            let guard = FileGuard::capture(repository, &path, allow_directory)?;
            guard_bytes += path.len() + guard.size();
            ensure_guard_budget(guard_bytes)?;
            files.insert(path, guard);
        }
        if matches!(operation, GitWrite::StageAll) {
            for directory in &status.untracked_directories {
                budget(start)?;
                // The trailing slash remains a directory capability, never a
                // selected file or an argument to per-file mutation plans.
                let guard = FileGuard::directory(repository, directory)?;
                guard_bytes += directory.len() + guard.size();
                ensure_guard_budget(guard_bytes)?;
                files.insert(directory.clone(), guard);
            }
        }
        let mut baseline = Self {
            status_bytes,
            head: status.head,
            index,
            tracked,
            files,
            inventory: None,
            target: None,
            branch_tip: None,
        };
        match operation {
            GitWrite::Commit { message } => {
                cli::commit_plan(message).map_err(GitError::domain)?;
            }
            GitWrite::WorktreeAdd {
                target,
                branch,
                create_branch,
                base,
            } => {
                let inventory = repository.worktrees()?.scan.worktrees;
                if inventory.iter().any(|fact| {
                    fact_path(fact).ok() == Some(target.as_str())
                        || fact.branch_ref.as_deref() == Some(branch.as_str())
                }) {
                    return Err(GitError::changed());
                }
                let branches = repository.branches_cli(&repository.status_cli()?)?;
                let exists = branches
                    .iter()
                    .any(|entry| GitRef::from_branch(entry).ok().as_ref() == Some(branch));
                if exists == *create_branch {
                    return Err(GitError::changed());
                }
                if branch.is_remote() {
                    return Err(GitError::invalid("Worktrees require a local branch"));
                }
                let tip = if *create_branch {
                    base.clone().or(baseline.head.oid.clone())
                } else {
                    Some(repository.resolve(branch)?)
                };
                if let Some(tip) = &tip {
                    repository.parents(tip)?;
                }
                baseline.branch_tip = tip;
                baseline.target = Some(TargetGuard::creation(repository, target)?);
                baseline.inventory = Some(inventory);
            }
            GitWrite::WorktreeRemove { target, .. } => {
                let inventory = repository.worktrees()?.scan.worktrees;
                let fact = inventory
                    .iter()
                    .find(|fact| fact_path(fact).ok() == Some(target.as_str()))
                    .ok_or_else(GitError::changed)?;
                if fact.is_main || fact.is_bare || fact.locked.is_some() {
                    return Err(GitError::invalid("This worktree cannot be removed"));
                }
                baseline.target = Some(TargetGuard::removal(repository, target)?);
                baseline.inventory = Some(inventory);
            }
            GitWrite::WorktreePrune => {
                let inventory = repository.worktrees()?.scan.worktrees;
                if inventory
                    .iter()
                    .any(|fact| fact.path_state == WorktreePathState::Unknown)
                {
                    return Err(GitError::unavailable(
                        "Worktree absence could not be verified on its host",
                    ));
                }
                baseline.inventory = Some(inventory);
            }
            _ => {}
        }
        budget(start)?;
        if repository.command(&cli::status_plan())? != baseline.status_bytes
            || repository.command(&host::auxiliary(
                "git",
                &["ls-files", "--stage", "-z"],
                cli::MAX_LIST_BYTES,
            ))? != baseline.index
        {
            return Err(GitError::changed());
        }
        Ok(baseline)
    }
}

#[derive(Clone, PartialEq, Eq)]
enum FileGuard {
    Missing,
    Regular(ObjectId),
    Symlink(Vec<u8>),
    Directory {
        authority: RepositoryAuthority,
        head: cli::HeadState,
    },
}

impl FileGuard {
    fn size(&self) -> usize {
        match self {
            Self::Missing => 0,
            Self::Regular(oid) => oid.as_str().len(),
            Self::Symlink(bytes) => bytes.len(),
            Self::Directory { authority, .. } => {
                authority.worktree_root.len()
                    + authority.git_dir.len()
                    + authority.common_dir.len()
                    + 128
            }
        }
    }
    fn directory(repository: &GitRepository, reported: &str) -> GitResult<Self> {
        let relative = reported.strip_suffix('/').unwrap_or(reported);
        cli::validate_repo_path(relative).map_err(GitError::domain)?;
        if repository
            .backend
            .host
            .file_kind(&repository.authority.worktree_root, relative)?
            != Some(NodeKind::Directory)
        {
            return Err(GitError::changed());
        }
        let path = repository
            .backend
            .join(&repository.authority.worktree_root, relative)?;
        let authority = repository.backend.authority_at(&path)?;
        if authority.worktree_root != path {
            return Err(GitError::invalid("Directory is not an embedded repository"));
        }
        let head = cli::parse_status(&repository.backend.command(&path, &cli::status_plan())?)
            .map_err(GitError::domain)?
            .head;
        Ok(Self::Directory { authority, head })
    }
    fn capture(repository: &GitRepository, path: &str, allow_directory: bool) -> GitResult<Self> {
        cli::validate_repo_path(path).map_err(GitError::domain)?;
        repository.backend.check()?;
        let root = &repository.authority.worktree_root;
        let kind = repository.backend.host.file_kind(root, path)?;
        let guard = match kind {
            None => Self::Missing,
            Some(NodeKind::File) => {
                // No -w and no filters: inspect the exact working bytes
                // without writing objects or executing a clean filter.
                let plan = host::auxiliary("git", &["hash-object", "--no-filters", "--", path], 65);
                Self::Regular(
                    cli::parse_object_id(&repository.command(&plan)?).map_err(GitError::domain)?,
                )
            }
            Some(NodeKind::Symlink) => match repository.backend.host.read_file(root, path)? {
                OwnedContent::Bytes(bytes) => Self::Symlink(bytes),
                _ => return Err(GitError::changed()),
            },
            Some(NodeKind::Directory) if allow_directory => Self::directory(repository, path)?,
            Some(_) => {
                return Err(GitError::new(
                    GitErrorKind::Unsupported,
                    "Per-file Git operations require a file, deletion or symlink",
                ));
            }
        };
        if repository.backend.host.file_kind(root, path)? != kind {
            return Err(GitError::changed());
        }
        repository.backend.check()?;
        Ok(guard)
    }
}

#[derive(Clone, PartialEq, Eq)]
struct TargetGuard {
    path: String,
    parent: String,
    kind: Option<NodeKind>,
    authority: Option<RepositoryAuthority>,
    status: Option<Vec<u8>>,
    index: Vec<u8>,
    files: BTreeMap<String, FileGuard>,
}

impl TargetGuard {
    fn base(repository: &GitRepository, path: &str) -> GitResult<Self> {
        repository.backend.validate_path(path)?;
        let parent = repository
            .backend
            .parent(path)
            .ok_or_else(|| GitError::invalid("A filesystem root is not a worktree target"))?;
        if repository.backend.host.canonical_directory(&parent)? != parent {
            return Err(GitError::invalid(
                "Worktree parent is a symlink or changed directory",
            ));
        }
        let kind = repository.backend.host.kind(path)?;
        if !matches!(kind, None | Some(NodeKind::Directory)) {
            return Err(GitError::invalid(
                "Worktree target is not an absent path or real directory",
            ));
        }
        if kind.is_some() && repository.backend.host.canonical_directory(path)? != path {
            return Err(GitError::stale());
        }
        repository.backend.check()?;
        Ok(Self {
            path: path.to_owned(),
            parent,
            kind,
            authority: None,
            status: None,
            index: Vec::new(),
            files: BTreeMap::new(),
        })
    }
    fn creation(repository: &GitRepository, path: &str) -> GitResult<Self> {
        let target = Self::base(repository, path)?;
        if target.kind.is_some() && !repository.backend.host.directory_empty(path)? {
            return Err(GitError::invalid("Worktree target directory is not empty"));
        }
        Ok(target)
    }
    fn removal(repository: &GitRepository, path: &str) -> GitResult<Self> {
        let mut target = Self::base(repository, path)?;
        if target.kind.is_none() {
            return Err(GitError::invalid(
                "Missing worktrees require inventory pruning",
            ));
        }
        let child = repository.backend.repository_at(path)?;
        if child.authority.common_dir != repository.authority.common_dir
            || !child.authority.is_linked_worktree()
        {
            return Err(GitError::stale());
        }
        let bytes = child.command(&cli::status_plan())?;
        let status = cli::parse_status(&bytes).map_err(GitError::domain)?;
        if !status.untracked_directories.is_empty() {
            return Err(GitError::new(
                GitErrorKind::Unsupported,
                "Worktree contains an embedded untracked repository",
            ));
        }
        target.index = child.command(&host::auxiliary(
            "git",
            &["ls-files", "--stage", "-z"],
            cli::MAX_LIST_BYTES,
        ))?;
        let mut paths = index_paths(&target.index)?;
        for change in status.changes {
            paths.insert(change.path);
            paths.extend(change.old_path);
        }
        let ignored = child.command(&host::auxiliary(
            "git",
            &[
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-standard",
                "-z",
            ],
            cli::MAX_LIST_BYTES,
        ))?;
        for path in nul_paths(&ignored)? {
            paths.insert(path);
        }
        if paths.len() > cli::MAX_RECORDS {
            return Err(GitError::new(
                GitErrorKind::Limit,
                "Worktree removal exceeds its file limit",
            ));
        }
        let start = Instant::now();
        let mut guard_bytes = 0;
        for path in paths {
            budget(start)?;
            let guard = FileGuard::capture(&child, &path, false)?;
            guard_bytes += path.len() + guard.size();
            ensure_guard_budget(guard_bytes)?;
            target.files.insert(path, guard);
        }
        if child.command(&cli::status_plan())? != bytes
            || child.command(&host::auxiliary(
                "git",
                &["ls-files", "--stage", "-z"],
                cli::MAX_LIST_BYTES,
            ))? != target.index
        {
            return Err(GitError::changed());
        }
        target.authority = Some(child.authority);
        target.status = Some(bytes);
        Ok(target)
    }
}

enum Step {
    Git(GitCommand),
    Remove(String),
    WorktreeRemove {
        plan: GitCommand,
        executor: RepositoryAuthority,
    },
}
impl Step {
    fn leaf(&self) -> Option<&str> {
        match self {
            Self::Remove(path) => Some(path),
            _ => None,
        }
    }
}

fn plan_steps(
    repository: &GitRepository,
    operation: &GitWrite,
    baseline: &Baseline,
) -> GitResult<Vec<Step>> {
    let head = baseline.head.oid.as_ref();
    let selected: Vec<String> = baseline.files.keys().cloned().collect();
    let plan = match operation {
        GitWrite::Stage { .. } => cli::stage_plan(&selected).map_err(GitError::domain)?,
        GitWrite::Unstage { .. } => cli::unstage_plan(head, &selected).map_err(GitError::domain)?,
        GitWrite::StageAll => cli::stage_all_plan(),
        GitWrite::UnstageAll => cli::unstage_all_plan(head),
        GitWrite::Discard { .. } => {
            let mut steps = Vec::new();
            let mut tracked = Vec::new();
            for path in &selected {
                let in_head = if let Some(head) = head {
                    let plan = cli::tree_entry_plan(head, path).map_err(GitError::domain)?;
                    cli::parse_tree_entry(&repository.command(&plan)?, path)
                        .map_err(GitError::domain)?
                        .is_some()
                } else {
                    false
                };
                if in_head || baseline.tracked.contains(path) {
                    tracked.push(path.clone());
                } else {
                    match cli::discard_plan(path, head, false).map_err(GitError::domain)? {
                        cli::DiscardPlan::RemoveUntrackedFile { path } => {
                            steps.push(Step::Remove(path))
                        }
                        cli::DiscardPlan::Git(_) => {
                            return Err(GitError::invalid("Unexpected tracked discard plan"));
                        }
                    }
                }
            }
            // A rename must restore old + new in one Git operation. Extending
            // the validated domain plan retains its exact literal path mode.
            if let Some(first) = tracked.first() {
                let cli::DiscardPlan::Git(mut plan) =
                    cli::discard_plan(first, head, true).map_err(GitError::domain)?
                else {
                    return Err(GitError::invalid("Missing tracked discard plan"));
                };
                plan.args.extend(tracked.into_iter().skip(1));
                steps.insert(0, Step::Git(plan));
            }
            return Ok(steps);
        }
        GitWrite::Commit { message } => cli::commit_plan(message).map_err(GitError::domain)?,
        GitWrite::Pull => cli::pull_plan(),
        GitWrite::Push => cli::push_plan(),
        GitWrite::WorktreeAdd {
            target,
            branch,
            create_branch,
            base,
        } => native_target_plan(repository, target, |target| {
            cli::worktree_add_plan(target, branch, *create_branch, base.as_ref())
        })?,
        GitWrite::WorktreeRemove { target, force } => {
            let main = baseline
                .inventory
                .as_ref()
                .and_then(|facts| facts.iter().find(|fact| fact.is_main))
                .ok_or_else(GitError::changed)?;
            if fact_path(main)? == target {
                return Err(GitError::invalid("Cannot remove the main worktree"));
            }
            let executor_path = if main.is_bare {
                baseline
                    .inventory
                    .as_ref()
                    .and_then(|facts| {
                        facts.iter().find(|fact| {
                            !fact.is_bare
                                && fact_path(fact).ok() != Some(target.as_str())
                                && fact.path_state == WorktreePathState::Present
                        })
                    })
                    .map(fact_path)
                    .transpose()?
                    .ok_or_else(|| {
                        GitError::new(
                            GitErrorKind::Unsupported,
                            "Removing this worktree requires a surviving non-bare worktree",
                        )
                    })?
            } else {
                fact_path(main)?
            };
            let executor = repository.backend.authority_at(executor_path)?;
            if executor.common_dir != repository.authority.common_dir
                || executor.worktree_root != executor_path
            {
                return Err(GitError::stale());
            }
            let main_path = if repository.backend.local() && cfg!(windows) {
                "/__git_main_worktree__"
            } else {
                fact_path(main)?
            };
            let plan = native_target_plan(repository, target, |target| {
                cli::worktree_remove_plan(target, main_path, *force)
            })?;
            return Ok(vec![Step::WorktreeRemove { plan, executor }]);
        }
        GitWrite::WorktreePrune => cli::worktree_prune_plan(),
    };
    Ok(vec![Step::Git(plan)])
}

fn native_target_plan(
    repository: &GitRepository,
    target: &str,
    build: impl FnOnce(&str) -> anyhow::Result<GitCommand>,
) -> GitResult<GitCommand> {
    repository.backend.validate_path(target)?;
    if repository.backend.local() && cfg!(windows) {
        let placeholder = "/__git_native_target__";
        let mut plan = build(placeholder).map_err(GitError::domain)?;
        let slot = plan
            .args
            .iter_mut()
            .find(|argument| argument.as_str() == placeholder)
            .ok_or_else(|| GitError::invalid("Git plan target is missing"))?;
        *slot = target.to_owned();
        Ok(plan)
    } else {
        build(target).map_err(GitError::domain)
    }
}

fn index_paths(bytes: &[u8]) -> GitResult<BTreeSet<String>> {
    if bytes.is_empty() {
        return Ok(BTreeSet::new());
    }
    if !bytes.is_empty() && !bytes.ends_with(&[0]) {
        return Err(GitError::invalid("Incomplete Git index inventory"));
    }
    let mut paths = BTreeSet::new();
    let mut stages: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    let mut width = None;
    let body = bytes.strip_suffix(&[0]).unwrap_or(bytes);
    if body.is_empty() {
        return Err(GitError::invalid("Empty Git index record"));
    }
    for (count, record) in body.split(|byte| *byte == 0).enumerate() {
        if count >= cli::MAX_RECORDS {
            return Err(GitError::new(
                GitErrorKind::Limit,
                "Too many Git index records",
            ));
        }
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| GitError::invalid("Malformed Git index record"))?;
        let header = std::str::from_utf8(&record[..tab]).map_err(GitError::domain)?;
        let fields: Vec<_> = header.split(' ').take(4).collect();
        if fields.len() != 3
            || !["100644", "100755", "120000", "160000"].contains(&fields[0])
            || !["0", "1", "2", "3"].contains(&fields[2])
            || ![40, 64].contains(&fields[1].len())
            || !fields[1]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(GitError::invalid("Malformed Git index metadata"));
        }
        if width.is_some_and(|width| width != fields[1].len()) {
            return Err(GitError::invalid("Mixed Git index object formats"));
        }
        width = Some(fields[1].len());
        let path = std::str::from_utf8(&record[tab + 1..]).map_err(GitError::domain)?;
        cli::validate_repo_path(path).map_err(GitError::domain)?;
        let path_stages = stages.entry(path.to_owned()).or_default();
        if !path_stages.insert(fields[2]) || path_stages.len() > 1 && path_stages.contains("0") {
            return Err(GitError::invalid(
                "Duplicate or inconsistent Git index stages",
            ));
        }
        paths.insert(path.to_owned());
    }
    Ok(paths)
}

fn nul_paths(bytes: &[u8]) -> GitResult<Vec<String>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let body = bytes
        .strip_suffix(&[0])
        .ok_or_else(|| GitError::invalid("Incomplete Git filename inventory"))?;
    let mut paths = Vec::new();
    for field in body.split(|byte| *byte == 0) {
        let path = std::str::from_utf8(field).map_err(GitError::domain)?;
        cli::validate_repo_path(path).map_err(GitError::domain)?;
        if paths.len() >= cli::MAX_RECORDS {
            return Err(GitError::new(GitErrorKind::Limit, "Too many Git filenames"));
        }
        paths.push(path.to_owned());
    }
    Ok(paths)
}

fn ensure_guard_budget(bytes: usize) -> GitResult<()> {
    if bytes > cli::MAX_LIST_BYTES {
        Err(GitError::new(
            GitErrorKind::Limit,
            "Git confirmation baseline exceeds its byte limit",
        ))
    } else {
        Ok(())
    }
}

fn removal_result(attempt: Attempt) -> GitResult<()> {
    if let Some(error) = attempt.error {
        return Err(error);
    }
    if attempt.dispatch != Dispatch::Completed
        || attempt.output.as_ref().and_then(|output| output.exit_code) != Some(0)
    {
        return Err(GitError::unavailable(
            "Git file removal did not confirm completion",
        ));
    }
    Ok(())
}

fn validate_write_authority(repository: &GitRepository) -> GitResult<()> {
    repository.validate()?;
    // libgit2 native reads do not consult process Git environment overrides.
    // A mutation must also prove the actual CLI resolves this exact source.
    if repository.backend.local()
        && repository
            .backend
            .authority_cli_at(&repository.authority.worktree_root)?
            != repository.authority
    {
        return Err(GitError::stale());
    }
    repository.backend.check()
}

fn reconcile(
    original: &GitRepository,
    operation: &GitWrite,
    baseline: &Baseline,
    successful_exit: bool,
) -> GitResult<GitReconciliation> {
    // This explicit read may reconnect. It never changes the captured write's
    // epoch and never authorizes a retry on the replacement connection.
    let mut backend = GitBackend {
        host: original.backend.host.clone(),
        lifetime: GitLifetime::new(),
        anchor: original.backend.anchor.clone(),
        probe_anchor: original.backend.probe_anchor.clone(),
        recovery_only: false,
    };
    if backend.check().is_err() {
        let mut snapshot = original.backend.snapshot().clone();
        if let ExecutionBackend::Ssh {
            connection,
            connection_fingerprint,
            connection_epoch,
        } = &mut snapshot.backend
        {
            *connection_epoch = Some(
                remote_ssh::git_connection_epoch(connection, *connection_fingerprint)
                    .map_err(GitError::unavailable)?,
            );
        }
        backend.host = Arc::new(ExecutionGitHost(snapshot));
        backend.check()?;
    }
    let mut root = original.authority.worktree_root.clone();
    if let GitWrite::WorktreeRemove { target, .. } = operation {
        if backend.host.kind(target)?.is_none()
            && (root == *target || backend.host.kind(&backend.probe_anchor)?.is_none())
        {
            let survivor = baseline
                .inventory
                .as_deref()
                .unwrap_or_default()
                .iter()
                .find(|fact| {
                    !fact.is_bare
                        && fact_path(fact).ok() != Some(target.as_str())
                        && fact.path_state == WorktreePathState::Present
                })
                .ok_or_else(|| {
                    GitError::unavailable(
                        "No verified surviving worktree for removal reconciliation",
                    )
                })?;
            root = fact_path(survivor)?.to_owned();
            backend.anchor = root.clone();
            backend.probe_anchor = root.clone();
            // This handle only transports reconciliation facts. Its original
            // project/worktree identity cannot authorize new reads or writes.
            backend.recovery_only = true;
        }
    }
    let repository = backend.repository_at(&root)?;
    if repository.authority.common_dir != original.authority.common_dir
        || !backend.recovery_only && repository.authority != original.authority
    {
        return Err(GitError::stale());
    }
    let status = repository.status_cli()?;
    let branches = repository.branches_cli(&status)?;
    let worktrees = repository.worktrees()?;
    let postcondition = match operation {
        GitWrite::WorktreeAdd { target, branch, .. } => {
            let fact = worktrees
                .scan
                .worktrees
                .iter()
                .find(|fact| fact_path(fact).ok() == Some(target.as_str()));
            if let Some(fact) = fact {
                if fact.is_main
                    || fact.branch_ref.as_deref() != Some(branch.as_str())
                    || fact.path_state != WorktreePathState::Present
                {
                    return Err(GitError::changed());
                }
                let authority = repository.backend.authority_at(target)?;
                let head =
                    cli::parse_status(&repository.backend.command(target, &cli::status_plan())?)
                        .map_err(GitError::domain)?
                        .head;
                if authority.worktree_root != *target
                    || authority.common_dir != original.authority.common_dir
                    || !authority.is_linked_worktree()
                    || head.branch.as_ref() != Some(branch)
                    || head.oid != baseline.branch_tip
                {
                    return Err(GitError::changed());
                }
                GitPostcondition::WorktreeCreated {
                    authority,
                    branch: branch.clone(),
                    head: head.oid,
                }
            } else if successful_exit {
                return Err(GitError::changed());
            } else {
                GitPostcondition::Refreshed
            }
        }
        GitWrite::WorktreeRemove { target, .. } => {
            let absent = !worktrees
                .scan
                .worktrees
                .iter()
                .any(|fact| fact_path(fact).ok() == Some(target.as_str()))
                && repository.backend.host.kind(target)?.is_none();
            if absent {
                GitPostcondition::WorktreeRemoved {
                    path: target.clone(),
                }
            } else if successful_exit {
                return Err(GitError::changed());
            } else {
                GitPostcondition::Refreshed
            }
        }
        GitWrite::WorktreePrune => {
            let mut paths = Vec::new();
            for fact in baseline.inventory.as_deref().unwrap_or_default() {
                let path = fact_path(fact)?;
                if !fact.is_main
                    && !worktrees
                        .scan
                        .worktrees
                        .iter()
                        .any(|row| fact_path(row).ok() == Some(path))
                {
                    if repository.backend.host.kind(path)?.is_some() {
                        return Err(GitError::changed());
                    }
                    paths.push(path.to_owned());
                }
            }
            GitPostcondition::WorktreesPruned { paths }
        }
        _ => GitPostcondition::Refreshed,
    };
    repository.validate()?;
    Ok(GitReconciliation {
        repository,
        status,
        branches,
        worktrees,
        postcondition,
    })
}

// Common-dir coordination deliberately excludes project, worktree, view,
// credentials and epoch: reconnecting must not bypass an earlier host write.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct WriteKey {
    host: mt_identity::ExecutionHostId,
    backend: String,
    common_dir: String,
}
impl WriteKey {
    fn of(repository: &GitRepository) -> Self {
        let backend = match &repository.backend.snapshot().backend {
            ExecutionBackend::Local => "local".to_owned(),
            ExecutionBackend::Wsl { distro } => format!("wsl:{}", distro.to_ascii_lowercase()),
            ExecutionBackend::Ssh { .. } => "ssh".to_owned(),
        };
        let common_dir = if repository.backend.local() {
            worktree::normalize_path_for_comparison(&repository.authority.common_dir)
        } else {
            repository.authority.common_dir.clone()
        };
        Self {
            host: repository.backend.snapshot().execution_host_id.clone(),
            backend,
            common_dir,
        }
    }
}

struct Slot {
    busy: GitBusy,
    repository: GitRepository,
    operation: GitWrite,
    baseline: Baseline,
}
static WRITES: LazyLock<Mutex<HashMap<WriteKey, Slot>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct Lease {
    key: WriteKey,
    id: u64,
    dispatched: bool,
    released: bool,
}
impl Lease {
    fn acquire(prepared: &PreparedGitWrite) -> GitResult<Self> {
        let key = WriteKey::of(&prepared.repository);
        let mut writes = WRITES.lock();
        if writes.contains_key(&key) {
            return Err(GitError::new(
                GitErrorKind::Busy,
                "A conflicting Git operation owns this repository",
            ));
        }
        writes.insert(
            key.clone(),
            Slot {
                busy: GitBusy {
                    operation_id: prepared.id,
                    phase: GitWritePhase::Validating,
                    source: prepared.repository.backend.snapshot().source_signature(),
                    project_id: prepared.repository.backend.snapshot().project_id.clone(),
                    repository: prepared.repository.authority.clone(),
                },
                repository: prepared.repository.clone(),
                operation: prepared.operation.clone(),
                baseline: prepared.baseline.clone(),
            },
        );
        Ok(Self {
            key,
            id: prepared.id,
            dispatched: false,
            released: false,
        })
    }
    fn phase(&self, phase: GitWritePhase) {
        if let Some(slot) = WRITES.lock().get_mut(&self.key) {
            if slot.busy.operation_id == self.id {
                slot.busy.phase = phase;
            }
        }
    }
    fn release(&mut self) {
        let mut writes = WRITES.lock();
        if writes
            .get(&self.key)
            .is_some_and(|slot| slot.busy.operation_id == self.id)
        {
            writes.remove(&self.key);
        }
        self.released = true;
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if !self.released {
            if self.dispatched {
                self.phase(GitWritePhase::Uncertain);
            } else {
                self.release();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_host_lease_survives_ssh_alias_epoch_and_worktree_changes() {
        use mt_identity::{ExecutionHostId, HostInstallId, RepoId, WorktreeId};

        let host = ExecutionHostId::derive("lease-fixture", &HostInstallId::new());
        let repo = RepoId::derive(&host, "/repo/.git");
        let snapshot = ProjectExecutionSnapshot {
            project_id: "original-project".into(),
            root_project_id: "original-root".into(),
            worktree_id: WorktreeId::derive(&repo, "/repo", None),
            execution_host_id: host,
            canonical_path: "/repo".into(),
            root_source_path: "/repo".into(),
            host_label: "fixture".into(),
            backend: ExecutionBackend::Ssh {
                connection: mt_config::SshConnection {
                    id: "first-alias".into(),
                    name: "fixture".into(),
                    host: "127.0.0.1".into(),
                    port: 2222,
                    user: "fixture".into(),
                    password: None,
                    identity_file: None,
                    group: None,
                },
                connection_fingerprint: 1,
                connection_epoch: Some(1),
            },
        };
        let repository = GitRepository {
            backend: GitBackend {
                host: Arc::new(ExecutionGitHost(snapshot.clone())),
                lifetime: GitLifetime::new(),
                anchor: "/repo".into(),
                probe_anchor: "/repo".into(),
                recovery_only: false,
            },
            authority: RepositoryAuthority {
                worktree_root: "/repo".into(),
                git_dir: "/repo/.git".into(),
                common_dir: "/repo/.git".into(),
            },
            info: git::GitRepoInfo {
                name: "repo".into(),
                path: "/repo".into(),
                current_branch: None,
                is_worktree: false,
            },
        };
        let mut prepared = PreparedGitWrite {
            id: next_id().unwrap(),
            repository,
            operation: GitWrite::Push,
            steps: Vec::new(),
            baseline: Baseline {
                status_bytes: Vec::new(),
                head: cli::HeadState {
                    oid: None,
                    branch: None,
                },
                index: Vec::new(),
                tracked: BTreeSet::new(),
                files: BTreeMap::new(),
                inventory: None,
                target: None,
                branch_tip: None,
            },
        };
        let mut lease = Lease::acquire(&prepared).unwrap();
        lease.phase(GitWritePhase::Uncertain);
        let mut alias = snapshot;
        alias.project_id = "other-project".into();
        alias.root_project_id = "other-root".into();
        alias.canonical_path = "/linked".into();
        alias.worktree_id = WorktreeId::derive(&repo, "/linked", None);
        if let ExecutionBackend::Ssh {
            connection,
            connection_fingerprint,
            connection_epoch,
        } = &mut alias.backend
        {
            connection.id = "second-alias".into();
            connection.identity_file = Some("/fixture/replacement-key".into());
            *connection_fingerprint = 2;
            *connection_epoch = Some(2);
        }
        prepared.repository.backend.host = Arc::new(ExecutionGitHost(alias));
        prepared.repository.backend.lifetime = GitLifetime::new();
        prepared.repository.authority.worktree_root = "/linked".into();
        prepared.repository.authority.git_dir = "/repo/.git/worktrees/linked".into();
        let busy = prepared.repository.busy().unwrap();
        assert_eq!(busy.operation_id, prepared.id);
        assert_eq!(busy.project_id, "original-project");
        assert_eq!(busy.repository.worktree_root, "/repo");
        assert!(matches!(
            Lease::acquire(&prepared),
            Err(GitError {
                kind: GitErrorKind::Busy,
                ..
            })
        ));
        lease.release();
        assert!(prepared.repository.busy().is_none());
    }

    #[test]
    fn confirmation_index_framing_rejects_empty_duplicate_mixed_and_partial_records() {
        let oid = "1111111111111111111111111111111111111111";
        let row = format!("100644 {oid} 0\tline\nname\0");
        assert!(index_paths(&[]).unwrap().is_empty());
        assert!(index_paths(b"\0").is_err());
        assert!(index_paths(row.trim_end_matches('\0').as_bytes()).is_err());
        assert!(index_paths(format!("{row}{row}").as_bytes()).is_err());
        assert!(index_paths(format!("{row}100644 {oid} 1\tline\nname\0").as_bytes()).is_err());
        assert!(
            index_paths(format!("{row}100644 {} 0\tother\0", "2".repeat(64)).as_bytes()).is_err()
        );
        assert!(index_paths(row.as_bytes()).unwrap().contains("line\nname"));
    }
}
