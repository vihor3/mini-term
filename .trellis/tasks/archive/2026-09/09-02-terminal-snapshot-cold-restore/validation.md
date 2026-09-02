# Validation

Date: 2026-09-02

## Delivered

- Added a bounded zstd terminal snapshot codec preserving source size, grid,
  scrollback, cursor state, cell attributes, wide cells, and parser tail.
- Added current-user per-session terminal history with atomic metadata and
  checkpoints plus an 8 MiB checksummed framed output log.
- Added explicit protocol-v2 cold restore with worktree/previous-incarnation
  validation, new-incarnation fencing, cleanup, and corruption rejection.
- Integrated distinct app recovery states and snapshot-before-attach ordering;
  warm reattach alone suppresses provider resume.

## Docker Verification

- Targeted `rustfmt --check` for every task-owned Rust file: passed.
- `cargo test --locked -p mt-terminal`: 17 tests passed.
- `cargo test --locked -p mt-terminal-host`: 11 unit tests and 4 real
  lifecycle tests passed.
- `cargo test --locked -p mt-app terminal -- --test-threads=1`: 19 passed.
- `cargo check --locked -p mt-app --tests`: passed with only pre-existing tray
  dead-code warnings.
- `cargo clippy --locked --no-deps -p mt-terminal -p mt-terminal-host
  --all-targets -- -D warnings`: passed.
- `cargo xwin check --locked --target x86_64-pc-windows-msvc
  -p mt-terminal-host -p mt-app`: passed.
- A final incremental Windows check of `mt-terminal-host` after the warning fix
  passed without warnings.
- `git diff --check`: passed.

## Recovery Assertions

- Valid history restores the same logical session into a new incarnation and
  accepts new output only after the snapshot is installed.
- Old incarnations and wrong worktrees cannot attach, restore, or mutate the
  recovered session.
- Only an incomplete final frame is truncated; checksum, version, generation,
  sequence, and payload-bound failures reject recovery.
- Explicit kill closes the recorder before removing its directory, including
  on Windows-compatible file semantics.
- Persisted history excludes spawn arguments, user environment, and autofill
  credentials.

## Host Hygiene

- Repository `target/`, `~/.cargo`, and `~/.rustup` are absent.
- All Rust formatting, compilation, linting, and tests ran in Docker caches.

## Residual Limit

- Windows compilation is verified in Docker; interactive restore on a
  physical Windows desktop remains part of final installer smoke testing.
