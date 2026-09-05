//! Shared Local/WSL/SSH Git worktree inventory for navigation surfaces.
//!
//! The configured top-level folder is always the command cwd. This module
//! never walks the filesystem or enumerates repositories outside that anchor.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::time::Duration;

use gpui::{App, Context, Entity, Global, Task, Window};
use mt_config::{HiddenWorktree, ProjectConfig, ProjectTreeItem, WorktreeVisibilitySource};
use mt_github::{CommandOutput, CommandPlan};
use mt_identity::ExecutionHostId;
use mt_project::worktree::{
    WorktreeFact, WorktreePathSemantics, WorktreePathState, WorktreePorcelainMode, WorktreeScan,
    WorktreeScanSource,
};

use crate::execution_host::{
    ExecutionBackend, ExecutionSourceSignature, ProjectExecutionSnapshot,
    configured_execution_path, execute_host_command, normalize_absolute_posix_path,
    normalize_host_visible_project_path, wsl_host_visible_path,
};
use crate::store::{AppStore, ProjectLocationKey, ProjectPlacement, ProjectRegistrationOutcome};

const SCAN_TIMEOUT: Duration = Duration::from_secs(30);
const OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
const REMOTE_POLL_INTERVAL: Duration = Duration::from_secs(10);
const MAX_CONCURRENT_SCANS: usize = 4;
const WARNING_LIMIT: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CatalogBackend {
    Local,
    Wsl { distro: String },
    Ssh { connection_id: String },
}

impl CatalogBackend {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Local => "Local",
            Self::Wsl { .. } => "WSL",
            Self::Ssh { .. } => "SSH",
        }
    }

    fn is_remote(&self) -> bool {
        !matches!(self, Self::Local)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WorktreeCatalogOwner {
    pub source: ExecutionSourceSignature,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WorktreeCatalogTarget {
    pub root_project_id: String,
    pub row_key: String,
    pub root_config_key: String,
    pub configured_project_id: Option<String>,
    pub host_visible_path: String,
    pub execution_path: String,
    pub suggested_name: String,
    pub backend: CatalogBackend,
    pub owner: Option<WorktreeCatalogOwner>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeCatalogRow {
    pub target: WorktreeCatalogTarget,
    pub visibility_key: Option<HiddenWorktree>,
    pub configured_visibility_key: Option<HiddenWorktree>,
    pub label: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub is_main: bool,
    pub is_detached: bool,
    pub is_bare: bool,
    pub is_sparse: bool,
    pub is_locked: bool,
    pub is_prunable: bool,
    pub locked_reason: Option<String>,
    pub prunable_reason: Option<String>,
    pub path_state: WorktreePathState,
    pub authoritative: bool,
    pub last_known: bool,
    pub selectable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectWorktreeGroup {
    pub root_project_id: String,
    pub root_project_name: String,
    pub root_project_path: String,
    pub execution_host_id: Option<ExecutionHostId>,
    pub host_label: String,
    pub backend: CatalogBackend,
    pub warning: Option<String>,
    pub refreshing: bool,
    pub rows: Vec<WorktreeCatalogRow>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ScanTargetKey {
    source: ExecutionSourceSignature,
    local_generation: Option<u64>,
}

#[derive(Clone, Debug)]
struct ScanTarget {
    root_project_id: String,
    root_config_key: String,
    snapshot: ProjectExecutionSnapshot,
    backend: CatalogBackend,
    local_generation: Option<u64>,
    visibility_source: Option<WorktreeVisibilitySource>,
}

impl ScanTarget {
    fn key(&self) -> ScanTargetKey {
        ScanTargetKey {
            source: self.snapshot.source_signature(),
            local_generation: self.local_generation,
        }
    }

    fn owner(&self, revision: u64, observed_epoch: Option<u64>) -> WorktreeCatalogOwner {
        WorktreeCatalogOwner {
            source: self.snapshot.observed_source_signature(observed_epoch),
            revision,
        }
    }
}

#[derive(Clone)]
enum CatalogInventory {
    Git(WorktreeScan),
    NonGit,
}

#[derive(Clone)]
struct CatalogSnapshot {
    target: ScanTarget,
    owner: WorktreeCatalogOwner,
    inventory: CatalogInventory,
    warning: Option<String>,
}

#[derive(Default)]
struct CatalogEntry {
    root_config_key: String,
    target_generation: u64,
    desired: Option<ScanTarget>,
    snapshot: Option<CatalogSnapshot>,
    warning: Option<String>,
    in_flight_revision: Option<u64>,
    dirty: bool,
}

impl CatalogEntry {
    fn begin_scan(&mut self, revision: u64) {
        self.in_flight_revision = Some(revision);
        self.dirty = false;
    }
}

fn enqueue_scan_once(queue: &mut VecDeque<String>, root_project_id: &str) {
    if !queue.iter().any(|queued| queued == root_project_id) {
        queue.push_back(root_project_id.to_string());
    }
}

fn request_scan(entry: &mut CatalogEntry, queue: &mut VecDeque<String>, root_project_id: &str) {
    if entry.in_flight_revision.is_some() {
        entry.dirty = true;
    } else {
        enqueue_scan_once(queue, root_project_id);
    }
}

fn queue_dirty_rerun(
    entry: &mut CatalogEntry,
    queue: &mut VecDeque<String>,
    root_project_id: &str,
) {
    if !entry.dirty {
        return;
    }
    entry.dirty = false;
    enqueue_scan_once(queue, root_project_id);
}

fn scan_capacity_available(in_flight: usize) -> bool {
    in_flight < MAX_CONCURRENT_SCANS
}

fn target_generation_matches(entry: &CatalogEntry, target_generation: u64) -> bool {
    entry.target_generation == target_generation
}

enum ScanInventoryResult {
    Git(WorktreeScan),
    NonGit,
}

struct ScanTaskResult {
    inventory: ScanInventoryResult,
    observed_connection_epoch: Option<u64>,
}

struct ScanCompletion {
    target: ScanTarget,
    target_generation: u64,
    revision: u64,
    result: Result<ScanTaskResult, String>,
}

struct GlobalWorktreeCatalog(Entity<WorktreeCatalog>);
impl Global for GlobalWorktreeCatalog {}

pub fn install(catalog: Entity<WorktreeCatalog>, cx: &mut App) {
    cx.set_global(GlobalWorktreeCatalog(catalog));
}

pub fn global(cx: &App) -> Option<Entity<WorktreeCatalog>> {
    cx.try_global::<GlobalWorktreeCatalog>()
        .map(|global| global.0.clone())
}

pub fn force_refresh_global(cx: &mut App) {
    if let Some(catalog) = global(cx) {
        catalog.update(cx, |catalog, cx| catalog.force_refresh(cx));
    }
}

pub struct WorktreeCatalog {
    store: Entity<AppStore>,
    entries: HashMap<String, CatalogEntry>,
    queue: VecDeque<String>,
    in_flight: usize,
    next_target_generation: u64,
    next_revision: u64,
    was_focused: bool,
    _poll_task: Task<()>,
}

impl WorktreeCatalog {
    pub fn new(store: Entity<AppStore>, cx: &mut Context<Self>) -> Self {
        cx.observe(&store, |this, _, cx| {
            let focused = this.store.read(cx).window_focused();
            let regained_focus = focused && !this.was_focused;
            this.was_focused = focused;
            this.refresh(regained_focus, false, cx);
            cx.notify();
        })
        .detach();

        let poll_task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REMOTE_POLL_INTERVAL).await;
                let Ok(()) = this.update(cx, |catalog: &mut WorktreeCatalog, cx| {
                    if catalog.store.read(cx).window_focused() {
                        catalog.refresh(true, true, cx);
                    }
                }) else {
                    return;
                };
            }
        });

        let was_focused = store.read(cx).window_focused();
        let mut catalog = Self {
            store,
            entries: HashMap::new(),
            queue: VecDeque::new(),
            in_flight: 0,
            next_target_generation: 0,
            next_revision: 0,
            was_focused,
            _poll_task: poll_task,
        };
        catalog.refresh(true, false, cx);
        catalog
    }

    pub fn force_refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh(true, false, cx);
    }

    pub fn groups(&self, cx: &App) -> Vec<ProjectWorktreeGroup> {
        build_groups(self.store.read(cx), &self.entries)
    }

    pub fn resolve_target(
        &self,
        target: &WorktreeCatalogTarget,
        cx: &App,
    ) -> Option<WorktreeCatalogRow> {
        let row = self
            .groups(cx)
            .into_iter()
            .find(|group| group.root_project_id == target.root_project_id)
            .and_then(|group| group.rows.into_iter().find(|row| row.target == *target))?;
        if let Some(owner) = row.target.owner.as_ref() {
            let store = self.store.read(cx);
            let current = store
                .project(&row.target.root_project_id)
                .and_then(|project| build_scan_target(store, project).ok())?;
            let snapshot = self
                .entries
                .get(&row.target.root_project_id)
                .and_then(|entry| entry.snapshot.as_ref())?;
            if !snapshot_owner_matches_current(
                snapshot,
                owner,
                &row.target.root_config_key,
                &current,
            ) {
                return None;
            }
        }
        Some(row)
    }

    fn refresh(&mut self, force: bool, remote_only: bool, cx: &mut Context<Self>) {
        let roots = {
            let store = self.store.read(cx);
            ordered_top_level_project_ids(store.projects(), store.config().project_tree.as_deref())
                .into_iter()
                .filter_map(|root_project_id| {
                    let project = store.project(&root_project_id)?.clone();
                    let config_key = root_config_key(&project);
                    let target = build_scan_target(store, &project);
                    Some((root_project_id, config_key, target))
                })
                .collect::<Vec<_>>()
        };
        let live_roots: HashSet<String> = roots
            .iter()
            .map(|(root_project_id, _, _)| root_project_id.clone())
            .collect();
        self.entries
            .retain(|root_project_id, _| live_roots.contains(root_project_id));
        self.queue
            .retain(|root_project_id| live_roots.contains(root_project_id));

        for (root_project_id, config_key, target) in roots {
            let entry = self.entries.entry(root_project_id.clone()).or_default();
            let config_changed = entry.root_config_key != config_key;
            let desired_changed = entry.desired.as_ref().map(ScanTarget::key)
                != target.as_ref().ok().map(ScanTarget::key);
            if config_changed {
                let in_flight = entry.in_flight_revision;
                let target_generation = entry.target_generation;
                *entry = CatalogEntry {
                    root_config_key: config_key,
                    target_generation,
                    in_flight_revision: in_flight,
                    dirty: in_flight.is_some(),
                    ..CatalogEntry::default()
                };
            }
            if config_changed || desired_changed {
                let Some(generation) = self.next_target_generation.checked_add(1) else {
                    entry.desired = None;
                    entry.warning = Some("worktree catalog target generation is exhausted".into());
                    continue;
                };
                self.next_target_generation = generation;
                entry.target_generation = generation;
            }
            match target {
                Ok(target) => {
                    let target_changed = desired_changed;
                    let eligible_force = force && (!remote_only || target.backend.is_remote());
                    entry.desired = Some(target);
                    if target_changed {
                        entry.warning = None;
                        mark_snapshot_last_known(
                            entry.snapshot.as_mut(),
                            "worktree inventory source changed",
                        );
                    }
                    if target_changed || eligible_force || entry.snapshot.is_none() {
                        request_scan(entry, &mut self.queue, &root_project_id);
                    }
                }
                Err(error) => {
                    entry.desired = None;
                    entry.warning = Some(bounded_warning(&error));
                    mark_snapshot_last_known(entry.snapshot.as_mut(), &error);
                    if entry.in_flight_revision.is_some() {
                        entry.dirty = true;
                    }
                }
            }
        }
        self.start_queued(cx);
        cx.notify();
    }

    fn start_queued(&mut self, cx: &mut Context<Self>) {
        while scan_capacity_available(self.in_flight) {
            let Some(root_project_id) = self.queue.pop_front() else {
                break;
            };
            let Some(entry) = self.entries.get_mut(&root_project_id) else {
                continue;
            };
            if entry.in_flight_revision.is_some() {
                entry.dirty = true;
                continue;
            }
            let Some(target) = entry.desired.clone() else {
                continue;
            };
            let target_generation = entry.target_generation;
            let Some(revision) = self.next_revision.checked_add(1) else {
                entry.warning = Some("worktree catalog revision counter is exhausted".into());
                continue;
            };
            self.next_revision = revision;
            self.in_flight += 1;
            entry.begin_scan(revision);

            cx.spawn(async move |this, cx| {
                let task_target = target.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { scan_target(&task_target, revision) })
                    .await;
                let _ = this.update(cx, |catalog: &mut WorktreeCatalog, cx| {
                    catalog.finish_scan(
                        ScanCompletion {
                            target,
                            target_generation,
                            revision,
                            result,
                        },
                        cx,
                    );
                });
            })
            .detach();
        }
    }

    fn finish_scan(&mut self, completion: ScanCompletion, cx: &mut Context<Self>) {
        let current_target = {
            let store = self.store.read(cx);
            store
                .project(&completion.target.root_project_id)
                .and_then(|project| build_scan_target(store, project).ok())
        };
        let current_target_exists = current_target.is_some();
        let Some(entry) = self.entries.get_mut(&completion.target.root_project_id) else {
            self.in_flight = self.in_flight.saturating_sub(1);
            self.start_queued(cx);
            return;
        };
        if entry.in_flight_revision != Some(completion.revision) {
            self.in_flight = self.in_flight.saturating_sub(1);
            self.start_queued(cx);
            return;
        }
        entry.in_flight_revision = None;
        self.in_flight = self.in_flight.saturating_sub(1);

        let mut retry_stale = !target_generation_matches(entry, completion.target_generation);
        if !retry_stale {
            match completion.result {
                Ok(result) => {
                    let observed_owner = completion
                        .target
                        .owner(completion.revision, result.observed_connection_epoch);
                    let accepted_target = current_target
                        .as_ref()
                        .filter(|current| {
                            completion_matches_current_target(
                                &completion.target,
                                current,
                                &observed_owner,
                                &result.inventory,
                            )
                        })
                        .cloned();
                    if let Some(accepted_target) = accepted_target {
                        let warning = match &result.inventory {
                            ScanInventoryResult::Git(scan) => scan.warning.clone(),
                            ScanInventoryResult::NonGit => None,
                        }
                        .map(|warning| bounded_warning(&warning));
                        let inventory = match result.inventory {
                            ScanInventoryResult::Git(scan) => CatalogInventory::Git(scan),
                            ScanInventoryResult::NonGit => CatalogInventory::NonGit,
                        };
                        entry.desired = Some(accepted_target.clone());
                        entry.snapshot = Some(CatalogSnapshot {
                            target: accepted_target,
                            owner: observed_owner,
                            inventory,
                            warning,
                        });
                        entry.warning = None;
                    } else {
                        retry_stale = current_target_exists;
                        mark_snapshot_last_known(
                            entry.snapshot.as_mut(),
                            "worktree inventory source changed",
                        );
                    }
                }
                Err(error) => {
                    entry.warning = Some(bounded_warning(&error));
                    mark_snapshot_last_known(entry.snapshot.as_mut(), &error);
                }
            }
        }

        if retry_stale && current_target_exists {
            mark_snapshot_last_known(entry.snapshot.as_mut(), "worktree inventory source changed");
            entry.dirty = true;
        }
        queue_dirty_rerun(entry, &mut self.queue, &completion.target.root_project_id);
        self.start_queued(cx);
        cx.notify();
    }
}

fn completion_matches_current_target(
    started: &ScanTarget,
    current: &ScanTarget,
    observed_owner: &WorktreeCatalogOwner,
    inventory: &ScanInventoryResult,
) -> bool {
    current.root_config_key == started.root_config_key
        && current.key().source == observed_owner.source
        && match inventory {
            ScanInventoryResult::Git(scan) => current
                .local_generation
                .is_none_or(|generation| generation == scan.generation),
            ScanInventoryResult::NonGit => true,
        }
}

fn snapshot_owner_matches_current(
    snapshot: &CatalogSnapshot,
    owner: &WorktreeCatalogOwner,
    root_config_key: &str,
    current: &ScanTarget,
) -> bool {
    root_config_key == current.root_config_key
        && snapshot.owner == *owner
        && snapshot.owner.source == snapshot.target.key().source
        && snapshot.target.key() == current.key()
}

pub fn activate_target(
    catalog: &Entity<WorktreeCatalog>,
    store: &Entity<AppStore>,
    target: &WorktreeCatalogTarget,
    window: &mut Window,
    cx: &mut App,
) -> Result<ProjectRegistrationOutcome, String> {
    let row = catalog
        .read(cx)
        .resolve_target(target, cx)
        .ok_or_else(|| "Worktree is no longer available".to_string())?;
    if !row.selectable {
        return Err("Worktree cannot be opened in its current Git state".into());
    }

    if let Some(project_id) = row.target.configured_project_id.as_deref() {
        let configured_project = store.read(cx).project(project_id).cloned();
        let current = configured_project
            .as_ref()
            .and_then(|project| configured_location(store.read(cx), project).ok())
            .or_else(|| {
                configured_project
                    .as_ref()
                    .and_then(fallback_configured_location)
            })
            .ok_or_else(|| "Configured worktree no longer exists".to_string())?;
        if current.row_key != row.target.row_key {
            return Err("Configured worktree location changed".into());
        }
    }
    let location = match &row.target.backend {
        CatalogBackend::Local | CatalogBackend::Wsl { .. } => ProjectLocationKey::Local {
            normalized_canonical_path: normalize_host_visible_project_path(
                &row.target.host_visible_path,
            )?,
        },
        CatalogBackend::Ssh { connection_id } => ProjectLocationKey::Ssh {
            connection_id: connection_id.clone(),
            normalized_posix_path: normalize_absolute_posix_path(&row.target.host_visible_path)?,
        },
    };
    let outcome = store.update(cx, |store, cx| {
        store.register_or_activate_project_with_placement(
            location,
            &row.target.host_visible_path,
            Some(&row.target.suggested_name),
            ProjectPlacement::ChildWorktree {
                root_project_id: &row.target.root_project_id,
            },
            cx,
        )
    })?;

    crate::workbench_area::reactivate_active_page(
        &outcome.project_id,
        &outcome.worktree_id,
        window,
        cx,
    );
    catalog.update(cx, |catalog, cx| catalog.force_refresh(cx));
    Ok(outcome)
}

fn build_scan_target(store: &AppStore, project: &ProjectConfig) -> Result<ScanTarget, String> {
    if project.parent_project_id.is_some() {
        return Err("child worktrees do not own repository scans".into());
    }
    let mut snapshot = store.project_execution_snapshot(&project.id)?;
    if snapshot.root_project_id != project.id {
        return Err("catalog scan target is not a top-level project".into());
    }
    let configured_path = configured_execution_path(&snapshot.backend, &project.path)?;
    snapshot.canonical_path = configured_path.clone();
    snapshot.root_source_path = configured_path;
    let backend = catalog_backend(&snapshot.backend);
    let local_generation = matches!(backend, CatalogBackend::Local)
        .then(|| mt_project::worktree::current_generation(Path::new(&project.path)));
    Ok(ScanTarget {
        root_project_id: project.id.clone(),
        root_config_key: root_config_key(project),
        snapshot,
        backend,
        local_generation,
        visibility_source: store.worktree_visibility_source(&project.id),
    })
}

fn scan_target(target: &ScanTarget, revision: u64) -> Result<ScanTaskResult, String> {
    match &target.backend {
        CatalogBackend::Local => {
            match mt_project::worktree::scan(Path::new(&target.snapshot.canonical_path)) {
                Ok(scan) => Ok(ScanTaskResult {
                    inventory: ScanInventoryResult::Git(scan),
                    observed_connection_epoch: None,
                }),
                Err(error) if is_not_repository(error.to_string().as_bytes(), &[]) => {
                    Ok(ScanTaskResult {
                        inventory: ScanInventoryResult::NonGit,
                        observed_connection_epoch: None,
                    })
                }
                Err(error) => Err(bounded_warning(&format!("{error:#}"))),
            }
        }
        CatalogBackend::Wsl { .. } | CatalogBackend::Ssh { .. } => {
            scan_host_porcelain(target, revision)
        }
    }
}

fn scan_host_porcelain(target: &ScanTarget, revision: u64) -> Result<ScanTaskResult, String> {
    let nul = execute_host_command(
        &target.snapshot,
        &CommandPlan::new("git", ["worktree", "list", "--porcelain", "-z"]),
        SCAN_TIMEOUT,
        OUTPUT_LIMIT,
    )
    .map_err(|error| bounded_warning(&error.message))?;
    match parse_captured_output(WorktreePorcelainMode::Nul, &nul.output, revision)? {
        CapturedInventory::Git(scan) => {
            validate_execution_host_scan(&scan)?;
            Ok(ScanTaskResult {
                inventory: ScanInventoryResult::Git(scan),
                observed_connection_epoch: nul.observed_connection_epoch,
            })
        }
        CapturedInventory::NonGit => Ok(ScanTaskResult {
            inventory: ScanInventoryResult::NonGit,
            observed_connection_epoch: nul.observed_connection_epoch,
        }),
        CapturedInventory::UnsupportedNul => {
            let text = execute_host_command(
                &target.snapshot,
                &CommandPlan::new("git", ["worktree", "list", "--porcelain"]),
                SCAN_TIMEOUT,
                OUTPUT_LIMIT,
            )
            .map_err(|error| bounded_warning(&error.message))?;
            match parse_captured_output(WorktreePorcelainMode::Text, &text.output, revision)? {
                CapturedInventory::Git(scan) => {
                    validate_execution_host_scan(&scan)?;
                    Ok(ScanTaskResult {
                        inventory: ScanInventoryResult::Git(scan),
                        observed_connection_epoch: text
                            .observed_connection_epoch
                            .or(nul.observed_connection_epoch),
                    })
                }
                CapturedInventory::NonGit => Ok(ScanTaskResult {
                    inventory: ScanInventoryResult::NonGit,
                    observed_connection_epoch: text
                        .observed_connection_epoch
                        .or(nul.observed_connection_epoch),
                }),
                CapturedInventory::UnsupportedNul => {
                    Err("text porcelain unexpectedly reported unsupported NUL mode".into())
                }
            }
        }
    }
}

fn validate_execution_host_scan(scan: &WorktreeScan) -> Result<(), String> {
    let mut paths = HashSet::new();
    for worktree in &scan.worktrees {
        let path = worktree
            .path
            .to_str()
            .ok_or_else(|| "Git returned a non-UTF-8 remote worktree path".to_string())?;
        let normalized = normalize_absolute_posix_path(path)?;
        if !paths.insert(normalized) {
            return Err("Git returned duplicate remote worktree paths".into());
        }
    }
    Ok(())
}

enum CapturedInventory {
    Git(WorktreeScan),
    NonGit,
    UnsupportedNul,
}

fn parse_captured_output(
    mode: WorktreePorcelainMode,
    output: &CommandOutput,
    generation: u64,
) -> Result<CapturedInventory, String> {
    if output.timed_out {
        return Err("Git worktree discovery timed out".into());
    }
    if output.stdout_truncated || output.stderr_truncated {
        return Err("Git worktree discovery output exceeded its bounded capture".into());
    }
    let Some(exit_code) = output.exit_code else {
        return Err("Git worktree discovery returned no exit status".into());
    };
    if exit_code == 0 {
        let worktrees = mt_project::worktree::parse_porcelain_with_path_semantics(
            mode,
            &output.stdout,
            WorktreePathSemantics::Posix,
        )
        .map_err(|error| bounded_warning(&format!("invalid Git worktree porcelain: {error:#}")))?;
        return Ok(CapturedInventory::Git(WorktreeScan {
            generation,
            source: match mode {
                WorktreePorcelainMode::Nul => WorktreeScanSource::PorcelainZ,
                WorktreePorcelainMode::Text => WorktreeScanSource::PorcelainText,
            },
            authoritative: true,
            worktrees,
            warning: None,
        }));
    }
    if mode == WorktreePorcelainMode::Nul
        && exit_code == 129
        && is_unsupported_nul_option(&output.stderr, &output.stdout)
    {
        return Ok(CapturedInventory::UnsupportedNul);
    }
    if is_not_repository(&output.stderr, &output.stdout) {
        return Ok(CapturedInventory::NonGit);
    }
    Err(bounded_warning(&format!(
        "Git worktree discovery failed with exit code {exit_code}: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

fn is_not_repository(stderr: &[u8], stdout: &[u8]) -> bool {
    let mut text = String::from_utf8_lossy(stderr).to_lowercase();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(stdout).to_lowercase());
    text.contains("not a git repository")
}

fn is_unsupported_nul_option(stderr: &[u8], stdout: &[u8]) -> bool {
    let mut text = String::from_utf8_lossy(stderr).to_lowercase();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(stdout).to_lowercase());
    text.lines().any(|line| {
        let unsupported = line.contains("unknown option")
            || line.contains("unknown switch")
            || line.contains("unrecognized option");
        let mentions_z = line.contains("-z")
            || line.contains("'z'")
            || line.contains("`z'")
            || line.contains("\"z\"");
        unsupported && mentions_z
    })
}

fn catalog_backend(backend: &ExecutionBackend) -> CatalogBackend {
    match backend {
        ExecutionBackend::Local => CatalogBackend::Local,
        ExecutionBackend::Wsl { distro } => CatalogBackend::Wsl {
            distro: distro.clone(),
        },
        ExecutionBackend::Ssh { connection, .. } => CatalogBackend::Ssh {
            connection_id: connection.id.clone(),
        },
    }
}

pub(crate) fn root_config_key(project: &ProjectConfig) -> String {
    format!(
        "{}\0{}\0{}",
        project.id,
        project.path,
        project.ssh_connection_id.as_deref().unwrap_or("")
    )
}

fn ordered_top_level_project_ids(
    projects: &[ProjectConfig],
    tree: Option<&[ProjectTreeItem]>,
) -> Vec<String> {
    let known_ids: HashSet<&str> = projects.iter().map(|project| project.id.as_str()).collect();
    let top_level_ids: HashSet<&str> = projects
        .iter()
        .filter(|project| {
            project
                .parent_project_id
                .as_deref()
                .is_none_or(|parent| !known_ids.contains(parent))
        })
        .map(|project| project.id.as_str())
        .collect();

    fn walk(
        items: &[ProjectTreeItem],
        top_level_ids: &HashSet<&str>,
        seen: &mut HashSet<String>,
        out: &mut Vec<String>,
    ) {
        for item in items {
            match item {
                ProjectTreeItem::ProjectId(id) => {
                    if top_level_ids.contains(id.as_str()) && seen.insert(id.clone()) {
                        out.push(id.clone());
                    }
                }
                ProjectTreeItem::Group(group) => {
                    walk(&group.children, top_level_ids, seen, out);
                }
            }
        }
    }

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    walk(tree.unwrap_or(&[]), &top_level_ids, &mut seen, &mut out);
    for project in projects {
        if top_level_ids.contains(project.id.as_str()) && seen.insert(project.id.clone()) {
            out.push(project.id.clone());
        }
    }
    out
}

struct ConfiguredLocation {
    row_key: String,
    execution_path: String,
    host_visible_path: String,
    backend: CatalogBackend,
    host_label: String,
}

fn configured_location(
    store: &AppStore,
    project: &ProjectConfig,
) -> Result<ConfiguredLocation, String> {
    let snapshot = store.project_execution_snapshot(&project.id)?;
    let host_visible_path = store
        .trusted_canonical_worktree_path_for_project(&project.id)
        .unwrap_or(project.path.as_str());
    let execution_path = configured_execution_path(&snapshot.backend, host_visible_path)?;
    let backend = catalog_backend(&snapshot.backend);
    let row_key = row_key(&snapshot, &backend, &execution_path)?;
    Ok(ConfiguredLocation {
        row_key,
        execution_path,
        host_visible_path: host_visible_path.to_string(),
        backend,
        host_label: snapshot.host_label,
    })
}

fn fallback_configured_location(project: &ProjectConfig) -> Option<ConfiguredLocation> {
    if let Some(connection_id) = project.ssh_connection_id.as_deref() {
        let execution_path = normalize_absolute_posix_path(&project.path).ok()?;
        return Some(ConfiguredLocation {
            row_key: format!("unavailable\0ssh:{connection_id}\0{execution_path}"),
            execution_path: execution_path.clone(),
            host_visible_path: execution_path,
            backend: CatalogBackend::Ssh {
                connection_id: connection_id.to_string(),
            },
            host_label: "SSH".into(),
        });
    }
    if let Some(wsl) = mt_core::parse_wsl_unc(&project.path.replace('/', "\\")) {
        let execution_path = normalize_absolute_posix_path(&wsl.unix_path).ok()?;
        return Some(ConfiguredLocation {
            row_key: format!(
                "unavailable\0wsl:{}\0{}",
                wsl.distro.to_lowercase(),
                execution_path
            ),
            execution_path,
            host_visible_path: project.path.clone(),
            backend: CatalogBackend::Wsl {
                distro: wsl.distro.clone(),
            },
            host_label: format!("WSL ({})", wsl.distro),
        });
    }
    Some(ConfiguredLocation {
        row_key: format!(
            "unavailable\0local\0{}",
            mt_project::worktree::normalize_path_for_comparison(&project.path)
        ),
        execution_path: project.path.clone(),
        host_visible_path: project.path.clone(),
        backend: CatalogBackend::Local,
        host_label: "Local machine".into(),
    })
}

fn captured_configured_location(
    target: &ScanTarget,
    project: &ProjectConfig,
    host_visible_path: &str,
) -> Option<ConfiguredLocation> {
    let execution_path = match &target.backend {
        CatalogBackend::Local => {
            if project.ssh_connection_id.is_some()
                || mt_core::parse_wsl_unc(&host_visible_path.replace('/', "\\")).is_some()
            {
                return None;
            }
            host_visible_path.to_string()
        }
        CatalogBackend::Wsl { distro } => {
            if project.ssh_connection_id.is_some() {
                return None;
            }
            let wsl = mt_core::parse_wsl_unc(&host_visible_path.replace('/', "\\"))?;
            if !wsl.distro.eq_ignore_ascii_case(distro) {
                return None;
            }
            normalize_absolute_posix_path(&wsl.unix_path).ok()?
        }
        CatalogBackend::Ssh { connection_id } => {
            if project.ssh_connection_id.as_deref() != Some(connection_id.as_str()) {
                return None;
            }
            normalize_absolute_posix_path(host_visible_path).ok()?
        }
    };
    Some(ConfiguredLocation {
        row_key: row_key(&target.snapshot, &target.backend, &execution_path).ok()?,
        execution_path,
        host_visible_path: host_visible_path.to_string(),
        backend: target.backend.clone(),
        host_label: target.snapshot.host_label.clone(),
    })
}

fn configured_location_for_projection(
    store: &AppStore,
    project: &ProjectConfig,
    target: Option<&ScanTarget>,
) -> Option<ConfiguredLocation> {
    let host_visible_path = store
        .trusted_canonical_worktree_path_for_project(&project.id)
        .unwrap_or(project.path.as_str());
    configured_location(store, project)
        .ok()
        .or_else(|| {
            target
                .and_then(|target| captured_configured_location(target, project, host_visible_path))
        })
        .or_else(|| fallback_configured_location(project))
}

fn row_key(
    snapshot: &ProjectExecutionSnapshot,
    backend: &CatalogBackend,
    execution_path: &str,
) -> Result<String, String> {
    let path = match backend {
        CatalogBackend::Local => {
            mt_project::worktree::normalize_path_for_comparison(execution_path)
        }
        CatalogBackend::Wsl { .. } | CatalogBackend::Ssh { .. } => {
            normalize_absolute_posix_path(execution_path)?
        }
    };
    let qualifier = match backend {
        CatalogBackend::Local => "local".to_string(),
        CatalogBackend::Wsl { distro } => format!("wsl:{}", distro.to_lowercase()),
        CatalogBackend::Ssh { connection_id } => format!("ssh:{connection_id}"),
    };
    Ok(format!(
        "{}\0{}\0{}",
        snapshot.execution_host_id, qualifier, path
    ))
}

fn fact_location(target: &ScanTarget, fact: &WorktreeFact) -> Result<ConfiguredLocation, String> {
    let execution_path = match &target.backend {
        CatalogBackend::Local => fact.path.to_string_lossy().to_string(),
        CatalogBackend::Wsl { .. } | CatalogBackend::Ssh { .. } => normalize_absolute_posix_path(
            fact.path
                .to_str()
                .ok_or_else(|| "Git returned a non-UTF-8 remote worktree path".to_string())?,
        )?,
    };
    let host_visible_path = match &target.backend {
        CatalogBackend::Wsl { distro } => wsl_host_visible_path(distro, &execution_path)?,
        CatalogBackend::Local | CatalogBackend::Ssh { .. } => execution_path.clone(),
    };
    Ok(ConfiguredLocation {
        row_key: row_key(&target.snapshot, &target.backend, &execution_path)?,
        execution_path,
        host_visible_path,
        backend: target.backend.clone(),
        host_label: target.snapshot.host_label.clone(),
    })
}

fn configured_project_for_row<'a>(
    store: &'a AppStore,
    root_project_id: &str,
    row_key: &str,
    target: &ScanTarget,
) -> Option<&'a ProjectConfig> {
    let root = store.project(root_project_id);
    root.into_iter()
        .chain(
            store
                .projects()
                .iter()
                .filter(|project| project.parent_project_id.as_deref() == Some(root_project_id)),
        )
        .chain(store.projects().iter())
        .find(|project| {
            configured_location_for_projection(store, project, Some(target))
                .is_some_and(|location| location.row_key == row_key)
        })
}

fn should_project_configured_children(snapshot: Option<&CatalogSnapshot>) -> bool {
    match snapshot.map(|snapshot| &snapshot.inventory) {
        None => true,
        Some(CatalogInventory::Git(scan)) => !scan.authoritative,
        Some(CatalogInventory::NonGit) => false,
    }
}

fn build_groups(
    store: &AppStore,
    entries: &HashMap<String, CatalogEntry>,
) -> Vec<ProjectWorktreeGroup> {
    ordered_top_level_project_ids(store.projects(), store.config().project_tree.as_deref())
        .into_iter()
        .filter_map(|root_project_id| {
            let root = store.project(&root_project_id)?;
            let config_key = root_config_key(root);
            let entry = entries
                .get(&root_project_id)
                .filter(|entry| entry.root_config_key == config_key);
            let snapshot = entry.and_then(|entry| entry.snapshot.as_ref());
            let target = entry
                .and_then(|entry| entry.desired.as_ref())
                .or_else(|| snapshot.map(|snapshot| &snapshot.target));
            let root_location = configured_location_for_projection(store, root, target);
            let backend = target
                .map(|target| target.backend.clone())
                .or_else(|| {
                    root_location
                        .as_ref()
                        .map(|location| location.backend.clone())
                })
                .unwrap_or(CatalogBackend::Local);
            let host_label = target
                .map(|target| target.snapshot.host_label.clone())
                .or_else(|| {
                    root_location
                        .as_ref()
                        .map(|location| location.host_label.clone())
                })
                .unwrap_or_else(|| backend.label().to_string());
            let mut rows = Vec::new();
            let mut seen = HashSet::new();

            if let Some(snapshot) = snapshot
                && let CatalogInventory::Git(scan) = &snapshot.inventory
            {
                let snapshot_target = &snapshot.target;
                for fact in scan
                    .worktrees
                    .iter()
                    .filter(|fact| fact.is_main)
                    .chain(scan.worktrees.iter().filter(|fact| !fact.is_main))
                {
                    let Ok(location) = fact_location(snapshot_target, fact) else {
                        continue;
                    };
                    if !seen.insert(location.row_key.clone()) {
                        continue;
                    }
                    let configured = configured_project_for_row(
                        store,
                        &root_project_id,
                        &location.row_key,
                        snapshot_target,
                    );
                    let mut row =
                        row_from_fact(root, &config_key, snapshot, location, fact, configured);
                    apply_refresh_eligibility(
                        &mut row,
                        entry.is_some_and(|entry| entry.in_flight_revision.is_some()),
                    );
                    rows.push(row);
                }
            }

            if let Some(location) = root_location.as_ref()
                && seen.insert(location.row_key.clone())
            {
                rows.push(configured_row(
                    root,
                    &root_project_id,
                    &config_key,
                    location,
                    rows.is_empty(),
                    snapshot,
                ));
            }

            if should_project_configured_children(snapshot) {
                for child in store.projects().iter().filter(|project| {
                    project.parent_project_id.as_deref() == Some(root_project_id.as_str())
                }) {
                    let Some(location) = configured_location_for_projection(store, child, target)
                    else {
                        continue;
                    };
                    if seen.insert(location.row_key.clone()) {
                        rows.push(configured_row(
                            child,
                            &root_project_id,
                            &config_key,
                            &location,
                            false,
                            snapshot,
                        ));
                    }
                }
            }

            // Configured preferences survive later canonical resolution, but
            // never infer a relationship between unmatched aliases and facts.
            for row in &mut rows {
                if let Some(id) = row.target.configured_project_id.as_deref() {
                    row.configured_visibility_key =
                        store.configured_project_visibility_key(&root_project_id, id);
                    if row.target.owner.is_none() {
                        row.visibility_key =
                            store.configured_worktree_visibility_key(&root_project_id, id);
                    }
                }
            }

            Some(ProjectWorktreeGroup {
                root_project_id,
                root_project_name: root.name.clone(),
                root_project_path: root.path.clone(),
                execution_host_id: target.map(|target| target.snapshot.execution_host_id.clone()),
                host_label,
                backend,
                warning: entry
                    .and_then(|entry| entry.warning.clone())
                    .or_else(|| snapshot.and_then(|snapshot| snapshot.warning.clone())),
                refreshing: entry.is_some_and(|entry| entry.in_flight_revision.is_some()),
                rows,
            })
        })
        .collect()
}

fn row_from_fact(
    root: &ProjectConfig,
    root_config_key: &str,
    snapshot: &CatalogSnapshot,
    location: ConfiguredLocation,
    fact: &WorktreeFact,
    configured: Option<&ProjectConfig>,
) -> WorktreeCatalogRow {
    let branch = fact.branch_ref.as_deref().map(short_branch);
    let suggested_name = branch
        .clone()
        .or_else(|| configured.map(|project| project.name.clone()))
        .unwrap_or_else(|| path_leaf(&location.host_visible_path, "worktree"));
    let authoritative = match &snapshot.inventory {
        CatalogInventory::Git(scan) => scan.authoritative,
        CatalogInventory::NonGit => true,
    };
    let selectable = configured.is_some()
        || (authoritative
            && !fact.is_bare
            && fact.prunable.is_none()
            && fact.path_state != WorktreePathState::Missing);
    WorktreeCatalogRow {
        visibility_key: snapshot
            .target
            .visibility_source
            .as_ref()
            .and_then(|source| {
                crate::worktree_visibility::preference_key(source, &location.execution_path)
            }),
        configured_visibility_key: None,
        target: WorktreeCatalogTarget {
            root_project_id: root.id.clone(),
            row_key: location.row_key,
            root_config_key: root_config_key.to_string(),
            configured_project_id: configured.map(|project| project.id.clone()),
            host_visible_path: location.host_visible_path,
            execution_path: location.execution_path,
            suggested_name: suggested_name.clone(),
            backend: location.backend,
            owner: Some(snapshot.owner.clone()),
        },
        label: suggested_name,
        branch,
        head: fact.head.clone(),
        is_main: fact.is_main,
        is_detached: fact.is_detached,
        is_bare: fact.is_bare,
        is_sparse: fact.is_sparse,
        is_locked: fact.locked.is_some(),
        is_prunable: fact.prunable.is_some(),
        locked_reason: fact
            .locked
            .as_ref()
            .and_then(|annotation| annotation.reason.clone()),
        prunable_reason: fact
            .prunable
            .as_ref()
            .and_then(|annotation| annotation.reason.clone()),
        path_state: fact.path_state,
        authoritative,
        last_known: match &snapshot.inventory {
            CatalogInventory::Git(scan) => !scan.authoritative,
            CatalogInventory::NonGit => false,
        },
        selectable,
    }
}

fn configured_row(
    project: &ProjectConfig,
    root_project_id: &str,
    root_config_key: &str,
    location: &ConfiguredLocation,
    is_main: bool,
    snapshot: Option<&CatalogSnapshot>,
) -> WorktreeCatalogRow {
    WorktreeCatalogRow {
        visibility_key: None,
        configured_visibility_key: None,
        target: WorktreeCatalogTarget {
            root_project_id: root_project_id.to_string(),
            row_key: location.row_key.clone(),
            root_config_key: root_config_key.to_string(),
            configured_project_id: Some(project.id.clone()),
            host_visible_path: location.host_visible_path.clone(),
            execution_path: location.execution_path.clone(),
            suggested_name: project.name.clone(),
            backend: location.backend.clone(),
            owner: None,
        },
        label: project.name.clone(),
        branch: None,
        head: None,
        is_main,
        is_detached: false,
        is_bare: false,
        is_sparse: false,
        is_locked: false,
        is_prunable: false,
        locked_reason: None,
        prunable_reason: None,
        path_state: WorktreePathState::Unknown,
        authoritative: false,
        last_known: snapshot.is_some_and(|snapshot| {
            matches!(
                &snapshot.inventory,
                CatalogInventory::Git(scan) if !scan.authoritative
            )
        }),
        selectable: true,
    }
}

/// Refreshing is an activation fence, not evidence that the last scan failed.
fn apply_refresh_eligibility(row: &mut WorktreeCatalogRow, refreshing: bool) {
    if refreshing {
        row.authoritative = false;
        if row.target.configured_project_id.is_none() {
            row.selectable = false;
        }
    }
}

fn short_branch(branch: &str) -> String {
    branch
        .strip_prefix("refs/heads/")
        .unwrap_or(branch)
        .to_string()
}

fn path_leaf(path: &str, fallback: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn bounded_warning(warning: &str) -> String {
    warning.chars().take(WARNING_LIMIT).collect()
}

fn mark_snapshot_last_known(snapshot: Option<&mut CatalogSnapshot>, warning: &str) {
    let Some(snapshot) = snapshot else {
        return;
    };
    if let CatalogInventory::Git(scan) = &mut snapshot.inventory {
        scan.authoritative = false;
        scan.source = WorktreeScanSource::LastKnown;
        scan.warning = Some(bounded_warning(warning));
    }
    snapshot.warning = Some(bounded_warning(warning));
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use mt_config::{ProjectGroup, SshConnection};

    pub(crate) fn groups_for_visibility_test(
        store: &AppStore,
        root_project_id: &str,
        canonical_path: Option<&str>,
    ) -> Vec<ProjectWorktreeGroup> {
        let mut entries = HashMap::new();
        if let Some(path) = canonical_path {
            let target = build_scan_target(store, store.project(root_project_id).unwrap()).unwrap();
            let mut fact = worktree_fact(path);
            fact.is_main = true;
            let snapshot = CatalogSnapshot {
                owner: target.owner(1, None),
                target: target.clone(),
                inventory: CatalogInventory::Git(WorktreeScan {
                    generation: 1,
                    source: WorktreeScanSource::PorcelainZ,
                    authoritative: true,
                    worktrees: vec![fact],
                    warning: None,
                }),
                warning: None,
            };
            entries.insert(
                root_project_id.to_string(),
                CatalogEntry {
                    root_config_key: target.root_config_key.clone(),
                    desired: Some(target),
                    snapshot: Some(snapshot),
                    ..Default::default()
                },
            );
        }
        build_groups(store, &entries)
    }

    fn output(exit_code: i32, stdout: &[u8], stderr: &[u8]) -> CommandOutput {
        CommandOutput {
            stdout: stdout.to_vec(),
            stderr: stderr.to_vec(),
            exit_code: Some(exit_code),
            ..CommandOutput::default()
        }
    }

    fn project(id: &str, path: &str, parent: Option<&str>) -> ProjectConfig {
        ProjectConfig {
            id: id.into(),
            name: id.into(),
            path: path.into(),
            description: None,
            saved_layout: None,
            expanded_dirs: Vec::new(),
            ssh_mcp_enabled: false,
            ssh_cli_token: None,
            ssh_connection_ids: None,
            env_vars: Vec::new(),
            hidden_worktrees: Vec::new(),
            wsl_sessions_distro: None,
            ssh_connection_id: None,
            parent_project_id: parent.map(str::to_string),
            kind_override: None,
        }
    }

    fn worktree_fact(path: &str) -> WorktreeFact {
        WorktreeFact {
            path: std::path::PathBuf::from(path),
            head: Some("abc".into()),
            branch_ref: Some("refs/heads/main".into()),
            is_main: false,
            is_detached: false,
            is_bare: false,
            is_sparse: false,
            locked: None,
            prunable: None,
            path_state: WorktreePathState::Present,
        }
    }

    fn ssh_scan_target(configured_path: &str, epoch: u64) -> ScanTarget {
        let connection = SshConnection {
            id: "ssh-a".into(),
            name: "SSH A".into(),
            host: "host.example".into(),
            port: 22,
            user: "deploy".into(),
            password: None,
            identity_file: None,
            group: None,
        };
        let execution_host_id = mt_identity::ExecutionHostId::derive(
            "ssh-catalog-test",
            &mt_identity::HostInstallId::new(),
        );
        let repo_id = mt_identity::RepoId::derive(&execution_host_id, "/srv/repo/.git");
        ScanTarget {
            root_project_id: "p".into(),
            root_config_key: "config".into(),
            snapshot: ProjectExecutionSnapshot {
                project_id: "p".into(),
                root_project_id: "p".into(),
                worktree_id: mt_identity::WorktreeId::derive(&repo_id, configured_path, None),
                execution_host_id,
                canonical_path: configured_path.into(),
                root_source_path: configured_path.into(),
                backend: ExecutionBackend::Ssh {
                    connection,
                    connection_fingerprint: 41,
                    connection_epoch: Some(epoch),
                },
                host_label: "SSH A".into(),
            },
            backend: CatalogBackend::Ssh {
                connection_id: "ssh-a".into(),
            },
            local_generation: None,
            visibility_source: None,
        }
    }

    #[test]
    fn routine_refresh_fences_registration_without_last_known_warning_churn() {
        let mut target = ssh_scan_target("/repo", 4);
        target.visibility_source =
            crate::worktree_visibility::source_from_snapshot(&target.snapshot, "/repo");
        let fact = worktree_fact("/feature");
        let root = project("p", "/repo", None);
        let snapshot = CatalogSnapshot {
            owner: target.owner(7, Some(4)),
            target: target.clone(),
            inventory: CatalogInventory::Git(WorktreeScan {
                generation: 2,
                source: WorktreeScanSource::PorcelainZ,
                authoritative: true,
                worktrees: vec![fact.clone()],
                warning: None,
            }),
            warning: None,
        };
        let mut entry = CatalogEntry {
            snapshot: Some(snapshot),
            ..Default::default()
        };
        let row = |snapshot: &CatalogSnapshot| {
            row_from_fact(
                &root,
                "config",
                snapshot,
                fact_location(&target, &fact).unwrap(),
                &fact,
                None,
            )
        };
        let initial_target = row(entry.snapshot.as_ref().unwrap()).target;
        for revision in 8..11 {
            entry.begin_scan(revision);
            let snapshot = entry.snapshot.as_ref().unwrap();
            let mut refreshing = row(snapshot);
            apply_refresh_eligibility(&mut refreshing, entry.in_flight_revision.is_some());
            assert!(!refreshing.authoritative);
            assert!(!refreshing.selectable);
            assert!(!refreshing.last_known);
            assert!(snapshot.warning.is_none());
            assert_eq!(refreshing.target, initial_target);
            let fresh = row(snapshot);
            assert!(fresh.authoritative && fresh.selectable && !fresh.last_known);
            assert!(!should_project_configured_children(Some(snapshot)));
            entry.in_flight_revision = None;
        }
        let mut snapshot = entry.snapshot.unwrap();
        let configured = project("child", "/feature", Some("p"));
        let mut configured_row = row_from_fact(
            &root,
            "config",
            &snapshot,
            fact_location(&target, &fact).unwrap(),
            &fact,
            Some(&configured),
        );
        apply_refresh_eligibility(&mut configured_row, true);
        assert!(configured_row.selectable);
        assert!(!configured_row.last_known);

        let hidden = vec![row(&snapshot).visibility_key.unwrap()];
        assert!(!crate::worktree_visibility::sidebar_visible(
            &row(&snapshot),
            &hidden
        ));
        assert!(row(&snapshot).selectable);
        assert_eq!(row(&snapshot).target, initial_target);
        mark_snapshot_last_known(Some(&mut snapshot), "offline");
        let mut retrying = row(&snapshot);
        apply_refresh_eligibility(&mut retrying, true);
        assert!(retrying.last_known);
        assert!(!retrying.selectable);
        assert_eq!(snapshot.warning.as_deref(), Some("offline"));
        assert_eq!(retrying.visibility_key, hidden.first().cloned());
    }

    #[test]
    fn captured_output_is_authoritative_only_when_complete_and_valid() {
        let parsed = parse_captured_output(
            WorktreePorcelainMode::Nul,
            &output(
                0,
                b"worktree /repo\0HEAD abc\0branch refs/heads/main\0\0",
                b"",
            ),
            7,
        )
        .unwrap();
        let CapturedInventory::Git(scan) = parsed else {
            panic!("expected Git inventory");
        };
        assert!(scan.authoritative);
        assert_eq!(scan.generation, 7);
        assert_eq!(scan.source, WorktreeScanSource::PorcelainZ);

        let mut truncated = output(0, b"worktree /repo\0\0", b"");
        truncated.stdout_truncated = true;
        assert!(parse_captured_output(WorktreePorcelainMode::Nul, &truncated, 8).is_err());
        assert!(
            parse_captured_output(
                WorktreePorcelainMode::Nul,
                &output(0, b"worktree \xff\0\0", b""),
                9,
            )
            .is_err()
        );

        let mut timed_out = output(0, b"worktree /repo\0\0", b"");
        timed_out.timed_out = true;
        assert!(parse_captured_output(WorktreePorcelainMode::Nul, &timed_out, 10).is_err());
        let mut no_status = output(0, b"worktree /repo\0\0", b"");
        no_status.exit_code = None;
        assert!(parse_captured_output(WorktreePorcelainMode::Nul, &no_status, 11).is_err());
        assert!(!is_unsupported_nul_option(
            b"error: unknown option `--format'\nusage: git worktree list [-z]",
            b"",
        ));
    }

    #[test]
    fn execution_host_scan_requires_absolute_unique_posix_paths() {
        let mut scan = WorktreeScan {
            generation: 1,
            source: WorktreeScanSource::PorcelainZ,
            authoritative: true,
            worktrees: vec![worktree_fact("relative")],
            warning: None,
        };
        assert!(validate_execution_host_scan(&scan).is_err());

        scan.worktrees = vec![worktree_fact("/srv/Repo"), worktree_fact("/srv/repo")];
        assert!(validate_execution_host_scan(&scan).is_ok());

        scan.worktrees.push(worktree_fact("/srv/./Repo"));
        assert!(validate_execution_host_scan(&scan).is_err());
    }

    #[test]
    fn scheduler_coalesces_busy_refreshes_and_queues_one_dirty_rerun() {
        let mut entry = CatalogEntry {
            in_flight_revision: Some(7),
            ..CatalogEntry::default()
        };
        let mut queue = VecDeque::new();

        request_scan(&mut entry, &mut queue, "root");
        request_scan(&mut entry, &mut queue, "root");

        assert!(entry.dirty);
        assert!(queue.is_empty());

        entry.in_flight_revision = None;
        queue_dirty_rerun(&mut entry, &mut queue, "root");
        request_scan(&mut entry, &mut queue, "root");

        assert!(!entry.dirty);
        assert_eq!(queue.into_iter().collect::<Vec<_>>(), vec!["root"]);
        assert!(scan_capacity_available(MAX_CONCURRENT_SCANS - 1));
        assert!(!scan_capacity_available(MAX_CONCURRENT_SCANS));
        assert!(!scan_capacity_available(MAX_CONCURRENT_SCANS + 1));
    }

    #[test]
    fn target_generation_fences_an_a_b_a_source_cycle() {
        let entry = CatalogEntry {
            target_generation: 3,
            in_flight_revision: Some(9),
            ..CatalogEntry::default()
        };

        assert!(!target_generation_matches(&entry, 1));
        assert!(!target_generation_matches(&entry, 2));
        assert!(target_generation_matches(&entry, 3));
    }

    #[test]
    fn read_only_ssh_scan_accepts_only_the_exact_fresh_epoch_and_fingerprint() {
        let started = ssh_scan_target("/srv/repo-link", 4);
        let observed_owner = started.owner(8, Some(5));
        let mut current = started.clone();
        let ExecutionBackend::Ssh {
            connection_epoch, ..
        } = &mut current.snapshot.backend
        else {
            unreachable!();
        };
        *connection_epoch = Some(5);

        assert!(completion_matches_current_target(
            &started,
            &current,
            &observed_owner,
            &ScanInventoryResult::NonGit,
        ));
        assert!(!completion_matches_current_target(
            &started,
            &started,
            &observed_owner,
            &ScanInventoryResult::NonGit,
        ));

        let mut changed_fingerprint = current;
        let ExecutionBackend::Ssh {
            connection_fingerprint,
            ..
        } = &mut changed_fingerprint.snapshot.backend
        else {
            unreachable!();
        };
        *connection_fingerprint += 1;
        assert!(!completion_matches_current_target(
            &started,
            &changed_fingerprint,
            &observed_owner,
            &ScanInventoryResult::NonGit,
        ));
    }

    #[test]
    fn trusted_ssh_canonical_alias_merges_with_the_git_fact_row() {
        let target = ssh_scan_target("/srv/repo-link", 4);
        let mut configured = project("p", "/srv/repo-link", None);
        configured.ssh_connection_id = Some("ssh-a".into());
        let configured_location =
            captured_configured_location(&target, &configured, "/srv/repo-real").unwrap();
        let fact_location = fact_location(&target, &worktree_fact("/srv/repo-real")).unwrap();

        assert_eq!(configured_location.row_key, fact_location.row_key);
        assert_eq!(configured_location.execution_path, "/srv/repo-real");
        assert_eq!(configured_location.host_visible_path, "/srv/repo-real");
    }

    #[test]
    fn nul_fallback_and_non_repository_are_distinct() {
        assert!(matches!(
            parse_captured_output(
                WorktreePorcelainMode::Nul,
                &output(129, b"", b"error: unknown switch 'z'"),
                1,
            )
            .unwrap(),
            CapturedInventory::UnsupportedNul
        ));
        assert!(
            parse_captured_output(
                WorktreePorcelainMode::Nul,
                &output(129, b"", b"usage error unrelated to -z"),
                1,
            )
            .is_err()
        );
        assert!(matches!(
            parse_captured_output(
                WorktreePorcelainMode::Nul,
                &output(128, b"", b"fatal: not a git repository"),
                1,
            )
            .unwrap(),
            CapturedInventory::NonGit
        ));
        assert!(
            parse_captured_output(
                WorktreePorcelainMode::Nul,
                &output(1, b"", b"permission denied"),
                1,
            )
            .is_err()
        );
    }

    #[test]
    fn top_level_order_never_schedules_configured_children() {
        let projects = vec![
            project("a", "/a", None),
            project("a-child", "/a-child", Some("a")),
            project("b", "/b", None),
        ];
        let tree = vec![ProjectTreeItem::Group(ProjectGroup {
            id: "group".into(),
            name: "Group".into(),
            collapsed: true,
            children: vec![ProjectTreeItem::ProjectId("b".into())],
        })];
        assert_eq!(
            ordered_top_level_project_ids(&projects, Some(&tree)),
            vec!["b", "a"]
        );
    }

    #[test]
    fn configured_child_fallbacks_require_a_missing_or_degraded_git_snapshot() {
        assert!(should_project_configured_children(None));

        let target = ssh_scan_target("/srv/repo", 4);
        let mut snapshot = CatalogSnapshot {
            owner: target.owner(7, Some(4)),
            target,
            inventory: CatalogInventory::NonGit,
            warning: None,
        };
        assert!(!should_project_configured_children(Some(&snapshot)));

        snapshot.inventory = CatalogInventory::Git(WorktreeScan {
            generation: 1,
            source: WorktreeScanSource::PorcelainZ,
            authoritative: true,
            worktrees: Vec::new(),
            warning: None,
        });
        assert!(!should_project_configured_children(Some(&snapshot)));

        let CatalogInventory::Git(scan) = &mut snapshot.inventory else {
            unreachable!();
        };
        scan.authoritative = false;
        scan.source = WorktreeScanSource::LastKnown;
        assert!(should_project_configured_children(Some(&snapshot)));
    }

    #[test]
    fn row_keys_keep_host_and_posix_case_in_identity() {
        let host = mt_identity::ExecutionHostId::derive("test", &mt_identity::HostInstallId::new());
        let worktree = mt_identity::WorktreeId::derive(
            &mt_identity::RepoId::derive(&host, "/repo/.git"),
            "/repo",
            None,
        );
        let snapshot = ProjectExecutionSnapshot {
            project_id: "p".into(),
            root_project_id: "p".into(),
            worktree_id: worktree,
            execution_host_id: host,
            canonical_path: "/repo".into(),
            root_source_path: "/repo".into(),
            backend: ExecutionBackend::Wsl {
                distro: "Ubuntu".into(),
            },
            host_label: "WSL".into(),
        };
        let backend = CatalogBackend::Wsl {
            distro: "Ubuntu".into(),
        };
        assert_ne!(
            row_key(&snapshot, &backend, "/home/User/repo").unwrap(),
            row_key(&snapshot, &backend, "/home/user/repo").unwrap()
        );
        assert_ne!(
            row_key(&snapshot, &backend, "/home/User/repo").unwrap(),
            row_key(
                &snapshot,
                &CatalogBackend::Wsl {
                    distro: "Debian".into(),
                },
                "/home/User/repo",
            )
            .unwrap()
        );
    }

    #[test]
    fn snapshot_owner_fences_source_generation_and_config_changes() {
        let host = mt_identity::ExecutionHostId::derive(
            "catalog-owner-test",
            &mt_identity::HostInstallId::new(),
        );
        let worktree = mt_identity::WorktreeId::derive(
            &mt_identity::RepoId::derive(&host, "/repo/.git"),
            "/repo",
            None,
        );
        let target = ScanTarget {
            root_project_id: "p".into(),
            root_config_key: "config-a".into(),
            snapshot: ProjectExecutionSnapshot {
                project_id: "p".into(),
                root_project_id: "p".into(),
                worktree_id: worktree,
                execution_host_id: host,
                canonical_path: "/repo".into(),
                root_source_path: "/repo".into(),
                backend: ExecutionBackend::Local,
                host_label: "Local".into(),
            },
            backend: CatalogBackend::Local,
            local_generation: Some(4),
            visibility_source: None,
        };
        let owner = target.owner(9, None);
        let snapshot = CatalogSnapshot {
            target: target.clone(),
            owner: owner.clone(),
            inventory: CatalogInventory::NonGit,
            warning: None,
        };
        assert!(snapshot_owner_matches_current(
            &snapshot, &owner, "config-a", &target,
        ));

        let mut changed_generation = target.clone();
        changed_generation.local_generation = Some(5);
        assert!(!snapshot_owner_matches_current(
            &snapshot,
            &owner,
            "config-a",
            &changed_generation,
        ));
        let mut changed_source = target.clone();
        changed_source.snapshot.canonical_path = "/other".into();
        assert!(!snapshot_owner_matches_current(
            &snapshot,
            &owner,
            "config-a",
            &changed_source,
        ));
        let mut changed_owner = owner.clone();
        changed_owner.revision += 1;
        assert!(!snapshot_owner_matches_current(
            &snapshot,
            &changed_owner,
            "config-a",
            &target,
        ));
        assert!(!snapshot_owner_matches_current(
            &snapshot, &owner, "config-b", &target,
        ));
    }

    #[test]
    fn degraded_snapshot_keeps_its_target_and_marks_git_rows_last_known() {
        let host = mt_identity::ExecutionHostId::derive(
            "catalog-last-known-test",
            &mt_identity::HostInstallId::new(),
        );
        let worktree = mt_identity::WorktreeId::derive(
            &mt_identity::RepoId::derive(&host, "/repo/.git"),
            "/repo",
            None,
        );
        let target = ScanTarget {
            root_project_id: "p".into(),
            root_config_key: "config".into(),
            snapshot: ProjectExecutionSnapshot {
                project_id: "p".into(),
                root_project_id: "p".into(),
                worktree_id: worktree,
                execution_host_id: host,
                canonical_path: "/repo".into(),
                root_source_path: "/repo".into(),
                backend: ExecutionBackend::Local,
                host_label: "Local".into(),
            },
            backend: CatalogBackend::Local,
            local_generation: Some(2),
            visibility_source: None,
        };
        let fact = worktree_fact("/linked");
        let mut snapshot = CatalogSnapshot {
            owner: target.owner(7, None),
            target: target.clone(),
            inventory: CatalogInventory::Git(WorktreeScan {
                generation: 2,
                source: WorktreeScanSource::PorcelainZ,
                authoritative: true,
                worktrees: vec![fact.clone()],
                warning: None,
            }),
            warning: None,
        };

        mark_snapshot_last_known(Some(&mut snapshot), "offline");

        assert_eq!(snapshot.target.key(), target.key());
        assert_eq!(snapshot.warning.as_deref(), Some("offline"));
        let root = project("p", "/repo", None);
        let location = fact_location(&target, &fact).unwrap();
        assert!(!row_from_fact(&root, "config", &snapshot, location, &fact, None).selectable);
        let configured = project("linked", "/linked", Some("p"));
        let location = fact_location(&target, &fact).unwrap();
        assert!(
            row_from_fact(
                &root,
                "config",
                &snapshot,
                location,
                &fact,
                Some(&configured),
            )
            .selectable
        );
        let CatalogInventory::Git(scan) = snapshot.inventory else {
            panic!("expected Git snapshot");
        };
        assert!(!scan.authoritative);
        assert_eq!(scan.source, WorktreeScanSource::LastKnown);
        assert_eq!(scan.warning.as_deref(), Some("offline"));
    }
}
