use std::io::Read as _;

use alacritty_terminal::grid::Grid;
use alacritty_terminal::index::Point;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::vte::ansi::Processor;
use anyhow::{Context as _, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::{EventQueue, TermSize, TerminalEmulator};

const SNAPSHOT_VERSION: u32 = 1;
const SNAPSHOT_MAX_DECOMPRESSED_BYTES: u64 = 128 * 1024 * 1024;
const PARSER_TAIL_MAX_BYTES: usize = 2 * 1024 * 1024;
pub const SNAPSHOT_MAX_COMPRESSED_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotMetadata {
    pub source_size: TermSize,
    pub scrollback: usize,
}

#[derive(Clone, Serialize, Deserialize)]
struct SnapshotCursor {
    point: Point,
    template: Cell,
    input_needs_wrap: bool,
}

impl SnapshotCursor {
    fn active(grid: &Grid<Cell>) -> Self {
        Self {
            point: grid.cursor.point,
            template: grid.cursor.template.clone(),
            input_needs_wrap: grid.cursor.input_needs_wrap,
        }
    }

    fn saved(grid: &Grid<Cell>) -> Self {
        Self {
            point: grid.saved_cursor.point,
            template: grid.saved_cursor.template.clone(),
            input_needs_wrap: grid.saved_cursor.input_needs_wrap,
        }
    }

    fn install_active(self, grid: &mut Grid<Cell>) {
        grid.cursor.point = self.point;
        grid.cursor.template = self.template;
        grid.cursor.input_needs_wrap = self.input_needs_wrap;
    }

    fn install_saved(self, grid: &mut Grid<Cell>) {
        grid.saved_cursor.point = self.point;
        grid.saved_cursor.template = self.template;
        grid.saved_cursor.input_needs_wrap = self.input_needs_wrap;
    }
}

#[derive(Serialize, Deserialize)]
struct SnapshotBody {
    version: u32,
    columns: usize,
    screen_lines: usize,
    scrollback: usize,
    grid: Grid<Cell>,
    cursor: SnapshotCursor,
    saved_cursor: SnapshotCursor,
    #[serde(default)]
    parser_tail: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct TerminalSnapshot(Vec<u8>);

impl std::fmt::Debug for TerminalSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TerminalSnapshot")
            .field("compressed_bytes", &self.0.len())
            .finish()
    }
}

impl TerminalSnapshot {
    pub fn from_bytes(bytes: Vec<u8>) -> anyhow::Result<Self> {
        if bytes.len() > SNAPSHOT_MAX_COMPRESSED_BYTES {
            bail!("terminal snapshot exceeds compressed byte limit");
        }
        let snapshot = Self(bytes);
        snapshot.decode_body()?;
        Ok(snapshot)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    fn encode(body: &SnapshotBody) -> anyhow::Result<Self> {
        let json = serde_json::to_vec(body).context("encode terminal snapshot")?;
        let compressed =
            zstd::stream::encode_all(json.as_slice(), 3).context("compress terminal snapshot")?;
        if compressed.len() > SNAPSHOT_MAX_COMPRESSED_BYTES {
            bail!("terminal snapshot exceeds compressed byte limit");
        }
        Ok(Self(compressed))
    }

    fn decode_body(&self) -> anyhow::Result<SnapshotBody> {
        if self.0.len() > SNAPSHOT_MAX_COMPRESSED_BYTES {
            bail!("terminal snapshot exceeds compressed byte limit");
        }
        let decoder = zstd::stream::read::Decoder::new(self.0.as_slice())
            .context("open terminal snapshot")?;
        let mut json = Vec::new();
        decoder
            .take(SNAPSHOT_MAX_DECOMPRESSED_BYTES + 1)
            .read_to_end(&mut json)
            .context("decompress terminal snapshot")?;
        if json.len() as u64 > SNAPSHOT_MAX_DECOMPRESSED_BYTES {
            bail!("terminal snapshot exceeds decompressed byte limit");
        }
        let body: SnapshotBody =
            serde_json::from_slice(&json).context("decode terminal snapshot")?;
        validate_body(&body)?;
        Ok(body)
    }
}

fn validate_body(body: &SnapshotBody) -> anyhow::Result<()> {
    use alacritty_terminal::grid::Dimensions as _;

    if body.version != SNAPSHOT_VERSION {
        bail!("unsupported terminal snapshot version {}", body.version);
    }
    if body.columns == 0 || body.screen_lines == 0 {
        bail!("terminal snapshot dimensions must be non-zero");
    }
    if body.columns != body.grid.columns() || body.screen_lines != body.grid.screen_lines() {
        bail!("terminal snapshot dimensions do not match the grid");
    }
    if body.parser_tail.len() > PARSER_TAIL_MAX_BYTES {
        bail!("terminal snapshot parser tail exceeds byte limit");
    }
    validate_cursor(&body.grid, &body.cursor)?;
    validate_cursor(&body.grid, &body.saved_cursor)?;
    Ok(())
}

fn validate_cursor(grid: &Grid<Cell>, cursor: &SnapshotCursor) -> anyhow::Result<()> {
    use alacritty_terminal::grid::Dimensions as _;

    let history = grid.history_size() as i32;
    if cursor.point.line.0 < -history
        || cursor.point.line.0 >= grid.screen_lines() as i32
        || cursor.point.column.0 >= grid.columns()
    {
        return Err(anyhow!("terminal snapshot cursor is outside the grid"));
    }
    Ok(())
}

pub(crate) fn capture(emulator: &TerminalEmulator) -> anyhow::Result<TerminalSnapshot> {
    use alacritty_terminal::grid::Dimensions as _;

    let term = emulator.term.lock();
    let parser = emulator.parser.lock();
    let grid = term.grid().clone();
    let body = SnapshotBody {
        version: SNAPSHOT_VERSION,
        columns: grid.columns(),
        screen_lines: grid.screen_lines(),
        scrollback: emulator.scrollback(),
        cursor: SnapshotCursor::active(&grid),
        saved_cursor: SnapshotCursor::saved(&grid),
        grid,
        parser_tail: parser.tail().to_vec(),
    };
    TerminalSnapshot::encode(&body)
}

pub(crate) fn restore(
    emulator: &TerminalEmulator,
    snapshot: &TerminalSnapshot,
) -> anyhow::Result<SnapshotMetadata> {
    let body = snapshot.decode_body()?;
    let metadata = SnapshotMetadata {
        source_size: TermSize::new(body.columns, body.screen_lines),
        scrollback: body.scrollback,
    };
    let mut term = emulator.term.lock();
    let mut parser = emulator.parser.lock();
    let mut grid = body.grid;
    body.cursor.install_active(&mut grid);
    body.saved_cursor.install_saved(&mut grid);
    *term.grid_mut() = grid;
    emulator
        .scrollback
        .store(metadata.scrollback, std::sync::atomic::Ordering::Relaxed);
    *parser = ParserState::new();
    parser.advance(&mut term, &body.parser_tail);
    Ok(metadata)
}

pub(crate) struct ParserState {
    processor: Processor,
    tail: ParserTail,
}

impl ParserState {
    pub(crate) fn new() -> Self {
        Self {
            processor: Processor::new(),
            tail: ParserTail::default(),
        }
    }

    pub(crate) fn advance(&mut self, term: &mut Term<EventQueue>, bytes: &[u8]) {
        self.tail.advance(bytes);
        self.processor.advance(term, bytes);
    }

    fn tail(&self) -> &[u8] {
        &self.tail.bytes
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum TailState {
    #[default]
    Ground,
    Escape,
    Csi,
    Osc {
        escaped: bool,
    },
    String {
        escaped: bool,
    },
    Utf8 {
        remaining: u8,
    },
    Sync,
}

#[derive(Default)]
struct ParserTail {
    state: TailState,
    bytes: Vec<u8>,
}

impl ParserTail {
    fn advance(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.advance_byte(byte);
            if self.bytes.len() > PARSER_TAIL_MAX_BYTES {
                self.state = TailState::Ground;
                self.bytes.clear();
            }
        }
    }

    fn advance_byte(&mut self, byte: u8) {
        if self.state == TailState::Sync {
            self.bytes.push(byte);
            if self.bytes.ends_with(b"\x1b[?2026l") || self.bytes.len() >= PARSER_TAIL_MAX_BYTES {
                self.state = TailState::Ground;
                self.bytes.clear();
            }
            return;
        }

        match self.state {
            TailState::Ground => self.start(byte),
            TailState::Utf8 { remaining } => {
                if byte & 0xc0 == 0x80 {
                    self.bytes.push(byte);
                    if remaining == 1 {
                        self.state = TailState::Ground;
                        self.bytes.clear();
                    } else {
                        self.state = TailState::Utf8 {
                            remaining: remaining - 1,
                        };
                    }
                } else {
                    self.state = TailState::Ground;
                    self.bytes.clear();
                    self.start(byte);
                }
            }
            TailState::Escape => {
                self.bytes.push(byte);
                match byte {
                    b'[' => self.state = TailState::Csi,
                    b']' => self.state = TailState::Osc { escaped: false },
                    b'P' | b'_' | b'^' | b'X' => self.state = TailState::String { escaped: false },
                    0x20..=0x2f => {}
                    0x30..=0x7e => {
                        self.state = TailState::Ground;
                        self.bytes.clear();
                    }
                    0x1b => {
                        self.bytes.clear();
                        self.bytes.push(0x1b);
                    }
                    _ => {
                        self.state = TailState::Ground;
                        self.bytes.clear();
                    }
                }
            }
            TailState::Csi => {
                self.bytes.push(byte);
                if (0x40..=0x7e).contains(&byte) {
                    if self.bytes == b"\x1b[?2026h" {
                        self.state = TailState::Sync;
                    } else {
                        self.state = TailState::Ground;
                        self.bytes.clear();
                    }
                } else if byte == 0x1b {
                    self.state = TailState::Escape;
                    self.bytes.clear();
                    self.bytes.push(0x1b);
                }
            }
            TailState::Osc { escaped } => {
                self.bytes.push(byte);
                if byte == 0x07 || (escaped && byte == b'\\') {
                    self.state = TailState::Ground;
                    self.bytes.clear();
                } else {
                    self.state = TailState::Osc {
                        escaped: byte == 0x1b,
                    };
                }
            }
            TailState::String { escaped } => {
                self.bytes.push(byte);
                if escaped && byte == b'\\' {
                    self.state = TailState::Ground;
                    self.bytes.clear();
                } else {
                    self.state = TailState::String {
                        escaped: byte == 0x1b,
                    };
                }
            }
            TailState::Sync => unreachable!(),
        }
    }

    fn start(&mut self, byte: u8) {
        match byte {
            0x1b => {
                self.state = TailState::Escape;
                self.bytes.push(byte);
            }
            0xc2..=0xdf => {
                self.state = TailState::Utf8 { remaining: 1 };
                self.bytes.push(byte);
            }
            0xe0..=0xef => {
                self.state = TailState::Utf8 { remaining: 2 };
                self.bytes.push(byte);
            }
            0xf0..=0xf4 => {
                self.state = TailState::Utf8 { remaining: 3 };
                self.bytes.push(byte);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_keeps_only_incomplete_sequences() {
        let mut tracker = ParserTail::default();
        tracker.advance(b"plain\x1b[31");
        assert_eq!(tracker.bytes, b"\x1b[31");
        tracker.advance(b"mred");
        assert!(tracker.bytes.is_empty());

        tracker.advance(&[0xe4, 0xb8]);
        assert_eq!(tracker.bytes, [0xe4, 0xb8]);
        tracker.advance(&[0xad]);
        assert!(tracker.bytes.is_empty());
    }

    #[test]
    fn snapshot_round_trip_preserves_grid_cursor_and_split_escape() {
        let source = TerminalEmulator::with_scrollback(TermSize::new(12, 4), 64);
        source.advance(b"first\r\n\x1b[31mred\x1b[0m\r\nwide: ");
        source.advance("中".as_bytes());
        source.advance(b"\r\n\x1b[32");
        let before = source.visible_lines();
        let before_cursor = source.with_term(|term| term.grid().cursor.point);
        let before_grid = source.with_term(|term| term.grid().clone());

        let snapshot = source.snapshot().unwrap();
        let restored = TerminalEmulator::new(TermSize::new(80, 24));
        let metadata = restored.restore_snapshot(&snapshot).unwrap();
        assert_eq!(metadata.source_size, TermSize::new(12, 4));
        assert_eq!(metadata.scrollback, 64);
        assert_eq!(restored.visible_lines(), before);
        assert_eq!(restored.with_term(|term| term.grid().clone()), before_grid);
        assert_eq!(
            restored.with_term(|term| term.grid().cursor.point),
            before_cursor
        );

        restored.advance(b"mgreen\x1b[0m");
        assert!(
            restored
                .visible_lines()
                .iter()
                .any(|line| line.contains("green"))
        );
    }

    #[test]
    fn corrupted_or_oversized_snapshot_is_rejected() {
        assert!(TerminalSnapshot::from_bytes(vec![0; SNAPSHOT_MAX_COMPRESSED_BYTES + 1]).is_err());
        assert!(TerminalSnapshot::from_bytes(b"not-zstd".to_vec()).is_err());
    }
}
