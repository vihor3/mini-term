//! Synthetic orchestration fixtures only. Execute through GitHub Actions.

use std::collections::{HashMap, VecDeque};
use std::ffi::OsStr;

use mt_config::{AppConfig, SshConnection};
use mt_github::{
    AccountCapability, AccountCommandStage, AccountError, CommandExecutionError,
    CommandExecutionErrorKind, CommandOutput, GitHubAccountIdentity, GitHubErrorKind,
    GitHubRepoIdentity, KnownGitHubAccounts, SelectedAccountRequestPlan, WorkItemKind,
    parse_known_accounts, require_account_success, verify_account_capability,
    verify_selected_account,
};
use mt_identity::{ExecutionHostId, HostInstallId, RepoId, WorktreeId};

use super::model::*;
use super::pipeline::*;
use super::service::{
    apply_detail_completion, apply_list_completion, apply_source_completion, detail_slot_view,
    invalidates_account, list_request_ids, preparation_scope_matches, rotate_scope_state,
};
use super::{bounded_context, github_project_tasks_enabled_for};
use crate::execution_host::{
    ExecutionBackend, ExecutionSourceSignature, HostCommandResult, ProjectExecutionSnapshot,
};
use crate::tasks_account_executor::{
    AccountCancellation, AccountExecutionControl, AccountExecutionError, AccountHostResult,
};

fn snapshot(project: &str, path: &str, backend: ExecutionBackend) -> ProjectExecutionSnapshot {
    let install: HostInstallId = "install-v1:00000000-0000-4000-8000-000000000001"
        .parse()
        .unwrap();
    let host = ExecutionHostId::derive("tasks-fixture", &install);
    let repo = RepoId::derive(&host, "/repo/.git");
    ProjectExecutionSnapshot {
        project_id: project.into(),
        root_project_id: "root".into(),
        worktree_id: WorktreeId::derive(&repo, path, None),
        execution_host_id: host,
        canonical_path: path.into(),
        root_source_path: "/repo".into(),
        backend,
        host_label: "fixture execution host".into(),
    }
}

fn ssh(epoch: Option<u64>) -> ExecutionBackend {
    ExecutionBackend::Ssh {
        connection: SshConnection {
            id: "ssh".into(),
            name: "fixture".into(),
            host: "example.test".into(),
            port: 22,
            user: "developer".into(),
            password: None,
            identity_file: None,
            group: None,
        },
        connection_fingerprint: 7,
        connection_epoch: epoch,
    }
}

fn repo() -> GitHubRepoIdentity {
    GitHubRepoIdentity::new("github.com", "owner", "repo").unwrap()
}
fn identity(login: &str) -> GitHubAccountIdentity {
    GitHubAccountIdentity::new("github.com", login).unwrap()
}
fn output(text: impl AsRef<str>) -> CommandOutput {
    CommandOutput {
        stdout: text.as_ref().as_bytes().to_vec(),
        exit_code: Some(0),
        ..Default::default()
    }
}

fn accounts_output(rows: &[(&str, bool, Option<&str>)]) -> CommandOutput {
    let rows = rows
        .iter()
        .map(|(login, active, problem)| {
            let mut value = serde_json::json!({"login":login, "active":active,
            "state":if problem.is_some() {"error"} else {"success"}, "host":"github.com"});
            if let Some(problem) = problem {
                value["error"] = serde_json::json!(problem);
            }
            value
        })
        .collect::<Vec<_>>();
    output(serde_json::json!({"hosts":{"github.com":rows}}).to_string())
}

fn accounts(rows: &[(&str, bool, Option<&str>)]) -> KnownGitHubAccounts {
    parse_known_accounts("github.com", &accounts_output(rows)).unwrap()
}

fn item_output(kind: WorkItemKind, list: bool) -> CommandOutput {
    let value = serde_json::json!({"number":7, "title":"Fixture", "state":"OPEN",
        "author":{"login":"Alice"}, "labels":[], "updatedAt":"2026-09-06T01:02:03Z",
        "url":format!("https://github.com/owner/repo/{}/7", if kind == WorkItemKind::Issue {"issues"} else {"pull"}),
        "body":"Inert fixture body", "isDraft":false});
    output(if list {
        serde_json::json!([value]).to_string()
    } else {
        value.to_string()
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Origin,
    Capability(AccountCapability),
    Accounts,
    Selected(AccountCommandStage),
}

struct Step {
    stage: Stage,
    result: Result<CommandOutput, AccountExecutionError>,
    epoch: Option<u64>,
    cancel: bool,
}

impl Step {
    fn ok(stage: Stage, output: CommandOutput, epoch: Option<u64>) -> Self {
        Self {
            stage,
            result: Ok(output),
            epoch,
            cancel: false,
        }
    }

    fn origin(epoch: Option<u64>) -> Self {
        Self::ok(
            Stage::Origin,
            output("git@github.com:owner/repo.git\n"),
            epoch,
        )
    }

    fn accounts(rows: &[(&str, bool, Option<&str>)], epoch: Option<u64>) -> Self {
        Self::ok(Stage::Accounts, accounts_output(rows), epoch)
    }

    fn capability(capability: AccountCapability, epoch: Option<u64>) -> Self {
        let flag = if capability == AccountCapability::AuthStatusJson {
            "--json"
        } else {
            "--user"
        };
        Self::ok(
            Stage::Capability(capability),
            output(format!("FLAGS\n  --hostname string\n  {flag} string\n")),
            epoch,
        )
    }
}

struct ScriptedHost {
    source: ExecutionSourceSignature,
    steps: VecDeque<Step>,
    cancellation: AccountCancellation,
    last_epoch: Option<u64>,
    selected: Vec<SelectedAccountRequestPlan>,
}

impl ScriptedHost {
    fn new(
        snapshot: &ProjectExecutionSnapshot,
        steps: Vec<Step>,
        cancellation: AccountCancellation,
    ) -> Self {
        Self {
            source: snapshot.source_signature(),
            steps: steps.into(),
            cancellation,
            last_epoch: None,
            selected: Vec::new(),
        }
    }

    fn take(
        &mut self,
        snapshot: &ProjectExecutionSnapshot,
        stage: Stage,
        control: Option<&AccountExecutionControl>,
    ) -> Step {
        assert_eq!(snapshot.source_signature(), self.source);
        assert!(
            !self.cancellation.is_cancelled(),
            "cancelled request dispatched another stage"
        );
        if let Some(control) = control {
            assert_eq!(control.expected_connection_epoch(), self.last_epoch);
        }
        let step = self.steps.pop_front().expect("unexpected extra stage");
        assert_eq!(step.stage, stage);
        if step.epoch.is_some() {
            self.last_epoch = step.epoch;
        }
        if step.cancel {
            self.cancellation.cancel();
        }
        step
    }

    fn exhausted(&self) {
        assert!(self.steps.is_empty(), "pipeline skipped a required stage");
    }
}

impl TasksExecutor for ScriptedHost {
    fn origin(
        &mut self,
        snapshot: &ProjectExecutionSnapshot,
    ) -> Result<HostCommandResult, CommandExecutionError> {
        let step = self.take(snapshot, Stage::Origin, None);
        step.result
            .map(|output| HostCommandResult {
                output,
                observed_connection_epoch: step.epoch,
            })
            .map_err(|_| {
                CommandExecutionError::new(CommandExecutionErrorKind::Disconnected, "fixture")
            })
    }

    fn capability(
        &mut self,
        snapshot: &ProjectExecutionSnapshot,
        capability: AccountCapability,
        control: &AccountExecutionControl,
    ) -> AccountHostResult<()> {
        let step = self.take(snapshot, Stage::Capability(capability), Some(control));
        AccountHostResult {
            result: step.result.and_then(|output| {
                verify_account_capability(capability, &output).map_err(Into::into)
            }),
            observed_connection_epoch: step.epoch,
        }
    }

    fn accounts(
        &mut self,
        snapshot: &ProjectExecutionSnapshot,
        host: &str,
        control: &AccountExecutionControl,
    ) -> AccountHostResult<KnownGitHubAccounts> {
        let step = self.take(snapshot, Stage::Accounts, Some(control));
        AccountHostResult {
            result: step
                .result
                .and_then(|output| parse_known_accounts(host, &output).map_err(Into::into)),
            observed_connection_epoch: step.epoch,
        }
    }

    fn selected(
        &mut self,
        snapshot: &ProjectExecutionSnapshot,
        plan: &SelectedAccountRequestPlan,
        control: &AccountExecutionControl,
    ) -> AccountHostResult<CommandOutput> {
        let step = self.take(snapshot, Stage::Selected(plan.stage()), Some(control));
        self.selected.push(plan.clone());
        let result = step.result.and_then(|output| {
            require_account_success(plan.stage(), &output)?;
            if plan.stage() == AccountCommandStage::Identity {
                verify_selected_account(plan.account(), &output)?;
            }
            Ok(output)
        });
        AccountHostResult {
            result,
            observed_connection_epoch: step.epoch,
        }
    }
}

fn preparation_steps(rows: &[(&str, bool, Option<&str>)], epoch: Option<u64>) -> Vec<Step> {
    vec![
        Step::origin(epoch),
        Step::capability(AccountCapability::AuthStatusJson, epoch),
        Step::capability(AccountCapability::NamedAccountLookup, epoch),
        Step::accounts(rows, epoch),
        Step::origin(epoch),
    ]
}

fn read_steps(kind: WorkItemKind, detail: bool, epoch: Option<u64>) -> Vec<Step> {
    vec![
        Step::origin(epoch),
        Step::accounts(&[("Alice", true, None), ("Bob", false, None)], epoch),
        Step::ok(
            Stage::Selected(if detail {
                AccountCommandStage::Detail
            } else {
                AccountCommandStage::List
            }),
            item_output(kind, !detail),
            epoch,
        ),
        Step::origin(epoch),
        Step::accounts(&[("Alice", false, None), ("Bob", true, None)], epoch),
        Step::ok(
            Stage::Selected(AccountCommandStage::Identity),
            output(r#"{"login":"Alice"}"#),
            epoch,
        ),
        Step::origin(epoch),
    ]
}

#[test]
fn selection_states_preserve_broken_peers_and_missing_choices() {
    let mixed = accounts(&[
        ("Alice", false, None),
        ("Bob", true, Some("HTTP 401 bad credentials")),
    ]);
    assert_eq!(mixed.accounts().len(), 2);
    assert_eq!(
        resolve_selection(&mixed, None),
        Err(TasksError::ChooseAccount)
    );
    assert_eq!(
        resolve_selection(&mixed, Some(&identity("alice")))
            .unwrap()
            .login(),
        "Alice"
    );
    assert_eq!(
        resolve_selection(&mixed, Some(&identity("bob"))),
        Err(AccountError::AuthenticationFailed.into())
    );
    let solo = accounts(&[("Alice", false, None)]);
    assert_eq!(
        resolve_selection(&solo, None).unwrap().identity(),
        &identity("alice")
    );
    assert_eq!(
        resolve_selection(&solo, Some(&identity("missing"))),
        Err(AccountError::SelectedAccountUnavailable.into())
    );
    assert_eq!(
        resolve_selection(&accounts(&[]), None),
        Err(AccountError::AuthRequired.into())
    );
}

#[test]
fn choices_round_trip_independently_by_root_execution_host_and_github_hostname() {
    let local = snapshot("main", "/repo", ExecutionBackend::Local);
    let sibling = snapshot("linked", "/repo-feature", ExecutionBackend::Local);
    let mut other_root = local.clone();
    other_root.root_project_id = "other".into();
    let mut other_host = local.clone();
    other_host.execution_host_id = ExecutionHostId::derive("different", &HostInstallId::new());
    let remote = snapshot("remote", "/repo", ssh(Some(7)));
    let wsl = snapshot(
        "wsl",
        "/repo",
        ExecutionBackend::Wsl {
            distro: "Ubuntu".into(),
        },
    );
    let scopes = [
        account_scope(&local, "github.com"),
        account_scope(&other_root, "github.com"),
        account_scope(&other_host, "github.com"),
        account_scope(&remote, "github.com"),
        account_scope(&wsl, "github.com"),
        account_scope(&local, "github.enterprise.test"),
    ];
    assert_eq!(scopes[0], account_scope(&sibling, "github.com"));
    let mut config = AppConfig::default();
    for (index, scope) in scopes.iter().enumerate() {
        write_selection(
            &mut config,
            scope.clone(),
            &GitHubAccountIdentity::new(&scope.github_host, &format!("User{index}")).unwrap(),
        );
    }
    assert_eq!(config.tasks_account_selections.len(), scopes.len());
    let restored: AppConfig =
        serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
    for (index, scope) in scopes.iter().enumerate() {
        assert_eq!(
            saved_selection(&restored, scope).unwrap().unwrap().login(),
            format!("user{index}")
        );
    }
    write_selection(&mut config, scopes[0].clone(), &identity("reselected"));
    assert_eq!(
        saved_selection(&config, &scopes[1])
            .unwrap()
            .unwrap()
            .login(),
        "user1"
    );
    let mut reconnected = remote;
    if let ExecutionBackend::Ssh {
        connection_epoch,
        connection_fingerprint,
        ..
    } = &mut reconnected.backend
    {
        *connection_epoch = Some(99);
        *connection_fingerprint = 42;
    }
    assert_eq!(account_scope(&reconnected, "github.com"), scopes[3]);
    let serialized = serde_json::to_string(&scopes[3]).unwrap();
    for forbidden in [
        "epoch",
        "fingerprint",
        "password",
        "token",
        "canonicalPath",
        "worktreeId",
    ] {
        assert!(
            !serialized
                .to_ascii_lowercase()
                .contains(&forbidden.to_ascii_lowercase())
        );
    }
}

#[test]
fn invalid_or_duplicate_saved_choice_does_not_autoselect() {
    let source = snapshot("main", "/repo", ExecutionBackend::Local);
    let scope = account_scope(&source, "github.com");
    let mut config = AppConfig::default();
    write_selection(&mut config, scope.clone(), &identity("Alice"));
    config.tasks_account_selections[0].login = "--bad".into();
    assert_eq!(
        saved_selection(&config, &scope),
        Err(AccountError::InvalidLogin.into())
    );
    config.tasks_account_selections[0].login = "alice".into();
    config
        .tasks_account_selections
        .push(config.tasks_account_selections[0].clone());
    assert_eq!(
        saved_selection(&config, &scope),
        Err(AccountError::MalformedResponse.into())
    );
}

#[test]
fn preparation_and_reads_use_exact_backend_and_ignore_external_active_account_changes() {
    for backend in [
        ExecutionBackend::Local,
        ExecutionBackend::Wsl {
            distro: "Ubuntu".into(),
        },
        ssh(Some(9)),
    ] {
        let epoch = if matches!(&backend, ExecutionBackend::Ssh { .. }) {
            Some(9)
        } else {
            None
        };
        let source = snapshot("main", "/repo with ' quote", backend);
        let cancellation = AccountCancellation::default();
        let mut host = ScriptedHost::new(
            &source,
            preparation_steps(&[("Alice", false, None), ("Bob", true, None)], epoch),
            cancellation.clone(),
        );
        let prepared = prepare_with(&source, &cancellation, &mut host)
            .result
            .unwrap();
        assert_eq!(prepared.repository, repo());
        assert_eq!(
            resolve_selection(&prepared.accounts, Some(&identity("alice")))
                .unwrap()
                .login(),
            "Alice"
        );
        host.exhausted();
        for kind in WorkItemKind::ALL {
            for detail in [false, true] {
                let mut host = ScriptedHost::new(
                    &source,
                    read_steps(kind, detail, epoch),
                    cancellation.clone(),
                );
                let target = if detail {
                    ReadTarget::Detail(kind, 7)
                } else {
                    ReadTarget::List(kind)
                };
                let read = read_with(
                    &source,
                    &repo(),
                    &identity("Alice"),
                    target,
                    &cancellation,
                    &mut host,
                );
                assert!(read.result.is_ok());
                assert_eq!(read.observed_source, source.source_signature());
                assert_eq!(host.selected.len(), 2);
                for plan in &host.selected {
                    assert_eq!(plan.lookup_login(), "Alice");
                    assert_eq!(plan.account(), &identity("alice"));
                    let argv = plan.data_plan().display_argv().join(" ");
                    for forbidden in ["auth switch", "auth login", "--web", "--show-token"] {
                        assert!(!argv.contains(forbidden));
                    }
                }
                assert!(
                    host.selected[0]
                        .data_plan()
                        .args
                        .iter()
                        .any(|arg| arg == &repo().cli_spec())
                );
                assert!(
                    host.selected[0]
                        .data_plan()
                        .args
                        .iter()
                        .any(|arg| arg == "--json")
                );
                host.exhausted();
            }
        }
    }
}

#[test]
fn first_connection_is_adopted_but_later_success_and_failure_epochs_are_rejected() {
    let source = snapshot("remote", "/repo", ssh(Some(3)));
    let cancellation = AccountCancellation::default();
    let mut host = ScriptedHost::new(
        &source,
        preparation_steps(&[("Alice", false, None)], Some(4)),
        cancellation.clone(),
    );
    let ready = prepare_with(&source, &cancellation, &mut host);
    assert!(ready.result.is_ok());
    assert_eq!(
        ready.observed_source,
        source.observed_source_signature(Some(4))
    );
    for failed in [false, true] {
        let mut changed = Step::capability(AccountCapability::AuthStatusJson, Some(5));
        if failed {
            changed.result = Err(AccountError::Offline.into());
        }
        let mut host = ScriptedHost::new(
            &source,
            vec![Step::origin(Some(4)), changed],
            cancellation.clone(),
        );
        let result = prepare_with(&source, &cancellation, &mut host);
        assert_eq!(result.result.unwrap_err(), TasksError::changed());
        assert_eq!(
            result.observed_source,
            source.observed_source_signature(Some(5))
        );
        host.exhausted();
    }
}

#[test]
fn account_failure_keeps_its_observed_epoch_and_distinct_category() {
    let source = snapshot("remote", "/repo", ssh(None));
    for error in [
        AccountExecutionError::HostHelperUnavailable,
        AccountError::ClientMissing.into(),
        AccountError::UnsupportedAuthStatusJson.into(),
        AccountError::UnsupportedNamedAccountLookup.into(),
        AccountError::CredentialStoreUnavailable.into(),
        AccountError::CredentialLookupFailed.into(),
        AccountError::RateLimited.into(),
        AccountError::PermissionDenied.into(),
        AccountExecutionError::TimedOut,
        AccountExecutionError::CleanupFailed,
        AccountExecutionError::SecretOutputRejected,
        AccountExecutionError::Protocol,
    ] {
        let cancellation = AccountCancellation::default();
        let mut failed = Step::capability(AccountCapability::AuthStatusJson, Some(9));
        failed.result = Err(error);
        let mut host = ScriptedHost::new(
            &source,
            vec![Step::origin(Some(9)), failed, Step::origin(Some(9))],
            cancellation.clone(),
        );
        let result = prepare_with(&source, &cancellation, &mut host);
        assert_eq!(result.result.unwrap_err(), TasksError::Account(error));
        assert_eq!(
            result.observed_source,
            source.observed_source_signature(Some(9))
        );
        host.exhausted();
    }
}

#[test]
fn cancellation_before_or_after_origin_never_dispatches_an_account_stage() {
    let source = snapshot("main", "/repo", ExecutionBackend::Local);
    for pre_cancelled in [true, false] {
        let cancellation = AccountCancellation::default();
        let steps = if pre_cancelled {
            cancellation.cancel();
            vec![]
        } else {
            let mut origin = Step::origin(None);
            origin.cancel = true;
            vec![origin]
        };
        let mut host = ScriptedHost::new(&source, steps, cancellation.clone());
        let result = prepare_with(&source, &cancellation, &mut host);
        assert_eq!(
            result.result.unwrap_err(),
            AccountExecutionError::Cancelled.into()
        );
        assert!(host.selected.is_empty());
        host.exhausted();
    }
}

#[test]
fn logout_and_wrong_identity_after_list_or_detail_reject_data() {
    let source = snapshot("main", "/repo", ExecutionBackend::Local);
    for detail in [false, true] {
        for logout in [false, true] {
            let cancellation = AccountCancellation::default();
            let mut steps = read_steps(WorkItemKind::Issue, detail, None);
            let expected = if logout {
                steps[4] = Step::accounts(&[("Bob", true, None)], None);
                steps.remove(5);
                AccountError::SelectedAccountUnavailable
            } else {
                steps[5] = Step::ok(
                    Stage::Selected(AccountCommandStage::Identity),
                    output(r#"{"login":"Bob"}"#),
                    None,
                );
                AccountError::WrongHostOrAccount
            };
            let mut host = ScriptedHost::new(&source, steps, cancellation.clone());
            let target = if detail {
                ReadTarget::Detail(WorkItemKind::Issue, 7)
            } else {
                ReadTarget::List(WorkItemKind::Issue)
            };
            let result = read_with(
                &source,
                &repo(),
                &identity("alice"),
                target,
                &cancellation,
                &mut host,
            );
            assert_eq!(result.result.unwrap_err(), expected.into());
            host.exhausted();
        }
    }
}

#[test]
fn cancellation_after_selected_data_does_not_publish_or_continue_identity_checks() {
    let source = snapshot("main", "/repo", ExecutionBackend::Local);
    for detail in [false, true] {
        let cancellation = AccountCancellation::default();
        let mut steps = read_steps(WorkItemKind::Issue, detail, None);
        steps.truncate(3);
        steps[2].cancel = true;
        let mut host = ScriptedHost::new(&source, steps, cancellation.clone());
        let target = if detail {
            ReadTarget::Detail(WorkItemKind::Issue, 7)
        } else {
            ReadTarget::List(WorkItemKind::Issue)
        };
        let result = read_with(
            &source,
            &repo(),
            &identity("alice"),
            target,
            &cancellation,
            &mut host,
        );
        assert_eq!(
            result.result.unwrap_err(),
            AccountExecutionError::Cancelled.into()
        );
        host.exhausted();
    }
}

#[test]
fn list_and_detail_reject_reconnected_data_and_origin_aba() {
    for detail in [false, true] {
        let source = snapshot("remote", "/repo", ssh(Some(9)));
        let cancellation = AccountCancellation::default();
        let mut steps = read_steps(WorkItemKind::Issue, detail, Some(9));
        steps.truncate(3);
        steps[2].epoch = Some(10);
        let target = if detail {
            ReadTarget::Detail(WorkItemKind::Issue, 7)
        } else {
            ReadTarget::List(WorkItemKind::Issue)
        };
        let mut host = ScriptedHost::new(&source, steps, cancellation.clone());
        let result = read_with(
            &source,
            &repo(),
            &identity("alice"),
            target,
            &cancellation,
            &mut host,
        );
        assert_eq!(result.result.unwrap_err(), TasksError::changed());
        assert_eq!(
            result.observed_source,
            source.observed_source_signature(Some(10))
        );
        host.exhausted();

        let mut steps = read_steps(WorkItemKind::Issue, detail, Some(9));
        steps.truncate(4);
        steps[3] = Step::ok(
            Stage::Origin,
            output("git@github.com:owner/replacement.git"),
            Some(9),
        );
        steps.push(Step::origin(Some(9)));
        let mut host = ScriptedHost::new(&source, steps, cancellation.clone());
        let result = read_with(
            &source,
            &repo(),
            &identity("alice"),
            target,
            &cancellation,
            &mut host,
        );
        assert!(
            matches!(result.result, Err(TasksError::Repository(error)) if error.kind == GitHubErrorKind::RepositoryChanged)
        );
        host.exhausted();
    }
}

#[test]
fn changed_origin_after_account_error_cannot_publish_that_error_for_another_repository() {
    let source = snapshot("main", "/repo", ExecutionBackend::Local);
    let cancellation = AccountCancellation::default();
    let mut failed = Step::capability(AccountCapability::AuthStatusJson, None);
    failed.result = Err(AccountError::AuthRequired.into());
    let mut host = ScriptedHost::new(
        &source,
        vec![
            Step::origin(None),
            failed,
            Step::ok(
                Stage::Origin,
                output("git@github.com:owner/replacement.git"),
                None,
            ),
        ],
        cancellation.clone(),
    );
    let result = prepare_with(&source, &cancellation, &mut host);
    assert!(
        matches!(result.result, Err(TasksError::Repository(error)) if error.kind == GitHubErrorKind::RepositoryChanged)
    );
    host.exhausted();
}

#[test]
fn each_worktree_proves_its_own_origin_before_sharing_a_data_identity() {
    let main = snapshot("main", "/repo", ExecutionBackend::Local);
    let linked = snapshot("linked", "/repo-linked", ExecutionBackend::Local);
    assert_ne!(main.source_signature(), linked.source_signature());
    let key = |source: &ProjectExecutionSnapshot, repository| {
        RepositoryCacheKey::new(&source.source_signature(), repository, "alice".into(), 7)
    };
    assert_eq!(key(&main, repo()), key(&linked, repo()));
    assert_eq!(
        key(&main, repo()),
        RepositoryCacheKey::new(&main.source_signature(), repo(), "Alice".into(), 7)
    );
    let changed = GitHubRepoIdentity::new("github.com", "owner", "different").unwrap();
    assert_ne!(key(&main, repo()), key(&linked, changed));
    assert_ne!(
        key(&main, repo()),
        key(&snapshot("ssh", "/repo", ssh(Some(1))), repo())
    );
}

fn ready_source(snapshot: ProjectExecutionSnapshot, generation: u64) -> SourceRecord {
    let mut source = SourceRecord::new(snapshot.clone(), 10);
    source.scope = Some(account_scope(&snapshot, "github.com"));
    source.selected = Some(identity("alice"));
    source.repository = Some(repo());
    source.auth_generation = generation;
    source.cache_key = Some(RepositoryCacheKey::new(
        &snapshot.source_signature(),
        repo(),
        "alice".into(),
        generation,
    ));
    source.phase = SourcePhase::Ready;
    source
}

fn owner(source: &SourceRecord, id: u64) -> RequestOwner {
    RequestOwner {
        source: source.snapshot.source_signature(),
        source_request: source.request_id,
        request_id: id,
        scope: source.scope.clone(),
        cache_key: source.cache_key.clone(),
        cancellation: AccountCancellation::default(),
    }
}

#[test]
fn scope_rotation_cancels_lists_details_and_sibling_readiness_without_touching_other_choices() {
    let main = ready_source(snapshot("main", "/repo", ExecutionBackend::Local), 7);
    let linked = ready_source(
        snapshot("linked", "/repo-linked", ExecutionBackend::Local),
        7,
    );
    let mut other_snapshot = snapshot(
        "other",
        "/repo",
        ExecutionBackend::Wsl {
            distro: "Ubuntu".into(),
        },
    );
    other_snapshot.root_project_id = "other".into();
    let other = ready_source(other_snapshot, 8);
    let scope = main.scope.clone().unwrap();
    let mut generations = HashMap::from([(scope.clone(), 7), (other.scope.clone().unwrap(), 8)]);
    let list_owner = owner(&main, 21);
    let detail_owner = owner(&linked, 22);
    let other_owner = owner(&other, 23);
    let old_key = main.cache_key.clone().unwrap();
    let other_key = other.cache_key.clone().unwrap();
    let mut requests = HashMap::from([
        (21, list_owner.clone()),
        (22, detail_owner.clone()),
        (23, other_owner.clone()),
    ]);
    let mut repositories = HashMap::from([
        (old_key.clone(), RepositoryCache::default()),
        (other_key.clone(), RepositoryCache::default()),
    ]);
    let main_key = main.snapshot.source_signature();
    let linked_key = linked.snapshot.source_signature();
    let mut sources = HashMap::from([
        (main_key.clone(), main),
        (linked_key.clone(), linked),
        (other.snapshot.source_signature(), other),
    ]);
    rotate_scope_state(
        &scope,
        30,
        &mut generations,
        &mut requests,
        &mut sources,
        &mut repositories,
    );
    assert!(list_owner.cancellation.is_cancelled() && detail_owner.cancellation.is_cancelled());
    assert!(!other_owner.cancellation.is_cancelled());
    assert_eq!(requests.len(), 1);
    assert!(requests.contains_key(&23));
    assert!(!repositories.contains_key(&old_key));
    assert!(repositories.contains_key(&other_key));
    assert!(!source_is_ready(sources.get(&main_key), &old_key));
    assert!(!source_is_ready(sources.get(&linked_key), &old_key));
    assert!(!owns_request(sources.get(&main_key), &list_owner));
    assert!(owns_request(sources.get(&other_owner.source), &other_owner));
}

#[test]
fn aba_source_account_repository_and_request_generations_cannot_revive_a_completion() {
    let mut source = ready_source(snapshot("main", "/repo", ExecutionBackend::Local), 7);
    let owner = owner(&source, 21);
    let key = source.cache_key.clone().unwrap();
    assert!(owns_request(Some(&source), &owner));
    assert!(owns_slot(21, true, &owner));
    assert!(!owns_slot(22, true, &owner));
    assert!(!owns_slot(21, false, &owner));
    source.invalidate(25, false);
    source.phase = SourcePhase::Ready;
    assert!(!owns_request(Some(&source), &owner));
    source.request_id = owner.source_request;
    source.auth_generation = 8;
    assert!(!owns_request(Some(&source), &owner));
    assert!(!source_is_ready(Some(&source), &key));
    source.auth_generation = 7;
    source.selected = Some(identity("bob"));
    assert!(!owns_request(Some(&source), &owner));
    source.selected = Some(identity("alice"));
    source.cache_key.as_mut().unwrap().repository =
        GitHubRepoIdentity::new("github.com", "owner", "replacement").unwrap();
    assert!(!owns_request(Some(&source), &owner));
    source.cache_key = Some(key);
    source.snapshot.canonical_path = "/replaced".into();
    assert!(!owns_request(Some(&source), &owner));
    source.snapshot.canonical_path = "/repo".into();
    owner.cancellation.cancel();
    assert!(!owns_request(Some(&source), &owner));
}

#[test]
fn preparation_fences_known_scope_changes_including_aba_but_not_other_github_hosts() {
    let snapshot = snapshot("main", "/repo", ExecutionBackend::Local);
    let scope = account_scope(&snapshot, "github.com");
    let enterprise = account_scope(&snapshot, "github.enterprise.test");
    let mut config = AppConfig::default();
    write_selection(&mut config, scope.clone(), &identity("alice"));
    let choices = config.tasks_account_selections.clone();
    let before = HashMap::from([(scope.clone(), 7)]);
    let mut after = before.clone();
    after.insert(enterprise, 8);
    assert!(preparation_scope_matches(
        &scope,
        &before,
        &after,
        &choices,
        &config.tasks_account_selections
    ));
    write_selection(&mut config, scope.clone(), &identity("bob"));
    assert!(!preparation_scope_matches(
        &scope,
        &before,
        &after,
        &choices,
        &config.tasks_account_selections
    ));
    write_selection(&mut config, scope.clone(), &identity("alice"));
    after.insert(scope.clone(), 9);
    assert!(!preparation_scope_matches(
        &scope,
        &before,
        &after,
        &choices,
        &config.tasks_account_selections
    ));
}

#[test]
fn work_item_tabs_preserve_worktree_identity_and_errors_do_not_collapse_into_auth_required() {
    let main = snapshot("main", "/repo", ExecutionBackend::Local);
    let sibling = snapshot("sibling", "/repo-sibling", ExecutionBackend::Local);
    let key = |worktree_id| GitHubWorkItemTabKey {
        worktree_id,
        repository: repo(),
        kind: WorkItemKind::Issue,
        number: 7,
    };
    assert_ne!(key(main.worktree_id), key(sibling.worktree_id));
    for error in [
        AccountError::CredentialStoreUnavailable,
        AccountError::ScopeRequired,
        AccountError::PermissionDenied,
        AccountError::UnsupportedNamedAccountLookup,
        AccountError::RateLimited,
    ] {
        let tasks = TasksError::from(error);
        assert!(!tasks.offers_login());
        assert_eq!(tasks.summary(), error.summary());
        assert!(!tasks.summary().contains("AuthRequired"));
    }
    assert!(TasksError::from(AccountError::Offline).retains_last_known());
    assert!(!TasksError::from(AccountError::AuthenticationFailed).retains_last_known());
    assert!(TasksError::from(AccountError::SelectedAccountUnavailable).offers_login());
}

#[test]
fn rollback_and_bounded_context_remain_inert() {
    assert!(!github_project_tasks_enabled_for(Some(OsStr::new("0"))));
    for value in [None, Some(OsStr::new("false")), Some(OsStr::new("1"))] {
        assert!(github_project_tasks_enabled_for(value));
    }
    assert_eq!(bounded_context("host\nname\0"), "hostname");
    assert_eq!(bounded_context(&"x".repeat(1000)).len(), 512);
}

#[test]
fn foreground_list_access_is_consumed_once_and_notifications_cannot_rearm_it() {
    let mut source = ready_source(snapshot("main", "/repo", ExecutionBackend::Local), 7);
    let previous = owner(&source, 21);
    let key = source.cache_key.clone().unwrap();
    source.begin_list_access(30, WorkItemKind::Issue);
    assert!(!source_is_ready(Some(&source), &key));
    assert!(owns_request(Some(&source), &previous));
    assert_eq!(source.list_access_id, 30);
    assert_eq!(source.take_list_access(), Some((30, WorkItemKind::Issue)));
    for phase in [
        SourcePhase::Loading,
        SourcePhase::Ready,
        SourcePhase::Error,
        SourcePhase::Idle,
    ] {
        source.phase = phase;
        for _ in 0..5 {
            assert_eq!(source.take_list_access(), None);
        }
    }
    source.begin_list_access(31, WorkItemKind::PullRequest);
    source.begin_list_access(32, WorkItemKind::Issue);
    assert_eq!(source.take_list_access(), Some((32, WorkItemKind::Issue)));
    assert_eq!(source.take_list_access(), None);
    source.begin_list_access(33, WorkItemKind::Issue);
    source.invalidate(34, false);
    assert_eq!(source.take_list_access(), None);
}

#[test]
fn cached_list_foreground_rechecks_logout_and_origin_without_passive_retries() {
    for replaced in [false, true] {
        let mut source = ready_source(snapshot("main", "/repo", ExecutionBackend::Local), 7);
        let key = source.cache_key.clone().unwrap();
        let mut slot = ListSlot {
            rows: mt_github::parse_work_item_list(
                WorkItemKind::Issue,
                &item_output(WorkItemKind::Issue, true),
            )
            .unwrap(),
            updated_at_unix_ms: Some(1),
            request_id: 20,
            ..Default::default()
        };
        source.begin_list_access(30, WorkItemKind::Issue);
        let (_, kind) = source.take_list_access().unwrap();
        assert!(!source_is_ready(Some(&source), &key));
        let access = owner(&source, 31);
        slot.request_id = access.request_id;
        slot.loading = true;
        let steps = if replaced {
            let changed = || {
                Step::ok(
                    Stage::Origin,
                    output("git@github.com:owner/replaced.git"),
                    None,
                )
            };
            vec![changed(), changed()]
        } else {
            vec![
                Step::origin(None),
                Step::accounts(&[("Bob", true, None)], None),
                Step::origin(None),
            ]
        };
        let mut host = ScriptedHost::new(&source.snapshot, steps, access.cancellation.clone());
        let completion = read_with(
            &source.snapshot,
            &repo(),
            &identity("alice"),
            ReadTarget::List(kind),
            &access.cancellation,
            &mut host,
        );
        let error = completion.result.as_ref().unwrap_err().clone();
        assert_eq!(invalidates_account(&error), !replaced);
        assert!(apply_list_completion(&mut slot, &access, completion.result));
        assert!(slot.rows.is_empty() && slot.updated_at_unix_ms.is_none());
        assert_eq!(
            apply_source_completion(
                &mut source,
                ReadTarget::List(kind),
                None,
                Some(error.clone())
            ),
            Some(error)
        );
        assert!(!source_is_ready(Some(&source), &key));
        for _ in 0..5 {
            if let Some((_, kind)) = source.take_list_access() {
                read_with(
                    &source.snapshot,
                    &repo(),
                    &identity("alice"),
                    ReadTarget::List(kind),
                    &access.cancellation,
                    &mut host,
                );
                panic!("passive notification dispatched another list request");
            }
        }
        assert_eq!(source.list_access_id, 30);
        host.exhausted();
    }
}

#[test]
fn new_list_access_cancels_only_list_and_preparation_owners_not_detail_accesses() {
    let source = ready_source(snapshot("main", "/repo", ExecutionBackend::Local), 7);
    let list = owner(&source, 21);
    let detail = owner(&source, 22);
    let mut preparation = owner(&source, 23);
    preparation.cache_key = None;
    let mut other_source = source.snapshot.clone();
    other_source.canonical_path = "/another-worktree".into();
    let other = owner(&ready_source(other_source, 7), 24);
    let requests = HashMap::from([
        (21, list.clone()),
        (22, detail.clone()),
        (23, preparation),
        (24, other),
    ]);
    let key = source.cache_key.clone().unwrap();
    let repositories = HashMap::from([(
        key,
        RepositoryCache {
            lists: HashMap::from([(
                WorkItemKind::Issue,
                ListSlot {
                    request_id: 21,
                    loading: true,
                    ..Default::default()
                },
            )]),
            details: HashMap::from([(
                (WorkItemKind::Issue, 7),
                DetailSlot {
                    request_id: 22,
                    loading: true,
                    ..Default::default()
                },
            )]),
        },
    )]);
    let mut cancelled = list_request_ids(&list.source, &requests, &repositories);
    cancelled.sort_unstable();
    assert_eq!(cancelled, [21, 23]);
    assert!(owns_request(Some(&source), &detail));
}

fn cached_detail_slot() -> DetailSlot {
    DetailSlot {
        detail: Some(
            mt_github::parse_work_item_detail(
                WorkItemKind::Issue,
                &item_output(WorkItemKind::Issue, false),
            )
            .unwrap(),
        ),
        request_id: 20,
        ..Default::default()
    }
}

#[test]
fn cached_detail_reopen_rejects_logout_through_pipeline_and_scope_invalidation() {
    let source = ready_source(snapshot("main", "/repo", ExecutionBackend::Local), 7);
    let access = owner(&source, 21);
    let scope = source.scope.clone().unwrap();
    let key = source.cache_key.clone().unwrap();
    let mut slot = cached_detail_slot();
    slot.begin_access(access.request_id);
    let stale = detail_slot_view(Some(&slot), &access);
    assert!(stale.loading && stale.detail.is_some());
    let mut host = ScriptedHost::new(
        &source.snapshot,
        vec![
            Step::origin(None),
            Step::accounts(&[("Bob", true, None)], None),
            Step::origin(None),
        ],
        access.cancellation.clone(),
    );
    let completion = read_with(
        &source.snapshot,
        &repo(),
        &identity("alice"),
        ReadTarget::Detail(WorkItemKind::Issue, 7),
        &access.cancellation,
        &mut host,
    );
    let error = completion.result.unwrap_err();
    assert_eq!(error, AccountError::SelectedAccountUnavailable.into());
    assert!(invalidates_account(&error));
    host.exhausted();
    let mut sources = HashMap::from([(access.source.clone(), source)]);
    let mut requests = HashMap::from([(access.request_id, access.clone())]);
    let mut repositories = HashMap::from([(
        key.clone(),
        RepositoryCache {
            details: HashMap::from([((WorkItemKind::Issue, 7), slot)]),
            ..Default::default()
        },
    )]);
    let mut scopes = HashMap::from([(scope.clone(), 7)]);
    rotate_scope_state(
        &scope,
        30,
        &mut scopes,
        &mut requests,
        &mut sources,
        &mut repositories,
    );
    assert!(!repositories.contains_key(&key));
    assert!(!owns_request(sources.get(&access.source), &access));
    assert!(!source_has_identity(sources.get(&access.source), &key));
    assert!(detail_slot_view(None, &access).detail.is_none());
}

#[test]
fn cached_detail_reopen_revalidates_origin_and_preserves_only_transient_last_known_data() {
    let source = ready_source(snapshot("main", "/repo", ExecutionBackend::Local), 7);
    for replaced in [true, false] {
        let access = owner(&source, 21);
        let mut slot = cached_detail_slot();
        slot.begin_access(access.request_id);
        let steps = if replaced {
            let changed = || {
                Step::ok(
                    Stage::Origin,
                    output("git@github.com:owner/replaced.git"),
                    None,
                )
            };
            vec![changed(), changed()]
        } else {
            let mut offline = Step::accounts(&[], None);
            offline.result = Err(AccountError::Offline.into());
            vec![Step::origin(None), offline, Step::origin(None)]
        };
        let mut host = ScriptedHost::new(&source.snapshot, steps, access.cancellation.clone());
        let completion = read_with(
            &source.snapshot,
            &repo(),
            &identity("alice"),
            ReadTarget::Detail(WorkItemKind::Issue, 7),
            &access.cancellation,
            &mut host,
        );
        assert!(completion.result.is_err());
        assert!(apply_detail_completion(
            &mut slot,
            &access,
            completion.result
        ));
        let view = detail_slot_view(Some(&slot), &access);
        assert!(!view.loading && view.error.is_some());
        assert_eq!(view.detail.is_some(), !replaced);
        host.exhausted();
    }
}

#[test]
fn revalidated_detail_does_not_inherit_an_earlier_source_failure() {
    let mut source = ready_source(snapshot("main", "/repo", ExecutionBackend::Local), 7);
    source.phase = SourcePhase::Error;
    source.error = Some(mt_github::GitHubError::repository_changed().into());
    let access = owner(&source, 21);
    let mut slot = cached_detail_slot();
    slot.begin_access(access.request_id);
    let target = ReadTarget::Detail(WorkItemKind::Issue, 7);
    let mut host = ScriptedHost::new(
        &source.snapshot,
        read_steps(WorkItemKind::Issue, true, None),
        access.cancellation.clone(),
    );
    let completion = read_with(
        &source.snapshot,
        &repo(),
        &identity("alice"),
        target,
        &access.cancellation,
        &mut host,
    );
    let result = completion.result.unwrap();
    let accounts = result.accounts.clone();
    assert!(apply_detail_completion(&mut slot, &access, Ok(result)));
    assert_eq!(
        apply_source_completion(&mut source, target, Some(accounts), None),
        None
    );
    assert!(owns_request(Some(&source), &access));
    let view = detail_slot_view(Some(&slot), &access);
    assert!(view.detail.is_some() && view.error.is_none() && !view.loading);
    assert!(!source_is_ready(
        Some(&source),
        access.cache_key.as_ref().unwrap()
    ));
    host.exhausted();
    assert_eq!(
        apply_source_completion(
            &mut source,
            target,
            None,
            Some(AccountError::NotFound.into())
        ),
        None
    );
    assert_eq!(
        apply_source_completion(
            &mut source,
            ReadTarget::List(WorkItemKind::Issue),
            None,
            Some(AccountError::NotFound.into())
        ),
        Some(AccountError::NotFound.into())
    );
    let replaced: TasksError = mt_github::GitHubError::repository_changed().into();
    assert_eq!(
        apply_source_completion(&mut source, target, None, Some(replaced.clone())),
        Some(replaced)
    );
}

#[test]
fn detail_access_receipts_reject_reopen_aba_and_never_borrow_another_access_result() {
    let source = ready_source(snapshot("main", "/repo", ExecutionBackend::Local), 7);
    let old = owner(&source, 21);
    let current = owner(&source, 23);
    let mut slot = cached_detail_slot();
    slot.begin_access(old.request_id);
    slot.begin_access(current.request_id);
    let result = || {
        Ok(ReadSuccess {
            data: ReadData::Detail(
                mt_github::parse_work_item_detail(
                    WorkItemKind::Issue,
                    &item_output(WorkItemKind::Issue, false),
                )
                .unwrap(),
            ),
            accounts: accounts(&[("Alice", false, None)]),
        })
    };
    assert!(!apply_detail_completion(&mut slot, &old, result()));
    assert!(detail_slot_view(Some(&slot), &old).detail.is_none());
    assert!(detail_slot_view(Some(&slot), &current).loading);
    assert!(apply_detail_completion(&mut slot, &current, result()));
    assert!(!detail_slot_view(Some(&slot), &current).loading);
    assert!(detail_slot_view(Some(&slot), &current).detail.is_some());
    assert!(!apply_detail_completion(
        &mut slot,
        &old,
        Err(AccountError::Offline.into())
    ));
    assert!(!apply_detail_completion(&mut slot, &current, result()));
    current.cancellation.cancel();
    assert!(detail_slot_view(Some(&slot), &current).detail.is_none());
}
