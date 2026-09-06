use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use mt_github::{GitHubRepoIdentity, WorkItemKind};
use mt_identity::{ExecutionHostId, HostInstallId, RepoId, WorktreeId};

use super::*;
#[cfg(windows)]
use crate::execution_host::{
    HostCommandResult, TasksWslProbeCwd, TasksWslProbeStdin, TasksWslProbeUser,
    tasks_wsl_marker_probe,
};
#[cfg(windows)]
use mt_github::{CommandExecutionError, CommandExecutionErrorKind};

struct Fixture {
    root: PathBuf,
    executable: PathBuf,
}

struct DescendantEvidence {
    ready: PathBuf,
    release: PathBuf,
    marker: PathBuf,
}

impl DescendantEvidence {
    fn new() -> Self {
        let id = uuid::Uuid::new_v4();
        let root = &fixture().root;
        Self {
            ready: root.join(format!("ready-{id}")),
            release: root.join(format!("release-{id}")),
            marker: root.join(format!("marker-{id}")),
        }
    }

    fn configure(&self, command: &mut Command) {
        command
            .env("MT_FIXTURE_READY", &self.ready)
            .env("MT_FIXTURE_RELEASE", &self.release)
            .env("MT_FIXTURE_MARKER", &self.marker);
    }

    fn cancel_after_start(
        &self,
        cancellation: AccountCancellation,
    ) -> std::thread::JoinHandle<bool> {
        let ready = self.ready.clone();
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !ready.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            let started = ready.exists();
            cancellation.cancel();
            started
        })
    }

    fn assert_retired(&self) {
        assert!(
            self.ready.exists(),
            "cleanup fixture never started its descendant"
        );
        std::fs::write(&self.release, b"release").unwrap();
        std::thread::sleep(Duration::from_secs(1));
        assert!(!self.marker.exists(), "request left a live descendant");
    }

    #[cfg(unix)]
    fn in_directory(root: &Path) -> Self {
        Self {
            ready: root.join("ready"),
            release: root.join("release"),
            marker: root.join("marker"),
        }
    }
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        assert_eq!(
            std::env::var("GITHUB_ACTIONS").as_deref(),
            Ok("true"),
            "Actions-only fixture"
        );
        let root = std::env::temp_dir().join(format!("mt-tasks-accounts-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("gh_fixture.rs");
        std::fs::write(&source, include_str!("gh_fixture.rs")).unwrap();
        let executable = root.join(if cfg!(windows) { "gh.exe" } else { "gh" });
        let result = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .arg("--edition=2021")
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "synthetic gh fixture compilation failed"
        );
        Fixture { root, executable }
    })
}

fn snapshot(path: &Path) -> ProjectExecutionSnapshot {
    let host = ExecutionHostId::derive("tasks-fixture", &HostInstallId::new());
    let repo = RepoId::derive(&host, "fixture");
    ProjectExecutionSnapshot {
        project_id: "fixture".into(),
        root_project_id: "fixture".into(),
        worktree_id: WorktreeId::derive(&repo, "fixture", None),
        execution_host_id: host,
        canonical_path: path.to_str().unwrap().into(),
        root_source_path: path.to_str().unwrap().into(),
        backend: ExecutionBackend::Local,
        host_label: "Actions fixture".into(),
    }
}

fn selected(host: &str, login: &str) -> SelectedAccountRequestPlan {
    selected_reads(host, login)[0].clone()
}

fn selected_reads(host: &str, login: &str) -> [SelectedAccountRequestPlan; 2] {
    let output = CommandOutput {
        stdout: serde_json::to_vec(&serde_json::json!({"hosts": {(host): [{
            "state": "success", "active": true, "host": host, "login": login,
            "tokenSource": "keyring"
        }]}}))
        .unwrap(),
        exit_code: Some(0),
        ..CommandOutput::default()
    };
    let known = parse_known_accounts(host, &output).unwrap();
    let repo = GitHubRepoIdentity::new(host, "owner", "repo").unwrap();
    [
        SelectedAccountRequestPlan::list(&known.accounts()[0], &repo, WorkItemKind::Issue).unwrap(),
        SelectedAccountRequestPlan::detail(&known.accounts()[0], &repo, WorkItemKind::Issue, 7)
            .unwrap(),
    ]
}

fn control(timeout: Duration) -> AccountExecutionControl {
    AccountExecutionControl::new(timeout, AccountCancellation::default(), None).unwrap()
}

fn contaminate(command: &mut Command) {
    for key in [
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_ENTERPRISE_TOKEN",
        "GITHUB_ENTERPRISE_TOKEN",
        "GH_DEBUG",
        "DEBUG",
        "GH_HOST",
        "GH_REPO",
        "GH_FORCE_TTY",
        "BASH_ENV",
        "ENV",
        "WSLENV",
    ] {
        command.env(key, "inherited_fixture_secret");
    }
}

fn native_request(
    plan: &SelectedAccountRequestPlan,
    control: &AccountExecutionControl,
) -> Result<CommandOutput, AccountExecutionError> {
    let fixture = fixture();
    let root = fixture.root.join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&root).unwrap();
    process::native_fixture(
        &snapshot(&root),
        &plan.data_plan(),
        Some(plan),
        control,
        plan.output_limit(),
        &|| {
            let mut command = Command::new(&fixture.executable);
            contaminate(&mut command);
            command
        },
    )
}

fn assert_selected_data(output: &CommandOutput, plan: &SelectedAccountRequestPlan, login: &str) {
    let summary = match plan.stage() {
        AccountCommandStage::List => {
            let mut rows = mt_github::parse_work_item_list(WorkItemKind::Issue, output).unwrap();
            assert_eq!(rows.len(), 1);
            rows.remove(0)
        }
        AccountCommandStage::Detail => {
            let detail = mt_github::parse_work_item_detail(WorkItemKind::Issue, output).unwrap();
            assert_eq!(detail.summary.number, 7);
            assert_eq!(detail.body, login);
            detail.summary
        }
        _ => panic!("fixture requires a data request"),
    };
    assert_eq!(summary.title, login);
    assert_eq!(summary.author.as_deref(), Some(login));
    assert!(output.stderr.is_empty());
}

#[test]
fn native_sentinel_is_only_in_the_intended_child_environment() {
    for host in ["github.com", "org.ghe.com", "github.example.com"] {
        for login in ["Alice", "Bob"] {
            for plan in selected_reads(host, login) {
                let result = native_request(&plan, &control(Duration::from_secs(10))).unwrap();
                assert_selected_data(&result, &plan, login);
                assert!(!format!("{plan:?} {result:?}").contains("fixture_credential_"));
            }
        }
    }
    std::thread::scope(|scope| {
        for login in ["Alice", "Bob"] {
            scope.spawn(move || {
                let plan = selected("github.com", login);
                let result = native_request(&plan, &control(Duration::from_secs(10))).unwrap();
                assert_selected_data(&result, &plan, login);
            });
        }
    });
}

#[test]
fn native_lookup_failures_wrong_proof_and_echoes_are_static_errors() {
    for (login, expected) in [
        ("Broken", AccountError::CredentialLookupFailed.into()),
        ("Store", AccountError::CredentialStoreUnavailable.into()),
        ("Wrong", AccountError::WrongHostOrAccount.into()),
        ("WrongAfter", AccountError::WrongHostOrAccount.into()),
        ("Leak", AccountExecutionError::SecretOutputRejected),
        ("LeakStderr", AccountExecutionError::SecretOutputRejected),
        ("Large", AccountError::MalformedResponse.into()),
    ] {
        let error = native_request(
            &selected("github.com", login),
            &control(Duration::from_secs(10)),
        )
        .unwrap_err();
        assert_eq!(error, expected);
        assert!(!format!("{error:?} {error}").contains("fixture_credential_"));
    }
}

#[test]
fn native_post_read_proof_keeps_the_original_credential_after_lookup_changes() {
    for plan in selected_reads("github.com", "Rotate") {
        let result = native_request(&plan, &control(Duration::from_secs(10))).unwrap();
        assert_selected_data(&result, &plan, "Rotate");
    }
    for (login, expected) in [
        ("WrongAfter", AccountError::WrongHostOrAccount.into()),
        ("LeakStderr", AccountExecutionError::SecretOutputRejected),
    ] {
        for plan in selected_reads("github.com", login) {
            let error = native_request(&plan, &control(Duration::from_secs(10))).unwrap_err();
            assert_eq!(error, expected);
        }
    }
}

#[test]
fn native_discovery_and_capability_execute_with_sanitized_environment() {
    let fixture = fixture();
    let make_gh = || {
        let mut command = Command::new(&fixture.executable);
        contaminate(&mut command);
        command
    };
    let run = |plan: &CommandPlan| {
        process::native_fixture(
            &snapshot(&fixture.root),
            plan,
            None,
            &control(Duration::from_secs(10)),
            mt_github::COMMAND_OUTPUT_LIMIT,
            &make_gh,
        )
        .unwrap()
    };
    for capability in [
        AccountCapability::AuthStatusJson,
        AccountCapability::NamedAccountLookup,
    ] {
        verify_account_capability(capability, &run(&account_capability_plan(capability))).unwrap();
    }
    let known = parse_known_accounts(
        "github.com",
        &run(&known_accounts_plan("github.com").unwrap()),
    )
    .unwrap();
    assert_eq!(known.accounts().len(), 2);
    assert_eq!(
        known.accounts()[1].problem(),
        Some(AccountError::AuthenticationFailed)
    );
    assert!(!format!("{known:?}").contains("fixture_credential_"));
    let enumeration = known_accounts_plan("github.com").unwrap();
    assert!(!format!("{:?}", run(&enumeration)).contains("fixture_credential_"));
}

#[test]
fn native_process_tree_cleans_descendants_on_success_timeout_and_cancel() {
    let fixture = fixture();
    for (login, cancel) in [
        ("Descendant", false),
        ("PipeDescendant", false),
        ("Slow", false),
        ("Slow", true),
        ("LookupSlow", false),
        ("LookupSlow", true),
    ] {
        let evidence = DescendantEvidence::new();
        let plan = selected("github.com", login);
        let cancellation = AccountCancellation::default();
        let success = login.ends_with("Descendant");
        let control = AccountExecutionControl::new(
            Duration::from_secs(if success || cancel { 15 } else { 5 }),
            cancellation.clone(),
            None,
        )
        .unwrap();
        let canceller = cancel.then(|| evidence.cancel_after_start(cancellation));
        let result = process::native_fixture(
            &snapshot(&fixture.root),
            &plan.data_plan(),
            Some(&plan),
            &control,
            plan.output_limit(),
            &|| {
                let mut command = Command::new(&fixture.executable);
                evidence.configure(&mut command);
                command
            },
        );
        if let Some(canceller) = canceller {
            assert!(canceller.join().unwrap());
        }
        if success {
            assert!(result.is_ok());
        } else {
            assert_eq!(
                result.unwrap_err(),
                if cancel {
                    AccountExecutionError::Cancelled
                } else {
                    AccountExecutionError::TimedOut
                }
            );
        }
        evidence.assert_retired();
    }
}

#[cfg(windows)]
#[test]
fn native_windows_failed_attachment_reaps_the_suspended_child() {
    let fixture = fixture();
    let marker = fixture
        .root
        .join(format!("suspended-{}", uuid::Uuid::new_v4()));
    let mut command = Command::new(&fixture.executable);
    command.arg("--suspended").env("MT_FIXTURE_MARKER", &marker);
    process::suspended_cleanup_fixture(command);
    assert!(!marker.exists(), "unassigned child was resumed");
}

#[cfg(unix)]
fn envelope_request(
    plan: &SelectedAccountRequestPlan,
    control: &AccountExecutionControl,
    evidence: Option<&DescendantEvidence>,
) -> Result<CommandOutput, AccountExecutionError> {
    envelope_command(
        &plan.data_plan(),
        Some(plan),
        control,
        plan.output_limit(),
        |command| {
            if let Some(evidence) = evidence {
                evidence.configure(command);
            }
        },
    )
}

#[cfg(unix)]
fn envelope_command(
    data: &CommandPlan,
    plan: Option<&SelectedAccountRequestPlan>,
    control: &AccountExecutionControl,
    limit: usize,
    configure: impl FnOnce(&mut Command),
) -> Result<CommandOutput, AccountExecutionError> {
    let fixture = fixture();
    let root = fixture.root.join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&root).unwrap();
    let mut source = snapshot(&root);
    source.backend = ExecutionBackend::Wsl {
        distro: "fixture".into(),
    };
    let envelope = host_envelope_plan(&source, data, plan, control, limit).unwrap();
    let mut command = Command::new("/bin/sh");
    command.args(["-x", "-c", &ssh_envelope_command(&envelope).unwrap()]);
    let mut paths = vec![fixture.root.clone()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    command.env("PATH", std::env::join_paths(paths).unwrap());
    contaminate(&mut command);
    configure(&mut command);
    let bytes = process::envelope_fixture(command, control, wire_limit(limit))?;
    assert!(!String::from_utf8_lossy(&bytes).contains("fixture_credential_"));
    decode_host_reply(&bytes, limit)
}

#[cfg(unix)]
#[test]
fn host_envelope_discovery_projects_only_nonsecret_status_and_preserves_categories() {
    let control = control(Duration::from_secs(10));
    for capability in [
        AccountCapability::AuthStatusJson,
        AccountCapability::NamedAccountLookup,
    ] {
        let output = envelope_command(
            &account_capability_plan(capability),
            None,
            &control,
            65536,
            |_| {},
        )
        .unwrap();
        verify_account_capability(capability, &output).unwrap();
        let unsupported = envelope_command(
            &account_capability_plan(capability),
            None,
            &control,
            65536,
            |command| {
                command.env("MT_FIXTURE_UNSUPPORTED", "1");
            },
        )
        .unwrap();
        assert!(verify_account_capability(capability, &unsupported).is_err());
    }
    let plan = known_accounts_plan("github.com").unwrap();
    for (diagnostic, expected) in [
        ("Bad credentials", AccountError::AuthenticationFailed),
        (
            "keyring access denied",
            AccountError::CredentialStoreUnavailable,
        ),
        ("no oauth token found", AccountError::CredentialLookupFailed),
        ("insufficient scopes", AccountError::ScopeRequired),
        ("HTTP 403", AccountError::PermissionDenied),
        ("rate limit", AccountError::RateLimited),
        ("no such host", AccountError::Offline),
        ("unknown problem", AccountError::CommandFailed),
    ] {
        let output = envelope_command(&plan, None, &control, 65536, |command| {
            command.env("MT_FIXTURE_ENUM_ERROR", diagnostic);
        })
        .unwrap();
        assert!(!format!("{output:?}").contains("fixture_credential_"));
        let known = parse_known_accounts("github.com", &output).unwrap();
        assert_eq!(known.accounts().len(), 2);
        assert_eq!(known.accounts()[1].problem(), Some(expected));
    }
    let none = envelope_command(&plan, None, &control, 65536, |command| {
        command.env("MT_FIXTURE_ENUM_CASE", "none");
    })
    .unwrap();
    assert!(
        parse_known_accounts("github.com", &none)
            .unwrap()
            .accounts()
            .is_empty()
    );
    let unsupported = envelope_command(&plan, None, &control, 65536, |command| {
        command.env("MT_FIXTURE_ENUM_CASE", "unsupported");
    })
    .unwrap();
    assert_eq!(
        parse_known_accounts("github.com", &unsupported).unwrap_err(),
        AccountError::UnsupportedAuthStatusJson
    );
    let malformed = envelope_command(&plan, None, &control, 65536, |command| {
        command.env("MT_FIXTURE_ENUM_CASE", "malformed");
    })
    .unwrap_err();
    assert_eq!(malformed, AccountError::MalformedResponse.into());
    let empty_bin = fixture().root.join("no-python");
    std::fs::create_dir_all(&empty_bin).unwrap();
    let missing = envelope_command(&plan, None, &control, 65536, |command| {
        command.env("PATH", &empty_bin);
    })
    .unwrap_err();
    assert_eq!(missing, AccountExecutionError::HostHelperUnavailable);
}

#[cfg(unix)]
#[test]
fn host_envelope_executes_sentinels_with_tracing_and_inherited_auth_disabled() {
    for host in ["github.com", "org.ghe.com", "github.example.com"] {
        for login in ["Alice", "Bob", "Rotate"] {
            for plan in selected_reads(host, login) {
                let output =
                    envelope_request(&plan, &control(Duration::from_secs(10)), None).unwrap();
                assert_selected_data(&output, &plan, login);
            }
        }
    }
    for (login, expected) in [
        ("Broken", AccountError::CredentialLookupFailed.into()),
        ("Store", AccountError::CredentialStoreUnavailable.into()),
        ("Wrong", AccountError::WrongHostOrAccount.into()),
        ("WrongAfter", AccountError::WrongHostOrAccount.into()),
        ("Leak", AccountExecutionError::SecretOutputRejected),
        ("LeakStderr", AccountExecutionError::SecretOutputRejected),
    ] {
        for plan in selected_reads("github.com", login) {
            let error =
                envelope_request(&plan, &control(Duration::from_secs(10)), None).unwrap_err();
            assert_eq!(error, expected);
        }
    }
}

#[cfg(unix)]
#[test]
fn host_envelope_cancellation_and_timeout_reap_credential_children() {
    for (login, cancel) in [
        ("PipeDescendant", false),
        ("Slow", false),
        ("Slow", true),
        ("LookupSlow", false),
        ("LookupSlow", true),
    ] {
        let evidence = DescendantEvidence::new();
        let cancellation = AccountCancellation::default();
        let success = login == "PipeDescendant";
        let control = AccountExecutionControl::new(
            Duration::from_secs(if success || cancel { 15 } else { 5 }),
            cancellation.clone(),
            None,
        )
        .unwrap();
        let canceller = cancel.then(|| evidence.cancel_after_start(cancellation));
        let result = envelope_request(&selected("github.com", login), &control, Some(&evidence));
        if let Some(canceller) = canceller {
            assert!(canceller.join().unwrap());
        }
        if success {
            assert!(result.is_ok());
        } else {
            assert_eq!(
                result.unwrap_err(),
                if cancel {
                    AccountExecutionError::Cancelled
                } else {
                    AccountExecutionError::TimedOut
                }
            );
        }
        evidence.assert_retired();
    }
}

#[test]
fn host_envelope_protocol_and_host_token_routing_fail_closed() {
    for (host, expected) in [
        ("github.com", "GH_TOKEN"),
        ("org.ghe.com", "GH_TOKEN"),
        ("ghe.com", "GH_ENTERPRISE_TOKEN"),
        ("notghe.com", "GH_ENTERPRISE_TOKEN"),
        ("github.com.example", "GH_ENTERPRISE_TOKEN"),
    ] {
        assert_eq!(auth_variable(host), expected);
    }
    for raw in [
        "{}",
        "null",
        "not-json",
        r#"{"status":"unknown","token":"fixture_credential_Alice"}"#,
    ] {
        assert_eq!(
            decode_host_reply(raw.as_bytes(), 64).unwrap_err(),
            AccountExecutionError::Protocol
        );
        assert!(!host_reply_confirms_cleanup(raw.as_bytes()));
    }
    assert_eq!(
        decode_host_reply(br#"{"status":"helper-unavailable"}"#, 64).unwrap_err(),
        AccountExecutionError::HostHelperUnavailable
    );
    assert!(!host_reply_confirms_cleanup(
        br#"{"status":"cleanup-failed"}"#
    ));
}

#[cfg(unix)]
pub(crate) fn exercise_ssh_fixture(
    connection: &mt_config::SshConnection,
    root: &Path,
    retire: impl Fn(),
) {
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let gh = bin.join("gh");
    std::fs::copy(&fixture().executable, &gh).unwrap();
    let directory = root.join(format!(
        "tasks ' $(never-executed) {}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let mut source = snapshot(&directory);
    source.backend = ExecutionBackend::Ssh {
        connection: connection.clone(),
        connection_fingerprint: crate::remote_ssh::connection_fingerprint(connection),
        connection_epoch: None,
    };
    // Refuse to call gh until the loopback host proves it resolves our shim.
    let resolved = crate::execution_host::execute_host_command(
        &source,
        &CommandPlan::new("/bin/sh", ["-c", "command -v gh"]),
        Duration::from_secs(15),
        4096,
    )
    .unwrap();
    assert_eq!(
        std::str::from_utf8(&resolved.output.stdout).unwrap().trim(),
        gh.to_str().unwrap()
    );
    let control = AccountExecutionControl::new(
        Duration::from_secs(15),
        AccountCancellation::default(),
        resolved.observed_connection_epoch,
    )
    .unwrap();
    for capability in [
        AccountCapability::AuthStatusJson,
        AccountCapability::NamedAccountLookup,
    ] {
        let result = probe_account_capability(&source, capability, &control);
        result.result.unwrap();
        assert_eq!(
            result.observed_connection_epoch,
            resolved.observed_connection_epoch
        );
    }
    let known = discover_accounts(&source, "github.com", &control);
    assert_eq!(
        known.observed_connection_epoch,
        resolved.observed_connection_epoch
    );
    let known = known.result.unwrap();
    assert_eq!(known.accounts().len(), 2);
    assert_eq!(
        known.accounts()[1].problem(),
        Some(AccountError::AuthenticationFailed)
    );
    assert!(!format!("{known:?}").contains("fixture_credential_"));
    for host in ["github.com", "org.ghe.com", "github.example.com"] {
        for login in ["Alice", "Bob", "Rotate"] {
            for plan in selected_reads(host, login) {
                let case = directory.join(uuid::Uuid::new_v4().to_string());
                std::fs::create_dir_all(&case).unwrap();
                let mut source = source.clone();
                source.canonical_path = case.to_str().unwrap().into();
                let result = execute_selected_account(&source, &plan, &control);
                assert_eq!(
                    result.observed_connection_epoch,
                    resolved.observed_connection_epoch
                );
                let output = result.result.unwrap();
                assert_selected_data(&output, &plan, login);
                assert!(!format!("{output:?}").contains("fixture_credential_"));
            }
        }
    }
    for (login, error) in [
        ("Broken", AccountError::CredentialLookupFailed.into()),
        ("Store", AccountError::CredentialStoreUnavailable.into()),
        ("Wrong", AccountError::WrongHostOrAccount.into()),
        ("WrongAfter", AccountError::WrongHostOrAccount.into()),
        ("Leak", AccountExecutionError::SecretOutputRejected),
        ("LeakStderr", AccountExecutionError::SecretOutputRejected),
    ] {
        for plan in selected_reads("github.com", login) {
            let case = directory.join(uuid::Uuid::new_v4().to_string());
            std::fs::create_dir_all(&case).unwrap();
            let mut source = source.clone();
            source.canonical_path = case.to_str().unwrap().into();
            let result = execute_selected_account(&source, &plan, &control);
            assert_eq!(
                result.observed_connection_epoch,
                resolved.observed_connection_epoch
            );
            assert_eq!(result.result.unwrap_err(), error);
        }
    }

    let evidence = DescendantEvidence::in_directory(&directory);
    let plan = selected("github.com", "Slow");
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| execute_selected_account(&source, &plan, &control));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !evidence.ready.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            evidence.ready.exists(),
            "SSH credential descendant never started"
        );
        retire();
        std::fs::write(directory.join("finish"), b"finish").unwrap();
        let result = worker.join().unwrap();
        assert_eq!(
            result.result.unwrap_err(),
            AccountExecutionError::ContextChanged
        );
        assert_eq!(
            result.observed_connection_epoch,
            resolved.observed_connection_epoch
        );
    });
    evidence.assert_retired();
    let replaced = discover_accounts(&source, "github.com", &control);
    assert_eq!(
        replaced.result.unwrap_err(),
        AccountExecutionError::ContextChanged
    );
    assert!(replaced.observed_connection_epoch.is_some());
    assert_ne!(
        replaced.observed_connection_epoch,
        resolved.observed_connection_epoch
    );

    for (login, cancel) in [("Slow", false), ("Slow", true), ("LookupSlow", true)] {
        let case = directory.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&case).unwrap();
        let mut source = source.clone();
        source.canonical_path = case.to_str().unwrap().into();
        let evidence = DescendantEvidence::in_directory(&case);
        let cancellation = AccountCancellation::default();
        let control = AccountExecutionControl::new(
            Duration::from_secs(if cancel { 15 } else { 5 }),
            cancellation.clone(),
            None,
        )
        .unwrap();
        let canceller = cancel.then(|| evidence.cancel_after_start(cancellation));
        let result = execute_selected_account(&source, &selected("github.com", login), &control);
        if let Some(canceller) = canceller {
            assert!(canceller.join().unwrap());
        }
        assert!(result.observed_connection_epoch.is_some());
        assert_eq!(
            result.result.unwrap_err(),
            if cancel {
                AccountExecutionError::Cancelled
            } else {
                AccountExecutionError::TimedOut
            }
        );
        evidence.assert_retired();
    }
    retire();
}

#[cfg(windows)]
#[derive(Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct WslFixtureOwner {
    schema: u64,
    kind: String,
    run_id: String,
    run_attempt: String,
    repository: String,
    sha: String,
}

#[cfg(windows)]
const WSL_FIXTURE_OUTPUT_CAP: usize = 4096;

#[cfg(windows)]
#[derive(Clone, Copy, Debug)]
enum WslPreludeStage {
    ReadOwner,
    ResolveGh,
    ResolvePython,
    VerifyGhHash,
    CheckCasesDirectory,
    CapturedCwd,
    LiteralArgv,
    MissingCwd,
    NonDirectoryCwd,
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug)]
enum WslCwdProgram {
    Absolute,
    Path,
    Relative,
}

#[cfg(windows)]
const WSL_LITERAL_ARGUMENTS: [&str; 8] = [
    r"%s\0",
    "literal '\";$(printf injected)",
    "-n",
    "--",
    "",
    "NAME=value",
    "*",
    "line one\nline two",
];

#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WslArgvDiscriminator {
    Ascii,
    AsciiNul,
    Hostile,
    Dash,
    DoubleDash,
    Empty,
    Assignment,
    Wildcard,
    Newline,
    WithoutEmpty,
}

#[cfg(windows)]
const WSL_ARGV_DISCRIMINATORS: [WslArgvDiscriminator; 10] = [
    WslArgvDiscriminator::Ascii,
    WslArgvDiscriminator::AsciiNul,
    WslArgvDiscriminator::Hostile,
    WslArgvDiscriminator::Dash,
    WslArgvDiscriminator::DoubleDash,
    WslArgvDiscriminator::Empty,
    WslArgvDiscriminator::Assignment,
    WslArgvDiscriminator::Wildcard,
    WslArgvDiscriminator::Newline,
    WslArgvDiscriminator::WithoutEmpty,
];

#[cfg(windows)]
fn wsl_argv_discriminator_plan(row: WslArgvDiscriminator) -> (CommandPlan, Vec<u8>) {
    let (nul_terminated, data) = match row {
        WslArgvDiscriminator::Ascii => (false, vec!["synthetic-ascii"]),
        WslArgvDiscriminator::AsciiNul => (true, vec!["synthetic-ascii"]),
        WslArgvDiscriminator::Hostile => (false, vec![WSL_LITERAL_ARGUMENTS[1]]),
        WslArgvDiscriminator::Dash => (false, vec![WSL_LITERAL_ARGUMENTS[2]]),
        WslArgvDiscriminator::DoubleDash => (false, vec![WSL_LITERAL_ARGUMENTS[3]]),
        WslArgvDiscriminator::Empty => (false, vec![WSL_LITERAL_ARGUMENTS[4]]),
        WslArgvDiscriminator::Assignment => (false, vec![WSL_LITERAL_ARGUMENTS[5]]),
        WslArgvDiscriminator::Wildcard => (false, vec![WSL_LITERAL_ARGUMENTS[6]]),
        WslArgvDiscriminator::Newline => (false, vec![WSL_LITERAL_ARGUMENTS[7]]),
        WslArgvDiscriminator::WithoutEmpty => (
            true,
            WSL_LITERAL_ARGUMENTS[1..]
                .iter()
                .copied()
                .filter(|arg| !arg.is_empty())
                .collect(),
        ),
    };
    let mut expected = Vec::new();
    for arg in &data {
        expected.extend_from_slice(arg.as_bytes());
        if nul_terminated {
            expected.push(0);
        }
    }
    let format = if nul_terminated {
        WSL_LITERAL_ARGUMENTS[0]
    } else {
        "%s"
    };
    (
        CommandPlan::new("/usr/bin/printf", std::iter::once(format).chain(data)),
        expected,
    )
}

#[cfg(windows)]
fn wsl_prelude_output_class(bytes: &[u8]) -> &'static str {
    if bytes.is_empty() {
        return "empty";
    }
    if bytes.len() > WSL_FIXTURE_OUTPUT_CAP {
        return "over-limit";
    }
    let text = if bytes.contains(&0) {
        let mut pairs = bytes.chunks_exact(2);
        let units = pairs
            .by_ref()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        if !pairs.remainder().is_empty() {
            return "non-text";
        }
        let Ok(text) = String::from_utf16(&units) else {
            return "non-text";
        };
        text
    } else {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return "non-text";
        };
        text.to_string()
    };
    let text = text.to_ascii_lowercase();
    match text.trim_start_matches('\u{feff}').trim() {
        "the system cannot find the file specified." => return "windows-file-not-found",
        "the system cannot find the path specified." => return "windows-path-not-found",
        "the parameter is incorrect." => return "windows-invalid-parameter",
        _ => {}
    }
    // Return fixed categories only, never any part of a command's output.
    for (needle, class) in [
        ("error_invalid_handle", "invalid-handle"),
        ("the handle is invalid", "invalid-handle"),
        ("e_accessdenied", "access-denied"),
        ("access is denied", "access-denied"),
        ("permission denied", "access-denied"),
        ("wsl_e_distro_not_found", "distro-not-found"),
        ("hcs_e_service_not_available", "service-unavailable"),
        ("failed to translate", "path-translation"),
        ("chdir(", "cwd-failed"),
        ("execvpe(", "exec-failed"),
        ("error_file_not_found", "file-not-found"),
        ("no such file or directory", "file-not-found"),
        ("error_path_not_found", "path-not-found"),
        ("not supported", "unsupported"),
        ("wsl/", "wsl-error"),
    ] {
        if text.contains(needle) {
            return class;
        }
    }
    "unclassified"
}

#[cfg(windows)]
fn wsl_prelude_diagnostic(stage: WslPreludeStage, output: &CommandOutput) -> String {
    let exit_hex = output
        .exit_code
        .map(|code| format!("0x{:08x}", code as u32))
        .unwrap_or_else(|| "none".into());
    format!(
        "stage={stage:?} exit={:?} exit_hex={exit_hex} timed_out={} stdout_truncated={} stderr_truncated={} stdout_bytes={} stdout_class={} stderr_bytes={} stderr_class={}",
        output.exit_code,
        output.timed_out,
        output.stdout_truncated,
        output.stderr_truncated,
        output.stdout.len(),
        wsl_prelude_output_class(&output.stdout),
        output.stderr.len(),
        wsl_prelude_output_class(&output.stderr),
    )
}

#[cfg(windows)]
fn wsl_cwd_diagnostic(
    pre_project: bool,
    program: WslCwdProgram,
    stage: WslPreludeStage,
    output: &CommandOutput,
) -> String {
    let mode = if pre_project { "PreProject" } else { "Project" };
    format!(
        "mode={mode} program_kind={program:?} {}",
        wsl_prelude_diagnostic(stage, output),
    )
}

#[cfg(windows)]
fn wsl_literal_result_matches(
    result: &Result<CommandOutput, CommandExecutionError>,
    expected: &[u8],
) -> bool {
    result.as_ref().is_ok_and(|output| {
        output.exit_code == Some(0)
            && !output.timed_out
            && !output.stdout_truncated
            && !output.stderr_truncated
            && output.stdout.len() <= WSL_FIXTURE_OUTPUT_CAP
            && output.stderr.len() <= WSL_FIXTURE_OUTPUT_CAP
            && output.stdout == expected
    })
}

#[cfg(windows)]
fn wsl_literal_result_diagnostic(
    pre_project: bool,
    program: WslCwdProgram,
    result: &Result<CommandOutput, CommandExecutionError>,
    expected: &[u8],
) -> String {
    let diagnostic = match result {
        Ok(output) => wsl_cwd_diagnostic(pre_project, program, WslPreludeStage::LiteralArgv, output),
        Err(error) => {
            let mode = if pre_project { "PreProject" } else { "Project" };
            format!(
                "mode={mode} program_kind={program:?} stage=LiteralArgv dispatch_kind={:?}",
                error.kind,
            )
        }
    };
    format!(
        "{diagnostic} output_matches={}",
        wsl_literal_result_matches(result, expected),
    )
}

#[cfg(windows)]
fn wsl_require_literal_argv(
    pre_project: bool,
    program: WslCwdProgram,
    baseline: Result<CommandOutput, CommandExecutionError>,
    expected: &[u8],
    mut probe: impl FnMut(WslArgvDiscriminator) -> Result<CommandOutput, CommandExecutionError>,
) -> Result<(), String> {
    if wsl_literal_result_matches(&baseline, expected) {
        return Ok(());
    }
    let mut rows = vec![format!(
        "baseline {}",
        wsl_literal_result_diagnostic(pre_project, program, &baseline, expected),
    )];
    for row in WSL_ARGV_DISCRIMINATORS {
        let (_, expected) = wsl_argv_discriminator_plan(row);
        let result = probe(row);
        rows.push(format!(
            "probe={row:?} {}",
            wsl_literal_result_diagnostic(pre_project, WslCwdProgram::Absolute, &result, &expected),
        ));
    }
    Err(format!(
        "Actions WSL original literal-argv baseline rejected\n{}",
        rows.join("\n"),
    ))
}

#[cfg(windows)]
type WslMarkerProbeRow = (TasksWslProbeCwd, TasksWslProbeUser, TasksWslProbeStdin);

#[cfg(windows)]
const WSL_MARKER_PROBE_ROWS: [WslMarkerProbeRow; 8] = [
    (
        TasksWslProbeCwd::Captured,
        TasksWslProbeUser::Default,
        TasksWslProbeStdin::Null,
    ),
    (
        TasksWslProbeCwd::Captured,
        TasksWslProbeUser::Default,
        TasksWslProbeStdin::ClosedPipe,
    ),
    (
        TasksWslProbeCwd::Root,
        TasksWslProbeUser::Default,
        TasksWslProbeStdin::Null,
    ),
    (
        TasksWslProbeCwd::Root,
        TasksWslProbeUser::Default,
        TasksWslProbeStdin::ClosedPipe,
    ),
    (
        TasksWslProbeCwd::Captured,
        TasksWslProbeUser::Root,
        TasksWslProbeStdin::Null,
    ),
    (
        TasksWslProbeCwd::Captured,
        TasksWslProbeUser::Root,
        TasksWslProbeStdin::ClosedPipe,
    ),
    (
        TasksWslProbeCwd::Root,
        TasksWslProbeUser::Root,
        TasksWslProbeStdin::Null,
    ),
    (
        TasksWslProbeCwd::Root,
        TasksWslProbeUser::Root,
        TasksWslProbeStdin::ClosedPipe,
    ),
];

#[cfg(windows)]
fn wsl_marker_matches(result: &HostCommandResult, expected: &WslFixtureOwner) -> bool {
    let output = &result.output;
    result.observed_connection_epoch.is_none()
        && output.exit_code == Some(0)
        && !output.timed_out
        && !output.stdout_truncated
        && !output.stderr_truncated
        && output.stdout.len() <= WSL_FIXTURE_OUTPUT_CAP
        && output.stderr.len() <= WSL_FIXTURE_OUTPUT_CAP
        && serde_json::from_slice::<WslFixtureOwner>(&output.stdout)
            .is_ok_and(|owner| owner == *expected)
}

#[cfg(windows)]
fn wsl_marker_probe_diagnostic(
    (cwd, user, stdin): WslMarkerProbeRow,
    result: &Result<HostCommandResult, CommandExecutionError>,
    expected: &WslFixtureOwner,
) -> String {
    let row = format!("row={cwd:?}/{user:?}/{stdin:?}");
    match result {
        Ok(result) => format!(
            "{row} {} epoch_present={} marker_matches={}",
            wsl_prelude_diagnostic(WslPreludeStage::ReadOwner, &result.output),
            result.observed_connection_epoch.is_some(),
            wsl_marker_matches(result, expected),
        ),
        Err(error) => format!(
            "{row} stage=ReadOwner dispatch_kind={:?} marker_matches=false",
            error.kind,
        ),
    }
}

#[cfg(windows)]
fn wsl_require_owner_marker(
    baseline: Result<HostCommandResult, CommandExecutionError>,
    expected: &WslFixtureOwner,
    mut probe: impl FnMut(
        TasksWslProbeCwd,
        TasksWslProbeUser,
        TasksWslProbeStdin,
    ) -> Result<HostCommandResult, CommandExecutionError>,
) -> Result<(), String> {
    if baseline
        .as_ref()
        .is_ok_and(|result| wsl_marker_matches(result, expected))
    {
        return Ok(());
    }
    // The first row is the original production launch, never a diagnostic retry.
    let mut rows = vec![wsl_marker_probe_diagnostic(
        WSL_MARKER_PROBE_ROWS[0],
        &baseline,
        expected,
    )];
    for &(cwd, user, stdin) in &WSL_MARKER_PROBE_ROWS[1..] {
        rows.push(wsl_marker_probe_diagnostic(
            (cwd, user, stdin),
            &probe(cwd, user, stdin),
            expected,
        ));
    }
    Err(format!(
        "Actions WSL pre-auth original ReadOwner baseline rejected\n{}",
        rows.join("\n")
    ))
}

#[cfg(windows)]
#[derive(Clone)]
struct WslFixture {
    source: ProjectExecutionSnapshot,
}

#[cfg(windows)]
impl WslFixture {
    fn open() -> Self {
        assert!(
            std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true"),
            "Actions-only fixture"
        );
        let required =
            |key| std::env::var(key).unwrap_or_else(|_| panic!("missing WSL fixture input: {key}"));
        let run_id = required("GITHUB_RUN_ID");
        let run_attempt = required("GITHUB_RUN_ATTEMPT");
        for value in [&run_id, &run_attempt] {
            assert!(
                !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()),
                "invalid Actions run identity"
            );
        }
        let repository = required("GITHUB_REPOSITORY");
        assert!(
            !repository.is_empty(),
            "missing Actions repository identity"
        );
        let sha = required("GITHUB_SHA");
        assert!(
            sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "invalid Actions commit identity"
        );
        let distro = required("MT_TEST_WSL_DISTRO");
        assert!(
            distro == format!("mt-tasks-{run_id}-{run_attempt}"),
            "refusing a non-owned distro"
        );
        let marker = required("MT_TEST_WSL_MARKER");
        assert!(
            marker == "/mini-term-fixture/owner.json",
            "refusing a non-fixture owner marker path"
        );
        let gh_hash = required("MT_TEST_WSL_GH_SHA256");
        assert!(
            gh_hash.len() == 64
                && gh_hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "invalid fixture ELF checksum"
        );
        let mut source = snapshot(Path::new("/mini-term-fixture"));
        source.backend = ExecutionBackend::Wsl { distro };
        let fixture = Self { source };
        // These bounded fixture commands read only the explicitly named distro.
        // No account API is called until both provenance and the executable match.
        let expected_owner = WslFixtureOwner {
            schema: 1,
            kind: "mini-term-tasks-wsl".into(),
            run_id,
            run_attempt,
            repository,
            sha,
        };
        wsl_require_owner_marker(
            fixture.run_command("/bin/cat", &[&marker]),
            &expected_owner,
            |cwd, user, stdin| tasks_wsl_marker_probe(&fixture.source, cwd, user, stdin),
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        for (stage, command, expected) in [
            (
                WslPreludeStage::ResolveGh,
                "command -v gh",
                "/usr/local/bin/gh\n",
            ),
            (
                WslPreludeStage::ResolvePython,
                "command -v python3",
                "/usr/local/bin/python3\n",
            ),
        ] {
            let output = fixture.pre_auth_command(stage, "/bin/sh", &["-c", command]);
            assert!(
                output.stdout == expected.as_bytes(),
                "WSL resolved a non-fixture executable at {stage:?}"
            );
        }
        let output = fixture.pre_auth_command(
            WslPreludeStage::VerifyGhHash,
            "/usr/bin/sha256sum",
            &["/usr/local/bin/gh"],
        );
        assert!(
            output.stdout == format!("{gh_hash}  /usr/local/bin/gh\n").as_bytes(),
            "WSL fixture ELF checksum mismatch"
        );
        fixture.pre_auth_command(
            WslPreludeStage::CheckCasesDirectory,
            "/usr/bin/test",
            &["-d", "/mini-term-fixture/cases"],
        );
        fixture
    }

    fn run_command(
        &self,
        program: &str,
        args: &[&str],
    ) -> Result<crate::execution_host::HostCommandResult, mt_github::CommandExecutionError> {
        crate::execution_host::execute_host_command(
            &self.source,
            &CommandPlan::new(program, args.iter().copied()),
            Duration::from_secs(5),
            WSL_FIXTURE_OUTPUT_CAP,
        )
    }

    fn pre_auth_command(
        &self,
        stage: WslPreludeStage,
        program: &str,
        args: &[&str],
    ) -> CommandOutput {
        let result = self.run_command(program, args).unwrap_or_else(|error| {
            panic!(
                "Actions WSL pre-auth stage={stage:?} dispatch_kind={:?}",
                error.kind
            )
        });
        let output = result.output;
        assert!(
            result.observed_connection_epoch.is_none()
                && output.exit_code == Some(0)
                && !output.timed_out
                && !output.stdout_truncated
                && !output.stderr_truncated,
            "Actions WSL pre-auth command failed: {}",
            wsl_prelude_diagnostic(stage, &output)
        );
        output
    }

    fn command(&self, program: &str, args: &[&str]) -> CommandOutput {
        let result = self
            .run_command(program, args)
            .unwrap_or_else(|_| panic!("Actions WSL fixture command could not run"));
        assert!(result.observed_connection_epoch.is_none());
        assert!(
            !result.output.timed_out
                && !result.output.stdout_truncated
                && !result.output.stderr_truncated,
            "incomplete WSL fixture command output"
        );
        result.output
    }

    fn require_success(&self, stage: &'static str, output: &CommandOutput) {
        assert!(
            output.exit_code == Some(0),
            "Actions WSL fixture stage={stage} exit={:?}",
            output.exit_code
        );
    }

    fn case(&self) -> Self {
        let path = format!("/mini-term-fixture/cases/{}", uuid::Uuid::new_v4());
        let output = self.command("/bin/mkdir", &["--", &path]);
        self.require_success("create-case", &output);
        let mut source = self.source.clone();
        source.canonical_path = path;
        Self { source }
    }

    fn assert_cwd_routing(&self) -> Self {
        let case = self.case();
        let path = format!(
            "{}/cwd space '\";$(printf injected) [literal]\nnext",
            case.source.canonical_path
        );
        let output = case.command("/bin/mkdir", &["--", &path]);
        case.require_success("create-literal-cwd", &output);
        let relative = "./-relative '\";$(printf injected)";
        let executable = format!("{path}/{}", &relative[2..]);
        let output = case.command("/bin/ln", &["-s", "--", "/usr/bin/printf", &executable]);
        case.require_success("create-relative-executable", &output);
        case.touch("not-directory");
        let mut captured = case.clone();
        captured.source.canonical_path = path;

        for pre_project in [false, true] {
            let dispatch = |source: &ProjectExecutionSnapshot,
                            plan: &CommandPlan,
                            stage: WslPreludeStage,
                            program: WslCwdProgram| {
                if pre_project {
                    let ExecutionBackend::Wsl { distro } = &source.backend else {
                        panic!("expected the owned WSL fixture")
                    };
                    crate::execution_host::execute_pre_project_local_command(
                        &crate::execution_host::PreProjectLocalContext::Wsl {
                            distro: distro.clone(),
                            cwd: source.canonical_path.clone(),
                        },
                        plan,
                        Duration::from_secs(5),
                        WSL_FIXTURE_OUTPUT_CAP,
                    )
                } else {
                    crate::execution_host::execute_host_command(
                        source,
                        plan,
                        Duration::from_secs(5),
                        WSL_FIXTURE_OUTPUT_CAP,
                    )
                    .map(|result| {
                        assert!(
                            result.observed_connection_epoch.is_none(),
                            "Actions WSL cwd assertion observed an epoch: {}",
                            wsl_cwd_diagnostic(pre_project, program, stage, &result.output)
                        );
                        result.output
                    })
                }
            };
            let run = |source: &ProjectExecutionSnapshot,
                       plan: &CommandPlan,
                       stage: WslPreludeStage,
                       program: WslCwdProgram| {
                let output = dispatch(source, plan, stage, program).unwrap_or_else(|error| {
                    let mode = if pre_project { "PreProject" } else { "Project" };
                    panic!(
                        "Actions WSL cwd assertion mode={mode} program_kind={program:?} stage={stage:?} dispatch_kind={:?}",
                        error.kind
                    )
                });
                assert!(
                    !output.timed_out && !output.stdout_truncated && !output.stderr_truncated,
                    "Actions WSL cwd assertion output was incomplete: {}",
                    wsl_cwd_diagnostic(pre_project, program, stage, &output)
                );
                output
            };
            let output = run(
                &captured.source,
                &CommandPlan::new("/bin/pwd", ["-P"]),
                WslPreludeStage::CapturedCwd,
                WslCwdProgram::Absolute,
            );
            assert!(
                output.exit_code == Some(0)
                    && output.stdout == format!("{}\n", captured.source.canonical_path).as_bytes(),
                "WSL command did not enter its exact captured directory: {}",
                wsl_cwd_diagnostic(
                    pre_project,
                    WslCwdProgram::Absolute,
                    WslPreludeStage::CapturedCwd,
                    &output,
                )
            );
            let mut expected = Vec::new();
            for arg in &WSL_LITERAL_ARGUMENTS[1..] {
                expected.extend_from_slice(arg.as_bytes());
                expected.push(0);
            }
            for (program_kind, program) in [
                (WslCwdProgram::Absolute, "/usr/bin/printf"),
                (WslCwdProgram::Path, "printf"),
                (WslCwdProgram::Relative, relative),
            ] {
                let baseline = dispatch(
                    &captured.source,
                    &CommandPlan::new(program, WSL_LITERAL_ARGUMENTS),
                    WslPreludeStage::LiteralArgv,
                    program_kind,
                );
                wsl_require_literal_argv(pre_project, program_kind, baseline, &expected, |row| {
                    let (plan, _) = wsl_argv_discriminator_plan(row);
                    dispatch(
                        &captured.source,
                        &plan,
                        WslPreludeStage::LiteralArgv,
                        WslCwdProgram::Absolute,
                    )
                })
                .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
            }
            let marker = if pre_project {
                "preproject-dispatched"
            } else {
                "project-dispatched"
            };
            for (stage, path) in [
                (
                    WslPreludeStage::MissingCwd,
                    format!("{}/missing", captured.source.canonical_path),
                ),
                (WslPreludeStage::NonDirectoryCwd, case.path("not-directory")),
            ] {
                let mut invalid = captured.source.clone();
                invalid.canonical_path = path;
                let marker_path = case.path(marker);
                let output = run(
                    &invalid,
                    &CommandPlan::new("/usr/bin/touch", ["--", &marker_path]),
                    stage,
                    WslCwdProgram::Absolute,
                );
                assert!(
                    output.exit_code.is_some_and(|code| code != 0),
                    "invalid WSL cwd did not fail: {}",
                    wsl_cwd_diagnostic(pre_project, WslCwdProgram::Absolute, stage, &output)
                );
                assert!(
                    !case.exists(marker),
                    "invalid WSL cwd dispatched its target: {}",
                    wsl_cwd_diagnostic(pre_project, WslCwdProgram::Absolute, stage, &output)
                );
            }
        }
        captured
    }

    fn path(&self, name: &str) -> String {
        assert!(
            name.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        );
        format!("{}/{name}", self.source.canonical_path)
    }

    fn touch(&self, name: &str) {
        let output = self.command("/usr/bin/touch", &["--", &self.path(name)]);
        self.require_success("touch-marker", &output);
    }

    fn exists(&self, name: &str) -> bool {
        let output = self.command("/usr/bin/test", &["-f", &self.path(name)]);
        assert!(
            matches!(output.exit_code, Some(0 | 1)),
            "WSL marker presence is indeterminate"
        );
        output.exit_code == Some(0)
    }

    fn cancel_after_start(
        &self,
        cancellation: AccountCancellation,
    ) -> std::thread::JoinHandle<bool> {
        let fixture = self.clone();
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let ready = fixture.exists("ready");
                if ready || Instant::now() >= deadline {
                    cancellation.cancel();
                    return ready;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        })
    }

    fn assert_retired(&self) {
        assert!(
            self.exists("ready"),
            "WSL cleanup fixture never started its descendant"
        );
        self.touch("release");
        std::thread::sleep(Duration::from_secs(1));
        assert!(
            !self.exists("marker"),
            "WSL request left a live Linux descendant"
        );
    }
}

#[cfg(windows)]
#[test]
fn wsl_prelude_diagnostics_are_bounded_stage_specific_and_secret_safe() {
    let mut output = CommandOutput {
        stdout: br#"{"token":"fixture_credential_stdout"}"#.to_vec(),
        stderr: "The handle is invalid. fixture_credential_stderr"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect(),
        exit_code: Some(-1),
        ..Default::default()
    };
    for stage in [
        WslPreludeStage::ReadOwner,
        WslPreludeStage::ResolveGh,
        WslPreludeStage::ResolvePython,
        WslPreludeStage::VerifyGhHash,
        WslPreludeStage::CheckCasesDirectory,
    ] {
        let diagnostic = wsl_prelude_diagnostic(stage, &output);
        assert!(diagnostic.starts_with(&format!("stage={stage:?} ")));
        assert!(diagnostic.contains("exit=Some(-1) exit_hex=0xffffffff"));
        assert!(diagnostic.contains("stdout_class=unclassified"));
        assert!(diagnostic.contains("stderr_class=invalid-handle"));
        assert!(!diagnostic.contains("fixture_credential_"));
        assert!(diagnostic.len() <= 512);
    }
    for (text, class) in [
        ("chdir(/fixture_credential_path) failed 2", "cwd-failed"),
        (
            "Error code: Wsl/Service/E_ACCESSDENIED fixture_credential_error",
            "access-denied",
        ),
        (
            "Failed to translate fixture_credential_path",
            "path-translation",
        ),
        (
            "No such file or directory: fixture_credential_path",
            "file-not-found",
        ),
    ] {
        assert_eq!(wsl_prelude_output_class(text.as_bytes()), class);
    }
    assert_eq!(wsl_prelude_output_class(&[]), "empty");
    assert_eq!(wsl_prelude_output_class(&[0xff]), "non-text");
    assert_eq!(wsl_prelude_output_class(&[0x00, 0xd8]), "non-text");
    assert_eq!(
        wsl_prelude_output_class(&vec![b'x'; WSL_FIXTURE_OUTPUT_CAP + 1]),
        "over-limit"
    );
    output.timed_out = true;
    output.stdout_truncated = true;
    output.stderr_truncated = true;
    let diagnostic = wsl_prelude_diagnostic(WslPreludeStage::ReadOwner, &output);
    assert!(diagnostic.contains("timed_out=true stdout_truncated=true stderr_truncated=true"));
    assert!(!diagnostic.contains("fixture_credential_"));
}

#[cfg(windows)]
#[test]
fn wsl_cwd_diagnostics_identify_route_program_and_stage_without_raw_output() {
    let output = CommandOutput {
        stdout: "The parameter is incorrect.\r\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect(),
        stderr: b"fixture_credential_stderr".to_vec(),
        exit_code: Some(-1),
        ..Default::default()
    };
    for (pre_project, mode) in [(false, "Project"), (true, "PreProject")] {
        for (program, kind) in [
            (WslCwdProgram::Absolute, "Absolute"),
            (WslCwdProgram::Path, "Path"),
            (WslCwdProgram::Relative, "Relative"),
        ] {
            for stage in [
                WslPreludeStage::CapturedCwd,
                WslPreludeStage::LiteralArgv,
                WslPreludeStage::MissingCwd,
                WslPreludeStage::NonDirectoryCwd,
            ] {
                let diagnostic = wsl_cwd_diagnostic(pre_project, program, stage, &output);
                assert!(diagnostic.starts_with(&format!(
                    "mode={mode} program_kind={kind} stage={stage:?} "
                )));
                assert!(diagnostic.contains("exit=Some(-1) exit_hex=0xffffffff"));
                assert!(diagnostic.contains("stdout_class=windows-invalid-parameter"));
                assert!(diagnostic.contains("stderr_class=unclassified"));
                assert!(!diagnostic.contains("fixture_credential_"));
                assert!(!diagnostic.contains("The parameter is incorrect"));
                assert!(diagnostic.len() < 640);
            }
        }
    }
}

#[cfg(windows)]
#[test]
fn wsl_literal_discriminator_plans_are_fixed_and_preserve_edge_cases() {
    for (row, value) in [
        (WslArgvDiscriminator::Ascii, "synthetic-ascii"),
        (WslArgvDiscriminator::Hostile, "literal '\";$(printf injected)"),
        (WslArgvDiscriminator::Dash, "-n"),
        (WslArgvDiscriminator::DoubleDash, "--"),
        (WslArgvDiscriminator::Empty, ""),
        (WslArgvDiscriminator::Assignment, "NAME=value"),
        (WslArgvDiscriminator::Wildcard, "*"),
        (WslArgvDiscriminator::Newline, "line one\nline two"),
    ] {
        let (plan, expected) = wsl_argv_discriminator_plan(row);
        assert_eq!(plan, CommandPlan::new("/usr/bin/printf", ["%s", value]));
        assert_eq!(expected, value.as_bytes());
    }
    let (plan, expected) = wsl_argv_discriminator_plan(WslArgvDiscriminator::AsciiNul);
    assert_eq!(
        plan,
        CommandPlan::new("/usr/bin/printf", [r"%s\0", "synthetic-ascii"])
    );
    assert_eq!(expected, b"synthetic-ascii\0");
    let (plan, expected) = wsl_argv_discriminator_plan(WslArgvDiscriminator::WithoutEmpty);
    assert_eq!(
        plan,
        CommandPlan::new(
            "/usr/bin/printf",
            [
                r"%s\0",
                "literal '\";$(printf injected)",
                "-n",
                "--",
                "NAME=value",
                "*",
                "line one\nline two",
            ],
        )
    );
    assert_eq!(
        expected,
        b"literal '\";$(printf injected)\0-n\0--\0NAME=value\0*\0line one\nline two\0"
    );
    assert_eq!(WSL_LITERAL_ARGUMENTS[4], "");
    assert_eq!(WSL_LITERAL_ARGUMENTS[7], "line one\nline two");
}

#[cfg(windows)]
#[test]
fn wsl_literal_discriminators_never_run_on_success_or_adopt_alternatives() {
    let success = CommandOutput {
        stdout: b"expected".to_vec(),
        exit_code: Some(0),
        ..Default::default()
    };
    wsl_require_literal_argv(true, WslCwdProgram::Path, Ok(success), b"expected", |_| {
        panic!("successful original literal command must not start probes")
    })
    .unwrap();
    let baseline = CommandOutput {
        stdout: b"fixture_credential_stdout".to_vec(),
        stderr: b"fixture_credential_stderr".to_vec(),
        exit_code: Some(-1),
        ..Default::default()
    };
    let mut probed = Vec::new();
    let diagnostic = wsl_require_literal_argv(
        false,
        WslCwdProgram::Relative,
        Ok(baseline),
        b"expected",
        |row| {
            probed.push(row);
            let (_, stdout) = wsl_argv_discriminator_plan(row);
            Ok(CommandOutput {
                stdout,
                exit_code: Some(0),
                ..Default::default()
            })
        },
    )
    .expect_err("probe successes cannot replace the original literal command failure");
    assert_eq!(probed.len(), 10);
    for row in WSL_ARGV_DISCRIMINATORS {
        assert_eq!(probed.iter().filter(|probed| **probed == row).count(), 1);
    }
    assert_eq!(diagnostic.lines().count(), 12);
    assert!(diagnostic.contains(
        "baseline mode=Project program_kind=Relative stage=LiteralArgv exit=Some(-1)"
    ));
    assert_eq!(diagnostic.matches("program_kind=Absolute").count(), 10);
    assert_eq!(diagnostic.matches("output_matches=true").count(), 10);
    assert_eq!(diagnostic.matches("output_matches=false").count(), 1);
    assert!(!diagnostic.contains("fixture_credential_"));
    assert!(!diagnostic.contains("synthetic-ascii"));
    assert!(diagnostic.len() < 8192);
}

#[cfg(windows)]
#[test]
fn wsl_literal_discriminators_reject_incomplete_mismatched_and_dispatch_failures() {
    let mut failures = Vec::new();
    for case in 0..7 {
        let mut output = CommandOutput {
            stdout: b"expected".to_vec(),
            exit_code: Some(0),
            ..Default::default()
        };
        match case {
            0 => output.stdout = b"fixture_credential_wrong_bytes".to_vec(),
            1 => output.exit_code = None,
            2 => output.timed_out = true,
            3 => output.stdout_truncated = true,
            4 => output.stderr_truncated = true,
            5 => output.stdout = vec![b'x'; WSL_FIXTURE_OUTPUT_CAP + 1],
            _ => output.stderr = vec![b'x'; WSL_FIXTURE_OUTPUT_CAP + 1],
        }
        failures.push(Ok(output));
    }
    for kind in [
        CommandExecutionErrorKind::ProgramNotFound,
        CommandExecutionErrorKind::Disconnected,
        CommandExecutionErrorKind::Rejected,
        CommandExecutionErrorKind::Io,
    ] {
        failures.push(Err(CommandExecutionError::new(
            kind,
            "fixture_credential_dispatch_error",
        )));
    }
    for baseline in failures {
        let mut probes = 0;
        let diagnostic = wsl_require_literal_argv(
            true,
            WslCwdProgram::Absolute,
            baseline,
            b"expected",
            |_| {
                probes += 1;
                Err(CommandExecutionError::new(
                    CommandExecutionErrorKind::Io,
                    "fixture_credential_probe_error",
                ))
            },
        )
        .unwrap_err();
        assert_eq!(probes, 10);
        assert_eq!(diagnostic.lines().count(), 12);
        assert_eq!(diagnostic.matches("output_matches=false").count(), 11);
        assert!(diagnostic.contains("mode=PreProject program_kind=Absolute stage=LiteralArgv"));
        assert!(!diagnostic.contains("fixture_credential_"));
        assert!(diagnostic.len() < 8192);
    }
}

#[cfg(windows)]
#[test]
fn wsl_prelude_system_messages_require_exact_text_not_length() {
    for (message, class) in [
        (
            "The system cannot find the file specified.",
            "windows-file-not-found",
        ),
        (
            "The system cannot find the path specified.",
            "windows-path-not-found",
        ),
        ("The parameter is incorrect.", "windows-invalid-parameter"),
    ] {
        for prefix in ["", "\u{feff}"] {
            let text = format!("{prefix}{message}\r\n");
            assert_eq!(wsl_prelude_output_class(text.as_bytes()), class);
            let bytes = text
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>();
            assert_eq!(wsl_prelude_output_class(&bytes), class);
            let output = CommandOutput {
                stdout: bytes,
                exit_code: Some(-1),
                ..Default::default()
            };
            let diagnostic = wsl_prelude_diagnostic(WslPreludeStage::ReadOwner, &output);
            assert!(diagnostic.contains(&format!("stdout_class={class}")));
            assert!(!diagnostic.contains(message));
        }
        let decorated = format!("{message} fixture_credential_suffix");
        assert_eq!(
            wsl_prelude_output_class(decorated.as_bytes()),
            "unclassified"
        );
    }
    let unrelated = [b'x', 0].repeat(45);
    assert_eq!(unrelated.len(), 90);
    assert_eq!(wsl_prelude_output_class(&unrelated), "unclassified");
}

#[cfg(windows)]
fn wsl_marker_test_inputs() -> (WslFixtureOwner, HostCommandResult) {
    let stdout = br#"{"schema":1,"kind":"mini-term-tasks-wsl","run_id":"12345","run_attempt":"2","repository":"fixture/repository","sha":"1111111111111111111111111111111111111111"}"#.to_vec();
    let expected = serde_json::from_slice(&stdout).unwrap();
    (
        expected,
        HostCommandResult {
            output: CommandOutput {
                stdout,
                exit_code: Some(0),
                ..Default::default()
            },
            observed_connection_epoch: None,
        },
    )
}

#[cfg(windows)]
#[test]
fn wsl_marker_matrix_reuses_failed_baseline_without_success_fallback() {
    let (expected, success) = wsl_marker_test_inputs();
    let mut baseline = success.clone();
    baseline.output.exit_code = Some(-1);
    baseline.output.stdout = b"fixture_credential_stdout".to_vec();
    baseline.output.stderr = b"fixture_credential_stderr".to_vec();
    let mut probed = Vec::new();
    let diagnostic = wsl_require_owner_marker(Ok(baseline), &expected, |cwd, user, stdin| {
        probed.push((cwd, user, stdin));
        Ok(success.clone())
    })
    .expect_err("alternate launch success must never replace a failed production baseline");
    assert_eq!(probed.len(), 7);
    for cwd in [TasksWslProbeCwd::Captured, TasksWslProbeCwd::Root] {
        for user in [TasksWslProbeUser::Default, TasksWslProbeUser::Root] {
            for stdin in [TasksWslProbeStdin::Null, TasksWslProbeStdin::ClosedPipe] {
                let baseline = cwd == TasksWslProbeCwd::Captured
                    && user == TasksWslProbeUser::Default
                    && stdin == TasksWslProbeStdin::Null;
                assert_eq!(
                    probed
                        .iter()
                        .filter(|row| **row == (cwd, user, stdin))
                        .count(),
                    if baseline { 0 } else { 1 }
                );
            }
        }
    }
    assert_eq!(diagnostic.lines().count(), 9);
    assert!(
        diagnostic.contains(
            "row=Captured/Default/Null stage=ReadOwner exit=Some(-1) exit_hex=0xffffffff"
        )
    );
    assert_eq!(diagnostic.matches("marker_matches=true").count(), 7);
    assert_eq!(diagnostic.matches("marker_matches=false").count(), 1);
    assert!(!diagnostic.contains("fixture_credential_"));
    assert!(!diagnostic.contains("fixture/repository"));
    assert!(diagnostic.len() < 4096);
}

#[cfg(windows)]
#[test]
fn wsl_marker_matrix_does_not_probe_after_valid_owner() {
    let (expected, success) = wsl_marker_test_inputs();
    wsl_require_owner_marker(Ok(success), &expected, |_, _, _| {
        panic!("a valid baseline must not start diagnostic launches")
    })
    .unwrap();
}

#[cfg(windows)]
#[test]
fn wsl_marker_matrix_rejects_unowned_incomplete_and_dispatch_failures() {
    let (expected, success) = wsl_marker_test_inputs();
    let mut failures = Vec::new();
    for (field, value) in [
        ("schema", serde_json::json!(2)),
        ("schema", serde_json::json!("1")),
        ("kind", serde_json::json!("fixture_credential_wrong_kind")),
        ("run_id", serde_json::json!("12346")),
        ("run_attempt", serde_json::json!("3")),
        (
            "repository",
            serde_json::json!("fixture_credential_repository"),
        ),
        ("sha", serde_json::json!("fixture_credential_sha")),
        ("unknown", serde_json::json!("fixture_credential_unknown")),
    ] {
        let mut marker: serde_json::Value = serde_json::from_slice(&success.output.stdout).unwrap();
        marker[field] = value;
        let mut invalid = success.clone();
        invalid.output.stdout = serde_json::to_vec(&marker).unwrap();
        failures.push(Ok(invalid));
    }
    let mut marker: serde_json::Value = serde_json::from_slice(&success.output.stdout).unwrap();
    marker.as_object_mut().unwrap().remove("sha");
    let mut invalid = success.clone();
    invalid.output.stdout = serde_json::to_vec(&marker).unwrap();
    failures.push(Ok(invalid));
    for case in 0..9 {
        let mut invalid = success.clone();
        match case {
            0 => invalid.output.exit_code = Some(-1),
            1 => invalid.output.exit_code = None,
            2 => invalid.output.timed_out = true,
            3 => invalid.output.stdout_truncated = true,
            4 => invalid.output.stderr_truncated = true,
            5 => invalid.observed_connection_epoch = Some(1),
            6 => invalid.output.stdout = vec![b'x'; WSL_FIXTURE_OUTPUT_CAP + 1],
            7 => invalid.output.stderr = vec![b'x'; WSL_FIXTURE_OUTPUT_CAP + 1],
            _ => invalid.output.stdout = b"fixture_credential_invalid_json".to_vec(),
        }
        failures.push(Ok(invalid));
    }
    for kind in [
        CommandExecutionErrorKind::ProgramNotFound,
        CommandExecutionErrorKind::Disconnected,
        CommandExecutionErrorKind::Rejected,
        CommandExecutionErrorKind::Io,
    ] {
        failures.push(Err(CommandExecutionError::new(
            kind,
            "fixture_credential_dispatch_error",
        )));
    }
    for baseline in failures {
        let mut probes = 0;
        let diagnostic = wsl_require_owner_marker(baseline, &expected, |_, _, _| {
            probes += 1;
            Err(CommandExecutionError::new(
                CommandExecutionErrorKind::Io,
                "fixture_credential_probe_error",
            ))
        })
        .unwrap_err();
        assert_eq!(probes, 7);
        assert_eq!(diagnostic.lines().count(), 9);
        assert_eq!(diagnostic.matches("marker_matches=false").count(), 8);
        assert!(!diagnostic.contains("fixture_credential_"));
        assert!(diagnostic.len() < 4096);
    }
}

#[cfg(windows)]
#[test]
#[ignore = "requires the same-run Actions-owned WSL distro/rootfs/marker and Linux gh fixture"]
fn tasks_account_executor_wsl_sentinels_cleanup_and_foreground_host() {
    let fixture = WslFixture::open();
    let cwd_case = fixture.assert_cwd_routing();
    let source = cwd_case.source.clone();
    let bounded = control(Duration::from_secs(30));
    for capability in [
        AccountCapability::AuthStatusJson,
        AccountCapability::NamedAccountLookup,
    ] {
        let result = probe_account_capability(&source, capability, &bounded);
        assert!(result.observed_connection_epoch.is_none());
        result.result.unwrap();
    }
    let discovered = discover_accounts(&source, "github.com", &bounded);
    assert!(discovered.observed_connection_epoch.is_none());
    let discovered = discovered.result.unwrap();
    assert_eq!(discovered.accounts().len(), 2);
    assert_eq!(
        discovered.accounts()[1].problem(),
        Some(AccountError::AuthenticationFailed)
    );
    assert!(!format!("{discovered:?}").contains("fixture_credential_"));

    let [plan, _] = selected_reads("github.com", "Rotate");
    let result = execute_selected_account(&source, &plan, &bounded);
    assert!(result.observed_connection_epoch.is_none());
    let output = result
        .result
        .unwrap_or_else(|error| panic!("WSL captured-cwd account request failed: {error:?}"));
    assert_selected_data(&output, &plan, "Rotate");
    assert!(
        cwd_case.exists("data-seen"),
        "WSL account envelope did not use its captured directory"
    );
    let mut missing = source.clone();
    missing.canonical_path.push_str("/missing");
    let result = execute_selected_account(&missing, &selected("github.com", "Alice"), &bounded);
    assert!(result.observed_connection_epoch.is_none());
    assert!(
        matches!(
            result.result,
            Err(AccountExecutionError::Account(AccountError::CommandFailed))
        ),
        "missing WSL cwd did not fail before account execution"
    );

    for host in ["github.com", "org.ghe.com", "github.example.com"] {
        for login in ["Alice", "Bob", "Rotate"] {
            for plan in selected_reads(host, login) {
                let case = fixture.case();
                let result = execute_selected_account(&case.source, &plan, &bounded);
                assert!(result.observed_connection_epoch.is_none());
                let output = result.result.unwrap();
                assert_selected_data(&output, &plan, login);
                assert!(!format!("{output:?}").contains("fixture_credential_"));
            }
        }
    }
    for (login, expected) in [
        ("Broken", AccountError::CredentialLookupFailed.into()),
        ("Store", AccountError::CredentialStoreUnavailable.into()),
        ("Wrong", AccountError::WrongHostOrAccount.into()),
        ("WrongAfter", AccountError::WrongHostOrAccount.into()),
        ("Leak", AccountExecutionError::SecretOutputRejected),
        ("LeakStderr", AccountExecutionError::SecretOutputRejected),
    ] {
        for plan in selected_reads("github.com", login) {
            let case = fixture.case();
            let result = execute_selected_account(&case.source, &plan, &bounded);
            assert!(result.observed_connection_epoch.is_none());
            assert_eq!(result.result.unwrap_err(), expected);
        }
    }
    for (name, expected) in [
        (
            "missing-helper",
            AccountExecutionError::HostHelperUnavailable,
        ),
        ("malformed-reply", AccountExecutionError::Protocol),
        (
            "malformed-enumeration",
            AccountError::MalformedResponse.into(),
        ),
    ] {
        let case = fixture.case();
        case.touch(name);
        let result = discover_accounts(&case.source, "github.com", &bounded);
        assert_eq!(result.result.unwrap_err(), expected);
    }
    let case = fixture.case();
    assert_eq!(
        execute_selected_account(&case.source, &selected("github.com", "Large"), &bounded)
            .result
            .unwrap_err(),
        AccountError::MalformedResponse.into()
    );

    for (login, cancel) in [
        ("PipeDescendant", false),
        ("Slow", false),
        ("Slow", true),
        ("LookupSlow", false),
        ("LookupSlow", true),
    ] {
        let case = fixture.case();
        let cancellation = AccountCancellation::default();
        let success = login == "PipeDescendant";
        let bounded = AccountExecutionControl::new(
            Duration::from_secs(if success || cancel { 30 } else { 5 }),
            cancellation.clone(),
            None,
        )
        .unwrap();
        let canceller = cancel.then(|| case.cancel_after_start(cancellation));
        let result =
            execute_selected_account(&case.source, &selected("github.com", login), &bounded);
        if let Some(canceller) = canceller {
            assert!(
                canceller.join().unwrap(),
                "WSL descendant did not become ready"
            );
        }
        if success {
            assert!(result.result.is_ok());
        } else {
            assert_eq!(
                result.result.unwrap_err(),
                if cancel {
                    AccountExecutionError::Cancelled
                } else {
                    AccountExecutionError::TimedOut
                }
            );
        }
        case.assert_retired();
    }
}
