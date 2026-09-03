//! Read-only GitHub Issues and Pull Requests for the active project runtime.
//!
//! All Git and GitHub CLI commands run on the project's execution host. The
//! service owns source/repository generations and shared network data, while
//! the panel owns only worktree-scoped presentation.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{
    AnyElement, App, ClipboardItem, Context, Entity, InteractiveElement, IntoElement,
    ParentElement, Render, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Task,
    Window, div, prelude::FluentBuilder, px,
};
use gpui_component::ActiveTheme as _;
use gpui_component::scroll::Scrollbar;
use gpui_component::text::{TextView, TextViewStyle};
use mt_github::{
    COMMAND_OUTPUT_LIMIT, CommandExecutionError, CommandOutput, CommandPlan, CommandStage,
    DETAIL_OUTPUT_LIMIT, GitHubError, GitHubErrorKind, GitHubRepoIdentity, GitHubWorkItemDetail,
    GitHubWorkItemSummary, LIST_OUTPUT_LIMIT, WorkItemKind, WorkItemState, WorkItemStateFilter,
    account_plan, auth_login_command, auth_status_plan, classify_execution_error, detail_plan,
    discover_remote_plan, list_plan, parse_account, parse_remote_url, parse_work_item_detail,
    parse_work_item_list, require_success, version_plan,
};
use mt_identity::{ExecutionHostId, WorktreeId};
use mt_ui::icons::usage_glyphs::ICON_REFRESH;
use mt_ui::icons::vector::VectorIcon;
use mt_ui::tooltip::Tooltip;

use crate::execution_host::{
    ExecutionBackendSignature, ExecutionSourceSignature, HostCommandResult,
    ProjectExecutionSnapshot, execute_host_command,
};
use crate::store::AppStore;
use crate::ui;

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);
const AUTH_TIMEOUT: Duration = Duration::from_secs(12);
const LIST_TIMEOUT: Duration = Duration::from_secs(20);
const DETAIL_TIMEOUT: Duration = Duration::from_secs(15);

/// Only the exact value `0` restores the old unavailable placeholder.
pub fn github_project_tasks_enabled() -> bool {
    github_project_tasks_enabled_for(std::env::var_os("MINI_TERM_GITHUB_PROJECT_TASKS").as_deref())
}

fn github_project_tasks_enabled_for(value: Option<&OsStr>) -> bool {
    value.is_none_or(|value| value != "0")
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RepositoryCacheSource {
    execution_host_id: ExecutionHostId,
    root_project_id: String,
    root_source_path: String,
    backend: ExecutionBackendSignature,
}

impl From<&ExecutionSourceSignature> for RepositoryCacheSource {
    fn from(source: &ExecutionSourceSignature) -> Self {
        Self {
            execution_host_id: source.execution_host_id.clone(),
            root_project_id: source.root_project_id.clone(),
            root_source_path: source.root_source_path.clone(),
            backend: source.backend.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RepositoryCacheKey {
    source: RepositoryCacheSource,
    repository: GitHubRepoIdentity,
    account: String,
    auth_generation: u64,
}

impl RepositoryCacheKey {
    fn new(
        source: &ExecutionSourceSignature,
        repository: GitHubRepoIdentity,
        account: impl Into<String>,
        auth_generation: u64,
    ) -> Self {
        Self {
            source: source.into(),
            repository,
            account: account.into().to_ascii_lowercase(),
            auth_generation,
        }
    }
}

fn apply_auth_generation_rotation(
    scope: RepositoryCacheSource,
    generation: u64,
    auth_scopes: &mut HashMap<RepositoryCacheSource, u64>,
    sources: &mut HashMap<ExecutionSourceSignature, SourceRecord>,
    repositories: &mut HashMap<RepositoryCacheKey, RepositoryCache>,
) {
    auth_scopes.insert(scope.clone(), generation);
    repositories.retain(|key, _| key.source != scope);
    for (source_key, source) in sources {
        if RepositoryCacheSource::from(source_key) == scope {
            source.reset_auth(generation);
        }
    }
}

#[derive(Clone, Debug, Default)]
struct ListSlot {
    rows: Vec<GitHubWorkItemSummary>,
    loading: bool,
    error: Option<GitHubError>,
    request_id: u64,
    updated_at_unix_ms: Option<i64>,
}

#[derive(Clone, Debug, Default)]
struct DetailSlot {
    detail: Option<GitHubWorkItemDetail>,
    loading: bool,
    error: Option<GitHubError>,
    request_id: u64,
}

#[derive(Clone, Debug, Default)]
struct RepositoryCache {
    lists: HashMap<WorkItemKind, ListSlot>,
    details: HashMap<(WorkItemKind, u64), DetailSlot>,
}

#[derive(Clone, Debug)]
enum SourcePhase {
    Idle,
    Loading,
    Ready(Box<RepositoryCacheKey>),
    Error,
}

#[derive(Clone, Debug)]
struct SourceRecord {
    host_label: String,
    project_id: String,
    phase: SourcePhase,
    repository: Option<GitHubRepoIdentity>,
    account: Option<String>,
    error: Option<GitHubError>,
    auth_generation: u64,
    request_id: u64,
    last_known: Option<RepositoryCacheKey>,
}

impl SourceRecord {
    fn new(snapshot: &ProjectExecutionSnapshot, auth_generation: u64) -> Self {
        Self {
            host_label: snapshot.host_label.clone(),
            project_id: snapshot.project_id.clone(),
            phase: SourcePhase::Idle,
            repository: None,
            account: None,
            error: None,
            auth_generation,
            request_id: 0,
            last_known: None,
        }
    }

    fn reset_auth(&mut self, auth_generation: u64) {
        self.phase = SourcePhase::Idle;
        self.repository = None;
        self.account = None;
        self.error = None;
        self.auth_generation = auth_generation;
        self.last_known = None;
    }
}

#[derive(Clone, Debug)]
struct PipelineFailure {
    error: GitHubError,
    repository: Option<GitHubRepoIdentity>,
    observed_source: Box<ExecutionSourceSignature>,
}

#[derive(Clone, Debug)]
struct ListPipelineSuccess {
    repository: GitHubRepoIdentity,
    account: String,
    rows: Vec<GitHubWorkItemSummary>,
    observed_source: ExecutionSourceSignature,
}

#[derive(Clone, Debug)]
struct StageTracker {
    observed_epoch: Option<u64>,
}

impl StageTracker {
    fn new() -> Self {
        Self {
            observed_epoch: None,
        }
    }

    fn observe(&mut self, result: &HostCommandResult) -> Result<(), GitHubError> {
        let Some(epoch) = result.observed_connection_epoch else {
            return Ok(());
        };
        if self.observed_epoch.is_some_and(|current| current != epoch) {
            return Err(GitHubError::new(
                GitHubErrorKind::RepositoryChanged,
                "The SSH execution session changed while this request was running",
                true,
            ));
        }
        self.observed_epoch = Some(epoch);
        Ok(())
    }
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}

type HostCommandExecutor<'a> = dyn FnMut(
        &ProjectExecutionSnapshot,
        &CommandPlan,
        Duration,
        usize,
    ) -> Result<HostCommandResult, CommandExecutionError>
    + 'a;

fn run_stage_with(
    snapshot: &ProjectExecutionSnapshot,
    tracker: &mut StageTracker,
    stage: CommandStage,
    plan: &CommandPlan,
    timeout: Duration,
    output_cap: usize,
    execute: &mut HostCommandExecutor<'_>,
) -> Result<CommandOutput, GitHubError> {
    let result = execute(snapshot, plan, timeout, output_cap)
        .map_err(|error| classify_execution_error(stage, &error))?;
    tracker.observe(&result)?;
    require_success(stage, &result.output)?;
    Ok(result.output)
}

fn verify_repository_context(
    snapshot: &ProjectExecutionSnapshot,
    tracker: &mut StageTracker,
    expected_repository: &GitHubRepoIdentity,
    expected_account: &str,
    execute: &mut HostCommandExecutor<'_>,
) -> Result<(), GitHubError> {
    let remote = run_stage_with(
        snapshot,
        tracker,
        CommandStage::DiscoverRemote,
        &discover_remote_plan(),
        DISCOVERY_TIMEOUT,
        COMMAND_OUTPUT_LIMIT,
        execute,
    )?;
    let remote = std::str::from_utf8(&remote.stdout)
        .map_err(|_| GitHubError::malformed("Git remote discovery"))?;
    let current_repository =
        parse_remote_url(remote).map_err(|_| GitHubError::repository_changed())?;
    if current_repository != *expected_repository {
        return Err(GitHubError::repository_changed());
    }

    run_stage_with(
        snapshot,
        tracker,
        CommandStage::AuthStatus,
        &auth_status_plan(expected_repository.host()),
        AUTH_TIMEOUT,
        COMMAND_OUTPUT_LIMIT,
        execute,
    )?;
    let account_output = run_stage_with(
        snapshot,
        tracker,
        CommandStage::Account,
        &account_plan(expected_repository.host()),
        AUTH_TIMEOUT,
        COMMAND_OUTPUT_LIMIT,
        execute,
    )?;
    let current_account = parse_account(&account_output)?;
    if !current_account.eq_ignore_ascii_case(expected_account) {
        return Err(GitHubError::new(
            GitHubErrorKind::WrongHostOrAccount,
            "The active GitHub account changed while this request was running",
            true,
        ));
    }
    Ok(())
}

fn run_list_pipeline(
    snapshot: ProjectExecutionSnapshot,
    kind: WorkItemKind,
) -> Result<ListPipelineSuccess, PipelineFailure> {
    let mut execute = execute_host_command;
    run_list_pipeline_with(snapshot, kind, &mut execute)
}

fn run_list_pipeline_with(
    snapshot: ProjectExecutionSnapshot,
    kind: WorkItemKind,
    execute: &mut HostCommandExecutor<'_>,
) -> Result<ListPipelineSuccess, PipelineFailure> {
    let initial_source = snapshot.source_signature();
    let mut tracker = StageTracker::new();
    let mut repository = None;
    let result = (|| {
        let remote = run_stage_with(
            &snapshot,
            &mut tracker,
            CommandStage::DiscoverRemote,
            &discover_remote_plan(),
            DISCOVERY_TIMEOUT,
            COMMAND_OUTPUT_LIMIT,
            execute,
        )?;
        let remote = std::str::from_utf8(&remote.stdout)
            .map_err(|_| GitHubError::malformed("Git remote discovery"))?;
        let parsed = parse_remote_url(remote).map_err(|_| {
            GitHubError::new(
                GitHubErrorKind::NoGitHubRemote,
                "The origin remote is not a supported GitHub repository",
                true,
            )
        })?;
        repository = Some(parsed.clone());

        run_stage_with(
            &snapshot,
            &mut tracker,
            CommandStage::Version,
            &version_plan(),
            DISCOVERY_TIMEOUT,
            COMMAND_OUTPUT_LIMIT,
            execute,
        )?;
        run_stage_with(
            &snapshot,
            &mut tracker,
            CommandStage::AuthStatus,
            &auth_status_plan(parsed.host()),
            AUTH_TIMEOUT,
            COMMAND_OUTPUT_LIMIT,
            execute,
        )?;
        let account_output = run_stage_with(
            &snapshot,
            &mut tracker,
            CommandStage::Account,
            &account_plan(parsed.host()),
            AUTH_TIMEOUT,
            COMMAND_OUTPUT_LIMIT,
            execute,
        )?;
        let account = parse_account(&account_output)?;
        let list_output = run_stage_with(
            &snapshot,
            &mut tracker,
            CommandStage::List,
            &list_plan(&parsed, kind),
            LIST_TIMEOUT,
            LIST_OUTPUT_LIMIT,
            execute,
        )?;
        let rows = parse_work_item_list(kind, &list_output)?;
        verify_repository_context(&snapshot, &mut tracker, &parsed, &account, execute)?;
        Ok((parsed, account, rows))
    })();
    let observed_source = snapshot.observed_source_signature(tracker.observed_epoch);
    match result {
        Ok((repository, account, rows)) => Ok(ListPipelineSuccess {
            repository,
            account,
            rows,
            observed_source,
        }),
        Err(error) => Err(PipelineFailure {
            error,
            repository,
            observed_source: Box::new(if tracker.observed_epoch.is_some() {
                observed_source
            } else {
                initial_source
            }),
        }),
    }
}

fn run_list_only(
    snapshot: ProjectExecutionSnapshot,
    repository: GitHubRepoIdentity,
    account: String,
    kind: WorkItemKind,
) -> Result<(Vec<GitHubWorkItemSummary>, ExecutionSourceSignature), PipelineFailure> {
    let mut execute = execute_host_command;
    run_list_only_with(snapshot, repository, account, kind, &mut execute)
}

fn run_list_only_with(
    snapshot: ProjectExecutionSnapshot,
    repository: GitHubRepoIdentity,
    account: String,
    kind: WorkItemKind,
    execute: &mut HostCommandExecutor<'_>,
) -> Result<(Vec<GitHubWorkItemSummary>, ExecutionSourceSignature), PipelineFailure> {
    let mut tracker = StageTracker::new();
    let result = (|| {
        verify_repository_context(&snapshot, &mut tracker, &repository, &account, execute)?;
        let output = run_stage_with(
            &snapshot,
            &mut tracker,
            CommandStage::List,
            &list_plan(&repository, kind),
            LIST_TIMEOUT,
            LIST_OUTPUT_LIMIT,
            execute,
        )?;
        let rows = parse_work_item_list(kind, &output)?;
        verify_repository_context(&snapshot, &mut tracker, &repository, &account, execute)?;
        Ok(rows)
    })();
    let observed_source = snapshot.observed_source_signature(tracker.observed_epoch);
    result
        .map(|rows| (rows, observed_source.clone()))
        .map_err(|error| PipelineFailure {
            error,
            repository: Some(repository),
            observed_source: Box::new(observed_source),
        })
}

fn run_detail_only(
    snapshot: ProjectExecutionSnapshot,
    repository: GitHubRepoIdentity,
    account: String,
    kind: WorkItemKind,
    number: u64,
) -> Result<(GitHubWorkItemDetail, ExecutionSourceSignature), PipelineFailure> {
    let mut execute = execute_host_command;
    run_detail_only_with(snapshot, repository, account, kind, number, &mut execute)
}

fn run_detail_only_with(
    snapshot: ProjectExecutionSnapshot,
    repository: GitHubRepoIdentity,
    account: String,
    kind: WorkItemKind,
    number: u64,
    execute: &mut HostCommandExecutor<'_>,
) -> Result<(GitHubWorkItemDetail, ExecutionSourceSignature), PipelineFailure> {
    let mut tracker = StageTracker::new();
    let result = (|| {
        verify_repository_context(&snapshot, &mut tracker, &repository, &account, execute)?;
        let output = run_stage_with(
            &snapshot,
            &mut tracker,
            CommandStage::Detail,
            &detail_plan(&repository, kind, number),
            DETAIL_TIMEOUT,
            DETAIL_OUTPUT_LIMIT,
            execute,
        )?;
        let detail = parse_work_item_detail(kind, &output)?;
        verify_repository_context(&snapshot, &mut tracker, &repository, &account, execute)?;
        Ok(detail)
    })();
    let observed_source = snapshot.observed_source_signature(tracker.observed_epoch);
    result
        .map(|detail| (detail, observed_source.clone()))
        .map_err(|error| PipelineFailure {
            error,
            repository: Some(repository),
            observed_source: Box::new(observed_source),
        })
}

fn invalidates_repository_source(error: &GitHubError) -> bool {
    matches!(
        error.kind,
        GitHubErrorKind::NoGitHubRemote
            | GitHubErrorKind::ClientMissing
            | GitHubErrorKind::AuthRequired
            | GitHubErrorKind::WrongHostOrAccount
            | GitHubErrorKind::ScopeRequired
            | GitHubErrorKind::RepositoryChanged
    )
}

fn invalidates_list_source(error: &GitHubError) -> bool {
    invalidates_repository_source(error) || error.kind == GitHubErrorKind::NotFound
}

fn current_source_matches(
    store: &AppStore,
    project_id: &str,
    expected: &ExecutionSourceSignature,
) -> bool {
    store
        .project_execution_snapshot(project_id)
        .is_ok_and(|snapshot| snapshot.source_signature() == *expected)
}

/// Immutable list projection consumed by the panel.
#[derive(Clone, Debug)]
pub struct GitHubListView {
    pub host_label: String,
    pub repository: Option<GitHubRepoIdentity>,
    pub account: Option<String>,
    pub source: ExecutionSourceSignature,
    pub auth_generation: u64,
    pub rows: Vec<GitHubWorkItemSummary>,
    pub loading: bool,
    pub error: Option<GitHubError>,
    pub updated_at_unix_ms: Option<i64>,
    pub interactive: bool,
}

/// Exact request identity carried from a list row into a workbench tab.
#[derive(Clone, Debug)]
pub struct OpenGitHubWorkItem {
    pub project_id: String,
    pub worktree_id: WorktreeId,
    pub source: ExecutionSourceSignature,
    pub repository: GitHubRepoIdentity,
    pub account: String,
    pub auth_generation: u64,
    pub summary: GitHubWorkItemSummary,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GitHubWorkItemTabKey {
    pub worktree_id: WorktreeId,
    pub repository: GitHubRepoIdentity,
    pub kind: WorkItemKind,
    pub number: u64,
}

impl OpenGitHubWorkItem {
    pub fn tab_key(&self) -> GitHubWorkItemTabKey {
        GitHubWorkItemTabKey {
            worktree_id: self.worktree_id.clone(),
            repository: self.repository.clone(),
            kind: self.summary.kind,
            number: self.summary.number,
        }
    }

    fn repository_cache_key(&self) -> RepositoryCacheKey {
        RepositoryCacheKey::new(
            &self.source,
            self.repository.clone(),
            self.account.clone(),
            self.auth_generation,
        )
    }
}

fn source_is_ready(
    source: Option<&SourceRecord>,
    cache_key: &RepositoryCacheKey,
    auth_generation: u64,
) -> bool {
    source.is_some_and(|source| {
        source.auth_generation == auth_generation
            && matches!(&source.phase, SourcePhase::Ready(current) if current.as_ref() == cache_key)
    })
}

#[derive(Clone, Debug)]
pub struct GitHubDetailView {
    pub detail: Option<GitHubWorkItemDetail>,
    pub loading: bool,
    pub error: Option<GitHubError>,
}

/// Process-local shared cache and single-flight owner.
pub struct GitHubTaskService {
    store: Entity<AppStore>,
    sources: HashMap<ExecutionSourceSignature, SourceRecord>,
    auth_scopes: HashMap<RepositoryCacheSource, u64>,
    repositories: HashMap<RepositoryCacheKey, RepositoryCache>,
    next_request_id: u64,
    next_auth_generation: u64,
    _tasks: Vec<Task<()>>,
}

impl GitHubTaskService {
    pub fn new(store: Entity<AppStore>) -> Self {
        Self {
            store,
            sources: HashMap::new(),
            auth_scopes: HashMap::new(),
            repositories: HashMap::new(),
            next_request_id: 0,
            next_auth_generation: 0,
            _tasks: Vec::new(),
        }
    }

    fn allocate_request(&mut self) -> u64 {
        self.next_request_id = self.next_request_id.checked_add(1).unwrap_or_else(|| {
            self.sources.clear();
            self.auth_scopes.clear();
            self.repositories.clear();
            1
        });
        self.next_request_id
    }

    fn allocate_auth_generation(&mut self) -> u64 {
        self.next_auth_generation = self.next_auth_generation.checked_add(1).unwrap_or_else(|| {
            self.sources.clear();
            self.auth_scopes.clear();
            self.repositories.clear();
            1
        });
        self.next_auth_generation
    }

    fn auth_generation_for_source(&mut self, source: &ExecutionSourceSignature) -> u64 {
        let scope = RepositoryCacheSource::from(source);
        if let Some(generation) = self.auth_scopes.get(&scope) {
            return *generation;
        }
        let generation = self.allocate_auth_generation();
        self.auth_scopes.insert(scope, generation);
        generation
    }

    fn rotate_auth_generation(&mut self, source: &ExecutionSourceSignature) -> u64 {
        let scope = RepositoryCacheSource::from(source);
        let generation = self.allocate_auth_generation();
        apply_auth_generation_rotation(
            scope,
            generation,
            &mut self.auth_scopes,
            &mut self.sources,
            &mut self.repositories,
        );
        generation
    }

    fn mark_list_request_stale(
        &mut self,
        cache_key: &RepositoryCacheKey,
        kind: WorkItemKind,
        request_id: u64,
    ) {
        if let Some(slot) = self
            .repositories
            .get_mut(cache_key)
            .and_then(|cache| cache.lists.get_mut(&kind))
            && slot.request_id == request_id
            && slot.loading
        {
            slot.loading = false;
            slot.error = Some(GitHubError::repository_changed());
        }
    }

    fn mark_detail_request_stale(
        &mut self,
        cache_key: &RepositoryCacheKey,
        item_key: (WorkItemKind, u64),
        request_id: u64,
    ) {
        if let Some(slot) = self
            .repositories
            .get_mut(cache_key)
            .and_then(|cache| cache.details.get_mut(&item_key))
            && slot.request_id == request_id
            && slot.loading
        {
            slot.loading = false;
            slot.error = Some(GitHubError::repository_changed());
        }
    }

    fn invalidate_ready_source(
        &mut self,
        source_key: &ExecutionSourceSignature,
        cache_key: &RepositoryCacheKey,
        error: GitHubError,
    ) {
        let Some(source) = self.sources.get_mut(source_key) else {
            return;
        };
        if matches!(&source.phase, SourcePhase::Ready(current) if current.as_ref() == cache_key) {
            source.error = Some(error);
            source.phase = SourcePhase::Error;
        }
    }

    pub fn ensure_list(
        &mut self,
        snapshot: ProjectExecutionSnapshot,
        kind: WorkItemKind,
        force: bool,
        cx: &mut Context<Self>,
    ) {
        let source_key = snapshot.source_signature();
        if !self.sources.contains_key(&source_key) {
            let auth_generation = self.auth_generation_for_source(&source_key);
            self.sources.insert(
                source_key.clone(),
                SourceRecord::new(&snapshot, auth_generation),
            );
        }

        if force {
            let auth_generation = self.rotate_auth_generation(&source_key);
            self.sources
                .entry(source_key.clone())
                .or_insert_with(|| SourceRecord::new(&snapshot, auth_generation));
        }

        let ready_key = self.sources.get(&source_key).and_then(|source| {
            if let SourcePhase::Ready(key) = &source.phase {
                Some((key.as_ref().clone(), source.auth_generation))
            } else {
                None
            }
        });
        if let Some((cache_key, auth_generation)) = ready_key {
            self.ensure_ready_list(snapshot, cache_key, auth_generation, kind, force, cx);
            return;
        }

        let should_start = self
            .sources
            .get(&source_key)
            .is_some_and(|source| matches!(source.phase, SourcePhase::Idle));
        if !should_start {
            return;
        }
        let request_id = self.allocate_request();
        let auth_generation = self.sources[&source_key].auth_generation;
        if let Some(source) = self.sources.get_mut(&source_key) {
            source.phase = SourcePhase::Loading;
            source.request_id = request_id;
            source.error = None;
            source.project_id = snapshot.project_id.clone();
            source.host_label = snapshot.host_label.clone();
        }
        cx.notify();

        let project_id = snapshot.project_id.clone();
        let request_source = source_key.clone();
        let store = self.store.clone();
        self._tasks.push(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { run_list_pipeline(snapshot, kind) })
                .await;
            let _ = this.update(cx, |service, cx| {
                let Some(source) = service.sources.get(&request_source) else {
                    return;
                };
                if source.request_id != request_id
                    || source.auth_generation != auth_generation
                    || !matches!(source.phase, SourcePhase::Loading)
                {
                    return;
                }
                let observed_source = match &result {
                    Ok(success) => success.observed_source.clone(),
                    Err(failure) => failure.observed_source.as_ref().clone(),
                };
                if !current_source_matches(store.read(cx), &project_id, &observed_source) {
                    let owns_request = service.sources.get(&request_source).is_some_and(|source| {
                        source.request_id == request_id
                            && source.auth_generation == auth_generation
                            && matches!(source.phase, SourcePhase::Loading)
                    });
                    if owns_request {
                        service.sources.remove(&request_source);
                    }
                    cx.notify();
                    return;
                }

                let observed_scope = RepositoryCacheSource::from(&observed_source);
                let current_auth_generation = service
                    .auth_scopes
                    .entry(observed_scope)
                    .or_insert(auth_generation);
                if *current_auth_generation != auth_generation {
                    service.sources.remove(&request_source);
                    cx.notify();
                    return;
                }

                let Some(mut source) = service.sources.remove(&request_source) else {
                    return;
                };
                match result {
                    Ok(success) => {
                        let cache_key = RepositoryCacheKey::new(
                            &success.observed_source,
                            success.repository.clone(),
                            success.account.clone(),
                            auth_generation,
                        );
                        let cache = service.repositories.entry(cache_key.clone()).or_default();
                        cache.lists.insert(
                            kind,
                            ListSlot {
                                rows: success.rows,
                                loading: false,
                                error: None,
                                request_id,
                                updated_at_unix_ms: Some(now_unix_ms()),
                            },
                        );
                        source.repository = Some(success.repository);
                        source.account = Some(success.account);
                        source.error = None;
                        source.last_known = Some(cache_key.clone());
                        source.phase = SourcePhase::Ready(Box::new(cache_key));
                    }
                    Err(failure) => {
                        source.repository = failure.repository;
                        source.account = None;
                        source.error = Some(failure.error);
                        source.phase = SourcePhase::Error;
                    }
                }
                service.sources.insert(observed_source, source);
                cx.notify();
            });
        }));
    }

    fn ensure_ready_list(
        &mut self,
        snapshot: ProjectExecutionSnapshot,
        cache_key: RepositoryCacheKey,
        auth_generation: u64,
        kind: WorkItemKind,
        force: bool,
        cx: &mut Context<Self>,
    ) {
        let slot = self
            .repositories
            .entry(cache_key.clone())
            .or_default()
            .lists
            .entry(kind)
            .or_default();
        if slot.loading || (!force && slot.error.is_none() && slot.updated_at_unix_ms.is_some()) {
            return;
        }
        let request_id = self.allocate_request();
        let slot = self
            .repositories
            .entry(cache_key.clone())
            .or_default()
            .lists
            .entry(kind)
            .or_default();
        slot.loading = true;
        slot.error = None;
        slot.request_id = request_id;
        cx.notify();

        let project_id = snapshot.project_id.clone();
        let repository = cache_key.repository.clone();
        let account = cache_key.account.clone();
        let expected_source = snapshot.source_signature();
        let store = self.store.clone();
        self._tasks.push(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { run_list_only(snapshot, repository, account, kind) })
                .await;
            let _ = this.update(cx, |service, cx| {
                let observed_source = match &result {
                    Ok((_, source)) => source,
                    Err(failure) => failure.observed_source.as_ref(),
                };
                if *observed_source != expected_source
                    || !current_source_matches(store.read(cx), &project_id, observed_source)
                {
                    service.mark_list_request_stale(&cache_key, kind, request_id);
                    cx.notify();
                    return;
                }
                let source_ready = source_is_ready(
                    service.sources.get(&expected_source),
                    &cache_key,
                    auth_generation,
                );
                if !source_ready {
                    service.mark_list_request_stale(&cache_key, kind, request_id);
                    cx.notify();
                    return;
                }
                let Some(slot) = service
                    .repositories
                    .get_mut(&cache_key)
                    .and_then(|cache| cache.lists.get_mut(&kind))
                else {
                    return;
                };
                if slot.request_id != request_id || !slot.loading {
                    return;
                }
                slot.loading = false;
                let mut source_error = None;
                match result {
                    Ok((rows, _)) => {
                        slot.rows = rows;
                        slot.error = None;
                        slot.updated_at_unix_ms = Some(now_unix_ms());
                    }
                    Err(failure) => {
                        let error = failure.error;
                        if !error.retains_last_known() {
                            slot.rows.clear();
                            slot.updated_at_unix_ms = None;
                        }
                        if invalidates_list_source(&error) {
                            source_error = Some(error.clone());
                        }
                        slot.error = Some(error);
                    }
                }
                if let Some(error) = source_error {
                    service.invalidate_ready_source(&expected_source, &cache_key, error);
                }
                cx.notify();
            });
        }));
    }

    pub fn list_view(
        &self,
        snapshot: &ProjectExecutionSnapshot,
        kind: WorkItemKind,
    ) -> GitHubListView {
        let source_key = snapshot.source_signature();
        let Some(source) = self.sources.get(&source_key) else {
            return GitHubListView {
                host_label: snapshot.host_label.clone(),
                repository: None,
                account: None,
                source: source_key,
                auth_generation: 0,
                rows: Vec::new(),
                loading: false,
                error: None,
                updated_at_unix_ms: None,
                interactive: false,
            };
        };
        let mut view = GitHubListView {
            host_label: source.host_label.clone(),
            repository: source.repository.clone(),
            account: source.account.clone(),
            source: source_key,
            auth_generation: source.auth_generation,
            rows: Vec::new(),
            loading: matches!(source.phase, SourcePhase::Loading),
            error: source.error.clone(),
            updated_at_unix_ms: None,
            interactive: matches!(source.phase, SourcePhase::Ready(_)),
        };
        let cache_key = match &source.phase {
            SourcePhase::Ready(key) => Some(key.as_ref()),
            SourcePhase::Error => source.last_known.as_ref().filter(|_| {
                source
                    .error
                    .as_ref()
                    .is_some_and(GitHubError::retains_last_known)
            }),
            SourcePhase::Idle | SourcePhase::Loading => source.last_known.as_ref(),
        };
        if let Some(cache_key) = cache_key
            && let Some(slot) = self
                .repositories
                .get(cache_key)
                .and_then(|cache| cache.lists.get(&kind))
        {
            view.repository = Some(cache_key.repository.clone());
            view.account = Some(cache_key.account.clone());
            view.rows = slot.rows.clone();
            view.loading |= slot.loading;
            if slot.error.is_some() {
                view.error = slot.error.clone();
            }
            view.updated_at_unix_ms = slot.updated_at_unix_ms;
        }
        view
    }

    pub fn ensure_detail(&mut self, request: OpenGitHubWorkItem, cx: &mut Context<Self>) {
        let cache_key = request.repository_cache_key();
        let source_ready = source_is_ready(
            self.sources.get(&request.source),
            &cache_key,
            request.auth_generation,
        );
        if !source_ready {
            return;
        }
        let item_key = (request.summary.kind, request.summary.number);
        let slot = self
            .repositories
            .entry(cache_key.clone())
            .or_default()
            .details
            .entry(item_key)
            .or_default();
        if slot.loading || (slot.detail.is_some() && slot.error.is_none()) {
            return;
        }
        let snapshot = match self
            .store
            .read(cx)
            .project_execution_snapshot(&request.project_id)
        {
            Ok(snapshot) if snapshot.source_signature() == request.source => snapshot,
            _ => return,
        };
        let request_id = self.allocate_request();
        let slot = self
            .repositories
            .entry(cache_key.clone())
            .or_default()
            .details
            .entry(item_key)
            .or_default();
        slot.loading = true;
        slot.error = None;
        slot.request_id = request_id;
        cx.notify();

        let project_id = request.project_id.clone();
        let repository = request.repository.clone();
        let account = request.account.clone();
        let expected_source = request.source.clone();
        let store = self.store.clone();
        self._tasks.push(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    run_detail_only(snapshot, repository, account, item_key.0, item_key.1)
                })
                .await;
            let _ = this.update(cx, |service, cx| {
                let observed_source = match &result {
                    Ok((_, source)) => source,
                    Err(failure) => failure.observed_source.as_ref(),
                };
                if *observed_source != expected_source
                    || !current_source_matches(store.read(cx), &project_id, observed_source)
                {
                    service.mark_detail_request_stale(&cache_key, item_key, request_id);
                    cx.notify();
                    return;
                }
                let source_ready = source_is_ready(
                    service.sources.get(&expected_source),
                    &cache_key,
                    request.auth_generation,
                );
                if !source_ready {
                    service.mark_detail_request_stale(&cache_key, item_key, request_id);
                    cx.notify();
                    return;
                }
                let Some(slot) = service
                    .repositories
                    .get_mut(&cache_key)
                    .and_then(|cache| cache.details.get_mut(&item_key))
                else {
                    return;
                };
                if slot.request_id != request_id || !slot.loading {
                    return;
                }
                slot.loading = false;
                let mut source_error = None;
                match result {
                    Ok((detail, _)) => {
                        slot.detail = Some(detail);
                        slot.error = None;
                    }
                    Err(failure) => {
                        let error = failure.error;
                        if invalidates_repository_source(&error) {
                            source_error = Some(error.clone());
                        }
                        slot.error = Some(error);
                    }
                }
                if let Some(error) = source_error {
                    service.invalidate_ready_source(&expected_source, &cache_key, error);
                }
                cx.notify();
            });
        }));
    }

    pub fn detail_view(&self, request: &OpenGitHubWorkItem) -> GitHubDetailView {
        let cache_key = request.repository_cache_key();
        let Some(source) = self.sources.get(&request.source) else {
            return GitHubDetailView {
                detail: None,
                loading: false,
                error: Some(GitHubError::repository_changed()),
            };
        };
        if !source_is_ready(Some(source), &cache_key, request.auth_generation) {
            return GitHubDetailView {
                detail: None,
                loading: false,
                error: Some(
                    source
                        .error
                        .clone()
                        .unwrap_or_else(GitHubError::repository_changed),
                ),
            };
        }
        let slot = self.repositories.get(&cache_key).and_then(|cache| {
            cache
                .details
                .get(&(request.summary.kind, request.summary.number))
        });
        GitHubDetailView {
            detail: slot.and_then(|slot| slot.detail.clone()),
            loading: slot.is_none_or(|slot| slot.loading),
            error: slot.and_then(|slot| slot.error.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SelectedWorkItem {
    repository: GitHubRepoIdentity,
    kind: WorkItemKind,
    number: u64,
}

struct PanelScopeState {
    mode: WorkItemKind,
    filter: WorkItemStateFilter,
    selected: Option<SelectedWorkItem>,
    scroll: ScrollHandle,
}

/// Right-side Tasks surface. Network data lives in [`GitHubTaskService`].
pub struct GitHubTasksPanel {
    store: Entity<AppStore>,
    service: Entity<GitHubTaskService>,
    current_project: Option<String>,
    current_worktree: Option<WorktreeId>,
    current_source: Option<ExecutionSourceSignature>,
    mode: WorkItemKind,
    filter: WorkItemStateFilter,
    selected: Option<SelectedWorkItem>,
    scroll: ScrollHandle,
    scope_cache: HashMap<WorktreeId, PanelScopeState>,
    visible: bool,
}

impl GitHubTasksPanel {
    pub fn new(
        store: Entity<AppStore>,
        service: Entity<GitHubTaskService>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&store, |this, _, cx| {
            if this.visible {
                this.sync_scope(cx);
            }
            cx.notify();
        })
        .detach();
        cx.observe(&service, |_this, _, cx| cx.notify()).detach();
        Self {
            store,
            service,
            current_project: None,
            current_worktree: None,
            current_source: None,
            mode: WorkItemKind::Issue,
            filter: WorkItemStateFilter::Open,
            selected: None,
            scroll: ScrollHandle::new(),
            scope_cache: HashMap::new(),
            visible: false,
        }
    }

    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if visible {
            self.sync_scope(cx);
            self.ensure_current(false, cx);
        }
        cx.notify();
    }

    fn active_snapshot(&self, cx: &App) -> Option<ProjectExecutionSnapshot> {
        let store = self.store.read(cx);
        let project_id = store.active_project_id.as_deref()?;
        store.project_execution_snapshot(project_id).ok()
    }

    fn save_scope(&mut self) {
        let Some(worktree_id) = self.current_worktree.clone() else {
            return;
        };
        self.scope_cache.insert(
            worktree_id,
            PanelScopeState {
                mode: self.mode,
                filter: self.filter,
                selected: self.selected.take(),
                scroll: std::mem::replace(&mut self.scroll, ScrollHandle::new()),
            },
        );
    }

    fn restore_scope(&mut self, worktree_id: Option<&WorktreeId>) {
        if let Some(state) = worktree_id.and_then(|id| self.scope_cache.remove(id)) {
            self.mode = state.mode;
            self.filter = state.filter;
            self.selected = state.selected;
            self.scroll = state.scroll;
        } else {
            self.mode = WorkItemKind::Issue;
            self.filter = WorkItemStateFilter::Open;
            self.selected = None;
            self.scroll = ScrollHandle::new();
        }
    }

    fn sync_scope(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.active_snapshot(cx);
        let next_project = snapshot
            .as_ref()
            .map(|snapshot| snapshot.project_id.clone());
        let next_worktree = snapshot
            .as_ref()
            .map(|snapshot| snapshot.worktree_id.clone());
        let next_source = snapshot
            .as_ref()
            .map(ProjectExecutionSnapshot::source_signature);
        if self.current_project == next_project
            && self.current_worktree == next_worktree
            && self.current_source == next_source
        {
            return;
        }
        self.save_scope();
        self.restore_scope(next_worktree.as_ref());
        self.current_project = next_project;
        self.current_worktree = next_worktree;
        self.current_source = next_source;
        self.ensure_current(false, cx);
    }

    fn ensure_current(&mut self, force: bool, cx: &mut Context<Self>) {
        let Some(snapshot) = self.active_snapshot(cx) else {
            return;
        };
        let mode = self.mode;
        self.service.update(cx, |service, cx| {
            service.ensure_list(snapshot, mode, force, cx)
        });
    }

    fn set_mode(&mut self, mode: WorkItemKind, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        self.ensure_current(false, cx);
        cx.notify();
    }

    fn set_filter(&mut self, filter: WorkItemStateFilter, cx: &mut Context<Self>) {
        if self.filter == filter {
            return;
        }
        self.filter = filter;
        cx.notify();
    }

    fn open_row(
        &mut self,
        view: &GitHubListView,
        row: GitHubWorkItemSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(project_id), Some(worktree_id), Some(repository), Some(account)) = (
            self.current_project.clone(),
            self.current_worktree.clone(),
            view.repository.clone(),
            view.account.clone(),
        ) else {
            return;
        };
        self.selected = Some(SelectedWorkItem {
            repository: repository.clone(),
            kind: row.kind,
            number: row.number,
        });
        crate::workbench_area::open_github_work_item(
            self.service.clone(),
            OpenGitHubWorkItem {
                project_id,
                worktree_id,
                source: view.source.clone(),
                repository,
                account,
                auth_generation: view.auth_generation,
                summary: row,
            },
            window,
            cx,
        );
        cx.notify();
    }

    fn render_segment(
        &self,
        id: SharedString,
        label: &'static str,
        active: bool,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .h(px(25.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(3.0))
            .text_size(ui::font_px(10.0))
            .cursor_pointer()
            .when(active, |el| {
                el.bg(ui::accent_muted()).text_color(ui::text_primary())
            })
            .when(!active, |el| {
                el.text_color(ui::text_muted())
                    .hover(|el| el.bg(ui::border_subtle()).text_color(ui::text_primary()))
            })
            .child(label)
    }

    fn render_toolbar(&self, loading: bool, cx: &mut Context<Self>) -> AnyElement {
        let mut modes = div()
            .flex()
            .items_center()
            .p(px(2.0))
            .rounded(px(4.0))
            .bg(ui::bg_elevated());
        for kind in WorkItemKind::ALL {
            modes = modes.child(
                self.render_segment(
                    SharedString::from(format!("github-mode-{:?}", kind)),
                    kind.short_label(),
                    self.mode == kind,
                )
                .on_click(cx.listener(move |this, _, _window, cx| this.set_mode(kind, cx))),
            );
        }

        let mut filters = div()
            .flex()
            .items_center()
            .p(px(2.0))
            .rounded(px(4.0))
            .bg(ui::bg_elevated());
        for filter in WorkItemStateFilter::ALL {
            filters = filters.child(
                self.render_segment(
                    SharedString::from(format!("github-filter-{:?}", filter)),
                    filter.label(),
                    self.filter == filter,
                )
                .on_click(cx.listener(move |this, _, _window, cx| this.set_filter(filter, cx))),
            );
        }

        div()
            .h(px(38.0))
            .flex_none()
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .border_b_1()
            .border_color(ui::border_subtle())
            .child(modes)
            .child(filters)
            .child(div().flex_1())
            .child(
                div()
                    .id("github-tasks-refresh")
                    .w(px(26.0))
                    .h(px(26.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .text_color(ui::text_muted())
                    .when(loading, |el| el.opacity(0.45))
                    .when(!loading, |el| {
                        el.cursor_pointer()
                            .hover(|el| el.bg(ui::border_subtle()))
                            .on_click(
                                cx.listener(|this, _, _window, cx| this.ensure_current(true, cx)),
                            )
                    })
                    .tooltip(|window, cx| {
                        Tooltip::new("Refresh GitHub tasks")
                            .instant()
                            .build(window, cx)
                    })
                    .child(VectorIcon::new(ICON_REFRESH, px(14.0)).ink(ui::text_muted())),
            )
            .into_any_element()
    }

    fn render_status(
        &self,
        title: impl Into<String>,
        body: impl Into<String>,
        retry: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .px(px(18.0))
            .text_center()
            .child(
                div()
                    .text_size(ui::font_px(12.0))
                    .text_color(ui::text_primary())
                    .child(title.into()),
            )
            .child(
                div()
                    .text_size(ui::font_px(10.0))
                    .text_color(ui::text_muted())
                    .child(body.into()),
            )
            .when(retry, |el| {
                el.child(
                    ui::ghost_button("github-tasks-retry", "Retry").on_click(
                        cx.listener(|this, _, _window, cx| this.ensure_current(true, cx)),
                    ),
                )
            })
            .into_any_element()
    }

    fn render_auth_required(&self, view: &GitHubListView, cx: &mut Context<Self>) -> AnyElement {
        let host = view
            .repository
            .as_ref()
            .map(GitHubRepoIdentity::host)
            .unwrap_or("github.com");
        let command = auth_login_command(host);
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(9.0))
            .px(px(18.0))
            .text_center()
            .child(
                div()
                    .text_size(ui::font_px(12.0))
                    .text_color(ui::text_primary())
                    .child("GitHub CLI sign-in required"),
            )
            .child(
                div()
                    .text_size(ui::font_px(10.0))
                    .text_color(ui::text_muted())
                    .child(format!("Run this on {}:", view.host_label)),
            )
            .child(
                div()
                    .w_full()
                    .px(px(9.0))
                    .py(px(7.0))
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(ui::border_default())
                    .bg(ui::bg_elevated())
                    .text_size(ui::font_px(10.0))
                    .text_color(ui::text_secondary())
                    .child(command.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(ui::ghost_button("github-auth-copy", "Copy").on_click(
                        move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(command.clone()));
                        },
                    ))
                    .child(ui::ghost_button("github-auth-retry", "Retry").on_click(
                        cx.listener(|this, _, _window, cx| this.ensure_current(true, cx)),
                    )),
            )
            .into_any_element()
    }

    fn render_error(&self, view: &GitHubListView, cx: &mut Context<Self>) -> AnyElement {
        let Some(error) = view.error.as_ref() else {
            return self.render_status("GitHub Tasks", "No data is available", true, cx);
        };
        if error.kind == GitHubErrorKind::AuthRequired {
            return self.render_auth_required(view, cx);
        }
        let title = match error.kind {
            GitHubErrorKind::NoGitHubRemote => "No GitHub repository",
            GitHubErrorKind::ClientMissing => "GitHub CLI is unavailable",
            GitHubErrorKind::WrongHostOrAccount => "GitHub account mismatch",
            GitHubErrorKind::ScopeRequired => "Additional GitHub scope required",
            GitHubErrorKind::RateLimited => "GitHub rate limit reached",
            GitHubErrorKind::Offline => "Execution host is offline",
            GitHubErrorKind::NotFound => "Repository or item not found",
            GitHubErrorKind::MalformedResponse => "GitHub returned invalid data",
            GitHubErrorKind::RepositoryChanged => "Repository context changed",
            GitHubErrorKind::CommandFailed => "GitHub command failed",
            GitHubErrorKind::AuthRequired => unreachable!(),
        };
        self.render_status(title, error.summary.clone(), error.retryable, cx)
    }

    fn render_row(
        &self,
        view: GitHubListView,
        row: GitHubWorkItemSummary,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.selected.as_ref().is_some_and(|selected| {
            view.repository.as_ref() == Some(&selected.repository)
                && selected.kind == row.kind
                && selected.number == row.number
        });
        let state_color = match row.state {
            WorkItemState::Open => ui::color_success(),
            WorkItemState::Merged => ui::color_info(),
            WorkItemState::Closed => ui::text_muted(),
        };
        let author = row.author.clone().unwrap_or_else(|| "unknown".into());
        let time = chrono::DateTime::parse_from_rfc3339(&row.updated_at)
            .ok()
            .map(|timestamp| {
                crate::git_history::format_relative_time(
                    timestamp.timestamp(),
                    chrono::Utc::now().timestamp(),
                )
            })
            .unwrap_or_default();
        let row_for_click = row.clone();
        let interactive = view.interactive;
        div()
            .id(SharedString::from(format!(
                "github-row-{:?}-{}",
                row.kind, row.number
            )))
            .w_full()
            .px(px(9.0))
            .py(px(8.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .border_b_1()
            .border_color(ui::border_subtle())
            .when(selected, |el| el.bg(ui::accent_subtle()))
            .when(!interactive, |el| el.opacity(0.72))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(6.0))
                    .child(
                        div()
                            .flex_none()
                            .text_size(ui::font_px(10.0))
                            .text_color(state_color)
                            .child(format!("#{}", row.number)),
                    )
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_1()
                            .text_size(ui::font_px(11.0))
                            .text_color(ui::text_primary())
                            .child(row.title.clone()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .text_size(ui::font_px(9.0))
                    .text_color(ui::text_muted())
                    .child(row.state.label())
                    .child(format!("@{author}"))
                    .when(!time.is_empty(), |el| el.child(time))
                    .when(row.is_draft, |el| el.child("Draft"))
                    .when(!row.labels.is_empty(), |el| {
                        el.child(
                            row.labels
                                .iter()
                                .take(2)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", "),
                        )
                    }),
            )
            .when(interactive, |el| {
                el.cursor_pointer()
                    .when(!selected, |el| el.hover(|el| el.bg(ui::border_subtle())))
                    .on_click(cx.listener(move |this, _event, window, cx| {
                        this.open_row(&view, row_for_click.clone(), window, cx)
                    }))
            })
            .into_any_element()
    }
}

impl Render for GitHubTasksPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_scope(cx);
        let snapshot = self.active_snapshot(cx);
        let view = snapshot
            .as_ref()
            .map(|snapshot| self.service.read(cx).list_view(snapshot, self.mode));
        let loading = view.as_ref().is_some_and(|view| view.loading);
        let body = match view {
            None => self.render_status("No active project", "Select a project worktree", false, cx),
            Some(view) if view.error.is_some() && view.rows.is_empty() => {
                self.render_error(&view, cx)
            }
            Some(view) if view.loading && view.rows.is_empty() => {
                self.render_status("Loading GitHub Tasks", view.host_label.clone(), false, cx)
            }
            Some(view) => {
                let filtered = view
                    .rows
                    .iter()
                    .filter(|row| self.filter.matches(row))
                    .cloned()
                    .collect::<Vec<_>>();
                if filtered.is_empty() {
                    self.render_status(
                        format!("No {}", self.mode.label()),
                        format!("No {} items match this filter", self.filter.label()),
                        false,
                        cx,
                    )
                } else {
                    let mut list = div().id("github-tasks-list").w_full().flex().flex_col();
                    if let Some(error) = view.error.as_ref() {
                        let stale = view.updated_at_unix_ms.map(|timestamp| {
                            format!("Last updated {}", format_refresh_time(timestamp))
                        });
                        list = list.child(
                            div()
                                .px(px(9.0))
                                .py(px(6.0))
                                .bg(ui::with_alpha(ui::color_warning(), 0.10))
                                .text_size(ui::font_px(9.0))
                                .text_color(ui::color_warning())
                                .child(error.summary.clone())
                                .when_some(stale, |el, stale| el.child(stale)),
                        );
                    }
                    for row in filtered {
                        list = list.child(self.render_row(view.clone(), row, cx));
                    }
                    div()
                        .size_full()
                        .relative()
                        .overflow_hidden()
                        .child(list.track_scroll(&self.scroll).overflow_y_scroll())
                        .child(Scrollbar::vertical(&self.scroll).id("github-tasks-scrollbar"))
                        .into_any_element()
                }
            }
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(ui::bg_surface())
            .child(self.render_toolbar(loading, cx))
            .child(div().flex_1().min_h(px(0.0)).overflow_hidden().child(body))
    }
}

fn format_refresh_time(timestamp_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp_ms)
        .map(|timestamp| {
            timestamp
                .with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "earlier".into())
}

fn detail_text_style(cx: &mut App) -> TextViewStyle {
    let mut code_block = gpui::StyleRefinement::default();
    {
        let text = code_block.text.get_or_insert_default();
        text.font_size = Some(ui::font_px(11.0).into());
        text.line_height = Some(gpui::relative(1.5));
    }
    TextViewStyle {
        highlight_theme: cx.theme().highlight_theme.clone(),
        is_dark: cx.theme().mode.is_dark(),
        heading_base_font_size: ui::font_px(13.0),
        paragraph_gap: gpui::rems(0.65),
        code_block,
        ..Default::default()
    }
}

/// Read-only, internal work-item detail surface. It exposes no URL action.
pub struct GitHubWorkItemViewer {
    service: Entity<GitHubTaskService>,
    request: OpenGitHubWorkItem,
    scroll: ScrollHandle,
}

impl GitHubWorkItemViewer {
    pub fn new(
        service: Entity<GitHubTaskService>,
        request: OpenGitHubWorkItem,
        cx: &mut Context<Self>,
    ) -> Self {
        service.update(cx, |service, cx| service.ensure_detail(request.clone(), cx));
        cx.observe(&service, |_this, _, cx| cx.notify()).detach();
        Self {
            service,
            request,
            scroll: ScrollHandle::new(),
        }
    }

    pub fn matches_request(&self, request: &OpenGitHubWorkItem) -> bool {
        self.request.project_id == request.project_id
            && self.request.worktree_id == request.worktree_id
            && self.request.source == request.source
            && self.request.repository == request.repository
            && self.request.account == request.account
            && self.request.auth_generation == request.auth_generation
            && self.request.summary.kind == request.summary.kind
            && self.request.summary.number == request.summary.number
    }
}

impl Render for GitHubWorkItemViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.service.read(cx).detail_view(&self.request);
        let summary = view
            .detail
            .as_ref()
            .map(|detail| &detail.summary)
            .unwrap_or(&self.request.summary);
        let body = if let Some(detail) = view.detail.as_ref() {
            let sanitized = crate::file_viewer::sanitize_github_markdown(&detail.body);
            if sanitized.trim().is_empty() {
                div()
                    .py(px(24.0))
                    .text_size(ui::font_px(11.0))
                    .text_color(ui::text_muted())
                    .child("No description provided")
                    .into_any_element()
            } else {
                TextView::markdown(
                    SharedString::from(format!(
                        "github-detail-{:?}-{}",
                        summary.kind, summary.number
                    )),
                    sanitized,
                    window,
                    cx,
                )
                .style(detail_text_style(cx))
                .selectable(true)
                .into_any_element()
            }
        } else if let Some(error) = view.error.as_ref() {
            div()
                .py(px(24.0))
                .text_size(ui::font_px(11.0))
                .text_color(ui::color_error())
                .child(error.summary.clone())
                .into_any_element()
        } else {
            div()
                .py(px(24.0))
                .text_size(ui::font_px(11.0))
                .text_color(ui::text_muted())
                .child(if view.loading {
                    "Loading work item..."
                } else {
                    "Work item is unavailable"
                })
                .into_any_element()
        };
        let author = summary.author.as_deref().unwrap_or("unknown");
        let content =
            div()
                .id("github-detail-content")
                .w_full()
                .max_w(px(880.0))
                .mx_auto()
                .px(px(28.0))
                .py(px(24.0))
                .flex()
                .flex_col()
                .gap(px(12.0))
                .child(
                    div()
                        .text_size(ui::font_px(20.0))
                        .text_color(ui::text_primary())
                        .child(summary.title.clone()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .text_size(ui::font_px(10.0))
                        .text_color(ui::text_muted())
                        .child(format!(
                            "{} #{}",
                            summary.kind.short_label(),
                            summary.number
                        ))
                        .child(summary.state.label())
                        .child(format!("@{author}"))
                        .when(summary.is_draft, |el| el.child("Draft")),
                )
                .when(!summary.labels.is_empty(), |el| {
                    el.child(div().flex().flex_wrap().gap(px(5.0)).children(
                        summary.labels.iter().map(|label| {
                            div()
                                .px(px(6.0))
                                .py(px(2.0))
                                .rounded(px(3.0))
                                .bg(ui::border_subtle())
                                .text_size(ui::font_px(9.0))
                                .text_color(ui::text_secondary())
                                .child(label.clone())
                        }),
                    ))
                })
                .child(
                    div()
                        .pt(px(8.0))
                        .border_t_1()
                        .border_color(ui::border_subtle())
                        .text_size(ui::font_px(12.0))
                        .text_color(ui::text_secondary())
                        .child(body),
                );
        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(ui::bg_document())
            .child(content.track_scroll(&self.scroll).overflow_y_scroll())
            .child(Scrollbar::vertical(&self.scroll).id("github-detail-scrollbar"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_host::{
        ExecutionBackend, PlannedHostCommand, ProjectExecutionSnapshot, plan_host_command,
    };
    use mt_config::SshConnection;
    use mt_identity::{ExecutionHostId, HostInstallId, RepoId};

    fn snapshot_with_backend(
        project: &str,
        worktree_path: &str,
        backend: ExecutionBackend,
    ) -> ProjectExecutionSnapshot {
        let install: HostInstallId = "install-v1:00000000-0000-4000-8000-000000000001"
            .parse()
            .unwrap();
        let host = ExecutionHostId::derive("test", &install);
        let repo = RepoId::derive(&host, "/repo/.git");
        let worktree = WorktreeId::derive(&repo, worktree_path, None);
        ProjectExecutionSnapshot {
            project_id: project.into(),
            root_project_id: "root".into(),
            worktree_id: worktree,
            execution_host_id: host,
            canonical_path: worktree_path.into(),
            root_source_path: "/repo/.git".into(),
            backend,
            host_label: "test execution host".into(),
        }
    }

    fn snapshot(project: &str, worktree_path: &str) -> ProjectExecutionSnapshot {
        snapshot_with_backend(project, worktree_path, ExecutionBackend::Local)
    }

    fn ssh_backend(epoch: Option<u64>) -> ExecutionBackend {
        ExecutionBackend::Ssh {
            connection: SshConnection {
                id: "ssh".into(),
                name: "host".into(),
                host: "example.com".into(),
                port: 22,
                user: "u".into(),
                password: None,
                identity_file: None,
                group: None,
            },
            connection_fingerprint: 7,
            connection_epoch: epoch,
        }
    }

    fn fixture_result(
        plan: &CommandPlan,
        remote: &str,
        account: &str,
        observed_connection_epoch: Option<u64>,
    ) -> HostCommandResult {
        let stdout = if plan.program == "git" {
            remote.to_string()
        } else if plan.args.first().is_some_and(|arg| arg == "api") {
            format!(r#"{{"login":"{account}"}}"#)
        } else if plan.args.get(1).is_some_and(|arg| arg == "list") {
            r#"[{"number":7,"title":"Fixture","state":"OPEN","author":{"login":"octocat"},"labels":[],"updatedAt":"2026-09-03T01:02:03Z","url":"https://github.com/owner/repo/issues/7"}]"#.into()
        } else if plan.args.get(1).is_some_and(|arg| arg == "view") {
            r#"{"number":7,"title":"Fixture","state":"OPEN","author":{"login":"octocat"},"labels":[],"updatedAt":"2026-09-03T01:02:03Z","url":"https://github.com/owner/repo/issues/7","body":"Body"}"#.into()
        } else {
            String::new()
        };
        HostCommandResult {
            output: CommandOutput {
                stdout: stdout.into_bytes(),
                exit_code: Some(0),
                ..CommandOutput::default()
            },
            observed_connection_epoch,
        }
    }

    #[test]
    fn discovery_source_identity_is_exact_to_worktree_and_path() {
        let main = snapshot("main", "/repo");
        let linked = snapshot("linked", "/repo-feature");
        let main_source = main.source_signature();
        let linked_source = linked.source_signature();
        assert_ne!(main_source, linked_source);
        assert_eq!(main_source.worktree_id, main.worktree_id);
        assert_eq!(main_source.canonical_path, "/repo");
        assert_eq!(linked_source.worktree_id, linked.worktree_id);
        assert_eq!(linked_source.canonical_path, "/repo-feature");

        let mut same_id_different_path = main.clone();
        same_id_different_path.canonical_path = "/repo-alias".into();
        assert_ne!(main_source, same_id_different_path.source_signature());

        let mut same_path_different_id = main.clone();
        same_path_different_id.worktree_id = linked.worktree_id;
        assert_ne!(main_source, same_path_different_id.source_signature());
    }

    #[test]
    fn normalized_repository_and_account_share_downstream_cache() {
        let main = snapshot("main", "/repo");
        let linked = snapshot("linked", "/repo-feature");
        let repository = GitHubRepoIdentity::new("github.com", "Owner", "Repo").unwrap();
        let mut auth_scopes = HashMap::new();
        auth_scopes.insert(RepositoryCacheSource::from(&main.source_signature()), 7);

        let main_key =
            RepositoryCacheKey::new(&main.source_signature(), repository.clone(), "OctoCat", 7);
        let linked_key =
            RepositoryCacheKey::new(&linked.source_signature(), repository, "octocat", 7);
        assert_ne!(main.source_signature(), linked.source_signature());
        assert_eq!(
            auth_scopes.get(&RepositoryCacheSource::from(&linked.source_signature())),
            Some(&7)
        );
        assert_eq!(main_key, linked_key);
    }

    #[test]
    fn sibling_worktrees_keep_distinct_remote_discovery_results() {
        let main = snapshot("main", "/repo");
        let linked = snapshot("linked", "/repo-feature");
        let mut main_execute = |_: &ProjectExecutionSnapshot,
                                plan: &CommandPlan,
                                _: Duration,
                                _: usize|
         -> Result<HostCommandResult, CommandExecutionError> {
            Ok(fixture_result(
                plan,
                "git@github.com:owner/main.git\n",
                "octocat",
                None,
            ))
        };
        let mut linked_execute = |_: &ProjectExecutionSnapshot,
                                  plan: &CommandPlan,
                                  _: Duration,
                                  _: usize|
         -> Result<HostCommandResult, CommandExecutionError> {
            Ok(fixture_result(
                plan,
                "git@github.com:owner/feature.git\n",
                "octocat",
                None,
            ))
        };

        let main_result =
            run_list_pipeline_with(main, WorkItemKind::Issue, &mut main_execute).unwrap();
        let linked_result =
            run_list_pipeline_with(linked, WorkItemKind::Issue, &mut linked_execute).unwrap();
        assert_ne!(main_result.observed_source, linked_result.observed_source);
        assert_ne!(main_result.repository, linked_result.repository);
        assert_eq!(
            RepositoryCacheSource::from(&main_result.observed_source),
            RepositoryCacheSource::from(&linked_result.observed_source)
        );

        let main_key = RepositoryCacheKey::new(
            &main_result.observed_source,
            main_result.repository,
            main_result.account,
            7,
        );
        let linked_key = RepositoryCacheKey::new(
            &linked_result.observed_source,
            linked_result.repository,
            linked_result.account,
            7,
        );
        assert_ne!(main_key, linked_key);
    }

    #[test]
    fn force_rotation_invalidates_shared_downstream_cache_and_source_readiness() {
        let main = snapshot("main", "/repo");
        let linked = snapshot("linked", "/repo-feature");
        let unrelated = snapshot_with_backend(
            "other",
            "/other",
            ExecutionBackend::Wsl {
                distro: "Ubuntu".into(),
            },
        );
        let repository = GitHubRepoIdentity::new("github.com", "owner", "repo").unwrap();
        let old_key =
            RepositoryCacheKey::new(&main.source_signature(), repository.clone(), "octocat", 7);
        let unrelated_key =
            RepositoryCacheKey::new(&unrelated.source_signature(), repository, "octocat", 9);
        let mut main_source = SourceRecord::new(&main, 7);
        main_source.phase = SourcePhase::Ready(Box::new(old_key.clone()));
        main_source.last_known = Some(old_key.clone());
        let mut linked_source = SourceRecord::new(&linked, 7);
        linked_source.phase = SourcePhase::Ready(Box::new(old_key.clone()));
        linked_source.last_known = Some(old_key.clone());
        let mut sources = HashMap::from([
            (main.source_signature(), main_source),
            (linked.source_signature(), linked_source),
            (
                unrelated.source_signature(),
                SourceRecord::new(&unrelated, 9),
            ),
        ]);
        let scope = RepositoryCacheSource::from(&main.source_signature());
        let unrelated_scope = RepositoryCacheSource::from(&unrelated.source_signature());
        let mut auth_scopes = HashMap::from([(scope.clone(), 7), (unrelated_scope, 9)]);
        let mut repositories = HashMap::new();
        repositories.insert(old_key.clone(), RepositoryCache::default());
        repositories.insert(unrelated_key.clone(), RepositoryCache::default());

        apply_auth_generation_rotation(
            scope.clone(),
            8,
            &mut auth_scopes,
            &mut sources,
            &mut repositories,
        );

        assert_eq!(auth_scopes.get(&scope), Some(&8));
        assert!(!repositories.contains_key(&old_key));
        assert!(repositories.contains_key(&unrelated_key));
        for snapshot in [&main, &linked] {
            let source = &sources[&snapshot.source_signature()];
            assert_eq!(source.auth_generation, 8);
            assert!(matches!(source.phase, SourcePhase::Idle));
            assert!(source.last_known.is_none());
            assert!(source.repository.is_none());
            assert!(source.account.is_none());
        }
        assert_eq!(sources[&unrelated.source_signature()].auth_generation, 9);
    }

    #[test]
    fn shared_repository_cache_still_requires_the_source_auth_generation() {
        let snapshot = snapshot("main", "/repo");
        let repository = GitHubRepoIdentity::new("github.com", "owner", "repo").unwrap();
        let cache_key =
            RepositoryCacheKey::new(&snapshot.source_signature(), repository, "octocat", 7);
        let mut source = SourceRecord::new(&snapshot, 7);
        source.phase = SourcePhase::Ready(Box::new(cache_key.clone()));

        assert!(source_is_ready(Some(&source), &cache_key, 7));
        assert!(!source_is_ready(Some(&source), &cache_key, 8));
    }

    #[test]
    fn rollback_gate_disables_only_exact_zero() {
        assert!(!github_project_tasks_enabled_for(Some(OsStr::new("0"))));
        assert!(github_project_tasks_enabled_for(None));
        assert!(github_project_tasks_enabled_for(Some(OsStr::new("false"))));
        assert!(github_project_tasks_enabled_for(Some(OsStr::new("1"))));
    }

    #[test]
    fn work_item_tab_identity_isolated_by_worktree() {
        let first = snapshot("a", "/repo");
        let second = snapshot("b", "/repo-feature");
        let repository = GitHubRepoIdentity::new("github.com", "owner", "repo").unwrap();
        let key = |worktree_id| GitHubWorkItemTabKey {
            worktree_id,
            repository: repository.clone(),
            kind: WorkItemKind::Issue,
            number: 7,
        };
        assert_ne!(key(first.worktree_id), key(second.worktree_id));
    }

    #[test]
    fn list_and_detail_pipelines_route_every_stage_through_the_selected_execution_host() {
        let cases = [
            (
                "local",
                snapshot_with_backend("local", "/repo", ExecutionBackend::Local),
            ),
            (
                "wsl",
                snapshot_with_backend(
                    "wsl",
                    "/home/u/repo",
                    ExecutionBackend::Wsl {
                        distro: "Ubuntu".into(),
                    },
                ),
            ),
            (
                "ssh",
                snapshot_with_backend("ssh", "/srv/repo", ssh_backend(Some(9))),
            ),
        ];

        for (expected_backend, snapshot) in cases {
            let detail_snapshot = snapshot.clone();
            let mut routed = Vec::new();
            let mut execute = |snapshot: &ProjectExecutionSnapshot,
                               plan: &CommandPlan,
                               _timeout: Duration,
                               _output_cap: usize|
             -> Result<HostCommandResult, CommandExecutionError> {
                let planned = plan_host_command(snapshot, plan)?;
                match (expected_backend, &planned) {
                    ("local", PlannedHostCommand::Process { program, .. }) => {
                        assert_ne!(program, "wsl.exe");
                    }
                    ("wsl", PlannedHostCommand::Process { program, .. }) => {
                        assert_eq!(program, "wsl.exe");
                    }
                    ("ssh", PlannedHostCommand::Ssh { remote_command }) => {
                        assert!(remote_command.starts_with("cd '/srv/repo' && exec "));
                    }
                    _ => panic!("stage escaped the selected {expected_backend} backend"),
                }
                routed.push(planned);
                let epoch = (expected_backend == "ssh").then_some(9);
                Ok(fixture_result(
                    plan,
                    "git@github.com:Owner/Repo.git\n",
                    "octocat",
                    epoch,
                ))
            };

            let result = run_list_pipeline_with(snapshot, WorkItemKind::Issue, &mut execute)
                .expect("fixture pipeline");
            assert_eq!(result.repository.cli_spec(), "github.com/owner/repo");
            assert_eq!(result.account, "octocat");
            assert_eq!(result.rows.len(), 1);
            assert_eq!(routed.len(), 8);

            let repository = GitHubRepoIdentity::new("github.com", "owner", "repo").unwrap();
            let mut detail_routed = Vec::new();
            let mut detail_execute =
                |snapshot: &ProjectExecutionSnapshot,
                 plan: &CommandPlan,
                 _timeout: Duration,
                 _output_cap: usize|
                 -> Result<HostCommandResult, CommandExecutionError> {
                    let planned = plan_host_command(snapshot, plan)?;
                    match (expected_backend, &planned) {
                        ("local", PlannedHostCommand::Process { program, .. }) => {
                            assert_ne!(program, "wsl.exe");
                        }
                        ("wsl", PlannedHostCommand::Process { program, .. }) => {
                            assert_eq!(program, "wsl.exe");
                        }
                        ("ssh", PlannedHostCommand::Ssh { remote_command }) => {
                            assert!(remote_command.starts_with("cd '/srv/repo' && exec "));
                        }
                        _ => panic!("detail stage escaped the selected {expected_backend} backend"),
                    }
                    detail_routed.push(planned);
                    let epoch = (expected_backend == "ssh").then_some(9);
                    Ok(fixture_result(
                        plan,
                        "git@github.com:Owner/Repo.git\n",
                        "octocat",
                        epoch,
                    ))
                };
            let detail = run_detail_only_with(
                detail_snapshot,
                repository,
                "octocat".into(),
                WorkItemKind::Issue,
                7,
                &mut detail_execute,
            )
            .expect("fixture detail pipeline");
            assert_eq!(detail.0.summary.number, 7);
            assert_eq!(detail_routed.len(), 7);
        }
    }

    #[test]
    fn source_signature_changes_with_root_or_execution_backend_identity() {
        let baseline = snapshot("main", "/repo");

        let mut different_root = baseline.clone();
        different_root.root_project_id = "another-root".into();
        assert_ne!(
            baseline.source_signature(),
            different_root.source_signature()
        );

        let wsl_a = snapshot_with_backend(
            "wsl",
            "/repo",
            ExecutionBackend::Wsl {
                distro: "Ubuntu".into(),
            },
        );
        let wsl_b = snapshot_with_backend(
            "wsl",
            "/repo",
            ExecutionBackend::Wsl {
                distro: "Debian".into(),
            },
        );
        assert_ne!(wsl_a.source_signature(), wsl_b.source_signature());

        let ssh_a = snapshot_with_backend("ssh", "/repo", ssh_backend(Some(9)));
        let mut ssh_b = snapshot_with_backend("ssh", "/repo", ssh_backend(Some(10)));
        if let ExecutionBackend::Ssh {
            connection_fingerprint,
            ..
        } = &mut ssh_b.backend
        {
            *connection_fingerprint = 8;
        }
        assert_ne!(ssh_a.source_signature(), ssh_b.source_signature());
    }

    #[test]
    fn cached_list_rejects_remote_or_account_changes() {
        let repository = GitHubRepoIdentity::new("github.com", "owner", "repo").unwrap();
        let mut remote_probes = 0;
        let mut remote_change = |_snapshot: &ProjectExecutionSnapshot,
                                 plan: &CommandPlan,
                                 _timeout: Duration,
                                 _output_cap: usize|
         -> Result<HostCommandResult, CommandExecutionError> {
            let remote = if plan.program == "git" {
                remote_probes += 1;
                if remote_probes == 1 {
                    "git@github.com:owner/repo.git\n"
                } else {
                    "git@github.com:owner/other.git\n"
                }
            } else {
                "git@github.com:owner/repo.git\n"
            };
            Ok(fixture_result(plan, remote, "octocat", None))
        };
        let remote_error = run_list_only_with(
            snapshot("local", "/repo"),
            repository.clone(),
            "octocat".into(),
            WorkItemKind::Issue,
            &mut remote_change,
        )
        .unwrap_err();
        assert_eq!(remote_error.error.kind, GitHubErrorKind::RepositoryChanged);

        let mut account_probes = 0;
        let mut account_change = |_snapshot: &ProjectExecutionSnapshot,
                                  plan: &CommandPlan,
                                  _timeout: Duration,
                                  _output_cap: usize|
         -> Result<HostCommandResult, CommandExecutionError> {
            let account = if plan.args.first().is_some_and(|arg| arg == "api") {
                account_probes += 1;
                if account_probes == 1 {
                    "octocat"
                } else {
                    "hubot"
                }
            } else {
                "octocat"
            };
            Ok(fixture_result(
                plan,
                "git@github.com:owner/repo.git\n",
                account,
                None,
            ))
        };
        let account_error = run_list_only_with(
            snapshot("local", "/repo"),
            repository,
            "octocat".into(),
            WorkItemKind::Issue,
            &mut account_change,
        )
        .unwrap_err();
        assert_eq!(
            account_error.error.kind,
            GitHubErrorKind::WrongHostOrAccount
        );
    }

    #[test]
    fn detail_rejects_account_changes_and_pipeline_rejects_epoch_changes() {
        let repository = GitHubRepoIdentity::new("github.com", "owner", "repo").unwrap();
        let mut account_probes = 0;
        let mut account_change = |_snapshot: &ProjectExecutionSnapshot,
                                  plan: &CommandPlan,
                                  _timeout: Duration,
                                  _output_cap: usize|
         -> Result<HostCommandResult, CommandExecutionError> {
            let account = if plan.args.first().is_some_and(|arg| arg == "api") {
                account_probes += 1;
                if account_probes == 1 {
                    "octocat"
                } else {
                    "hubot"
                }
            } else {
                "octocat"
            };
            Ok(fixture_result(
                plan,
                "git@github.com:owner/repo.git\n",
                account,
                None,
            ))
        };
        let detail_error = run_detail_only_with(
            snapshot("local", "/repo"),
            repository,
            "octocat".into(),
            WorkItemKind::Issue,
            7,
            &mut account_change,
        )
        .unwrap_err();
        assert_eq!(detail_error.error.kind, GitHubErrorKind::WrongHostOrAccount);

        let mut calls = 0_u64;
        let mut epoch_change = |_snapshot: &ProjectExecutionSnapshot,
                                plan: &CommandPlan,
                                _timeout: Duration,
                                _output_cap: usize|
         -> Result<HostCommandResult, CommandExecutionError> {
            calls += 1;
            Ok(fixture_result(
                plan,
                "git@github.com:owner/repo.git\n",
                "octocat",
                Some(8 + calls),
            ))
        };
        let epoch_error = run_list_pipeline_with(
            snapshot_with_backend("ssh", "/srv/repo", ssh_backend(Some(8))),
            WorkItemKind::Issue,
            &mut epoch_change,
        )
        .unwrap_err();
        assert_eq!(epoch_error.error.kind, GitHubErrorKind::RepositoryChanged);
    }

    #[test]
    fn auth_failure_never_plans_login_browser_or_terminal_side_effects() {
        let mut plans = Vec::new();
        let mut execute = |_snapshot: &ProjectExecutionSnapshot,
                           plan: &CommandPlan,
                           _timeout: Duration,
                           _output_cap: usize|
         -> Result<HostCommandResult, CommandExecutionError> {
            plans.push(plan.clone());
            if plan.args.first().is_some_and(|arg| arg == "auth") {
                return Ok(HostCommandResult {
                    output: CommandOutput {
                        stderr: b"not logged into github.com".to_vec(),
                        exit_code: Some(1),
                        ..CommandOutput::default()
                    },
                    observed_connection_epoch: None,
                });
            }
            Ok(fixture_result(
                plan,
                "git@github.com:owner/repo.git\n",
                "octocat",
                None,
            ))
        };
        let error = run_list_pipeline_with(
            snapshot("local", "/repo"),
            WorkItemKind::Issue,
            &mut execute,
        )
        .unwrap_err();
        assert_eq!(error.error.kind, GitHubErrorKind::AuthRequired);
        assert_eq!(plans.len(), 3);
        assert!(plans.iter().all(|plan| {
            plan.program != "cmd"
                && plan.program != "powershell"
                && !plan.args.iter().any(|arg| arg == "login" || arg == "--web")
        }));
    }

    #[test]
    fn only_repository_identity_errors_invalidate_the_ready_source() {
        assert!(invalidates_repository_source(
            &GitHubError::repository_changed()
        ));
        assert!(invalidates_repository_source(&GitHubError::new(
            GitHubErrorKind::AuthRequired,
            "auth",
            true,
        )));
        assert!(!invalidates_repository_source(&GitHubError::new(
            GitHubErrorKind::Offline,
            "offline",
            true,
        )));
        assert!(!invalidates_repository_source(&GitHubError::new(
            GitHubErrorKind::NotFound,
            "item missing",
            false,
        )));
        assert!(invalidates_list_source(&GitHubError::new(
            GitHubErrorKind::NotFound,
            "repository missing",
            false,
        )));
        assert!(!invalidates_repository_source(&GitHubError::malformed(
            "list"
        )));
    }
}
