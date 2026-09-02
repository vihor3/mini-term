# Validation

Date: 2026-09-02

## Delivered

- Added the independent `mt-terminal-host` library and executable with a
  versioned JSONL/base64 current-user IPC protocol.
- Moved eligible local/WSL PTY ownership behind a hosted transport while
  retaining the legacy compatibility backend and
  `MINI_TERM_TERMINAL_HOST=0` rollback.
- Persisted host-generated incarnations only after create/attach success and
  fenced mutations and pane callbacks by the returned incarnation.
- Made GUI/project unregister paths detach hosted sessions and kept explicit
  terminal/worktree close paths on the kill route.
- Added bounded ordered replay, stale socket recovery, endpoint contention,
  current-user permissions, idle shutdown, and packaged sidecar wiring.

## Docker Verification

Native Linux checks ran only in `mini-term-ci:rust-1.95`:

- `cargo test --locked -p mt-terminal-host`: 5 unit tests and 3 real lifecycle
  tests passed.
- Lifecycle coverage verifies missing attach, owner-only socket modes, same PID
  and incarnation after detach/attach, detached output replay exactly once,
  queued hosted writes, stale write/resize/autofill/kill rejection, explicit
  child termination, busy idle-shutdown rejection, stale socket recovery, and
  live endpoint owner contention.
- `cargo test --locked -p mt-pty`: 59 unit tests and 1 doctest passed.
- `cargo test --locked -p mt-app terminal -- --test-threads=1`: 19 matching
  tests passed, 792 unrelated tests filtered out.
- `cargo check --locked -p mt-app --tests`: passed with only the two existing
  Linux tray dead-code warnings.
- `cargo clippy --locked --no-deps -p mt-pty -p mt-terminal-host
  --all-targets`: passed without warnings from either changed crate.
- `node --check` passed for `stage-sidecars.mjs` and `stage-conpty.mjs` in the
  Docker CI image.
- `actionlint` passed for `.github/workflows/release.yml` in Docker.

## Windows Verification

Windows checks ran only in Docker with Rust 1.95, `cargo-xwin 0.19.2`, and the
`x86_64-pc-windows-msvc` target:

- `cargo xwin check --locked --target x86_64-pc-windows-msvc
  -p mt-terminal-host`: passed without warnings.
- `cargo xwin build --locked --release --target x86_64-pc-windows-msvc
  -p mt-terminal-host`: passed.
- The release host is `PE32+ executable (console) x86-64`.
- Host SHA-256:
  `4671bef504164e823fa5cee9cf7d089a2aee6909e3f5d3d83ff11bcf595244e0`.

## Installer Payload Check

A temporary NSIS validation installer was compiled and extracted in Docker.
It used the previously validated stable-identity main/sidecar/ConPTY payload as
the fixed baseline and replaced only `mt-terminal-host.exe` with this task's
new Windows release binary. This check is not the final user-facing installer.

- Installer size: `17,310,503` bytes.
- Installer SHA-256:
  `44866b3dac5ca9dab419db0c16c1d5f37b48caf4c755b783e532cd566e5650d0`.
- Extracted payload contains `mini-term.exe`, all three existing sidecars,
  `mt-terminal-host.exe`, and all portable ConPTY files.
- Staged and extracted `mt-terminal-host.exe` hashes match exactly.
- NSIS upgrade kill, install, and uninstall definitions all include
  `mt-terminal-host.exe`.

The final parent integration gate will rebuild the main executable and every
sidecar from the completed parent source before producing the deliverable
installer.

## Host Hygiene

- The repository has no host `target/` directory.
- The host has no `~/.cargo` or `~/.rustup` directory.
- Rust compilation, formatting, linting, tests, Windows cross-build, NSIS
  compilation, and package extraction were all performed in Docker.

## Residual Limit

The current Linux environment cannot exercise installation and warm reattach
on a physical Windows desktop. Windows compile, PE, NSIS, and extracted-payload
contracts are verified; runtime smoke testing remains a real-Windows check.
