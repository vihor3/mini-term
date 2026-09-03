//! Bounded command execution on a project's own execution host.
//!
//! Local and WSL commands retain structured argv through `Command`. SSH is the
//! only boundary that serializes argv, using [`serialize_posix_argv`] before
//! the existing authenticated pooled bounded-exec API.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use mt_config::SshConnection;
use mt_github::{CommandExecutionError, CommandExecutionErrorKind, CommandOutput, CommandPlan};
use mt_identity::{ExecutionHostId, WorktreeId};

const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ExecutionBackendSignature {
    Local,
    Wsl {
        distro: String,
    },
    Ssh {
        connection_id: String,
        connection_fingerprint: u64,
        connection_epoch: Option<u64>,
    },
}

#[derive(Clone, Debug)]
pub enum ExecutionBackend {
    Local,
    Wsl {
        distro: String,
    },
    Ssh {
        connection: SshConnection,
        connection_fingerprint: u64,
        connection_epoch: Option<u64>,
    },
}

impl ExecutionBackend {
    pub fn signature(&self) -> ExecutionBackendSignature {
        match self {
            Self::Local => ExecutionBackendSignature::Local,
            Self::Wsl { distro } => ExecutionBackendSignature::Wsl {
                distro: distro.clone(),
            },
            Self::Ssh {
                connection,
                connection_fingerprint,
                connection_epoch,
            } => ExecutionBackendSignature::Ssh {
                connection_id: connection.id.clone(),
                connection_fingerprint: *connection_fingerprint,
                connection_epoch: *connection_epoch,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ExecutionSourceSignature {
    pub execution_host_id: ExecutionHostId,
    pub root_project_id: String,
    pub root_source_path: String,
    pub backend: ExecutionBackendSignature,
}

impl ExecutionSourceSignature {
    pub fn with_connection_epoch(&self, epoch: Option<u64>) -> Self {
        let mut next = self.clone();
        if let ExecutionBackendSignature::Ssh {
            connection_epoch, ..
        } = &mut next.backend
        {
            *connection_epoch = epoch;
        }
        next
    }
}

#[derive(Clone, Debug)]
pub struct ProjectExecutionSnapshot {
    pub project_id: String,
    pub root_project_id: String,
    pub worktree_id: WorktreeId,
    pub execution_host_id: ExecutionHostId,
    /// Path spelling understood by the execution host, not necessarily by the
    /// client OS. WSL and SSH paths are absolute POSIX paths.
    pub canonical_path: String,
    pub root_source_path: String,
    pub backend: ExecutionBackend,
    pub host_label: String,
}

impl ProjectExecutionSnapshot {
    pub fn source_signature(&self) -> ExecutionSourceSignature {
        ExecutionSourceSignature {
            execution_host_id: self.execution_host_id.clone(),
            root_project_id: self.root_project_id.clone(),
            root_source_path: self.root_source_path.clone(),
            backend: self.backend.signature(),
        }
    }

    pub fn observed_source_signature(
        &self,
        observed_connection_epoch: Option<u64>,
    ) -> ExecutionSourceSignature {
        self.source_signature()
            .with_connection_epoch(observed_connection_epoch.or(match &self.backend {
                ExecutionBackend::Ssh {
                    connection_epoch, ..
                } => *connection_epoch,
                ExecutionBackend::Local | ExecutionBackend::Wsl { .. } => None,
            }))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlannedHostCommand {
    Process {
        program: String,
        args: Vec<String>,
        cwd: Option<PathBuf>,
    },
    Ssh {
        remote_command: String,
    },
}

#[derive(Clone, Debug)]
pub struct HostCommandResult {
    pub output: CommandOutput,
    pub observed_connection_epoch: Option<u64>,
}

pub fn plan_host_command(
    snapshot: &ProjectExecutionSnapshot,
    plan: &CommandPlan,
) -> Result<PlannedHostCommand, CommandExecutionError> {
    validate_argv(plan)?;
    match &snapshot.backend {
        ExecutionBackend::Local => Ok(PlannedHostCommand::Process {
            program: plan.program.clone(),
            args: plan.args.clone(),
            cwd: Some(PathBuf::from(&snapshot.canonical_path)),
        }),
        ExecutionBackend::Wsl { distro } => {
            if distro.is_empty() || distro.contains('\0') {
                return Err(command_error(
                    CommandExecutionErrorKind::Io,
                    "WSL distribution identity is invalid",
                ));
            }
            let mut args = vec![
                "--distribution".to_string(),
                distro.clone(),
                "--cd".to_string(),
                snapshot.canonical_path.clone(),
                "--exec".to_string(),
                plan.program.clone(),
            ];
            args.extend(plan.args.clone());
            Ok(PlannedHostCommand::Process {
                program: "wsl.exe".into(),
                args,
                cwd: None,
            })
        }
        ExecutionBackend::Ssh { .. } => {
            let cwd = posix_quote(&snapshot.canonical_path)?;
            let argv = serialize_posix_argv(
                std::iter::once(plan.program.as_str()).chain(plan.args.iter().map(String::as_str)),
            )?;
            Ok(PlannedHostCommand::Ssh {
                remote_command: format!("cd {cwd} && exec {argv}"),
            })
        }
    }
}

pub fn execute_host_command(
    snapshot: &ProjectExecutionSnapshot,
    plan: &CommandPlan,
    timeout: Duration,
    output_cap: usize,
) -> Result<HostCommandResult, CommandExecutionError> {
    match plan_host_command(snapshot, plan)? {
        PlannedHostCommand::Process { program, args, cwd } => {
            let output = run_process(&program, &args, cwd, timeout, output_cap)?;
            Ok(HostCommandResult {
                output,
                observed_connection_epoch: None,
            })
        }
        PlannedHostCommand::Ssh { remote_command } => {
            let ExecutionBackend::Ssh { connection, .. } = &snapshot.backend else {
                unreachable!("SSH plan must have an SSH backend")
            };
            let result =
                crate::remote_ssh::bounded_exec(connection, &remote_command, timeout, output_cap)
                    .map_err(|message| {
                    command_error(CommandExecutionErrorKind::Disconnected, message)
                })?;
            let output = result.output;
            if output.requires_session_retirement() {
                return Err(command_error(
                    CommandExecutionErrorKind::Disconnected,
                    "SSH command left the authenticated session uncertain",
                ));
            }
            if output.state != mt_ssh::BoundedExecState::Started && !output.timed_out {
                return Err(command_error(
                    CommandExecutionErrorKind::Rejected,
                    "SSH server rejected the command",
                ));
            }
            Ok(HostCommandResult {
                output: CommandOutput {
                    stdout: output.stdout,
                    stderr: output.stderr,
                    exit_code: output.exit_code.and_then(|code| i32::try_from(code).ok()),
                    timed_out: output.timed_out,
                    stdout_truncated: output.stdout_truncated,
                    stderr_truncated: output.stderr_truncated,
                },
                observed_connection_epoch: Some(result.connection_epoch),
            })
        }
    }
}

pub fn serialize_posix_argv<'a>(
    argv: impl IntoIterator<Item = &'a str>,
) -> Result<String, CommandExecutionError> {
    let mut serialized = Vec::new();
    for value in argv {
        serialized.push(posix_quote(value)?);
    }
    if serialized.is_empty() {
        return Err(command_error(
            CommandExecutionErrorKind::Io,
            "command argv is empty",
        ));
    }
    Ok(serialized.join(" "))
}

fn posix_quote(value: &str) -> Result<String, CommandExecutionError> {
    if value.contains('\0') {
        return Err(command_error(
            CommandExecutionErrorKind::Io,
            "command argv contains NUL",
        ));
    }
    Ok(format!("'{}'", value.replace('\'', "'\\''")))
}

fn validate_argv(plan: &CommandPlan) -> Result<(), CommandExecutionError> {
    if plan.program.is_empty() || plan.program.contains('\0') {
        return Err(command_error(
            CommandExecutionErrorKind::Io,
            "command program is invalid",
        ));
    }
    if plan.args.iter().any(|arg| arg.contains('\0')) {
        return Err(command_error(
            CommandExecutionErrorKind::Io,
            "command argv contains NUL",
        ));
    }
    Ok(())
}

fn command_error(
    kind: CommandExecutionErrorKind,
    message: impl Into<String>,
) -> CommandExecutionError {
    CommandExecutionError::new(kind, message)
}

fn run_process(
    program: &str,
    args: &[String],
    cwd: Option<PathBuf>,
    timeout: Duration,
    output_cap: usize,
) -> Result<CommandOutput, CommandExecutionError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    hide_console_window(&mut command);
    let mut child = command.spawn().map_err(|error| {
        let kind = if error.kind() == std::io::ErrorKind::NotFound {
            CommandExecutionErrorKind::ProgramNotFound
        } else {
            CommandExecutionErrorKind::Io
        };
        command_error(kind, format!("process could not start: {error}"))
    })?;

    let stdout = child.stdout.take().ok_or_else(|| {
        command_error(
            CommandExecutionErrorKind::Io,
            "process stdout was unavailable",
        )
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        command_error(
            CommandExecutionErrorKind::Io,
            "process stderr was unavailable",
        )
    })?;
    let stdout_reader = thread::spawn(move || read_bounded(stdout, output_cap));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, output_cap));

    let deadline = Instant::now() + timeout;
    let (status, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (Some(status), false),
            Ok(None) if Instant::now() < deadline => thread::sleep(PROCESS_POLL_INTERVAL),
            Ok(None) => {
                let _ = child.kill();
                let status = child.wait().ok();
                break (status, true);
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(command_error(
                    CommandExecutionErrorKind::Io,
                    format!("process status failed: {error}"),
                ));
            }
        }
    };

    let stdout = stdout_reader
        .join()
        .map_err(|_| command_error(CommandExecutionErrorKind::Io, "stdout reader failed"))??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| command_error(CommandExecutionErrorKind::Io, "stderr reader failed"))??;
    Ok(CommandOutput {
        stdout: stdout.bytes,
        stderr: stderr.bytes,
        exit_code: status.and_then(|status| status.code()),
        timed_out,
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
    })
}

struct BoundedRead {
    bytes: Vec<u8>,
    truncated: bool,
}

fn read_bounded(mut reader: impl Read, cap: usize) -> Result<BoundedRead, CommandExecutionError> {
    let mut bytes = Vec::with_capacity(cap.min(16 * 1024));
    let mut truncated = false;
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let read = reader.read(&mut chunk).map_err(|error| {
            command_error(
                CommandExecutionErrorKind::Io,
                format!("process output read failed: {error}"),
            )
        })?;
        if read == 0 {
            break;
        }
        let remaining = cap.saturating_sub(bytes.len());
        let retained = remaining.min(read);
        bytes.extend_from_slice(&chunk[..retained]);
        truncated |= retained < read;
    }
    Ok(BoundedRead { bytes, truncated })
}

#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt as _;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;
    use mt_identity::{HostInstallId, RepoId};

    fn identities() -> (ExecutionHostId, WorktreeId) {
        let host = ExecutionHostId::derive("test", &HostInstallId::new());
        let repo = RepoId::derive(&host, "/repo");
        let worktree = WorktreeId::derive(&repo, "/repo", None);
        (host, worktree)
    }

    fn snapshot(backend: ExecutionBackend, path: &str) -> ProjectExecutionSnapshot {
        let (host, worktree) = identities();
        ProjectExecutionSnapshot {
            project_id: "worktree".into(),
            root_project_id: "root".into(),
            worktree_id: worktree,
            execution_host_id: host,
            canonical_path: path.into(),
            root_source_path: "/repo".into(),
            backend,
            host_label: "test".into(),
        }
    }

    fn command() -> CommandPlan {
        CommandPlan::new("gh", ["issue", "list", "--repo", "host/o/r"])
    }

    #[test]
    fn local_and_wsl_keep_structured_argv() {
        let local = plan_host_command(&snapshot(ExecutionBackend::Local, "/repo"), &command())
            .expect("local plan");
        assert_eq!(
            local,
            PlannedHostCommand::Process {
                program: "gh".into(),
                args: ["issue", "list", "--repo", "host/o/r"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                cwd: Some(PathBuf::from("/repo")),
            }
        );

        let wsl = plan_host_command(
            &snapshot(
                ExecutionBackend::Wsl {
                    distro: "Ubuntu".into(),
                },
                "/home/u/repo",
            ),
            &command(),
        )
        .expect("WSL plan");
        assert_eq!(
            wsl,
            PlannedHostCommand::Process {
                program: "wsl.exe".into(),
                args: [
                    "--distribution",
                    "Ubuntu",
                    "--cd",
                    "/home/u/repo",
                    "--exec",
                    "gh",
                    "issue",
                    "list",
                    "--repo",
                    "host/o/r",
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
                cwd: None,
            }
        );
    }

    #[test]
    fn ssh_serialization_keeps_hostile_text_as_data() {
        let quoted = serialize_posix_argv([
            "gh",
            "issue",
            "view",
            "'; touch /tmp/pwned; echo '",
            "line one\nline two",
        ])
        .expect("serialized argv");
        assert_eq!(
            quoted,
            "'gh' 'issue' 'view' ''\\''; touch /tmp/pwned; echo '\\''' 'line one\nline two'"
        );

        let ssh = plan_host_command(
            &snapshot(
                ExecutionBackend::Ssh {
                    connection: SshConnection {
                        id: "ssh".into(),
                        name: "host".into(),
                        host: "example.com".into(),
                        port: 22,
                        user: "u".into(),
                        password: None,
                        identity_file: None,
                        group: None,
                    },
                    connection_fingerprint: 7,
                    connection_epoch: Some(9),
                },
                "/srv/repo with spaces",
            ),
            &command(),
        )
        .expect("SSH plan");
        let PlannedHostCommand::Ssh { remote_command } = ssh else {
            panic!("expected SSH plan");
        };
        assert!(remote_command.starts_with(
            "cd '/srv/repo with spaces' && exec 'gh' 'issue' 'list' '--repo' 'host/o/r'"
        ));
        assert!(!remote_command.contains("sh -c"));
    }

    #[test]
    fn nul_is_rejected_before_any_host_dispatch() {
        let plan = CommandPlan::new("gh", ["issue", "bad\0arg"]);
        let error = plan_host_command(&snapshot(ExecutionBackend::Local, "/repo"), &plan)
            .expect_err("NUL should fail");
        assert_eq!(error.kind, CommandExecutionErrorKind::Io);
    }

    #[test]
    fn first_observed_ssh_epoch_replaces_the_captured_epoch() {
        let snapshot = snapshot(
            ExecutionBackend::Ssh {
                connection: SshConnection {
                    id: "ssh".into(),
                    name: "host".into(),
                    host: "example.com".into(),
                    port: 22,
                    user: "u".into(),
                    password: None,
                    identity_file: None,
                    group: None,
                },
                connection_fingerprint: 7,
                connection_epoch: Some(9),
            },
            "/srv/repo",
        );

        assert_eq!(
            snapshot.observed_source_signature(Some(10)).backend,
            ExecutionBackendSignature::Ssh {
                connection_id: "ssh".into(),
                connection_fingerprint: 7,
                connection_epoch: Some(10),
            }
        );
    }
}
