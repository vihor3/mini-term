//! Bounded command execution on a project's own execution host.
//!
//! Local and WSL commands retain structured argv through `Command`. SSH is the
//! only boundary that serializes argv, using [`serialize_posix_argv`] before
//! the existing authenticated pooled bounded-exec API.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use mt_config::SshConnection;
use mt_github::{CommandExecutionError, CommandExecutionErrorKind, CommandOutput, CommandPlan};
use mt_identity::{ExecutionHostId, WorktreeId};

const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);
const PROCESS_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

#[cfg(windows)]
const WINDOWS_PROCESS_CREATION_FLAGS: u32 = 0x0800_0000 | 0x0000_0004;

#[cfg(windows)]
#[link(name = "ntdll")]
unsafe extern "system" {
    #[link_name = "NtResumeProcess"]
    fn nt_resume_process(process_handle: *mut std::ffi::c_void) -> i32;
}

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
    pub worktree_id: WorktreeId,
    pub canonical_path: String,
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
            worktree_id: self.worktree_id.clone(),
            canonical_path: self.canonical_path.clone(),
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

/// Normalize an execution-host POSIX path without borrowing local filesystem
/// semantics. WSL and SSH paths are case-sensitive and may not escape root.
pub fn normalize_absolute_posix_path(path: &str) -> Result<String, String> {
    if !path.starts_with('/') || path.contains('\0') {
        return Err(format!("execution-host path must be absolute POSIX: {path}"));
    }
    let mut segments = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                return Err(format!(
                    "execution-host path cannot contain `..`: {path}"
                ));
            }
            value => segments.push(value),
        }
    }
    Ok(if segments.is_empty() {
        "/".into()
    } else {
        format!("/{}", segments.join("/"))
    })
}

/// Convert a WSL-owned POSIX path to the canonical host-visible UNC spelling
/// used at project registration and persistence boundaries.
pub fn wsl_host_visible_path(distro: &str, path: &str) -> Result<String, String> {
    if distro.is_empty() || distro.contains('\0') {
        return Err("WSL distribution identity is invalid".into());
    }
    let path = normalize_absolute_posix_path(path)?;
    if path == "/" {
        Ok(format!(r"\\wsl.localhost\{distro}"))
    } else {
        Ok(format!(
            r"\\wsl.localhost\{distro}\{}",
            path.trim_start_matches('/').replace('/', "\\")
        ))
    }
}

/// Host-qualified canonical key for configured Local/WSL project paths. WSL
/// aliases (`wsl$` and `wsl.localhost`) collapse to distro plus POSIX path,
/// while native local paths retain the platform comparison rules.
pub fn normalize_host_visible_project_path(path: &str) -> Result<String, String> {
    if path.is_empty() || path.contains('\0') {
        return Err("configured project path is invalid".into());
    }
    if let Some(wsl) = mt_core::parse_wsl_unc(&path.replace('/', "\\")) {
        return Ok(format!(
            "wsl:{}:{}",
            wsl.distro.to_lowercase(),
            normalize_absolute_posix_path(&wsl.unix_path)?
        ));
    }
    Ok(format!(
        "local:{}",
        mt_project::worktree::normalize_path_for_comparison(path)
    ))
}

/// Resolve the exact configured project folder into the spelling understood
/// by its execution backend. Catalog discovery uses this rather than a
/// persisted canonical binding so the folder the user added remains the Git
/// scan anchor.
pub fn configured_execution_path(
    backend: &ExecutionBackend,
    configured_path: &str,
) -> Result<String, String> {
    if configured_path.is_empty() || configured_path.contains('\0') {
        return Err("configured project path is invalid".into());
    }
    match backend {
        ExecutionBackend::Local => Ok(configured_path.to_string()),
        ExecutionBackend::Wsl { distro } => {
            let parsed = mt_core::parse_wsl_unc(&configured_path.replace('/', "\\"))
                .ok_or_else(|| "configured WSL project path is not a WSL UNC path".to_string())?;
            if !parsed.distro.eq_ignore_ascii_case(distro) {
                return Err("configured WSL project path belongs to another distribution".into());
            }
            normalize_absolute_posix_path(&parsed.unix_path)
        }
        ExecutionBackend::Ssh { .. } => normalize_absolute_posix_path(configured_path),
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

/// Local-side command context used before a project/worktree identity exists.
/// A Windows WSL UNC path remains a local picker result, but commands execute
/// inside the owning distribution with a POSIX cwd.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreProjectLocalContext {
    Native { cwd: PathBuf },
    Wsl { distro: String, cwd: String },
}

impl PreProjectLocalContext {
    pub fn from_host_path(path: &str) -> Result<Self, CommandExecutionError> {
        if path.is_empty() || path.contains('\0') {
            return Err(command_error(
                CommandExecutionErrorKind::Io,
                "pre-project path is invalid",
            ));
        }
        if let Some(wsl) = mt_core::parse_wsl_unc(&path.replace('/', "\\")) {
            if wsl.distro.is_empty() || wsl.distro.contains('\0') {
                return Err(command_error(
                    CommandExecutionErrorKind::Io,
                    "WSL distribution identity is invalid",
                ));
            }
            return Ok(Self::Wsl {
                distro: wsl.distro,
                cwd: wsl.unix_path,
            });
        }
        Ok(Self::Native {
            cwd: PathBuf::from(path),
        })
    }
}

pub fn plan_pre_project_local_command(
    context: &PreProjectLocalContext,
    plan: &CommandPlan,
) -> Result<PlannedHostCommand, CommandExecutionError> {
    validate_argv(plan)?;
    match context {
        PreProjectLocalContext::Native { cwd } => Ok(PlannedHostCommand::Process {
            program: plan.program.clone(),
            args: plan.args.clone(),
            cwd: Some(cwd.clone()),
        }),
        PreProjectLocalContext::Wsl { distro, cwd } => {
            let mut args = vec![
                "--distribution".to_string(),
                distro.clone(),
                "--cd".to_string(),
                cwd.clone(),
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
    }
}

/// Execute a local or automatic-WSL command before project registration while
/// retaining the existing timeout, bounded-output, and process-tree cleanup.
pub fn execute_pre_project_local_command(
    context: &PreProjectLocalContext,
    plan: &CommandPlan,
    timeout: Duration,
    output_cap: usize,
) -> Result<CommandOutput, CommandExecutionError> {
    let PlannedHostCommand::Process { program, args, cwd } =
        plan_pre_project_local_command(context, plan)?
    else {
        unreachable!("pre-project local planning never produces SSH commands")
    };
    run_process(&program, &args, cwd, timeout, output_cap)
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

#[cfg(unix)]
struct ProcessTree {
    process_group_id: Option<i32>,
    attached: bool,
}

#[cfg(unix)]
impl ProcessTree {
    fn configure(command: &mut Command) -> Result<Self, CommandExecutionError> {
        use std::os::unix::process::CommandExt as _;

        command.process_group(0);
        Ok(Self {
            process_group_id: None,
            attached: false,
        })
    }

    fn attach(&mut self, child: &Child) -> Result<(), CommandExecutionError> {
        self.process_group_id = Some(i32::try_from(child.id()).map_err(|_| {
            command_error(
                CommandExecutionErrorKind::Io,
                "process group identity exceeded the platform PID range",
            )
        })?);
        self.attached = true;
        Ok(())
    }

    fn terminate(&mut self) -> Result<(), CommandExecutionError> {
        if !self.attached {
            return Err(command_error(
                CommandExecutionErrorKind::Io,
                "process group was not attached",
            ));
        }
        let Some(process_group_id) = self.process_group_id else {
            return Ok(());
        };
        // SAFETY: the child was spawned with process_group(0), so its positive
        // PID is the group ID and the negated value targets only that group.
        if unsafe { libc::kill(-process_group_id, libc::SIGKILL) } == 0 {
            self.process_group_id = None;
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            self.process_group_id = None;
            return Ok(());
        }
        Err(command_error(
            CommandExecutionErrorKind::Io,
            format!("process group could not be terminated: {error}"),
        ))
    }
}

#[cfg(unix)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

#[cfg(windows)]
struct ProcessTree {
    job: windows::core::Owned<windows::Win32::Foundation::HANDLE>,
    attached: bool,
    terminated: bool,
}

#[cfg(windows)]
impl ProcessTree {
    fn configure(command: &mut Command) -> Result<Self, CommandExecutionError> {
        use std::os::windows::process::CommandExt as _;
        use windows::Win32::System::JobObjects::{
            CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };

        // CREATE_SUSPENDED closes the spawn-to-assignment window: no child code
        // can create a descendant before the process belongs to this job.
        command.creation_flags(WINDOWS_PROCESS_CREATION_FLAGS);
        let job =
            unsafe { CreateJobObjectW(None, windows::core::PCWSTR::null()) }.map_err(|error| {
                command_error(
                    CommandExecutionErrorKind::Io,
                    format!("process job could not be created: {error}"),
                )
            })?;
        // SAFETY: CreateJobObjectW returned a newly owned handle.
        let job = unsafe { windows::core::Owned::new(job) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let limits_size = u32::try_from(std::mem::size_of_val(&limits)).map_err(|_| {
            command_error(
                CommandExecutionErrorKind::Io,
                "process job limit structure exceeded the Windows API size range",
            )
        })?;
        unsafe {
            SetInformationJobObject(
                *job,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast(),
                limits_size,
            )
        }
        .map_err(|error| {
            command_error(
                CommandExecutionErrorKind::Io,
                format!("process job could not enable kill-on-close: {error}"),
            )
        })?;
        Ok(Self {
            job,
            attached: false,
            terminated: false,
        })
    }

    fn attach(&mut self, child: &Child) -> Result<(), CommandExecutionError> {
        use std::os::windows::io::AsRawHandle as _;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::JobObjects::AssignProcessToJobObject;

        let process = HANDLE(child.as_raw_handle());
        unsafe { AssignProcessToJobObject(*self.job, process) }.map_err(|error| {
            command_error(
                CommandExecutionErrorKind::Io,
                format!("process could not be attached to its cleanup job: {error}"),
            )
        })?;
        self.attached = true;

        // SAFETY: Command created this valid child handle suspended. Marking the
        // job attached first makes a resume failure follow job cleanup.
        let status = unsafe { nt_resume_process(child.as_raw_handle()) };
        if status < 0 {
            return Err(command_error(
                CommandExecutionErrorKind::Io,
                format!(
                    "process could not be resumed after job attachment: NTSTATUS 0x{:08x}",
                    status as u32
                ),
            ));
        }
        Ok(())
    }

    fn terminate(&mut self) -> Result<(), CommandExecutionError> {
        use windows::Win32::System::JobObjects::TerminateJobObject;

        if !self.attached {
            return Err(command_error(
                CommandExecutionErrorKind::Io,
                "process job was not attached",
            ));
        }
        if self.terminated {
            return Ok(());
        }

        unsafe { TerminateJobObject(*self.job, 1) }.map_err(|error| {
            command_error(
                CommandExecutionErrorKind::Io,
                format!("process job could not be terminated: {error}"),
            )
        })?;
        self.terminated = true;
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        if self.attached && !self.terminated {
            let _ = self.terminate();
        }
    }
}

type OutputReader = JoinHandle<Result<BoundedRead, CommandExecutionError>>;

struct ChildPipes {
    stdout: ChildStdout,
    stderr: ChildStderr,
}

struct ProcessCleanup {
    status: Option<ExitStatus>,
    error: Option<CommandExecutionError>,
}

fn output_readers_finished(stdout: &OutputReader, stderr: &OutputReader) -> bool {
    stdout.is_finished() && stderr.is_finished()
}

fn wait_for_output_readers(
    stdout: &OutputReader,
    stderr: &OutputReader,
    timeout: Duration,
) -> bool {
    let started = Instant::now();
    let deadline = started.checked_add(timeout).unwrap_or(started);
    while !output_readers_finished(stdout, stderr) {
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
    true
}

fn wait_for_output_reader(reader: &OutputReader, timeout: Duration) -> bool {
    let started = Instant::now();
    let deadline = started.checked_add(timeout).unwrap_or(started);
    while !reader.is_finished() {
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
    true
}

fn spawn_output_reader(
    name: &str,
    reader: impl Read + Send + 'static,
    output_cap: usize,
) -> Result<OutputReader, CommandExecutionError> {
    thread::Builder::new()
        .name(name.into())
        .spawn(move || read_bounded(reader, output_cap))
        .map_err(|error| {
            command_error(
                CommandExecutionErrorKind::Io,
                format!("process {name} reader could not start: {error}"),
            )
        })
}

fn wait_for_child_exit(
    child: &mut Child,
    timeout: Duration,
) -> std::io::Result<Option<ExitStatus>> {
    let started = Instant::now();
    let deadline = started.checked_add(timeout).unwrap_or(started);
    loop {
        match child.try_wait()? {
            Some(status) => return Ok(Some(status)),
            None if Instant::now() >= deadline => return Ok(None),
            None => thread::sleep(PROCESS_POLL_INTERVAL),
        }
    }
}

fn cleanup_spawned_process(
    process_tree: &mut ProcessTree,
    child: &mut Child,
    known_status: Option<ExitStatus>,
) -> ProcessCleanup {
    let mut issues = Vec::new();
    if let Err(error) = process_tree.terminate() {
        issues.push(error.message);
    }
    let mut status = known_status;
    if status.is_none() {
        match child.try_wait() {
            Ok(next) => status = next,
            Err(error) => issues.push(format!("direct child status failed: {error}")),
        }
    }
    let mut direct_kill_error = None;
    if status.is_none() {
        direct_kill_error = child.kill().err();
        match wait_for_child_exit(child, PROCESS_CLEANUP_TIMEOUT) {
            Ok(next) => status = next,
            Err(error) => issues.push(format!("direct child reap failed: {error}")),
        }
    }
    if status.is_none() {
        if let Some(error) = direct_kill_error {
            issues.push(format!(
                "direct child fallback could not terminate: {error}"
            ));
        }
        issues.push("direct child did not exit before the cleanup deadline".into());
    }
    ProcessCleanup {
        status,
        error: (!issues.is_empty()).then(|| {
            command_error(
                CommandExecutionErrorKind::Io,
                format!("process cleanup failed: {}", issues.join("; ")),
            )
        }),
    }
}

fn error_after_spawn_cleanup(
    error: CommandExecutionError,
    process_tree: &mut ProcessTree,
    child: &mut Child,
) -> CommandExecutionError {
    let cleanup = cleanup_spawned_process(process_tree, child, None);
    let Some(cleanup_error) = cleanup.error else {
        return error;
    };
    command_error(
        error.kind,
        format!("{}; {}", error.message, cleanup_error.message),
    )
}

fn error_with_cleanup_context(
    error: CommandExecutionError,
    cleanup_error: Option<CommandExecutionError>,
    readers_closed: bool,
) -> CommandExecutionError {
    if cleanup_error.is_none() && readers_closed {
        return error;
    }
    let mut message = error.message;
    if let Some(cleanup_error) = cleanup_error {
        message.push_str("; ");
        message.push_str(&cleanup_error.message);
    }
    if !readers_closed {
        message.push_str("; process output pipes remained open after cleanup");
    }
    command_error(error.kind, message)
}

fn take_child_pipes(
    child: &mut Child,
    process_tree: &mut ProcessTree,
) -> Result<ChildPipes, CommandExecutionError> {
    let Some(stdout) = child.stdout.take() else {
        return Err(error_after_spawn_cleanup(
            command_error(
                CommandExecutionErrorKind::Io,
                "process stdout was unavailable",
            ),
            process_tree,
            child,
        ));
    };
    let Some(stderr) = child.stderr.take() else {
        return Err(error_after_spawn_cleanup(
            command_error(
                CommandExecutionErrorKind::Io,
                "process stderr was unavailable",
            ),
            process_tree,
            child,
        ));
    };
    Ok(ChildPipes { stdout, stderr })
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
    let mut process_tree = ProcessTree::configure(&mut command)?;
    let mut child = command.spawn().map_err(|error| {
        let kind = if error.kind() == std::io::ErrorKind::NotFound {
            CommandExecutionErrorKind::ProgramNotFound
        } else {
            CommandExecutionErrorKind::Io
        };
        command_error(kind, format!("process could not start: {error}"))
    })?;
    if let Err(error) = process_tree.attach(&child) {
        return Err(error_after_spawn_cleanup(
            error,
            &mut process_tree,
            &mut child,
        ));
    }

    let ChildPipes { stdout, stderr } = take_child_pipes(&mut child, &mut process_tree)?;
    let stdout_reader = match spawn_output_reader("stdout", stdout, output_cap) {
        Ok(reader) => reader,
        Err(error) => {
            drop(stderr);
            return Err(error_after_spawn_cleanup(
                error,
                &mut process_tree,
                &mut child,
            ));
        }
    };
    let stderr_reader = match spawn_output_reader("stderr", stderr, output_cap) {
        Ok(reader) => reader,
        Err(error) => {
            let cleanup = cleanup_spawned_process(&mut process_tree, &mut child, None);
            let reader_closed = wait_for_output_reader(&stdout_reader, PROCESS_CLEANUP_TIMEOUT);
            return Err(error_with_cleanup_context(
                error,
                cleanup.error,
                reader_closed,
            ));
        }
    };

    let started = Instant::now();
    let deadline = started.checked_add(timeout).unwrap_or(started);
    let mut status = None;
    let timed_out = loop {
        if status.is_none() {
            match child.try_wait() {
                Ok(Some(next)) => status = Some(next),
                Ok(None) => {}
                Err(error) => {
                    let cleanup = cleanup_spawned_process(&mut process_tree, &mut child, None);
                    let readers_closed = wait_for_output_readers(
                        &stdout_reader,
                        &stderr_reader,
                        PROCESS_CLEANUP_TIMEOUT,
                    );
                    return Err(error_with_cleanup_context(
                        command_error(
                            CommandExecutionErrorKind::Io,
                            format!("process status failed: {error}"),
                        ),
                        cleanup.error,
                        readers_closed,
                    ));
                }
            }
        }
        if status.is_some() && output_readers_finished(&stdout_reader, &stderr_reader) {
            break false;
        }
        if Instant::now() >= deadline {
            let cleanup = cleanup_spawned_process(&mut process_tree, &mut child, status);
            let readers_closed =
                wait_for_output_readers(&stdout_reader, &stderr_reader, PROCESS_CLEANUP_TIMEOUT);
            if cleanup.error.is_some() || !readers_closed {
                return Err(error_with_cleanup_context(
                    command_error(
                        CommandExecutionErrorKind::Io,
                        format!("process timed out after {}ms", timeout.as_millis()),
                    ),
                    cleanup.error,
                    readers_closed,
                ));
            }
            status = cleanup.status;
            break true;
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    };

    if !output_readers_finished(&stdout_reader, &stderr_reader)
        && !wait_for_output_readers(&stdout_reader, &stderr_reader, PROCESS_CLEANUP_TIMEOUT)
    {
        return Err(command_error(
            CommandExecutionErrorKind::Io,
            "process output pipes remained open after process-tree termination",
        ));
    }

    let stdout = stdout_reader
        .join()
        .map_err(|_| command_error(CommandExecutionErrorKind::Io, "stdout reader failed"))??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| command_error(CommandExecutionErrorKind::Io, "stderr reader failed"))??;
    if !timed_out {
        process_tree.terminate()?;
    }
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
        let read = match reader.read(&mut chunk) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(command_error(
                    CommandExecutionErrorKind::Io,
                    format!("process output read failed: {error}"),
                ));
            }
        };
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
    fn host_posix_normalization_and_wsl_projection_are_case_preserving() {
        assert_eq!(
            normalize_absolute_posix_path("/srv/Repo/./feature/").unwrap(),
            "/srv/Repo/feature"
        );
        assert!(normalize_absolute_posix_path("srv/repo").is_err());
        assert!(normalize_absolute_posix_path("/srv/../repo").is_err());
        assert_eq!(
            wsl_host_visible_path("Ubuntu", "/home/User/repo").unwrap(),
            r"\\wsl.localhost\Ubuntu\home\User\repo"
        );
    }

    #[test]
    fn configured_scan_anchor_stays_on_its_owning_backend() {
        assert_eq!(
            configured_execution_path(&ExecutionBackend::Local, "/repo/./linked").unwrap(),
            "/repo/./linked"
        );
        assert_eq!(
            configured_execution_path(
                &ExecutionBackend::Wsl {
                    distro: "Ubuntu".into(),
                },
                r"\\wsl$\ubuntu\home\User\linked",
            )
            .unwrap(),
            "/home/User/linked"
        );
        assert!(
            configured_execution_path(
                &ExecutionBackend::Wsl {
                    distro: "Debian".into(),
                },
                r"\\wsl$\Ubuntu\home\User\linked",
            )
            .is_err()
        );
    }

    #[test]
    fn host_visible_project_keys_collapse_wsl_unc_aliases_only() {
        assert_eq!(
            normalize_host_visible_project_path(r"\\wsl$\Ubuntu\home\User\repo").unwrap(),
            normalize_host_visible_project_path(
                r"\\wsl.localhost\ubuntu\home\User\repo\"
            )
            .unwrap()
        );
        assert_ne!(
            normalize_host_visible_project_path(r"\\wsl$\Ubuntu\home\User\repo").unwrap(),
            normalize_host_visible_project_path(r"\\wsl$\Ubuntu\home\user\repo").unwrap()
        );
    }

    #[cfg(target_os = "linux")]
    fn linux_process_running(pid: u32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|stat| {
                stat.rsplit_once(") ")
                    .and_then(|(_, rest)| rest.chars().next())
            })
            .is_some_and(|state| !matches!(state, 'Z' | 'X'))
    }

    #[cfg(target_os = "linux")]
    fn assert_linux_process_stops(pid: u32) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while linux_process_running(pid) {
            assert!(
                Instant::now() < deadline,
                "process {pid} survived bounded cleanup"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(target_os = "linux")]
    fn wait_for_pid_file(path: &std::path::Path) -> u32 {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok(pid) = std::fs::read_to_string(path)
                && let Ok(pid) = pid.trim().parse()
            {
                return pid;
            }
            assert!(
                Instant::now() < deadline,
                "child did not publish its descendant PID"
            );
            thread::sleep(Duration::from_millis(10));
        }
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
    fn project_onboarding_local_and_wsl_plans_keep_structured_argv() {
        let local = plan_pre_project_local_command(
            &PreProjectLocalContext::from_host_path("/repo with spaces").unwrap(),
            &CommandPlan::new("git", ["init", "--", "name with spaces"]),
        )
        .unwrap();
        assert_eq!(
            local,
            PlannedHostCommand::Process {
                program: "git".into(),
                args: vec!["init".into(), "--".into(), "name with spaces".into()],
                cwd: Some(PathBuf::from("/repo with spaces")),
            }
        );

        let wsl = plan_pre_project_local_command(
            &PreProjectLocalContext::from_host_path(
                r"\\wsl.localhost\Ubuntu\home\u\repo with spaces",
            )
            .unwrap(),
            &CommandPlan::new("git", ["status", "--short"]),
        )
        .unwrap();
        assert_eq!(
            wsl,
            PlannedHostCommand::Process {
                program: "wsl.exe".into(),
                args: [
                    "--distribution",
                    "Ubuntu",
                    "--cd",
                    "/home/u/repo with spaces",
                    "--exec",
                    "git",
                    "status",
                    "--short",
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

    #[cfg(target_os = "linux")]
    #[test]
    fn timeout_kills_descendants_that_keep_inherited_pipes_open() {
        let script = "sleep 30 & child=$!; printf '%s\\n' \"$child\"; exit 0";
        let started = Instant::now();
        let output = run_process(
            "sh",
            &["-c".into(), script.into()],
            None,
            Duration::from_millis(150),
            1024,
        )
        .expect("timed-out process tree should be collected");

        assert!(output.timed_out);
        assert!(started.elapsed() < Duration::from_secs(5));
        let pid = std::str::from_utf8(&output.stdout)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        assert_linux_process_stops(pid);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn completion_kills_descendants_that_close_inherited_pipes() {
        let script = "sleep 30 >/dev/null 2>&1 & child=$!; printf '%s\\n' \"$child\"; exit 0";
        let output = run_process(
            "sh",
            &["-c".into(), script.into()],
            None,
            Duration::from_secs(2),
            1024,
        )
        .expect("completed process tree should be collected");

        assert!(!output.timed_out);
        let pid = std::str::from_utf8(&output.stdout)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        assert_linux_process_stops(pid);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn missing_post_spawn_pipe_terminates_and_reaps_the_process_tree() {
        let pid_path = std::env::temp_dir().join(format!(
            "mini-term-execution-host-{}-missing-pipe.pid",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&pid_path);
        let mut command = Command::new("sh");
        command
            .args([
                "-c",
                "sleep 30 & child=$!; printf '%s\\n' \"$child\" > \"$1\"; wait",
                "sh",
            ])
            .arg(&pid_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut process_tree = ProcessTree::configure(&mut command).unwrap();
        let mut child = command.spawn().unwrap();
        process_tree.attach(&child).unwrap();
        let parent_pid = child.id();
        let descendant_pid = wait_for_pid_file(&pid_path);

        let started = Instant::now();
        let error = match take_child_pipes(&mut child, &mut process_tree) {
            Ok(_) => panic!("missing stdout should fail after cleaning up the child"),
            Err(error) => error,
        };
        assert_eq!(error.kind, CommandExecutionErrorKind::Io);
        assert!(error.message.contains("stdout was unavailable"));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_linux_process_stops(parent_pid);
        assert_linux_process_stops(descendant_pid);
        let _ = std::fs::remove_file(pid_path);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unattached_tree_cleanup_uses_bounded_direct_child_fallback() {
        let mut command = Command::new("sleep");
        command
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut process_tree = ProcessTree::configure(&mut command).unwrap();
        let mut child = command.spawn().unwrap();
        let pid = child.id();

        let started = Instant::now();
        let error = error_after_spawn_cleanup(
            command_error(CommandExecutionErrorKind::Io, "process attachment failed"),
            &mut process_tree,
            &mut child,
        );
        assert!(error.message.contains("process attachment failed"));
        assert!(error.message.contains("process group was not attached"));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_linux_process_stops(pid);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unix_process_tree_disarms_after_kill_and_esrch() {
        let mut running_command = Command::new("sleep");
        running_command
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut running_tree = ProcessTree::configure(&mut running_command).unwrap();
        let mut running_child = running_command.spawn().unwrap();
        running_tree.attach(&running_child).unwrap();
        running_tree.terminate().unwrap();
        assert!(running_tree.process_group_id.is_none());
        running_tree.terminate().unwrap();
        running_child.wait().unwrap();

        let mut exited_command = Command::new("sh");
        exited_command
            .args(["-c", "exit 0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut exited_tree = ProcessTree::configure(&mut exited_command).unwrap();
        let mut exited_child = exited_command.spawn().unwrap();
        exited_tree.attach(&exited_child).unwrap();
        exited_child.wait().unwrap();
        exited_tree.terminate().unwrap();
        assert!(exited_tree.process_group_id.is_none());
        exited_tree.terminate().unwrap();
    }
}
