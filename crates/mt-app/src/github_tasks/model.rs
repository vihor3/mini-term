use std::collections::HashMap;

use mt_config::{AppConfig, TasksAccountScope, TasksAccountSelection, WorktreeVisibilityBackend};
use mt_github::{
    AccountError, GitHubAccountIdentity, GitHubError, GitHubErrorKind, GitHubRepoIdentity,
    GitHubWorkItemDetail, GitHubWorkItemSummary, KnownGitHubAccount, KnownGitHubAccounts,
    WorkItemKind,
};
use mt_identity::{ExecutionHostId, WorktreeId};

use crate::execution_host::{
    ExecutionBackend, ExecutionBackendSignature, ExecutionSourceSignature, ProjectExecutionSnapshot,
};
use crate::tasks_account_executor::{AccountCancellation, AccountExecutionError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TasksError {
    Repository(GitHubError),
    Account(AccountExecutionError),
    ChooseAccount,
}

impl From<GitHubError> for TasksError {
    fn from(error: GitHubError) -> Self {
        Self::Repository(error)
    }
}

impl From<AccountError> for TasksError {
    fn from(error: AccountError) -> Self {
        Self::Account(error.into())
    }
}

impl From<AccountExecutionError> for TasksError {
    fn from(error: AccountExecutionError) -> Self {
        Self::Account(error)
    }
}

impl TasksError {
    pub fn changed() -> Self {
        AccountExecutionError::ContextChanged.into()
    }

    pub fn summary(&self) -> String {
        match self {
            Self::Repository(error) => error.summary.clone(),
            Self::Account(error) => error.to_string(),
            Self::ChooseAccount => "No Tasks account selected".into(),
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::ChooseAccount => "Select a GitHub account",
            Self::Repository(error) => match error.kind {
                GitHubErrorKind::NoGitHubRemote => "No GitHub repository",
                GitHubErrorKind::RepositoryChanged => "Repository context changed",
                GitHubErrorKind::MalformedResponse => "GitHub returned invalid data",
                GitHubErrorKind::Offline => "Execution host is offline",
                _ => "GitHub Tasks unavailable",
            },
            Self::Account(AccountExecutionError::Account(error)) => match error {
                AccountError::ClientMissing => "GitHub CLI is unavailable",
                AccountError::UnsupportedAuthStatusJson
                | AccountError::UnsupportedNamedAccountLookup => "GitHub CLI update required",
                AccountError::AuthRequired => "No GitHub accounts available",
                AccountError::SelectedAccountUnavailable => "Selected account is missing",
                AccountError::CredentialStoreUnavailable => "Credential store unavailable",
                AccountError::CredentialLookupFailed => "Account credential unavailable",
                AccountError::AuthenticationFailed => "Selected account authentication failed",
                AccountError::PermissionDenied => "Account access denied",
                AccountError::ScopeRequired => "Additional GitHub scope required",
                AccountError::RateLimited => "GitHub rate limit reached",
                AccountError::Offline => "GitHub is unreachable",
                AccountError::NotFound => "Repository or item not found",
                AccountError::WrongHostOrAccount => "GitHub account mismatch",
                AccountError::InvalidHost | AccountError::InvalidLogin => "Invalid account selection",
                AccountError::DuplicateAccount | AccountError::MalformedResponse => {
                    "Invalid GitHub account data"
                }
                AccountError::InheritedAuthentication => "GitHub authentication override detected",
                AccountError::CommandFailed => "GitHub account request failed",
            },
            Self::Account(AccountExecutionError::HostHelperUnavailable) => {
                "Execution-host capability unavailable"
            }
            Self::Account(AccountExecutionError::Cancelled) => "GitHub request cancelled",
            Self::Account(AccountExecutionError::TimedOut) => "GitHub request timed out",
            Self::Account(AccountExecutionError::InvalidContext
                | AccountExecutionError::ContextChanged) => "Tasks context changed",
            Self::Account(AccountExecutionError::CleanupFailed) => "Process cleanup unconfirmed",
            Self::Account(AccountExecutionError::SecretOutputRejected) => "Unsafe output rejected",
            Self::Account(AccountExecutionError::Protocol) => "Invalid execution-host response",
        }
    }

    pub fn offers_login(&self) -> bool {
        matches!(self, Self::Account(AccountExecutionError::Account(
            AccountError::AuthRequired | AccountError::SelectedAccountUnavailable
                | AccountError::AuthenticationFailed | AccountError::CredentialLookupFailed
        )))
    }

    pub fn retains_last_known(&self) -> bool {
        match self {
            Self::Repository(error) => error.retains_last_known(),
            Self::Account(AccountExecutionError::Account(error)) => error.retains_last_known(),
            Self::Account(AccountExecutionError::TimedOut) => true,
            _ => false,
        }
    }
}

pub(super) fn account_scope(snapshot: &ProjectExecutionSnapshot, host: &str) -> TasksAccountScope {
    let backend = match &snapshot.backend {
        ExecutionBackend::Local => WorktreeVisibilityBackend::Local,
        ExecutionBackend::Wsl { distro } => WorktreeVisibilityBackend::Wsl {
            distro: distro.to_lowercase(),
        },
        ExecutionBackend::Ssh { connection, .. } => WorktreeVisibilityBackend::Ssh {
            connection_id: connection.id.clone(),
            host: connection.host.to_ascii_lowercase(),
            port: if connection.port == 0 { 22 } else { connection.port },
            user: connection.user.clone(),
        },
    };
    TasksAccountScope {
        root_project_id: snapshot.root_project_id.clone(),
        execution_host_id: snapshot.execution_host_id.clone(),
        backend,
        github_host: host.to_string(),
    }
}

pub(super) fn saved_selection(
    config: &AppConfig,
    scope: &TasksAccountScope,
) -> Result<Option<GitHubAccountIdentity>, TasksError> {
    let mut choices = config.tasks_account_selections.iter().filter(|entry| entry.scope == *scope);
    let selection = choices.next().map(|entry| {
        GitHubAccountIdentity::new(&scope.github_host, &entry.login).map_err(TasksError::from)
    }).transpose()?;
    if choices.next().is_some() {
        return Err(AccountError::MalformedResponse.into());
    }
    Ok(selection)
}

pub(super) fn write_selection(
    config: &mut AppConfig,
    scope: TasksAccountScope,
    identity: &GitHubAccountIdentity,
) {
    config.tasks_account_selections.retain(|entry| entry.scope != scope);
    config.tasks_account_selections.push(TasksAccountSelection {
        scope,
        login: identity.login().to_string(),
    });
}

/// A stored-but-missing choice must never become a sole-account auto-choice.
pub(super) fn resolve_selection(
    accounts: &KnownGitHubAccounts,
    saved: Option<&GitHubAccountIdentity>,
) -> Result<KnownGitHubAccount, TasksError> {
    let account = match saved {
        Some(identity) => accounts.find(identity)?,
        None => accounts.initial_selection().ok_or_else(|| {
            if accounts.accounts().is_empty() {
                TasksError::from(AccountError::AuthRequired)
            } else {
                TasksError::ChooseAccount
            }
        })?,
    };
    if let Some(problem) = account.problem() {
        return Err(problem.into());
    }
    Ok(account.clone())
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct RepositoryCacheSource {
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
pub(super) struct RepositoryCacheKey {
    pub source: RepositoryCacheSource,
    pub repository: GitHubRepoIdentity,
    pub account: String,
    pub auth_generation: u64,
}

impl RepositoryCacheKey {
    pub fn new(source: &ExecutionSourceSignature, repository: GitHubRepoIdentity,
        account: String, auth_generation: u64) -> Self {
        Self { source: source.into(), repository, account: account.to_ascii_lowercase(), auth_generation }
    }
}

#[derive(Clone, Default)]
pub(super) struct ListSlot {
    pub rows: Vec<GitHubWorkItemSummary>,
    pub loading: bool,
    pub error: Option<TasksError>,
    pub request_id: u64,
    pub updated_at_unix_ms: Option<i64>,
}

#[derive(Clone, Default)]
pub(super) struct DetailSlot {
    pub detail: Option<GitHubWorkItemDetail>,
    pub loading: bool,
    pub error: Option<TasksError>,
    pub request_id: u64,
}

impl DetailSlot {
    pub fn begin_access(&mut self, request_id: u64) {
        self.request_id = request_id;
        self.loading = true;
        self.error = None;
    }
}

#[derive(Default)]
pub(super) struct RepositoryCache {
    pub lists: HashMap<WorkItemKind, ListSlot>,
    pub details: HashMap<(WorkItemKind, u64), DetailSlot>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SourcePhase { Idle, Loading, Choosing, Ready, Error }

pub(super) struct SourceRecord {
    pub snapshot: ProjectExecutionSnapshot,
    pub phase: SourcePhase,
    pub repository: Option<GitHubRepoIdentity>,
    pub accounts: Option<KnownGitHubAccounts>,
    pub selected: Option<GitHubAccountIdentity>,
    pub scope: Option<TasksAccountScope>,
    pub error: Option<TasksError>,
    pub auth_generation: u64,
    pub request_id: u64,
    pub cache_key: Option<RepositoryCacheKey>,
    pub list_access_id: u64,
    pending_list_access: Option<(u64, WorkItemKind)>,
}

impl SourceRecord {
    pub fn new(snapshot: ProjectExecutionSnapshot, request_id: u64) -> Self {
        Self { snapshot, phase: SourcePhase::Idle, repository: None, accounts: None,
            selected: None, scope: None, error: None, auth_generation: 0, request_id,
            cache_key: None, list_access_id: request_id, pending_list_access: None }
    }

    pub fn invalidate(&mut self, request_id: u64, clear_data: bool) {
        self.phase = SourcePhase::Idle;
        self.request_id = request_id;
        self.error = None;
        self.list_access_id = request_id;
        self.pending_list_access = None;
        if clear_data {
            self.cache_key = None;
            self.accounts = None;
        }
    }

    pub fn begin_list_access(&mut self, access_id: u64, kind: WorkItemKind) {
        if self.cache_key.is_none() { self.request_id = access_id; }
        self.phase = SourcePhase::Loading;
        self.error = None;
        self.list_access_id = access_id;
        self.pending_list_access = Some((access_id, kind));
    }

    /// Passive notifications may consume an explicit access only once.
    pub fn take_list_access(&mut self) -> Option<(u64, WorkItemKind)> {
        self.pending_list_access.take()
    }
}

#[derive(Clone)]
pub(super) struct RequestOwner {
    pub source: ExecutionSourceSignature,
    pub source_request: u64,
    pub request_id: u64,
    pub scope: Option<TasksAccountScope>,
    pub cache_key: Option<RepositoryCacheKey>,
    pub cancellation: AccountCancellation,
}

pub(super) fn owns_request(source: Option<&SourceRecord>, owner: &RequestOwner) -> bool {
    !owner.cancellation.is_cancelled() && source.is_some_and(|source| {
        source.snapshot.source_signature() == owner.source
            && source.request_id == owner.source_request
            && owner.cache_key.as_ref().is_none_or(|key| {
                source.cache_key.as_ref() == Some(key)
                    && source.auth_generation == key.auth_generation
                    && source.selected.as_ref().is_some_and(|id| id.login() == key.account)
            })
    })
}

pub(super) fn owns_slot(request_id: u64, loading: bool, owner: &RequestOwner) -> bool {
    request_id == owner.request_id && loading && !owner.cancellation.is_cancelled()
}

pub(super) fn source_is_ready(source: Option<&SourceRecord>, key: &RepositoryCacheKey) -> bool {
    source.is_some_and(|source| source.phase == SourcePhase::Ready)
        && source_has_identity(source, key)
}

pub(super) fn source_has_identity(source: Option<&SourceRecord>, key: &RepositoryCacheKey) -> bool {
    source.is_some_and(|source| source.cache_key.as_ref() == Some(key)
        && source.auth_generation == key.auth_generation
        && source.selected.as_ref().is_some_and(|id| id.login() == key.account))
}

#[derive(Clone)]
pub(super) struct GitHubListView {
    pub host_label: String,
    pub repository: Option<GitHubRepoIdentity>,
    pub account: Option<String>,
    pub accounts: Option<KnownGitHubAccounts>,
    pub source: ExecutionSourceSignature,
    pub source_request: u64,
    pub list_access_id: u64,
    pub auth_generation: u64,
    pub rows: Vec<GitHubWorkItemSummary>,
    pub loading: bool,
    pub error: Option<TasksError>,
    pub updated_at_unix_ms: Option<i64>,
    pub interactive: bool,
}

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
        GitHubWorkItemTabKey { worktree_id: self.worktree_id.clone(),
            repository: self.repository.clone(), kind: self.summary.kind, number: self.summary.number }
    }

    pub(super) fn repository_cache_key(&self) -> RepositoryCacheKey {
        RepositoryCacheKey::new(&self.source, self.repository.clone(), self.account.clone(), self.auth_generation)
    }
}

pub(super) struct GitHubDetailView {
    pub detail: Option<GitHubWorkItemDetail>,
    pub loading: bool,
    pub error: Option<TasksError>,
}
