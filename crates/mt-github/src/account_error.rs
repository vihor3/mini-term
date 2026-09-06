use std::fmt;

use crate::{
    COMMAND_OUTPUT_LIMIT, CommandExecutionError, CommandExecutionErrorKind, CommandOutput,
    DETAIL_OUTPUT_LIMIT, LIST_OUTPUT_LIMIT,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountCapability {
    AuthStatusJson,
    NamedAccountLookup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountCommandStage {
    Capability(AccountCapability),
    Enumeration,
    Identity,
    List,
    Detail,
}

impl AccountCommandStage {
    pub fn output_limit(self) -> usize {
        match self {
            Self::Capability(_) | Self::Enumeration | Self::Identity => COMMAND_OUTPUT_LIMIT,
            Self::List => LIST_OUTPUT_LIMIT,
            Self::Detail => DETAIL_OUTPUT_LIMIT,
        }
    }
}

/// Allowlisted failures only. Credential executors must return a category,
/// never put credential-stage stdout/stderr into a general command result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountError {
    ClientMissing,
    UnsupportedAuthStatusJson,
    UnsupportedNamedAccountLookup,
    InvalidHost,
    InvalidLogin,
    WrongHostOrAccount,
    DuplicateAccount,
    MalformedResponse,
    InheritedAuthentication,
    AuthRequired,
    SelectedAccountUnavailable,
    CredentialStoreUnavailable,
    CredentialLookupFailed,
    AuthenticationFailed,
    ScopeRequired,
    PermissionDenied,
    RateLimited,
    Offline,
    NotFound,
    CommandFailed,
}

impl AccountError {
    pub fn summary(self) -> &'static str {
        match self {
            Self::ClientMissing => "GitHub CLI is not installed on the selected execution host",
            Self::UnsupportedAuthStatusJson => {
                "GitHub CLI does not support the required structured account status"
            }
            Self::UnsupportedNamedAccountLookup => {
                "GitHub CLI does not support named-account credential lookup"
            }
            Self::InvalidHost => "The GitHub hostname is invalid",
            Self::InvalidLogin => "The GitHub account name is invalid",
            Self::WrongHostOrAccount => {
                "The GitHub host or authenticated account does not match this request"
            }
            Self::DuplicateAccount => "GitHub CLI returned duplicate account identities",
            Self::MalformedResponse => "GitHub CLI returned malformed or incomplete account data",
            Self::InheritedAuthentication => {
                "Inherited authentication overrides interfered with GitHub account discovery"
            }
            Self::AuthRequired => {
                "GitHub CLI authentication is required on the selected execution host"
            }
            Self::SelectedAccountUnavailable => {
                "The selected GitHub account is no longer configured on this execution host"
            }
            Self::CredentialStoreUnavailable => {
                "The GitHub credential store is unavailable on the selected execution host"
            }
            Self::CredentialLookupFailed => {
                "The selected account credential could not be read; it may be missing or inaccessible"
            }
            Self::AuthenticationFailed => {
                "GitHub rejected authentication for this account; its credential may be revoked"
            }
            Self::ScopeRequired => "The selected GitHub account lacks a required read scope",
            Self::PermissionDenied => "The selected GitHub account does not have permission",
            Self::RateLimited => "GitHub rate limit reached; try again later",
            Self::Offline => "GitHub could not be reached from the selected execution host",
            Self::NotFound => "The repository or work item was not found for the selected account",
            Self::CommandFailed => {
                "The GitHub account request failed on the selected execution host"
            }
        }
    }

    /// Still requires the same complete source, selected identity, and generation.
    pub fn retains_last_known(self) -> bool {
        matches!(
            self,
            Self::RateLimited | Self::Offline | Self::CommandFailed
        )
    }
}

impl fmt::Display for AccountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.summary())
    }
}

impl std::error::Error for AccountError {}

pub fn classify_account_execution_error(error: &CommandExecutionError) -> AccountError {
    match error.kind {
        CommandExecutionErrorKind::ProgramNotFound => AccountError::ClientMissing,
        CommandExecutionErrorKind::Disconnected | CommandExecutionErrorKind::Rejected => {
            AccountError::Offline
        }
        CommandExecutionErrorKind::Io => AccountError::CommandFailed,
    }
}

/// For nonsecret capability, enumeration, and data stages only, never token lookup.
pub fn require_account_success(
    stage: AccountCommandStage,
    output: &CommandOutput,
) -> Result<&[u8], AccountError> {
    if output.timed_out {
        return Err(AccountError::Offline);
    }
    if output.stdout_truncated
        || output.stderr_truncated
        || output.stdout.len() > stage.output_limit()
        || output.stderr.len() > COMMAND_OUTPUT_LIMIT
    {
        return Err(AccountError::MalformedResponse);
    }
    let stderr =
        std::str::from_utf8(&output.stderr).map_err(|_| AccountError::MalformedResponse)?;
    let stdout =
        std::str::from_utf8(&output.stdout).map_err(|_| AccountError::MalformedResponse)?;
    match output.exit_code {
        Some(0) => Ok(&output.stdout),
        None => Err(AccountError::MalformedResponse),
        Some(_) => {
            let diagnostic = format!("{stderr}\n{stdout}");
            let text = diagnostic.to_ascii_lowercase();
            let unsupported = text.contains("unknown flag")
                || text.contains("unknown shorthand flag")
                || text.contains("unknown command")
                || text.contains("unknown json field");
            let fallback = if unsupported {
                match stage {
                    AccountCommandStage::Capability(AccountCapability::AuthStatusJson)
                    | AccountCommandStage::Enumeration => AccountError::UnsupportedAuthStatusJson,
                    AccountCommandStage::Capability(AccountCapability::NamedAccountLookup) => {
                        AccountError::UnsupportedNamedAccountLookup
                    }
                    _ => AccountError::CommandFailed,
                }
            } else {
                AccountError::CommandFailed
            };
            Err(classify_account_diagnostic(&diagnostic).unwrap_or(fallback))
        }
    }
}

pub(crate) fn classify_account_diagnostic(diagnostic: &str) -> Option<AccountError> {
    let text = diagnostic.to_ascii_lowercase();
    let contains = |needles: &[&str]| needles.iter().any(|needle| text.contains(needle));
    if contains(&[
        "gh: command not found",
        "gh: not found",
        "gh: no such file or directory",
        "gh\": no such file or directory",
        "'gh': no such file or directory",
        "'gh' is not recognized as an internal or external command",
        "\"gh\" is not recognized as an internal or external command",
    ]) {
        return Some(AccountError::ClientMissing);
    }
    if contains(&[
        "unknown flag",
        "unknown shorthand flag",
        "unknown json field",
    ]) {
        if contains(&["--user", "'u' in -u"]) {
            return Some(AccountError::UnsupportedNamedAccountLookup);
        }
        if contains(&["--json", "--jq", "unknown json field"]) {
            return Some(AccountError::UnsupportedAuthStatusJson);
        }
    }
    if text.contains("rate limit") || contains(&["http 429", "status code 429"]) {
        return Some(AccountError::RateLimited);
    }
    if contains(&[
        "could not resolve host",
        "no such host",
        "network is unreachable",
        "connection refused",
        "connection reset",
        "connection timed out",
        "tls handshake timeout",
        "context deadline exceeded",
        "temporary failure in name resolution",
        "no route to host",
        "error connecting to",
        "i/o timeout",
    ]) {
        return Some(AccountError::Offline);
    }
    if contains(&[
        "keyring is locked",
        "keyring access denied",
        "failed to unlock keyring",
        "cannot access keyring",
        "failed to open keyring",
        "credential store is unavailable",
        "secure storage is unavailable",
        "org.freedesktop.secrets",
        "interaction is not allowed",
    ]) {
        return Some(AccountError::CredentialStoreUnavailable);
    }
    if contains(&["no oauth token found", "no token found for"]) {
        return Some(AccountError::CredentialLookupFailed);
    }
    if contains(&[
        "not a known github host",
        "hostname mismatch",
        "account mismatch",
        "does not match the authenticated account",
    ]) {
        return Some(AccountError::WrongHostOrAccount);
    }
    if contains(&[
        "http 401",
        "status code 401",
        "bad credentials",
        "token is invalid",
        "token has been revoked",
        "authentication failed",
    ]) {
        return Some(AccountError::AuthenticationFailed);
    }
    if contains(&[
        "insufficient scopes",
        "missing required scope",
        "requires the `read:org` scope",
    ]) {
        return Some(AccountError::ScopeRequired);
    }
    if contains(&[
        "http 403",
        "status code 403",
        "resource not accessible by personal access token",
        "resource not accessible by integration",
        "permission denied",
        "forbidden",
    ]) {
        return Some(AccountError::PermissionDenied);
    }
    if contains(&[
        "http 404",
        "status code 404",
        "could not resolve to",
        "not found",
    ]) {
        return Some(AccountError::NotFound);
    }
    if contains(&["not logged into", "gh auth login"]) {
        return Some(AccountError::AuthRequired);
    }
    None
}
