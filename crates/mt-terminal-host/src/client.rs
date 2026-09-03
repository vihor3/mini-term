use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Child as ProcessChild, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::io::{AsyncBufRead, BufReader};
use tokio::sync::{mpsc, oneshot, watch};

use mt_identity::{TerminalIncarnationId, TerminalSessionId, WorktreeId};
use mt_terminal::TerminalSnapshot;

use crate::ipc;
use crate::protocol::{
    ClientRequest, ErrorCode, HostSpawnSpec, MAX_WRITE_BYTES, PROTOCOL_VERSION, ServerFrame,
    SessionDescriptor, decode_bytes, decode_frame, encode_frame, encode_write_bytes,
    read_frame_line, write_frame_line_until,
};

const CONNECT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(2);
const HOST_START_TIMEOUT: Duration = Duration::from_secs(2);
const RPC_TIMEOUT: Duration = Duration::from_secs(10);
const START_RETRY_DELAY: Duration = Duration::from_millis(35);
const COMMAND_QUEUE_CAPACITY: usize = 32;
const HOST_BIN_ENV: &str = "MINITERM_TERMINAL_HOST_BIN";
const HOST_ENABLED_ENV: &str = "MINI_TERM_TERMINAL_HOST";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientError {
    code: Option<ErrorCode>,
    message: String,
}

impl ClientError {
    fn transport(message: impl Into<String>) -> Self {
        Self {
            code: None,
            message: message.into(),
        }
    }

    fn protocol(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code: Some(code),
            message: message.into(),
        }
    }

    pub fn recovery_unavailable(message: impl Into<String>) -> Self {
        Self::protocol(ErrorCode::RecoveryUnavailable, message)
    }

    pub fn code(&self) -> Option<ErrorCode> {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn is_code(&self, code: ErrorCode) -> bool {
        self.code == Some(code)
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(code) = self.code {
            write!(formatter, "terminal host {code:?}: {}", self.message)
        } else {
            formatter.write_str(&self.message)
        }
    }
}

impl std::error::Error for ClientError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostedEvent {
    Output { sequence: u64, bytes: Vec<u8> },
    Exited { exit_code: Option<u32> },
    Disconnected(ClientError),
}

enum SessionCommand {
    Write(Vec<u8>),
    Resize {
        rows: u16,
        cols: u16,
    },
    ArmAutofill {
        password: String,
        disarm_on_input: bool,
    },
}

struct ClientInner {
    endpoint: String,
    host_binary: Option<PathBuf>,
    auto_spawn: bool,
    runtime: tokio::runtime::Runtime,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum HostStartState {
    #[default]
    Pending,
    Cancelled,
    Committed,
}

#[derive(Default)]
struct HostStartFence {
    state: Mutex<HostStartState>,
}

impl HostStartFence {
    fn run<T>(
        &self,
        spawn: impl FnOnce() -> Result<T, ClientError>,
        commit: impl FnOnce(T) -> Result<(), ClientError>,
        cleanup: impl FnOnce(T),
    ) -> Result<(), ClientError> {
        if *self.state.lock() != HostStartState::Pending {
            return Err(host_start_cancelled());
        }
        let resource = spawn()?;
        let committed = {
            let mut state = self.state.lock();
            if *state == HostStartState::Pending {
                *state = HostStartState::Committed;
                true
            } else {
                false
            }
        };
        if committed {
            commit(resource)
        } else {
            cleanup(resource);
            Err(host_start_cancelled())
        }
    }

    fn cancel(&self) -> HostStartState {
        let mut state = self.state.lock();
        if *state == HostStartState::Pending {
            *state = HostStartState::Cancelled;
        }
        *state
    }
}

fn host_start_cancelled() -> ClientError {
    ClientError::transport("terminal host start was cancelled before process commit")
}

struct AttachmentState {
    error: Mutex<Option<ClientError>>,
    failure_tx: watch::Sender<Option<ClientError>>,
}

impl AttachmentState {
    fn new() -> (Arc<Self>, watch::Receiver<Option<ClientError>>) {
        let (failure_tx, failure_rx) = watch::channel(None);
        (
            Arc::new(Self {
                error: Mutex::new(None),
                failure_tx,
            }),
            failure_rx,
        )
    }

    fn error(&self) -> Option<ClientError> {
        self.error.lock().clone()
    }

    fn fail(&self, error: ClientError) -> ClientError {
        let mut stored = self.error.lock();
        if let Some(existing) = stored.as_ref() {
            return existing.clone();
        }
        *stored = Some(error.clone());
        drop(stored);
        self.failure_tx.send_replace(Some(error.clone()));
        error
    }

    fn enqueue(
        &self,
        commands: &mpsc::Sender<SessionCommand>,
        command: SessionCommand,
    ) -> Result<(), ClientError> {
        let mut stored = self.error.lock();
        if let Some(error) = stored.as_ref() {
            return Err(error.clone());
        }
        match commands.try_send(command) {
            Ok(()) => Ok(()),
            Err(error) => {
                let error = match error {
                    mpsc::error::TrySendError::Full(_) => ClientError::transport(
                        "terminal host command queue is full; attachment disconnected",
                    ),
                    mpsc::error::TrySendError::Closed(_) => ClientError::transport(
                        "terminal host command queue closed; attachment disconnected",
                    ),
                };
                *stored = Some(error.clone());
                drop(stored);
                self.failure_tx.send_replace(Some(error.clone()));
                Err(error)
            }
        }
    }
}

#[derive(Clone)]
pub struct TerminalHostClient {
    inner: Arc<ClientInner>,
}

impl fmt::Debug for TerminalHostClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TerminalHostClient")
            .field("endpoint", &self.inner.endpoint)
            .field("host_binary", &self.inner.host_binary)
            .field("auto_spawn", &self.inner.auto_spawn)
            .finish_non_exhaustive()
    }
}

impl TerminalHostClient {
    pub fn production() -> Result<Self, ClientError> {
        let endpoint = ipc::endpoint();
        let host_binary = resolve_host_binary().ok_or_else(|| {
            ClientError::transport(format!(
                "cannot find mt-terminal-host beside the application; set {HOST_BIN_ENV}"
            ))
        })?;
        Self::build(endpoint, Some(host_binary), true)
    }

    pub fn for_endpoint(endpoint: impl Into<String>) -> Result<Self, ClientError> {
        Self::build(endpoint.into(), None, false)
    }

    fn build(
        endpoint: String,
        host_binary: Option<PathBuf>,
        auto_spawn: bool,
    ) -> Result<Self, ClientError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("mini-term-host-client")
            .build()
            .map_err(|error| ClientError::transport(format!("create client runtime: {error}")))?;
        Ok(Self {
            inner: Arc::new(ClientInner {
                endpoint,
                host_binary,
                auto_spawn,
                runtime,
            }),
        })
    }

    pub fn endpoint(&self) -> &str {
        &self.inner.endpoint
    }

    pub fn create(
        &self,
        session_id: TerminalSessionId,
        worktree_id: WorktreeId,
        spawn: HostSpawnSpec,
    ) -> Result<SessionDescriptor, ClientError> {
        let request = ClientRequest::Create {
            v: PROTOCOL_VERSION,
            session_id,
            worktree_id,
            expected_absent: true,
            spawn,
        };
        match self.request(request)? {
            ServerFrame::Created { descriptor } => Ok(descriptor),
            frame => Err(unexpected("created", frame)),
        }
    }

    pub fn restore(
        &self,
        session_id: TerminalSessionId,
        worktree_id: WorktreeId,
        expected_previous_incarnation_id: TerminalIncarnationId,
        spawn: HostSpawnSpec,
    ) -> Result<(SessionDescriptor, TerminalSnapshot), ClientError> {
        let request = ClientRequest::Restore {
            v: PROTOCOL_VERSION,
            session_id,
            worktree_id,
            expected_previous_incarnation_id,
            spawn,
        };
        let (descriptor, snapshot) = match self.request(request)? {
            ServerFrame::Restored {
                descriptor,
                snapshot_b64,
            } => {
                let bytes = decode_bytes(&snapshot_b64).map_err(ClientError::transport)?;
                let snapshot = TerminalSnapshot::from_bytes(bytes).map_err(|error| {
                    ClientError::protocol(
                        ErrorCode::RecoveryUnavailable,
                        format!("invalid restored terminal snapshot: {error:#}"),
                    )
                })?;
                (descriptor, snapshot)
            }
            frame => return Err(unexpected("restored", frame)),
        };
        Ok((descriptor, snapshot))
    }

    pub fn attach<F>(
        &self,
        session_id: TerminalSessionId,
        expected_incarnation_id: TerminalIncarnationId,
        after_sequence: u64,
        on_event: F,
    ) -> Result<HostedTerminalSession, ClientError>
    where
        F: FnMut(HostedEvent) + Send + 'static,
    {
        let inner = self.inner.clone();
        let request = ClientRequest::Attach {
            v: PROTOCOL_VERSION,
            session_id,
            expected_incarnation_id,
            after_sequence,
        };
        let (descriptor, reader) = self
            .inner
            .runtime
            .block_on(async { attach_stream(inner.clone(), request).await })?;
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let (command_tx, command_rx) = mpsc::channel(COMMAND_QUEUE_CAPACITY);
        let (state, failure_rx) = AttachmentState::new();
        let attachment = HostedTerminalSession {
            client: self.clone(),
            descriptor: descriptor.clone(),
            last_size: Mutex::new((descriptor.cols, descriptor.rows)),
            cancel: Mutex::new(Some(cancel_tx)),
            commands: command_tx,
            state: state.clone(),
        };
        self.inner.runtime.spawn(run_commands(
            inner,
            descriptor.clone(),
            command_rx,
            state.clone(),
        ));
        self.inner.runtime.spawn(read_attachment(
            reader,
            after_sequence,
            cancel_rx,
            state,
            failure_rx,
            on_event,
        ));
        Ok(attachment)
    }

    pub fn create_attached<F>(
        &self,
        session_id: TerminalSessionId,
        worktree_id: WorktreeId,
        spawn: HostSpawnSpec,
        on_event: F,
    ) -> Result<HostedTerminalSession, ClientError>
    where
        F: FnMut(HostedEvent) + Send + 'static,
    {
        let descriptor = self.create(session_id.clone(), worktree_id, spawn)?;
        let incarnation_id = descriptor.incarnation_id.clone();
        match self.attach(session_id.clone(), incarnation_id.clone(), 0, on_event) {
            Ok(session) => Ok(session),
            Err(attach_error) => match self.kill(session_id, incarnation_id) {
                Ok(()) => Err(attach_error),
                Err(cleanup_error) => Err(ClientError {
                    code: attach_error.code,
                    message: format!(
                        "{}; fenced cleanup failed: {cleanup_error}",
                        attach_error.message
                    ),
                }),
            },
        }
    }

    pub fn write(
        &self,
        session_id: TerminalSessionId,
        expected_incarnation_id: TerminalIncarnationId,
        bytes: &[u8],
    ) -> Result<(), ClientError> {
        let data_b64 = encode_write_bytes(bytes)
            .map_err(|error| ClientError::protocol(ErrorCode::InvalidRequest, error))?;
        self.expect_ok(ClientRequest::Write {
            v: PROTOCOL_VERSION,
            session_id,
            expected_incarnation_id,
            data_b64,
        })
    }

    pub fn resize(
        &self,
        session_id: TerminalSessionId,
        expected_incarnation_id: TerminalIncarnationId,
        rows: u16,
        cols: u16,
    ) -> Result<(), ClientError> {
        self.expect_ok(ClientRequest::Resize {
            v: PROTOCOL_VERSION,
            session_id,
            expected_incarnation_id,
            rows,
            cols,
        })
    }

    pub fn arm_ssh_autofill(
        &self,
        session_id: TerminalSessionId,
        expected_incarnation_id: TerminalIncarnationId,
        password: String,
        disarm_on_input: bool,
    ) -> Result<(), ClientError> {
        self.expect_ok(ClientRequest::ArmAutofill {
            v: PROTOCOL_VERSION,
            session_id,
            expected_incarnation_id,
            password,
            disarm_on_input,
        })
    }

    pub fn detach(
        &self,
        session_id: TerminalSessionId,
        expected_incarnation_id: TerminalIncarnationId,
    ) -> Result<(), ClientError> {
        self.expect_ok(ClientRequest::Detach {
            v: PROTOCOL_VERSION,
            session_id,
            expected_incarnation_id,
        })
    }

    pub fn kill(
        &self,
        session_id: TerminalSessionId,
        expected_incarnation_id: TerminalIncarnationId,
    ) -> Result<(), ClientError> {
        self.expect_ok(ClientRequest::Kill {
            v: PROTOCOL_VERSION,
            session_id,
            expected_incarnation_id,
        })
    }

    pub fn list(&self) -> Result<Vec<SessionDescriptor>, ClientError> {
        match self.request(ClientRequest::List {
            v: PROTOCOL_VERSION,
        })? {
            ServerFrame::Sessions { sessions } => Ok(sessions),
            frame => Err(unexpected("sessions", frame)),
        }
    }

    pub fn status(&self) -> Result<(u32, usize), ClientError> {
        match self.request(ClientRequest::Status {
            v: PROTOCOL_VERSION,
        })? {
            ServerFrame::Status { pid, live_sessions } => Ok((pid, live_sessions)),
            frame => Err(unexpected("status", frame)),
        }
    }

    pub fn shutdown_if_idle(&self) -> Result<(), ClientError> {
        self.expect_ok(ClientRequest::ShutdownIfIdle {
            v: PROTOCOL_VERSION,
        })
    }

    fn expect_ok(&self, request: ClientRequest) -> Result<(), ClientError> {
        match self.request(request)? {
            ServerFrame::Ok => Ok(()),
            frame => Err(unexpected("ok", frame)),
        }
    }

    fn request(&self, request: ClientRequest) -> Result<ServerFrame, ClientError> {
        let inner = self.inner.clone();
        self.inner
            .runtime
            .block_on(async move { request_async(inner, request).await })
    }
}

pub struct HostedTerminalSession {
    client: TerminalHostClient,
    descriptor: SessionDescriptor,
    last_size: Mutex<(u16, u16)>,
    cancel: Mutex<Option<oneshot::Sender<()>>>,
    commands: mpsc::Sender<SessionCommand>,
    state: Arc<AttachmentState>,
}

impl fmt::Debug for HostedTerminalSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostedTerminalSession")
            .field("descriptor", &self.descriptor)
            .finish_non_exhaustive()
    }
}

impl HostedTerminalSession {
    pub fn descriptor(&self) -> &SessionDescriptor {
        &self.descriptor
    }

    pub fn write(&self, bytes: &[u8]) -> Result<(), ClientError> {
        if bytes.len() > MAX_WRITE_BYTES {
            return Err(ClientError::protocol(
                ErrorCode::InvalidRequest,
                format!("terminal write exceeds {MAX_WRITE_BYTES} byte limit"),
            ));
        }
        self.enqueue(SessionCommand::Write(bytes.to_vec()))
    }

    pub fn resize_if_changed(&self, rows: u16, cols: u16) -> Result<bool, ClientError> {
        self.check_command_error()?;
        let mut size = self.last_size.lock();
        if *size == (cols, rows) {
            return Ok(false);
        }
        self.enqueue(SessionCommand::Resize { rows, cols })?;
        *size = (cols, rows);
        Ok(true)
    }

    pub fn arm_ssh_autofill(
        &self,
        password: String,
        disarm_on_input: bool,
    ) -> Result<(), ClientError> {
        self.enqueue(SessionCommand::ArmAutofill {
            password,
            disarm_on_input,
        })
    }

    pub fn kill(&self) -> Result<(), ClientError> {
        self.client.kill(
            self.descriptor.session_id.clone(),
            self.descriptor.incarnation_id.clone(),
        )
    }

    fn check_command_error(&self) -> Result<(), ClientError> {
        self.state.error().map_or(Ok(()), Err)
    }

    fn enqueue(&self, command: SessionCommand) -> Result<(), ClientError> {
        self.state.enqueue(&self.commands, command)
    }
}

impl Drop for HostedTerminalSession {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.lock().take() {
            let _ = cancel.send(());
        }
    }
}

pub fn terminal_host_enabled() -> bool {
    std::env::var(HOST_ENABLED_ENV)
        .map(|value| {
            !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
        })
        .unwrap_or(true)
}

fn resolve_host_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(HOST_BIN_ENV).map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    let sibling = std::env::current_exe()
        .ok()?
        .parent()?
        .join(host_binary_name());
    sibling.is_file().then_some(sibling)
}

fn host_binary_name() -> &'static str {
    if cfg!(windows) {
        "mt-terminal-host.exe"
    } else {
        "mt-terminal-host"
    }
}

fn spawn_host_process(path: &Path, endpoint: &str) -> Result<ProcessChild, ClientError> {
    let mut command = Command::new(path);
    command
        .env(ipc::ENDPOINT_ENV, endpoint)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        command.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }
    command
        .spawn()
        .map_err(|error| ClientError::transport(format!("start {}: {error}", path.display())))
}

fn cleanup_spawned_child(mut child: ProcessChild) {
    let _ = child.kill();
    let _ = child.wait();
}

fn spawn_host(fence: &HostStartFence, path: &Path, endpoint: &str) -> Result<(), ClientError> {
    fence.run(
        || spawn_host_process(path, endpoint),
        spawn_child_reaper,
        cleanup_spawned_child,
    )
}

fn spawn_child_reaper(child: ProcessChild) -> Result<(), ClientError> {
    let child = Arc::new(std::sync::Mutex::new(Some(child)));
    let reaper_child = child.clone();
    let thread = std::thread::Builder::new()
        .name("mini-term-host-reaper".into())
        .spawn(move || {
            let mut guard = reaper_child
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(mut child) = guard.take() {
                drop(guard);
                let _ = child.wait();
            }
        });
    if let Err(error) = thread {
        let mut guard = child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        return Err(ClientError::transport(format!(
            "start terminal host reaper: {error}"
        )));
    }
    Ok(())
}

fn capped_deadline(deadline: tokio::time::Instant, maximum: Duration) -> tokio::time::Instant {
    deadline.min(tokio::time::Instant::now() + maximum)
}

async fn connect_ready(
    inner: Arc<ClientInner>,
    deadline: tokio::time::Instant,
) -> Result<Box<dyn ipc::IpcStream>, ClientError> {
    match connect_hello(
        &inner.endpoint,
        capped_deadline(deadline, CONNECT_ATTEMPT_TIMEOUT),
    )
    .await
    {
        Ok(stream) => return Ok(stream),
        Err(error) if !inner.auto_spawn => return Err(error),
        Err(_) => {}
    }
    let binary = inner
        .host_binary
        .as_ref()
        .ok_or_else(|| ClientError::transport("terminal host auto-start is unavailable"))?;
    let binary = binary.clone();
    let endpoint = inner.endpoint.clone();
    let start_fence = Arc::new(HostStartFence::default());
    let task_fence = start_fence.clone();
    let mut start_task =
        tokio::task::spawn_blocking(move || spawn_host(&task_fence, &binary, &endpoint));
    match tokio::time::timeout_at(deadline, &mut start_task).await {
        Ok(result) => result.map_err(|error| {
            ClientError::transport(format!("terminal host start task failed: {error}"))
        })??,
        Err(_) => {
            start_fence.cancel();
            return Err(ClientError::transport(
                "terminal host RPC timed out while starting host",
            ));
        }
    }

    let startup_deadline = capped_deadline(deadline, HOST_START_TIMEOUT);
    let mut last_error = ClientError::transport("terminal host did not start");
    while tokio::time::Instant::now() < startup_deadline {
        match connect_hello(
            &inner.endpoint,
            capped_deadline(startup_deadline, CONNECT_ATTEMPT_TIMEOUT),
        )
        .await
        {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = error,
        }
        tokio::time::sleep_until(
            startup_deadline.min(tokio::time::Instant::now() + START_RETRY_DELAY),
        )
        .await;
    }
    Err(last_error)
}

async fn connect_hello(
    endpoint: &str,
    deadline: tokio::time::Instant,
) -> Result<Box<dyn ipc::IpcStream>, ClientError> {
    let mut stream = tokio::time::timeout_at(deadline, ipc::connect(endpoint))
        .await
        .map_err(|_| ClientError::transport("terminal host connection timed out"))?
        .map_err(|error| ClientError::transport(format!("connect terminal host: {error}")))?;
    let frame = read_response_until(&mut stream, deadline).await?;
    match frame {
        ServerFrame::Hello {
            protocol_version, ..
        } if protocol_version == PROTOCOL_VERSION => Ok(stream),
        ServerFrame::Hello {
            protocol_version,
            live_sessions,
            ..
        } => Err(ClientError::protocol(
            ErrorCode::ProtocolMismatch,
            format!(
                "host protocol v{protocol_version}, client v{PROTOCOL_VERSION}, host owns {live_sessions} live sessions"
            ),
        )),
        frame => Err(unexpected("hello", frame)),
    }
}

async fn attach_stream(
    inner: Arc<ClientInner>,
    request: ClientRequest,
) -> Result<(SessionDescriptor, BufReader<Box<dyn ipc::IpcStream>>), ClientError> {
    let deadline = tokio::time::Instant::now() + RPC_TIMEOUT;
    attach_stream_until(inner, request, deadline).await
}

async fn attach_stream_until(
    inner: Arc<ClientInner>,
    request: ClientRequest,
    deadline: tokio::time::Instant,
) -> Result<(SessionDescriptor, BufReader<Box<dyn ipc::IpcStream>>), ClientError> {
    let mut stream = connect_ready(inner, deadline).await?;
    let line = encode_frame(&request).map_err(ClientError::transport)?;
    write_frame_line_until(&mut stream, &line, deadline)
        .await
        .map_err(|error| ClientError::transport(format!("write attach: {error}")))?;
    let mut reader = BufReader::new(stream);
    match read_buffered_response_until(&mut reader, deadline).await? {
        ServerFrame::Attached { descriptor } => Ok((descriptor, reader)),
        frame => Err(unexpected("attached", frame)),
    }
}

async fn request_async(
    inner: Arc<ClientInner>,
    request: ClientRequest,
) -> Result<ServerFrame, ClientError> {
    let deadline = tokio::time::Instant::now() + RPC_TIMEOUT;
    request_async_until(inner, request, deadline).await
}

async fn request_async_until(
    inner: Arc<ClientInner>,
    request: ClientRequest,
    deadline: tokio::time::Instant,
) -> Result<ServerFrame, ClientError> {
    let mut stream = connect_ready(inner, deadline).await?;
    let line = encode_frame(&request).map_err(ClientError::transport)?;
    write_frame_line_until(&mut stream, &line, deadline)
        .await
        .map_err(|error| ClientError::transport(format!("write request: {error}")))?;
    read_response_until(&mut stream, deadline).await
}

async fn run_commands(
    inner: Arc<ClientInner>,
    descriptor: SessionDescriptor,
    mut commands: mpsc::Receiver<SessionCommand>,
    state: Arc<AttachmentState>,
) {
    let mut failure_rx = state.failure_tx.subscribe();
    loop {
        if state.error().is_some() {
            return;
        }
        let command = tokio::select! {
            biased;
            _ = failure_rx.changed() => return,
            command = commands.recv() => match command {
                Some(command) => command,
                None => return,
            },
        };
        if state.error().is_some() {
            return;
        }
        let request = match command {
            SessionCommand::Write(bytes) => {
                let data_b64 = match encode_write_bytes(&bytes) {
                    Ok(data_b64) => data_b64,
                    Err(error) => {
                        state.fail(ClientError::protocol(ErrorCode::InvalidRequest, error));
                        return;
                    }
                };
                ClientRequest::Write {
                    v: PROTOCOL_VERSION,
                    session_id: descriptor.session_id.clone(),
                    expected_incarnation_id: descriptor.incarnation_id.clone(),
                    data_b64,
                }
            }
            SessionCommand::Resize { rows, cols } => ClientRequest::Resize {
                v: PROTOCOL_VERSION,
                session_id: descriptor.session_id.clone(),
                expected_incarnation_id: descriptor.incarnation_id.clone(),
                rows,
                cols,
            },
            SessionCommand::ArmAutofill {
                password,
                disarm_on_input,
            } => ClientRequest::ArmAutofill {
                v: PROTOCOL_VERSION,
                session_id: descriptor.session_id.clone(),
                expected_incarnation_id: descriptor.incarnation_id.clone(),
                password,
                disarm_on_input,
            },
        };
        let response = tokio::select! {
            biased;
            _ = failure_rx.changed() => return,
            response = request_async(inner.clone(), request) => response,
        };
        let result = match response {
            Ok(ServerFrame::Ok) => Ok(()),
            Ok(frame) => Err(unexpected("ok", frame)),
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            state.fail(error);
            return;
        }
    }
}

async fn read_attachment<F>(
    mut reader: BufReader<Box<dyn ipc::IpcStream>>,
    mut last_sequence: u64,
    mut cancel: oneshot::Receiver<()>,
    state: Arc<AttachmentState>,
    mut failure_rx: watch::Receiver<Option<ClientError>>,
    mut on_event: F,
) where
    F: FnMut(HostedEvent) + Send + 'static,
{
    loop {
        let line = tokio::select! {
            biased;
            _ = &mut cancel => return,
            changed = failure_rx.changed() => {
                if changed.is_ok() {
                    let error = failure_rx.borrow().clone().or_else(|| state.error());
                    if let Some(error) = error {
                        on_event(HostedEvent::Disconnected(error));
                    }
                }
                return;
            }
            read = read_frame_line(&mut reader) => {
                match read {
                    Ok(Some(line)) => line,
                    Ok(None) => {
                        let error = state.fail(ClientError::transport("terminal host attachment closed"));
                        on_event(HostedEvent::Disconnected(error));
                        return;
                    }
                    Err(error) => {
                        let error = state.fail(ClientError::transport(format!("read attachment: {error}")));
                        on_event(HostedEvent::Disconnected(error));
                        return;
                    }
                }
            }
        };
        if let Some(error) = state.error() {
            on_event(HostedEvent::Disconnected(error));
            return;
        }
        let frame = match decode_frame::<ServerFrame>(&line) {
            Ok(frame) => frame,
            Err(error) => {
                let error = state.fail(ClientError::transport(format!(
                    "decode attachment: {error}"
                )));
                on_event(HostedEvent::Disconnected(error));
                return;
            }
        };
        match frame {
            ServerFrame::Output { sequence, data_b64 } => {
                if sequence != last_sequence.saturating_add(1) {
                    let error = ClientError::protocol(
                        ErrorCode::ReplayGap,
                        format!(
                            "expected output sequence {}, received {sequence}",
                            last_sequence.saturating_add(1)
                        ),
                    );
                    let error = state.fail(error);
                    on_event(HostedEvent::Disconnected(error));
                    return;
                }
                let bytes = match decode_bytes(&data_b64) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        let error = state.fail(ClientError::transport(error));
                        on_event(HostedEvent::Disconnected(error));
                        return;
                    }
                };
                last_sequence = sequence;
                on_event(HostedEvent::Output { sequence, bytes });
            }
            ServerFrame::Exited { exit_code } => {
                state.fail(ClientError::protocol(
                    ErrorCode::SessionExited,
                    format!("terminal session exited with code {exit_code:?}"),
                ));
                on_event(HostedEvent::Exited { exit_code });
                return;
            }
            ServerFrame::Error { code, message } => {
                let error = state.fail(ClientError::protocol(code, message));
                on_event(HostedEvent::Disconnected(error));
                return;
            }
            frame => {
                let error = state.fail(unexpected("output or exited", frame));
                on_event(HostedEvent::Disconnected(error));
                return;
            }
        }
    }
}

async fn read_response_until(
    stream: &mut Box<dyn ipc::IpcStream>,
    deadline: tokio::time::Instant,
) -> Result<ServerFrame, ClientError> {
    let mut reader = BufReader::new(stream);
    read_buffered_response_until(&mut reader, deadline).await
}

async fn read_buffered_response_until<R>(
    reader: &mut R,
    deadline: tokio::time::Instant,
) -> Result<ServerFrame, ClientError>
where
    R: AsyncBufRead + Unpin,
{
    let line = tokio::time::timeout_at(deadline, read_frame_line(reader))
        .await
        .map_err(|_| ClientError::transport("terminal host RPC timed out waiting for response"))?
        .map_err(|error| ClientError::transport(format!("read terminal host: {error}")))?
        .ok_or_else(|| ClientError::transport("terminal host closed the connection"))?;
    match decode_frame::<ServerFrame>(&line).map_err(ClientError::transport)? {
        ServerFrame::Error { code, message } => Err(ClientError::protocol(code, message)),
        frame => Ok(frame),
    }
}

fn unexpected(expected: &str, frame: ServerFrame) -> ClientError {
    ClientError::transport(format!(
        "terminal host expected {expected} response, received {frame:?}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor() -> SessionDescriptor {
        SessionDescriptor {
            session_id: TerminalSessionId::new(),
            incarnation_id: TerminalIncarnationId::new(),
            worktree_id: format!("worktree-v1:{}", "0".repeat(64)).parse().unwrap(),
            process_id: Some(42),
            rows: 24,
            cols: 80,
            first_sequence: 1,
            latest_sequence: 0,
            wsl_override: None,
            recovery_available: true,
        }
    }

    #[test]
    fn host_toggle_defaults_on_and_accepts_common_false_values() {
        unsafe { std::env::remove_var(HOST_ENABLED_ENV) };
        assert!(terminal_host_enabled());
        unsafe { std::env::set_var(HOST_ENABLED_ENV, "off") };
        assert!(!terminal_host_enabled());
        unsafe { std::env::set_var(HOST_ENABLED_ENV, "1") };
        assert!(terminal_host_enabled());
        unsafe { std::env::remove_var(HOST_ENABLED_ENV) };
    }

    #[test]
    fn command_queue_saturation_disconnects_without_accepting_the_overflow() {
        let client = TerminalHostClient::for_endpoint("unused-test-endpoint").unwrap();
        let (commands, mut receiver) = mpsc::channel(1);
        let (state, failure_rx) = AttachmentState::new();
        let (stream, peer) = tokio::io::duplex(64);
        let reader = BufReader::new(Box::new(stream) as Box<dyn ipc::IpcStream>);
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let read_task = client.inner.runtime.spawn(read_attachment(
            reader,
            0,
            cancel_rx,
            state.clone(),
            failure_rx,
            move |event| {
                let _ = event_tx.send(event);
            },
        ));
        let session = HostedTerminalSession {
            client: client.clone(),
            descriptor: descriptor(),
            last_size: Mutex::new((80, 24)),
            cancel: Mutex::new(Some(cancel_tx)),
            commands,
            state,
        };

        session.write(b"first").unwrap();
        let full = session.write(b"second").unwrap_err();
        assert!(full.message().contains("queue is full"));
        assert!(matches!(
            event_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            HostedEvent::Disconnected(error) if error.message().contains("queue is full")
        ));
        assert!(event_rx.recv_timeout(Duration::from_millis(50)).is_err());
        match receiver.try_recv().unwrap() {
            SessionCommand::Write(bytes) => assert_eq!(bytes, b"first"),
            _ => panic!("the accepted command changed kind"),
        }
        assert!(
            receiver.try_recv().is_err(),
            "overflow command was accepted"
        );

        let oversized = vec![0; MAX_WRITE_BYTES + 1];
        let error = session.write(&oversized).unwrap_err();
        assert!(error.is_code(ErrorCode::InvalidRequest));
        drop(peer);
        client.inner.runtime.block_on(read_task).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn command_queue_failure_discards_buffered_commands_before_rpc() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "mth-client-queue-failure-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let endpoint = directory.join("host.sock").to_string_lossy().into_owned();
        let client = TerminalHostClient::for_endpoint(endpoint.clone()).unwrap();
        let listener = client
            .inner
            .runtime
            .block_on(async { tokio::net::UnixListener::bind(&endpoint).unwrap() });
        let (commands, receiver) = mpsc::channel(1);
        let (state, _) = AttachmentState::new();
        state
            .enqueue(&commands, SessionCommand::Write(b"first".to_vec()))
            .unwrap();
        let error = state
            .enqueue(&commands, SessionCommand::Write(b"second".to_vec()))
            .unwrap_err();
        assert!(error.message().contains("queue is full"));

        let worker = client.inner.runtime.spawn(run_commands(
            client.inner.clone(),
            descriptor(),
            receiver,
            state,
        ));
        client.inner.runtime.block_on(async {
            tokio::time::timeout(Duration::from_millis(250), worker)
                .await
                .expect("failed command worker attempted a buffered RPC")
                .unwrap();
            assert!(
                tokio::time::timeout(Duration::from_millis(50), listener.accept())
                    .await
                    .is_err(),
                "failed command worker connected to the terminal host"
            );
        });

        drop(listener);
        drop(client);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn host_start_cancellation_is_nonblocking_and_cleans_a_late_resource() {
        let fence = Arc::new(HostStartFence::default());
        let start_fence = fence.clone();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let committed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cleaned = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let committed_by_worker = committed.clone();
        let cleaned_by_worker = cleaned.clone();
        let start = std::thread::spawn(move || {
            start_fence.run(
                || {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(())
                },
                |_| {
                    committed_by_worker.store(true, std::sync::atomic::Ordering::Relaxed);
                    Ok(())
                },
                |_| cleaned_by_worker.store(true, std::sync::atomic::Ordering::Relaxed),
            )
        });
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        let cancelled_at = std::time::Instant::now();
        assert_eq!(fence.cancel(), HostStartState::Cancelled);
        assert!(cancelled_at.elapsed() < Duration::from_millis(50));

        release_tx.send(()).unwrap();
        let error = start.join().unwrap().unwrap_err();
        assert!(error.message().contains("cancelled"));
        assert!(!committed.load(std::sync::atomic::Ordering::Relaxed));
        assert!(cleaned.load(std::sync::atomic::Ordering::Relaxed));

        let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called_by_start = called.clone();
        let error = fence
            .run(
                || {
                    called_by_start.store(true, std::sync::atomic::Ordering::Relaxed);
                    Ok(())
                },
                |_| Ok(()),
                |_| {},
            )
            .unwrap_err();
        assert!(error.message().contains("cancelled"));
        assert!(!called.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[tokio::test]
    async fn attachment_disconnect_closes_the_command_path() {
        let (client, server) = tokio::io::duplex(64);
        drop(server);
        let reader = BufReader::new(Box::new(client) as Box<dyn ipc::IpcStream>);
        let (_cancel_tx, cancel_rx) = oneshot::channel();
        let (state, failure_rx) = AttachmentState::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let event_sink = events.clone();

        read_attachment(
            reader,
            0,
            cancel_rx,
            state.clone(),
            failure_rx,
            move |event| event_sink.lock().push(event),
        )
        .await;

        assert!(state.error().is_some());
        assert!(matches!(
            events.lock().as_slice(),
            [HostedEvent::Disconnected(_)]
        ));
    }

    #[cfg(unix)]
    #[test]
    fn one_rpc_deadline_covers_hello_write_and_first_response() {
        use std::time::Instant;

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "mth-client-deadline-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let endpoint = directory.join("host.sock").to_string_lossy().into_owned();
        let client = TerminalHostClient::for_endpoint(endpoint.clone()).unwrap();
        let listener = client
            .inner
            .runtime
            .block_on(async { tokio::net::UnixListener::bind(&endpoint).unwrap() });
        let server = client.inner.runtime.spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = tokio::io::split(stream);
            let mut reader = BufReader::new(reader);
            tokio::time::sleep(Duration::from_millis(90)).await;
            let hello = encode_frame(&ServerFrame::Hello {
                version: "test".into(),
                protocol_version: PROTOCOL_VERSION,
                pid: std::process::id(),
                live_sessions: 0,
            })
            .unwrap();
            write_frame_line_until(
                &mut writer,
                &hello,
                tokio::time::Instant::now() + Duration::from_secs(1),
            )
            .await
            .unwrap();
            let request = read_frame_line(&mut reader).await.unwrap().unwrap();
            assert!(matches!(
                decode_frame::<ClientRequest>(&request).unwrap(),
                ClientRequest::Status { .. }
            ));
            tokio::time::sleep(Duration::from_millis(90)).await;
            let response = encode_frame(&ServerFrame::Status {
                pid: std::process::id(),
                live_sessions: 0,
            })
            .unwrap();
            let _ = write_frame_line_until(
                &mut writer,
                &response,
                tokio::time::Instant::now() + Duration::from_secs(1),
            )
            .await;
        });

        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + Duration::from_millis(150);
        let error = client
            .inner
            .runtime
            .block_on(request_async_until(
                client.inner.clone(),
                ClientRequest::Status {
                    v: PROTOCOL_VERSION,
                },
                deadline,
            ))
            .unwrap_err();
        assert!(error.message().contains("RPC timed out"));
        assert!(
            started.elapsed() < Duration::from_millis(300),
            "RPC phases exceeded their shared deadline: {:?}",
            started.elapsed()
        );
        client.inner.runtime.block_on(server).unwrap();
        drop(client);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn spawned_children_are_reaped() {
        let child = Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap();
        let pid = child.id();
        spawn_child_reaper(child).unwrap();

        let proc_path = PathBuf::from(format!("/proc/{pid}"));
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while proc_path.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "child {pid} remained as an unreaped process"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
