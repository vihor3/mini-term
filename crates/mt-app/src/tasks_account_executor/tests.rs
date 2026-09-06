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
    TasksWslRetirementTrace, TasksWslRootRole, TasksWslRoots, tasks_wsl_marker_probe,
    tasks_wsl_retirement_trace, tasks_wsl_root_scope,
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

#[test]
fn private_capture_diagnostics_keep_cancellation_acknowledgement_required() {
    for (mode, expected, ack) in [
        ("cancel-ack", AccountExecutionError::Cancelled, true),
        ("cancel-no-ack", AccountExecutionError::CleanupFailed, false),
        (
            "cancel-cleanup-failed",
            AccountExecutionError::CleanupFailed,
            false,
        ),
        (
            "cancel-extra-field",
            AccountExecutionError::CleanupFailed,
            false,
        ),
        (
            "cancel-array-ack",
            AccountExecutionError::CleanupFailed,
            false,
        ),
    ] {
        let evidence = DescendantEvidence::new();
        let cancellation = AccountCancellation::default();
        let bounded =
            AccountExecutionControl::new(Duration::from_secs(15), cancellation.clone(), None)
                .unwrap();
        let mut command = Command::new(&fixture().executable);
        command.args(["--capture-lifecycle", mode]);
        evidence.configure(&mut command);
        let canceller = evidence.cancel_after_start(cancellation);
        let (result, diagnostics) = process::trace_capture(Instant::now(), || {
            process::envelope_fixture(command, &bounded, 4096)
        });
        assert!(
            canceller.join().unwrap(),
            "private capture fixture never became ready"
        );
        assert!(
            result.err() == Some(expected),
            "wrong capture error for {mode}: {}",
            diagnostics.describe()
        );
        let diagnostic = diagnostics.describe();
        assert!(diagnostic.contains("stage=StopLatched"));
        assert!(diagnostic.contains("latched=Some(Cancelled)"));
        assert!(diagnostic.contains("write_ok=Some(true)"));
        assert!(diagnostic.contains("stage=TreeRetired"));
        assert!(diagnostic.contains(&format!("cleanup_ack=Some({ack})")));
    }
}

#[test]
fn private_capture_unstopped_exit_is_not_relabelled_by_later_cancellation() {
    let cancellation = AccountCancellation::default();
    let bounded =
        AccountExecutionControl::new(Duration::from_secs(15), cancellation.clone(), None).unwrap();
    let mut command = Command::new(&fixture().executable);
    command.args(["--capture-lifecycle", "exit-no-ack"]);
    let (result, diagnostics) = process::trace_capture(Instant::now(), || {
        process::envelope_fixture(command, &bounded, 4096)
    });
    cancellation.cancel();
    assert!(bounded.cancellation().is_cancelled());
    assert!(matches!(
        result,
        Err(AccountExecutionError::HostHelperUnavailable)
    ));
    let diagnostic = diagnostics.describe();
    assert!(diagnostic.contains("stage=Exited"));
    assert!(diagnostic.contains("exit=Some(23) latched=None control=None"));
    assert!(diagnostic.contains("stage=TreeRetired"));
    assert!(diagnostic.contains("cleanup_ack=Some(false)"));
    assert!(!diagnostic.contains("stage=StopLatched"));
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

fn assert_invalid_host_reply(bytes: &[u8]) {
    assert_eq!(
        decode_host_reply(bytes, 64).err(),
        Some(AccountExecutionError::Protocol),
        "invalid host reply was accepted"
    );
    assert!(
        !host_reply_confirms_cleanup(bytes),
        "invalid cleanup acknowledgement"
    );
}

#[test]
fn host_reply_statuses_keep_valid_mappings_and_reject_unknown_or_duplicate_fields() {
    for (status, expected) in [
        ("cancelled", AccountExecutionError::Cancelled),
        ("timed-out", AccountExecutionError::TimedOut),
        (
            "helper-unavailable",
            AccountExecutionError::HostHelperUnavailable,
        ),
        ("client-missing", AccountError::ClientMissing.into()),
        (
            "credential-lookup-failed",
            AccountError::CredentialLookupFailed.into(),
        ),
        (
            "credential-store-unavailable",
            AccountError::CredentialStoreUnavailable.into(),
        ),
        (
            "named-account-unsupported",
            AccountError::UnsupportedNamedAccountLookup.into(),
        ),
        ("identity-mismatch", AccountError::WrongHostOrAccount.into()),
        ("cleanup-failed", AccountExecutionError::CleanupFailed),
        ("unsafe-output", AccountExecutionError::SecretOutputRejected),
        ("malformed", AccountError::MalformedResponse.into()),
        ("failed", AccountError::CommandFailed.into()),
    ] {
        let valid = serde_json::json!({"status": status});
        let bytes = serde_json::to_vec(&valid).unwrap();
        assert_eq!(decode_host_reply(&bytes, 64).err(), Some(expected));
        assert_eq!(
            host_reply_confirms_cleanup(&bytes),
            status != "cleanup-failed"
        );
        for (key, value) in [
            ("token", serde_json::json!("fixture_credential_extra")),
            (
                "extra",
                serde_json::json!({"nested": {"token": "fixture_credential_nested"}}),
            ),
            (
                "stdout",
                serde_json::json!("fixture_credential_wrong_shape"),
            ),
            ("exit_code", serde_json::json!(0)),
        ] {
            let mut invalid = valid.clone();
            invalid[key] = value;
            assert_invalid_host_reply(&serde_json::to_vec(&invalid).unwrap());
        }
        assert_invalid_host_reply(
            format!(r#"{{"status":"{status}","status":"{status}"}}"#).as_bytes(),
        );
        for wrong_type in [
            serde_json::Value::Null,
            serde_json::json!(true),
            serde_json::json!(1),
            serde_json::json!([status]),
            serde_json::json!({"status": status}),
        ] {
            assert_invalid_host_reply(
                &serde_json::to_vec(&serde_json::json!({"status": wrong_type})).unwrap(),
            );
        }
    }
}

#[test]
fn host_reply_output_requires_typed_unique_closed_fields() {
    let valid =
        serde_json::json!({"status": "output", "stdout": "ok", "stderr": "", "exit_code": 0});
    let bytes = serde_json::to_vec(&valid).unwrap();
    let output = decode_host_reply(&bytes, 64).unwrap();
    assert_eq!(output.stdout, b"ok");
    assert!(output.stderr.is_empty());
    assert_eq!(output.exit_code, Some(0));
    assert!(host_reply_confirms_cleanup(&bytes));
    for key in ["status", "stdout", "stderr", "exit_code"] {
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert_invalid_host_reply(&serde_json::to_vec(&missing).unwrap());
        for wrong_type in [
            serde_json::Value::Null,
            serde_json::json!(false),
            serde_json::json!([]),
            serde_json::json!({"token": "fixture_credential_nested"}),
        ] {
            let mut invalid = valid.clone();
            invalid[key] = wrong_type;
            assert_invalid_host_reply(&serde_json::to_vec(&invalid).unwrap());
        }
    }
    for (key, value) in [
        ("stdout", serde_json::json!(1)),
        ("stderr", serde_json::json!(1)),
        ("exit_code", serde_json::json!("0")),
        ("exit_code", serde_json::json!(0.5)),
        ("exit_code", serde_json::json!(2147483648_i64)),
        ("token", serde_json::json!("fixture_credential_extra")),
        (
            "extra",
            serde_json::json!({"token": "fixture_credential_nested"}),
        ),
    ] {
        let mut invalid = valid.clone();
        invalid[key] = value;
        assert_invalid_host_reply(&serde_json::to_vec(&invalid).unwrap());
    }
    for extra in [
        r#""status":"output""#,
        r#""stdout":"ok""#,
        r#""stderr":"""#,
        r#""exit_code":0"#,
    ] {
        assert_invalid_host_reply(
            format!(r#"{{"status":"output","stdout":"ok","stderr":"","exit_code":0,{extra}}}"#)
                .as_bytes(),
        );
    }
    for invalid in [b"null".as_slice(), b"true", b"42", br#""output""#, b"{}"] {
        assert_invalid_host_reply(invalid);
    }
}

#[test]
fn host_reply_statuses_require_single_objects_not_positional_arrays() {
    for status in [
        "cancelled",
        "timed-out",
        "helper-unavailable",
        "client-missing",
        "credential-lookup-failed",
        "credential-store-unavailable",
        "named-account-unsupported",
        "identity-mismatch",
        "cleanup-failed",
        "unsafe-output",
        "malformed",
        "failed",
    ] {
        let object = serde_json::json!({"status": status});
        let padded = format!(" \n{object}\r\n\t");
        assert!(parse_host_reply(padded.as_bytes()).is_ok());
        assert_eq!(
            host_reply_confirms_cleanup(padded.as_bytes()),
            status != "cleanup-failed"
        );
        for invalid in [
            serde_json::json!([status]),
            serde_json::json!([[status]]),
            serde_json::json!([object.clone()]),
            serde_json::json!(status),
            serde_json::Value::Null,
        ] {
            assert_invalid_host_reply(&serde_json::to_vec(&invalid).unwrap());
        }
        for suffix in ["{}", "[]", "null", "true", r#"["cancelled"]"#] {
            assert_invalid_host_reply(format!("{object}\n{suffix}").as_bytes());
        }
    }
}

#[test]
fn host_reply_output_rejects_positional_nested_scalar_and_trailing_json() {
    let object =
        serde_json::json!({"status": "output", "stdout": "ok", "stderr": "", "exit_code": 0});
    let padded = format!("\t{object} \r\n");
    let output = decode_host_reply(padded.as_bytes(), 64).unwrap();
    assert_eq!(output.stdout, b"ok");
    assert!(output.stderr.is_empty());
    assert_eq!(output.exit_code, Some(0));
    assert!(host_reply_confirms_cleanup(padded.as_bytes()));
    for invalid in [
        serde_json::json!(["output", "ok", "", 0]),
        serde_json::json!([["output", "ok", "", 0]]),
        serde_json::json!([object.clone()]),
        serde_json::json!("output"),
        serde_json::json!(0),
        serde_json::json!(false),
        serde_json::Value::Null,
    ] {
        assert_invalid_host_reply(&serde_json::to_vec(&invalid).unwrap());
    }
    for field in ["status", "stdout", "stderr", "exit_code"] {
        let mut invalid = object.clone();
        invalid[field] = serde_json::json!([object[field].clone()]);
        assert_invalid_host_reply(&serde_json::to_vec(&invalid).unwrap());
    }
    for suffix in [
        object.to_string(),
        "[]".into(),
        "null".into(),
        "false".into(),
        "123".into(),
        r#""fixture_credential_trailing""#.into(),
    ] {
        assert_invalid_host_reply(format!("{object}\n{suffix}").as_bytes());
    }
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
    SingleEmptyArgv,
    MultipleEmptyArgv,
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
        Ok(output) => {
            wsl_cwd_diagnostic(pre_project, program, WslPreludeStage::LiteralArgv, output)
        }
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
    attested_distro: Option<String>,
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug)]
enum WslLifecycleCase {
    PipeDescendant,
    DataTimeout,
    DataCancel,
    LookupTimeout,
    LookupCancel,
}

#[cfg(windows)]
#[derive(Default)]
struct WslReadinessObservation {
    ready: bool,
    probe_count: u32,
    first_probe_started_us: u128,
    first_probe_returned_us: u128,
    first_retirement: TasksWslRetirementTrace,
    probe_started_us: u128,
    probe_returned_us: u128,
    retirement: TasksWslRetirementTrace,
    cancel_us: u128,
}

#[cfg(windows)]
impl WslReadinessObservation {
    fn record_probe(
        &mut self,
        ready: bool,
        started_us: u128,
        returned_us: u128,
        retirement: TasksWslRetirementTrace,
    ) {
        if self.probe_count == 0 {
            self.first_probe_started_us = started_us;
            self.first_probe_returned_us = returned_us;
            self.first_retirement = retirement;
        }
        self.probe_count = self.probe_count.saturating_add(1);
        self.ready = ready;
        self.probe_started_us = started_us;
        self.probe_returned_us = returned_us;
        self.retirement = retirement;
    }

    fn describe(&self) -> String {
        format!(
            "ready={} probe_count={} first_probe_started_us={} first_probe_returned_us={} first_retirement={:?} probe_started_us={} probe_returned_us={} retirement={:?} cancel_us={}",
            self.ready,
            self.probe_count,
            self.first_probe_started_us,
            self.first_probe_returned_us,
            self.first_retirement,
            self.probe_started_us,
            self.probe_returned_us,
            self.retirement,
            self.cancel_us
        )
    }
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
        source.backend = ExecutionBackend::Wsl {
            distro: distro.clone(),
        };
        let mut fixture = Self {
            source,
            attested_distro: None,
        };
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
        fixture.attested_distro = Some(distro);
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
        Self {
            source,
            attested_distro: self.attested_distro.clone(),
        }
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
            let cardinality_cases: [(WslPreludeStage, &[&str], &[u8]); 2] = [
                (WslPreludeStage::SingleEmptyArgv, &[""], b"1\0\0"),
                (
                    WslPreludeStage::MultipleEmptyArgv,
                    &["", "", "middle", ""],
                    b"4\0\0\0middle\0\0",
                ),
            ];
            for (stage, data, expected) in cardinality_cases {
                let command = CommandPlan::new(
                    "/bin/sh",
                    [
                        "-c",
                        r#"exec /usr/bin/printf '%s\0' "$#" "$@""#,
                        "mini-term-argv",
                    ]
                    .into_iter()
                    .chain(data.iter().copied()),
                );
                let output = run(&captured.source, &command, stage, WslCwdProgram::Absolute);
                assert!(
                    output.exit_code == Some(0) && output.stdout == expected,
                    "WSL empty argument cardinality was not preserved: {}",
                    wsl_cwd_diagnostic(pre_project, WslCwdProgram::Absolute, stage, &output)
                );
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
        started: Instant,
        roots: TasksWslRoots,
    ) -> std::thread::JoinHandle<WslReadinessObservation> {
        let fixture = self.clone();
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut observation = WslReadinessObservation::default();
            loop {
                let probe_started_us = started.elapsed().as_micros();
                let (ready, retirement) =
                    tasks_wsl_root_scope(&roots, TasksWslRootRole::Readiness, || {
                        tasks_wsl_retirement_trace(started, || fixture.exists("ready"))
                    });
                let probe_returned_us = started.elapsed().as_micros();
                observation.record_probe(ready, probe_started_us, probe_returned_us, retirement);
                if ready || Instant::now() >= deadline {
                    cancellation.cancel();
                    observation.cancel_us = started.elapsed().as_micros();
                    return observation;
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

// This boundary accepts an attested fixture and typed failure metadata, never
// private command outputs. Only its own fixed producer can supply preview bytes.
#[cfg(windows)]
mod wsl_public_comparison {
    use super::*;
    use crate::execution_host::tasks_wsl_public_timing::{Pair, ROWS, Release, Row, Timing};

    const SCRIPT: &str = r"printf '%s\n' mt-public-start && : > ready && /usr/bin/sleep 1 && printf '%s\n' mt-public-end";
    const START: &[u8] = b"mt-public-start\n";
    const EXPECTED: &[u8] = b"mt-public-start\nmt-public-end\n";

    #[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
    enum Stage {
        CreateCase,
        Producer,
        Readiness,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
    enum Failure {
        Ownership,
        ProgramNotFound,
        Disconnected,
        Rejected,
        Io,
        SourceChanged,
        Incomplete,
        UnexpectedExit,
        UnexpectedProbeOutput,
        ThreadStart,
        ThreadPanic,
        ScopeRejected,
    }

    #[derive(Clone, Copy, Debug, serde::Serialize)]
    struct Metadata {
        stage: Stage,
        failure: Option<Failure>,
        exit: Option<i32>,
        timed_out: bool,
        stdout_truncated: bool,
        stderr_truncated: bool,
        stdout_bytes: usize,
        stderr_bytes: usize,
    }

    struct Reply {
        metadata: Metadata,
        producer_output: Option<CommandOutput>,
        fixture_credential: bool,
    }

    impl Reply {
        fn received(
            stage: Stage,
            result: Result<HostCommandResult, CommandExecutionError>,
        ) -> Self {
            let mut metadata = Metadata {
                stage,
                failure: None,
                exit: None,
                timed_out: false,
                stdout_truncated: false,
                stderr_truncated: false,
                stdout_bytes: 0,
                stderr_bytes: 0,
            };
            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    metadata.failure = Some(match error.kind {
                        CommandExecutionErrorKind::ProgramNotFound => Failure::ProgramNotFound,
                        CommandExecutionErrorKind::Disconnected => Failure::Disconnected,
                        CommandExecutionErrorKind::Rejected => Failure::Rejected,
                        CommandExecutionErrorKind::Io => Failure::Io,
                    });
                    return Self {
                        metadata,
                        producer_output: None,
                        fixture_credential: false,
                    };
                }
            };
            let output = result.output;
            // Preserve only this suppression bit when incomplete/stale public
            // producer bytes are discarded. Decide for both rows before previews.
            let fixture_credential = stage == Stage::Producer && has_fixture_credential(&output);
            metadata.exit = output.exit_code;
            metadata.timed_out = output.timed_out;
            metadata.stdout_truncated = output.stdout_truncated;
            metadata.stderr_truncated = output.stderr_truncated;
            metadata.stdout_bytes = output.stdout.len();
            metadata.stderr_bytes = output.stderr.len();
            if result.observed_connection_epoch.is_some() {
                metadata.failure = Some(Failure::SourceChanged);
            } else if output.timed_out
                || output.stdout_truncated
                || output.stderr_truncated
                || output.stdout.len() > WSL_FIXTURE_OUTPUT_CAP
                || output.stderr.len() > WSL_FIXTURE_OUTPUT_CAP
                || output.exit_code.is_none()
            {
                metadata.failure = Some(Failure::Incomplete);
            }
            let producer_output =
                (stage == Stage::Producer && metadata.failure.is_none()).then_some(output);
            Self {
                metadata,
                producer_output,
                fixture_credential,
            }
        }

        fn success(&self) -> bool {
            self.metadata.failure.is_none() && self.metadata.exit == Some(0)
        }
    }

    struct PublicCase {
        root: WslFixture,
        case: WslFixture,
    }

    impl PublicCase {
        fn new(fixture: &WslFixture, id: uuid::Uuid) -> Result<Self, Failure> {
            let (ExecutionBackend::Wsl { distro }, Some(attested)) =
                (&fixture.source.backend, &fixture.attested_distro)
            else {
                return Err(Failure::Ownership);
            };
            if distro != attested
                || fixture.source.canonical_path != "/mini-term-fixture"
                || fixture.source.root_source_path != "/mini-term-fixture"
            {
                return Err(Failure::Ownership);
            }
            let mut case = fixture.clone();
            case.source.canonical_path = format!("/mini-term-fixture/cases/{id}");
            Ok(Self {
                root: fixture.clone(),
                case,
            })
        }

        fn plan(&self, stage: Stage) -> (&WslFixture, CommandPlan) {
            match stage {
                Stage::CreateCase => (
                    &self.root,
                    CommandPlan::new(
                        "/bin/mkdir",
                        ["--", self.case.source.canonical_path.as_str()],
                    ),
                ),
                Stage::Producer => (&self.case, CommandPlan::new("/bin/sh", ["-c", SCRIPT])),
                Stage::Readiness => (
                    &self.case,
                    CommandPlan::new("/usr/bin/test", ["-f", "ready"]),
                ),
            }
        }

        fn execute(&self, stage: Stage) -> Reply {
            let (fixture, plan) = self.plan(stage);
            let args = plan.args.iter().map(String::as_str).collect::<Vec<_>>();
            Reply::received(stage, fixture.run_command(&plan.program, &args))
        }
    }

    #[derive(Default)]
    struct Readiness {
        command: Option<Metadata>,
        ready: bool,
        failure: Option<Failure>,
        retirement: TasksWslRetirementTrace,
    }

    fn read_once(pair: &Pair, probe: impl FnOnce() -> Reply) -> Readiness {
        let (result, retirement) = tasks_wsl_retirement_trace(pair.started(), || pair.probe(probe));
        let reply = match result {
            Ok(reply) => reply,
            Err(_) => {
                return Readiness {
                    failure: Some(Failure::ScopeRejected),
                    retirement,
                    ..Default::default()
                };
            }
        };
        let command = reply.metadata;
        let failure = command.failure.or_else(|| {
            if !matches!(command.exit, Some(0 | 1)) {
                Some(Failure::UnexpectedExit)
            } else if command.stdout_bytes != 0 || command.stderr_bytes != 0 {
                Some(Failure::UnexpectedProbeOutput)
            } else {
                None
            }
        });
        Readiness {
            command: Some(command),
            ready: failure.is_none() && command.exit == Some(0),
            failure,
            retirement,
        }
    }

    struct RowReport {
        row: Row,
        failure: Option<Failure>,
        setup: Option<Metadata>,
        producer: Option<Reply>,
        readiness: Option<Readiness>,
        timing: Timing,
    }

    impl RowReport {
        fn new(row: Row) -> Self {
            Self {
                row,
                failure: None,
                setup: None,
                producer: None,
                readiness: None,
                timing: Timing::default(),
            }
        }
    }

    pub(super) struct Report {
        rows: [RowReport; 2],
    }

    impl Default for Report {
        fn default() -> Self {
            Self {
                rows: ROWS.map(RowReport::new),
            }
        }
    }

    struct ProbeOwner {
        pair: Pair,
        thread: Option<std::thread::JoinHandle<Readiness>>,
    }

    impl ProbeOwner {
        fn join(&mut self) -> Result<Readiness, Failure> {
            self.thread
                .take()
                .ok_or(Failure::ThreadPanic)?
                .join()
                .map_err(|_| Failure::ThreadPanic)
        }
    }

    impl Drop for ProbeOwner {
        fn drop(&mut self) {
            if let Some(thread) = self.thread.take() {
                // The single probe retains its 5s runner/cleanup bounds; abort
                // also releases its at-most-10s hold before this unwind join.
                self.pair.abort();
                let _ = thread.join();
            }
        }
    }

    fn collect_pair(
        pair: &Pair,
        spawn: impl FnOnce() -> std::io::Result<std::thread::JoinHandle<Readiness>>,
        producer: impl FnOnce() -> Reply,
    ) -> RowReport {
        let mut report = RowReport::new(pair.row());
        let thread = match spawn() {
            Ok(thread) => thread,
            Err(_) => {
                report.failure = Some(Failure::ThreadStart);
                return report;
            }
        };
        let mut owner = ProbeOwner {
            pair: pair.clone(),
            thread: Some(thread),
        };
        pair.producer_started();
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(producer)) {
            Ok(producer) => {
                pair.producer_returned();
                report.producer = Some(producer);
            }
            Err(_) => {
                pair.abort();
                report.failure = Some(Failure::ThreadPanic);
            }
        }
        match owner.join() {
            Ok(readiness) => report.readiness = Some(readiness),
            Err(failure) => report.failure = Some(failure),
        }
        report.timing = pair.timing();
        report
    }

    fn run_row(fixture: &WslFixture, row: Row) -> RowReport {
        let case = match PublicCase::new(fixture, uuid::Uuid::new_v4()) {
            Ok(case) => case,
            Err(failure) => {
                return RowReport {
                    failure: Some(failure),
                    ..RowReport::new(row)
                };
            }
        };
        // mkdir has no -p: an existing directory is a failure, never reused.
        let setup = case.execute(Stage::CreateCase);
        if !setup.success() {
            return RowReport {
                failure: Some(setup.metadata.failure.unwrap_or(Failure::UnexpectedExit)),
                setup: Some(setup.metadata),
                ..RowReport::new(row)
            };
        }
        let peer = PublicCase {
            root: case.root.clone(),
            case: case.case.clone(),
        };
        let pair = Pair::new(row, Instant::now());
        let probe_pair = pair.clone();
        let mut report = collect_pair(
            &pair,
            || {
                std::thread::Builder::new()
                    .spawn(move || read_once(&probe_pair, || peer.execute(Stage::Readiness)))
            },
            || case.execute(Stage::Producer),
        );
        report.setup = Some(setup.metadata);
        report
    }

    fn run_rows(mut run: impl FnMut(Row) -> RowReport) -> Report {
        Report {
            rows: ROWS.map(&mut run),
        }
    }

    #[derive(Default)]
    pub(super) struct Attempt(bool);

    impl Attempt {
        fn after_failure_with(
            &mut self,
            lifecycle: WslLifecycleCase,
            actual: Option<AccountExecutionError>,
            cancelled_at_return: bool,
            action: impl FnOnce() -> Report,
        ) -> Option<Report> {
            if self.0
                || !matches!(
                    lifecycle,
                    WslLifecycleCase::DataCancel | WslLifecycleCase::LookupCancel
                )
                || actual != Some(AccountExecutionError::HostHelperUnavailable)
                || cancelled_at_return
            {
                return None;
            }
            self.0 = true;
            Some(action())
        }

        pub(super) fn after_failure(
            &mut self,
            fixture: &WslFixture,
            lifecycle: WslLifecycleCase,
            actual: Option<AccountExecutionError>,
            cancelled_at_return: bool,
        ) -> Option<Report> {
            self.after_failure_with(lifecycle, actual, cancelled_at_return, || {
                run_rows(|row| run_row(fixture, row))
            })
        }
    }

    fn has_fixture_credential(output: &CommandOutput) -> bool {
        // Fixture sentinel detection only, not a general secret sanitizer.
        let prefix = b"fixture_credential_";
        let needles = [
            prefix.to_vec(),
            prefix.iter().flat_map(|byte| [*byte, 0]).collect(),
            prefix.iter().flat_map(|byte| [0, *byte]).collect(),
        ];
        [&output.stdout, &output.stderr].iter().any(|bytes| {
            needles.iter().any(|needle| {
                bytes
                    .windows(needle.len())
                    .any(|window| window == needle.as_slice())
            })
        })
    }

    fn decode_public(bytes: &[u8]) -> Option<String> {
        let (bytes, big_endian) = if let Some(body) = bytes.strip_prefix(&[0xff, 0xfe]) {
            (body, false)
        } else if let Some(body) = bytes.strip_prefix(&[0xfe, 0xff]) {
            (body, true)
        } else if !bytes.contains(&0) {
            return std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
                .ok()
                .map(str::to_owned);
        } else {
            match bytes.get(..2)? {
                [0, next] if *next != 0 => (bytes, true),
                [first, 0] if *first != 0 => (bytes, false),
                _ => return None,
            }
        };
        let mut pairs = bytes.chunks_exact(2);
        let units = pairs
            .by_ref()
            .map(|pair| {
                if big_endian {
                    u16::from_be_bytes([pair[0], pair[1]])
                } else {
                    u16::from_le_bytes([pair[0], pair[1]])
                }
            })
            .collect::<Vec<_>>();
        if !pairs.remainder().is_empty() {
            return None;
        }
        String::from_utf16(&units).ok()
    }

    fn previews(reply: &Reply, suppress: bool) -> (bool, bool, [Option<String>; 2]) {
        let Some(output) = reply
            .producer_output
            .as_ref()
            .filter(|_| reply.metadata.stage == Stage::Producer)
        else {
            return (false, reply.fixture_credential, [None, None]);
        };
        let start = output.stdout.starts_with(START);
        if suppress {
            return (start, reply.fixture_credential, [None, None]);
        }
        let stdout = output.stdout.strip_prefix(START).unwrap_or(&output.stdout);
        let texts = [stdout, output.stderr.as_slice()].map(|bytes| {
            decode_public(bytes).map(|text| text.chars().take(256).collect::<String>())
        });
        (start, false, texts)
    }

    fn escaped_json(value: &serde_json::Value) -> String {
        // JSON handles C0; explicitly escape remaining control/directional scalars.
        value.to_string().chars().map(|ch| {
            if ch.is_control() || matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
                format!("\\u{:04x}", ch as u32)
            } else { ch.to_string() }
        }).collect()
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
    enum TimingIssue {
        Missing,
        ProbeFailure,
        RetirementFailed,
        RetirementUnavailable,
        Release,
        NonOverlapping,
    }

    impl RowReport {
        fn timing_issue(&self) -> Option<TimingIssue> {
            let Some(readiness) = &self.readiness else {
                return Some(TimingIssue::Missing);
            };
            if readiness
                .retirement
                .first
                .is_some_and(|entry| !entry.succeeded)
                || readiness
                    .retirement
                    .last
                    .is_some_and(|entry| !entry.succeeded)
            {
                return Some(TimingIssue::RetirementFailed);
            }
            if self.failure.is_some() || readiness.failure.is_some() {
                return Some(TimingIssue::ProbeFailure);
            }
            if readiness.retirement.calls != 1 {
                return Some(TimingIssue::RetirementUnavailable);
            }
            if !self.timing.eligible
                || !matches!(
                    (self.row, self.timing.release),
                    (Row::Immediate, Some(Release::Immediate))
                        | (Row::AfterProducer, Some(Release::ProducerReturned))
                )
            {
                return Some(TimingIssue::Release);
            }
            match (
                self.timing.producer_start_us,
                self.timing.probe_completed_us,
                self.timing.producer_return_us,
            ) {
                (Some(start), Some(completed), Some(returned))
                    if start <= completed && completed < returned =>
                {
                    None
                }
                _ => Some(TimingIssue::NonOverlapping),
            }
        }

        fn value(&self, suppress: bool) -> serde_json::Value {
            let (start, suppressed, texts) = self
                .producer
                .as_ref()
                .map(|reply| previews(reply, suppress))
                .unwrap_or((false, false, [None, None]));
            let markers_match = self
                .producer
                .as_ref()
                .and_then(|reply| reply.producer_output.as_ref())
                .is_some_and(|output| output.stdout == EXPECTED && output.stderr.is_empty());
            serde_json::json!({
                "row": self.row, "failure": self.failure, "setup": self.setup,
                "producer": self.producer.as_ref().map(|reply| reply.metadata),
                "readiness": self.readiness.as_ref().map(|readiness| serde_json::json!({
                    "command": readiness.command, "ready": readiness.ready,
                    "failure": readiness.failure,
                    "retirement": format!("{:?}", readiness.retirement),
                })),
                "timing": self.timing, "timing_issue": self.timing_issue(),
                "start_marker": start, "markers_match": markers_match,
                "fixture_secret_suppressed": suppressed,
                "stdout_preview": texts[0], "stderr_preview": texts[1],
            })
        }
    }

    impl Report {
        pub(super) fn describe(&self) -> String {
            let suppress = self.rows.iter().any(|row| {
                row.producer
                    .as_ref()
                    .is_some_and(|reply| reply.fixture_credential)
            });
            let mut value = serde_json::json!({
                "public_comparison": true, "inconclusive": true, "preview_limit": false,
                "fixture_secret_suppressed": suppress,
                "rows": self.rows.each_ref().map(|row| row.value(suppress)),
            });
            let rendered = escaped_json(&value);
            if rendered.len() <= 4096 {
                return rendered;
            }
            for row in value["rows"].as_array_mut().unwrap() {
                row["stdout_preview"] = serde_json::Value::Null;
                row["stderr_preview"] = serde_json::Value::Null;
            }
            value["preview_limit"] = true.into();
            let metadata = escaped_json(&value);
            if metadata.len() <= 4096 {
                metadata
            } else {
                let compact = escaped_json(&serde_json::json!({
                    "public_comparison": true, "inconclusive": true, "diagnostic_limit": true,
                    "retirement_omitted": true, "fixture_secret_suppressed": suppress,
                    "rows": self.rows.each_ref().map(|row| serde_json::json!({
                        "row": row.row, "failure": row.failure, "timing": row.timing,
                        "producer": row.producer.as_ref().map(|reply| reply.metadata),
                        "readiness": row.readiness.as_ref().map(|probe| probe.command),
                        "timing_issue": row.timing_issue(),
                    })),
                }));
                if compact.len() <= 4096 {
                    compact
                } else {
                    r#"{"public_comparison":true,"inconclusive":true,"diagnostic_limit":true,"rows":[{"row":"Immediate"},{"row":"AfterProducer"}]}"#.into()
                }
            }
        }
    }

    fn test_root() -> WslFixture {
        let mut source = snapshot(Path::new("/mini-term-fixture"));
        source.backend = ExecutionBackend::Wsl {
            distro: "mt-tasks-12345-1".into(),
        };
        WslFixture {
            source,
            attested_distro: Some("mt-tasks-12345-1".into()),
        }
    }

    fn output(stdout: Vec<u8>, stderr: Vec<u8>, exit: i32) -> HostCommandResult {
        HostCommandResult {
            output: CommandOutput {
                stdout,
                stderr,
                exit_code: Some(exit),
                ..Default::default()
            },
            observed_connection_epoch: None,
        }
    }

    fn describe_output(
        result: Result<HostCommandResult, CommandExecutionError>,
    ) -> (String, serde_json::Value) {
        let row = RowReport {
            producer: Some(Reply::received(Stage::Producer, result)),
            ..RowReport::new(Row::Immediate)
        };
        let text = describe_row(row);
        assert!(text.len() <= 4096);
        assert!(!text.chars().any(char::is_control));
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        (text, parsed["rows"][0].clone())
    }

    fn describe_row(row: RowReport) -> String {
        Report {
            rows: [row, RowReport::new(Row::AfterProducer)],
        }
        .describe()
    }

    #[test]
    fn public_comparison_plans_are_fixed_and_require_the_attested_root() {
        let root = test_root();
        let id = uuid::Uuid::new_v4();
        let case =
            PublicCase::new(&root, id).unwrap_or_else(|_| panic!("synthetic owner rejected"));
        assert_eq!(
            case.case.source.canonical_path,
            format!("/mini-term-fixture/cases/{id}")
        );
        assert!(matches!(
            (&case.case.source.backend, &root.source.backend),
            (ExecutionBackend::Wsl { distro: case_distro }, ExecutionBackend::Wsl { distro: root_distro })
                if case_distro == root_distro
        ));
        assert_eq!(
            case.case.source.execution_host_id,
            root.source.execution_host_id
        );
        assert_eq!(
            case.case.source.root_project_id,
            root.source.root_project_id
        );
        assert_eq!(case.case.source.worktree_id, root.source.worktree_id);
        let (source, mkdir) = case.plan(Stage::CreateCase);
        assert_eq!(source.source.canonical_path, "/mini-term-fixture");
        assert_eq!(mkdir.program, "/bin/mkdir");
        assert_eq!(mkdir.args, ["--", case.case.source.canonical_path.as_str()]);
        let (source, producer) = case.plan(Stage::Producer);
        assert_eq!(
            source.source.canonical_path,
            case.case.source.canonical_path
        );
        assert_eq!(producer.program, "/bin/sh");
        assert_eq!(
            producer.args,
            [
                "-c",
                r"printf '%s\n' mt-public-start && : > ready && /usr/bin/sleep 1 && printf '%s\n' mt-public-end"
            ]
        );
        let (source, probe) = case.plan(Stage::Readiness);
        assert_eq!(
            source.source.canonical_path,
            case.case.source.canonical_path
        );
        assert_eq!(probe.program, "/usr/bin/test");
        assert_eq!(probe.args, ["-f", "ready"]);
        for change in 0..5 {
            let mut denied = root.clone();
            match change {
                0 => denied.attested_distro = None,
                1 => denied.source.backend = ExecutionBackend::Local,
                2 => {
                    denied.source.backend = ExecutionBackend::Wsl {
                        distro: "another-distro".into(),
                    }
                }
                3 => denied.source.canonical_path = case.case.source.canonical_path.clone(),
                _ => denied.source.root_source_path = "/sibling".into(),
            }
            assert!(matches!(
                PublicCase::new(&denied, id),
                Err(Failure::Ownership)
            ));
        }
    }

    #[test]
    fn public_comparison_attempt_is_exact_once_and_never_replaces_private_failure() {
        for lifecycle in [
            WslLifecycleCase::PipeDescendant,
            WslLifecycleCase::DataTimeout,
            WslLifecycleCase::LookupTimeout,
            WslLifecycleCase::DataCancel,
            WslLifecycleCase::LookupCancel,
        ] {
            for actual in [
                None,
                Some(AccountExecutionError::Cancelled),
                Some(AccountExecutionError::TimedOut),
                Some(AccountExecutionError::CleanupFailed),
                Some(AccountExecutionError::HostHelperUnavailable),
            ] {
                for cancelled in [false, true] {
                    let mut attempt = Attempt::default();
                    let calls = std::cell::Cell::new(0);
                    let should_run = matches!(
                        lifecycle,
                        WslLifecycleCase::DataCancel | WslLifecycleCase::LookupCancel
                    ) && actual
                        == Some(AccountExecutionError::HostHelperUnavailable)
                        && !cancelled;
                    let report = attempt.after_failure_with(lifecycle, actual, cancelled, || {
                        calls.set(calls.get() + 1);
                        run_rows(|row| RowReport {
                            failure: Some(Failure::Ownership),
                            ..RowReport::new(row)
                        })
                    });
                    assert_eq!(report.is_some(), should_run);
                    assert_eq!(calls.get(), usize::from(should_run));
                    assert!(
                        attempt
                            .after_failure_with(lifecycle, actual, cancelled, || panic!(
                                "second public attempt"
                            ))
                            .is_none()
                    );
                    if let Some(report) = report {
                        assert_eq!(report.rows[0].failure, Some(Failure::Ownership));
                        assert_eq!(report.rows[1].failure, Some(Failure::Ownership));
                        assert_eq!(actual, Some(AccountExecutionError::HostHelperUnavailable));
                    }
                }
            }
        }
        for result in [
            Report::default(),
            run_rows(|row| RowReport {
                failure: Some(Failure::ThreadStart),
                ..RowReport::new(row)
            }),
        ] {
            let original = Some(AccountExecutionError::HostHelperUnavailable);
            let mut attempt = Attempt::default();
            assert!(
                attempt
                    .after_failure_with(WslLifecycleCase::DataCancel, original, false, || result)
                    .is_some()
            );
            assert_eq!(original, Some(AccountExecutionError::HostHelperUnavailable));
        }
    }

    #[test]
    fn public_comparison_joins_readiness_on_producer_and_thread_errors() {
        for producer_error in [false, true] {
            let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let peer = finished.clone();
            let pair = Pair::new(Row::AfterProducer, Instant::now());
            let report = collect_pair(
                &pair,
                || {
                    std::thread::Builder::new().spawn(move || {
                        peer.store(true, std::sync::atomic::Ordering::Release);
                        Readiness {
                            failure: Some(Failure::UnexpectedExit),
                            ..Default::default()
                        }
                    })
                },
                || {
                    Reply::received(
                        Stage::Producer,
                        if producer_error {
                            Err(CommandExecutionError::new(
                                CommandExecutionErrorKind::Io,
                                "fixture_credential_not_logged",
                            ))
                        } else {
                            Ok(output(EXPECTED.to_vec(), Vec::new(), 0))
                        },
                    )
                },
            );
            assert!(finished.load(std::sync::atomic::Ordering::Acquire));
            assert_eq!(
                report.readiness.as_ref().unwrap().failure,
                Some(Failure::UnexpectedExit)
            );
            assert!(pair.timing().producer_return_us.is_some());
            assert!(!describe_row(report).contains("fixture_credential_"));
        }
        let pair = Pair::new(Row::AfterProducer, Instant::now());
        let report = collect_pair(
            &pair,
            || Err(std::io::Error::other("fixture_credential_thread_error")),
            || panic!("producer started without readiness thread"),
        );
        assert_eq!(report.failure, Some(Failure::ThreadStart));
        assert!(pair.timing().producer_start_us.is_none());
        assert!(!describe_row(report).contains("fixture_credential_"));
        let report = collect_pair(
            &Pair::new(Row::AfterProducer, Instant::now()),
            || std::thread::Builder::new().spawn(|| panic!("synthetic public probe failure")),
            || Reply::received(Stage::Producer, Ok(output(Vec::new(), Vec::new(), -1))),
        );
        assert_eq!(report.failure, Some(Failure::ThreadPanic));
        assert_eq!(report.producer.unwrap().metadata.exit, Some(-1));
    }

    #[test]
    fn public_comparison_two_rows_each_probe_once_even_when_absent_or_failed() {
        for exit in [0, 1, 23] {
            let mut rows = Vec::new();
            let calls = std::cell::Cell::new(0);
            let report = run_rows(|row| {
                rows.push(row);
                let pair = Pair::new(row, Instant::now());
                let readiness = read_once(&pair, || {
                    calls.set(calls.get() + 1);
                    Reply::received(Stage::Readiness, Ok(output(Vec::new(), Vec::new(), exit)))
                });
                assert_eq!(readiness.ready, exit == 0);
                assert_eq!(
                    readiness.failure,
                    (exit == 23).then_some(Failure::UnexpectedExit)
                );
                let rejected = read_once(&pair, || panic!("second public probe"));
                assert_eq!(rejected.failure, Some(Failure::ScopeRejected));
                RowReport {
                    readiness: Some(readiness),
                    timing: pair.timing(),
                    ..RowReport::new(row)
                }
            });
            assert_eq!(rows, [Row::Immediate, Row::AfterProducer]);
            assert_eq!(calls.get(), 2);
            for row in report.rows {
                assert!(row.timing.probe_start_us <= row.timing.probe_return_us);
                assert_eq!(row.timing.release, Some(Release::Bypassed));
            }
        }
        for (result, expected) in [
            (output(Vec::new(), Vec::new(), 23), Failure::UnexpectedExit),
            (
                output(b"public unexpected text".to_vec(), Vec::new(), 0),
                Failure::UnexpectedProbeOutput,
            ),
        ] {
            let failed = read_once(&Pair::new(Row::Immediate, Instant::now()), || {
                Reply::received(Stage::Readiness, Ok(result))
            });
            assert_eq!(failed.failure, Some(expected));
        }
    }

    fn native_readiness() -> Reply {
        assert_eq!(
            std::env::var("GITHUB_ACTIONS").as_deref(),
            Ok("true"),
            "Actions-only fixture"
        );
        Reply::received(
            Stage::Readiness,
            crate::execution_host::execute_host_command(
                &snapshot(Path::new(".")),
                &CommandPlan::new("cmd.exe", ["/d", "/c", "exit /b 1"]),
                Duration::from_secs(5),
                WSL_FIXTURE_OUTPUT_CAP,
            ),
        )
    }

    #[test]
    fn public_comparison_signals_before_join_on_return_error_and_producer_unwind() {
        for outcome in 0..3 {
            let pair = Pair::new(Row::AfterProducer, Instant::now());
            let peer = pair.clone();
            let report = collect_pair(
                &pair,
                || std::thread::Builder::new().spawn(move || read_once(&peer, native_readiness)),
                || match outcome {
                    0 => Reply::received(
                        Stage::Producer,
                        Ok(output(EXPECTED.to_vec(), Vec::new(), 0)),
                    ),
                    1 => Reply::received(
                        Stage::Producer,
                        Err(CommandExecutionError::new(
                            CommandExecutionErrorKind::Io,
                            "synthetic producer error",
                        )),
                    ),
                    _ => panic!("synthetic producer unwind"),
                },
            );
            let readiness = report.readiness.unwrap();
            assert_eq!(readiness.command.unwrap().exit, Some(1));
            assert!(!readiness.ready);
            assert_eq!(readiness.retirement.calls, 1);
            assert!(readiness.retirement.first.unwrap().succeeded);
            assert!(pair.timing().probe_return_us.is_some());
            if outcome == 2 {
                assert_eq!(report.failure, Some(Failure::ThreadPanic));
                assert!(report.producer.is_none());
                assert!(pair.timing().producer_return_us.is_none());
                assert_eq!(pair.timing().release, Some(Release::ProducerAborted));
            } else {
                assert!(matches!(
                    pair.timing().release,
                    Some(Release::ProducerReturned | Release::Late)
                ));
                assert!(pair.timing().producer_return_us <= pair.timing().probe_return_us);
                assert_eq!(
                    report.producer.unwrap().metadata.failure,
                    (outcome == 1).then_some(Failure::Io)
                );
            }
        }
    }

    #[test]
    fn public_comparison_owner_unwind_aborts_before_join_and_probe_panic_is_inconclusive() {
        for probe_panics in [false, true] {
            let pair = Pair::new(Row::AfterProducer, Instant::now());
            let peer = pair.clone();
            let thread = std::thread::spawn(move || {
                read_once(&peer, || {
                    if probe_panics {
                        panic!("synthetic probe unwind during owner unwind");
                    }
                    native_readiness()
                })
            });
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _owner = ProbeOwner {
                    pair: pair.clone(),
                    thread: Some(thread),
                };
                panic!("synthetic owner unwind");
            }));
            assert!(result.is_err());
            assert_eq!(
                pair.timing().release,
                Some(if probe_panics {
                    Release::ProbeAborted
                } else {
                    Release::ProducerAborted
                })
            );
            assert_eq!(pair.timing().probe_return_us.is_some(), !probe_panics);
        }
        let pair = Pair::new(Row::AfterProducer, Instant::now());
        let peer = pair.clone();
        let report = collect_pair(
            &pair,
            || {
                std::thread::Builder::new()
                    .spawn(move || read_once(&peer, || panic!("synthetic probe unwind")))
            },
            || Reply::received(Stage::Producer, Ok(output(Vec::new(), Vec::new(), -1))),
        );
        assert_eq!(report.failure, Some(Failure::ThreadPanic));
        assert_eq!(report.producer.as_ref().unwrap().metadata.exit, Some(-1));
        assert_eq!(report.timing.release, Some(Release::ProbeAborted));
        assert_eq!(report.timing_issue(), Some(TimingIssue::Missing));
    }

    #[test]
    fn public_previews_reject_nonproducer_incomplete_and_undecodable_sources() {
        for stage in [Stage::CreateCase, Stage::Readiness] {
            let report = RowReport {
                producer: Some(Reply::received(
                    stage,
                    Ok(output(b"never-preview-this".to_vec(), Vec::new(), 0)),
                )),
                ..RowReport::new(Row::Immediate)
            };
            assert!(!describe_row(report).contains("never-preview-this"));
        }
        for change in 0..7 {
            let mut result = output(b"never-preview-this".to_vec(), b"nor-this".to_vec(), -1);
            match change {
                0 => result.observed_connection_epoch = Some(1),
                1 => result.output.stdout_truncated = true,
                2 => result.output.stderr_truncated = true,
                3 => result.output.timed_out = true,
                4 => result.output.exit_code = None,
                5 => result.output.stdout = vec![b'x'; 4097],
                _ => result.output.stderr = vec![b'x'; 4097],
            }
            let (text, parsed) = describe_output(Ok(result));
            assert!(parsed["stdout_preview"].is_null() && parsed["stderr_preview"].is_null());
            assert!(!text.contains("never-preview-this") && !text.contains("nor-this"));
        }
        let (_, failed) = describe_output(Err(CommandExecutionError::new(
            CommandExecutionErrorKind::Io,
            "fixture_credential_private_error",
        )));
        assert!(failed["stdout_preview"].is_null() && failed["stderr_preview"].is_null());
        for bytes in [
            vec![0xff],
            vec![0xff, 0xfe, 0x20],
            vec![0xff, 0xfe, 0x00, 0xd8],
            b"mixed\0unsupported".to_vec(),
        ] {
            let (_, parsed) = describe_output(Ok(output(bytes, Vec::new(), -1)));
            assert!(parsed["stdout_preview"].is_null());
        }
    }

    fn encode(text: &str, encoding: usize) -> Vec<u8> {
        match encoding {
            0 => text.as_bytes().to_vec(),
            1 => text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
            _ => text.encode_utf16().flat_map(u16::to_be_bytes).collect(),
        }
    }

    #[test]
    fn public_previews_scan_full_both_streams_all_encodings_before_cropping() {
        for encoding in 0..3 {
            for stderr in [false, true] {
                for after_cutoff in [false, true] {
                    let secret = format!(
                        "{}fixture_credential_hidden",
                        if after_cutoff {
                            "x".repeat(300)
                        } else {
                            String::new()
                        }
                    );
                    let bytes = encode(&secret, encoding);
                    let mut result =
                        output(b"public-benign".to_vec(), b"public-benign".to_vec(), -1);
                    if stderr {
                        result.output.stderr = bytes;
                    } else {
                        result.output.stdout = [START, bytes.as_slice()].concat();
                    }
                    let (text, parsed) = describe_output(Ok(result));
                    assert_eq!(parsed["fixture_secret_suppressed"], true);
                    assert!(
                        parsed["stdout_preview"].is_null() && parsed["stderr_preview"].is_null()
                    );
                    assert!(
                        !text.contains("public-benign")
                            && !text.contains("fixture_credential_hidden")
                    );
                }
            }
        }
    }

    #[test]
    fn public_previews_decode_only_exact_start_prefix_and_native_error_framing() {
        for encoding in 0..3 {
            for start in [false, true] {
                for bom in [false, true] {
                    let mut body = if bom {
                        match encoding {
                            0 => vec![0xef, 0xbb, 0xbf],
                            1 => vec![0xff, 0xfe],
                            _ => vec![0xfe, 0xff],
                        }
                    } else {
                        Vec::new()
                    };
                    body.extend(encode("synthetic public transport error\r\n", encoding));
                    let stdout = if start {
                        [START, body.as_slice()].concat()
                    } else {
                        body
                    };
                    let (_, parsed) = describe_output(Ok(output(stdout, Vec::new(), -1)));
                    assert_eq!(parsed["start_marker"], start);
                    assert_eq!(
                        parsed["stdout_preview"],
                        "synthetic public transport error\r\n"
                    );
                }
            }
        }
        let bytes = [
            b"mt-public-start\r\n".as_slice(),
            encode("native error", 1).as_slice(),
        ]
        .concat();
        let (_, parsed) = describe_output(Ok(output(bytes, Vec::new(), -1)));
        assert_eq!(parsed["start_marker"], false);
        assert!(parsed["stdout_preview"].is_null());
    }

    #[test]
    fn public_previews_and_total_json_are_bounded_and_escape_controls() {
        let directional = [
            '\u{061c}', '\u{200e}', '\u{200f}', '\u{2028}', '\u{2029}', '\u{202a}', '\u{202b}',
            '\u{202c}', '\u{202d}', '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
        ];
        let content = format!(
            "quote=\" slash=\\ newline=\n tab=\t esc=\u{1b} csi=\u{9b} bidi={}{}",
            directional.iter().copied().collect::<String>(),
            "\u{754c}".repeat(300)
        );
        let (text, parsed) =
            describe_output(Ok(output(content.as_bytes().to_vec(), Vec::new(), -1)));
        assert_eq!(
            parsed["stdout_preview"].as_str().unwrap().chars().count(),
            256
        );
        assert!(!text.contains('\u{1b}') && !text.contains('\u{9b}'));
        for ch in directional {
            assert!(!text.contains(ch));
            assert!(text.contains(&format!("\\u{:04x}", ch as u32)));
        }
        let (_, parsed) = describe_output(Ok(output(encode("public\0text", 1), Vec::new(), -1)));
        assert_eq!(parsed["stdout_preview"], "public\0text");
        let (text, _) = describe_output(Ok(output(vec![7; 256], vec![7; 256], -1)));
        assert!(text.len() <= 4096);
    }

    #[test]
    fn public_two_row_privacy_and_total_budget_keep_metadata_when_previews_are_dropped() {
        let report = run_rows(|row| RowReport {
            producer: Some(Reply::received(
                Stage::Producer,
                Ok(output(vec![7; 256], vec![7; 256], -1)),
            )),
            timing: Timing {
                release: Some(Release::Deadline),
                released_us: Some(u64::MAX),
                ..Default::default()
            },
            ..RowReport::new(row)
        });
        let rendered = report.describe();
        assert!(rendered.len() <= 4096);
        assert!(!rendered.chars().any(char::is_control));
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(value["preview_limit"], true);
        assert_eq!(value["inconclusive"], true);
        assert_eq!(value["rows"].as_array().unwrap().len(), 2);
        for (index, row) in value["rows"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                row["row"],
                if index == 0 {
                    "Immediate"
                } else {
                    "AfterProducer"
                }
            );
            assert_eq!(row["producer"]["exit"], -1);
            assert_eq!(row["timing"]["release"], "Deadline");
            assert!(row["stdout_preview"].is_null() && row["stderr_preview"].is_null());
        }
        for encoding in 0..3 {
            for secret_row in ROWS {
                for stderr in [false, true] {
                    for after_cutoff in [false, true] {
                        for invalid in 0..8 {
                            let report = run_rows(|row| {
                                let mut result = output(
                                    b"benign-public".to_vec(),
                                    b"benign-public".to_vec(),
                                    -1,
                                );
                                if row == secret_row {
                                    let secret = encode(
                                        &format!(
                                            "{}fixture_credential_hidden",
                                            "x".repeat(if after_cutoff { 300 } else { 0 })
                                        ),
                                        encoding,
                                    );
                                    if stderr {
                                        result.output.stderr = secret;
                                    } else {
                                        result.output.stdout = [START, secret.as_slice()].concat();
                                    }
                                    match invalid {
                                        1 => result.output.stdout_truncated = true,
                                        2 => result.output.stderr_truncated = true,
                                        3 => result.output.timed_out = true,
                                        4 => result.observed_connection_epoch = Some(1),
                                        5 => result.output.exit_code = None,
                                        6 => result.output.stdout.resize(4097, b'x'),
                                        7 => result.output.stderr.resize(4097, b'x'),
                                        _ => {}
                                    }
                                }
                                let reply = Reply::received(Stage::Producer, Ok(result));
                                if row == secret_row {
                                    assert!(reply.fixture_credential);
                                    assert_eq!(reply.producer_output.is_none(), invalid != 0);
                                }
                                RowReport {
                                    producer: Some(reply),
                                    ..RowReport::new(row)
                                }
                            });
                            let rendered = report.describe();
                            assert!(rendered.len() <= 4096);
                            assert!(
                                !rendered.contains("benign-public")
                                    && !rendered.contains("fixture_credential_hidden")
                            );
                            let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();
                            assert_eq!(value["fixture_secret_suppressed"], true);
                            for row in value["rows"].as_array().unwrap() {
                                assert!(
                                    row["stdout_preview"].is_null()
                                        && row["stderr_preview"].is_null()
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn public_retirement_api_failure_and_missing_or_late_evidence_remain_inconclusive() {
        use crate::execution_host::{TasksWslActiveProcesses, TasksWslRetirement};

        let failed = TasksWslRetirement {
            before_us: u128::MAX,
            after_us: u128::MAX,
            active_processes: TasksWslActiveProcesses::QueryFailed,
            roots: None,
            succeeded: false,
        };
        let mut row = RowReport {
            readiness: Some(Readiness {
                failure: Some(Failure::Io),
                retirement: TasksWslRetirementTrace {
                    calls: 2,
                    first: Some(failed),
                    last: Some(failed),
                },
                ..Default::default()
            }),
            ..RowReport::new(Row::AfterProducer)
        };
        assert_eq!(row.timing_issue(), Some(TimingIssue::RetirementFailed));
        row.readiness.as_mut().unwrap().retirement = TasksWslRetirementTrace::default();
        assert_eq!(row.timing_issue(), Some(TimingIssue::ProbeFailure));
        row.readiness.as_mut().unwrap().failure = None;
        assert_eq!(row.timing_issue(), Some(TimingIssue::RetirementUnavailable));
        row.readiness.as_mut().unwrap().retirement = TasksWslRetirementTrace {
            calls: 1,
            first: Some(TasksWslRetirement {
                succeeded: true,
                ..failed
            }),
            last: Some(TasksWslRetirement {
                succeeded: true,
                ..failed
            }),
        };
        for release in [
            Release::Late,
            Release::Deadline,
            Release::Bypassed,
            Release::ScopeRejected,
            Release::ProducerAborted,
        ] {
            row.timing.release = Some(release);
            row.timing.eligible = true;
            assert_eq!(row.timing_issue(), Some(TimingIssue::Release));
        }
        row.timing.release = Some(Release::ProducerReturned);
        assert_eq!(row.timing_issue(), Some(TimingIssue::NonOverlapping));
    }
}

#[cfg(windows)]
#[test]
fn wsl_readiness_diagnostics_retain_earlier_false_probes_in_fixed_storage() {
    use crate::execution_host::{
        TasksWslActiveProcesses, TasksWslRetirement, TasksWslRootLiveness, TasksWslRootMembership,
        TasksWslRootState, TasksWslRootsObservation,
    };

    let first = TasksWslRetirement {
        before_us: 2,
        after_us: 3,
        active_processes: TasksWslActiveProcesses::Count(1),
        roots: Some(TasksWslRootsObservation::Roots {
            private: TasksWslRootState {
                membership: TasksWslRootMembership::NotInJob,
                liveness: TasksWslRootLiveness::Alive,
            },
            readiness: TasksWslRootState {
                membership: TasksWslRootMembership::InJob,
                liveness: TasksWslRootLiveness::Exited,
            },
        }),
        succeeded: true,
    };
    let retirement = TasksWslRetirementTrace {
        calls: 1,
        first: Some(first),
        last: Some(first),
    };
    let mut observation = WslReadinessObservation::default();
    observation.record_probe(false, 0, 5, retirement);
    observation.record_probe(false, 10, 15, TasksWslRetirementTrace::default());
    assert_eq!(observation.probe_count, 2);
    assert_eq!(observation.first_probe_started_us, 0);
    assert_eq!(observation.first_probe_returned_us, 5);
    assert_eq!(observation.first_retirement, retirement);
    assert!(!observation.ready);
    observation.record_probe(true, 20, 25, TasksWslRetirementTrace::default());
    observation.cancel_us = 30;
    assert_eq!(observation.first_retirement, retirement);
    assert_eq!(observation.retirement, TasksWslRetirementTrace::default());
    assert!(
        observation.describe().contains(
            "ready=true probe_count=3 first_probe_started_us=0 first_probe_returned_us=5"
        )
    );
    assert!(
        observation
            .describe()
            .contains("probe_started_us=20 probe_returned_us=25")
    );
    assert!(observation.describe().contains("cancel_us=30"));
    assert!(
        observation
            .describe()
            .contains("private: TasksWslRootState { membership: NotInJob, liveness: Alive }")
    );
    assert!(
        observation
            .describe()
            .contains("readiness: TasksWslRootState { membership: InJob, liveness: Exited }")
    );

    observation.probe_count = u32::MAX;
    observation.record_probe(
        false,
        u128::MAX - 1,
        u128::MAX,
        TasksWslRetirementTrace::default(),
    );
    assert_eq!(observation.probe_count, u32::MAX);
    assert_eq!(observation.first_probe_started_us, 0);
    assert_eq!(observation.first_probe_returned_us, 5);
    assert_eq!(observation.first_retirement, retirement);
    assert!(!observation.ready);
    assert!(observation.describe().len() < 2048);
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
                WslPreludeStage::SingleEmptyArgv,
                WslPreludeStage::MultipleEmptyArgv,
                WslPreludeStage::MissingCwd,
                WslPreludeStage::NonDirectoryCwd,
            ] {
                let diagnostic = wsl_cwd_diagnostic(pre_project, program, stage, &output);
                assert!(
                    diagnostic
                        .starts_with(&format!("mode={mode} program_kind={kind} stage={stage:?} "))
                );
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
        (
            WslArgvDiscriminator::Hostile,
            "literal '\";$(printf injected)",
        ),
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
    assert!(
        diagnostic.contains(
            "baseline mode=Project program_kind=Relative stage=LiteralArgv exit=Some(-1)"
        )
    );
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
        let diagnostic =
            wsl_require_literal_argv(true, WslCwdProgram::Absolute, baseline, b"expected", |_| {
                probes += 1;
                Err(CommandExecutionError::new(
                    CommandExecutionErrorKind::Io,
                    "fixture_credential_probe_error",
                ))
            })
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

    let mut public_attempt = wsl_public_comparison::Attempt::default();
    for lifecycle in [
        WslLifecycleCase::PipeDescendant,
        WslLifecycleCase::DataTimeout,
        WslLifecycleCase::DataCancel,
        WslLifecycleCase::LookupTimeout,
        WslLifecycleCase::LookupCancel,
    ] {
        let (login, cancel, expected) = match lifecycle {
            WslLifecycleCase::PipeDescendant => ("PipeDescendant", false, None),
            WslLifecycleCase::DataTimeout => ("Slow", false, Some(AccountExecutionError::TimedOut)),
            WslLifecycleCase::DataCancel => ("Slow", true, Some(AccountExecutionError::Cancelled)),
            WslLifecycleCase::LookupTimeout => {
                ("LookupSlow", false, Some(AccountExecutionError::TimedOut))
            }
            WslLifecycleCase::LookupCancel => {
                ("LookupSlow", true, Some(AccountExecutionError::Cancelled))
            }
        };
        let case = fixture.case();
        let cancellation = AccountCancellation::default();
        let success = expected.is_none();
        let bounded = AccountExecutionControl::new(
            Duration::from_secs(if success || cancel { 30 } else { 5 }),
            cancellation.clone(),
            None,
        )
        .unwrap();
        // Capture and the concurrent guarded readiness command share one clock.
        let roots = TasksWslRoots::default();
        let started = Instant::now();
        let canceller =
            cancel.then(|| case.cancel_after_start(cancellation, started, roots.clone()));
        let (result, diagnostics) = tasks_wsl_root_scope(&roots, TasksWslRootRole::Private, || {
            process::trace_capture(started, || {
                execute_selected_account(&case.source, &selected("github.com", login), &bounded)
            })
        });
        let returned_us = started.elapsed().as_micros();
        let cancelled_at_return = bounded.cancellation().is_cancelled();
        let readiness = canceller.map(|canceller| {
            canceller.join().unwrap_or_else(|_| {
                panic!(
                    "WSL readiness thread failed: case={lifecycle:?} returned_us={returned_us} cancelled_at_return={cancelled_at_return}\n{}",
                    diagnostics.describe()
                )
            })
        });
        let actual = result.result.as_ref().err().copied();
        let public_report =
            public_attempt.after_failure(&fixture, lifecycle, actual, cancelled_at_return);
        let diagnostic = || {
            let readiness = readiness
                .as_ref()
                .map(WslReadinessObservation::describe)
                .unwrap_or_else(|| "none".into());
            format!(
                "case={lifecycle:?} result={actual:?} returned_us={returned_us} cancelled_at_return={cancelled_at_return} {readiness}\n{}\n{}",
                diagnostics.describe(),
                public_report
                    .as_ref()
                    .map(wsl_public_comparison::Report::describe)
                    .unwrap_or_else(|| "public_comparison=not-run".into())
            )
        };
        assert!(
            readiness.as_ref().is_none_or(|readiness| readiness.ready),
            "WSL descendant did not become ready: {}",
            diagnostic()
        );
        assert!(result.observed_connection_epoch.is_none());
        assert!(actual == expected, "WSL lifecycle failed: {}", diagnostic());
        case.assert_retired();
    }
    wsl_containment::assert_peer_survival(&fixture);
}

#[cfg(windows)]
mod wsl_containment {
    use super::*;
    use crate::execution_host::{
        PreProjectLocalContext, execute_host_command, execute_pre_project_local_command,
    };

    const PEER_SLEEP: Duration = Duration::from_secs(15);
    const PEER_TIMEOUT: Duration = Duration::from_secs(20);
    const SHORT_TIMEOUT: Duration = Duration::from_secs(5);
    const PRIVATE_CANCEL_TIMEOUT: Duration = Duration::from_secs(15);
    const PRIVATE_TIMEOUT: Duration = Duration::from_secs(5);
    const READINESS_TIMEOUT: Duration = Duration::from_secs(10);
    const READINESS_INTERVAL: Duration = Duration::from_millis(50);
    const MAX_READINESS_PROBES: usize = 200;
    const PEER_SCRIPT: &str = r"printf '%s\n' mt-containment-start && : > peer-ready && /usr/bin/sleep 15 && printf '%s\n' mt-containment-end";
    const PEER_OUTPUT: &[u8] = b"mt-containment-start\nmt-containment-end\n";
    const COMPLETE_SCRIPT: &str = r": > short-ready && exec /usr/bin/sleep 2";
    const TIMEOUT_SCRIPT: &str = r": > short-ready && exec /usr/bin/sleep 10";

    #[derive(Clone, Copy, Debug)]
    enum Route {
        Project,
        PreProject,
    }

    impl Route {
        fn execute(
            self,
            fixture: &WslFixture,
            plan: &CommandPlan,
            timeout: Duration,
        ) -> Result<CommandOutput, CommandExecutionError> {
            match self {
                Self::Project => {
                    let result = execute_host_command(
                        &fixture.source,
                        plan,
                        timeout,
                        WSL_FIXTURE_OUTPUT_CAP,
                    )?;
                    assert!(result.observed_connection_epoch.is_none());
                    Ok(result.output)
                }
                Self::PreProject => {
                    let ExecutionBackend::Wsl { distro } = &fixture.source.backend else {
                        panic!("WSL containment requires its captured distribution")
                    };
                    execute_pre_project_local_command(
                        &PreProjectLocalContext::Wsl {
                            distro: distro.clone(),
                            cwd: fixture.source.canonical_path.clone(),
                        },
                        plan,
                        timeout,
                        WSL_FIXTURE_OUTPUT_CAP,
                    )
                }
            }
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum ShortClient {
        Complete,
        Timeout,
    }

    impl ShortClient {
        fn plan(self) -> CommandPlan {
            match self {
                Self::Complete => CommandPlan::new("/bin/sh", ["-c", COMPLETE_SCRIPT]),
                Self::Timeout => CommandPlan::new("/bin/sh", ["-c", TIMEOUT_SCRIPT]),
            }
        }

        fn overlap_limit(self) -> Duration {
            match self {
                Self::Complete => Duration::from_secs(2),
                Self::Timeout => SHORT_TIMEOUT,
            }
        }
    }

    fn owns_root(fixture: &WslFixture) -> bool {
        matches!(
            (&fixture.source.backend, &fixture.attested_distro),
            (ExecutionBackend::Wsl { distro }, Some(attested)) if distro == attested
        ) && fixture.source.canonical_path == "/mini-term-fixture"
            && fixture.source.root_source_path == "/mini-term-fixture"
    }

    struct Worker<T> {
        thread: Option<std::thread::JoinHandle<T>>,
        abort_request: Option<AccountCancellation>,
    }

    impl<T: Send + 'static> Worker<T> {
        fn spawn(
            abort_request: Option<AccountCancellation>,
            action: impl FnOnce() -> T + Send + 'static,
        ) -> Self {
            let thread = std::thread::Builder::new()
                .spawn(action)
                .unwrap_or_else(|_| panic!("WSL containment worker could not start"));
            Self {
                thread: Some(thread),
                abort_request,
            }
        }

        fn is_finished(&self) -> bool {
            self.thread
                .as_ref()
                .is_none_or(|thread| thread.is_finished())
        }

        fn join(&mut self) -> T {
            self.thread
                .take()
                .expect("WSL containment worker already joined")
                .join()
                .unwrap_or_else(|_| panic!("WSL containment worker panicked"))
        }
    }

    impl<T> Drop for Worker<T> {
        fn drop(&mut self) {
            if let Some(thread) = self.thread.take() {
                if let Some(cancellation) = &self.abort_request {
                    cancellation.cancel();
                }
                // Public commands have no cancellation API: their fixed sleep
                // and runner deadline bound this join, including during unwind.
                let _ = thread.join();
            }
        }
    }

    fn wait_ready<T: Send + 'static>(case: &WslFixture, name: &str, worker: &Worker<T>) {
        let deadline = Instant::now() + READINESS_TIMEOUT;
        for _ in 0..MAX_READINESS_PROBES {
            assert!(
                Instant::now() < deadline && !worker.is_finished(),
                "WSL containment worker did not become ready while running"
            );
            if case.exists(name) {
                return;
            }
            std::thread::sleep(READINESS_INTERVAL);
        }
        panic!("WSL containment readiness probe limit exceeded");
    }

    fn peer_plan() -> CommandPlan {
        CommandPlan::new("/bin/sh", ["-c", PEER_SCRIPT])
    }

    type Peer = Worker<Result<CommandOutput, CommandExecutionError>>;

    fn start_peer(
        case: &WslFixture,
        abort_request: Option<AccountCancellation>,
    ) -> (Instant, Peer) {
        let source = case.clone();
        let started = Instant::now();
        // Abort the earlier private request before joining this peer if its
        // startup/readiness or the owning case unwinds.
        let peer = Worker::spawn(abort_request, move || {
            Route::Project.execute(&source, &peer_plan(), PEER_TIMEOUT)
        });
        wait_ready(case, "peer-ready", &peer);
        (started, peer)
    }

    fn assert_bounded_output(output: &CommandOutput) {
        assert!(
            !output.stdout_truncated
                && !output.stderr_truncated
                && output.stdout.len() <= WSL_FIXTURE_OUTPUT_CAP
                && output.stderr.len() <= WSL_FIXTURE_OUTPUT_CAP,
            "WSL containment output exceeded its capture bounds"
        );
    }

    fn finish_peer(started: Instant, mut peer: Peer) {
        // Readiness proved guest entry; before the fixed sleep can elapse,
        // require the independent client still to be pending after retirement.
        assert!(
            started.elapsed() < PEER_SLEEP && !peer.is_finished(),
            "WSL containment did not retain an overlapping public peer"
        );
        let output = peer.join().unwrap_or_else(|error| {
            panic!("WSL containment peer dispatch failed: {:?}", error.kind)
        });
        assert_bounded_output(&output);
        assert!(
            output.exit_code == Some(0)
                && !output.timed_out
                && output.stdout == PEER_OUTPUT
                && output.stderr.is_empty(),
            "WSL containment peer did not complete with its exact public markers: exit={:?} timed_out={} stdout_bytes={} stderr_bytes={}",
            output.exit_code,
            output.timed_out,
            output.stdout.len(),
            output.stderr.len()
        );
    }

    pub(super) fn assert_peer_survival(fixture: &WslFixture) {
        assert!(owns_root(fixture), "WSL containment source is not attested");
        for route in [Route::Project, Route::PreProject] {
            for client in [ShortClient::Complete, ShortClient::Timeout] {
                let case = fixture.case();
                let source = case.clone();
                let short_started = Instant::now();
                let mut short = Worker::spawn(None, move || {
                    route.execute(&source, &client.plan(), SHORT_TIMEOUT)
                });
                wait_ready(&case, "short-ready", &short);
                let (started, peer) = start_peer(&case, None);
                assert!(
                    short_started.elapsed() < client.overlap_limit()
                        && !short.is_finished()
                        && !peer.is_finished(),
                    "WSL containment short client did not overlap its later peer: {route:?} {client:?}"
                );
                let output = short.join().unwrap_or_else(|error| {
                    panic!(
                        "WSL containment short client failed: {route:?} {client:?} {:?}",
                        error.kind
                    )
                });
                assert_bounded_output(&output);
                assert!(output.stdout.is_empty() && output.stderr.is_empty());
                let matched = match client {
                    ShortClient::Complete => output.exit_code == Some(0) && !output.timed_out,
                    ShortClient::Timeout => output.timed_out && output.exit_code != Some(0),
                };
                assert!(
                    matched,
                    "WSL containment short result failed: {route:?} {client:?} exit={:?} timed_out={}",
                    output.exit_code, output.timed_out
                );
                finish_peer(started, peer);
            }
        }

        for lifecycle in [
            WslLifecycleCase::DataCancel,
            WslLifecycleCase::DataTimeout,
            WslLifecycleCase::LookupCancel,
            WslLifecycleCase::LookupTimeout,
        ] {
            let (login, cancel, expected) = match lifecycle {
                WslLifecycleCase::DataCancel => ("Slow", true, AccountExecutionError::Cancelled),
                WslLifecycleCase::DataTimeout => ("Slow", false, AccountExecutionError::TimedOut),
                WslLifecycleCase::LookupCancel => {
                    ("LookupSlow", true, AccountExecutionError::Cancelled)
                }
                WslLifecycleCase::LookupTimeout => {
                    ("LookupSlow", false, AccountExecutionError::TimedOut)
                }
                WslLifecycleCase::PipeDescendant => unreachable!(),
            };
            let peer_case = fixture.case();
            let case = fixture.case();
            let cancellation = AccountCancellation::default();
            let timeout = if cancel {
                PRIVATE_CANCEL_TIMEOUT
            } else {
                PRIVATE_TIMEOUT
            };
            let bounded =
                AccountExecutionControl::new(timeout, cancellation.clone(), None).unwrap();
            let source = case.source.clone();
            let request_started = Instant::now();
            let mut request = Worker::spawn(Some(cancellation.clone()), move || {
                execute_selected_account(&source, &selected("github.com", login), &bounded)
            });
            wait_ready(&case, "ready", &request);
            let (started, peer) = start_peer(&peer_case, Some(cancellation.clone()));
            assert!(
                request_started.elapsed() < timeout
                    && !request.is_finished()
                    && !peer.is_finished(),
                "WSL containment private request did not overlap its later peer: {lifecycle:?}"
            );
            if cancel {
                cancellation.cancel();
            }
            let result = request.join();
            assert!(result.observed_connection_epoch.is_none());
            // Readiness excludes pre-dispatch stops. The real WSL executor
            // requires the strict host cleanup acknowledgement for these errors.
            let actual = result.result.as_ref().err().copied();
            assert!(
                actual == Some(expected),
                "WSL containment private cleanup failed: {lifecycle:?} result={actual:?}"
            );
            case.assert_retired();
            finish_peer(started, peer);
        }
    }

    #[test]
    fn containment_requires_attested_root_before_case_creation() {
        let mut fixture = WslFixture {
            source: snapshot(Path::new("/mini-term-fixture")),
            attested_distro: Some("owned-fixture".into()),
        };
        assert!(!owns_root(&fixture));
        fixture.source.backend = ExecutionBackend::Wsl {
            distro: "owned-fixture".into(),
        };
        assert!(owns_root(&fixture));
        let mut changed = fixture.clone();
        changed.attested_distro = None;
        assert!(!owns_root(&changed));
        changed.attested_distro = Some("different-fixture".into());
        assert!(!owns_root(&changed));
        let mut changed = fixture.clone();
        changed.source.canonical_path.push_str("/cases/reused");
        assert!(!owns_root(&changed));
        fixture.source.root_source_path.push_str("/different");
        assert!(!owns_root(&fixture));
    }

    #[test]
    fn containment_public_plans_and_limits_are_fixed() {
        assert_eq!(peer_plan().program, "/bin/sh");
        assert_eq!(
            peer_plan().args,
            [
                "-c",
                r"printf '%s\n' mt-containment-start && : > peer-ready && /usr/bin/sleep 15 && printf '%s\n' mt-containment-end",
            ]
        );
        assert_eq!(PEER_SLEEP, Duration::from_secs(15));
        assert_eq!(PEER_TIMEOUT, Duration::from_secs(20));
        assert_eq!(PEER_OUTPUT, b"mt-containment-start\nmt-containment-end\n");
        assert_eq!(ShortClient::Complete.plan().program, "/bin/sh");
        assert_eq!(
            ShortClient::Complete.plan().args,
            ["-c", ": > short-ready && exec /usr/bin/sleep 2"]
        );
        assert_eq!(
            ShortClient::Complete.overlap_limit(),
            Duration::from_secs(2)
        );
        assert_eq!(ShortClient::Timeout.plan().program, "/bin/sh");
        assert_eq!(
            ShortClient::Timeout.plan().args,
            ["-c", ": > short-ready && exec /usr/bin/sleep 10"]
        );
        assert_eq!(ShortClient::Timeout.overlap_limit(), Duration::from_secs(5));
        assert_eq!(SHORT_TIMEOUT, Duration::from_secs(5));
        assert_eq!(PRIVATE_CANCEL_TIMEOUT, Duration::from_secs(15));
        assert_eq!(PRIVATE_TIMEOUT, Duration::from_secs(5));
        assert_eq!(WSL_FIXTURE_OUTPUT_CAP, 4096);
        assert_eq!(READINESS_TIMEOUT, Duration::from_secs(10));
        assert_eq!(READINESS_INTERVAL, Duration::from_millis(50));
        assert_eq!(MAX_READINESS_PROBES, 200);
    }

    #[test]
    fn containment_worker_ownership_joins_and_cancels_on_unwind() {
        let cancellation = AccountCancellation::default();
        let mut worker = Worker::spawn(Some(cancellation.clone()), || 7);
        assert_eq!(worker.join(), 7);
        drop(worker);
        assert!(!cancellation.is_cancelled());

        let finished = Arc::new(AtomicBool::new(false));
        let completed = finished.clone();
        {
            let _worker = Worker::spawn(None, move || {
                completed.store(true, Ordering::Release);
            });
        }
        assert!(finished.load(Ordering::Acquire));

        let observed_cancel = cancellation.clone();
        let cancelled_before_join = Arc::new(AtomicBool::new(false));
        let observed_order = cancelled_before_join.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _worker: Worker<()> = Worker::spawn(Some(cancellation.clone()), move || {
                // Drop must signal before joining, even if that worker panics.
                let deadline = Instant::now() + Duration::from_secs(1);
                while !observed_cancel.is_cancelled() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(1));
                }
                observed_order.store(observed_cancel.is_cancelled(), Ordering::Release);
                panic!("synthetic containment worker unwind");
            });
            panic!("synthetic containment owner unwind");
        }));
        assert!(result.is_err());
        assert!(cancellation.is_cancelled());
        assert!(cancelled_before_join.load(Ordering::Acquire));
    }
}
