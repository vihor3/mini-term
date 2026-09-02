//! Dedicated local PTY owner used by mini-term warm reattach.

mod client;
mod history;
pub mod ipc;
pub mod protocol;
pub mod server;

pub use client::{
    ClientError, HostedEvent, HostedTerminalSession, TerminalHostClient, terminal_host_enabled,
};
pub use protocol::{
    ErrorCode, HostSpawnSpec, PROTOCOL_VERSION, SessionDescriptor, SshAutofillSpec,
    WslOverrideDescriptor,
};
pub use server::{DEFAULT_IDLE_EXIT, ServeError, ServeOutcome, serve, serve_with_history_root};
