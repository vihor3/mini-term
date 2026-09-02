# Implementation Plan

1. Extend `mt-terminal` with the bounded snapshot codec, incomplete-sequence
   tracker, restore API, and golden/edge-case tests.
2. Add terminal-host durable-history storage, binary frame reader/writer,
   atomic checkpoints, rotation, cleanup, and corruption tests.
3. Upgrade the host protocol/client/server with explicit restore, worktree and
   previous-incarnation validation, and restored-snapshot handoff.
4. Integrate the application recovery enum, snapshot application, cold restore
   notice, and provider-resume ordering without changing SSH behavior.
5. Add real host-process integration coverage for crash/restart restore,
   sequence continuity, stale fencing, torn tail, corruption, bounds, and
   secret exclusion.
6. Update terminal-host/workbench specifications and packaging protocol notes.
7. Run Docker-only format, targeted tests, workspace checks/clippy, Windows
   MSVC checks, diff checks, then commit and archive.

## Validation Commands

All Rust commands run in the project Docker images with external Cargo/target
caches:

```text
cargo fmt --all -- --check
cargo test --locked -p mt-terminal
cargo test --locked -p mt-terminal-host
cargo test --locked -p mt-app terminal -- --test-threads=1
cargo check --locked -p mt-app --tests
cargo clippy --locked --no-deps -p mt-terminal -p mt-terminal-host --all-targets
cargo xwin check --locked --target x86_64-pc-windows-msvc -p mt-terminal-host -p mt-app
git diff --check
```

## Rollback Point

The pre-change warm-host commits are `8e8a7dd` and `ffbc6db`. The compatibility
runtime remains selectable through `MINI_TERM_TERMINAL_HOST=0` throughout this
child.
