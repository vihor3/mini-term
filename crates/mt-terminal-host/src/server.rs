use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{Notify, broadcast};

use mt_identity::{TerminalIncarnationId, TerminalSessionId, WorktreeId};
use mt_pty::{PtyOptions, PtySession, PtySpawn};

use crate::ipc;
use crate::protocol::{
    ClientRequest, ErrorCode, HostSpawnSpec, PROTOCOL_VERSION, ServerFrame, SessionDescriptor,
    WslOverrideDescriptor, decode_bytes, decode_frame, encode_bytes, encode_frame,
};

pub const DEFAULT_IDLE_EXIT: Duration = Duration::from_secs(10 * 60);
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(30);
const HELLO_PROBE_TIMEOUT: Duration = Duration::from_secs(1);
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
    pty: Mutex<Option<PtySession>>,
    size: Mutex<(u16, u16)>,
    wsl_override: Mutex<Option<WslOverrideDescriptor>>,
    stream: Mutex<SessionStream>,
    exit: Mutex<Option<Option<u32>>>,
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
    ) -> Result<Arc<Self>, HostFailure> {
        let incarnation_id = TerminalIncarnationId::new();
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
            pty: Mutex::new(None),
            size: Mutex::new((spawn.cols, spawn.rows)),
            wsl_override: Mutex::new(None),
            stream: Mutex::new(SessionStream::new()),
            exit: Mutex::new(None),
        });

        let output_session = session.clone();
        let exit_session = session.clone();
        let options = PtyOptions::default()
            .with_user_env(spawn.user_env.clone())
            .on_exit(move |exit_code| exit_session.note_exit(exit_code));
        let spec = PtySpawn {
            program: spawn.program,
            args: spawn.args,
            cwd: spawn.cwd,
            env: spawn.env,
            rows: spawn.rows,
            cols: spawn.cols,
        };
        let pty = PtySession::spawn_with_options(spec, options, move |bytes| {
            output_session.stream.lock().push(bytes);
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
        *session.pty.lock() = Some(pty);
        Ok(session)
    }

    fn note_exit(&self, exit_code: Option<u32>) {
        let mut exit = self.exit.lock();
        if exit.is_some() {
            return;
        }
        *exit = Some(exit_code);
        let stream = self.stream.lock();
        let _ = stream.events.send(StreamEvent::Exited(exit_code));
    }

    fn is_live(&self) -> bool {
        self.exit.lock().is_none() && self.pty.lock().is_some()
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
        }
    }

    fn descriptor(&self) -> SessionDescriptor {
        let stream = self.stream.lock();
        self.descriptor_with_stream(&stream)
    }

    fn validate_incarnation(&self, expected: &TerminalIncarnationId) -> Result<(), HostFailure> {
        if &self.incarnation_id != expected {
            return Err(HostFailure::new(
                ErrorCode::IncarnationMismatch,
                format!(
                    "terminal incarnation mismatch for session {}",
                    self.session_id
                ),
            ));
        }
        if let Some(exit_code) = *self.exit.lock() {
            return Err(HostFailure::new(
                ErrorCode::SessionExited,
                format!("terminal session exited with code {exit_code:?}"),
            ));
        }
        Ok(())
    }

    fn prepare_attach(
        &self,
        expected: &TerminalIncarnationId,
        after_sequence: u64,
    ) -> Result<AttachState, HostFailure> {
        self.validate_incarnation(expected)?;
        let stream = self.stream.lock();
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
        Ok(AttachState {
            descriptor,
            replay,
            receiver,
        })
    }

    fn write(&self, expected: &TerminalIncarnationId, bytes: &[u8]) -> Result<(), HostFailure> {
        self.validate_incarnation(expected)?;
        self.pty
            .lock()
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
        self.validate_incarnation(expected)?;
        self.pty
            .lock()
            .as_ref()
            .ok_or_else(|| HostFailure::new(ErrorCode::SessionMissing, "PTY is unavailable"))?
            .resize(rows, cols)
            .map_err(|error| HostFailure::new(ErrorCode::IoFailed, format!("{error:#}")))?;
        *self.size.lock() = (cols, rows);
        Ok(())
    }

    fn arm_autofill(
        &self,
        expected: &TerminalIncarnationId,
        password: String,
        disarm_on_input: bool,
    ) -> Result<(), HostFailure> {
        self.validate_incarnation(expected)?;
        self.pty
            .lock()
            .as_ref()
            .ok_or_else(|| HostFailure::new(ErrorCode::SessionMissing, "PTY is unavailable"))?
            .arm_ssh_autofill(password, disarm_on_input);
        Ok(())
    }

    fn kill(&self, expected: &TerminalIncarnationId) -> Result<(), HostFailure> {
        self.validate_incarnation(expected)?;
        let mut pty = self.pty.lock();
        pty.as_mut()
            .ok_or_else(|| HostFailure::new(ErrorCode::SessionMissing, "PTY is unavailable"))?
            .kill()
            .map_err(|error| HostFailure::new(ErrorCode::IoFailed, format!("{error:#}")))
    }
}

#[derive(Default)]
struct Registry {
    sessions: HashMap<TerminalSessionId, Arc<HostedSession>>,
    creating: HashSet<TerminalSessionId>,
}

struct DaemonState {
    registry: Mutex<Registry>,
    active_connections: AtomicUsize,
    last_activity_ms: AtomicU64,
    shutdown: Notify,
}

impl DaemonState {
    fn new() -> Self {
        Self {
            registry: Mutex::new(Registry::default()),
            active_connections: AtomicUsize::new(0),
            last_activity_ms: AtomicU64::new(now_millis()),
            shutdown: Notify::new(),
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

    fn session(&self, id: &TerminalSessionId) -> Result<Arc<HostedSession>, HostFailure> {
        self.registry
            .lock()
            .sessions
            .get(id)
            .cloned()
            .ok_or_else(|| {
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
            if !registry.creating.insert(session_id.clone()) {
                return Err(HostFailure::new(
                    ErrorCode::SessionCreating,
                    format!("terminal session {session_id} is already being created"),
                ));
            }
        }

        let result = HostedSession::spawn(session_id.clone(), worktree_id, spawn);
        let mut registry = self.registry.lock();
        registry.creating.remove(&session_id);
        let session = result?;
        let descriptor = session.descriptor();
        registry.sessions.insert(session_id, session);
        self.touch();
        Ok(descriptor)
    }

    fn remove_and_kill(
        &self,
        id: &TerminalSessionId,
        expected: &TerminalIncarnationId,
    ) -> Result<(), HostFailure> {
        let session = self.session(id)?;
        session.kill(expected)?;
        self.registry.lock().sessions.remove(id);
        self.touch();
        Ok(())
    }

    fn descriptors(&self) -> Vec<SessionDescriptor> {
        let mut descriptors: Vec<_> = self
            .registry
            .lock()
            .sessions
            .values()
            .filter(|session| session.is_live())
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
    writer.write_all(line.as_bytes()).await.is_ok() && writer.flush().await.is_ok()
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

    let mut line = String::new();
    let read = tokio::time::timeout(REQUEST_READ_TIMEOUT, reader.read_line(&mut line)).await;
    if !matches!(read, Ok(Ok(count)) if count > 0) {
        return;
    }
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
            let frame = match attachment.receiver.recv().await {
                Ok(StreamEvent::Output(chunk)) => ServerFrame::Output {
                    sequence: chunk.sequence,
                    data_b64: encode_bytes(&chunk.bytes),
                },
                Ok(StreamEvent::Exited(exit_code)) => ServerFrame::Exited { exit_code },
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
        ClientRequest::Write {
            session_id,
            expected_incarnation_id,
            data_b64,
            ..
        } => decode_bytes(&data_b64)
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
            .and_then(|session| session.validate_incarnation(&expected_incarnation_id))
            .map(|()| ServerFrame::Ok),
        ClientRequest::List { .. } => Ok(ServerFrame::Sessions {
            sessions: state.descriptors(),
        }),
        ClientRequest::Status { .. } => Ok(ServerFrame::Status {
            pid: std::process::id(),
            live_sessions: state.live_session_count(),
        }),
        ClientRequest::ShutdownIfIdle { .. } => {
            if state.live_session_count() == 0 {
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
    let mut line = String::new();
    matches!(
        tokio::time::timeout(HELLO_PROBE_TIMEOUT, reader.read_line(&mut line)).await,
        Ok(Ok(count))
            if count > 0
                && matches!(decode_frame::<ServerFrame>(&line), Ok(ServerFrame::Hello { .. }))
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
    let state = Arc::new(DaemonState::new());
    serve_until_exit(endpoint, idle_exit, &state).await
}

fn idle_tick(idle_exit: Duration) -> Duration {
    idle_exit
        .min(Duration::from_secs(30))
        .max(Duration::from_millis(25))
}

fn should_exit_idle(state: &DaemonState, idle_exit: Duration) -> bool {
    state.active_connections.load(Ordering::SeqCst) == 0
        && state.live_session_count() == 0
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
    use super::*;

    #[test]
    fn replay_bounds_start_at_one_and_advance() {
        let mut stream = SessionStream::new();
        assert_eq!(stream.bounds(), (1, 0));
        stream.push(b"a");
        stream.push(b"b");
        assert_eq!(stream.bounds(), (1, 2));
    }
}
