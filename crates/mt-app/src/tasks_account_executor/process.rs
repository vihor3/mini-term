use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use mt_github::{AccountError, CommandOutput, CommandPlan, SelectedAccountRequestPlan};

use crate::execution_host::{
    ExecutionBackend, ProcessTree, ProjectExecutionSnapshot, serialize_posix_argv,
};

use super::{AccountExecutionControl, AccountExecutionError, CLEANUP_TIMEOUT};

const SECRET_LIMIT: usize = 4096;
const POLL: Duration = Duration::from_millis(10);
const REMOVED_ENV: &[&str] = &[
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GH_ENTERPRISE_TOKEN",
    "GITHUB_ENTERPRISE_TOKEN",
    "GH_HOST",
    "GH_REPO",
    "GH_DEBUG",
    "DEBUG",
    "GH_FORCE_TTY",
    "CLICOLOR_FORCE",
    "GH_BROWSER",
    "BROWSER",
    "SSH_ASKPASS",
    "GIT_ASKPASS",
    "BASH_ENV",
    "ENV",
    "SHELLOPTS",
    "BASHOPTS",
    "WSLENV",
];

// Neither captured credential streams nor token buffers implement Debug/Clone.
struct PrivateBytes(Vec<u8>);

impl Drop for PrivateBytes {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            // SAFETY: each pointer is a valid unique byte in the live allocation.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
    }
}

struct PrivateCapture {
    stdout: PrivateBytes,
    stderr: PrivateBytes,
    exit_code: Option<i32>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
enum CaptureStage {
    Attached,
    StopLatched,
    ControlWrite,
    Exited,
    TreeRetired,
    Drained,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
enum CaptureDetail {
    None,
    ControlWrite(Option<bool>),
    Drained {
        cleanup_ack: Option<bool>,
        stdout_bytes: usize,
        stderr_bytes: usize,
    },
}

#[cfg(test)]
#[derive(Clone, Copy)]
struct CaptureObservation {
    stage: CaptureStage,
    at_us: u128,
    exit_code: Option<i32>,
    latched: Option<AccountExecutionError>,
    control: Option<AccountExecutionError>,
    detail: CaptureDetail,
}

#[cfg(test)]
pub(super) struct CaptureDiagnostics {
    started: Instant,
    observations: [Option<CaptureObservation>; 6],
}

#[cfg(test)]
impl CaptureDiagnostics {
    pub(super) fn describe(&self) -> String {
        use std::fmt::Write as _;

        let mut result = String::new();
        for observation in self.observations.iter().flatten() {
            let CaptureObservation {
                stage,
                at_us,
                exit_code,
                latched,
                control,
                detail,
            } = observation;
            // Only typed metadata enters this record, never either private pipe.
            let (write_ok, cleanup_ack, stdout_bytes, stderr_bytes) = match detail {
                CaptureDetail::None => (None, None, None, None),
                CaptureDetail::ControlWrite(ok) => (*ok, None, None, None),
                CaptureDetail::Drained {
                    cleanup_ack,
                    stdout_bytes,
                    stderr_bytes,
                } => (None, *cleanup_ack, Some(*stdout_bytes), Some(*stderr_bytes)),
            };
            let _ = writeln!(
                result,
                "stage={stage:?} at_us={at_us} exit={exit_code:?} latched={latched:?} control={control:?} write_ok={write_ok:?} cleanup_ack={cleanup_ack:?} stdout_bytes={stdout_bytes:?} stderr_bytes={stderr_bytes:?}"
            );
        }
        result
    }
}

#[cfg(test)]
thread_local! {
    static CAPTURE_DIAGNOSTICS: std::cell::RefCell<Option<CaptureDiagnostics>> = const {
        std::cell::RefCell::new(None)
    };
}

#[cfg(test)]
pub(super) fn trace_capture<T>(
    started: Instant,
    action: impl FnOnce() -> T,
) -> (T, CaptureDiagnostics) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            CAPTURE_DIAGNOSTICS.with(|slot| *slot.borrow_mut() = None);
        }
    }

    CAPTURE_DIAGNOSTICS.with(|slot| {
        let mut slot = slot.borrow_mut();
        assert!(slot.is_none(), "capture diagnostics cannot be nested");
        *slot = Some(CaptureDiagnostics {
            started,
            observations: [None; 6],
        });
    });
    let _reset = Reset;
    let result = action();
    let diagnostics = CAPTURE_DIAGNOSTICS.with(|slot| slot.borrow_mut().take().unwrap());
    (result, diagnostics)
}

#[cfg(test)]
fn observe_capture(
    stage: CaptureStage,
    control: &AccountExecutionControl,
    deadline: Instant,
    latched: Option<AccountExecutionError>,
    exit_code: Option<i32>,
    detail: CaptureDetail,
) {
    CAPTURE_DIAGNOSTICS.with(|slot| {
        if let Some(diagnostics) = slot.borrow_mut().as_mut() {
            diagnostics.observations[stage as usize] = Some(CaptureObservation {
                stage,
                at_us: diagnostics.started.elapsed().as_micros(),
                exit_code,
                latched,
                control: control.check(deadline).err(),
                detail,
            });
        }
    });
}

fn sanitize(command: &mut Command) {
    for key in REMOVED_ENV {
        command.env_remove(key);
    }
    command
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_PAGER", "cat")
        .env("PAGER", "cat")
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .env("LC_ALL", "C");
}

fn data_command(
    snapshot: &ProjectExecutionSnapshot,
    data: &CommandPlan,
    make_gh: &impl Fn() -> Command,
) -> Command {
    let mut command = make_gh();
    command
        .args(&data.args)
        .current_dir(&snapshot.canonical_path);
    sanitize(&mut command);
    command
}

pub(super) fn run_native(
    snapshot: &ProjectExecutionSnapshot,
    data: &CommandPlan,
    selected: Option<&SelectedAccountRequestPlan>,
    control: &AccountExecutionControl,
    output_limit: usize,
) -> Result<CommandOutput, AccountExecutionError> {
    run_native_with(snapshot, data, selected, control, output_limit, &|| {
        Command::new("gh")
    })
}

fn run_native_with(
    snapshot: &ProjectExecutionSnapshot,
    data: &CommandPlan,
    selected: Option<&SelectedAccountRequestPlan>,
    control: &AccountExecutionControl,
    output_limit: usize,
    make_gh: &impl Fn() -> Command,
) -> Result<CommandOutput, AccountExecutionError> {
    let deadline = Instant::now() + control.timeout();
    let Some(selected) = selected else {
        let captured = capture(
            data_command(snapshot, data, make_gh),
            control,
            deadline,
            output_limit,
            false,
        )?;
        control.check(deadline)?;
        return public_unselected(data, captured);
    };
    let mut lookup = make_gh();
    lookup
        .args([
            "auth",
            "token",
            "--hostname",
            selected.account().host(),
            "--user",
            selected.lookup_login(),
        ])
        .current_dir(&snapshot.canonical_path);
    sanitize(&mut lookup);
    let captured = capture(lookup, control, deadline, SECRET_LIMIT + 2, false)?;
    let token = credential(captured)?;
    let proof = mt_github::account_plan(selected.account().host());
    let run_data = |plan: &CommandPlan, limit: usize| {
        control.check(deadline)?;
        let mut command = data_command(snapshot, plan, make_gh);
        let value = std::str::from_utf8(&token.0)
            .map_err(|_| AccountExecutionError::Account(AccountError::CredentialLookupFailed))?;
        command.env(super::auth_variable(selected.account().host()), value);
        let captured = capture(command, control, deadline, limit, false)?;
        control.check(deadline)?;
        public_data(captured, Some(&token))
    };
    let before = run_data(&proof, mt_github::COMMAND_OUTPUT_LIMIT)?;
    mt_github::verify_selected_account(selected.account(), &before)?;
    if selected.stage() == mt_github::AccountCommandStage::Identity {
        return Ok(before);
    }
    let output = run_data(data, output_limit)?;
    let after = run_data(&proof, mt_github::COMMAND_OUTPUT_LIMIT)?;
    mt_github::verify_selected_account(selected.account(), &after)?;
    control.check(deadline)?;
    Ok(output)
}

fn credential(mut captured: PrivateCapture) -> Result<PrivateBytes, AccountExecutionError> {
    if captured.exit_code != Some(0) {
        let error = if has_ascii(&captured.stderr.0, b"unknown flag: --user")
            || has_ascii(&captured.stderr.0, b"unknown shorthand flag: 'u'")
        {
            AccountError::UnsupportedNamedAccountLookup
        } else if [
            b"keyring is locked".as_slice(),
            b"keyring access denied",
            b"failed to unlock keyring",
            b"cannot access keyring",
            b"failed to open keyring",
            b"credential store is unavailable",
            b"secure storage is unavailable",
            b"org.freedesktop.secrets",
            b"interaction is not allowed",
        ]
        .iter()
        .any(|text| has_ascii(&captured.stderr.0, text))
        {
            AccountError::CredentialStoreUnavailable
        } else {
            AccountError::CredentialLookupFailed
        };
        return Err(error.into());
    }
    if captured.stdout.0.last() == Some(&b'\n') {
        captured.stdout.0.pop();
        if captured.stdout.0.last() == Some(&b'\r') {
            captured.stdout.0.pop();
        }
    }
    if captured.stdout.0.is_empty()
        || captured.stdout.0.len() > SECRET_LIMIT
        || !captured.stdout.0.iter().all(u8::is_ascii_graphic)
    {
        return Err(AccountError::CredentialLookupFailed.into());
    }
    Ok(captured.stdout)
}

fn has_ascii(bytes: &[u8], expected: &[u8]) -> bool {
    bytes
        .windows(expected.len())
        .any(|part| part.eq_ignore_ascii_case(expected))
}

fn public_unselected(
    data: &CommandPlan,
    captured: PrivateCapture,
) -> Result<CommandOutput, AccountExecutionError> {
    let output = public_data(captured, None)?;
    let enumeration = data.args.get(1).is_some_and(|arg| arg == "status")
        && data.args.iter().any(|arg| arg == "--json");
    if !enumeration {
        let capability = if data.args.get(1).is_some_and(|arg| arg == "token") {
            mt_github::AccountCapability::NamedAccountLookup
        } else {
            mt_github::AccountCapability::AuthStatusJson
        };
        mt_github::require_account_success(
            mt_github::AccountCommandStage::Capability(capability),
            &output,
        )?;
        return Ok(output);
    }
    let host = data
        .args
        .windows(2)
        .find(|pair| pair[0] == "--hostname")
        .map(|pair| pair[1].as_str())
        .ok_or(AccountExecutionError::InvalidContext)?;
    let known = mt_github::parse_known_accounts(host, &output)?;
    let rows: Vec<_> = known
        .accounts()
        .iter()
        .map(|account| {
            let state = match account.state() {
                mt_github::KnownAccountState::Success => "success",
                mt_github::KnownAccountState::Error => "error",
                mt_github::KnownAccountState::Timeout => "timeout",
            };
            let mut row = serde_json::json!({
                "host": account.identity().host(), "login": account.login(),
                "active": account.is_active(), "state": state,
            });
            if let Some(problem) = account.problem() {
                row["error"] = serde_json::Value::String(nonsecret_diagnostic(problem).into());
            }
            row
        })
        .collect();
    Ok(CommandOutput {
        stdout: serde_json::to_vec(&serde_json::json!({"hosts": {(known.host()): rows}}))
            .map_err(|_| AccountError::MalformedResponse)?,
        exit_code: Some(0),
        ..CommandOutput::default()
    })
}

fn nonsecret_diagnostic(error: AccountError) -> &'static str {
    match error {
        AccountError::ClientMissing => "gh: command not found",
        AccountError::UnsupportedAuthStatusJson => "unknown flag: --json",
        AccountError::UnsupportedNamedAccountLookup => "unknown flag: --user",
        AccountError::CredentialStoreUnavailable => "credential store is unavailable",
        AccountError::CredentialLookupFailed => "no oauth token found",
        AccountError::WrongHostOrAccount => "hostname mismatch",
        AccountError::AuthenticationFailed => "bad credentials",
        AccountError::ScopeRequired => "insufficient scopes",
        AccountError::PermissionDenied => "permission denied",
        AccountError::RateLimited => "rate limit",
        AccountError::Offline => "connection timed out",
        AccountError::NotFound => "http 404",
        AccountError::AuthRequired => "not logged into",
        _ => "account request failed",
    }
}

fn public_data(
    mut captured: PrivateCapture,
    token: Option<&PrivateBytes>,
) -> Result<CommandOutput, AccountExecutionError> {
    if token.is_some_and(|token| {
        [&captured.stdout.0, &captured.stderr.0]
            .iter()
            .any(|bytes| bytes.windows(token.0.len()).any(|part| part == token.0))
    }) {
        return Err(AccountExecutionError::SecretOutputRejected);
    }
    Ok(CommandOutput {
        stdout: std::mem::take(&mut captured.stdout.0),
        stderr: std::mem::take(&mut captured.stderr.0),
        exit_code: captured.exit_code,
        ..CommandOutput::default()
    })
}

fn wsl_command(distro: &str, envelope: &CommandPlan) -> Result<Command, AccountExecutionError> {
    if distro.is_empty()
        || distro.starts_with('-')
        || distro.contains('\0')
        || envelope.program.is_empty()
        || envelope.program.starts_with('-')
    {
        return Err(AccountExecutionError::InvalidContext);
    }
    let argv = serialize_posix_argv(envelope.display_argv())
        .map_err(|_| AccountExecutionError::InvalidContext)?;
    let script = format!("exec {argv}");
    let mut command = Command::new("wsl.exe");
    command.args([
        "--distribution",
        distro,
        "--cd",
        "/",
        "--exec",
        "/bin/sh",
        "-c",
        &script,
    ]);
    Ok(command)
}

pub(super) fn run_wsl(
    snapshot: &ProjectExecutionSnapshot,
    envelope: &CommandPlan,
    control: &AccountExecutionControl,
    wire_cap: usize,
) -> Result<Vec<u8>, AccountExecutionError> {
    let ExecutionBackend::Wsl { distro } = &snapshot.backend else {
        return Err(AccountExecutionError::InvalidContext);
    };
    // The private envelope enters its captured cwd before any account lookup.
    let mut command = wsl_command(distro, envelope)?;
    sanitize(&mut command);
    let mut captured = capture(
        command,
        control,
        Instant::now() + control.timeout(),
        wire_cap,
        true,
    )?;
    if captured.exit_code != Some(0) {
        return Err(AccountExecutionError::HostHelperUnavailable);
    }
    Ok(std::mem::take(&mut captured.stdout.0))
}

struct OwnedChild {
    child: Child,
    tree: ProcessTree,
    cleaned: bool,
}

impl OwnedChild {
    fn cleanup(&mut self) -> Result<(), AccountExecutionError> {
        if self.cleaned {
            return Ok(());
        }
        let tree_ok = self.tree.terminate().is_ok();
        let _ = self.child.kill();
        let deadline = Instant::now() + CLEANUP_TIMEOUT;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => {
                    self.cleaned = true;
                    return if tree_ok {
                        Ok(())
                    } else {
                        Err(AccountExecutionError::CleanupFailed)
                    };
                }
                _ if Instant::now() >= deadline => {
                    return Err(AccountExecutionError::CleanupFailed);
                }
                _ => thread::sleep(POLL),
            }
        }
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn capture(
    mut command: Command,
    control: &AccountExecutionControl,
    deadline: Instant,
    output_limit: usize,
    cooperative: bool,
) -> Result<PrivateCapture, AccountExecutionError> {
    control.check(deadline)?;
    command
        .stdin(if cooperative {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let tree = ProcessTree::configure(&mut command).map_err(|_| AccountError::CommandFailed)?;
    let child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            AccountError::ClientMissing
        } else {
            AccountError::CommandFailed
        }
    })?;
    // Drop Command's private environment immediately; it is never formatted.
    drop(command);
    let mut owned = OwnedChild {
        child,
        tree,
        cleaned: false,
    };
    if owned.tree.attach(&owned.child).is_err() {
        let _ = owned.cleanup();
        return Err(AccountExecutionError::CleanupFailed);
    }
    #[cfg(test)]
    observe_capture(
        CaptureStage::Attached,
        control,
        deadline,
        None,
        None,
        CaptureDetail::None,
    );
    let (Some(stdout), Some(stderr)) = (owned.child.stdout.take(), owned.child.stderr.take())
    else {
        owned.cleanup()?;
        return Err(AccountError::CommandFailed.into());
    };
    let overflow = Arc::new(AtomicBool::new(false));
    let stdout_reader = match reader(stdout, output_limit, overflow.clone()) {
        Ok(reader) => reader,
        Err(error) => {
            owned.cleanup()?;
            return Err(error);
        }
    };
    let stderr_reader = match reader(stderr, mt_github::COMMAND_OUTPUT_LIMIT, overflow.clone()) {
        Ok(reader) => reader,
        Err(error) => {
            owned.cleanup()?;
            let deadline = Instant::now() + CLEANUP_TIMEOUT;
            while !stdout_reader.is_finished() {
                if Instant::now() >= deadline {
                    return Err(AccountExecutionError::CleanupFailed);
                }
                thread::sleep(POLL);
            }
            let _ = stdout_reader.join();
            return Err(error);
        }
    };
    let mut exit_code = None;
    let mut stopped = None;
    let mut stop_deadline = deadline;
    loop {
        if stopped.is_none() {
            stopped = control.check(deadline).err();
            if overflow.load(Ordering::Acquire) {
                stopped = Some(AccountError::MalformedResponse.into());
            }
            if stopped.is_some() {
                #[cfg(test)]
                observe_capture(
                    CaptureStage::StopLatched,
                    control,
                    deadline,
                    stopped,
                    exit_code,
                    CaptureDetail::None,
                );
                if cooperative {
                    #[cfg(test)]
                    let mut write_ok = None;
                    if let Some(mut stdin) = owned.child.stdin.take() {
                        let _write = stdin.write_all(b"x");
                        #[cfg(test)]
                        {
                            write_ok = Some(_write.is_ok());
                        }
                    }
                    #[cfg(test)]
                    observe_capture(
                        CaptureStage::ControlWrite,
                        control,
                        deadline,
                        stopped,
                        exit_code,
                        CaptureDetail::ControlWrite(write_ok),
                    );
                    stop_deadline = Instant::now() + CLEANUP_TIMEOUT;
                } else {
                    owned.cleanup()?;
                    break;
                }
            }
        }
        match owned.child.try_wait() {
            Ok(Some(status)) => {
                exit_code = status.code();
                #[cfg(test)]
                observe_capture(
                    CaptureStage::Exited,
                    control,
                    deadline,
                    stopped,
                    exit_code,
                    CaptureDetail::None,
                );
                // Retire descendants before draining: they may still own inherited pipes.
                break;
            }
            Ok(None) => {}
            Err(_) => {
                stopped = Some(AccountError::CommandFailed.into());
                break;
            }
        }
        if stopped.is_some() && Instant::now() >= stop_deadline {
            owned.cleanup()?;
            return Err(AccountExecutionError::CleanupFailed);
        }
        thread::sleep(POLL);
    }
    owned.cleanup()?;
    #[cfg(test)]
    observe_capture(
        CaptureStage::TreeRetired,
        control,
        deadline,
        stopped,
        exit_code,
        CaptureDetail::None,
    );
    let drain_deadline = Instant::now() + CLEANUP_TIMEOUT;
    while !stdout_reader.is_finished() || !stderr_reader.is_finished() {
        if Instant::now() >= drain_deadline {
            return Err(AccountExecutionError::CleanupFailed);
        }
        thread::sleep(POLL);
    }
    let stdout = stdout_reader
        .join()
        .map_err(|_| AccountError::CommandFailed)??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| AccountError::CommandFailed)??;
    #[cfg(test)]
    if cooperative {
        for bytes in [&stdout.0, &stderr.0] {
            assert!(
                !String::from_utf8_lossy(bytes).contains("fixture_credential_"),
                "host transport exposed the synthetic credential"
            );
        }
    }
    #[cfg(test)]
    observe_capture(
        CaptureStage::Drained,
        control,
        deadline,
        stopped,
        exit_code,
        CaptureDetail::Drained {
            cleanup_ack: cooperative.then(|| super::host_reply_confirms_cleanup(&stdout.0)),
            stdout_bytes: stdout.0.len(),
            stderr_bytes: stderr.0.len(),
        },
    );
    if let Some(error) = stopped {
        if cooperative && !super::host_reply_confirms_cleanup(&stdout.0) {
            return Err(AccountExecutionError::CleanupFailed);
        }
        return Err(error);
    }
    Ok(PrivateCapture {
        stdout,
        stderr,
        exit_code,
    })
}

#[cfg(all(test, windows))]
pub(super) fn suspended_cleanup_fixture(mut command: Command) {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let tree = ProcessTree::configure(&mut command).unwrap();
    let child = command.spawn().unwrap();
    // Exercise the same fallback as a failed Job assignment, before any resume.
    let mut owned = OwnedChild {
        child,
        tree,
        cleaned: false,
    };
    assert_eq!(owned.cleanup(), Err(AccountExecutionError::CleanupFailed));
    assert!(owned.child.try_wait().unwrap().is_some());
}

fn reader(
    mut pipe: impl Read + Send + 'static,
    cap: usize,
    overflow: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<Result<PrivateBytes, AccountExecutionError>>, AccountExecutionError>
{
    thread::Builder::new()
        .name("tasks-account-pipe".into())
        .spawn(move || {
            let mut bytes = PrivateBytes(Vec::with_capacity(cap.min(16 * 1024)));
            let mut chunk = PrivateBytes(vec![0; 8192]);
            loop {
                let n = match pipe.read(&mut chunk.0) {
                    Ok(n) => n,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => return Err(AccountError::CommandFailed.into()),
                };
                if n == 0 {
                    return Ok(bytes);
                }
                if n > cap.saturating_sub(bytes.0.len()) {
                    overflow.store(true, Ordering::Release);
                    return Err(AccountError::MalformedResponse.into());
                }
                bytes.0.extend_from_slice(&chunk.0[..n]);
            }
        })
        .map_err(|_| AccountError::CommandFailed.into())
}

#[cfg(test)]
pub(super) fn native_fixture(
    snapshot: &ProjectExecutionSnapshot,
    data: &CommandPlan,
    selected: Option<&SelectedAccountRequestPlan>,
    control: &AccountExecutionControl,
    output_limit: usize,
    make_gh: &impl Fn() -> Command,
) -> Result<CommandOutput, AccountExecutionError> {
    run_native_with(snapshot, data, selected, control, output_limit, make_gh)
}

#[cfg(test)]
pub(super) fn envelope_fixture(
    command: Command,
    control: &AccountExecutionControl,
    wire_cap: usize,
) -> Result<Vec<u8>, AccountExecutionError> {
    let mut result = capture(
        command,
        control,
        Instant::now() + control.timeout(),
        wire_cap,
        true,
    )?;
    if result.exit_code != Some(0) {
        return Err(AccountExecutionError::HostHelperUnavailable);
    }
    assert!(!String::from_utf8_lossy(&result.stderr.0).contains("fixture_credential_"));
    Ok(std::mem::take(&mut result.stdout.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_diagnostics_are_bounded_payload_free_metadata() {
        let control = AccountExecutionControl::new(
            Duration::from_secs(15),
            super::super::AccountCancellation::default(),
            None,
        )
        .unwrap();
        let deadline = Instant::now() + control.timeout();
        for (raw, ack) in [
            (
                br#"{"status":"output","stdout":"fixture_credential_stdout","stderr":"fixture_credential_stderr","exit_code":0}"#.as_slice(),
                true,
            ),
            (
                br#"{"status":"cancelled","token":"fixture_credential_extra"}"#.as_slice(),
                false,
            ),
            (br#"{"status":"cleanup-failed"}"#.as_slice(), false),
        ] {
            let ((), diagnostics) = trace_capture(Instant::now(), || {
                for stage in [
                    CaptureStage::Attached,
                    CaptureStage::StopLatched,
                    CaptureStage::ControlWrite,
                    CaptureStage::Exited,
                    CaptureStage::TreeRetired,
                    CaptureStage::Drained,
                ] {
                    observe_capture(
                        stage,
                        &control,
                        deadline,
                        Some(AccountExecutionError::Cancelled),
                        Some(-1),
                        CaptureDetail::Drained {
                            cleanup_ack: Some(super::super::host_reply_confirms_cleanup(raw)),
                            stdout_bytes: raw.len(),
                            stderr_bytes: 0,
                        },
                    );
                }
            });
            let diagnostic = diagnostics.describe();
            assert_eq!(diagnostic.lines().count(), 6);
            assert!(diagnostic.len() < 2048);
            assert!(diagnostic.contains(&format!("cleanup_ack=Some({ack})")));
            assert!(!diagnostic.contains("fixture_credential_"));
            assert!(diagnostic.contains(&format!("stdout_bytes=Some({})", raw.len())));
            assert!(diagnostic.contains("stderr_bytes=Some(0)"));
            assert!(!diagnostic.contains("\"status\""));
            assert!(!diagnostic.contains("token"));
        }
    }

    #[test]
    fn capture_diagnostics_preserve_results_and_reset_after_panics() {
        let cancellation = super::super::AccountCancellation::default();
        let control = AccountExecutionControl::new(
            Duration::from_secs(15),
            cancellation.clone(),
            None,
        )
        .unwrap();
        let deadline = Instant::now() + control.timeout();
        let (result, diagnostics) = trace_capture(Instant::now(), || {
            observe_capture(
                CaptureStage::Exited,
                &control,
                deadline,
                None,
                Some(23),
                CaptureDetail::None,
            );
            cancellation.cancel();
            observe_capture(
                CaptureStage::Drained,
                &control,
                deadline,
                None,
                Some(23),
                CaptureDetail::Drained {
                    cleanup_ack: Some(false),
                    stdout_bytes: 0,
                    stderr_bytes: 0,
                },
            );
            Err::<(), _>(AccountExecutionError::HostHelperUnavailable)
        });
        assert_eq!(result, Err(AccountExecutionError::HostHelperUnavailable));
        let exited = diagnostics.observations[CaptureStage::Exited as usize].unwrap();
        let drained = diagnostics.observations[CaptureStage::Drained as usize].unwrap();
        assert_eq!(exited.control, None);
        assert_eq!(drained.control, Some(AccountExecutionError::Cancelled));
        assert_eq!(drained.latched, None);
        assert!(
            std::panic::catch_unwind(|| {
                trace_capture(Instant::now(), || {
                    panic!("synthetic diagnostic scope failure")
                });
            })
            .is_err()
        );
        let ((), empty) = trace_capture(Instant::now(), || {
            std::thread::spawn(move || {
                observe_capture(
                    CaptureStage::Attached,
                    &control,
                    deadline,
                    None,
                    None,
                    CaptureDetail::None,
                );
            })
            .join()
            .unwrap();
        });
        assert!(empty.observations.iter().all(Option::is_none));
    }

    #[test]
    fn wsl_launcher_preserves_private_envelope_argv_with_root_cwd() {
        let path = "/mini-term-fixture/cases/space '\";$(printf injected)\nnext";
        for (host, login, expected_login) in [("github.com", "Alice", "alice"), ("", "", "")] {
            let envelope = CommandPlan::new(
                "python3",
                [
                    "-I",
                    "-c",
                    super::super::HOST_ENVELOPE,
                    path,
                    "3000",
                    "4096",
                    host,
                    login,
                    expected_login,
                    r#"["api","user","--hostname","github.com"]"#,
                    "[]",
                ],
            );
            let command = wsl_command("mt-tasks-12345-2", &envelope).unwrap();
            assert_eq!(command.get_program(), std::ffi::OsStr::new("wsl.exe"));
            let args = command.get_args().collect::<Vec<_>>();
            assert_eq!(args.len(), 8);
            assert!(args.iter().all(|arg| !arg.is_empty()));
            assert_eq!(
                args[..7],
                [
                    "--distribution",
                    "mt-tasks-12345-2",
                    "--cd",
                    "/",
                    "--exec",
                    "/bin/sh",
                    "-c"
                ]
                .map(std::ffi::OsStr::new)
            );
            let expected = format!(
                "exec {}",
                serialize_posix_argv(envelope.display_argv()).unwrap()
            );
            assert_eq!(args[7], std::ffi::OsStr::new(&expected));
            assert!(command.get_current_dir().is_none());
            assert_eq!(envelope.args[3], path);
        }
    }

    #[test]
    fn wsl_launcher_encodes_empty_fields_and_rejects_invalid_envelopes() {
        let envelope = CommandPlan::new("python3", ["-I", "-c", "pass", "", "", "middle", ""]);
        let command = wsl_command("mt-tasks-12345-2", &envelope).unwrap();
        let args = command.get_args().collect::<Vec<_>>();
        assert_eq!(args.len(), 8);
        assert!(args.iter().all(|arg| !arg.is_empty()));
        assert_eq!(
            args[7],
            std::ffi::OsStr::new("exec 'python3' '-I' '-c' 'pass' '' '' 'middle' ''")
        );
        for distro in ["", "-other", "bad\0distro"] {
            assert!(matches!(
                wsl_command(distro, &envelope),
                Err(AccountExecutionError::InvalidContext)
            ));
        }
        for plan in [
            CommandPlan::new("", ["-I"]),
            CommandPlan::new("-c", ["-I"]),
            CommandPlan::new("python\0", ["-I"]),
            CommandPlan::new("python3", ["bad\0arg"]),
        ] {
            assert!(matches!(
                wsl_command("mt-tasks-12345-2", &plan),
                Err(AccountExecutionError::InvalidContext)
            ));
        }
    }
}
