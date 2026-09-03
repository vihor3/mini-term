use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandStage {
    DiscoverRemote,
    Version,
    AuthStatus,
    Account,
    List,
    Detail,
}

impl CommandStage {
    fn uses_github_cli(self) -> bool {
        self != Self::DiscoverRemote
    }

    fn label(self) -> &'static str {
        match self {
            Self::DiscoverRemote => "Git remote discovery",
            Self::Version => "GitHub CLI probe",
            Self::AuthStatus => "GitHub authentication probe",
            Self::Account => "GitHub account probe",
            Self::List => "GitHub work-item list",
            Self::Detail => "GitHub work-item detail",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandExecutionErrorKind {
    ProgramNotFound,
    Disconnected,
    Rejected,
    Io,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandExecutionError {
    pub kind: CommandExecutionErrorKind,
    pub message: String,
}

impl CommandExecutionError {
    pub fn new(kind: CommandExecutionErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitHubErrorKind {
    NoGitHubRemote,
    ClientMissing,
    AuthRequired,
    WrongHostOrAccount,
    ScopeRequired,
    RateLimited,
    Offline,
    NotFound,
    MalformedResponse,
    RepositoryChanged,
    CommandFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHubError {
    pub kind: GitHubErrorKind,
    pub summary: String,
    pub retryable: bool,
}

impl GitHubError {
    pub fn new(kind: GitHubErrorKind, summary: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind,
            summary: summary.into(),
            retryable,
        }
    }

    pub fn malformed(label: &str) -> Self {
        Self::new(
            GitHubErrorKind::MalformedResponse,
            format!("{label} returned a malformed response"),
            true,
        )
    }

    pub fn repository_changed() -> Self {
        Self::new(
            GitHubErrorKind::RepositoryChanged,
            "The Git remote changed while this request was running",
            true,
        )
    }

    pub fn retains_last_known(&self) -> bool {
        matches!(
            self.kind,
            GitHubErrorKind::RateLimited
                | GitHubErrorKind::Offline
                | GitHubErrorKind::CommandFailed
        )
    }
}

impl fmt::Display for GitHubError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.summary)
    }
}

impl std::error::Error for GitHubError {}

pub fn classify_execution_error(stage: CommandStage, error: &CommandExecutionError) -> GitHubError {
    match error.kind {
        CommandExecutionErrorKind::ProgramNotFound if stage.uses_github_cli() => GitHubError::new(
            GitHubErrorKind::ClientMissing,
            "GitHub CLI is not installed on the selected execution host",
            true,
        ),
        CommandExecutionErrorKind::Disconnected => GitHubError::new(
            GitHubErrorKind::Offline,
            "The selected execution host is disconnected",
            true,
        ),
        CommandExecutionErrorKind::Rejected => GitHubError::new(
            GitHubErrorKind::Offline,
            "The selected execution host rejected the command",
            true,
        ),
        CommandExecutionErrorKind::ProgramNotFound | CommandExecutionErrorKind::Io => {
            GitHubError::new(
                GitHubErrorKind::CommandFailed,
                format!(
                    "{} could not run on the selected execution host",
                    stage.label()
                ),
                true,
            )
        }
    }
}

pub fn require_success(stage: CommandStage, output: &CommandOutput) -> Result<&[u8], GitHubError> {
    if output.timed_out {
        return Err(GitHubError::new(
            GitHubErrorKind::Offline,
            format!("{} timed out", stage.label()),
            true,
        ));
    }
    if output.stdout_truncated || output.stderr_truncated {
        return Err(GitHubError::malformed(stage.label()));
    }
    let Some(exit_code) = output.exit_code else {
        return Err(GitHubError::malformed(stage.label()));
    };
    if exit_code == 0 {
        return Ok(&output.stdout);
    }

    Err(classify_nonzero(stage, &output.stdout, &output.stderr))
}

fn classify_nonzero(stage: CommandStage, stdout: &[u8], stderr: &[u8]) -> GitHubError {
    let mut diagnostic = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    diagnostic.push('\n');
    diagnostic.push_str(&String::from_utf8_lossy(stdout).to_ascii_lowercase());

    if stage.uses_github_cli()
        && (contains_any(
            &diagnostic,
            &[
                "gh: command not found",
                "gh: not found",
                "'gh' is not recognized as an internal or external command",
                "\"gh\" is not recognized as an internal or external command",
            ],
        ) || (diagnostic.contains("gh") && diagnostic.contains("no such file or directory")))
    {
        return GitHubError::new(
            GitHubErrorKind::ClientMissing,
            "GitHub CLI is not installed on the selected execution host",
            true,
        );
    }
    if contains_any(
        &diagnostic,
        &[
            "rate limit",
            "secondary rate limit",
            "api rate limit exceeded",
        ],
    ) {
        return GitHubError::new(
            GitHubErrorKind::RateLimited,
            "GitHub rate limit reached; try again later",
            true,
        );
    }
    if contains_any(
        &diagnostic,
        &[
            "could not resolve host",
            "network is unreachable",
            "connection refused",
            "connection reset",
            "connection timed out",
            "tls handshake timeout",
            "temporary failure in name resolution",
            "no route to host",
        ],
    ) {
        return GitHubError::new(
            GitHubErrorKind::Offline,
            "GitHub could not be reached from the selected execution host",
            true,
        );
    }
    if contains_any(
        &diagnostic,
        &[
            "insufficient scopes",
            "missing required scope",
            "requires the `read:org` scope",
            "resource not accessible by personal access token",
        ],
    ) {
        return GitHubError::new(
            GitHubErrorKind::ScopeRequired,
            "The active GitHub account lacks a required read scope",
            true,
        );
    }
    if contains_any(
        &diagnostic,
        &[
            "http 404",
            "status code 404",
            "not found",
            "could not resolve to",
        ],
    ) && matches!(stage, CommandStage::List | CommandStage::Detail)
    {
        return GitHubError::new(
            GitHubErrorKind::NotFound,
            "The repository or work item was not found for the active account",
            false,
        );
    }
    if contains_any(
        &diagnostic,
        &[
            "not a known github host",
            "hostname mismatch",
            "account mismatch",
            "does not match the authenticated account",
        ],
    ) {
        return GitHubError::new(
            GitHubErrorKind::WrongHostOrAccount,
            "The active GitHub host or account does not match this repository",
            true,
        );
    }
    if stage == CommandStage::DiscoverRemote
        && contains_any(
            &diagnostic,
            &[
                "no such remote",
                "not a git repository",
                "does not appear to be a git repository",
            ],
        )
    {
        return GitHubError::new(
            GitHubErrorKind::NoGitHubRemote,
            "No GitHub remote was found for this project",
            true,
        );
    }
    if matches!(stage, CommandStage::AuthStatus)
        || contains_any(
            &diagnostic,
            &[
                "not logged into",
                "authentication failed",
                "token is invalid",
                "bad credentials",
                "gh auth login",
            ],
        )
    {
        return GitHubError::new(
            GitHubErrorKind::AuthRequired,
            "GitHub CLI authentication is required on the selected execution host",
            true,
        );
    }
    if stage == CommandStage::Account {
        return GitHubError::new(
            GitHubErrorKind::WrongHostOrAccount,
            "The active GitHub account could not read this host",
            true,
        );
    }

    GitHubError::new(
        GitHubErrorKind::CommandFailed,
        format!("{} failed on the selected execution host", stage.label()),
        true,
    )
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(stderr: &str) -> CommandOutput {
        CommandOutput {
            stderr: stderr.as_bytes().to_vec(),
            exit_code: Some(1),
            ..CommandOutput::default()
        }
    }

    #[test]
    fn authentication_and_network_failures_are_distinct() {
        let auth = require_success(
            CommandStage::AuthStatus,
            &failed("You are not logged into any GitHub hosts. Run gh auth login"),
        )
        .unwrap_err();
        assert_eq!(auth.kind, GitHubErrorKind::AuthRequired);

        let scope = require_success(
            CommandStage::List,
            &failed("GraphQL: Resource not accessible by personal access token"),
        )
        .unwrap_err();
        assert_eq!(scope.kind, GitHubErrorKind::ScopeRequired);

        let rate = require_success(
            CommandStage::List,
            &failed("API rate limit exceeded for user"),
        )
        .unwrap_err();
        assert_eq!(rate.kind, GitHubErrorKind::RateLimited);

        let offline = require_success(
            CommandStage::List,
            &failed("could not resolve host: github.com"),
        )
        .unwrap_err();
        assert_eq!(offline.kind, GitHubErrorKind::Offline);
    }

    #[test]
    fn missing_client_is_only_reported_for_gh_stages() {
        let missing = CommandExecutionError::new(
            CommandExecutionErrorKind::ProgramNotFound,
            "program missing",
        );
        assert_eq!(
            classify_execution_error(CommandStage::Version, &missing).kind,
            GitHubErrorKind::ClientMissing
        );
        assert_eq!(
            classify_execution_error(CommandStage::DiscoverRemote, &missing).kind,
            GitHubErrorKind::CommandFailed
        );
        assert_eq!(
            require_success(CommandStage::Version, &failed("sh: gh: command not found"))
                .unwrap_err()
                .kind,
            GitHubErrorKind::ClientMissing
        );
        assert_eq!(
            require_success(
                CommandStage::Version,
                &failed("'gh' is not recognized as an internal or external command"),
            )
            .unwrap_err()
            .kind,
            GitHubErrorKind::ClientMissing
        );
        assert_eq!(
            require_success(
                CommandStage::AuthStatus,
                &failed("sh: gh: command not found")
            )
            .unwrap_err()
            .kind,
            GitHubErrorKind::ClientMissing
        );
        assert_eq!(
            require_success(
                CommandStage::List,
                &failed("GraphQL: repository not found for the active account"),
            )
            .unwrap_err()
            .kind,
            GitHubErrorKind::NotFound
        );
    }

    #[test]
    fn truncated_and_statusless_outputs_fail_as_malformed() {
        let truncated = CommandOutput {
            exit_code: Some(0),
            stdout_truncated: true,
            ..CommandOutput::default()
        };
        assert_eq!(
            require_success(CommandStage::List, &truncated)
                .unwrap_err()
                .kind,
            GitHubErrorKind::MalformedResponse
        );
        assert_eq!(
            require_success(CommandStage::List, &CommandOutput::default())
                .unwrap_err()
                .kind,
            GitHubErrorKind::MalformedResponse
        );
    }
}
