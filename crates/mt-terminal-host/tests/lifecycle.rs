#![cfg(unix)]

use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mt_identity::{TerminalIncarnationId, TerminalSessionId, WorktreeId};
use mt_terminal_host::{
    ErrorCode, HostSpawnSpec, HostedEvent, ServeOutcome, TerminalHostClient, serve,
};

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
        ssh_autofill: None,
    }
}

fn start_server(endpoint: String) -> std::thread::JoinHandle<Result<ServeOutcome, String>> {
    std::thread::spawn(move || {
        mt_pty::conpty::initialize_default();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        runtime
            .block_on(serve(&endpoint, Duration::from_secs(30)))
            .map_err(|error| error.message().to_string())
    })
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
