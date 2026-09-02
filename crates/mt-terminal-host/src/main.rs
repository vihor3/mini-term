use std::time::Duration;

fn main() {
    mt_pty::conpty::initialize_default();

    let endpoint = mt_terminal_host::ipc::endpoint();
    let idle = std::env::var("MINITERM_TERMINAL_HOST_IDLE_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(mt_terminal_host::DEFAULT_IDLE_EXIT);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("terminal host runtime");
    let result = runtime.block_on(mt_terminal_host::serve(&endpoint, idle));
    match result {
        Ok(outcome) => eprintln!("[mt-terminal-host] exiting: {outcome:?}"),
        Err(mt_terminal_host::ServeError::AlreadyRunning(message)) => {
            eprintln!("[mt-terminal-host] already running: {message}");
        }
        Err(error) => {
            eprintln!("[mt-terminal-host] failed: {}", error.message());
            std::process::exit(2);
        }
    }
}
