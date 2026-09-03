use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use git2::Repository;
use parking_lot::{Condvar, Mutex};

use super::porcelain::{parse_porcelain_text, parse_porcelain_z};
use super::{GitAnnotation, WorktreeFact, WorktreePathState, WorktreeScan, WorktreeScanSource};

const SCAN_TIMEOUT: Duration = Duration::from_secs(30);
const GIT_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);
const PROCESS_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const READER_CLEANUP_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
struct RawGitOutput {
    success: bool,
    status_code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

trait GitRunner: Send + Sync + 'static {
    fn run(&self, repo_path: &Path, args: &[&str], timeout: Duration) -> Result<RawGitOutput>;
}

struct SystemGitRunner {
    program: OsString,
    output_limit: usize,
}

impl Default for SystemGitRunner {
    fn default() -> Self {
        Self {
            program: OsString::from("git"),
            output_limit: GIT_OUTPUT_LIMIT,
        }
    }
}

impl SystemGitRunner {
    #[cfg(test)]
    fn with_program(program: impl AsRef<std::ffi::OsStr>) -> Self {
        Self::with_program_and_output_limit(program, GIT_OUTPUT_LIMIT)
    }

    #[cfg(test)]
    fn with_program_and_output_limit(
        program: impl AsRef<std::ffi::OsStr>,
        output_limit: usize,
    ) -> Self {
        Self {
            program: program.as_ref().to_os_string(),
            output_limit,
        }
    }
}

#[derive(Debug)]
struct CapturedPipe {
    bytes: Vec<u8>,
    exceeded_limit: bool,
}

#[cfg(unix)]
struct ProcessTree {
    process_group_id: Option<i32>,
    attached: bool,
}

#[cfg(unix)]
impl ProcessTree {
    fn configure(command: &mut Command) -> io::Result<Self> {
        use std::os::unix::process::CommandExt as _;

        command.process_group(0);
        Ok(Self {
            process_group_id: None,
            attached: false,
        })
    }

    fn attach(&mut self, child: &Child) -> io::Result<()> {
        self.process_group_id = Some(i32::try_from(child.id()).map_err(|error| {
            io::Error::other(format!(
                "process group identity exceeded the platform PID range: {error}"
            ))
        })?);
        self.attached = true;
        Ok(())
    }

    fn terminate(&mut self) -> io::Result<()> {
        const ESRCH: i32 = 3;
        const SIGKILL: i32 = 9;

        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }

        if !self.attached {
            return Err(io::Error::other("process group was not attached"));
        }
        let Some(process_group_id) = self.process_group_id else {
            return Ok(());
        };
        // SAFETY: process_group(0) makes the spawned child's positive PID its
        // group ID; the negative value targets only that command tree.
        if unsafe { kill(-process_group_id, SIGKILL) } == 0 {
            self.process_group_id = None;
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ESRCH) {
            self.process_group_id = None;
            return Ok(());
        }
        Err(error)
    }
}

#[cfg(unix)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

#[cfg(windows)]
mod windows_process_tree {
    use std::ffi::c_void;
    use std::io;
    use std::os::windows::io::AsRawHandle as _;
    use std::os::windows::process::CommandExt as _;
    use std::process::{Child, Command};

    type Handle = *mut c_void;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_SUSPENDED: u32 = 0x0000_0004;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "CreateJobObjectW"]
        fn create_job_object_w(attributes: *const c_void, name: *const u16) -> Handle;
        #[link_name = "AssignProcessToJobObject"]
        fn assign_process_to_job_object(job: Handle, process: Handle) -> i32;
        #[link_name = "TerminateJobObject"]
        fn terminate_job_object(job: Handle, exit_code: u32) -> i32;
        #[link_name = "CloseHandle"]
        fn close_handle(handle: Handle) -> i32;
    }

    #[link(name = "ntdll")]
    unsafe extern "system" {
        #[link_name = "NtResumeProcess"]
        fn nt_resume_process(process: Handle) -> i32;
    }

    pub(super) struct ProcessTree {
        job: Handle,
        attached: bool,
        terminated: bool,
    }

    impl ProcessTree {
        pub(super) fn configure(command: &mut Command) -> io::Result<Self> {
            command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
            // SAFETY: null security attributes and name request a private job
            // whose returned handle is owned by this ProcessTree.
            let job = unsafe { create_job_object_w(std::ptr::null(), std::ptr::null()) };
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                job,
                attached: false,
                terminated: false,
            })
        }

        pub(super) fn attach(&mut self, child: &Child) -> io::Result<()> {
            let process = child.as_raw_handle();
            // SAFETY: both handles remain valid for this call. The child was
            // created suspended, so it cannot create descendants before this
            // assignment succeeds.
            if unsafe { assign_process_to_job_object(self.job, process) } == 0 {
                return Err(io::Error::last_os_error());
            }
            self.attached = true;
            // SAFETY: this is the same valid process handle, and the job is
            // marked attached first so a resume failure follows the job cleanup
            // path instead of leaving a suspended process behind.
            let status = unsafe { nt_resume_process(process) };
            if status < 0 {
                return Err(io::Error::other(format!(
                    "NtResumeProcess failed with NTSTATUS 0x{:08x}",
                    status as u32
                )));
            }
            Ok(())
        }

        pub(super) fn terminate(&mut self) -> io::Result<()> {
            if !self.attached {
                return Err(io::Error::other("process job was not attached"));
            }
            if self.terminated {
                return Ok(());
            }
            // SAFETY: self owns a live job handle until Drop.
            if unsafe { terminate_job_object(self.job, 1) } == 0 {
                return Err(io::Error::last_os_error());
            }
            self.terminated = true;
            Ok(())
        }
    }

    impl Drop for ProcessTree {
        fn drop(&mut self) {
            if self.attached && !self.terminated {
                let _ = self.terminate();
            }
            // SAFETY: configure accepted one non-null owned handle and Drop is
            // its only close path.
            let _ = unsafe { close_handle(self.job) };
        }
    }
}

#[cfg(windows)]
use windows_process_tree::ProcessTree;

type PipeReader = JoinHandle<io::Result<CapturedPipe>>;

struct ChildPipes {
    stdout: std::process::ChildStdout,
    stderr: std::process::ChildStderr,
}

struct ProcessCleanup {
    error: Option<String>,
}

impl GitRunner for SystemGitRunner {
    fn run(&self, repo_path: &Path, args: &[&str], timeout: Duration) -> Result<RawGitOutput> {
        let started = Instant::now();
        let mut command = Command::new(&self.program);
        command
            .args(args)
            .current_dir(repo_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        super::super::git::hide_console_window(&mut command);
        let mut process_tree = ProcessTree::configure(&mut command)?;
        let mut child = command.spawn()?;
        if let Err(error) = process_tree.attach(&child) {
            return Err(error_after_spawn_cleanup(
                format!("git process-tree attachment failed: {error}"),
                &mut process_tree,
                &mut child,
            ));
        }
        let ChildPipes { stdout, stderr } = take_child_pipes(&mut child, &mut process_tree)?;
        let stdout_reader = match spawn_pipe_reader("stdout", stdout, self.output_limit) {
            Ok(reader) => reader,
            Err(error) => {
                drop(stderr);
                return Err(error_after_spawn_cleanup(
                    format!("git stdout reader could not start: {error}"),
                    &mut process_tree,
                    &mut child,
                ));
            }
        };
        let stderr_reader = match spawn_pipe_reader("stderr", stderr, self.output_limit) {
            Ok(reader) => reader,
            Err(error) => {
                let cleanup = cleanup_spawned_process(&mut process_tree, &mut child, None);
                let reader_closed = wait_for_pipe_reader(&stdout_reader, READER_CLEANUP_TIMEOUT);
                return Err(error_with_cleanup_context(
                    format!("git stderr reader could not start: {error}"),
                    cleanup.error,
                    reader_closed,
                ));
            }
        };
        let deadline = started.checked_add(timeout).unwrap_or_else(Instant::now);
        let mut status = None;
        loop {
            if status.is_none() {
                match child.try_wait() {
                    Ok(Some(next)) => status = Some(next),
                    Ok(None) => {}
                    Err(error) => {
                        let cleanup = cleanup_spawned_process(&mut process_tree, &mut child, None);
                        let readers_closed = wait_for_pipe_readers(
                            &stdout_reader,
                            &stderr_reader,
                            READER_CLEANUP_TIMEOUT,
                        );
                        return Err(error_with_cleanup_context(
                            format!("failed to wait for git worktree list: {error}"),
                            cleanup.error,
                            readers_closed,
                        ));
                    }
                }
            }
            if status.is_some() && pipe_readers_finished(&stdout_reader, &stderr_reader) {
                break;
            }
            if Instant::now() >= deadline {
                let reason = if status.is_some() {
                    "git worktree list timed out while draining output pipes".to_string()
                } else {
                    format!(
                        "git worktree list timed out after {}ms",
                        timeout.as_millis()
                    )
                };
                let cleanup = cleanup_spawned_process(&mut process_tree, &mut child, status);
                let readers_closed =
                    wait_for_pipe_readers(&stdout_reader, &stderr_reader, READER_CLEANUP_TIMEOUT);
                return Err(error_with_cleanup_context(
                    reason,
                    cleanup.error,
                    readers_closed,
                ));
            }
            thread::sleep(PROCESS_POLL_INTERVAL);
        }

        let stdout = join_pipe_reader(stdout_reader, "stdout")?;
        let stderr = join_pipe_reader(stderr_reader, "stderr")?;
        let status = status.expect("process status set before successful reader collection");
        process_tree
            .terminate()
            .map_err(|error| anyhow!("git process-tree final cleanup failed: {error}"))?;
        let exceeded = match (stdout.exceeded_limit, stderr.exceeded_limit) {
            (true, true) => Some("stdout and stderr"),
            (true, false) => Some("stdout"),
            (false, true) => Some("stderr"),
            (false, false) => None,
        };
        if let Some(streams) = exceeded {
            return Err(anyhow!(
                "git worktree list {streams} exceeded the {} byte capture limit",
                self.output_limit
            ));
        }
        Ok(RawGitOutput {
            success: status.success(),
            status_code: status.code(),
            stdout: stdout.bytes,
            stderr: stderr.bytes,
        })
    }
}

fn spawn_pipe_reader(
    name: &str,
    pipe: impl Read + Send + 'static,
    output_limit: usize,
) -> io::Result<PipeReader> {
    thread::Builder::new()
        .name(format!("mt-worktree-git-{name}"))
        .spawn(move || read_pipe(pipe, output_limit))
}

fn read_pipe(mut pipe: impl Read, output_limit: usize) -> io::Result<CapturedPipe> {
    let mut bytes = Vec::with_capacity(output_limit.min(8 * 1024));
    let mut buffer = [0_u8; 8 * 1024];
    let mut exceeded_limit = false;
    loop {
        let read = match pipe.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        let remaining = output_limit.saturating_sub(bytes.len());
        let retained = remaining.min(read);
        bytes.extend_from_slice(&buffer[..retained]);
        exceeded_limit |= retained < read;
    }
    Ok(CapturedPipe {
        bytes,
        exceeded_limit,
    })
}

fn pipe_readers_finished(stdout: &PipeReader, stderr: &PipeReader) -> bool {
    stdout.is_finished() && stderr.is_finished()
}

fn wait_for_pipe_readers(stdout: &PipeReader, stderr: &PipeReader, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while !pipe_readers_finished(stdout, stderr) {
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
    true
}

fn wait_for_pipe_reader(reader: &PipeReader, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while !reader.is_finished() {
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
    true
}

fn join_pipe_reader(reader: PipeReader, name: &str) -> Result<CapturedPipe> {
    reader
        .join()
        .map_err(|_| anyhow!("git {name} reader panicked"))?
        .map_err(|error| anyhow!("failed to read git {name}: {error}"))
}

fn wait_for_child_exit(child: &mut Child, timeout: Duration) -> io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
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
        issues.push(format!("process tree could not be terminated: {error}"));
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
        error: (!issues.is_empty()).then(|| issues.join("; ")),
    }
}

fn error_after_spawn_cleanup(
    message: String,
    process_tree: &mut ProcessTree,
    child: &mut Child,
) -> anyhow::Error {
    let cleanup = cleanup_spawned_process(process_tree, child, None);
    error_with_cleanup_context(message, cleanup.error, true)
}

fn error_with_cleanup_context(
    mut message: String,
    cleanup_error: Option<String>,
    readers_closed: bool,
) -> anyhow::Error {
    if let Some(cleanup_error) = cleanup_error {
        message.push_str("; cleanup failed: ");
        message.push_str(&cleanup_error);
    }
    if !readers_closed {
        message.push_str("; output pipes remained open after bounded cleanup");
    }
    anyhow!(message)
}

fn take_child_pipes(child: &mut Child, process_tree: &mut ProcessTree) -> Result<ChildPipes> {
    let Some(stdout) = child.stdout.take() else {
        return Err(error_after_spawn_cleanup(
            "git stdout pipe was not created".into(),
            process_tree,
            child,
        ));
    };
    let Some(stderr) = child.stderr.take() else {
        drop(stdout);
        return Err(error_after_spawn_cleanup(
            "git stderr pipe was not created".into(),
            process_tree,
            child,
        ));
    };
    Ok(ChildPipes { stdout, stderr })
}

#[derive(Default)]
struct RepoState {
    generation: u64,
    last_authoritative: Option<Vec<WorktreeFact>>,
    flights: HashMap<u64, Arc<Flight>>,
}

struct Flight {
    result: Mutex<Option<Result<WorktreeScan, String>>>,
    ready: Condvar,
}

impl Flight {
    fn new() -> Self {
        Self {
            result: Mutex::new(None),
            ready: Condvar::new(),
        }
    }

    fn wait(&self) -> Result<WorktreeScan, String> {
        let mut result = self.result.lock();
        while result.is_none() {
            self.ready.wait(&mut result);
        }
        result.clone().expect("flight result set before notify")
    }

    fn finish(&self, value: Result<WorktreeScan, String>) {
        *self.result.lock() = Some(value);
        self.ready.notify_all();
    }
}

struct WorktreeCatalog<R: GitRunner> {
    runner: R,
    states: Mutex<HashMap<PathBuf, RepoState>>,
}

impl<R: GitRunner> WorktreeCatalog<R> {
    fn new(runner: R) -> Self {
        Self {
            runner,
            states: Mutex::new(HashMap::new()),
        }
    }

    fn generation(&self, repo_path: &Path) -> u64 {
        let key = repository_key(repo_path);
        self.states
            .lock()
            .get(&key)
            .map(|state| state.generation)
            .unwrap_or(0)
    }

    fn invalidate(&self, repo_path: &Path) {
        let key = repository_key(repo_path);
        let mut states = self.states.lock();
        let state = states.entry(key).or_default();
        state.generation = state.generation.wrapping_add(1);
    }

    fn scan(&self, repo_path: &Path) -> Result<WorktreeScan, String> {
        let key = repository_key(repo_path);
        let (generation, flight, owner) = {
            let mut states = self.states.lock();
            let state = states.entry(key.clone()).or_default();
            let generation = state.generation;
            if let Some(flight) = state.flights.get(&generation) {
                (generation, flight.clone(), false)
            } else {
                let flight = Arc::new(Flight::new());
                state.flights.insert(generation, flight.clone());
                (generation, flight, true)
            }
        };

        if !owner {
            return flight.wait();
        }

        let attempted = self
            .scan_authoritative(repo_path, generation)
            .or_else(|warning| self.degraded(repo_path, &key, generation, warning));

        {
            let mut states = self.states.lock();
            let state = states.entry(key).or_default();
            let result = if state.generation != generation {
                let warning = format!(
                    "worktree scan generation {generation} was invalidated by mutation generation {}",
                    state.generation
                );
                if let Some(last) = &state.last_authoritative {
                    Ok(WorktreeScan {
                        generation: state.generation,
                        source: WorktreeScanSource::LastKnown,
                        authoritative: false,
                        worktrees: last.clone(),
                        warning: Some(warning),
                    })
                } else {
                    attempted.map(|mut scan| {
                        scan.generation = state.generation;
                        scan.source = WorktreeScanSource::LastKnown;
                        scan.authoritative = false;
                        scan.warning = Some(warning);
                        scan
                    })
                }
            } else {
                if let Ok(scan) = &attempted
                    && scan.authoritative
                {
                    state.last_authoritative = Some(scan.worktrees.clone());
                }
                attempted
            };
            flight.finish(result.clone());
            state.flights.remove(&generation);
            result
        }
    }

    fn scan_authoritative(
        &self,
        repo_path: &Path,
        generation: u64,
    ) -> Result<WorktreeScan, String> {
        let nul = self
            .runner
            .run(
                repo_path,
                &["worktree", "list", "--porcelain", "-z"],
                SCAN_TIMEOUT,
            )
            .map_err(|err| format!("failed to run NUL porcelain scan: {err:#}"))?;
        if nul.success {
            let mut worktrees = parse_porcelain_z(&nul.stdout)
                .map_err(|err| format!("invalid NUL porcelain output: {err:#}"))?;
            enrich_paths(&mut worktrees, false);
            return Ok(WorktreeScan {
                generation,
                source: WorktreeScanSource::PorcelainZ,
                authoritative: true,
                worktrees,
                warning: None,
            });
        }

        if nul.status_code != Some(129) {
            return Err(command_failure("NUL porcelain scan", &nul));
        }

        let text = self
            .runner
            .run(
                repo_path,
                &["worktree", "list", "--porcelain"],
                SCAN_TIMEOUT,
            )
            .map_err(|err| format!("failed to run text porcelain scan: {err:#}"))?;
        if !text.success {
            return Err(command_failure("text porcelain scan", &text));
        }
        let mut worktrees = parse_porcelain_text(&text.stdout)
            .map_err(|err| format!("invalid text porcelain output: {err:#}"))?;
        enrich_paths(&mut worktrees, true);
        Ok(WorktreeScan {
            generation,
            source: WorktreeScanSource::PorcelainText,
            authoritative: true,
            worktrees,
            warning: None,
        })
    }

    fn degraded(
        &self,
        repo_path: &Path,
        key: &Path,
        generation: u64,
        warning: String,
    ) -> Result<WorktreeScan, String> {
        if let Some(last) = self
            .states
            .lock()
            .get(key)
            .and_then(|state| state.last_authoritative.clone())
        {
            return Ok(WorktreeScan {
                generation,
                source: WorktreeScanSource::LastKnown,
                authoritative: false,
                worktrees: last,
                warning: Some(warning),
            });
        }

        match libgit2_fallback(repo_path) {
            Ok(worktrees) => Ok(WorktreeScan {
                generation,
                source: WorktreeScanSource::Libgit2Fallback,
                authoritative: false,
                worktrees,
                warning: Some(warning),
            }),
            Err(fallback) => Err(format!(
                "{warning}; libgit2 fallback also failed: {fallback:#}"
            )),
        }
    }
}

fn command_failure(label: &str, output: &RawGitOutput) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!(
        "{label} failed with status {:?}: {}",
        output.status_code,
        stderr.trim()
    )
}

fn repository_key(repo_path: &Path) -> PathBuf {
    let candidate = Repository::open(repo_path)
        .ok()
        .map(|repo| common_git_dir(&repo))
        .unwrap_or_else(|| repo_path.to_path_buf());
    std::fs::canonicalize(&candidate).unwrap_or(candidate)
}

pub(super) fn common_git_dir(repo: &Repository) -> PathBuf {
    let git_dir = repo.path();
    let Ok(raw) = std::fs::read_to_string(git_dir.join("commondir")) else {
        return git_dir.to_path_buf();
    };
    let common = Path::new(raw.trim());
    if common.is_absolute() {
        common.to_path_buf()
    } else {
        git_dir.join(common)
    }
}

fn enrich_paths(worktrees: &mut [WorktreeFact], synthesize_prunable: bool) {
    for worktree in worktrees {
        worktree.path_state = match std::fs::metadata(&worktree.path) {
            Ok(_) => WorktreePathState::Present,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => WorktreePathState::Missing,
            Err(_) => WorktreePathState::Unknown,
        };
        if synthesize_prunable
            && !worktree.is_main
            && !worktree.is_bare
            && worktree.locked.is_none()
            && worktree.prunable.is_none()
            && worktree.path_state == WorktreePathState::Missing
        {
            worktree.prunable = Some(GitAnnotation { reason: None });
        }
    }
}

fn libgit2_fallback(repo_path: &Path) -> Result<Vec<WorktreeFact>> {
    let repo = Repository::open(repo_path)?;
    let main_repo = if repo.is_worktree() {
        Repository::open(common_git_dir(&repo))?
    } else {
        repo
    };

    let mut rows = Vec::new();
    if let Some(workdir) = main_repo.workdir() {
        rows.push(fact_from_repo(
            workdir.to_path_buf(),
            &main_repo,
            true,
            false,
        ));
    } else {
        let bare_path = main_repo.path().to_path_buf();
        rows.push(fact_from_repo(bare_path, &main_repo, true, true));
    }

    if let Ok(names) = main_repo.worktrees() {
        for name in names.iter().flatten() {
            let Ok(worktree) = main_repo.find_worktree(name) else {
                continue;
            };
            let path = worktree.path().to_path_buf();
            let repo = Repository::open_from_worktree(&worktree).ok();
            let branch_ref = repo
                .as_ref()
                .and_then(repository_branch_ref)
                .or_else(|| read_registered_branch(main_repo.path(), name));
            let head = repo.as_ref().and_then(repository_head_oid);
            let path_state = if path.exists() {
                WorktreePathState::Present
            } else {
                WorktreePathState::Missing
            };
            rows.push(WorktreeFact {
                path,
                head,
                branch_ref,
                is_main: false,
                is_detached: repo
                    .as_ref()
                    .is_some_and(|repo| repository_branch_ref(repo).is_none()),
                is_bare: false,
                is_sparse: false,
                locked: matches!(
                    worktree.is_locked(),
                    Ok(git2::WorktreeLockStatus::Locked(_))
                )
                .then_some(GitAnnotation { reason: None }),
                prunable: (path_state == WorktreePathState::Missing)
                    .then_some(GitAnnotation { reason: None }),
                path_state,
            });
        }
    }
    Ok(rows)
}

fn fact_from_repo(path: PathBuf, repo: &Repository, is_main: bool, is_bare: bool) -> WorktreeFact {
    let branch_ref = repository_branch_ref(repo);
    WorktreeFact {
        path,
        head: repository_head_oid(repo),
        is_detached: branch_ref.is_none() && repo.head().is_ok(),
        branch_ref,
        is_main,
        is_bare,
        is_sparse: false,
        locked: None,
        prunable: None,
        path_state: WorktreePathState::Present,
    }
}

fn repository_branch_ref(repo: &Repository) -> Option<String> {
    repo.head()
        .ok()
        .filter(|head| head.is_branch())
        .and_then(|head| head.name().map(str::to_string))
}

fn repository_head_oid(repo: &Repository) -> Option<String> {
    repo.head()
        .ok()
        .and_then(|head| head.target())
        .map(|oid| oid.to_string())
}

fn read_registered_branch(main_git_dir: &Path, name: &str) -> Option<String> {
    let head =
        std::fs::read_to_string(main_git_dir.join("worktrees").join(name).join("HEAD")).ok()?;
    head.strip_prefix("ref: ")
        .map(|value| value.trim().to_string())
}

static DEFAULT_CATALOG: std::sync::LazyLock<WorktreeCatalog<SystemGitRunner>> =
    std::sync::LazyLock::new(|| WorktreeCatalog::new(SystemGitRunner::default()));

pub fn scan(repo_path: &Path) -> Result<WorktreeScan> {
    DEFAULT_CATALOG.scan(repo_path).map_err(anyhow::Error::msg)
}

pub fn invalidate(repo_path: &Path) {
    DEFAULT_CATALOG.invalidate(repo_path);
}

pub fn current_generation(repo_path: &Path) -> u64 {
    DEFAULT_CATALOG.generation(repo_path)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[derive(Debug, Clone)]
    struct FakeRunner {
        outputs: Arc<Mutex<VecDeque<RawGitOutput>>>,
        calls: Arc<AtomicUsize>,
        delay: Duration,
    }

    impl FakeRunner {
        fn new(outputs: Vec<RawGitOutput>) -> Self {
            Self {
                outputs: Arc::new(Mutex::new(outputs.into())),
                calls: Arc::new(AtomicUsize::new(0)),
                delay: Duration::ZERO,
            }
        }

        fn delayed(mut self, delay: Duration) -> Self {
            self.delay = delay;
            self
        }
    }

    impl GitRunner for FakeRunner {
        fn run(
            &self,
            _repo_path: &Path,
            _args: &[&str],
            _timeout: Duration,
        ) -> Result<RawGitOutput> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(self.delay);
            self.outputs
                .lock()
                .pop_front()
                .ok_or_else(|| anyhow!("no fake output remaining"))
        }
    }

    fn success(stdout: &[u8]) -> RawGitOutput {
        RawGitOutput {
            success: true,
            status_code: Some(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        }
    }

    fn failure(code: i32, stderr: &str) -> RawGitOutput {
        RawGitOutput {
            success: false,
            status_code: Some(code),
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[test]
    fn pipe_capture_retains_only_the_configured_prefix() {
        let captured = read_pipe(std::io::Cursor::new(b"abcdefgh"), 5).unwrap();
        assert_eq!(captured.bytes, b"abcde");
        assert!(captured.exceeded_limit);

        let exact = read_pipe(std::io::Cursor::new(b"abcde"), 5).unwrap();
        assert_eq!(exact.bytes, b"abcde");
        assert!(!exact.exceeded_limit);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn system_runner_rejects_output_over_the_capture_limit() {
        use std::os::unix::fs::PermissionsExt;

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mini-term-git-output-limit-{nonce}"));
        std::fs::create_dir_all(&root).unwrap();
        let script = root.join("git-output.sh");
        std::fs::write(&script, "#!/bin/sh\nprintf '0123456789abcdef'\n").unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let error = SystemGitRunner::with_program_and_output_limit(&script, 8)
            .run(&root, &["ignored"], Duration::from_secs(1))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("stdout exceeded the 8 byte capture limit"),
            "actual error: {error}"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn missing_post_spawn_pipe_terminates_and_reaps_the_process_tree() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mini-term-git-missing-pipe-{nonce}"));
        std::fs::create_dir_all(&root).unwrap();
        let child_pid_file = root.join("child-pid");
        let mut command = Command::new("sh");
        command
            .args([
                "-c",
                "sleep 30 & child=$!; printf '%s' \"$child\" > \"$1\"; wait",
                "sh",
            ])
            .arg(&child_pid_file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut process_tree = ProcessTree::configure(&mut command).unwrap();
        let mut child = command.spawn().unwrap();
        process_tree.attach(&child).unwrap();
        let parent_pid = child.id().to_string();
        let descendant_pid = wait_for_pid_file(&child_pid_file);

        let started = Instant::now();
        let error = match take_child_pipes(&mut child, &mut process_tree) {
            Ok(_) => panic!("missing stdout should fail after cleanup"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("stdout pipe was not created"));
        assert!(started.elapsed() < Duration::from_secs(4));
        assert_linux_process_exits(&parent_pid);
        assert_linux_process_exits(&descendant_pid);
        std::fs::remove_dir_all(root).ok();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unattached_process_tree_uses_bounded_direct_child_fallback() {
        let mut command = Command::new("sleep");
        command
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut process_tree = ProcessTree::configure(&mut command).unwrap();
        let mut child = command.spawn().unwrap();
        let pid = child.id().to_string();

        let started = Instant::now();
        let error = error_after_spawn_cleanup(
            "git process-tree attachment failed".into(),
            &mut process_tree,
            &mut child,
        )
        .to_string();
        assert!(error.contains("process-tree attachment failed"));
        assert!(error.contains("process group was not attached"));
        assert!(started.elapsed() < Duration::from_secs(4));
        assert_linux_process_exits(&pid);
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

    #[test]
    fn unsupported_nul_mode_falls_back_to_authoritative_text() {
        let runner = FakeRunner::new(vec![
            failure(129, "unknown option z"),
            success(b"worktree /repo\nHEAD abc\nbranch refs/heads/main\n\n"),
        ]);
        let calls = runner.calls.clone();
        let catalog = WorktreeCatalog::new(runner);
        let scan = catalog.scan(Path::new("/repo")).unwrap();
        assert_eq!(scan.source, WorktreeScanSource::PorcelainText);
        assert!(scan.authoritative);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn ordinary_failure_does_not_retry_in_text_mode() {
        let runner = FakeRunner::new(vec![failure(128, "not a repository")]);
        let calls = runner.calls.clone();
        let catalog = WorktreeCatalog::new(runner);
        assert!(catalog.scan(Path::new("/definitely/missing/repo")).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn malformed_refresh_returns_last_authoritative_snapshot() {
        let runner = FakeRunner::new(vec![
            success(b"worktree /repo\0HEAD abc\0branch refs/heads/main\0\0"),
            success(b"HEAD missing-worktree\0\0"),
        ]);
        let catalog = WorktreeCatalog::new(runner);
        let first = catalog.scan(Path::new("/repo")).unwrap();
        assert!(first.authoritative);
        let second = catalog.scan(Path::new("/repo")).unwrap();
        assert!(!second.authoritative);
        assert_eq!(second.source, WorktreeScanSource::LastKnown);
        assert_eq!(second.worktrees, first.worktrees);
    }

    #[test]
    fn concurrent_scans_share_one_flight() {
        let runner = FakeRunner::new(vec![success(
            b"worktree /repo\0HEAD abc\0branch refs/heads/main\0\0",
        )])
        .delayed(Duration::from_millis(100));
        let calls = runner.calls.clone();
        let catalog = Arc::new(WorktreeCatalog::new(runner));
        let mut threads = Vec::new();
        for _ in 0..4 {
            let catalog = catalog.clone();
            threads.push(std::thread::spawn(move || {
                catalog.scan(Path::new("/repo")).unwrap()
            }));
        }
        for thread in threads {
            assert!(thread.join().unwrap().authoritative);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn main_and_linked_paths_share_common_dir_single_flight() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mini-term-common-dir-flight-{nonce}"));
        let repo = root.join("repo");
        let linked = root.join("linked");
        std::fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]);
        run_git(&repo, &["config", "user.email", "test@example.com"]);
        run_git(&repo, &["config", "user.name", "Test"]);
        std::fs::write(repo.join("file.txt"), "one").unwrap();
        run_git(&repo, &["add", "file.txt"]);
        run_git(&repo, &["commit", "-m", "initial"]);
        run_git(
            &repo,
            &["worktree", "add", "-b", "feature", linked.to_str().unwrap()],
        );

        let output = format!(
            "worktree {}\0HEAD abc\0branch refs/heads/main\0\0",
            repo.display()
        );
        let runner =
            FakeRunner::new(vec![success(output.as_bytes())]).delayed(Duration::from_millis(100));
        let calls = runner.calls.clone();
        let catalog = Arc::new(WorktreeCatalog::new(runner));
        let barrier = Arc::new(Barrier::new(3));
        let threads: Vec<_> = [repo.clone(), linked.clone()]
            .into_iter()
            .map(|path| {
                let catalog = catalog.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    catalog.scan(&path).unwrap()
                })
            })
            .collect();
        barrier.wait();
        for thread in threads {
            assert!(thread.join().unwrap().authoritative);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn mutation_generation_fences_an_in_flight_scan() {
        let runner = FakeRunner::new(vec![success(
            b"worktree /repo\0HEAD abc\0branch refs/heads/main\0\0",
        )])
        .delayed(Duration::from_millis(100));
        let catalog = Arc::new(WorktreeCatalog::new(runner));
        let scanning = {
            let catalog = catalog.clone();
            std::thread::spawn(move || catalog.scan(Path::new("/repo")).unwrap())
        };
        std::thread::sleep(Duration::from_millis(20));
        catalog.invalidate(Path::new("/repo"));
        let scan = scanning.join().unwrap();
        assert!(!scan.authoritative);
        assert_eq!(scan.generation, 1);
        assert_eq!(catalog.generation(Path::new("/repo")), 1);
    }

    #[test]
    fn text_enrichment_synthesizes_only_eligible_prunable_rows() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mini-term-missing-worktrees-{nonce}"));
        let mut rows: Vec<WorktreeFact> = ["eligible", "main", "bare", "locked", "prunable"]
            .into_iter()
            .map(|name| WorktreeFact {
                path: root.join(name),
                head: Some("abc".into()),
                branch_ref: Some(format!("refs/heads/{name}")),
                is_main: false,
                is_detached: false,
                is_bare: false,
                is_sparse: false,
                locked: None,
                prunable: None,
                path_state: WorktreePathState::Unknown,
            })
            .collect();
        rows[1].is_main = true;
        rows[2].is_bare = true;
        rows[3].locked = Some(GitAnnotation { reason: None });
        rows[4].prunable = Some(GitAnnotation {
            reason: Some("already stale".into()),
        });

        enrich_paths(&mut rows, true);

        assert!(
            rows.iter()
                .all(|row| row.path_state == WorktreePathState::Missing)
        );
        assert_eq!(rows[0].prunable, Some(GitAnnotation { reason: None }));
        assert!(rows[1].prunable.is_none());
        assert!(rows[2].prunable.is_none());
        assert!(rows[3].prunable.is_none());
        assert_eq!(
            rows[4]
                .prunable
                .as_ref()
                .and_then(|marker| marker.reason.as_deref()),
            Some("already stale")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn system_runner_kills_descendant_that_holds_pipes_after_parent_exit() {
        use std::os::unix::fs::PermissionsExt;

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mini-term-git-runner-timeout-{nonce}"));
        std::fs::create_dir_all(&root).unwrap();
        let parent_pid_file = root.join("parent-pid");
        let child_pid_file = root.join("child-pid");
        let script = root.join("git-sleeper.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s' $$ > '{}'\nsleep 30 &\nchild=$!\nprintf '%s' \"$child\" > '{}'\nexit 0\n",
                parent_pid_file.display(),
                child_pid_file.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let runner = SystemGitRunner::with_program(&script);
        let started = Instant::now();
        let error = runner
            .run(&root, &["ignored"], Duration::from_millis(250))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("timed out while draining output pipes"),
            "actual error: {error}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "timeout cleanup blocked for {:?}",
            started.elapsed()
        );
        let parent_pid = std::fs::read_to_string(parent_pid_file).unwrap();
        let child_pid = std::fs::read_to_string(child_pid_file).unwrap();
        assert_linux_process_exits(parent_pid.trim());
        assert_linux_process_exits(child_pid.trim());
        std::fs::remove_dir_all(root).ok();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn system_runner_kills_descendant_after_successful_leader_exit() {
        use std::os::unix::fs::PermissionsExt;

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mini-term-git-runner-success-{nonce}"));
        std::fs::create_dir_all(&root).unwrap();
        let child_pid_file = root.join("child-pid");
        let script = root.join("git-background.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nsleep 30 >/dev/null 2>&1 &\nchild=$!\nprintf '%s' \"$child\" > '{}'\nexit 0\n",
                child_pid_file.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let output = SystemGitRunner::with_program(&script)
            .run(&root, &["ignored"], Duration::from_secs(2))
            .expect("completed process tree should be collected");
        assert!(output.success);
        let child_pid = wait_for_pid_file(&child_pid_file);
        assert_linux_process_exits(&child_pid);
        std::fs::remove_dir_all(root).ok();
    }

    #[cfg(target_os = "linux")]
    fn assert_linux_process_exits(pid: &str) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while linux_process_running(pid) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !linux_process_running(pid),
            "timed-out child process {pid} is still alive"
        );
    }

    #[cfg(target_os = "linux")]
    fn linux_process_running(pid: &str) -> bool {
        std::fs::read_to_string(Path::new("/proc").join(pid).join("stat"))
            .ok()
            .and_then(|stat| {
                stat.rsplit_once(") ")
                    .and_then(|(_, rest)| rest.chars().next())
            })
            .is_some_and(|state| !matches!(state, 'Z' | 'X'))
    }

    #[cfg(target_os = "linux")]
    fn wait_for_pid_file(path: &Path) -> String {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok(pid) = std::fs::read_to_string(path)
                && !pid.trim().is_empty()
            {
                return pid.trim().to_string();
            }
            assert!(
                Instant::now() < deadline,
                "child did not publish its descendant PID"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn real_git_smoke_test_covers_linked_detached_locked_and_prunable() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mini-term-worktree-catalog-{nonce}"));
        let repo = root.join("repo");
        let linked = root.join("linked");
        let detached = root.join("detached");
        let stale = root.join("stale");
        std::fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]);
        run_git(&repo, &["config", "user.email", "test@example.com"]);
        run_git(&repo, &["config", "user.name", "Test"]);
        std::fs::write(repo.join("file.txt"), "one").unwrap();
        run_git(&repo, &["add", "file.txt"]);
        run_git(&repo, &["commit", "-m", "initial"]);
        run_git(
            &repo,
            &["worktree", "add", "-b", "feature", linked.to_str().unwrap()],
        );
        run_git(
            &repo,
            &["worktree", "add", "--detach", detached.to_str().unwrap()],
        );
        run_git(
            &repo,
            &[
                "worktree",
                "lock",
                "--reason",
                "busy",
                linked.to_str().unwrap(),
            ],
        );
        run_git(
            &repo,
            &["worktree", "add", "-b", "stale", stale.to_str().unwrap()],
        );
        std::fs::remove_dir_all(&stale).unwrap();

        assert_eq!(repository_key(&repo), repository_key(&linked));
        let catalog = WorktreeCatalog::new(SystemGitRunner::default());
        let scan = catalog.scan(&repo).unwrap();
        assert!(scan.authoritative);
        assert!(scan.worktrees.first().is_some_and(|row| row.is_main));
        assert!(scan.worktrees.iter().any(|row| {
            row.path == linked
                && row.branch_ref.as_deref() == Some("refs/heads/feature")
                && row
                    .locked
                    .as_ref()
                    .and_then(|locked| locked.reason.as_deref())
                    == Some("busy")
        }));
        assert!(
            scan.worktrees
                .iter()
                .any(|row| row.path == detached && row.is_detached)
        );
        assert!(
            scan.worktrees
                .iter()
                .any(|row| row.path == stale && row.prunable.is_some())
        );
        catalog.invalidate(&linked);
        assert_eq!(catalog.generation(&repo), 1);
        assert_eq!(catalog.generation(&linked), 1);
        std::fs::remove_dir_all(root).ok();
    }

    fn run_git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
