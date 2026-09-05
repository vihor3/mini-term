use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::str::FromStr as _;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context as _, anyhow, bail};
use base64::Engine as _;
use mt_identity::{TerminalIncarnationId, TerminalSessionId, WorktreeId};
use mt_terminal::{SNAPSHOT_MAX_COMPRESSED_BYTES, TermSize, TerminalEmulator, TerminalSnapshot};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

pub const HISTORY_DIR_ENV: &str = "MINITERM_TERMINAL_HISTORY_DIR";

const META_MAGIC: &str = "mini-term-terminal-history";
const CHECKPOINT_MAGIC: &str = "mini-term-terminal-checkpoint";
const HISTORY_VERSION: u32 = 1;
const LOG_MAGIC: &[u8; 8] = b"MTHLOG01";
const LOG_VERSION: u16 = 1;
const LOG_BYTES_LIMIT: usize = 8 * 1024 * 1024;
const FRAME_PAYLOAD_LIMIT: usize = 1024 * 1024;
const GENERATION_BYTES_LIMIT: usize = 128;
const JSON_BYTES_LIMIT: usize = SNAPSHOT_MAX_COMPRESSED_BYTES * 2;
const INVALIDATED_MARKER: &[u8] = b"terminal recovery invalidated\n";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryMeta {
    magic: String,
    version: u32,
    session_id: TerminalSessionId,
    worktree_id: WorktreeId,
    incarnation_id: TerminalIncarnationId,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryCheckpoint {
    magic: String,
    version: u32,
    session_id: TerminalSessionId,
    worktree_id: WorktreeId,
    generation: TerminalIncarnationId,
    through_sequence: u64,
    snapshot_b64: String,
    snapshot_crc32: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum FrameKind {
    Output = 1,
    Resize = 2,
    Clear = 3,
}

impl TryFrom<u8> for FrameKind {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Output),
            2 => Ok(Self::Resize),
            3 => Ok(Self::Clear),
            _ => bail!("unknown terminal history frame kind {value}"),
        }
    }
}

#[derive(Debug)]
struct HistoryFrame {
    kind: FrameKind,
    generation: TerminalIncarnationId,
    sequence: u64,
    payload: Vec<u8>,
}

#[derive(Debug)]
struct ParsedFrames {
    frames: Vec<HistoryFrame>,
    valid_len: usize,
    torn_tail: bool,
}

#[derive(Debug, Clone)]
struct HistoryPaths {
    directory: PathBuf,
    meta: PathBuf,
    checkpoint: PathBuf,
    log: PathBuf,
    invalidated: PathBuf,
}

impl HistoryPaths {
    fn new(root: &Path, session_id: &TerminalSessionId) -> anyhow::Result<Self> {
        let key = session_id
            .as_str()
            .strip_prefix(TerminalSessionId::PREFIX)
            .ok_or_else(|| anyhow!("invalid terminal session prefix"))?;
        if !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            bail!("terminal session cannot be used as a history key");
        }
        let directory = root.join(key);
        Ok(Self {
            meta: directory.join("meta.json"),
            checkpoint: directory.join("checkpoint.json"),
            log: directory.join("output.log"),
            invalidated: directory.join("invalidated"),
            directory,
        })
    }
}

pub(crate) struct RecoveredHistory {
    pub snapshot: TerminalSnapshot,
}

pub(crate) struct SessionHistory {
    paths: HistoryPaths,
    invalidated: AtomicBool,
    available: AtomicBool,
    state: Mutex<HistoryState>,
}

pub(crate) struct HistorySeed<'a> {
    pub root: &'a Path,
    pub session_id: TerminalSessionId,
    pub worktree_id: WorktreeId,
    pub generation: TerminalIncarnationId,
    pub rows: u16,
    pub cols: u16,
    pub scrollback: usize,
    pub initial_snapshot: Option<&'a TerminalSnapshot>,
}

struct HistoryState {
    paths: HistoryPaths,
    session_id: TerminalSessionId,
    worktree_id: WorktreeId,
    generation: TerminalIncarnationId,
    emulator: TerminalEmulator,
    next_sequence: u64,
    log: Option<File>,
    log_bytes: usize,
    active: bool,
    failure: Option<String>,
}

impl SessionHistory {
    pub(crate) fn pending(seed: HistorySeed<'_>) -> anyhow::Result<Self> {
        let paths = HistoryPaths::new(seed.root, &seed.session_id)?;
        let emulator = TerminalEmulator::with_scrollback(
            TermSize::new(seed.cols.max(1) as usize, seed.rows.max(1) as usize),
            seed.scrollback,
        );
        if let Some(snapshot) = seed.initial_snapshot {
            emulator.restore_snapshot(snapshot)?;
            emulator.reset_parser_state();
        }
        Ok(Self {
            paths: paths.clone(),
            invalidated: AtomicBool::new(false),
            available: AtomicBool::new(false),
            state: Mutex::new(HistoryState {
                paths,
                session_id: seed.session_id,
                worktree_id: seed.worktree_id,
                generation: seed.generation,
                emulator,
                next_sequence: 1,
                log: None,
                log_bytes: 0,
                active: false,
                failure: None,
            }),
        })
    }

    pub(crate) fn activate(&self) -> bool {
        if self.invalidated.load(Ordering::Acquire) {
            return false;
        }
        let mut state = self.state.lock();
        if self.invalidated.load(Ordering::Acquire) {
            return false;
        }
        if let Err(error) = state.activate() {
            self.fail_locked(&mut state, error);
            return false;
        }
        if self.invalidated.load(Ordering::Acquire) {
            state.deactivate();
            drop(state);
            if let Err(error) = write_invalidation_marker(&self.paths) {
                eprintln!(
                    "terminal history invalidation raced activation for {}: {error:#}",
                    self.paths.directory.display()
                );
            }
            return false;
        }
        self.available.store(true, Ordering::Release);
        true
    }

    pub(crate) fn record_output(&self, bytes: &[u8]) {
        if self.invalidated.load(Ordering::Acquire) {
            return;
        }
        let mut state = self.state.lock();
        if self.invalidated.load(Ordering::Acquire) || state.failure.is_some() {
            return;
        }
        if let Err(error) = state.record_output(bytes) {
            self.fail_locked(&mut state, error);
        }
    }

    pub(crate) fn record_resize(&self, rows: u16, cols: u16) {
        if self.invalidated.load(Ordering::Acquire) {
            return;
        }
        let mut state = self.state.lock();
        if self.invalidated.load(Ordering::Acquire) || state.failure.is_some() {
            return;
        }
        if let Err(error) = state.record_resize(rows, cols) {
            self.fail_locked(&mut state, error);
        }
    }

    pub(crate) fn flush_checkpoint(&self) {
        if self.invalidated.load(Ordering::Acquire) {
            return;
        }
        let mut state = self.state.lock();
        if self.invalidated.load(Ordering::Acquire) || state.failure.is_some() || !state.active {
            return;
        }
        if let Err(error) = state.checkpoint() {
            self.fail_locked(&mut state, error);
        }
    }

    pub(crate) fn seal(&self) -> bool {
        if self.invalidated.load(Ordering::Acquire) {
            return false;
        }
        let mut state = self.state.lock();
        if self.invalidated.load(Ordering::Acquire) || state.failure.is_some() || !state.active {
            return false;
        }
        if let Err(error) = state.checkpoint() {
            self.fail_locked(&mut state, error);
            return false;
        }
        if self.invalidated.load(Ordering::Acquire) {
            state.deactivate();
            return false;
        }
        state.log = None;
        state.active = false;
        self.available.store(false, Ordering::Release);
        true
    }

    pub(crate) fn invalidate(&self) -> anyhow::Result<()> {
        self.invalidated.store(true, Ordering::Release);
        self.available.store(false, Ordering::Release);
        if let Some(mut state) = self.state.try_lock() {
            state.deactivate();
        }
        write_invalidation_marker(&self.paths)
    }

    pub(crate) fn invalidate_and_wait(&self) -> anyhow::Result<()> {
        self.invalidated.store(true, Ordering::Release);
        self.available.store(false, Ordering::Release);
        let mut state = self.state.lock();
        state.deactivate();
        write_invalidation_marker(&self.paths)
    }

    pub(crate) fn is_available(&self) -> bool {
        self.available.load(Ordering::Acquire) && !self.invalidated.load(Ordering::Acquire)
    }

    fn fail_locked(&self, state: &mut HistoryState, error: anyhow::Error) {
        self.invalidated.store(true, Ordering::Release);
        self.available.store(false, Ordering::Release);
        state.fail(error);
    }
}

impl HistoryState {
    fn activate(&mut self) -> anyhow::Result<()> {
        prepare_directory(&self.paths.directory)?;
        let through_sequence = self.next_sequence.saturating_sub(1);
        write_checkpoint(
            &self.paths,
            &self.session_id,
            &self.worktree_id,
            &self.generation,
            through_sequence,
            &self.emulator.snapshot()?,
        )?;
        let log = open_log(&self.paths.log, true)?;
        write_meta(
            &self.paths,
            &self.session_id,
            &self.worktree_id,
            &self.generation,
        )?;
        match fs::remove_file(&self.paths.invalidated) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("clear terminal history invalidation marker"),
        }
        self.log = Some(log);
        self.log_bytes = 0;
        self.active = true;
        Ok(())
    }

    fn record_output(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        for chunk in bytes.chunks(FRAME_PAYLOAD_LIMIT) {
            self.emulator.advance(chunk);
            let sequence = self.allocate_sequence();
            self.append_frame(FrameKind::Output, sequence, chunk)?;
        }
        Ok(())
    }

    fn record_resize(&mut self, rows: u16, cols: u16) -> anyhow::Result<()> {
        self.emulator
            .resize(TermSize::new(cols.max(1) as usize, rows.max(1) as usize));
        let sequence = self.allocate_sequence();
        let mut payload = Vec::with_capacity(4);
        payload.extend_from_slice(&rows.to_le_bytes());
        payload.extend_from_slice(&cols.to_le_bytes());
        self.append_frame(FrameKind::Resize, sequence, &payload)
    }

    fn allocate_sequence(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        sequence
    }

    fn append_frame(
        &mut self,
        kind: FrameKind,
        sequence: u64,
        payload: &[u8],
    ) -> anyhow::Result<()> {
        if !self.active {
            return Ok(());
        }
        let encoded = encode_frame(kind, &self.generation, sequence, payload)?;
        if self.log_bytes.saturating_add(encoded.len()) > LOG_BYTES_LIMIT {
            self.checkpoint()?;
        }
        let log = self
            .log
            .as_mut()
            .ok_or_else(|| anyhow!("terminal history log is not open"))?;
        log.write_all(&encoded)
            .context("append terminal history frame")?;
        self.log_bytes = self.log_bytes.saturating_add(encoded.len());
        Ok(())
    }

    fn checkpoint(&mut self) -> anyhow::Result<()> {
        if let Some(log) = self.log.as_mut() {
            log.flush().context("flush terminal history log")?;
            let _ = log.sync_data();
        }
        let through_sequence = self.next_sequence.saturating_sub(1);
        write_checkpoint(
            &self.paths,
            &self.session_id,
            &self.worktree_id,
            &self.generation,
            through_sequence,
            &self.emulator.snapshot()?,
        )?;
        if let Some(log) = self.log.as_mut() {
            log.set_len(0).context("truncate terminal history log")?;
            log.seek(SeekFrom::Start(0))
                .context("rewind terminal history log")?;
        }
        self.log_bytes = 0;
        Ok(())
    }

    fn deactivate(&mut self) {
        self.log = None;
        self.active = false;
    }

    fn fail(&mut self, error: anyhow::Error) {
        self.deactivate();
        let mut message = format!("{error:#}");
        if let Err(invalidation_error) = write_invalidation_marker(&self.paths) {
            message.push_str(&format!(
                "; could not durably invalidate recovery history: {invalidation_error:#}"
            ));
        }
        eprintln!(
            "terminal history disabled for {}: {message}",
            self.session_id
        );
        self.failure = Some(message);
    }
}

pub(crate) fn default_root() -> PathBuf {
    if let Some(path) = std::env::var_os(HISTORY_DIR_ENV).map(PathBuf::from) {
        return path;
    }
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("com.mini-term.app")
        .join("terminal-history")
}

pub(crate) fn recover(
    root: &Path,
    session_id: &TerminalSessionId,
    worktree_id: &WorktreeId,
    expected_previous_incarnation: &TerminalIncarnationId,
) -> anyhow::Result<RecoveredHistory> {
    let paths = HistoryPaths::new(root, session_id)?;
    if paths
        .invalidated
        .try_exists()
        .context("check terminal history invalidation marker")?
    {
        bail!("terminal history was invalidated after an incomplete output drain");
    }
    let meta: HistoryMeta = read_json(&paths.meta, 64 * 1024)?;
    validate_meta(
        &meta,
        session_id,
        worktree_id,
        expected_previous_incarnation,
    )?;
    let checkpoint: HistoryCheckpoint = read_json(&paths.checkpoint, JSON_BYTES_LIMIT)?;
    validate_checkpoint(
        &checkpoint,
        session_id,
        worktree_id,
        expected_previous_incarnation,
    )?;
    let snapshot_bytes = base64::engine::general_purpose::STANDARD
        .decode(&checkpoint.snapshot_b64)
        .context("decode terminal checkpoint snapshot")?;
    if crc32fast::hash(&snapshot_bytes) != checkpoint.snapshot_crc32 {
        bail!("terminal checkpoint snapshot checksum mismatch");
    }
    let snapshot = TerminalSnapshot::from_bytes(snapshot_bytes)?;
    let emulator = TerminalEmulator::new(TermSize::new(1, 1));
    emulator.restore_snapshot(&snapshot)?;

    let log_bytes = match read_limited(&paths.log, LOG_BYTES_LIMIT + FRAME_PAYLOAD_LIMIT + 4096) {
        Ok(bytes) => bytes,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
        {
            Vec::new()
        }
        Err(error) => return Err(error),
    };
    let parsed = parse_frames(&log_bytes)?;
    if parsed.torn_tail {
        truncate_log(&paths.log, parsed.valid_len)?;
    }

    let mut previous_log_sequence = None;
    let mut applied_sequence = checkpoint.through_sequence;
    for frame in parsed.frames {
        if frame.generation != checkpoint.generation {
            bail!("terminal history generation mismatch");
        }
        if let Some(previous) = previous_log_sequence
            && frame.sequence != previous + 1
        {
            bail!(
                "terminal history sequence gap: expected {}, received {}",
                previous + 1,
                frame.sequence
            );
        }
        previous_log_sequence = Some(frame.sequence);
        if frame.sequence <= checkpoint.through_sequence {
            continue;
        }
        if frame.sequence != applied_sequence.saturating_add(1) {
            bail!(
                "terminal history sequence gap after checkpoint: expected {}, received {}",
                applied_sequence.saturating_add(1),
                frame.sequence
            );
        }
        apply_frame(&emulator, &frame)?;
        applied_sequence = frame.sequence;
    }
    emulator.reset_parser_state();
    Ok(RecoveredHistory {
        snapshot: emulator.snapshot()?,
    })
}

pub(crate) fn invalidate(root: &Path, session_id: &TerminalSessionId) -> anyhow::Result<()> {
    write_invalidation_marker(&HistoryPaths::new(root, session_id)?)
}

/// The caller must exclude create/restore until its subsequent purge finishes.
/// A missing directory is absence; missing or unreadable metadata is not.
pub(crate) fn stored_incarnation(
    root: &Path,
    session_id: &TerminalSessionId,
) -> anyhow::Result<Option<TerminalIncarnationId>> {
    let paths = HistoryPaths::new(root, session_id)?;
    let directory = match fs::symlink_metadata(&paths.directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("inspect terminal history directory"),
    };
    if !directory.is_dir() || directory.file_type().is_symlink() {
        bail!("terminal history is not a regular directory");
    }
    let metadata =
        fs::symlink_metadata(&paths.meta).context("inspect terminal history metadata")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        bail!("terminal history metadata is not a regular file");
    }
    let meta: HistoryMeta = read_json(&paths.meta, 64 * 1024)?;
    validate_meta(&meta, session_id, &meta.worktree_id, &meta.incarnation_id)?;
    Ok(Some(meta.incarnation_id))
}

pub(crate) fn purge(root: &Path, session_id: &TerminalSessionId) -> anyhow::Result<()> {
    let paths = HistoryPaths::new(root, session_id)?;
    match fs::remove_dir_all(&paths.directory) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("remove terminal history directory"),
    }
}

fn validate_meta(
    meta: &HistoryMeta,
    session_id: &TerminalSessionId,
    worktree_id: &WorktreeId,
    incarnation_id: &TerminalIncarnationId,
) -> anyhow::Result<()> {
    if meta.magic != META_MAGIC || meta.version != HISTORY_VERSION {
        bail!("unsupported terminal history metadata");
    }
    if &meta.session_id != session_id
        || &meta.worktree_id != worktree_id
        || &meta.incarnation_id != incarnation_id
    {
        bail!("terminal history identity mismatch");
    }
    Ok(())
}

fn validate_checkpoint(
    checkpoint: &HistoryCheckpoint,
    session_id: &TerminalSessionId,
    worktree_id: &WorktreeId,
    incarnation_id: &TerminalIncarnationId,
) -> anyhow::Result<()> {
    if checkpoint.magic != CHECKPOINT_MAGIC || checkpoint.version != HISTORY_VERSION {
        bail!("unsupported terminal history checkpoint");
    }
    if &checkpoint.session_id != session_id
        || &checkpoint.worktree_id != worktree_id
        || &checkpoint.generation != incarnation_id
    {
        bail!("terminal checkpoint identity mismatch");
    }
    Ok(())
}

fn apply_frame(emulator: &TerminalEmulator, frame: &HistoryFrame) -> anyhow::Result<()> {
    match frame.kind {
        FrameKind::Output => emulator.advance(&frame.payload),
        FrameKind::Resize => {
            if frame.payload.len() != 4 {
                bail!("terminal resize frame has invalid payload length");
            }
            let rows = u16::from_le_bytes([frame.payload[0], frame.payload[1]]);
            let cols = u16::from_le_bytes([frame.payload[2], frame.payload[3]]);
            if rows == 0 || cols == 0 {
                bail!("terminal resize frame has zero dimensions");
            }
            emulator.resize(TermSize::new(cols as usize, rows as usize));
        }
        FrameKind::Clear => emulator.advance(b"\x1bc"),
    }
    Ok(())
}

fn encode_frame(
    kind: FrameKind,
    generation: &TerminalIncarnationId,
    sequence: u64,
    payload: &[u8],
) -> anyhow::Result<Vec<u8>> {
    if payload.len() > FRAME_PAYLOAD_LIMIT {
        bail!("terminal history frame payload exceeds byte limit");
    }
    let generation = generation.as_str().as_bytes();
    if generation.len() > GENERATION_BYTES_LIMIT || generation.len() > u16::MAX as usize {
        bail!("terminal history generation exceeds byte limit");
    }
    let mut body = Vec::with_capacity(17 + generation.len() + payload.len());
    body.extend_from_slice(&LOG_VERSION.to_le_bytes());
    body.push(kind as u8);
    body.extend_from_slice(&(generation.len() as u16).to_le_bytes());
    body.extend_from_slice(generation);
    body.extend_from_slice(&sequence.to_le_bytes());
    body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    body.extend_from_slice(payload);
    let checksum = crc32fast::hash(&body);
    let mut encoded = Vec::with_capacity(LOG_MAGIC.len() + body.len() + 4);
    encoded.extend_from_slice(LOG_MAGIC);
    encoded.extend_from_slice(&body);
    encoded.extend_from_slice(&checksum.to_le_bytes());
    Ok(encoded)
}

fn torn_frames(frames: Vec<HistoryFrame>, valid_len: usize) -> anyhow::Result<ParsedFrames> {
    Ok(ParsedFrames {
        frames,
        valid_len,
        torn_tail: true,
    })
}

fn valid_generation_prefix(bytes: &[u8], total_len: usize) -> bool {
    let identity_prefix = TerminalIncarnationId::PREFIX.as_bytes();
    const UUID_LEN: usize = 36;
    if total_len != identity_prefix.len() + UUID_LEN || bytes.len() > total_len {
        return false;
    }
    bytes.iter().copied().enumerate().all(|(index, byte)| {
        if index < identity_prefix.len() {
            return byte == identity_prefix[index];
        }
        match index - identity_prefix.len() {
            8 | 13 | 18 | 23 => byte == b'-',
            14 => byte == b'4',
            19 => matches!(byte, b'8' | b'9' | b'a' | b'b'),
            _ => byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase(),
        }
    })
}

fn parse_frames(bytes: &[u8]) -> anyhow::Result<ParsedFrames> {
    let mut frames = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let start = offset;
        if bytes.len() - offset < LOG_MAGIC.len() {
            if LOG_MAGIC.starts_with(&bytes[offset..]) {
                return torn_frames(frames, start);
            }
            bail!("terminal history frame magic mismatch at byte {offset}");
        }
        if &bytes[offset..offset + LOG_MAGIC.len()] != LOG_MAGIC {
            bail!("terminal history frame magic mismatch at byte {offset}");
        }
        offset += LOG_MAGIC.len();

        let version_bytes = LOG_VERSION.to_le_bytes();
        let available = (bytes.len() - offset).min(version_bytes.len());
        if bytes[offset..offset + available] != version_bytes[..available] {
            bail!("unsupported terminal history log version prefix");
        }
        if available < version_bytes.len() {
            return torn_frames(frames, start);
        }
        let body_start = offset;
        let version = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
        offset += 2;
        if version != LOG_VERSION {
            bail!("unsupported terminal history log version {version}");
        }

        if offset == bytes.len() {
            return torn_frames(frames, start);
        }
        let kind = FrameKind::try_from(bytes[offset])?;
        offset += 1;

        let generation_length_bytes = (bytes.len() - offset).min(2);
        if generation_length_bytes == 0 {
            return torn_frames(frames, start);
        }
        let expected_generation_len = TerminalIncarnationId::PREFIX.len() + 36;
        if generation_length_bytes == 1 {
            if bytes[offset] as usize != expected_generation_len {
                bail!("terminal history generation length prefix is invalid");
            }
            return torn_frames(frames, start);
        }
        let generation_len = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) as usize;
        offset += 2;
        if generation_len > GENERATION_BYTES_LIMIT || generation_len != expected_generation_len {
            bail!("terminal history generation length exceeds limit");
        }

        let available_generation = (bytes.len() - offset).min(generation_len);
        if !valid_generation_prefix(
            &bytes[offset..offset + available_generation],
            generation_len,
        ) {
            bail!("terminal history generation prefix is invalid");
        }
        if available_generation < generation_len {
            return torn_frames(frames, start);
        }
        let generation = std::str::from_utf8(&bytes[offset..offset + generation_len])
            .context("terminal history generation is not UTF-8")?;
        let generation = TerminalIncarnationId::from_str(generation)
            .context("terminal history generation is invalid")?;
        offset += generation_len;

        if bytes.len() - offset < 8 {
            return torn_frames(frames, start);
        }
        let sequence = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
        offset += 8;

        let available_payload_len = (bytes.len() - offset).min(4);
        if available_payload_len < 4 {
            let mut lower_bound = [0u8; 4];
            lower_bound[..available_payload_len]
                .copy_from_slice(&bytes[offset..offset + available_payload_len]);
            if u32::from_le_bytes(lower_bound) as usize > FRAME_PAYLOAD_LIMIT {
                bail!("terminal history payload length exceeds limit");
            }
            return torn_frames(frames, start);
        }
        let payload_len =
            u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        if payload_len > FRAME_PAYLOAD_LIMIT {
            bail!("terminal history payload length exceeds limit");
        }
        if bytes.len() - offset < payload_len {
            return torn_frames(frames, start);
        }
        let payload = bytes[offset..offset + payload_len].to_vec();
        offset += payload_len;
        let actual = crc32fast::hash(&bytes[body_start..offset]);
        let checksum = actual.to_le_bytes();
        let available_checksum = (bytes.len() - offset).min(checksum.len());
        if bytes[offset..offset + available_checksum] != checksum[..available_checksum] {
            bail!("terminal history frame checksum mismatch at byte {start}");
        }
        if available_checksum < checksum.len() {
            return torn_frames(frames, start);
        }
        let expected = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        offset += 4;
        if actual != expected {
            bail!("terminal history frame checksum mismatch at byte {start}");
        }
        frames.push(HistoryFrame {
            kind,
            generation,
            sequence,
            payload,
        });
    }
    Ok(ParsedFrames {
        frames,
        valid_len: offset,
        torn_tail: false,
    })
}

fn prepare_directory(path: &Path) -> anyhow::Result<()> {
    if let Some(root) = path.parent() {
        fs::create_dir_all(root).context("create terminal history root")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(root, fs::Permissions::from_mode(0o700))
                .context("secure terminal history root")?;
        }
    }
    fs::create_dir_all(path).context("create terminal history directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .context("secure terminal history directory")?;
    }
    Ok(())
}

fn write_invalidation_marker(paths: &HistoryPaths) -> anyhow::Result<()> {
    prepare_directory(&paths.directory)?;
    mt_core::atomic_write(&paths.invalidated, INVALIDATED_MARKER)
        .context("write terminal history invalidation marker")?;
    secure_file(&paths.invalidated)
}

fn secure_file(_path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(_path, fs::Permissions::from_mode(0o600))
            .context("secure terminal history file")?;
    }
    Ok(())
}

fn write_meta(
    paths: &HistoryPaths,
    session_id: &TerminalSessionId,
    worktree_id: &WorktreeId,
    generation: &TerminalIncarnationId,
) -> anyhow::Result<()> {
    let meta = HistoryMeta {
        magic: META_MAGIC.into(),
        version: HISTORY_VERSION,
        session_id: session_id.clone(),
        worktree_id: worktree_id.clone(),
        incarnation_id: generation.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&meta).context("encode terminal history metadata")?;
    mt_core::atomic_write(&paths.meta, &bytes).context("write terminal history metadata")?;
    secure_file(&paths.meta)
}

fn write_checkpoint(
    paths: &HistoryPaths,
    session_id: &TerminalSessionId,
    worktree_id: &WorktreeId,
    generation: &TerminalIncarnationId,
    through_sequence: u64,
    snapshot: &TerminalSnapshot,
) -> anyhow::Result<()> {
    let checkpoint = HistoryCheckpoint {
        magic: CHECKPOINT_MAGIC.into(),
        version: HISTORY_VERSION,
        session_id: session_id.clone(),
        worktree_id: worktree_id.clone(),
        generation: generation.clone(),
        through_sequence,
        snapshot_b64: base64::engine::general_purpose::STANDARD.encode(snapshot.as_bytes()),
        snapshot_crc32: crc32fast::hash(snapshot.as_bytes()),
    };
    let bytes =
        serde_json::to_vec_pretty(&checkpoint).context("encode terminal history checkpoint")?;
    if bytes.len() > JSON_BYTES_LIMIT {
        bail!("terminal history checkpoint exceeds byte limit");
    }
    mt_core::atomic_write(&paths.checkpoint, &bytes)
        .context("write terminal history checkpoint")?;
    secure_file(&paths.checkpoint)
}

fn open_log(path: &Path, truncate: bool) -> anyhow::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(truncate)
        .open(path)
        .context("open terminal history log")?;
    secure_file(path)?;
    Ok(file)
}

fn truncate_log(path: &Path, len: usize) -> anyhow::Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .context("open torn terminal history log")?;
    file.set_len(len as u64)
        .context("truncate torn terminal history log")?;
    let _ = file.sync_data();
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, max_bytes: usize) -> anyhow::Result<T> {
    let bytes = read_limited(path, max_bytes)?;
    serde_json::from_slice(&bytes).with_context(|| format!("decode {}", path.display()))
}

fn read_limited(path: &Path, max_bytes: usize) -> anyhow::Result<Vec<u8>> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let read_limit = max_bytes
        .checked_add(1)
        .ok_or_else(|| anyhow!("{} byte limit overflow", path.display()))?;
    let mut limited = file.take(read_limit as u64);
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    limited.read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        bail!("{} exceeds byte limit", path.display());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::*;

    fn root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "mth-history-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn worktree_id() -> WorktreeId {
        format!("worktree-v1:{}", "0".repeat(64)).parse().unwrap()
    }

    #[test]
    fn cold_close_metadata_distinguishes_absence_from_unidentified_history() {
        let root = root("close-metadata");
        let session_id = TerminalSessionId::new();
        let generation = TerminalIncarnationId::new();
        let paths = HistoryPaths::new(&root, &session_id).unwrap();
        assert_eq!(stored_incarnation(&root, &session_id).unwrap(), None);

        prepare_directory(&paths.directory).unwrap();
        fs::write(&paths.log, b"unidentified history").unwrap();
        assert!(stored_incarnation(&root, &session_id).is_err());
        assert_eq!(fs::read(&paths.log).unwrap(), b"unidentified history");
        fs::write(&paths.meta, b"invalid metadata").unwrap();
        assert!(stored_incarnation(&root, &session_id).is_err());
        fs::write(&paths.meta, vec![b' '; 64 * 1024 + 1]).unwrap();
        assert!(stored_incarnation(&root, &session_id).is_err());

        write_meta(
            &paths,
            &TerminalSessionId::new(),
            &worktree_id(),
            &generation,
        )
        .unwrap();
        assert!(stored_incarnation(&root, &session_id).is_err());
        write_meta(&paths, &session_id, &worktree_id(), &generation).unwrap();
        assert_eq!(
            stored_incarnation(&root, &session_id).unwrap(),
            Some(generation)
        );
        assert_eq!(fs::read(&paths.log).unwrap(), b"unidentified history");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn cold_close_metadata_rejects_symlinked_history() {
        let root = root("close-symlink");
        let session_id = TerminalSessionId::new();
        let paths = HistoryPaths::new(&root, &session_id).unwrap();
        let other = root.join("other-history");
        fs::create_dir_all(&other).unwrap();
        std::os::unix::fs::symlink(&other, &paths.directory).unwrap();
        assert!(stored_incarnation(&root, &session_id).is_err());
        fs::remove_file(&paths.directory).unwrap();
        prepare_directory(&paths.directory).unwrap();
        let other_meta = other.join("meta.json");
        fs::write(&other_meta, b"untouched").unwrap();
        std::os::unix::fs::symlink(&other_meta, &paths.meta).unwrap();
        assert!(stored_incarnation(&root, &session_id).is_err());
        assert_eq!(fs::read(other_meta).unwrap(), b"untouched");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn frame_parser_accepts_torn_tail_but_rejects_checksum_corruption() {
        let generation = TerminalIncarnationId::new();
        let first = encode_frame(FrameKind::Output, &generation, 1, b"one").unwrap();
        let second = encode_frame(FrameKind::Output, &generation, 2, b"two").unwrap();
        let mut torn = first.clone();
        torn.extend_from_slice(&second[..second.len() - 3]);
        let parsed = parse_frames(&torn).unwrap();
        assert_eq!(parsed.frames.len(), 1);
        assert!(parsed.torn_tail);
        assert_eq!(parsed.valid_len, first.len());

        let mut corrupt = first;
        *corrupt.last_mut().unwrap() ^= 0xff;
        assert!(parse_frames(&corrupt).is_err());

        let mut invalid_suffix = encode_frame(FrameKind::Output, &generation, 1, b"one").unwrap();
        invalid_suffix.extend_from_slice(b"BAD");
        assert!(
            parse_frames(&invalid_suffix).is_err(),
            "an arbitrary suffix is not a torn frame prefix"
        );
    }

    #[test]
    fn recovery_replays_ordered_output_resize_and_split_escape() {
        let root = root("replay");
        let session_id = TerminalSessionId::new();
        let worktree_id = worktree_id();
        let generation = TerminalIncarnationId::new();
        let history = SessionHistory::pending(HistorySeed {
            root: &root,
            session_id: session_id.clone(),
            worktree_id: worktree_id.clone(),
            generation: generation.clone(),
            rows: 4,
            cols: 20,
            scrollback: 64,
            initial_snapshot: None,
        })
        .unwrap();
        assert!(history.activate());
        history.record_output(b"before\r\n\x1b[31");
        history.record_output(b"mred\x1b[0m");
        history.record_resize(5, 24);
        history.record_output(b"\r\nafter");

        let recovered = recover(&root, &session_id, &worktree_id, &generation).unwrap();
        let emulator = TerminalEmulator::new(TermSize::new(1, 1));
        let metadata = emulator.restore_snapshot(&recovered.snapshot).unwrap();
        assert_eq!(metadata.source_size, TermSize::new(24, 5));
        let lines = emulator.visible_lines().join("\n");
        assert!(lines.contains("before"));
        assert!(lines.contains("red"));
        assert!(lines.contains("after"));

        let paths = HistoryPaths::new(&root, &session_id).unwrap();
        let original_len = fs::metadata(&paths.log).unwrap().len();
        OpenOptions::new()
            .append(true)
            .open(&paths.log)
            .unwrap()
            .write_all(&LOG_MAGIC[..3])
            .unwrap();
        recover(&root, &session_id, &worktree_id, &generation).unwrap();
        assert_eq!(fs::metadata(&paths.log).unwrap().len(), original_len);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn identity_mismatch_and_explicit_purge_fail_closed() {
        let root = root("identity");
        let session_id = TerminalSessionId::new();
        let worktree_id = worktree_id();
        let generation = TerminalIncarnationId::new();
        let history = SessionHistory::pending(HistorySeed {
            root: &root,
            session_id: session_id.clone(),
            worktree_id: worktree_id.clone(),
            generation: generation.clone(),
            rows: 4,
            cols: 20,
            scrollback: 64,
            initial_snapshot: None,
        })
        .unwrap();
        assert!(history.activate());
        history.record_output(b"public-output");

        assert!(
            recover(
                &root,
                &session_id,
                &worktree_id,
                &TerminalIncarnationId::new()
            )
            .is_err()
        );
        let paths = HistoryPaths::new(&root, &session_id).unwrap();
        for path in [&paths.meta, &paths.checkpoint, &paths.log] {
            let bytes = fs::read(path).unwrap();
            assert!(!bytes.windows(6).any(|window| window == b"secret"));
        }
        purge(&root, &session_id).unwrap();
        assert!(!paths.directory.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn log_rotation_stays_within_the_byte_budget() {
        let root = root("bounds");
        let session_id = TerminalSessionId::new();
        let worktree_id = worktree_id();
        let generation = TerminalIncarnationId::new();
        let history = SessionHistory::pending(HistorySeed {
            root: &root,
            session_id: session_id.clone(),
            worktree_id,
            generation,
            rows: 4,
            cols: 20,
            scrollback: 8,
            initial_snapshot: None,
        })
        .unwrap();
        assert!(history.activate());
        let payload = vec![b'x'; FRAME_PAYLOAD_LIMIT];
        {
            let mut state = history.state.lock();
            for _ in 0..10 {
                let sequence = state.allocate_sequence();
                state
                    .append_frame(FrameKind::Output, sequence, &payload)
                    .unwrap();
            }
            assert!(state.log_bytes <= LOG_BYTES_LIMIT);
        }
        let paths = HistoryPaths::new(&root, &session_id).unwrap();
        assert!(fs::metadata(&paths.log).unwrap().len() <= LOG_BYTES_LIMIT as u64);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recovery_rejects_sequence_gaps_wrong_generations_and_versions() {
        let root = root("corrupt");
        let session_id = TerminalSessionId::new();
        let worktree_id = worktree_id();
        let generation = TerminalIncarnationId::new();
        let history = SessionHistory::pending(HistorySeed {
            root: &root,
            session_id: session_id.clone(),
            worktree_id: worktree_id.clone(),
            generation: generation.clone(),
            rows: 4,
            cols: 20,
            scrollback: 8,
            initial_snapshot: None,
        })
        .unwrap();
        assert!(history.activate());
        assert!(history.seal());
        let paths = HistoryPaths::new(&root, &session_id).unwrap();

        let first = encode_frame(FrameKind::Output, &generation, 1, b"one").unwrap();
        let third = encode_frame(FrameKind::Output, &generation, 3, b"three").unwrap();
        fs::write(&paths.log, [first, third].concat()).unwrap();
        assert!(recover(&root, &session_id, &worktree_id, &generation).is_err());

        let wrong = encode_frame(
            FrameKind::Output,
            &TerminalIncarnationId::new(),
            1,
            b"wrong",
        )
        .unwrap();
        fs::write(&paths.log, wrong).unwrap();
        assert!(recover(&root, &session_id, &worktree_id, &generation).is_err());

        let mut invalid_version = encode_frame(FrameKind::Output, &generation, 1, b"one").unwrap();
        invalid_version[LOG_MAGIC.len()] = 0xff;
        fs::write(&paths.log, invalid_version).unwrap();
        assert!(recover(&root, &session_id, &worktree_id, &generation).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn limited_reads_accept_the_bound_and_reject_one_extra_byte() {
        let root = root("read-limit");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("bounded.bin");
        fs::write(&path, b"1234").unwrap();
        assert_eq!(read_limited(&path, 4).unwrap(), b"1234");

        fs::write(&path, b"12345").unwrap();
        assert!(read_limited(&path, 4).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn history_io_failure_is_durably_invalidated() {
        let root = root("durable-invalidation");
        let session_id = TerminalSessionId::new();
        let worktree_id = worktree_id();
        let generation = TerminalIncarnationId::new();
        let history = SessionHistory::pending(HistorySeed {
            root: &root,
            session_id: session_id.clone(),
            worktree_id: worktree_id.clone(),
            generation: generation.clone(),
            rows: 4,
            cols: 20,
            scrollback: 8,
            initial_snapshot: None,
        })
        .unwrap();
        assert!(history.activate());
        history.state.lock().log = None;
        history.record_output(b"cannot-be-recorded");

        let paths = HistoryPaths::new(&root, &session_id).unwrap();
        assert!(paths.invalidated.is_file());
        assert!(recover(&root, &session_id, &worktree_id, &generation).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalidation_fence_quiesces_history_before_purge() {
        let root = root("purge-fence");
        let session_id = TerminalSessionId::new();
        let history = Arc::new(
            SessionHistory::pending(HistorySeed {
                root: &root,
                session_id: session_id.clone(),
                worktree_id: worktree_id(),
                generation: TerminalIncarnationId::new(),
                rows: 4,
                cols: 20,
                scrollback: 8,
                initial_snapshot: None,
            })
            .unwrap(),
        );
        assert!(history.activate());
        let paths = HistoryPaths::new(&root, &session_id).unwrap();

        let state = history.state.lock();
        let worker_history = history.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            done_tx.send(worker_history.invalidate_and_wait()).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            done_rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "purge fence returned while history state was still in flight"
        );
        drop(state);

        done_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap();
        worker.join().unwrap();
        purge(&root, &session_id).unwrap();
        history.record_output(b"late-output");
        assert!(
            !paths.directory.exists(),
            "invalidated history recreated files after purge"
        );
        let _ = fs::remove_dir_all(root);
    }
}
