//! Pure orchestration over a narrow injectable executor. Only origin uses the
//! ordinary host command path; every gh operation uses account isolation.

use std::time::Duration;

use mt_github::{
    AccountCapability, COMMAND_OUTPUT_LIMIT, CommandExecutionError, CommandOutput, CommandStage,
    GitHubAccountIdentity, GitHubError, GitHubErrorKind, GitHubRepoIdentity, GitHubWorkItemDetail,
    GitHubWorkItemSummary, KnownGitHubAccounts, SelectedAccountRequestPlan, WorkItemKind,
    classify_execution_error, discover_remote_plan, parse_remote_url, parse_work_item_detail,
    parse_work_item_list, require_success,
};

use super::model::{TasksError, resolve_selection};
use crate::execution_host::{
    ExecutionBackendSignature, ExecutionSourceSignature, HostCommandResult,
    ProjectExecutionSnapshot, execute_host_command,
};
use crate::tasks_account_executor::{
    AccountCancellation, AccountExecutionControl, AccountExecutionError, AccountHostResult,
    discover_accounts, execute_selected_account, probe_account_capability,
};

const ORIGIN_TIMEOUT: Duration = Duration::from_secs(8);
const ACCOUNT_TIMEOUT: Duration = Duration::from_secs(12);
const READ_TIMEOUT: Duration = Duration::from_secs(20);

pub(super) trait TasksExecutor {
    fn origin(
        &mut self,
        snapshot: &ProjectExecutionSnapshot,
    ) -> Result<HostCommandResult, CommandExecutionError>;
    fn capability(
        &mut self,
        snapshot: &ProjectExecutionSnapshot,
        capability: AccountCapability,
        control: &AccountExecutionControl,
    ) -> AccountHostResult<()>;
    fn accounts(
        &mut self,
        snapshot: &ProjectExecutionSnapshot,
        host: &str,
        control: &AccountExecutionControl,
    ) -> AccountHostResult<KnownGitHubAccounts>;
    fn selected(
        &mut self,
        snapshot: &ProjectExecutionSnapshot,
        plan: &SelectedAccountRequestPlan,
        control: &AccountExecutionControl,
    ) -> AccountHostResult<CommandOutput>;
}

pub(super) struct HostExecutor;

impl TasksExecutor for HostExecutor {
    fn origin(
        &mut self,
        snapshot: &ProjectExecutionSnapshot,
    ) -> Result<HostCommandResult, CommandExecutionError> {
        execute_host_command(
            snapshot,
            &discover_remote_plan(),
            ORIGIN_TIMEOUT,
            COMMAND_OUTPUT_LIMIT,
        )
    }

    fn capability(
        &mut self,
        snapshot: &ProjectExecutionSnapshot,
        capability: AccountCapability,
        control: &AccountExecutionControl,
    ) -> AccountHostResult<()> {
        probe_account_capability(snapshot, capability, control)
    }

    fn accounts(
        &mut self,
        snapshot: &ProjectExecutionSnapshot,
        host: &str,
        control: &AccountExecutionControl,
    ) -> AccountHostResult<KnownGitHubAccounts> {
        discover_accounts(snapshot, host, control)
    }

    fn selected(
        &mut self,
        snapshot: &ProjectExecutionSnapshot,
        plan: &SelectedAccountRequestPlan,
        control: &AccountExecutionControl,
    ) -> AccountHostResult<CommandOutput> {
        execute_selected_account(snapshot, plan, control)
    }
}

pub(super) struct PipelineCompletion<T> {
    pub result: Result<T, TasksError>,
    pub repository: Option<GitHubRepoIdentity>,
    pub observed_source: ExecutionSourceSignature,
}

#[derive(Debug)]
pub(super) struct PreparedAccounts {
    pub repository: GitHubRepoIdentity,
    pub accounts: KnownGitHubAccounts,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum ReadTarget {
    List(WorkItemKind),
    Detail(WorkItemKind, u64),
}

#[derive(Debug)]
pub(super) enum ReadData {
    List(Vec<GitHubWorkItemSummary>),
    Detail(GitHubWorkItemDetail),
}

#[derive(Debug)]
pub(super) struct ReadSuccess {
    pub data: ReadData,
    pub accounts: KnownGitHubAccounts,
}

struct Pipeline<'a, E> {
    snapshot: &'a ProjectExecutionSnapshot,
    executor: &'a mut E,
    cancellation: &'a AccountCancellation,
    epoch: Option<u64>,
}

impl<E: TasksExecutor> Pipeline<'_, E> {
    fn check(&self) -> Result<(), TasksError> {
        if self.cancellation.is_cancelled() {
            Err(AccountExecutionError::Cancelled.into())
        } else {
            Ok(())
        }
    }

    fn observe(&mut self, epoch: Option<u64>) -> Result<(), TasksError> {
        self.check()?;
        if let Some(epoch) = epoch {
            let changed = self.epoch.is_some_and(|current| current != epoch);
            self.epoch = Some(epoch);
            if changed {
                return Err(TasksError::changed());
            }
        } else if matches!(
            self.snapshot.backend.signature(),
            ExecutionBackendSignature::Ssh { .. }
        ) {
            return Err(TasksError::changed());
        }
        Ok(())
    }

    fn control(&self, timeout: Duration) -> Result<AccountExecutionControl, TasksError> {
        self.check()?;
        Ok(AccountExecutionControl::new(
            timeout,
            self.cancellation.clone(),
            self.epoch,
        )?)
    }

    fn account_result<T>(&mut self, result: AccountHostResult<T>) -> Result<T, TasksError> {
        // Observe failures too. A stale offline/auth response cannot replace new readiness.
        if result.observed_connection_epoch.is_some() {
            self.observe(result.observed_connection_epoch)?;
        }
        self.check()?;
        if result.result.is_ok()
            && matches!(
                self.snapshot.backend.signature(),
                ExecutionBackendSignature::Ssh { .. }
            )
            && result.observed_connection_epoch.is_none()
        {
            return Err(TasksError::changed());
        }
        result.result.map_err(Into::into)
    }

    fn origin(
        &mut self,
        expected: Option<&GitHubRepoIdentity>,
    ) -> Result<GitHubRepoIdentity, TasksError> {
        self.check()?;
        let output = self
            .executor
            .origin(self.snapshot)
            .map_err(|error| classify_execution_error(CommandStage::DiscoverRemote, &error))?;
        self.observe(output.observed_connection_epoch)?;
        require_success(CommandStage::DiscoverRemote, &output.output)?;
        let text = std::str::from_utf8(&output.output.stdout)
            .map_err(|_| GitHubError::malformed("Git remote discovery"))?;
        let repository = parse_remote_url(text).map_err(|_| {
            GitHubError::new(
                GitHubErrorKind::NoGitHubRemote,
                "The origin remote is not a supported GitHub repository",
                true,
            )
        })?;
        if expected.is_some_and(|expected| *expected != repository) {
            return Err(GitHubError::repository_changed().into());
        }
        Ok(repository)
    }

    fn accounts(&mut self, host: &str) -> Result<KnownGitHubAccounts, TasksError> {
        let control = self.control(ACCOUNT_TIMEOUT)?;
        let result = self.executor.accounts(self.snapshot, host, &control);
        self.account_result(result)
    }

    fn selected(&mut self, plan: &SelectedAccountRequestPlan) -> Result<CommandOutput, TasksError> {
        let control = self.control(READ_TIMEOUT)?;
        let result = self.executor.selected(self.snapshot, plan, &control);
        self.account_result(result)
    }
}

pub(super) fn prepare_with<E: TasksExecutor>(
    snapshot: &ProjectExecutionSnapshot,
    cancellation: &AccountCancellation,
    executor: &mut E,
) -> PipelineCompletion<PreparedAccounts> {
    let mut pipeline = Pipeline {
        snapshot,
        cancellation,
        executor,
        epoch: None,
    };
    let mut repository = None;
    let result = (|| {
        let repo = pipeline.origin(None)?;
        repository = Some(repo.clone());
        for capability in [
            AccountCapability::AuthStatusJson,
            AccountCapability::NamedAccountLookup,
        ] {
            let control = pipeline.control(ACCOUNT_TIMEOUT)?;
            let result = pipeline.executor.capability(snapshot, capability, &control);
            pipeline.account_result(result)?;
        }
        let accounts = pipeline.accounts(repo.host())?;
        Ok(PreparedAccounts {
            repository: repo,
            accounts,
        })
    })();
    let result = finish_origin(&mut pipeline, repository.as_ref(), result);
    PipelineCompletion {
        result,
        repository,
        observed_source: snapshot.observed_source_signature(pipeline.epoch),
    }
}

// Even an account failure is attached only to the origin that authorized it.
fn finish_origin<E: TasksExecutor, T>(
    pipeline: &mut Pipeline<'_, E>,
    repository: Option<&GitHubRepoIdentity>,
    result: Result<T, TasksError>,
) -> Result<T, TasksError> {
    pipeline.check()?;
    if matches!(
        &result,
        Err(TasksError::Account(AccountExecutionError::ContextChanged))
    ) {
        return result;
    }
    if let Some(repository) = repository {
        pipeline.origin(Some(repository))?;
    }
    result
}

pub(super) fn read_with<E: TasksExecutor>(
    snapshot: &ProjectExecutionSnapshot,
    repository: &GitHubRepoIdentity,
    identity: &GitHubAccountIdentity,
    target: ReadTarget,
    cancellation: &AccountCancellation,
    executor: &mut E,
) -> PipelineCompletion<ReadSuccess> {
    let epoch = match snapshot.backend.signature() {
        ExecutionBackendSignature::Ssh {
            connection_epoch, ..
        } => connection_epoch,
        _ => None,
    };
    let mut pipeline = Pipeline {
        snapshot,
        executor,
        cancellation,
        epoch,
    };
    let result = (|| {
        pipeline.origin(Some(repository))?;
        let accounts = pipeline.accounts(repository.host())?;
        let selected = resolve_selection(&accounts, Some(identity))?;
        let plan = match target {
            ReadTarget::List(kind) => {
                SelectedAccountRequestPlan::list(&selected, repository, kind)?
            }
            ReadTarget::Detail(kind, number) => {
                SelectedAccountRequestPlan::detail(&selected, repository, kind, number)?
            }
        };
        // Executor proves pre/post identity with one captured credential for this read.
        let output = pipeline.selected(&plan)?;
        pipeline.origin(Some(repository))?;
        let accounts = pipeline.accounts(repository.host())?;
        let selected = resolve_selection(&accounts, Some(identity))?;
        pipeline.selected(&SelectedAccountRequestPlan::identity(&selected))?;
        let data = match target {
            ReadTarget::List(kind) => ReadData::List(parse_work_item_list(kind, &output)?),
            ReadTarget::Detail(kind, _) => ReadData::Detail(parse_work_item_detail(kind, &output)?),
        };
        Ok(ReadSuccess { data, accounts })
    })();
    let result = finish_origin(&mut pipeline, Some(repository), result);
    PipelineCompletion {
        result,
        repository: Some(repository.clone()),
        observed_source: snapshot.observed_source_signature(pipeline.epoch),
    }
}
