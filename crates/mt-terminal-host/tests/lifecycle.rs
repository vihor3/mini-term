#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mt_identity::{TerminalIncarnationId, TerminalSessionId, WorktreeId};
use mt_terminal::{TermSize, TerminalEmulator};
use mt_terminal_host::protocol::{
    ClientRequest, PROTOCOL_VERSION, ServerFrame, decode_frame, encode_frame,
};
use mt_terminal_host::{
    ErrorCode, HostSpawnSpec, HostedEvent, ServeOutcome, SessionDescriptor, TerminalHostClient,
    serve_with_history_root,
};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

const WAIT: Duration = Duration::from_secs(5);

fn unique_endpoint() -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("/tmp/mth-{}-{nonce}/host.sock", std::process::id())
}

fn worktree_id() -> WorktreeId {
    format!("worktree-v1:{}", "0".repeat(64)).parse().unwrap()
}

fn spawn_spec() -> HostSpawnSpec {
    HostSpawnSpec {
        program: "/bin/sh".into(),
        args: vec![],
        cwd: Some("/tmp".into()),
        env: vec![("PS1".into(), String::new())],
        user_env: vec![],
        rows: 24,
        cols: 80,
        scrollback: 1_000,
        ssh_autofill: None,
    }
}

fn history_root(endpoint: &str) -> PathBuf {
    Path::new(endpoint).parent().unwrap().join("history")
}

fn start_server(endpoint: String) -> std::thread::JoinHandle<Result<ServeOutcome, String>> {
    std::thread::spawn(move || {
        let history_root = history_root(&endpoint);
        mt_pty::conpty::initialize_default();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        runtime
            .block_on(serve_with_history_root(
                &endpoint,
                Duration::from_secs(30),
                history_root,
            ))
            .map_err(|error| error.message().to_string())
    })
}

fn start_attach_failure_server(
    endpoint: String,
    descriptor: SessionDescriptor,
) -> std::thread::JoinHandle<()> {
    let parent = Path::new(&endpoint).parent().unwrap();
    std::fs::create_dir_all(parent).unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::UnixListener::bind(&endpoint).unwrap();
            ready_tx.send(()).unwrap();
            for step in 0..3 {
                let (stream, _) = tokio::time::timeout(WAIT, listener.accept())
                    .await
                    .expect("scripted server timed out waiting for request")
                    .unwrap();
                let (reader, mut writer) = tokio::io::split(stream);
                let hello = encode_frame(&ServerFrame::Hello {
                    version: "test".into(),
                    protocol_version: PROTOCOL_VERSION,
                    pid: std::process::id(),
                    live_sessions: usize::from(step > 0),
                })
                .unwrap();
                writer.write_all(hello.as_bytes()).await.unwrap();
                writer.flush().await.unwrap();

                let mut reader = BufReader::new(reader);
                let mut line = String::new();
                tokio::time::timeout(WAIT, reader.read_line(&mut line))
                    .await
                    .expect("scripted server timed out reading request")
                    .unwrap();
                let request = decode_frame::<ClientRequest>(&line).unwrap();
                let response = match (step, request) {
                    (
                        0,
                        ClientRequest::Create {
                            session_id,
                            worktree_id,
                            expected_absent: true,
                            ..
                        },
                    ) => {
                        assert_eq!(session_id, descriptor.session_id);
                        assert_eq!(worktree_id, descriptor.worktree_id);
                        ServerFrame::Created {
                            descriptor: descriptor.clone(),
                        }
                    }
                    (
                        1,
                        ClientRequest::Attach {
                            session_id,
                            expected_incarnation_id,
                            after_sequence: 0,
                            ..
                        },
                    ) => {
                        assert_eq!(session_id, descriptor.session_id);
                        assert_eq!(expected_incarnation_id, descriptor.incarnation_id);
                        ServerFrame::Error {
                            code: ErrorCode::SessionExited,
                            message: "scripted attach failure".into(),
                        }
                    }
                    (
                        2,
                        ClientRequest::Kill {
                            session_id,
                            expected_incarnation_id,
                            ..
                        },
                    ) => {
                        assert_eq!(session_id, descriptor.session_id);
                        assert_eq!(expected_incarnation_id, descriptor.incarnation_id);
                        ServerFrame::Ok
                    }
                    (_, request) => panic!("unexpected scripted request: {request:?}"),
                };
                let response = encode_frame(&response).unwrap();
                writer.write_all(response.as_bytes()).await.unwrap();
                writer.flush().await.unwrap();
            }
        });
    });
    ready_rx.recv_timeout(WAIT).unwrap();
    handle
}

fn wait_until_ready(client: &TerminalHostClient) {
    let deadline = Instant::now() + WAIT;
    loop {
        if client.status().is_ok() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "terminal host did not become ready"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn process_is_alive(pid: u32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    unsafe { kill(pid as i32, 0) == 0 }
}

fn wait_until_process_exits(pid: u32) {
    let deadline = Instant::now() + WAIT;
    while process_is_alive(pid) {
        assert!(
            Instant::now() < deadline,
            "terminal child {pid} stayed alive after explicit kill"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn cleanup_endpoint(endpoint: &str) {
    if let Some(parent) = std::path::Path::new(endpoint).parent() {
        let _ = std::fs::remove_dir_all(parent);
    }
}

fn wait_for_marker(receiver: &mpsc::Receiver<HostedEvent>, marker: &str) -> (String, u64) {
    let deadline = Instant::now() + WAIT;
    let mut output = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "timed out waiting for {marker:?}");
        match receiver.recv_timeout(remaining) {
            Ok(HostedEvent::Output { sequence, bytes }) => {
                output.extend_from_slice(&bytes);
                let rendered = String::from_utf8_lossy(&output);
                if rendered.contains(marker) {
                    return (rendered.into_owned(), sequence);
                }
            }
            Ok(HostedEvent::Exited { exit_code }) => {
                panic!("shell exited before marker {marker:?}: {exit_code:?}")
            }
            Ok(HostedEvent::Disconnected(error)) => {
                panic!("attachment disconnected before marker {marker:?}: {error}")
            }
            Err(error) => panic!("output channel closed before marker {marker:?}: {error}"),
        }
    }
}

fn wait_for_exit(receiver: &mpsc::Receiver<HostedEvent>) -> Option<u32> {
    let deadline = Instant::now() + WAIT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "timed out waiting for terminal exit");
        match receiver.recv_timeout(remaining) {
            Ok(HostedEvent::Output { .. }) => {}
            Ok(HostedEvent::Exited { exit_code }) => return exit_code,
            Ok(HostedEvent::Disconnected(error)) => {
                panic!("attachment disconnected before terminal exit: {error}")
            }
            Err(error) => panic!("output channel closed before terminal exit: {error}"),
        }
    }
}

fn collect_until_exit(receiver: &mpsc::Receiver<HostedEvent>) -> (Vec<u8>, Option<u32>) {
    let deadline = Instant::now() + WAIT;
    let mut output = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "timed out waiting for terminal exit");
        match receiver.recv_timeout(remaining) {
            Ok(HostedEvent::Output { bytes, .. }) => output.extend_from_slice(&bytes),
            Ok(HostedEvent::Exited { exit_code }) => return (output, exit_code),
            Ok(HostedEvent::Disconnected(error)) => {
                panic!("attachment disconnected before terminal exit: {error}")
            }
            Err(error) => panic!("output channel closed before terminal exit: {error}"),
        }
    }
}

#[test]
fn detached_session_reattaches_to_same_process_and_replays_output() {
    use std::os::unix::fs::PermissionsExt as _;

    let endpoint = unique_endpoint();
    let server = start_server(endpoint.clone());
    let client = TerminalHostClient::for_endpoint(endpoint.clone()).unwrap();
    wait_until_ready(&client);

    let parent_mode = std::fs::metadata(std::path::Path::new(&endpoint).parent().unwrap())
        .unwrap()
        .permissions();
    let socket_mode = std::fs::metadata(&endpoint).unwrap().permissions();
    assert_eq!(parent_mode.mode() & 0o777, 0o700);
    assert_eq!(socket_mode.mode() & 0o777, 0o600);

    let missing = client
        .attach(
            TerminalSessionId::new(),
            TerminalIncarnationId::new(),
            0,
            |_| {},
        )
        .unwrap_err();
    assert!(missing.is_code(ErrorCode::SessionMissing));

    let session_id = TerminalSessionId::new();
    let created = client
        .create(session_id.clone(), worktree_id(), spawn_spec())
        .unwrap();
    let original_pid = created.process_id.expect("shell pid");
    let incarnation = created.incarnation_id.clone();

    client
        .write(session_id.clone(), incarnation.clone(), b"stty -echo\r")
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    client
        .write(
            session_id.clone(),
            incarnation.clone(),
            b"printf '__WARM_ONE__\\n'\r",
        )
        .unwrap();

    let (first_tx, first_rx) = mpsc::channel();
    let first = client
        .attach(session_id.clone(), incarnation.clone(), 0, move |event| {
            let _ = first_tx.send(event);
        })
        .unwrap();
    assert_eq!(first.descriptor().process_id, Some(original_pid));
    assert_eq!(first.descriptor().incarnation_id, incarnation);
    let (_, first_sequence) = wait_for_marker(&first_rx, "__WARM_ONE__");
    drop(first);

    client
        .detach(session_id.clone(), incarnation.clone())
        .unwrap();
    assert_eq!(client.list().unwrap().len(), 1);
    client
        .write(
            session_id.clone(),
            incarnation.clone(),
            b"printf '__WARM_TWO__\\n'\r",
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));

    let (second_tx, second_rx) = mpsc::channel();
    let second = client
        .attach(
            session_id.clone(),
            incarnation.clone(),
            first_sequence,
            move |event| {
                let _ = second_tx.send(event);
            },
        )
        .unwrap();
    assert_eq!(second.descriptor().process_id, Some(original_pid));
    assert_eq!(second.descriptor().incarnation_id, incarnation);
    let (replayed, _) = wait_for_marker(&second_rx, "__WARM_TWO__");
    assert_eq!(replayed.matches("__WARM_TWO__").count(), 1);

    second.write(b"printf '__QUEUED_WRITE__\\n'\r").unwrap();
    let _ = wait_for_marker(&second_rx, "__QUEUED_WRITE__");

    let stale = TerminalIncarnationId::new();
    let write_error = client
        .write(session_id.clone(), stale.clone(), b"echo stale\r")
        .unwrap_err();
    assert!(write_error.is_code(ErrorCode::IncarnationMismatch));
    let resize_error = client
        .resize(session_id.clone(), stale.clone(), 30, 100)
        .unwrap_err();
    assert!(resize_error.is_code(ErrorCode::IncarnationMismatch));
    let autofill_error = client
        .arm_ssh_autofill(session_id.clone(), stale.clone(), "secret".into(), true)
        .unwrap_err();
    assert!(autofill_error.is_code(ErrorCode::IncarnationMismatch));
    let kill_error = client.kill(session_id.clone(), stale).unwrap_err();
    assert!(kill_error.is_code(ErrorCode::IncarnationMismatch));

    second.kill().unwrap();
    drop(second);
    wait_until_process_exits(original_pid);
    assert!(client.list().unwrap().is_empty());
    client.shutdown_if_idle().unwrap();
    assert_eq!(server.join().unwrap().unwrap(), ServeOutcome::Shutdown);
    cleanup_endpoint(&endpoint);
}

#[test]
fn natural_exit_orders_final_output_and_explicit_close_purges_history() {
    let endpoint = unique_endpoint();
    let root = history_root(&endpoint);
    let server = start_server(endpoint.clone());
    let client = TerminalHostClient::for_endpoint(endpoint.clone()).unwrap();
    wait_until_ready(&client);

    let session_id = TerminalSessionId::new();
    let worktree_id = worktree_id();
    let descriptor = client
        .create(session_id.clone(), worktree_id.clone(), spawn_spec())
        .unwrap();
    let process_id = descriptor.process_id.unwrap();
    let incarnation_id = descriptor.incarnation_id.clone();
    let (event_tx, event_rx) = mpsc::channel();
    let attachment = client
        .attach(
            session_id.clone(),
            incarnation_id.clone(),
            0,
            move |event| {
                let _ = event_tx.send(event);
            },
        )
        .unwrap();

    attachment.write(b"stty -echo\r").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    attachment
        .write(b"printf '__FINAL_OUTPUT__'; exit 7\r")
        .unwrap();
    let (output, exit_code) = collect_until_exit(&event_rx);
    assert_eq!(exit_code, Some(7));
    assert!(
        output
            .windows(b"__FINAL_OUTPUT__".len())
            .any(|window| window == b"__FINAL_OUTPUT__"),
        "Hosted Exited arrived before the final PTY output"
    );
    wait_until_process_exits(process_id);

    let attach_error = client
        .attach(session_id.clone(), incarnation_id.clone(), 0, |_| {})
        .unwrap_err();
    assert!(attach_error.is_code(ErrorCode::SessionExited));

    let stale_restore = client
        .restore(
            session_id.clone(),
            worktree_id,
            TerminalIncarnationId::new(),
            spawn_spec(),
        )
        .unwrap_err();
    assert!(stale_restore.is_code(ErrorCode::IncarnationMismatch));

    client.kill(session_id.clone(), incarnation_id).unwrap();
    drop(attachment);
    assert!(client.list().unwrap().is_empty());
    assert!(std::fs::read_dir(&root).unwrap().next().is_none());

    client.shutdown_if_idle().unwrap();
    assert_eq!(server.join().unwrap().unwrap(), ServeOutcome::Shutdown);
    cleanup_endpoint(&endpoint);
}

#[test]
fn create_attached_kills_the_created_incarnation_when_attach_fails() {
    let endpoint = unique_endpoint();
    let descriptor = SessionDescriptor {
        session_id: TerminalSessionId::new(),
        incarnation_id: TerminalIncarnationId::new(),
        worktree_id: worktree_id(),
        process_id: Some(42),
        rows: 24,
        cols: 80,
        first_sequence: 1,
        latest_sequence: 0,
        wsl_override: None,
        recovery_available: true,
    };
    let server = start_attach_failure_server(endpoint.clone(), descriptor.clone());
    let client = TerminalHostClient::for_endpoint(endpoint.clone()).unwrap();

    let error = client
        .create_attached(
            descriptor.session_id.clone(),
            descriptor.worktree_id.clone(),
            spawn_spec(),
            |_| {},
        )
        .unwrap_err();
    assert!(error.is_code(ErrorCode::SessionExited));
    server.join().unwrap();
    cleanup_endpoint(&endpoint);
}

#[test]
fn live_restore_drains_history_before_starting_the_new_incarnation() {
    let endpoint = unique_endpoint();
    let server = start_server(endpoint.clone());
    let client = TerminalHostClient::for_endpoint(endpoint.clone()).unwrap();
    wait_until_ready(&client);

    let session_id = TerminalSessionId::new();
    let worktree_id = worktree_id();
    let created = client
        .create(session_id.clone(), worktree_id.clone(), spawn_spec())
        .unwrap();
    let old_incarnation = created.incarnation_id.clone();
    let old_pid = created.process_id.unwrap();
    let (event_tx, event_rx) = mpsc::channel();
    let old_attachment = client
        .attach(
            session_id.clone(),
            old_incarnation.clone(),
            0,
            move |event| {
                let _ = event_tx.send(event);
            },
        )
        .unwrap();
    old_attachment.write(b"stty -echo\r").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    old_attachment
        .write(b"printf '__LIVE_RESTORE_HISTORY__\n'\r")
        .unwrap();
    let _ = wait_for_marker(&event_rx, "__LIVE_RESTORE_HISTORY__");

    let (restored, snapshot) = client
        .restore(
            session_id.clone(),
            worktree_id,
            old_incarnation,
            spawn_spec(),
        )
        .unwrap();
    assert_ne!(restored.process_id, Some(old_pid));
    let emulator = TerminalEmulator::new(TermSize::new(1, 1));
    emulator.restore_snapshot(&snapshot).unwrap();
    assert!(
        emulator
            .visible_lines()
            .join("\n")
            .contains("__LIVE_RESTORE_HISTORY__")
    );
    wait_until_process_exits(old_pid);

    client.kill(session_id, restored.incarnation_id).unwrap();
    drop(old_attachment);
    client.shutdown_if_idle().unwrap();
    assert_eq!(server.join().unwrap().unwrap(), ServeOutcome::Shutdown);
    cleanup_endpoint(&endpoint);
}

#[test]
fn stopped_host_restores_history_into_a_new_fenced_incarnation() {
    let endpoint = unique_endpoint();
    let root = history_root(&endpoint);
    let first_server = start_server(endpoint.clone());
    let client = TerminalHostClient::for_endpoint(endpoint.clone()).unwrap();
    wait_until_ready(&client);

    let session_id = TerminalSessionId::new();
    let worktree_id = worktree_id();
    let mut initial_spawn = spawn_spec();
    initial_spawn
        .user_env
        .push(("COLD_TEST_VALUE".into(), "never-persist-this".into()));
    let created = client
        .create(session_id.clone(), worktree_id.clone(), initial_spawn)
        .unwrap();
    assert!(created.recovery_available);
    let old_incarnation = created.incarnation_id.clone();
    let old_pid = created.process_id.unwrap();

    let (old_tx, old_rx) = mpsc::channel();
    let old_attachment = client
        .attach(
            session_id.clone(),
            old_incarnation.clone(),
            0,
            move |event| {
                let _ = old_tx.send(event);
            },
        )
        .unwrap();
    old_attachment
        .write(b"stty -echo; printf '__COLD_HISTORY__\\n'; exit\r")
        .unwrap();
    let _ = wait_for_marker(&old_rx, "__COLD_HISTORY__");
    let _ = wait_for_exit(&old_rx);
    drop(old_attachment);
    wait_until_process_exits(old_pid);

    client.shutdown_if_idle().unwrap();
    assert_eq!(
        first_server.join().unwrap().unwrap(),
        ServeOutcome::Shutdown
    );

    let second_server = start_server(endpoint.clone());
    wait_until_ready(&client);
    let (restored, snapshot) = client
        .restore(
            session_id.clone(),
            worktree_id.clone(),
            old_incarnation.clone(),
            spawn_spec(),
        )
        .unwrap();
    assert_ne!(restored.incarnation_id, old_incarnation);
    assert!(restored.recovery_available);
    let new_pid = restored.process_id.unwrap();
    let emulator = TerminalEmulator::new(TermSize::new(1, 1));
    emulator.restore_snapshot(&snapshot).unwrap();
    assert!(
        emulator
            .visible_lines()
            .join("\n")
            .contains("__COLD_HISTORY__")
    );

    let (new_tx, new_rx) = mpsc::channel();
    let new_attachment = client
        .attach(
            session_id.clone(),
            restored.incarnation_id.clone(),
            0,
            move |event| {
                let _ = new_tx.send(event);
            },
        )
        .unwrap();
    new_attachment
        .write(b"printf '__AFTER_RESTORE__\\n'\r")
        .unwrap();
    let _ = wait_for_marker(&new_rx, "__AFTER_RESTORE__");

    let stale = client
        .write(session_id.clone(), old_incarnation.clone(), b"echo stale\r")
        .unwrap_err();
    assert!(stale.is_code(ErrorCode::IncarnationMismatch));
    let stale_restore = client
        .restore(
            session_id.clone(),
            worktree_id,
            old_incarnation,
            spawn_spec(),
        )
        .unwrap_err();
    assert!(stale_restore.is_code(ErrorCode::IncarnationMismatch));

    let history_text = std::fs::read_dir(&root)
        .unwrap()
        .flat_map(|entry| std::fs::read_dir(entry.unwrap().path()).unwrap())
        .filter_map(|entry| std::fs::read(entry.ok()?.path()).ok())
        .flatten()
        .collect::<Vec<_>>();
    assert!(
        !history_text
            .windows(18)
            .any(|window| window == b"never-persist-this")
    );

    new_attachment.kill().unwrap();
    drop(new_attachment);
    wait_until_process_exits(new_pid);
    assert!(std::fs::read_dir(&root).unwrap().next().is_none());
    client.shutdown_if_idle().unwrap();
    assert_eq!(
        second_server.join().unwrap().unwrap(),
        ServeOutcome::Shutdown
    );
    cleanup_endpoint(&endpoint);
}

#[test]
fn stale_socket_is_recovered_without_displacing_a_live_owner() {
    let endpoint = unique_endpoint();
    let parent = std::path::Path::new(&endpoint).parent().unwrap();
    std::fs::create_dir_all(parent).unwrap();
    let stale = std::os::unix::net::UnixListener::bind(&endpoint).unwrap();
    drop(stale);
    assert!(std::path::Path::new(&endpoint).exists());

    let server = start_server(endpoint.clone());
    let client = TerminalHostClient::for_endpoint(endpoint.clone()).unwrap();
    wait_until_ready(&client);

    let contender = start_server(endpoint.clone());
    let contender_error = contender
        .join()
        .unwrap()
        .expect_err("second host must not replace the live endpoint owner");
    assert!(contender_error.contains("live terminal host"));
    assert_eq!(client.status().unwrap().1, 0);

    client.shutdown_if_idle().unwrap();
    assert_eq!(server.join().unwrap().unwrap(), ServeOutcome::Shutdown);
    cleanup_endpoint(&endpoint);
}

#[test]
fn idle_shutdown_refuses_to_exit_while_a_session_is_live() {
    let endpoint = unique_endpoint();
    let server = start_server(endpoint.clone());
    let client = TerminalHostClient::for_endpoint(endpoint.clone()).unwrap();
    wait_until_ready(&client);

    let session_id = TerminalSessionId::new();
    let descriptor = client
        .create(session_id.clone(), worktree_id(), spawn_spec())
        .unwrap();
    let error = client.shutdown_if_idle().unwrap_err();
    assert!(error.is_code(ErrorCode::HostBusy));

    client.kill(session_id, descriptor.incarnation_id).unwrap();
    client.shutdown_if_idle().unwrap();
    assert_eq!(server.join().unwrap().unwrap(), ServeOutcome::Shutdown);
    cleanup_endpoint(&endpoint);
}
