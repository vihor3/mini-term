# Terminal Snapshot Cold Restore

## Goal

Persist bounded terminal visual history and restore a dead hosted session into a new fenced incarnation with explicit cold-recovery status.

## Requirements

- Persist local hosted-terminal visual history independently of the GUI process
  under a current-user data directory. Each logical terminal owns `meta.json`,
  an atomically replaced `checkpoint.json`, and a bounded framed `output.log`.
- Keep warm attach pure. Missing/exited hosted sessions are recovered only by an
  explicit restore request that validates the persisted worktree and previous
  incarnation, starts a new PTY, and returns a new host-generated incarnation.
- Restore the terminal grid, cursor, source dimensions, scrollback, and any
  incomplete parser tail before new-process output is applied. Resize to the
  current view only after source-size restoration.
- Frame incremental output with magic, version, generation, monotonic sequence,
  kind, payload length, payload, and checksum. A torn final frame may be
  truncated; checksum failures, sequence gaps, generation mismatches, and
  malformed middle frames fail closed.
- Bound checkpoint and log storage and remove recovery state after an explicit
  terminal kill. Never persist passwords, user environment values, or other
  launch secrets.
- Distinguish `Fresh`, `Reattached`, `Restored from history`, and unavailable
  recovery in the application state. Provider resume remains a separate step;
  a warm attach must continue to suppress duplicate resume.
- Preserve `MINI_TERM_TERMINAL_HOST=0` as the rollback path and leave SSH panes
  on their existing compatibility transport.

## Acceptance Criteria

- [ ] Terminal snapshot round trips preserve visible history, cursor placement,
      source size, wide characters, colors, and a split escape sequence.
- [ ] A stopped terminal host can restore valid history into the same logical
      terminal session with a different incarnation, and new output follows the
      restored view without gaps or duplication.
- [ ] An old incarnation cannot write, resize, kill, attach, or restore over the
      new one; a worktree mismatch also fails closed.
- [ ] Torn-tail recovery truncates only the incomplete final frame. Corrupt
      checksums, sequence gaps, invalid versions, and wrong generations report
      recovery unavailable without silently skipping data.
- [ ] Checkpoint/log rotation remains under documented byte bounds, explicit
      kill removes recovery files, and persisted files contain no launch secret.
- [ ] App hydration labels cold restore separately, runs provider resume only
      after cold recovery when eligible, and keeps warm reattach quiet.
- [ ] Linux unit/integration checks and Windows MSVC compilation pass in Docker;
      no Rust compiler or build artifact is created on the host workspace.

## Notes

- Depends on the archived `09-02-terminal-host-warm-reattach` contract.
- This is visual/state recovery into a new process. It never claims to revive a
  dead OS process.
