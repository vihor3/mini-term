use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

use mt_identity::{TerminalIncarnationId, TerminalSessionId, WorktreeId};
use mt_terminal::TerminalSnapshot;

use crate::ipc;
use crate::protocol::{
    ClientRequest, ErrorCode, HostSpawnSpec, PROTOCOL_VERSION, ServerFrame, SessionDescriptor,
    decode_bytes, decode_frame, encode_bytes, encode_frame,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const START_RETRY_DELAY: Duration = Duration::from_millis(35);
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
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let command_error = Arc::new(Mutex::new(None));
        let attachment = HostedTerminalSession {
            client: self.clone(),
            descriptor: descriptor.clone(),
            last_size: Mutex::new((descriptor.cols, descriptor.rows)),
            cancel: Mutex::new(Some(cancel_tx)),
            commands: command_tx,
            command_error: command_error.clone(),
        };
        self.inner.runtime.spawn(run_commands(
            inner,
            descriptor.clone(),
            command_rx,
            command_error,
        ));
        self.inner
            .runtime
            .spawn(read_attachment(reader, after_sequence, cancel_rx, on_event));
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
        self.attach(session_id, descriptor.incarnation_id, 0, on_event)
    }

    pub fn write(
        &self,
        session_id: TerminalSessionId,
        expected_incarnation_id: TerminalIncarnationId,
        bytes: &[u8],
    ) -> Result<(), ClientError> {
        self.expect_ok(ClientRequest::Write {
            v: PROTOCOL_VERSION,
            session_id,
            expected_incarnation_id,
            data_b64: encode_bytes(bytes),
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
    commands: mpsc::UnboundedSender<SessionCommand>,
    command_error: Arc<Mutex<Option<ClientError>>>,
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
        self.check_command_error()?;
        self.commands
            .send(SessionCommand::Write(bytes.to_vec()))
            .map_err(|_| ClientError::transport("terminal host command queue closed"))
    }

    pub fn resize_if_changed(&self, rows: u16, cols: u16) -> Result<bool, ClientError> {
        self.check_command_error()?;
        let mut size = self.last_size.lock();
        if *size == (cols, rows) {
            return Ok(false);
        }
        self.commands
            .send(SessionCommand::Resize { rows, cols })
            .map_err(|_| ClientError::transport("terminal host command queue closed"))?;
        *size = (cols, rows);
        Ok(true)
    }

    pub fn arm_ssh_autofill(
        &self,
        password: String,
        disarm_on_input: bool,
    ) -> Result<(), ClientError> {
        self.check_command_error()?;
        self.commands
            .send(SessionCommand::ArmAutofill {
                password,
                disarm_on_input,
            })
            .map_err(|_| ClientError::transport("terminal host command queue closed"))
    }

    pub fn kill(&self) -> Result<(), ClientError> {
        self.client.kill(
            self.descriptor.session_id.clone(),
            self.descriptor.incarnation_id.clone(),
        )
    }

    fn check_command_error(&self) -> Result<(), ClientError> {
        self.command_error.lock().clone().map_or(Ok(()), Err)
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

fn spawn_host(path: &Path, endpoint: &str) -> Result<(), ClientError> {
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
        .map(|_| ())
        .map_err(|error| ClientError::transport(format!("start {}: {error}", path.display())))
}

async fn connect_ready(inner: Arc<ClientInner>) -> Result<Box<dyn ipc::IpcStream>, ClientError> {
    match connect_hello(&inner.endpoint).await {
        Ok(stream) => return Ok(stream),
        Err(error) if !inner.auto_spawn => return Err(error),
        Err(_) => {}
    }
    let binary = inner
        .host_binary
        .as_ref()
        .ok_or_else(|| ClientError::transport("terminal host auto-start is unavailable"))?;
    spawn_host(binary, &inner.endpoint)?;
    let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
    let mut last_error = ClientError::transport("terminal host did not start");
    while tokio::time::Instant::now() < deadline {
        match connect_hello(&inner.endpoint).await {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = error,
        }
        tokio::time::sleep(START_RETRY_DELAY).await;
    }
    Err(last_error)
}

async fn connect_hello(endpoint: &str) -> Result<Box<dyn ipc::IpcStream>, ClientError> {
    let mut stream = tokio::time::timeout(CONNECT_TIMEOUT, ipc::connect(endpoint))
        .await
        .map_err(|_| ClientError::transport("terminal host connection timed out"))?
        .map_err(|error| ClientError::transport(format!("connect terminal host: {error}")))?;
    let frame = read_response(&mut stream).await?;
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
    let mut stream = connect_ready(inner).await?;
    let line = encode_frame(&request).map_err(ClientError::transport)?;
    stream
        .write_all(line.as_bytes())
        .await
        .map_err(|error| ClientError::transport(format!("write attach: {error}")))?;
    stream
        .flush()
        .await
        .map_err(|error| ClientError::transport(format!("flush attach: {error}")))?;
    let mut reader = BufReader::new(stream);
    match read_buffered_response(&mut reader).await? {
        ServerFrame::Attached { descriptor } => Ok((descriptor, reader)),
        frame => Err(unexpected("attached", frame)),
    }
}

async fn request_async(
    inner: Arc<ClientInner>,
    request: ClientRequest,
) -> Result<ServerFrame, ClientError> {
    let mut stream = connect_ready(inner).await?;
    let line = encode_frame(&request).map_err(ClientError::transport)?;
    stream
        .write_all(line.as_bytes())
        .await
        .map_err(|error| ClientError::transport(format!("write request: {error}")))?;
    stream
        .flush()
        .await
        .map_err(|error| ClientError::transport(format!("flush request: {error}")))?;
    read_response(&mut stream).await
}

async fn run_commands(
    inner: Arc<ClientInner>,
    descriptor: SessionDescriptor,
    mut commands: mpsc::UnboundedReceiver<SessionCommand>,
    command_error: Arc<Mutex<Option<ClientError>>>,
) {
    while let Some(command) = commands.recv().await {
        let request = match command {
            SessionCommand::Write(bytes) => ClientRequest::Write {
                v: PROTOCOL_VERSION,
                session_id: descriptor.session_id.clone(),
                expected_incarnation_id: descriptor.incarnation_id.clone(),
                data_b64: encode_bytes(&bytes),
            },
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
        let result = match request_async(inner.clone(), request).await {
            Ok(ServerFrame::Ok) => Ok(()),
            Ok(frame) => Err(unexpected("ok", frame)),
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            *command_error.lock() = Some(error);
            return;
        }
    }
}

async fn read_attachment<F>(
    mut reader: BufReader<Box<dyn ipc::IpcStream>>,
    mut last_sequence: u64,
    mut cancel: oneshot::Receiver<()>,
    mut on_event: F,
) where
    F: FnMut(HostedEvent) + Send + 'static,
{
    loop {
        let mut line = String::new();
        tokio::select! {
            _ = &mut cancel => return,
            read = reader.read_line(&mut line) => {
                let count = match read {
                    Ok(count) => count,
                    Err(error) => {
                        on_event(HostedEvent::Disconnected(ClientError::transport(format!("read attachment: {error}"))));
                        return;
                    }
                };
                if count == 0 {
                    on_event(HostedEvent::Disconnected(ClientError::transport("terminal host attachment closed")));
                    return;
                }
            }
        }
        let frame = match decode_frame::<ServerFrame>(&line) {
            Ok(frame) => frame,
            Err(error) => {
                on_event(HostedEvent::Disconnected(ClientError::transport(format!(
                    "decode attachment: {error}"
                ))));
                return;
            }
        };
        match frame {
            ServerFrame::Output { sequence, data_b64 } => {
                if sequence != last_sequence.saturating_add(1) {
                    on_event(HostedEvent::Disconnected(ClientError::protocol(
                        ErrorCode::ReplayGap,
                        format!(
                            "expected output sequence {}, received {sequence}",
                            last_sequence.saturating_add(1)
                        ),
                    )));
                    return;
                }
                let bytes = match decode_bytes(&data_b64) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        on_event(HostedEvent::Disconnected(ClientError::transport(error)));
                        return;
                    }
                };
                last_sequence = sequence;
                on_event(HostedEvent::Output { sequence, bytes });
            }
            ServerFrame::Exited { exit_code } => {
                on_event(HostedEvent::Exited { exit_code });
                return;
            }
            ServerFrame::Error { code, message } => {
                on_event(HostedEvent::Disconnected(ClientError::protocol(
                    code, message,
                )));
                return;
            }
            frame => {
                on_event(HostedEvent::Disconnected(unexpected(
                    "output or exited",
                    frame,
                )));
                return;
            }
        }
    }
}

async fn read_response(stream: &mut Box<dyn ipc::IpcStream>) -> Result<ServerFrame, ClientError> {
    let mut reader = BufReader::new(stream);
    read_buffered_response(&mut reader).await
}

async fn read_buffered_response<R>(reader: &mut R) -> Result<ServerFrame, ClientError>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    let mut line = String::new();
    let count = tokio::time::timeout(REQUEST_TIMEOUT, reader.read_line(&mut line))
        .await
        .map_err(|_| ClientError::transport("terminal host response timed out"))?
        .map_err(|error| ClientError::transport(format!("read terminal host: {error}")))?;
    if count == 0 {
        return Err(ClientError::transport(
            "terminal host closed the connection",
        ));
    }
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
}
