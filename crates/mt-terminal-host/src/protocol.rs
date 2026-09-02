use base64::Engine as _;
use mt_identity::{TerminalIncarnationId, TerminalSessionId, WorktreeId};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HostSpawnSpec {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
    #[serde(default)]
    pub user_env: Vec<(String, String)>,
    pub rows: u16,
    pub cols: u16,
    pub ssh_autofill: Option<SshAutofillSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SshAutofillSpec {
    pub password: String,
    pub disarm_on_input: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WslOverrideDescriptor {
    pub distro: String,
    pub unix_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionDescriptor {
    pub session_id: TerminalSessionId,
    pub incarnation_id: TerminalIncarnationId,
    pub worktree_id: WorktreeId,
    pub process_id: Option<u32>,
    pub rows: u16,
    pub cols: u16,
    pub first_sequence: u64,
    pub latest_sequence: u64,
    pub wsl_override: Option<WslOverrideDescriptor>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    ProtocolMismatch,
    SessionExists,
    SessionCreating,
    SessionMissing,
    SessionExited,
    IncarnationMismatch,
    ReplayGap,
    HostBusy,
    SpawnFailed,
    IoFailed,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ClientRequest {
    Create {
        v: u32,
        session_id: TerminalSessionId,
        worktree_id: WorktreeId,
        expected_absent: bool,
        spawn: HostSpawnSpec,
    },
    Attach {
        v: u32,
        session_id: TerminalSessionId,
        expected_incarnation_id: TerminalIncarnationId,
        after_sequence: u64,
    },
    Write {
        v: u32,
        session_id: TerminalSessionId,
        expected_incarnation_id: TerminalIncarnationId,
        data_b64: String,
    },
    Resize {
        v: u32,
        session_id: TerminalSessionId,
        expected_incarnation_id: TerminalIncarnationId,
        rows: u16,
        cols: u16,
    },
    ArmAutofill {
        v: u32,
        session_id: TerminalSessionId,
        expected_incarnation_id: TerminalIncarnationId,
        password: String,
        disarm_on_input: bool,
    },
    Kill {
        v: u32,
        session_id: TerminalSessionId,
        expected_incarnation_id: TerminalIncarnationId,
    },
    Detach {
        v: u32,
        session_id: TerminalSessionId,
        expected_incarnation_id: TerminalIncarnationId,
    },
    List {
        v: u32,
    },
    Status {
        v: u32,
    },
    ShutdownIfIdle {
        v: u32,
    },
}

impl ClientRequest {
    pub fn protocol_version(&self) -> u32 {
        match self {
            Self::Create { v, .. }
            | Self::Attach { v, .. }
            | Self::Write { v, .. }
            | Self::Resize { v, .. }
            | Self::ArmAutofill { v, .. }
            | Self::Kill { v, .. }
            | Self::Detach { v, .. }
            | Self::List { v }
            | Self::Status { v }
            | Self::ShutdownIfIdle { v } => *v,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerFrame {
    Hello {
        version: String,
        protocol_version: u32,
        pid: u32,
        live_sessions: usize,
    },
    Created {
        descriptor: SessionDescriptor,
    },
    Attached {
        descriptor: SessionDescriptor,
    },
    Output {
        sequence: u64,
        data_b64: String,
    },
    Exited {
        exit_code: Option<u32>,
    },
    Sessions {
        sessions: Vec<SessionDescriptor>,
    },
    Status {
        pid: u32,
        live_sessions: usize,
    },
    Ok,
    Error {
        code: ErrorCode,
        message: String,
    },
}

pub fn encode_frame<T: Serialize>(frame: &T) -> Result<String, String> {
    let mut line = serde_json::to_string(frame).map_err(|error| error.to_string())?;
    line.push('\n');
    Ok(line)
}

pub fn decode_frame<'a, T: Deserialize<'a>>(line: &'a str) -> Result<T, String> {
    serde_json::from_str(line.trim_end()).map_err(|error| error.to_string())
}

pub fn encode_bytes(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn decode_bytes(encoded: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("invalid base64 payload: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip_keeps_stable_identity_fields() {
        let request = ClientRequest::Attach {
            v: PROTOCOL_VERSION,
            session_id: TerminalSessionId::new(),
            expected_incarnation_id: TerminalIncarnationId::new(),
            after_sequence: 42,
        };
        let encoded = encode_frame(&request).unwrap();
        assert_eq!(encoded.matches('\n').count(), 1);
        assert!(encoded.contains("expected_incarnation_id"));
        let decoded: ClientRequest = decode_frame(&encoded).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn binary_payload_round_trip_is_lossless() {
        let bytes = b"a\0b\xff\r\n";
        assert_eq!(decode_bytes(&encode_bytes(bytes)).unwrap(), bytes);
    }
}
