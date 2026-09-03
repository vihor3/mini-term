use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::{Condvar, Mutex};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, BufReader};
use tokio::sync::{Notify, broadcast};

use mt_identity::{TerminalIncarnationId, TerminalSessionId, WorktreeId};
use mt_pty::{PtyExitStatus, PtyOptions, PtySession, PtySpawn};
use mt_terminal::TerminalSnapshot;

use crate::history::{self, HistorySeed, SessionHistory};
use crate::ipc;
use crate::protocol::{
    ClientRequest, ErrorCode, HostSpawnSpec, PROTOCOL_VERSION, ServerFrame, SessionDescriptor,
    WslOverrideDescriptor, decode_frame, decode_write_bytes, encode_bytes, encode_frame,
    read_frame_line, write_frame_line,
};

pub const DEFAULT_IDLE_EXIT: Duration = Duration::from_secs(10 * 60);
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(30);
const FRAME_WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const HELLO_PROBE_TIMEOUT: Duration = Duration::from_secs(1);
const RESTORE_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
const PTY_OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const REPLAY_BYTES_LIMIT: usize = 64 * 1024 * 1024;
const LIVE_EVENT_CAPACITY: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeOutcome {
    Idle,
    Shutdown,
}

#[derive(Debug)]
pub enum ServeError {
    AlreadyRunning(String),
    Runtime(String),
}

impl ServeError {
    pub fn message(&self) -> &str {
        match self {
            Self::AlreadyRunning(message) | Self::Runtime(message) => message,
        }
    }
}

#[derive(Debug)]
struct HostFailure {
    code: ErrorCode,
    message: String,
}

impl HostFailure {
    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn frame(self) -> ServerFrame {
        ServerFrame::Error {
            code: self.code,
            message: self.message,
        }
    }
}

#[derive(Debug, Clone)]
struct OutputChunk {
    sequence: u64,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
enum StreamEvent {
    Output(OutputChunk),
    Exited(Option<u32>),
    Failed { code: ErrorCode, message: String },
}

struct SessionStream {
    next_sequence: u64,
    retained_bytes: usize,
    chunks: VecDeque<OutputChunk>,
    events: broadcast::Sender<StreamEvent>,
}

impl SessionStream {
    fn new() -> Self {
        let (events, _) = broadcast::channel(LIVE_EVENT_CAPACITY);
        Self {
            next_sequence: 1,
            retained_bytes: 0,
            chunks: VecDeque::new(),
            events,
        }
    }

    fn bounds(&self) -> (u64, u64) {
        let first = self
            .chunks
            .front()
            .map(|chunk| chunk.sequence)
            .unwrap_or(self.next_sequence);
        (first, self.next_sequence.saturating_sub(1))
    }

    fn push(&mut self, bytes: &[u8]) {
        let chunk = OutputChunk {
            sequence: self.next_sequence,
            bytes: bytes.to_vec(),
        };
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.retained_bytes = self.retained_bytes.saturating_add(chunk.bytes.len());
        self.chunks.push_back(chunk.clone());
        while self.retained_bytes > REPLAY_BYTES_LIMIT {
            let Some(removed) = self.chunks.pop_front() else {
                break;
            };
            self.retained_bytes = self.retained_bytes.saturating_sub(removed.bytes.len());
        }
        let _ = self.events.send(StreamEvent::Output(chunk));
    }
}

struct HostedSession {
    session_id: TerminalSessionId,
    incarnation_id: TerminalIncarnationId,
    worktree_id: WorktreeId,
    process_id: AtomicU32,
    lifecycle: Mutex<SessionLifecycle>,
    exit_ready: Condvar,
    size: Mutex<(u16, u16)>,
    wsl_override: Mutex<Option<WslOverrideDescriptor>>,
    stream: Mutex<SessionStream>,
    history: Arc<SessionHistory>,
}

#[derive(Default)]
struct SessionLifecycle {
    pty: Option<PtySession>,
    termination: Option<SessionTermination>,
}

#[derive(Clone)]
enum SessionTermination {
    Exited(Option<u32>),
    Failed { code: ErrorCode, message: String },
}

struct AttachState {
    descriptor: SessionDescriptor,
    replay: Vec<OutputChunk>,
    receiver: broadcast::Receiver<StreamEvent>,
}

impl HostedSession {
    fn spawn(
        session_id: TerminalSessionId,
        worktree_id: WorktreeId,
        mut spawn: HostSpawnSpec,
        history_root: &Path,
        initial_snapshot: Option<&TerminalSnapshot>,
    ) -> Result<Arc<Self>, HostFailure> {
        let incarnation_id = TerminalIncarnationId::new();
        let history = Arc::new(
            SessionHistory::pending(HistorySeed {
                root: history_root,
                session_id: session_id.clone(),
                worktree_id: worktree_id.clone(),
                generation: incarnation_id.clone(),
                rows: spawn.rows,
                cols: spawn.cols,
                scrollback: spawn.scrollback,
                initial_snapshot,
            })
            .map_err(|error| {
                HostFailure::new(ErrorCode::RecoveryUnavailable, format!("{error:#}"))
            })?,
        );
        spawn
            .env
            .retain(|(key, _)| key != "MINITERM_TERMINAL_INCARNATION_ID");
        spawn.env.push((
            "MINITERM_TERMINAL_INCARNATION_ID".into(),
            incarnation_id.to_string(),
        ));

        let session = Arc::new(Self {
            session_id,
            incarnation_id,
            worktree_id,
            process_id: AtomicU32::new(0),
            lifecycle: Mutex::new(SessionLifecycle::default()),
            exit_ready: Condvar::new(),
            size: Mutex::new((spawn.cols, spawn.rows)),
            wsl_override: Mutex::new(None),
            stream: Mutex::new(SessionStream::new()),
            history,
        });

        let output_session = session.clone();
        let exit_session = session.clone();
        let options = PtyOptions::default()
            .with_user_env(spawn.user_env.clone())
            .with_output_drain_timeout(PTY_OUTPUT_DRAIN_TIMEOUT)
            .on_exit_status(move |status| exit_session.note_exit(status));
        let spec = PtySpawn {
            program: spawn.program,
            args: spawn.args,
            cwd: spawn.cwd,
            env: spawn.env,
            rows: spawn.rows,
            cols: spawn.cols,
        };
        let pty = PtySession::spawn_with_options(spec, options, move |bytes| {
            output_session.record_output(bytes);
        })
        .map_err(|error| HostFailure::new(ErrorCode::SpawnFailed, format!("{error:#}")))?;

        session
            .process_id
            .store(pty.process_id().unwrap_or(0), Ordering::Relaxed);
        *session.wsl_override.lock() = pty.wsl_override().map(|value| WslOverrideDescriptor {
            distro: value.distro.clone(),
            unix_path: value.unix_path.clone(),
        });
        if let Some(autofill) = spawn.ssh_autofill {
            pty.arm_ssh_autofill(autofill.password, autofill.disarm_on_input);
        }
        let mut lifecycle = session.lifecycle.lock();
        if lifecycle.termination.is_none() {
            lifecycle.pty = Some(pty);
            drop(lifecycle);
        } else {
            drop(lifecycle);
            drop(pty);
        }
        session.history.activate();
        if matches!(
            session.lifecycle.lock().termination.as_ref(),
            Some(SessionTermination::Failed { .. })
        ) {
            let _ = session.history.invalidate();
        }
        Ok(session)
    }

    fn record_output(&self, bytes: &[u8]) {
        {
            let mut stream = self.stream.lock();
            if self.lifecycle.lock().termination.is_some() {
                return;
            }
            stream.push(bytes);
        }
        self.history.record_output(bytes);
    }

    fn note_exit(&self, status: PtyExitStatus) {
        let termination = match status {
            PtyExitStatus::Drained(exit_code) => SessionTermination::Exited(exit_code),
            PtyExitStatus::OutputDrainFailed(exit_code) => SessionTermination::Failed {
                code: ErrorCode::RecoveryUnavailable,
                message: format!(
                    "terminal output pump failed after child exit with code {exit_code:?}"
                ),
            },
            PtyExitStatus::OutputDrainTimedOut(exit_code) => SessionTermination::Failed {
                code: ErrorCode::RecoveryUnavailable,
                message: format!(
                    "terminal output did not drain after child exit with code {exit_code:?}"
                ),
            },
        };
        self.finish_termination(termination);
    }

    fn finish_termination(&self, mut termination: SessionTermination) {
        let stream = self.stream.lock();
        let mut lifecycle = self.lifecycle.lock();
        if lifecycle.termination.is_some() {
            return;
        }
        match &mut termination {
            SessionTermination::Exited(_) => self.history.flush_checkpoint(),
            SessionTermination::Failed { message, .. } => {
                if let Err(error) = self.history.invalidate() {
                    message.push_str(&format!(
                        "; could not durably invalidate recovery history: {error:#}"
                    ));
                }
            }
        }
        let pty = lifecycle.pty.take();
        lifecycle.termination = Some(termination.clone());
        drop(lifecycle);
        drop(pty);
        let event = match termination {
            SessionTermination::Exited(exit_code) => StreamEvent::Exited(exit_code),
            SessionTermination::Failed { code, message } => StreamEvent::Failed { code, message },
        };
        let _ = stream.events.send(event);
        self.exit_ready.notify_all();
    }

    fn fail_restore_drain(&self) {
        self.finish_termination(SessionTermination::Failed {
            code: ErrorCode::RecoveryUnavailable,
            message: "terminal session did not drain before restore timeout".into(),
        });
    }

    fn is_live(&self) -> bool {
        let lifecycle = self.lifecycle.lock();
        lifecycle.termination.is_none() && lifecycle.pty.is_some()
    }

    fn descriptor_with_stream(&self, stream: &SessionStream) -> SessionDescriptor {
        let (first_sequence, latest_sequence) = stream.bounds();
        let (cols, rows) = *self.size.lock();
        let process_id = match self.process_id.load(Ordering::Relaxed) {
            0 => None,
            value => Some(value),
        };
        SessionDescriptor {
            session_id: self.session_id.clone(),
            incarnation_id: self.incarnation_id.clone(),
            worktree_id: self.worktree_id.clone(),
            process_id,
            rows,
            cols,
            first_sequence,
            latest_sequence,
            wsl_override: self.wsl_override.lock().clone(),
            recovery_available: self.history.is_available(),
        }
    }

    fn descriptor(&self) -> SessionDescriptor {
        let stream = self.stream.lock();
        self.descriptor_with_stream(&stream)
    }

    fn validate_identity(&self, expected: &TerminalIncarnationId) -> Result<(), HostFailure> {
        if &self.incarnation_id != expected {
            return Err(HostFailure::new(
                ErrorCode::IncarnationMismatch,
                format!(
                    "terminal incarnation mismatch for session {}",
                    self.session_id
                ),
            ));
        }
        Ok(())
    }

    fn ensure_live(lifecycle: &SessionLifecycle) -> Result<(), HostFailure> {
        match lifecycle.termination.as_ref() {
            Some(SessionTermination::Exited(exit_code)) => Err(HostFailure::new(
                ErrorCode::SessionExited,
                format!("terminal session exited with code {exit_code:?}"),
            )),
            Some(SessionTermination::Failed { code, message }) => {
                Err(HostFailure::new(*code, message.clone()))
            }
            None => Ok(()),
        }
    }

    fn wait_for_termination(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut lifecycle = self.lifecycle.lock();
        while lifecycle.termination.is_none() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let wait = self.exit_ready.wait_for(&mut lifecycle, remaining);
            if wait.timed_out() && lifecycle.termination.is_none() {
                return false;
            }
        }
        true
    }

    fn recovery_failure(&self) -> Option<HostFailure> {
        match self.lifecycle.lock().termination.as_ref() {
            Some(SessionTermination::Failed { code, message }) => {
                Some(HostFailure::new(*code, message.clone()))
            }
            _ => None,
        }
    }

    fn prepare_attach(
        &self,
        expected: &TerminalIncarnationId,
        after_sequence: u64,
    ) -> Result<AttachState, HostFailure> {
        self.validate_identity(expected)?;
        let stream = self.stream.lock();
        let lifecycle = self.lifecycle.lock();
        Self::ensure_live(&lifecycle)?;
        let (first_sequence, latest_sequence) = stream.bounds();
        if latest_sequence > after_sequence && after_sequence.saturating_add(1) < first_sequence {
            return Err(HostFailure::new(
                ErrorCode::ReplayGap,
                format!(
                    "requested sequence {} but retained output starts at {}",
                    after_sequence.saturating_add(1),
                    first_sequence
                ),
            ));
        }
        let replay = stream
            .chunks
            .iter()
            .filter(|chunk| chunk.sequence > after_sequence)
            .cloned()
            .collect();
        let receiver = stream.events.subscribe();
        let descriptor = self.descriptor_with_stream(&stream);
        drop(lifecycle);
        Ok(AttachState {
            descriptor,
            replay,
            receiver,
        })
    }

    fn write(&self, expected: &TerminalIncarnationId, bytes: &[u8]) -> Result<(), HostFailure> {
        self.validate_identity(expected)?;
        let lifecycle = self.lifecycle.lock();
        Self::ensure_live(&lifecycle)?;
        lifecycle
            .pty
            .as_ref()
            .ok_or_else(|| HostFailure::new(ErrorCode::SessionMissing, "PTY is unavailable"))?
            .write(bytes)
            .map_err(|error| HostFailure::new(ErrorCode::IoFailed, format!("{error:#}")))
    }

    fn resize(
        &self,
        expected: &TerminalIncarnationId,
        rows: u16,
        cols: u16,
    ) -> Result<(), HostFailure> {
        self.validate_identity(expected)?;
        let lifecycle = self.lifecycle.lock();
        Self::ensure_live(&lifecycle)?;
        lifecycle
            .pty
            .as_ref()
            .ok_or_else(|| HostFailure::new(ErrorCode::SessionMissing, "PTY is unavailable"))?
            .resize(rows, cols)
            .map_err(|error| HostFailure::new(ErrorCode::IoFailed, format!("{error:#}")))?;
        *self.size.lock() = (cols, rows);
        self.history.record_resize(rows, cols);
        Ok(())
    }

    fn arm_autofill(
        &self,
        expected: &TerminalIncarnationId,
        password: String,
        disarm_on_input: bool,
    ) -> Result<(), HostFailure> {
        self.validate_identity(expected)?;
        let lifecycle = self.lifecycle.lock();
        Self::ensure_live(&lifecycle)?;
        lifecycle
            .pty
            .as_ref()
            .ok_or_else(|| HostFailure::new(ErrorCode::SessionMissing, "PTY is unavailable"))?
            .arm_ssh_autofill(password, disarm_on_input);
        Ok(())
    }

    fn kill(&self, expected: &TerminalIncarnationId) -> Result<(), HostFailure> {
        self.validate_identity(expected)?;
        let mut lifecycle = self.lifecycle.lock();
        if lifecycle.termination.is_some() {
            return Ok(());
        }
        lifecycle
            .pty
            .as_mut()
            .ok_or_else(|| HostFailure::new(ErrorCode::SessionMissing, "PTY is unavailable"))?
            .kill()
            .map_err(|error| HostFailure::new(ErrorCode::IoFailed, format!("{error:#}")))
    }

    fn close_explicitly(&self, expected: &TerminalIncarnationId) -> Result<(), HostFailure> {
        self.validate_identity(expected)?;
        let stream = self.stream.lock();
        let mut lifecycle = self.lifecycle.lock();
        if lifecycle.termination.is_some() {
            return Ok(());
        }
        let mut pty = lifecycle
            .pty
            .take()
            .ok_or_else(|| HostFailure::new(ErrorCode::SessionMissing, "PTY is unavailable"))?;
        let kill_result = pty
            .kill()
            .map_err(|error| HostFailure::new(ErrorCode::IoFailed, format!("{error:#}")));
        lifecycle.termination = Some(SessionTermination::Exited(None));
        drop(lifecycle);
        drop(pty);
        let _ = stream.events.send(StreamEvent::Exited(None));
        self.exit_ready.notify_all();
        kill_result
    }

    fn retire_uncommitted(&self) -> Result<(), HostFailure> {
        let close = self.close_explicitly(&self.incarnation_id);
        let history = self.history.invalidate_and_wait();
        match (close, history) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(error),
            (Ok(()), Err(error)) => Err(HostFailure::new(
                ErrorCode::IoFailed,
                format!("invalidate uncommitted terminal history: {error:#}"),
            )),
            (Err(close_error), Err(history_error)) => Err(HostFailure::new(
                ErrorCode::IoFailed,
                format!(
                    "invalidate uncommitted terminal history: {history_error:#}; terminal close also failed: {}",
                    close_error.message
                ),
            )),
        }
    }
}

fn restore_cancelled(session_id: &TerminalSessionId) -> HostFailure {
    HostFailure::new(
        ErrorCode::RecoveryUnavailable,
        format!("terminal restore for session {session_id} was cancelled by explicit close"),
    )
}

#[derive(Default)]
struct Registry {
    sessions: HashMap<TerminalSessionId, Arc<HostedSession>>,
    creating: HashSet<TerminalSessionId>,
    restoring: HashMap<TerminalSessionId, TerminalIncarnationId>,
    cancelled_restores: HashSet<TerminalSessionId>,
}

struct DaemonState {
    registry: Mutex<Registry>,
    active_connections: AtomicUsize,
    last_activity_ms: AtomicU64,
    shutdown: Notify,
    history_root: PathBuf,
}

impl DaemonState {
    fn new(history_root: PathBuf) -> Self {
        Self {
            registry: Mutex::new(Registry::default()),
            active_connections: AtomicUsize::new(0),
            last_activity_ms: AtomicU64::new(now_millis()),
            shutdown: Notify::new(),
            history_root,
        }
    }

    fn touch(&self) {
        self.last_activity_ms.store(now_millis(), Ordering::Relaxed);
    }

    fn idle_for(&self) -> Duration {
        Duration::from_millis(
            now_millis().saturating_sub(self.last_activity_ms.load(Ordering::Relaxed)),
        )
    }

    fn live_session_count(&self) -> usize {
        self.registry
            .lock()
            .sessions
            .values()
            .filter(|session| session.is_live())
            .count()
    }

    fn is_busy(&self) -> bool {
        let registry = self.registry.lock();
        !registry.creating.is_empty() || registry.sessions.values().any(|session| session.is_live())
    }

    fn session(&self, id: &TerminalSessionId) -> Result<Arc<HostedSession>, HostFailure> {
        let registry = self.registry.lock();
        if registry.creating.contains(id) {
            return Err(HostFailure::new(
                ErrorCode::SessionCreating,
                format!("terminal session {id} is being created or restored"),
            ));
        }
        registry.sessions.get(id).cloned().ok_or_else(|| {
            HostFailure::new(
                ErrorCode::SessionMissing,
                format!("terminal session {id} does not exist"),
            )
        })
    }

    fn create(
        &self,
        session_id: TerminalSessionId,
        worktree_id: WorktreeId,
        expected_absent: bool,
        spawn: HostSpawnSpec,
    ) -> Result<SessionDescriptor, HostFailure> {
        if !expected_absent {
            return Err(HostFailure::new(
                ErrorCode::InvalidRequest,
                "create requires expected_absent=true",
            ));
        }
        {
            let mut registry = self.registry.lock();
            if registry.creating.contains(&session_id) {
                return Err(HostFailure::new(
                    ErrorCode::SessionCreating,
                    format!("terminal session {session_id} is already being created"),
                ));
            }
            let existing_is_live = registry
                .sessions
                .get(&session_id)
                .is_some_and(|session| session.is_live());
            if existing_is_live {
                return Err(HostFailure::new(
                    ErrorCode::SessionExists,
                    format!("terminal session {session_id} already exists"),
                ));
            }
            registry.sessions.remove(&session_id);
            registry.creating.insert(session_id.clone());
        }

        let result = HostedSession::spawn(
            session_id.clone(),
            worktree_id,
            spawn,
            &self.history_root,
            None,
        );
        let mut registry = self.registry.lock();
        registry.creating.remove(&session_id);
        let session = result?;
        let descriptor = session.descriptor();
        registry.sessions.insert(session_id, session);
        self.touch();
        Ok(descriptor)
    }

    fn restore(
        &self,
        session_id: TerminalSessionId,
        worktree_id: WorktreeId,
        expected_previous_incarnation_id: TerminalIncarnationId,
        spawn: HostSpawnSpec,
    ) -> Result<(SessionDescriptor, TerminalSnapshot), HostFailure> {
        self.restore_with_timeout(
            session_id,
            worktree_id,
            expected_previous_incarnation_id,
            spawn,
            RESTORE_DRAIN_TIMEOUT,
        )
    }

    fn restore_with_timeout(
        &self,
        session_id: TerminalSessionId,
        worktree_id: WorktreeId,
        expected_previous_incarnation_id: TerminalIncarnationId,
        spawn: HostSpawnSpec,
        drain_timeout: Duration,
    ) -> Result<(SessionDescriptor, TerminalSnapshot), HostFailure> {
        self.restore_with_timeout_with_hooks(
            session_id,
            worktree_id,
            expected_previous_incarnation_id,
            spawn,
            drain_timeout,
            (|| {}, |_| {}),
        )
    }

    #[cfg(test)]
    fn restore_with_timeout_after_reserve<F>(
        &self,
        session_id: TerminalSessionId,
        worktree_id: WorktreeId,
        expected_previous_incarnation_id: TerminalIncarnationId,
        spawn: HostSpawnSpec,
        drain_timeout: Duration,
        after_reserve: F,
    ) -> Result<(SessionDescriptor, TerminalSnapshot), HostFailure>
    where
        F: FnOnce(),
    {
        self.restore_with_timeout_with_hooks(
            session_id,
            worktree_id,
            expected_previous_incarnation_id,
            spawn,
            drain_timeout,
            (after_reserve, |_| {}),
        )
    }

    #[cfg(test)]
    fn restore_with_timeout_after_spawn<F>(
        &self,
        session_id: TerminalSessionId,
        worktree_id: WorktreeId,
        expected_previous_incarnation_id: TerminalIncarnationId,
        spawn: HostSpawnSpec,
        drain_timeout: Duration,
        after_spawn: F,
    ) -> Result<(SessionDescriptor, TerminalSnapshot), HostFailure>
    where
        F: FnOnce(&Arc<HostedSession>),
    {
        self.restore_with_timeout_with_hooks(
            session_id,
            worktree_id,
            expected_previous_incarnation_id,
            spawn,
            drain_timeout,
            (|| {}, after_spawn),
        )
    }

    fn restore_with_timeout_with_hooks<F, G>(
        &self,
        session_id: TerminalSessionId,
        worktree_id: WorktreeId,
        expected_previous_incarnation_id: TerminalIncarnationId,
        spawn: HostSpawnSpec,
        drain_timeout: Duration,
        hooks: (F, G),
    ) -> Result<(SessionDescriptor, TerminalSnapshot), HostFailure>
    where
        F: FnOnce(),
        G: FnOnce(&Arc<HostedSession>),
    {
        let (after_reserve, after_spawn) = hooks;
        let existing = {
            let mut registry = self.registry.lock();
            if registry.creating.contains(&session_id) {
                return Err(HostFailure::new(
                    ErrorCode::SessionCreating,
                    format!("terminal session {session_id} is already being restored"),
                ));
            }
            if let Some(existing) = registry.sessions.get(&session_id) {
                existing.validate_identity(&expected_previous_incarnation_id)?;
                if existing.worktree_id != worktree_id {
                    return Err(HostFailure::new(
                        ErrorCode::RecoveryUnavailable,
                        format!("terminal session {session_id} belongs to another worktree"),
                    ));
                }
            }
            registry.creating.insert(session_id.clone());
            registry
                .restoring
                .insert(session_id.clone(), expected_previous_incarnation_id.clone());
            registry.sessions.get(&session_id).cloned()
        };
        after_reserve();

        let result = (|| {
            if self.restore_cancelled(&session_id) {
                return Err(restore_cancelled(&session_id));
            }
            if let Some(existing) = existing.as_ref() {
                if existing.is_live() {
                    existing.kill(&expected_previous_incarnation_id)?;
                    if !existing.wait_for_termination(drain_timeout) {
                        existing.fail_restore_drain();
                        return Err(HostFailure::new(
                            ErrorCode::RecoveryUnavailable,
                            "terminal session did not drain before restore timeout",
                        ));
                    }
                }
                if self.restore_cancelled(&session_id) {
                    return Err(restore_cancelled(&session_id));
                }
                if let Some(error) = existing.recovery_failure() {
                    return Err(error);
                }
                if !existing.history.seal() {
                    let mut message =
                        "terminal history could not be sealed before recovery".to_string();
                    if let Err(error) = existing.history.invalidate() {
                        message.push_str(&format!(
                            "; could not durably invalidate recovery history: {error:#}"
                        ));
                    }
                    return Err(HostFailure::new(ErrorCode::RecoveryUnavailable, message));
                }
            }

            if self.restore_cancelled(&session_id) {
                return Err(restore_cancelled(&session_id));
            }
            let recovered = history::recover(
                &self.history_root,
                &session_id,
                &worktree_id,
                &expected_previous_incarnation_id,
            )
            .map_err(|error| {
                let mut message = format!("terminal history recovery failed: {error:#}");
                if let Some(existing) = existing.as_ref()
                    && let Err(invalidation_error) = existing.history.invalidate()
                {
                    message.push_str(&format!(
                        "; could not durably invalidate recovery history: {invalidation_error:#}"
                    ));
                }
                HostFailure::new(ErrorCode::RecoveryUnavailable, message)
            })?;
            if self.restore_cancelled(&session_id) {
                return Err(restore_cancelled(&session_id));
            }
            let session = HostedSession::spawn(
                session_id.clone(),
                worktree_id.clone(),
                spawn,
                &self.history_root,
                Some(&recovered.snapshot),
            )?;
            after_spawn(&session);
            Ok((session, recovered.snapshot))
        })();

        let mut registry = self.registry.lock();
        registry.creating.remove(&session_id);
        registry.restoring.remove(&session_id);
        let cancelled = registry.cancelled_restores.remove(&session_id);
        let still_owned = existing.as_ref().map_or_else(
            || !registry.sessions.contains_key(&session_id),
            |existing| {
                registry
                    .sessions
                    .get(&session_id)
                    .is_some_and(|registered| Arc::ptr_eq(registered, existing))
            },
        );
        if cancelled || !still_owned {
            drop(registry);
            let mut failure = restore_cancelled(&session_id);
            if let Ok((session, _)) = result
                && let Err(error) = session.retire_uncommitted()
            {
                failure
                    .message
                    .push_str(&format!("; replacement cleanup failed: {}", error.message));
            }
            if let Err(error) = history::purge(&self.history_root, &session_id) {
                failure
                    .message
                    .push_str(&format!("; history purge failed: {error:#}"));
            }
            return Err(failure);
        }
        let (session, snapshot) = result?;
        let descriptor = session.descriptor();
        registry.sessions.insert(session_id, session);
        self.touch();
        Ok((descriptor, snapshot))
    }

    fn restore_cancelled(&self, session_id: &TerminalSessionId) -> bool {
        self.registry.lock().cancelled_restores.contains(session_id)
    }

    fn remove_and_kill(
        &self,
        id: &TerminalSessionId,
        expected: &TerminalIncarnationId,
    ) -> Result<(), HostFailure> {
        let session = {
            let mut registry = self.registry.lock();
            let cancelling_restore = match registry.restoring.get(id) {
                Some(restore_incarnation) if restore_incarnation == expected => {
                    registry.cancelled_restores.insert(id.clone());
                    true
                }
                Some(_) => {
                    return Err(HostFailure::new(
                        ErrorCode::IncarnationMismatch,
                        format!("terminal incarnation mismatch for session {id}"),
                    ));
                }
                None => false,
            };
            let session = registry.sessions.get(id).cloned();
            if let Some(session) = session.as_ref() {
                session.validate_identity(expected)?;
                registry.sessions.remove(id);
            } else if !cancelling_restore {
                return Err(HostFailure::new(
                    ErrorCode::SessionMissing,
                    format!("terminal session {id} does not exist"),
                ));
            }
            session
        };

        let invalidation = match session.as_ref() {
            Some(session) => session.history.invalidate(),
            None => history::invalidate(&self.history_root, id),
        };
        let close = session
            .as_ref()
            .map_or(Ok(()), |session| session.close_explicitly(expected));
        let history_fence = session
            .as_ref()
            .map_or(Ok(()), |session| session.history.invalidate_and_wait());
        let purge = history::purge(&self.history_root, id);
        self.touch();
        match (close, purge) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(error),
            (close, Err(purge_error)) => {
                let mut message = format!("remove terminal recovery history: {purge_error:#}");
                if let Err(invalidation_error) = invalidation {
                    message.push_str(&format!(
                        "; could not durably invalidate recovery history: {invalidation_error:#}"
                    ));
                }
                if let Err(fence_error) = history_fence {
                    message.push_str(&format!(
                        "; could not fence terminal history writes: {fence_error:#}"
                    ));
                }
                if let Err(close_error) = close {
                    message.push_str(&format!(
                        "; terminal close also failed: {}",
                        close_error.message
                    ));
                }
                Err(HostFailure::new(ErrorCode::IoFailed, message))
            }
        }
    }

    fn descriptors(&self) -> Vec<SessionDescriptor> {
        let registry = self.registry.lock();
        let mut descriptors: Vec<_> = registry
            .sessions
            .iter()
            .filter(|(session_id, session)| {
                !registry.creating.contains(*session_id) && session.is_live()
            })
            .map(|(_, session)| session)
            .map(|session| session.descriptor())
            .collect();
        descriptors.sort_by(|left, right| left.session_id.as_str().cmp(right.session_id.as_str()));
        descriptors
    }
}

struct ActiveConnection(Arc<DaemonState>);

impl ActiveConnection {
    fn new(state: Arc<DaemonState>) -> Self {
        state.active_connections.fetch_add(1, Ordering::SeqCst);
        state.touch();
        Self(state)
    }
}

impl Drop for ActiveConnection {
    fn drop(&mut self) {
        self.0.active_connections.fetch_sub(1, Ordering::SeqCst);
        self.0.touch();
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, frame: &ServerFrame) -> bool {
    let Ok(line) = encode_frame(frame) else {
        return false;
    };
    write_frame_line(writer, &line, FRAME_WRITE_TIMEOUT)
        .await
        .is_ok()
}

async fn handle_connection<S>(stream: S, state: Arc<DaemonState>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let _active = ActiveConnection::new(state.clone());
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let hello = ServerFrame::Hello {
        version: env!("CARGO_PKG_VERSION").to_string(),
        protocol_version: PROTOCOL_VERSION,
        pid: std::process::id(),
        live_sessions: state.live_session_count(),
    };
    if !write_frame(&mut writer, &hello).await {
        return;
    }

    let line = match tokio::time::timeout(REQUEST_READ_TIMEOUT, read_frame_line(&mut reader)).await
    {
        Ok(Ok(Some(line))) => line,
        Ok(Err(error)) => {
            let _ = write_frame(
                &mut writer,
                &HostFailure::new(ErrorCode::InvalidRequest, error).frame(),
            )
            .await;
            return;
        }
        Ok(Ok(None)) | Err(_) => return,
    };
    let request = match decode_frame::<ClientRequest>(&line) {
        Ok(request) => request,
        Err(error) => {
            let _ = write_frame(
                &mut writer,
                &HostFailure::new(ErrorCode::InvalidRequest, error).frame(),
            )
            .await;
            return;
        }
    };
    if request.protocol_version() != PROTOCOL_VERSION {
        let _ = write_frame(
            &mut writer,
            &HostFailure::new(
                ErrorCode::ProtocolMismatch,
                format!(
                    "host protocol v{} does not accept request v{}",
                    PROTOCOL_VERSION,
                    request.protocol_version()
                ),
            )
            .frame(),
        )
        .await;
        return;
    }

    if let ClientRequest::Attach {
        session_id,
        expected_incarnation_id,
        after_sequence,
        ..
    } = request
    {
        let session = match state.session(&session_id) {
            Ok(session) => session,
            Err(error) => {
                let _ = write_frame(&mut writer, &error.frame()).await;
                return;
            }
        };
        let mut attachment = match session.prepare_attach(&expected_incarnation_id, after_sequence)
        {
            Ok(attachment) => attachment,
            Err(error) => {
                let _ = write_frame(&mut writer, &error.frame()).await;
                return;
            }
        };
        if !write_frame(
            &mut writer,
            &ServerFrame::Attached {
                descriptor: attachment.descriptor,
            },
        )
        .await
        {
            return;
        }
        for chunk in attachment.replay {
            if !write_frame(
                &mut writer,
                &ServerFrame::Output {
                    sequence: chunk.sequence,
                    data_b64: encode_bytes(&chunk.bytes),
                },
            )
            .await
            {
                return;
            }
        }
        loop {
            let mut peer_byte = [0u8; 1];
            let event = tokio::select! {
                read = reader.read(&mut peer_byte) => {
                    match read {
                        Ok(0) | Err(_) => return,
                        Ok(_) => return,
                    }
                }
                event = attachment.receiver.recv() => event,
            };
            let frame = match event {
                Ok(StreamEvent::Output(chunk)) => ServerFrame::Output {
                    sequence: chunk.sequence,
                    data_b64: encode_bytes(&chunk.bytes),
                },
                Ok(StreamEvent::Exited(exit_code)) => ServerFrame::Exited { exit_code },
                Ok(StreamEvent::Failed { code, message }) => ServerFrame::Error { code, message },
                Err(broadcast::error::RecvError::Lagged(skipped)) => ServerFrame::Error {
                    code: ErrorCode::ReplayGap,
                    message: format!("attachment lagged by {skipped} output events"),
                },
                Err(broadcast::error::RecvError::Closed) => return,
            };
            let terminal = matches!(
                frame,
                ServerFrame::Exited { .. } | ServerFrame::Error { .. }
            );
            if !write_frame(&mut writer, &frame).await || terminal {
                return;
            }
        }
    }

    let mut request_shutdown = false;
    let response = match request {
        ClientRequest::Create {
            session_id,
            worktree_id,
            expected_absent,
            spawn,
            ..
        } => state
            .create(session_id, worktree_id, expected_absent, spawn)
            .map(|descriptor| ServerFrame::Created { descriptor }),
        ClientRequest::Restore {
            session_id,
            worktree_id,
            expected_previous_incarnation_id,
            spawn,
            ..
        } => state
            .restore(
                session_id,
                worktree_id,
                expected_previous_incarnation_id,
                spawn,
            )
            .map(|(descriptor, snapshot)| ServerFrame::Restored {
                descriptor,
                snapshot_b64: encode_bytes(snapshot.as_bytes()),
            }),
        ClientRequest::Write {
            session_id,
            expected_incarnation_id,
            data_b64,
            ..
        } => decode_write_bytes(&data_b64)
            .map_err(|error| HostFailure::new(ErrorCode::InvalidRequest, error))
            .and_then(|bytes| {
                state
                    .session(&session_id)?
                    .write(&expected_incarnation_id, &bytes)
            })
            .map(|()| ServerFrame::Ok),
        ClientRequest::Resize {
            session_id,
            expected_incarnation_id,
            rows,
            cols,
            ..
        } => state
            .session(&session_id)
            .and_then(|session| session.resize(&expected_incarnation_id, rows, cols))
            .map(|()| ServerFrame::Ok),
        ClientRequest::ArmAutofill {
            session_id,
            expected_incarnation_id,
            password,
            disarm_on_input,
            ..
        } => state
            .session(&session_id)
            .and_then(|session| {
                session.arm_autofill(&expected_incarnation_id, password, disarm_on_input)
            })
            .map(|()| ServerFrame::Ok),
        ClientRequest::Kill {
            session_id,
            expected_incarnation_id,
            ..
        } => state
            .remove_and_kill(&session_id, &expected_incarnation_id)
            .map(|()| ServerFrame::Ok),
        ClientRequest::Detach {
            session_id,
            expected_incarnation_id,
            ..
        } => state
            .session(&session_id)
            .and_then(|session| {
                session.validate_identity(&expected_incarnation_id)?;
                let lifecycle = session.lifecycle.lock();
                HostedSession::ensure_live(&lifecycle)
            })
            .map(|()| ServerFrame::Ok),
        ClientRequest::List { .. } => Ok(ServerFrame::Sessions {
            sessions: state.descriptors(),
        }),
        ClientRequest::Status { .. } => Ok(ServerFrame::Status {
            pid: std::process::id(),
            live_sessions: state.live_session_count(),
        }),
        ClientRequest::ShutdownIfIdle { .. } => {
            if !state.is_busy() {
                request_shutdown = true;
                Ok(ServerFrame::Ok)
            } else {
                Err(HostFailure::new(
                    ErrorCode::HostBusy,
                    "terminal host still owns live sessions",
                ))
            }
        }
        ClientRequest::Attach { .. } => unreachable!("attach handled above"),
    };
    let frame = response.unwrap_or_else(HostFailure::frame);
    if write_frame(&mut writer, &frame).await && request_shutdown {
        state.shutdown.notify_one();
    }
}

async fn stream_speaks_host_hello<S>(stream: S) -> bool
where
    S: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(stream);
    matches!(
        tokio::time::timeout(HELLO_PROBE_TIMEOUT, read_frame_line(&mut reader)).await,
        Ok(Ok(Some(line)))
            if matches!(decode_frame::<ServerFrame>(&line), Ok(ServerFrame::Hello { .. }))
    )
}

async fn endpoint_has_live_host(endpoint: &str) -> bool {
    let deadline = tokio::time::Instant::now() + HELLO_PROBE_TIMEOUT;
    loop {
        match ipc::connect(endpoint).await {
            Ok(stream) => return stream_speaks_host_hello(stream).await,
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(_) => return false,
        }
    }
}

pub async fn serve(endpoint: &str, idle_exit: Duration) -> Result<ServeOutcome, ServeError> {
    serve_with_history_root(endpoint, idle_exit, history::default_root()).await
}

pub async fn serve_with_history_root(
    endpoint: &str,
    idle_exit: Duration,
    history_root: PathBuf,
) -> Result<ServeOutcome, ServeError> {
    let state = Arc::new(DaemonState::new(history_root));
    serve_until_exit(endpoint, idle_exit, &state).await
}

fn idle_tick(idle_exit: Duration) -> Duration {
    idle_exit
        .min(Duration::from_secs(30))
        .max(Duration::from_millis(25))
}

fn should_exit_idle(state: &DaemonState, idle_exit: Duration) -> bool {
    state.active_connections.load(Ordering::SeqCst) == 0
        && !state.is_busy()
        && state.idle_for() >= idle_exit
}

#[cfg(unix)]
struct UnixEndpointLock {
    _file: std::fs::File,
}

#[cfg(unix)]
impl UnixEndpointLock {
    async fn acquire(endpoint: &std::path::Path) -> Result<Self, ServeError> {
        use std::os::fd::AsRawFd as _;
        use std::os::unix::fs::PermissionsExt as _;

        let mut name = endpoint.as_os_str().to_os_string();
        name.push(".lock");
        let path = std::path::PathBuf::from(name);
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| ServeError::Runtime(format!("endpoint lock open failed: {error}")))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| ServeError::Runtime(format!("endpoint lock chmod failed: {error}")))?;

        const LOCK_EX: i32 = 2;
        const LOCK_NB: i32 = 4;
        unsafe extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }
        if unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::WouldBlock
                && endpoint_has_live_host(&endpoint.to_string_lossy()).await
            {
                return Err(ServeError::AlreadyRunning(
                    "endpoint is held by a live terminal host".into(),
                ));
            }
            return Err(ServeError::Runtime(format!(
                "endpoint lock acquisition failed: {error}"
            )));
        }
        Ok(Self { _file: file })
    }
}

#[cfg(unix)]
fn socket_identity(path: &std::path::Path) -> Result<Option<(u64, u64)>, ServeError> {
    use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};

    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ServeError::Runtime(format!(
                "endpoint metadata failed: {error}"
            )));
        }
    };
    if !metadata.file_type().is_socket() {
        return Err(ServeError::Runtime(format!(
            "endpoint is not a Unix socket: {}",
            path.display()
        )));
    }
    Ok(Some((metadata.dev(), metadata.ino())))
}

#[cfg(unix)]
async fn bind_unix(path: &std::path::Path) -> Result<tokio::net::UnixListener, ServeError> {
    for _ in 0..3 {
        match tokio::net::UnixListener::bind(path) {
            Ok(listener) => return Ok(listener),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {}
            Err(error) => {
                return Err(ServeError::Runtime(format!(
                    "endpoint bind failed: {error}"
                )));
            }
        }
        let Some(before) = socket_identity(path)? else {
            continue;
        };
        match tokio::net::UnixStream::connect(path).await {
            Ok(stream) => {
                if stream_speaks_host_hello(stream).await {
                    return Err(ServeError::AlreadyRunning(
                        "endpoint is held by a live terminal host".into(),
                    ));
                }
                return Err(ServeError::Runtime(
                    "endpoint accepted a connection without terminal-host hello".into(),
                ));
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                ) => {}
            Err(error) => {
                return Err(ServeError::Runtime(format!(
                    "endpoint liveness probe failed: {error}"
                )));
            }
        }
        let Some(after) = socket_identity(path)? else {
            continue;
        };
        if before != after {
            continue;
        }
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(ServeError::Runtime(format!(
                    "stale endpoint removal failed: {error}"
                )));
            }
        }
    }
    Err(ServeError::Runtime(
        "endpoint changed repeatedly during stale recovery".into(),
    ))
}

#[cfg(unix)]
async fn serve_until_exit(
    endpoint: &str,
    idle_exit: Duration,
    state: &Arc<DaemonState>,
) -> Result<ServeOutcome, ServeError> {
    use std::os::unix::fs::PermissionsExt as _;

    let path = std::path::Path::new(endpoint);
    ipc::prepare_socket_parent(path).map_err(ServeError::Runtime)?;
    let _lock = UnixEndpointLock::acquire(path).await?;
    let listener = bind_unix(path).await?;
    if let Err(error) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        drop(listener);
        let _ = std::fs::remove_file(path);
        return Err(ServeError::Runtime(format!(
            "endpoint chmod 0600 failed: {error}"
        )));
    }
    let mut tick = tokio::time::interval(idle_tick(idle_exit));
    tick.tick().await;
    let outcome = loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted
                    .map_err(|error| ServeError::Runtime(format!("socket accept failed: {error}")))?;
                let state = state.clone();
                tokio::spawn(async move { handle_connection(stream, state).await });
            }
            _ = tick.tick() => {
                if should_exit_idle(state, idle_exit) {
                    break ServeOutcome::Idle;
                }
            }
            _ = state.shutdown.notified() => break ServeOutcome::Shutdown,
        }
    };
    let _ = std::fs::remove_file(path);
    Ok(outcome)
}

#[cfg(windows)]
async fn serve_until_exit(
    endpoint: &str,
    idle_exit: Duration,
    state: &Arc<DaemonState>,
) -> Result<ServeOutcome, ServeError> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let security =
        ipc::windows_security::PipeSecurity::current_user_only().map_err(ServeError::Runtime)?;
    let first = unsafe {
        ServerOptions::new()
            .first_pipe_instance(true)
            .create_with_security_attributes_raw(endpoint, security.attributes_ptr())
    };
    let mut server = match first {
        Ok(server) => server,
        Err(error) if endpoint_has_live_host(endpoint).await => {
            return Err(ServeError::AlreadyRunning(format!(
                "endpoint is held by a live terminal host: {error}"
            )));
        }
        Err(error) => {
            return Err(ServeError::Runtime(format!(
                "named pipe bind failed: {error}"
            )));
        }
    };
    let mut tick = tokio::time::interval(idle_tick(idle_exit));
    tick.tick().await;
    loop {
        tokio::select! {
            connected = server.connect() => {
                connected.map_err(|error| ServeError::Runtime(format!("pipe accept failed: {error}")))?;
                let next = unsafe {
                    ServerOptions::new()
                        .create_with_security_attributes_raw(endpoint, security.attributes_ptr())
                }
                .map_err(|error| ServeError::Runtime(format!("pipe recreate failed: {error}")))?;
                let stream = std::mem::replace(&mut server, next);
                let state = state.clone();
                tokio::spawn(async move { handle_connection(stream, state).await });
            }
            _ = tick.tick() => {
                if should_exit_idle(state, idle_exit) {
                    return Ok(ServeOutcome::Idle);
                }
            }
            _ = state.shutdown.notified() => return Ok(ServeOutcome::Shutdown),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn test_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("mth-server-{label}-{}-{nonce}", std::process::id()))
    }

    fn test_worktree_id() -> WorktreeId {
        format!("worktree-v1:{}", "0".repeat(64)).parse().unwrap()
    }

    fn test_spawn() -> HostSpawnSpec {
        HostSpawnSpec {
            program: if cfg!(windows) { "cmd.exe" } else { "/bin/sh" }.into(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            user_env: Vec::new(),
            rows: 24,
            cols: 80,
            scrollback: 100,
            ssh_autofill: None,
        }
    }

    #[cfg(unix)]
    fn process_is_alive(process_id: u32) -> bool {
        unsafe extern "C" {
            fn kill(process_id: i32, signal: i32) -> i32;
        }

        unsafe { kill(process_id as i32, 0) == 0 }
    }

    #[cfg(unix)]
    fn wait_for_process_exit(process_id: u32) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while process_is_alive(process_id) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !process_is_alive(process_id),
            "cancelled restore leaked process {process_id}"
        );
    }

    #[test]
    fn replay_bounds_start_at_one_and_advance() {
        let mut stream = SessionStream::new();
        assert_eq!(stream.bounds(), (1, 0));
        stream.push(b"a");
        stream.push(b"b");
        assert_eq!(stream.bounds(), (1, 2));
    }

    #[test]
    fn restore_timeout_reinserts_a_failed_incarnation_and_invalidates_history() {
        mt_pty::conpty::initialize_default();
        let root = test_root("restore-timeout");
        let state = DaemonState::new(root.clone());
        let session_id = TerminalSessionId::new();
        let worktree_id = test_worktree_id();
        let descriptor = state
            .create(session_id.clone(), worktree_id.clone(), true, test_spawn())
            .unwrap();

        let error = state
            .restore_with_timeout(
                session_id.clone(),
                worktree_id.clone(),
                descriptor.incarnation_id.clone(),
                test_spawn(),
                Duration::ZERO,
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RecoveryUnavailable);
        assert_eq!(
            state.session(&session_id).unwrap().incarnation_id,
            descriptor.incarnation_id
        );

        let failed = state.session(&session_id).unwrap();
        assert!(
            !failed.is_live(),
            "restore timeout must not reinsert an unusable live capability"
        );
        let attach_error = failed
            .prepare_attach(&descriptor.incarnation_id, 0)
            .err()
            .expect("failed incarnation must reject attach");
        assert_eq!(attach_error.code, ErrorCode::RecoveryUnavailable);

        let restarted = DaemonState::new(root.clone());
        let restart_error = restarted
            .restore(
                session_id.clone(),
                worktree_id,
                descriptor.incarnation_id.clone(),
                test_spawn(),
            )
            .unwrap_err();
        assert_eq!(restart_error.code, ErrorCode::RecoveryUnavailable);
        assert!(restart_error.message.contains("invalidated"));
        state
            .remove_and_kill(&session_id, &descriptor.incarnation_id)
            .unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn explicit_close_cancels_a_reserved_cold_restore_without_resurrection() {
        let root = test_root("close-restore-race");
        let state = Arc::new(DaemonState::new(root.clone()));
        let session_id = TerminalSessionId::new();
        let worktree_id = test_worktree_id();
        let generation = TerminalIncarnationId::new();
        let history = SessionHistory::pending(HistorySeed {
            root: &root,
            session_id: session_id.clone(),
            worktree_id: worktree_id.clone(),
            generation: generation.clone(),
            rows: 24,
            cols: 80,
            scrollback: 100,
            initial_snapshot: None,
        })
        .unwrap();
        assert!(history.activate());
        history.record_output(b"restorable history");
        assert!(history.seal());

        let (reserved_tx, reserved_rx) = std::sync::mpsc::channel();
        let (continue_tx, continue_rx) = std::sync::mpsc::channel();
        let restore_state = state.clone();
        let restore_session_id = session_id.clone();
        let restore_worktree_id = worktree_id.clone();
        let restore_generation = generation.clone();
        let restore = std::thread::spawn(move || {
            restore_state.restore_with_timeout_after_reserve(
                restore_session_id,
                restore_worktree_id,
                restore_generation,
                test_spawn(),
                Duration::from_secs(1),
                || {
                    reserved_tx.send(()).unwrap();
                    continue_rx.recv().unwrap();
                },
            )
        });

        reserved_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        state.remove_and_kill(&session_id, &generation).unwrap();
        continue_tx.send(()).unwrap();
        let restore_error = restore.join().unwrap().unwrap_err();
        assert_eq!(restore_error.code, ErrorCode::RecoveryUnavailable);
        assert!(
            restore_error
                .message
                .contains("cancelled by explicit close")
        );
        assert!(!state.registry.lock().sessions.contains_key(&session_id));

        let restarted = DaemonState::new(root.clone());
        let restart_error = restarted
            .restore(session_id, worktree_id, generation, test_spawn())
            .unwrap_err();
        assert_eq!(restart_error.code, ErrorCode::RecoveryUnavailable);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn explicit_close_after_restore_spawn_retires_the_replacement() {
        mt_pty::conpty::initialize_default();
        let root = test_root("close-after-restore-spawn");
        let state = Arc::new(DaemonState::new(root.clone()));
        let session_id = TerminalSessionId::new();
        let worktree_id = test_worktree_id();
        let generation = TerminalIncarnationId::new();
        let history = SessionHistory::pending(HistorySeed {
            root: &root,
            session_id: session_id.clone(),
            worktree_id: worktree_id.clone(),
            generation: generation.clone(),
            rows: 24,
            cols: 80,
            scrollback: 100,
            initial_snapshot: None,
        })
        .unwrap();
        assert!(history.activate());
        history.record_output(b"restorable history");
        assert!(history.seal());
        drop(history);

        let (spawned_tx, spawned_rx) = std::sync::mpsc::channel();
        let (continue_tx, continue_rx) = std::sync::mpsc::channel();
        let restore_state = state.clone();
        let restore_session_id = session_id.clone();
        let restore_worktree_id = worktree_id.clone();
        let restore_generation = generation.clone();
        let restore = std::thread::spawn(move || {
            restore_state.restore_with_timeout_after_spawn(
                restore_session_id,
                restore_worktree_id,
                restore_generation,
                test_spawn(),
                Duration::from_secs(1),
                |session| {
                    spawned_tx
                        .send(session.descriptor().process_id.unwrap())
                        .unwrap();
                    continue_rx.recv().unwrap();
                },
            )
        });

        let replacement_process_id = spawned_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        state.remove_and_kill(&session_id, &generation).unwrap();
        continue_tx.send(()).unwrap();
        let restore_error = restore.join().unwrap().unwrap_err();
        assert_eq!(restore_error.code, ErrorCode::RecoveryUnavailable);
        assert!(
            restore_error
                .message
                .contains("cancelled by explicit close")
        );
        wait_for_process_exit(replacement_process_id);
        assert!(!state.registry.lock().sessions.contains_key(&session_id));
        assert!(
            std::fs::read_dir(&root)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(true),
            "cancelled restore left terminal history behind"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn attachment_peer_eof_releases_the_server_connection() {
        mt_pty::conpty::initialize_default();
        let root = test_root("peer-eof");
        let state = Arc::new(DaemonState::new(root.clone()));
        let session_id = TerminalSessionId::new();
        let descriptor = state
            .create(session_id.clone(), test_worktree_id(), true, test_spawn())
            .unwrap();

        let (client, server) = tokio::io::duplex(64 * 1024);
        let task = tokio::spawn(handle_connection(server, state.clone()));
        let (reader, mut writer) = tokio::io::split(client);
        let mut reader = BufReader::new(reader);

        let hello = read_frame_line(&mut reader).await.unwrap().unwrap();
        assert!(matches!(
            decode_frame::<ServerFrame>(&hello).unwrap(),
            ServerFrame::Hello { .. }
        ));
        let request = ClientRequest::Attach {
            v: PROTOCOL_VERSION,
            session_id: session_id.clone(),
            expected_incarnation_id: descriptor.incarnation_id.clone(),
            after_sequence: 0,
        };
        let line = encode_frame(&request).unwrap();
        write_frame_line(&mut writer, &line, Duration::from_secs(1))
            .await
            .unwrap();
        let attached = read_frame_line(&mut reader).await.unwrap().unwrap();
        assert!(matches!(
            decode_frame::<ServerFrame>(&attached).unwrap(),
            ServerFrame::Attached { .. }
        ));

        drop(reader);
        drop(writer);
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("attachment server did not observe peer EOF")
            .unwrap();
        assert_eq!(state.active_connections.load(Ordering::SeqCst), 0);

        state
            .remove_and_kill(&session_id, &descriptor.incarnation_id)
            .unwrap();
        let _ = std::fs::remove_dir_all(root);
    }
}
