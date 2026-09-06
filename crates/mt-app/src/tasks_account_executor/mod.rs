//! Tasks-only account execution. Public inputs and results never carry credentials.
//! Call on a background executor and cancel explicitly when its request is invalidated.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mt_github::{
    AccountCapability, AccountCommandStage, AccountError, CommandOutput, CommandPlan,
    KnownGitHubAccounts, SelectedAccountRequestPlan, account_capability_plan, account_plan,
    known_accounts_plan, parse_known_accounts, require_account_success, verify_account_capability,
    verify_selected_account,
};
use serde::Deserialize;

use crate::execution_host::{ExecutionBackend, ProjectExecutionSnapshot, serialize_posix_argv};

mod process;
#[cfg(test)]
pub(crate) mod tests;

const HOST_ENVELOPE: &str = include_str!("host_envelope.py");
pub(super) const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Default)]
pub struct AccountCancellation(Arc<AtomicBool>);

impl AccountCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    pub(crate) async fn cancelled(&self) {
        while !self.is_cancelled() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

pub struct AccountExecutionControl {
    timeout: Duration,
    cancellation: AccountCancellation,
    expected_connection_epoch: Option<u64>,
}

impl AccountExecutionControl {
    pub fn new(
        timeout: Duration,
        cancellation: AccountCancellation,
        expected_connection_epoch: Option<u64>,
    ) -> Result<Self, AccountExecutionError> {
        if timeout < Duration::from_millis(100) || timeout > MAX_TIMEOUT {
            return Err(AccountExecutionError::InvalidContext);
        }
        Ok(Self {
            timeout,
            cancellation,
            expected_connection_epoch,
        })
    }

    pub(crate) fn timeout(&self) -> Duration {
        self.timeout
    }

    pub(crate) fn cancellation(&self) -> &AccountCancellation {
        &self.cancellation
    }

    pub(crate) fn expected_connection_epoch(&self) -> Option<u64> {
        self.expected_connection_epoch
    }

    fn check(&self, deadline: Instant) -> Result<(), AccountExecutionError> {
        if self.cancellation.is_cancelled() {
            Err(AccountExecutionError::Cancelled)
        } else if Instant::now() >= deadline {
            Err(AccountExecutionError::TimedOut)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountExecutionError {
    Account(AccountError),
    Cancelled,
    TimedOut,
    InvalidContext,
    ContextChanged,
    HostHelperUnavailable,
    CleanupFailed,
    SecretOutputRejected,
    Protocol,
}

impl fmt::Display for AccountExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Account(error) => error.summary(),
            Self::Cancelled => "The Tasks account request was cancelled",
            Self::TimedOut => "The Tasks account request timed out",
            Self::InvalidContext => "The Tasks execution context is invalid",
            Self::ContextChanged => "The Tasks execution host changed during the request",
            Self::HostHelperUnavailable => {
                "Tasks account isolation requires Python 3.8+ on this host"
            }
            Self::CleanupFailed => "Tasks account process cleanup could not be confirmed",
            Self::SecretOutputRejected => "The Tasks account request produced unsafe output",
            Self::Protocol => "The Tasks account execution host returned invalid data",
        })
    }
}

impl std::error::Error for AccountExecutionError {}

impl From<AccountError> for AccountExecutionError {
    fn from(error: AccountError) -> Self {
        Self::Account(error)
    }
}

/// Failure keeps the observed epoch as well. Never publish a result from an
/// obsolete epoch, including capability/account errors.
pub struct AccountHostResult<T> {
    pub result: Result<T, AccountExecutionError>,
    pub observed_connection_epoch: Option<u64>,
}

impl<T> AccountHostResult<T> {
    pub(crate) fn map<U>(
        self,
        f: impl FnOnce(T) -> Result<U, AccountExecutionError>,
    ) -> AccountHostResult<U> {
        AccountHostResult {
            result: self.result.and_then(f),
            observed_connection_epoch: self.observed_connection_epoch,
        }
    }
}

pub fn probe_account_capability(
    snapshot: &ProjectExecutionSnapshot,
    capability: AccountCapability,
    control: &AccountExecutionControl,
) -> AccountHostResult<()> {
    run(
        snapshot,
        &account_capability_plan(capability),
        None,
        control,
        mt_github::COMMAND_OUTPUT_LIMIT,
    )
    .map(|output| verify_account_capability(capability, &output).map_err(Into::into))
}

pub fn discover_accounts(
    snapshot: &ProjectExecutionSnapshot,
    host: &str,
    control: &AccountExecutionControl,
) -> AccountHostResult<KnownGitHubAccounts> {
    let plan = match known_accounts_plan(host) {
        Ok(plan) => plan,
        Err(error) => {
            return AccountHostResult {
                result: Err(error.into()),
                observed_connection_epoch: None,
            };
        }
    };
    run(
        snapshot,
        &plan,
        None,
        control,
        mt_github::COMMAND_OUTPUT_LIMIT,
    )
    .map(|output| parse_known_accounts(host, &output).map_err(Into::into))
}

/// Lookup plus data read; list/detail also prove identity before and after using
/// the SAME captured credential. Origin and application generations remain app-owned.
pub fn execute_selected_account(
    snapshot: &ProjectExecutionSnapshot,
    plan: &SelectedAccountRequestPlan,
    control: &AccountExecutionControl,
) -> AccountHostResult<CommandOutput> {
    run(
        snapshot,
        &plan.data_plan(),
        Some(plan),
        control,
        plan.output_limit(),
    )
    .map(|output| {
        require_account_success(plan.stage(), &output)?;
        if plan.stage() == AccountCommandStage::Identity {
            verify_selected_account(plan.account(), &output)?;
        }
        Ok(output)
    })
}

fn run(
    snapshot: &ProjectExecutionSnapshot,
    data: &CommandPlan,
    selected: Option<&SelectedAccountRequestPlan>,
    control: &AccountExecutionControl,
    output_limit: usize,
) -> AccountHostResult<CommandOutput> {
    if let Err(error) = validate_context(snapshot, control) {
        return AccountHostResult {
            result: Err(error),
            observed_connection_epoch: None,
        };
    }
    if matches!(snapshot.backend, ExecutionBackend::Local) {
        return AccountHostResult {
            result: process::run_native(snapshot, data, selected, control, output_limit),
            observed_connection_epoch: None,
        };
    }
    let envelope = match host_envelope_plan(snapshot, data, selected, control, output_limit) {
        Ok(plan) => plan,
        Err(error) => {
            return AccountHostResult {
                result: Err(error),
                observed_connection_epoch: None,
            };
        }
    };
    let wire_cap = wire_limit(output_limit);
    match &snapshot.backend {
        ExecutionBackend::Local => unreachable!(),
        ExecutionBackend::Wsl { .. } => AccountHostResult {
            result: process::run_wsl(snapshot, &envelope, control, wire_cap)
                .and_then(|bytes| decode_host_reply(&bytes, output_limit)),
            observed_connection_epoch: None,
        },
        ExecutionBackend::Ssh { .. } => {
            let command = match ssh_envelope_command(&envelope) {
                Ok(command) => command,
                Err(_) => {
                    return AccountHostResult {
                        result: Err(AccountExecutionError::InvalidContext),
                        observed_connection_epoch: None,
                    };
                }
            };
            crate::remote_ssh::run_tasks_account_envelope(snapshot, &command, control, wire_cap)
                .map(|bytes| decode_host_reply(&bytes, output_limit))
        }
    }
}

fn ssh_envelope_command(envelope: &CommandPlan) -> Result<String, AccountExecutionError> {
    let argv = serialize_posix_argv(envelope.display_argv())
        .map_err(|_| AccountExecutionError::InvalidContext)?;
    // No credential exists yet. The login shell only sees static source and
    // nonsecret args; Python does not inherit shell tracing or transport stderr.
    Ok(format!(
        "set +x; if command -v python3 >/dev/null 2>&1; then exec {argv} 2>/dev/null; else printf '%s' '{{\"status\":\"helper-unavailable\"}}'; fi"
    ))
}

fn validate_context(
    snapshot: &ProjectExecutionSnapshot,
    control: &AccountExecutionControl,
) -> Result<(), AccountExecutionError> {
    if control.cancellation.is_cancelled() {
        return Err(AccountExecutionError::Cancelled);
    }
    if snapshot.canonical_path.contains('\0') || snapshot.canonical_path.is_empty() {
        return Err(AccountExecutionError::InvalidContext);
    }
    match &snapshot.backend {
        ExecutionBackend::Local => {
            if !std::path::Path::new(&snapshot.canonical_path).is_absolute() {
                return Err(AccountExecutionError::InvalidContext);
            }
        }
        ExecutionBackend::Wsl { distro } => {
            if distro.is_empty() || distro.starts_with('-') || distro.contains('\0') {
                return Err(AccountExecutionError::InvalidContext);
            }
            crate::execution_host::normalize_absolute_posix_path(&snapshot.canonical_path)
                .map_err(|_| AccountExecutionError::InvalidContext)?;
        }
        ExecutionBackend::Ssh {
            connection,
            connection_fingerprint,
            ..
        } => {
            if crate::remote_ssh::connection_fingerprint(connection) != *connection_fingerprint {
                return Err(AccountExecutionError::ContextChanged);
            }
            crate::execution_host::normalize_absolute_posix_path(&snapshot.canonical_path)
                .map_err(|_| AccountExecutionError::InvalidContext)?;
        }
    }
    Ok(())
}

fn host_envelope_plan(
    snapshot: &ProjectExecutionSnapshot,
    data: &CommandPlan,
    selected: Option<&SelectedAccountRequestPlan>,
    control: &AccountExecutionControl,
    output_limit: usize,
) -> Result<CommandPlan, AccountExecutionError> {
    let (host, login, expected, proof) = selected.map_or(("", "", "", Vec::new()), |plan| {
        (
            plan.account().host(),
            plan.lookup_login(),
            plan.account().login(),
            account_plan(plan.account().host()).args,
        )
    });
    let args = vec![
        "-I".into(),
        "-c".into(),
        HOST_ENVELOPE.into(),
        snapshot.canonical_path.clone(),
        control.timeout.as_millis().to_string(),
        output_limit.to_string(),
        host.into(),
        login.into(),
        expected.into(),
        serde_json::to_string(&data.args).map_err(|_| AccountExecutionError::InvalidContext)?,
        serde_json::to_string(&proof).map_err(|_| AccountExecutionError::InvalidContext)?,
    ];
    Ok(CommandPlan::new("python3", args))
}

pub(super) fn auth_variable(host: &str) -> &'static str {
    if host == "github.com" || host.ends_with(".ghe.com") {
        "GH_TOKEN"
    } else {
        "GH_ENTERPRISE_TOKEN"
    }
}

fn wire_limit(output_limit: usize) -> usize {
    6 * (output_limit + mt_github::COMMAND_OUTPUT_LIMIT) + 1024
}

#[derive(Deserialize)]
#[serde(tag = "status", deny_unknown_fields)]
enum HostReply {
    #[serde(rename = "output")]
    Output {
        stdout: String,
        stderr: String,
        exit_code: i32,
    },
    // Empty struct variants enforce closed fields; Serde's unit variants do not.
    #[serde(rename = "cancelled")]
    Cancelled {},
    #[serde(rename = "timed-out")]
    TimedOut {},
    #[serde(rename = "helper-unavailable")]
    HelperUnavailable {},
    #[serde(rename = "client-missing")]
    ClientMissing {},
    #[serde(rename = "credential-lookup-failed")]
    CredentialLookupFailed {},
    #[serde(rename = "credential-store-unavailable")]
    CredentialStoreUnavailable {},
    #[serde(rename = "named-account-unsupported")]
    NamedAccountUnsupported {},
    #[serde(rename = "identity-mismatch")]
    IdentityMismatch {},
    #[serde(rename = "cleanup-failed")]
    CleanupFailed {},
    #[serde(rename = "unsafe-output")]
    UnsafeOutput {},
    #[serde(rename = "malformed")]
    Malformed {},
    #[serde(rename = "failed")]
    Failed {},
}

fn decode_host_reply(
    bytes: &[u8],
    output_limit: usize,
) -> Result<CommandOutput, AccountExecutionError> {
    if bytes.len() > wire_limit(output_limit) || std::str::from_utf8(bytes).is_err() {
        return Err(AccountExecutionError::Protocol);
    }
    match serde_json::from_slice::<HostReply>(bytes).map_err(|_| AccountExecutionError::Protocol)? {
        HostReply::Output {
            stdout,
            stderr,
            exit_code,
        } => {
            if stdout.len() > output_limit || stderr.len() > mt_github::COMMAND_OUTPUT_LIMIT {
                return Err(AccountError::MalformedResponse.into());
            }
            Ok(CommandOutput {
                stdout: stdout.into_bytes(),
                stderr: stderr.into_bytes(),
                exit_code: Some(exit_code),
                ..CommandOutput::default()
            })
        }
        HostReply::Cancelled {} => Err(AccountExecutionError::Cancelled),
        HostReply::TimedOut {} => Err(AccountExecutionError::TimedOut),
        HostReply::HelperUnavailable {} => Err(AccountExecutionError::HostHelperUnavailable),
        HostReply::ClientMissing {} => Err(AccountError::ClientMissing.into()),
        HostReply::CredentialLookupFailed {} => Err(AccountError::CredentialLookupFailed.into()),
        HostReply::CredentialStoreUnavailable {} => {
            Err(AccountError::CredentialStoreUnavailable.into())
        }
        HostReply::NamedAccountUnsupported {} => {
            Err(AccountError::UnsupportedNamedAccountLookup.into())
        }
        HostReply::IdentityMismatch {} => Err(AccountError::WrongHostOrAccount.into()),
        HostReply::CleanupFailed {} => Err(AccountExecutionError::CleanupFailed),
        HostReply::UnsafeOutput {} => Err(AccountExecutionError::SecretOutputRejected),
        HostReply::Malformed {} => Err(AccountError::MalformedResponse.into()),
        HostReply::Failed {} => Err(AccountError::CommandFailed.into()),
    }
}

pub(crate) fn host_reply_confirms_cleanup(bytes: &[u8]) -> bool {
    matches!(serde_json::from_slice::<HostReply>(bytes), Ok(reply) if !matches!(reply, HostReply::CleanupFailed {}))
}
