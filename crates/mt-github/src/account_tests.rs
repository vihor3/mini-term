use serde_json::{Value, json};

use crate::*;

fn output(text: &str) -> CommandOutput {
    CommandOutput {
        stdout: text.as_bytes().to_vec(),
        exit_code: Some(0),
        ..CommandOutput::default()
    }
}

fn failed(diagnostic: &str) -> CommandOutput {
    CommandOutput {
        stderr: diagnostic.as_bytes().to_vec(),
        exit_code: Some(1),
        ..CommandOutput::default()
    }
}

fn account(login: &str, active: bool) -> Value {
    json!({
        "state": "success",
        "active": active,
        "host": "github.com",
        "login": login,
        "tokenSource": "keyring",
        "scopes": "repo, read:org",
        "gitProtocol": "https"
    })
}

fn account_failure(login: &str, error: &str) -> Value {
    let mut entry = account(login, false);
    entry["state"] = json!("error");
    entry["error"] = json!(error);
    entry
}

fn enumeration(entries: Vec<Value>) -> CommandOutput {
    output(&json!({"hosts": {"github.com": entries}}).to_string())
}

fn accounts(entries: Vec<Value>) -> KnownGitHubAccounts {
    parse_known_accounts("github.com", &enumeration(entries)).unwrap()
}

#[test]
fn status_uses_official_host_map_schema_and_keeps_broken_peers() {
    let mut broken = account_failure(
        "MonaLisa",
        "HTTP 401: Bad credentials (https://api.github.com/)",
    );
    broken["active"] = json!(true);
    let known = accounts(vec![account("Octocat", false), broken]);
    assert_eq!(known.host(), "github.com");
    assert_eq!(known.accounts().len(), 2);
    assert_eq!(known.accounts()[0].identity().login(), "monalisa");
    assert_eq!(known.accounts()[0].login(), "MonaLisa");
    assert!(known.accounts()[0].is_active());
    assert_eq!(known.accounts()[0].state(), KnownAccountState::Error);
    assert_eq!(
        known.accounts()[0].problem(),
        Some(AccountError::AuthenticationFailed)
    );
    assert_eq!(known.accounts()[1].state(), KnownAccountState::Success);
    assert_eq!(known.accounts()[1].problem(), None);
    assert!(known.initial_selection().is_none());

    let selected = GitHubAccountIdentity::new("github.com", "MONALISA").unwrap();
    assert_eq!(
        known.find(&selected).unwrap().problem(),
        Some(AccountError::AuthenticationFailed)
    );
}

#[test]
fn success_sole_none_and_multiple_have_no_active_account_fallback() {
    let sole = accounts(vec![account("Octocat", false)]);
    assert_eq!(
        sole.initial_selection().unwrap().identity().login(),
        "octocat"
    );
    let multiple = accounts(vec![account("Octocat", true), account("MonaLisa", false)]);
    assert!(multiple.initial_selection().is_none());
    let unavailable = accounts(vec![account_failure("Octocat", "Bad credentials")]);
    assert!(unavailable.initial_selection().is_none());

    for text in [r#"{"hosts":{}}"#, r#"{"hosts":{"github.com":[]}}"#] {
        let mut empty = output(text);
        empty.stderr = b"You are not logged into any accounts on github.com".to_vec();
        let known = parse_known_accounts("github.com", &empty).unwrap();
        assert!(known.accounts().is_empty());
        assert!(known.initial_selection().is_none());
    }

    let missing = GitHubAccountIdentity::new("github.com", "Missing").unwrap();
    assert_eq!(
        sole.find(&missing),
        Err(AccountError::SelectedAccountUnavailable)
    );
    let wrong_host = GitHubAccountIdentity::new("github.example.com", "Octocat").unwrap();
    assert_eq!(
        sole.find(&wrong_host),
        Err(AccountError::WrongHostOrAccount)
    );
}

#[test]
fn active_changes_do_not_retarget_saved_identity_or_plans() {
    let before = accounts(vec![account("Alice", true), account("Bob", false)]);
    let after = accounts(vec![account("Bob", true), account("Alice", false)]);
    let alice = GitHubAccountIdentity::new("GitHub.COM.", "ALICE").unwrap();
    let bob = GitHubAccountIdentity::new("github.com", "bob").unwrap();
    for selected in [alice, bob] {
        let old_plan = SelectedAccountRequestPlan::identity(before.find(&selected).unwrap());
        let new_plan = SelectedAccountRequestPlan::identity(after.find(&selected).unwrap());
        assert_eq!(old_plan, new_plan);
        assert_eq!(new_plan.account(), &selected);
    }
}

#[test]
fn identity_normalizes_comparison_without_changing_lookup_spelling() {
    assert_eq!(
        GitHubAccountIdentity::new("GitHub.COM.", "Mona-Cat_OCTO").unwrap(),
        GitHubAccountIdentity::new("github.com", "mona-cat_octo").unwrap()
    );
    let known = accounts(vec![account("Mona-Cat_OCTO", true)]);
    let plan = SelectedAccountRequestPlan::identity(&known.accounts()[0]);
    assert_eq!(plan.lookup_login(), "Mona-Cat_OCTO");
    assert_eq!(plan.account().login(), "mona-cat_octo");
    assert_eq!(plan.account().host(), "github.com");
    assert!(GitHubAccountIdentity::new("ghe.example.com", "octo_admin").is_ok());
    assert!(GitHubAccountIdentity::new("org.ghe.com", "Alice").is_ok());
    assert!(
        GitHubAccountIdentity::new("github.com", &"a".repeat(ACCOUNT_LOGIN_LIMIT)).is_ok()
    );
}

#[test]
fn invalid_and_hostile_hosts_and_logins_are_rejected_without_echoing_them() {
    for host in [
        "",
        ".github.com",
        "github..com",
        "github.com..",
        "-github.com",
        "github-.com",
        "github.-example.com",
        "github.example-.com",
        "github.com:443",
        "https://github.com",
        "github.com/path",
        "github.com?token=sentinel",
        "git@github.com",
        "github.com;id",
        "$(id).github.com",
        "github.com\0",
        "github.com\n",
        " github.com",
        "github.com ",
        "github_com",
        "github\\com",
    ] {
        assert_eq!(
            GitHubAccountIdentity::new(host, "Alice"),
            Err(AccountError::InvalidHost)
        );
        assert_eq!(known_accounts_plan(host), Err(AccountError::InvalidHost));
    }
    for host in [format!("{}.com", "a".repeat(64)), "a.".repeat(128)] {
        assert_eq!(known_accounts_plan(&host), Err(AccountError::InvalidHost));
    }
    for login in [
        "",
        "-alice",
        "alice-",
        "al--ice",
        "alice;id",
        "$(id)",
        "a'b",
        "a\"b",
        "a\\b",
        "a/b",
        "a b",
        " Alice",
        "Alice\n",
        "Alice\0",
        "--user=other",
        "a@b",
        "a.b",
        "a__org",
        "a_",
        "_admin",
        "a_b_c",
        "a_-org",
        "a_org-",
    ] {
        assert_eq!(
            GitHubAccountIdentity::new("github.com", login),
            Err(AccountError::InvalidLogin)
        );
        assert_eq!(
            parse_known_accounts("github.com", &enumeration(vec![account(login, true)])),
            Err(AccountError::InvalidLogin)
        );
    }
    assert_eq!(
        GitHubAccountIdentity::new("github.com", &"a".repeat(ACCOUNT_LOGIN_LIMIT + 1)),
        Err(AccountError::InvalidLogin)
    );
}

#[test]
fn extra_secret_fields_and_raw_errors_never_survive_parser_or_plan_formatting() {
    const SENTINEL: &str = "DO_NOT_RETAIN_SECRET_123";
    let mut good = account("Alice", true);
    good["token"] = json!(SENTINEL);
    good["tokenSource"] = json!(format!("/private/{SENTINEL}/hosts.yml"));
    good["scopes"] = json!(SENTINEL);
    good["gitProtocol"] = json!(SENTINEL);
    good["extra"] = json!({"oauth_token": SENTINEL});
    let broken = account_failure("Bob", &format!("Bad credentials: {SENTINEL}"));
    let mut raw = json!({"hosts": {"github.com": [good, broken]}});
    raw["token"] = json!(SENTINEL);
    let known = parse_known_accounts("github.com", &output(&raw.to_string())).unwrap();
    assert!(!format!("{known:?}").contains(SENTINEL));
    let request = SelectedAccountRequestPlan::identity(&known.accounts()[0]);
    assert!(!format!("{request:?} {:?}", request.data_plan()).contains(SENTINEL));
    let error = known.accounts()[1].problem().unwrap();
    assert!(!format!("{error:?} {error}").contains(SENTINEL));
    assert_eq!(error, AccountError::AuthenticationFailed);

    let malformed = output(&format!(
        r#"{{"hosts":{{"github.com":[{{"login":"Alice","state":"{SENTINEL}"}}]}}}}"#
    ));
    let error = parse_known_accounts("github.com", &malformed).unwrap_err();
    assert_eq!(error, AccountError::MalformedResponse);
    assert!(!format!("{error:?} {error}").contains(SENTINEL));
}

#[test]
fn duplicate_accounts_duplicate_fields_wrong_hosts_and_invalid_schema_fail() {
    let duplicate = enumeration(vec![account("Alice", true), account("ALICE", false)]);
    assert_eq!(
        parse_known_accounts("github.com", &duplicate),
        Err(AccountError::DuplicateAccount)
    );
    let mut wrong_host = account("Alice", true);
    wrong_host["host"] = json!("github.example.com");
    assert_eq!(
        parse_known_accounts("github.com", &enumeration(vec![wrong_host])),
        Err(AccountError::WrongHostOrAccount)
    );
    assert_eq!(
        parse_known_accounts("github.com", &output(r#"{"hosts":{"github.example.com":[]}}"#)),
        Err(AccountError::WrongHostOrAccount)
    );

    for text in [
        "",
        "null",
        "[]",
        "{}",
        "not json",
        r#"{"hosts":null}"#,
        r#"{"hosts":[]}"#,
        r#"{"hosts":{"github.com":null}}"#,
        r#"{"hosts":{"github.com":[],"github.com":[]}}"#,
        r#"{"hosts":{"GitHub.COM":[],"github.com":[]}}"#,
        r#"{"hosts":{},"hosts":{}}"#,
        r#"{"hosts":{"github.com":[{"state":"success","active":true,"host":"github.com","login":"Alice","login":"Bob"}]}}"#,
        r#"{"hosts":{"github.com":[{"state":"success","host":"github.com","login":"Alice"}]}}"#,
        r#"{"hosts":{"github.com":[{"state":"success","active":"true","host":"github.com","login":"Alice"}]}}"#,
        r#"{"hosts":{"github.com":[{"state":"unknown","active":true,"host":"github.com","login":"Alice"}]}}"#,
    ] {
        assert_eq!(
            parse_known_accounts("github.com", &output(text)),
            Err(AccountError::MalformedResponse)
        );
    }
    let mut missing_error = account("Alice", true);
    missing_error["state"] = json!("error");
    assert_eq!(
        parse_known_accounts("github.com", &enumeration(vec![missing_error])),
        Err(AccountError::MalformedResponse)
    );
    let mut contradictory = account("Alice", true);
    contradictory["error"] = json!("Bad credentials");
    assert_eq!(
        parse_known_accounts("github.com", &enumeration(vec![contradictory])),
        Err(AccountError::MalformedResponse)
    );
    assert_eq!(
        parse_known_accounts(
            "github.com",
            &enumeration(vec![account("Alice", true), account("Bob", true)])
        ),
        Err(AccountError::MalformedResponse)
    );
}

#[test]
fn known_account_host_case_and_root_dot_match_repository_canonicalization() {
    let mut entry = account("Alice", true);
    entry["host"] = json!("GitHub.COM.");
    let raw = json!({"hosts": {"GITHUB.COM": [entry]}});
    let known = parse_known_accounts("github.com", &output(&raw.to_string())).unwrap();
    let repo = GitHubRepoIdentity::new("GitHub.COM.", "Owner", "Repo.git").unwrap();
    assert_eq!(known.accounts()[0].identity().host(), repo.host());
    assert!(
        SelectedAccountRequestPlan::list(&known.accounts()[0], &repo, WorkItemKind::Issue).is_ok()
    );
}

#[test]
fn account_counts_bytes_utf8_status_and_completeness_are_bounded() {
    let full: Vec<_> = (0..KNOWN_ACCOUNT_LIMIT)
        .map(|n| account(&format!("user-{n}"), n == 0))
        .collect();
    assert_eq!(accounts(full.clone()).accounts().len(), KNOWN_ACCOUNT_LIMIT);
    let mut too_many = full;
    too_many.push(account("one-more", false));
    assert_eq!(
        parse_known_accounts("github.com", &enumeration(too_many)),
        Err(AccountError::MalformedResponse)
    );

    let baseline = enumeration(vec![account("Alice", true)]);
    let mut invalid_outputs = Vec::new();
    let mut invalid = baseline.clone();
    invalid.stdout_truncated = true;
    invalid_outputs.push(invalid);
    let mut invalid = baseline.clone();
    invalid.stderr_truncated = true;
    invalid_outputs.push(invalid);
    let mut invalid = baseline.clone();
    invalid.exit_code = None;
    invalid_outputs.push(invalid);
    let mut invalid = baseline.clone();
    invalid.stdout = vec![0xff];
    invalid_outputs.push(invalid);
    let mut invalid = baseline.clone();
    invalid.stdout = b"{\"hosts\":{},\"ignored\":\"\xff\"}".to_vec();
    invalid_outputs.push(invalid);
    let mut invalid = baseline.clone();
    invalid.stderr = vec![0xff];
    invalid_outputs.push(invalid);
    let mut invalid = baseline.clone();
    invalid.stdout.extend(vec![b' '; COMMAND_OUTPUT_LIMIT]);
    invalid_outputs.push(invalid);
    let mut invalid = baseline.clone();
    invalid.stderr = vec![b'x'; COMMAND_OUTPUT_LIMIT + 1];
    invalid_outputs.push(invalid);
    for invalid in invalid_outputs {
        assert_eq!(
            parse_known_accounts("github.com", &invalid),
            Err(AccountError::MalformedResponse)
        );
    }
    let mut timed_out = baseline;
    timed_out.timed_out = true;
    assert_eq!(
        parse_known_accounts("github.com", &timed_out),
        Err(AccountError::Offline)
    );
}

#[test]
fn inherited_authentication_cannot_masquerade_as_a_stored_account() {
    for source in [
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_ENTERPRISE_TOKEN",
        "GITHUB_ENTERPRISE_TOKEN",
    ] {
        let mut inherited = account("Alice", true);
        inherited["tokenSource"] = json!(source);
        assert_eq!(
            parse_known_accounts(
                "github.com",
                &enumeration(vec![inherited, account("Bob", false)])
            ),
            Err(AccountError::InheritedAuthentication)
        );
    }
}

#[test]
fn per_account_problems_have_static_distinct_categories_and_do_not_drop_peers() {
    for (diagnostic, expected) in [
        (
            "HTTP 401: Bad credentials",
            AccountError::AuthenticationFailed,
        ),
        ("HTTP 403: forbidden", AccountError::PermissionDenied),
        (
            "Resource not accessible by personal access token",
            AccountError::PermissionDenied,
        ),
        ("missing required scope", AccountError::ScopeRequired),
        ("HTTP 403: API rate limit exceeded", AccountError::RateLimited),
        ("HTTP 429", AccountError::RateLimited),
        ("Get: dial tcp: no such host", AccountError::Offline),
        ("context deadline exceeded", AccountError::Offline),
        ("keyring is locked", AccountError::CredentialStoreUnavailable),
        (
            "no oauth token found for github.com account Bob",
            AccountError::CredentialLookupFailed,
        ),
        ("account mismatch", AccountError::WrongHostOrAccount),
        ("unexpected upstream failure", AccountError::CommandFailed),
    ] {
        let known = accounts(vec![
            account("Alice", true),
            account_failure("Bob", diagnostic),
        ]);
        assert_eq!(known.accounts().len(), 2);
        assert_eq!(known.accounts()[0].problem(), None);
        assert_eq!(known.accounts()[1].problem(), Some(expected));
    }
    let mut timeout = account_failure("Bob", "Get: context deadline exceeded");
    timeout["state"] = json!("timeout");
    let known = accounts(vec![account("Alice", true), timeout]);
    assert_eq!(known.accounts()[1].state(), KnownAccountState::Timeout);
    assert_eq!(known.accounts()[1].problem(), Some(AccountError::Offline));
}

#[test]
fn fatal_enumeration_errors_are_not_successful_empty_or_partial_lists() {
    for (diagnostic, expected) in [
        ("unknown flag: --json", AccountError::UnsupportedAuthStatusJson),
        (
            "Unknown JSON field: hosts",
            AccountError::UnsupportedAuthStatusJson,
        ),
        (
            "unknown flag: --hostname",
            AccountError::UnsupportedAuthStatusJson,
        ),
        (
            "You are not logged into any GitHub hosts. Run gh auth login",
            AccountError::AuthRequired,
        ),
        (
            "org.freedesktop.secrets: unavailable",
            AccountError::CredentialStoreUnavailable,
        ),
        ("sh: gh: command not found", AccountError::ClientMissing),
        (
            "sh: /usr/bin/gh: No such file or directory",
            AccountError::ClientMissing,
        ),
        (
            "'gh' is not recognized as an internal or external command",
            AccountError::ClientMissing,
        ),
        (
            "open /home/gh/hosts.yml: no such file or directory",
            AccountError::CommandFailed,
        ),
        ("unknown failure", AccountError::CommandFailed),
    ] {
        let mut result = failed(diagnostic);
        result.stdout = enumeration(vec![account("Alice", true)]).stdout;
        assert_eq!(parse_known_accounts("github.com", &result), Err(expected));
    }
}

#[test]
fn required_capabilities_are_help_only_and_must_be_declared_as_flags() {
    let status_help = output(concat!(
        "USAGE\n  gh auth status [flags]\n\nFLAGS\n",
        "  -h, --hostname string   Host\n      --json fields       JSON output\n",
        "\nINHERITED FLAGS\n      --help              Help\n",
    ));
    let token_help = output(concat!(
        "USAGE\n  gh auth token [flags]\n\nFLAGS\n",
        "  -h, --hostname string   Host\n  -u, --user string       Account\n",
        "\nINHERITED FLAGS\n      --help              Help\n",
    ));
    for (capability, help, command) in [
        (AccountCapability::AuthStatusJson, status_help, "status"),
        (AccountCapability::NamedAccountLookup, token_help, "token"),
    ] {
        let plan = account_capability_plan(capability);
        assert_eq!(plan.display_argv(), ["gh", "auth", command, "--help"]);
        assert_eq!(verify_account_capability(capability, &help), Ok(()));
        assert_eq!(
            verify_account_capability(capability, &failed("sh: gh: not found")),
            Err(AccountError::ClientMissing)
        );
        assert_eq!(
            verify_account_capability(capability, &failed("unexpected failure")),
            Err(AccountError::CommandFailed)
        );
    }
    for text in [
        "Without --hostname or --user the active account is used.",
        "FLAGS\n  -h, --hostname string  Host\n      --user-agent text  Agent\n",
        "FLAGS\n  -h, --hostname string  Mentions --user in description\n",
        "EXAMPLES\n  gh auth token --hostname HOST --user Alice\n",
    ] {
        assert_eq!(
            verify_account_capability(AccountCapability::NamedAccountLookup, &output(text)),
            Err(AccountError::UnsupportedNamedAccountLookup)
        );
    }
    assert_eq!(
        verify_account_capability(
            AccountCapability::AuthStatusJson,
            &output("FLAGS\n  -h, --hostname string  Host\n")
        ),
        Err(AccountError::UnsupportedAuthStatusJson)
    );
    for text in [
        "unknown flag: --user",
        "unknown command \"token\" for \"gh auth\"",
    ] {
        assert_eq!(
            verify_account_capability(AccountCapability::NamedAccountLookup, &failed(text)),
            Err(AccountError::UnsupportedNamedAccountLookup)
        );
    }
}

#[test]
fn known_account_plan_is_explicit_json_without_active_or_secret_flags() {
    let plan = known_accounts_plan("GitHub.Example.COM.").unwrap();
    assert_eq!(
        plan.display_argv(),
        [
            "gh",
            "auth",
            "status",
            "--hostname",
            "github.example.com",
            "--json",
            "hosts",
        ]
    );
    for forbidden in ["--active", "--show-token", "login", "switch", "token", "--web"] {
        assert!(!plan.args.iter().any(|arg| arg == forbidden));
    }
}

#[test]
fn selected_request_plans_keep_existing_data_dtos_fields_limits_and_explicit_host() {
    let known = accounts(vec![account("Alice", true)]);
    let selected = &known.accounts()[0];
    let repo = GitHubRepoIdentity::new("github.com", "Owner", "Repo.git").unwrap();
    let identity = SelectedAccountRequestPlan::identity(selected);
    assert_eq!(identity.data_plan(), account_plan(repo.host()));
    assert_eq!(identity.stage(), AccountCommandStage::Identity);
    assert_eq!(identity.output_limit(), COMMAND_OUTPUT_LIMIT);
    for kind in WorkItemKind::ALL {
        let list = SelectedAccountRequestPlan::list(selected, &repo, kind).unwrap();
        let detail = SelectedAccountRequestPlan::detail(selected, &repo, kind, 42).unwrap();
        assert_eq!(list.data_plan(), list_plan(&repo, kind));
        assert_eq!(detail.data_plan(), detail_plan(&repo, kind, 42));
        assert_eq!(list.output_limit(), LIST_OUTPUT_LIMIT);
        assert_eq!(detail.output_limit(), DETAIL_OUTPUT_LIMIT);
        assert_eq!(list.stage(), AccountCommandStage::List);
        assert_eq!(detail.stage(), AccountCommandStage::Detail);
        for plan in [list, detail] {
            assert_eq!(plan.account(), selected.identity());
            assert_eq!(plan.lookup_login(), "Alice");
            let command = plan.data_plan();
            assert_eq!(command.program, "gh");
            assert!(
                command
                    .args
                    .windows(2)
                    .any(|args| args == ["--repo", "github.com/owner/repo"])
            );
            assert!(
                !command
                    .args
                    .iter()
                    .any(|arg| ["auth", "login", "switch", "token", "--web"].contains(&arg.as_str()))
            );
        }
    }
    let other = GitHubRepoIdentity::new("github.example.com", "Owner", "Repo").unwrap();
    assert_eq!(
        SelectedAccountRequestPlan::list(selected, &other, WorkItemKind::Issue),
        Err(AccountError::WrongHostOrAccount)
    );
    assert_eq!(
        SelectedAccountRequestPlan::detail(selected, &other, WorkItemKind::Issue, 42),
        Err(AccountError::WrongHostOrAccount)
    );
    assert_eq!(
        SelectedAccountRequestPlan::detail(selected, &repo, WorkItemKind::Issue, 0),
        Err(AccountError::MalformedResponse)
    );
}

#[test]
fn selected_identity_proof_rejects_wrong_auth_invalid_login_and_failed_commands() {
    let identity = GitHubAccountIdentity::new("github.com", "Alice").unwrap();
    assert_eq!(
        verify_selected_account(&identity, &output(r#"{"login":"ALICE","token":"SENTINEL"}"#)),
        Ok(())
    );
    assert_eq!(
        verify_selected_account(&identity, &output(r#"{"login":"Bob"}"#)),
        Err(AccountError::WrongHostOrAccount)
    );
    assert_eq!(
        verify_selected_account(&identity, &output(r#"{"login":"Alice\n"}"#)),
        Err(AccountError::InvalidLogin)
    );
    for text in [
        "Alice",
        "{}",
        r#"{"login":null}"#,
        r#"{"login":"Alice","login":"Bob"}"#,
    ] {
        assert_eq!(
            verify_selected_account(&identity, &output(text)),
            Err(AccountError::MalformedResponse)
        );
    }
    let mut rejected = failed("HTTP 401: Bad credentials; credential=SENTINEL");
    rejected.stdout = br#"{"login":"Alice"}"#.to_vec();
    let error = verify_selected_account(&identity, &rejected).unwrap_err();
    assert_eq!(error, AccountError::AuthenticationFailed);
    assert!(!format!("{error:?} {error}").contains("SENTINEL"));
    rejected.exit_code = Some(0);
    rejected.stdout_truncated = true;
    assert_eq!(
        verify_selected_account(&identity, &rejected),
        Err(AccountError::MalformedResponse)
    );
}

#[test]
fn selected_data_errors_and_execution_errors_do_not_retain_raw_diagnostics() {
    for stage in [
        AccountCommandStage::Identity,
        AccountCommandStage::List,
        AccountCommandStage::Detail,
    ] {
        for (text, expected) in [
            (
                "HTTP 401: Bad credentials",
                AccountError::AuthenticationFailed,
            ),
            ("HTTP 403: forbidden", AccountError::PermissionDenied),
            ("HTTP 403: API rate limit exceeded", AccountError::RateLimited),
            ("HTTP 404: Not Found", AccountError::NotFound),
            ("could not resolve host", AccountError::Offline),
        ] {
            let result = failed(&format!("{text}; SENTINEL"));
            let error = require_account_success(stage, &result).unwrap_err();
            assert_eq!(error, expected);
            assert!(!format!("{error:?} {error}").contains("SENTINEL"));
        }
    }
    for (kind, expected) in [
        (
            CommandExecutionErrorKind::ProgramNotFound,
            AccountError::ClientMissing,
        ),
        (CommandExecutionErrorKind::Disconnected, AccountError::Offline),
        (CommandExecutionErrorKind::Rejected, AccountError::Offline),
        (CommandExecutionErrorKind::Io, AccountError::CommandFailed),
    ] {
        let error = classify_account_execution_error(&CommandExecutionError::new(kind, "SENTINEL"));
        assert_eq!(error, expected);
        assert!(!format!("{error:?} {error}").contains("SENTINEL"));
    }
    for error in [
        AccountError::AuthRequired,
        AccountError::CredentialLookupFailed,
        AccountError::CredentialStoreUnavailable,
        AccountError::SelectedAccountUnavailable,
        AccountError::AuthenticationFailed,
        AccountError::PermissionDenied,
        AccountError::WrongHostOrAccount,
        AccountError::MalformedResponse,
    ] {
        assert!(!error.retains_last_known());
    }
    assert!(AccountError::Offline.retains_last_known());
    assert!(AccountError::RateLimited.retains_last_known());
}

#[test]
fn account_json_does_not_accept_positional_arrays_for_objects() {
    for text in [
        r#"[{}]"#,
        r#"{"hosts":{"github.com":[["success",null,true,"github.com","Alice",false]]}}"#,
    ] {
        assert_eq!(
            parse_known_accounts("github.com", &output(text)),
            Err(AccountError::MalformedResponse)
        );
    }
    let identity = GitHubAccountIdentity::new("github.com", "Alice").unwrap();
    assert_eq!(
        verify_selected_account(&identity, &output(r#"["Alice"]"#)),
        Err(AccountError::MalformedResponse)
    );
}

#[test]
fn capability_and_identity_outputs_require_complete_bounded_captures() {
    let identity = GitHubAccountIdentity::new("github.com", "Alice").unwrap();
    let mut captures = Vec::new();
    let mut truncated = output(r#"{"login":"Alice"}"#);
    truncated.stdout_truncated = true;
    captures.push(truncated);
    let mut truncated = output(r#"{"login":"Alice"}"#);
    truncated.stderr_truncated = true;
    captures.push(truncated);
    let mut missing_status = output(r#"{"login":"Alice"}"#);
    missing_status.exit_code = None;
    captures.push(missing_status);
    captures.push(output(&" ".repeat(COMMAND_OUTPUT_LIMIT + 1)));
    let mut oversized_stderr = output(r#"{"login":"Alice"}"#);
    oversized_stderr.stderr = vec![b'x'; COMMAND_OUTPUT_LIMIT + 1];
    captures.push(oversized_stderr);
    for capture in captures {
        assert_eq!(
            verify_selected_account(&identity, &capture),
            Err(AccountError::MalformedResponse)
        );
        for capability in [AccountCapability::AuthStatusJson, AccountCapability::NamedAccountLookup] {
            assert_eq!(
                verify_account_capability(capability, &capture),
                Err(AccountError::MalformedResponse)
            );
        }
    }
}

#[test]
fn identity_host_length_boundaries_and_no_login_trim_are_explicit() {
    let maximal = format!("{}.{}.{}.{}", "a".repeat(63), "b".repeat(63), "c".repeat(63), "d".repeat(61));
    assert_eq!(maximal.len(), 253);
    assert!(GitHubAccountIdentity::new(&maximal, "Alice").is_ok());
    assert!(GitHubAccountIdentity::new(&format!("{maximal}."), "Alice").is_ok());
    assert_eq!(
        GitHubAccountIdentity::new(&format!("{maximal}x"), "Alice"),
        Err(AccountError::InvalidHost)
    );
    for login in ["Alice ", "Alice\t", "Alice\r", "\u{00e9}lodie"] {
        assert_eq!(
            GitHubAccountIdentity::new("github.com", login),
            Err(AccountError::InvalidLogin)
        );
    }
}
