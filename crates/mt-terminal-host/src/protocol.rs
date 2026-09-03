use std::io::{self, Write as _};
use std::time::Duration;

use base64::Engine as _;
use mt_identity::{TerminalIncarnationId, TerminalSessionId, WorktreeId};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

pub const PROTOCOL_VERSION: u32 = 2;
pub const MAX_JSON_LINE_BYTES: usize = 48 * 1024 * 1024;
pub const MAX_WRITE_BYTES: usize = 1024 * 1024;

const MAX_WRITE_BASE64_BYTES: usize = MAX_WRITE_BYTES.div_ceil(3) * 4;

fn default_scrollback() -> usize {
    10_000
}

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
    #[serde(default = "default_scrollback")]
    pub scrollback: usize,
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
    pub recovery_available: bool,
}

impl SessionDescriptor {
    pub fn same_process_as(&self, other: &Self) -> bool {
        self.session_id == other.session_id
            && self.incarnation_id == other.incarnation_id
            && self.worktree_id == other.worktree_id
            && self.process_id == other.process_id
    }
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
    RecoveryUnavailable,
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
    Restore {
        v: u32,
        session_id: TerminalSessionId,
        worktree_id: WorktreeId,
        expected_previous_incarnation_id: TerminalIncarnationId,
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
            | Self::Restore { v, .. }
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
    Restored {
        descriptor: SessionDescriptor,
        snapshot_b64: String,
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

struct LimitedWriter {
    bytes: Vec<u8>,
    max_bytes: usize,
}

impl io::Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > self.max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("JSONL frame exceeds {} byte limit", self.max_bytes),
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn encode_frame_with_limit<T: Serialize>(frame: &T, max_bytes: usize) -> Result<String, String> {
    let mut writer = LimitedWriter {
        bytes: Vec::new(),
        max_bytes,
    };
    serde_json::to_writer(&mut writer, frame).map_err(|error| error.to_string())?;
    writer.write_all(b"\n").map_err(|error| error.to_string())?;
    String::from_utf8(writer.bytes).map_err(|error| error.to_string())
}

pub fn encode_frame<T: Serialize>(frame: &T) -> Result<String, String> {
    encode_frame_with_limit(frame, MAX_JSON_LINE_BYTES)
}

pub fn decode_frame<'a, T: Deserialize<'a>>(line: &'a str) -> Result<T, String> {
    if line.len() > MAX_JSON_LINE_BYTES {
        return Err(format!(
            "JSONL frame exceeds {MAX_JSON_LINE_BYTES} byte limit"
        ));
    }
    serde_json::from_str(line.trim_end()).map_err(|error| error.to_string())
}

pub(crate) async fn read_frame_line<R>(reader: &mut R) -> Result<Option<String>, String>
where
    R: AsyncBufRead + Unpin,
{
    read_line_with_limit(reader, MAX_JSON_LINE_BYTES).await
}

async fn read_line_with_limit<R>(reader: &mut R, max_bytes: usize) -> Result<Option<String>, String>
where
    R: AsyncBufRead + Unpin,
{
    let mut bytes = Vec::with_capacity(max_bytes.min(8 * 1024));
    loop {
        let buffer = reader
            .fill_buf()
            .await
            .map_err(|error| format!("read JSONL frame: {error}"))?;
        if buffer.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            break;
        }
        let take = buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(buffer.len(), |position| position + 1);
        if bytes.len().saturating_add(take) > max_bytes {
            return Err(format!("JSONL frame exceeds {max_bytes} byte limit"));
        }
        let complete = buffer.get(take.saturating_sub(1)) == Some(&b'\n');
        bytes.extend_from_slice(&buffer[..take]);
        reader.consume(take);
        if complete {
            break;
        }
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| format!("JSONL frame is not UTF-8: {error}"))
}

pub(crate) async fn write_frame_line<W>(
    writer: &mut W,
    line: &str,
    timeout: Duration,
) -> Result<(), String>
where
    W: AsyncWrite + Unpin,
{
    write_frame_line_until(writer, line, tokio::time::Instant::now() + timeout).await
}

pub(crate) async fn write_frame_line_until<W>(
    writer: &mut W,
    line: &str,
    deadline: tokio::time::Instant,
) -> Result<(), String>
where
    W: AsyncWrite + Unpin,
{
    if line.len() > MAX_JSON_LINE_BYTES {
        return Err(format!(
            "JSONL frame exceeds {MAX_JSON_LINE_BYTES} byte limit"
        ));
    }
    tokio::time::timeout_at(deadline, async {
        writer.write_all(line.as_bytes()).await?;
        writer.flush().await
    })
    .await
    .map_err(|_| "write JSONL frame timed out".to_string())?
    .map_err(|error| format!("write JSONL frame: {error}"))
}

pub fn encode_bytes(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn decode_bytes(encoded: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("invalid base64 payload: {error}"))
}

pub(crate) fn encode_write_bytes(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_WRITE_BYTES {
        return Err(format!(
            "terminal write exceeds {MAX_WRITE_BYTES} byte limit"
        ));
    }
    Ok(encode_bytes(bytes))
}

pub(crate) fn decode_write_bytes(encoded: &str) -> Result<Vec<u8>, String> {
    if encoded.len() > MAX_WRITE_BASE64_BYTES {
        return Err(format!(
            "terminal write exceeds {MAX_WRITE_BYTES} byte limit"
        ));
    }
    let bytes = decode_bytes(encoded)?;
    if bytes.len() > MAX_WRITE_BYTES {
        return Err(format!(
            "terminal write exceeds {MAX_WRITE_BYTES} byte limit"
        ));
    }
    Ok(bytes)
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
    fn restore_request_round_trip_keeps_previous_generation_and_spawn() {
        let request = ClientRequest::Restore {
            v: PROTOCOL_VERSION,
            session_id: TerminalSessionId::new(),
            worktree_id: format!("worktree-v1:{}", "0".repeat(64)).parse().unwrap(),
            expected_previous_incarnation_id: TerminalIncarnationId::new(),
            spawn: HostSpawnSpec {
                program: "shell".into(),
                args: vec!["--login".into()],
                cwd: Some("/repo".into()),
                env: vec![("INTERNAL".into(), "value".into())],
                user_env: vec![("USER".into(), "value".into())],
                rows: 24,
                cols: 80,
                scrollback: 50_000,
                ssh_autofill: None,
            },
        };
        let encoded = encode_frame(&request).unwrap();
        let decoded: ClientRequest = decode_frame(&encoded).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn binary_payload_round_trip_is_lossless() {
        let bytes = b"a\0b\xff\r\n";
        assert_eq!(decode_bytes(&encode_bytes(bytes)).unwrap(), bytes);
    }

    #[test]
    fn restored_descriptor_comparison_ignores_dynamic_fields() {
        let descriptor = SessionDescriptor {
            session_id: TerminalSessionId::new(),
            incarnation_id: TerminalIncarnationId::new(),
            worktree_id: format!("worktree-v1:{}", "0".repeat(64)).parse().unwrap(),
            process_id: Some(42),
            rows: 24,
            cols: 80,
            first_sequence: 1,
            latest_sequence: 2,
            wsl_override: None,
            recovery_available: true,
        };
        let mut attached = descriptor.clone();
        attached.rows = 40;
        attached.cols = 120;
        attached.first_sequence = 3;
        attached.latest_sequence = 9;
        attached.recovery_available = false;
        assert!(descriptor.same_process_as(&attached));

        attached.process_id = Some(43);
        assert!(!descriptor.same_process_as(&attached));
    }

    #[test]
    fn encoding_and_terminal_writes_reject_oversized_payloads() {
        let error = encode_frame_with_limit(&vec!["payload"; 8], 16).unwrap_err();
        assert!(error.contains("byte limit"));

        let oversized = vec![0; MAX_WRITE_BYTES + 1];
        assert!(encode_write_bytes(&oversized).is_err());
        let oversized_b64 = "A".repeat(MAX_WRITE_BASE64_BYTES + 1);
        assert!(decode_write_bytes(&oversized_b64).is_err());
    }

    #[tokio::test]
    async fn jsonl_reads_are_bounded_without_consuming_the_next_frame() {
        let mut reader = tokio::io::BufReader::new(&b"{}\n[]\n"[..]);
        assert_eq!(
            read_line_with_limit(&mut reader, 3).await.unwrap(),
            Some("{}\n".into())
        );
        assert_eq!(
            read_line_with_limit(&mut reader, 3).await.unwrap(),
            Some("[]\n".into())
        );

        let mut oversized = tokio::io::BufReader::new(&b"1234\n"[..]);
        assert!(read_line_with_limit(&mut oversized, 4).await.is_err());
    }

    #[tokio::test]
    async fn frame_write_has_a_deadline() {
        let (mut writer, _reader) = tokio::io::duplex(1);
        let error = write_frame_line(&mut writer, "blocked\n", Duration::from_millis(20))
            .await
            .unwrap_err();
        assert!(error.contains("timed out"));
    }
}
