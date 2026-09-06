use crate::accounts::normalize_account_host;
use crate::{
    AccountCapability, AccountCommandStage, AccountError, CommandOutput, GitHubAccountIdentity,
    GitHubRepoIdentity, KnownGitHubAccount, WorkItemKind, require_account_success,
};

pub const COMMAND_OUTPUT_LIMIT: usize = 64 * 1024;
pub const LIST_OUTPUT_LIMIT: usize = 2 * 1024 * 1024;
pub const DETAIL_OUTPUT_LIMIT: usize = 4 * 1024 * 1024;
pub const LIST_LIMIT: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandPlan {
    pub program: String,
    pub args: Vec<String>,
}

impl CommandPlan {
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    pub fn display_argv(&self) -> Vec<&str> {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect()
    }
}

pub fn discover_remote_plan() -> CommandPlan {
    CommandPlan::new("git", ["remote", "get-url", "origin"])
}

pub fn version_plan() -> CommandPlan {
    CommandPlan::new("gh", ["--version"])
}

/// Legacy active-account preflight for the unmigrated Tasks pipeline.
/// Selected-account consumers must use known_accounts_plan and the dedicated
/// selected-account executor together, not replace this preflight alone.
pub fn auth_status_plan(host: &str) -> CommandPlan {
    CommandPlan::new("gh", ["auth", "status", "--active", "--hostname", host])
}

/// Legacy ambient-auth probe. Selected requests obtain these data argv through
/// SelectedAccountRequestPlan::identity and execute with isolated credentials.
pub fn account_plan(host: &str) -> CommandPlan {
    CommandPlan::new(
        "gh",
        ["api", "--hostname", host, "user", "--jq", "{login: .login}"],
    )
}

pub fn list_plan(repo: &GitHubRepoIdentity, kind: WorkItemKind) -> CommandPlan {
    let command = match kind {
        WorkItemKind::Issue => "issue",
        WorkItemKind::PullRequest => "pr",
    };
    let fields = match kind {
        WorkItemKind::Issue => "number,title,state,author,labels,updatedAt,url",
        WorkItemKind::PullRequest => "number,title,state,author,labels,updatedAt,url,isDraft",
    };
    CommandPlan::new(
        "gh",
        [
            command,
            "list",
            "--repo",
            repo.cli_spec().as_str(),
            "--state",
            "all",
            "--limit",
            LIST_LIMIT.to_string().as_str(),
            "--json",
            fields,
        ],
    )
}

pub fn detail_plan(repo: &GitHubRepoIdentity, kind: WorkItemKind, number: u64) -> CommandPlan {
    let command = match kind {
        WorkItemKind::Issue => "issue",
        WorkItemKind::PullRequest => "pr",
    };
    let fields = match kind {
        WorkItemKind::Issue => "number,title,state,author,labels,updatedAt,url,body",
        WorkItemKind::PullRequest => "number,title,state,author,labels,updatedAt,url,body,isDraft",
    };
    CommandPlan::new(
        "gh",
        [
            command,
            "view",
            number.to_string().as_str(),
            "--repo",
            repo.cli_spec().as_str(),
            "--json",
            fields,
        ],
    )
}

pub fn auth_login_command(host: &str) -> String {
    format!("gh auth login --hostname {host}")
}

/// Execute with inherited auth/debug overrides removed on the project host.
/// gh omits tokens in JSON unless --show-token is requested; the parser also
/// drops all non-allowlisted fields and converts errors into static categories.
pub fn known_accounts_plan(host: &str) -> Result<CommandPlan, AccountError> {
    let host = normalize_account_host(host)?;
    Ok(CommandPlan::new(
        "gh",
        ["auth", "status", "--hostname", host.as_str(), "--json", "hosts"],
    ))
}

/// Help-only capability checks. No credential lookup is planned here.
pub fn account_capability_plan(capability: AccountCapability) -> CommandPlan {
    let command = match capability {
        AccountCapability::AuthStatusJson => "status",
        AccountCapability::NamedAccountLookup => "token",
    };
    CommandPlan::new("gh", ["auth", command, "--help"])
}

pub fn verify_account_capability(
    capability: AccountCapability,
    output: &CommandOutput,
) -> Result<(), AccountError> {
    let bytes = require_account_success(AccountCommandStage::Capability(capability), output)?;
    let help = std::str::from_utf8(bytes).map_err(|_| AccountError::MalformedResponse)?;
    let (required, unsupported): (&[&str], _) = match capability {
        AccountCapability::AuthStatusJson => (
            &["--hostname", "--json"],
            AccountError::UnsupportedAuthStatusJson,
        ),
        AccountCapability::NamedAccountLookup => (
            &["--hostname", "--user"],
            AccountError::UnsupportedNamedAccountLookup,
        ),
    };
    if required.iter().all(|flag| help_has_flag(help, flag)) {
        Ok(())
    } else {
        Err(unsupported)
    }
}

fn help_has_flag(help: &str, expected: &str) -> bool {
    let mut in_flags = false;
    for line in help.lines() {
        if !line.starts_with([' ', '\t']) {
            if !line.trim().is_empty() {
                in_flags = matches!(line.trim(), "FLAGS" | "INHERITED FLAGS");
            }
            continue;
        }
        if in_flags {
            let mut words = line.split_whitespace();
            let first = words.next().unwrap_or_default();
            let flag = if first.starts_with('-') && first.ends_with(',') {
                words.next().unwrap_or_default()
            } else {
                first
            };
            if flag == expected {
                return true;
            }
        }
    }
    false
}

/// Nonsecret inputs for the dedicated execution-host credential owner only.
/// There is no arbitrary CommandPlan constructor or credential command here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedAccountRequestPlan {
    account: GitHubAccountIdentity,
    lookup_login: String,
    command: SelectedAccountCommand,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SelectedAccountCommand {
    Identity,
    List {
        repository: GitHubRepoIdentity,
        kind: WorkItemKind,
    },
    Detail {
        repository: GitHubRepoIdentity,
        kind: WorkItemKind,
        number: u64,
    },
}

impl SelectedAccountRequestPlan {
    /// Enumeration status is informational, not credential or API identity proof.
    /// A known but failing account may be retried without substituting a peer.
    pub fn identity(account: &KnownGitHubAccount) -> Self {
        Self::new(account, SelectedAccountCommand::Identity)
    }

    pub fn list(
        account: &KnownGitHubAccount,
        repository: &GitHubRepoIdentity,
        kind: WorkItemKind,
    ) -> Result<Self, AccountError> {
        if repository.host() != account.identity().host() {
            return Err(AccountError::WrongHostOrAccount);
        }
        Ok(Self::new(
            account,
            SelectedAccountCommand::List {
                repository: repository.clone(),
                kind,
            },
        ))
    }

    pub fn detail(
        account: &KnownGitHubAccount,
        repository: &GitHubRepoIdentity,
        kind: WorkItemKind,
        number: u64,
    ) -> Result<Self, AccountError> {
        if repository.host() != account.identity().host() {
            return Err(AccountError::WrongHostOrAccount);
        }
        if number == 0 {
            return Err(AccountError::MalformedResponse);
        }
        Ok(Self::new(
            account,
            SelectedAccountCommand::Detail {
                repository: repository.clone(),
                kind,
                number,
            },
        ))
    }

    fn new(account: &KnownGitHubAccount, command: SelectedAccountCommand) -> Self {
        Self {
            account: account.identity().clone(),
            lookup_login: account.login().to_owned(),
            command,
        }
    }

    pub fn account(&self) -> &GitHubAccountIdentity {
        &self.account
    }

    pub fn lookup_login(&self) -> &str {
        &self.lookup_login
    }

    /// Data argv only. Do not run via the ordinary ambient-auth command executor.
    pub fn data_plan(&self) -> CommandPlan {
        match &self.command {
            SelectedAccountCommand::Identity => account_plan(self.account.host()),
            SelectedAccountCommand::List { repository, kind } => list_plan(repository, *kind),
            SelectedAccountCommand::Detail {
                repository,
                kind,
                number,
            } => detail_plan(repository, *kind, *number),
        }
    }

    pub fn stage(&self) -> AccountCommandStage {
        match &self.command {
            SelectedAccountCommand::Identity => AccountCommandStage::Identity,
            SelectedAccountCommand::List { .. } => AccountCommandStage::List,
            SelectedAccountCommand::Detail { .. } => AccountCommandStage::Detail,
        }
    }

    pub fn output_limit(&self) -> usize {
        self.stage().output_limit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> GitHubRepoIdentity {
        GitHubRepoIdentity::new("github.example.com", "Owner", "Repo").unwrap()
    }

    #[test]
    fn plans_use_structured_argv_and_explicit_repository() {
        let list = list_plan(&repo(), WorkItemKind::PullRequest);
        assert_eq!(list.program, "gh");
        assert_eq!(list.args[0..2], ["pr", "list"]);
        assert!(
            list.args
                .windows(2)
                .any(|pair| { pair == ["--repo", "github.example.com/owner/repo"] })
        );
        assert!(list.args.windows(2).any(|pair| pair == ["--state", "all"]));
        assert!(!list.args.iter().any(|arg| arg == "--web"));

        let detail = detail_plan(&repo(), WorkItemKind::Issue, 42);
        assert_eq!(detail.args[0..3], ["issue", "view", "42"]);
        assert!(!detail.args.iter().any(|arg| arg == "--web"));
    }

    #[test]
    fn auth_probe_never_requests_or_prints_a_token() {
        let status = auth_status_plan("github.example.com");
        assert!(status.args.iter().any(|arg| arg == "--active"));
        assert!(!status.args.iter().any(|arg| arg == "--show-token"));
        assert_eq!(
            auth_login_command("github.example.com"),
            "gh auth login --hostname github.example.com"
        );
    }
}
