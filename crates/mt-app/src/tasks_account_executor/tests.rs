use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use mt_github::{GitHubRepoIdentity, WorkItemKind};
use mt_identity::{ExecutionHostId, HostInstallId, RepoId, WorktreeId};

use super::*;

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
#[derive(Clone)]
struct WslFixture {
    source: ProjectExecutionSnapshot,
}

#[cfg(windows)]
impl WslFixture {
    fn open() -> Self {
        assert_eq!(
            std::env::var("GITHUB_ACTIONS").as_deref(),
            Ok("true"),
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
        assert_eq!(
            distro,
            format!("mt-tasks-{run_id}-{run_attempt}"),
            "refusing a non-owned distro"
        );
        let marker = required("MT_TEST_WSL_MARKER");
        assert_eq!(marker, "/mini-term-fixture/owner.json");
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
        let output = fixture.command("/bin/cat", &[&marker]);
        fixture.require_success(&output);
        let owner: WslFixtureOwner = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|_| panic!("invalid WSL fixture owner marker"));
        assert!(
            owner
                == WslFixtureOwner {
                    schema: 1,
                    kind: "mini-term-tasks-wsl".into(),
                    run_id,
                    run_attempt,
                    repository,
                    sha,
                },
            "WSL fixture owner marker mismatch"
        );
        for (command, expected) in [
            ("command -v gh", "/usr/local/bin/gh\n"),
            ("command -v python3", "/usr/local/bin/python3\n"),
        ] {
            let output = fixture.command("/bin/sh", &["-c", command]);
            fixture.require_success(&output);
            assert!(
                output.stdout == expected.as_bytes(),
                "WSL resolved a non-fixture executable"
            );
        }
        let output = fixture.command("/usr/bin/sha256sum", &["/usr/local/bin/gh"]);
        fixture.require_success(&output);
        assert!(
            output.stdout == format!("{gh_hash}  /usr/local/bin/gh\n").as_bytes(),
            "WSL fixture ELF checksum mismatch"
        );
        let output = fixture.command("/usr/bin/test", &["-d", "/mini-term-fixture/cases"]);
        fixture.require_success(&output);
        fixture
    }

    fn command(&self, program: &str, args: &[&str]) -> CommandOutput {
        let result = crate::execution_host::execute_host_command(
            &self.source,
            &CommandPlan::new(program, args.iter().copied()),
            Duration::from_secs(5),
            4096,
        )
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

    fn require_success(&self, output: &CommandOutput) {
        assert!(
            output.exit_code == Some(0),
            "Actions WSL fixture command failed"
        );
    }

    fn case(&self) -> Self {
        let path = format!("/mini-term-fixture/cases/{}", uuid::Uuid::new_v4());
        let output = self.command("/bin/mkdir", &["--", &path]);
        self.require_success(&output);
        let mut source = self.source.clone();
        source.canonical_path = path;
        Self { source }
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
        self.require_success(&output);
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
#[ignore = "requires the same-run Actions-owned WSL distro/rootfs/marker and Linux gh fixture"]
fn tasks_account_executor_wsl_sentinels_cleanup_and_foreground_host() {
    let fixture = WslFixture::open();
    let source = fixture.case().source;
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
