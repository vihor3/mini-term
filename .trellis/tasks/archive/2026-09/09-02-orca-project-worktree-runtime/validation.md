# Validation

Date: 2026-09-03

## Completion Scope

- All ten child tasks are implemented and archived. The parent task now reflects
  the completed Project -> Worktree catalog, stable workbench identity, detached
  terminal host, cold history restore, remote runtime and Agent identity, Orca
  shell, worktree context sidebar, GitHub Tasks, and global Agent activity feed.
- Final integration fixes are committed in `25430a0`: the DnD path test now uses
  platform-correct semantics, the PTY stream test accepts callback ordering without
  losing its timeout, and the IME regression fixture no longer triggers a Clippy
  empty-range error.
- The independent `sidecars/Cargo.lock` is synchronized with the `mt-ssh`
  `mt-identity` dependency. This was required for reproducible `--locked` native
  and Windows sidecar builds.

## Docker Quality Gates

All Rust formatting, compilation, linting, tests, remote smoke work, and Windows
packaging ran in Docker. No host Rust toolchain or repository `target/` was used.

- Task-owned changed Rust files passed `rustfmt --check`. A full
  `cargo fmt --all -- --check` still reports the repository's pre-existing broad
  formatting baseline outside this task; those unrelated files were not rewritten.
- `cargo test --locked --workspace --all-targets --no-fail-fast --quiet`: 1,840
  passed and 1 ignored.
- `cargo check --locked --workspace --all-targets`: passed.
- `cargo clippy --locked --workspace --all-targets --no-deps`: passed with only
  existing warnings in untouched code.
- Sidecar `cargo metadata --locked --offline`, `cargo check --locked
  --all-targets`, and `cargo test --locked --all-targets`: passed; 94 tests passed.
- The PTY callback-order regression test passed 20 consecutive Docker runs.
- Focused DnD and IME regression tests passed.
- `cargo xwin check --locked --target x86_64-pc-windows-msvc -p mt-pty
  -p mt-ui -p mt-app --all-targets`: passed.
- Root Windows release build for `mt-app` and `mt-terminal-host`: passed.
- Locked Windows release build for `miniterm-hook`, `mt-ssh-cli`, and
  `mt-ssh-mcp` from `sidecars/Cargo.toml`: passed.
- `git diff --check`: passed.

## Integration Assertions

- Workbench tests prove identical paths are separated by `WorktreeId`, preview
  replacement is isolated per worktree, and stale callbacks cannot cross a
  project/worktree rebind. The full suite also covers scoped Files, Git, Tasks,
  Sessions, terminal layouts, and persisted mappings.
- Hosted terminal lifecycle tests prove GUI detach/attach keeps the same process
  and incarnation, replays detached output once, rejects stale mutations, and
  kills only on explicit terminal/worktree close. Snapshot tests prove cold restore
  creates a new incarnation and is visibly classified separately from warm reattach.
- Remote runtime and Agent tests cover exact connection epochs, reconnect fencing,
  replay, process disappearance confirmation, and host/worktree/pane ownership.
- A real OpenSSH 9.2 daemon smoke ran in Docker twice. It verified accept-new
  `known_hosts`, password authentication, pooled session reuse, bounded exec,
  heartbeat, SFTP install-ID initialization, permission hardening, and stable
  `ExecutionHostId`, `RepoId`, and `WorktreeId` across repeated inspection.
- GitHub Tasks tests route every repository/auth/list/detail command through the
  selected local, WSL, or SSH execution host. Auth failure plans only
  `gh auth login --hostname <host>`, Copy, and Retry; it never opens a browser or
  terminal and never falls back from remote credentials to local credentials.
- Orca interaction tests keep the shell enabled by default, enforce
  `Files / Git / Tasks / Sessions`, preserve single-click preview and double-click
  promotion/rename rules, and keep the anchored Agents overlay inside compact and
  normal desktop widths without replacing the workbench route.

## Rollback And Compatibility

The final executable contains and the code tests the independent rollback controls
`MINI_TERM_LEGACY_SHELL`, `MINI_TERM_TERMINAL_HOST`,
`MINI_TERM_REMOTE_RUNTIME`, `MINI_TERM_REMOTE_AGENT_STATUS`,
`MINI_TERM_ORCA_WORKTREE_CONTEXT`, `MINI_TERM_GITHUB_PROJECT_TASKS`, and
`MINI_TERM_GLOBAL_AGENT_ACTIVITY`. These gates switch presentation or ownership
without deleting the additive stable identity, workbench, terminal history, or
Agent records, so an older slice can be restored without corrupting newer state.

## Windows Installer

- Artifact: `dist/Mini-Term_1.2.2-orca-final-20260903_x64-setup.exe`.
- Size: 18,237,918 bytes.
- SHA-256: `fb49b7178be98c852cddbd397b7bb4c92c993f125daf0ced31bde52f547f9778`.
- Product version: `1.2.2-orca-final-20260903`; file version: `1.2.2.903`.
- The NSIS installer was extracted in Docker. All eight payload files matched the
  staging hashes exactly: the x64 GUI application, x64 terminal host, three x64
  sidecars, x64 ConPTY DLL, x64 OpenConsole, and ARM64 OpenConsole.
- Main PE resources are exactly IDs `3`, `14`, `16`, and `24`; all required Orca,
  terminal recovery, remote runtime, GitHub Tasks, and Agent feed markers are present.
- The installer is unsigned, as expected for this local release.

## Platform Limits

- The Windows executable was probed under Xvfb/Wine. Wine stopped before app
  initialization because that environment lacks Windows system DLLs
  `bcryptprimitives.dll` and `icuuc.dll`; no screenshot is claimed. Interactive
  installation, GPU rendering, and visual inspection still require a real Windows
  desktop.
- The repository-wide formatting baseline remains outside this task. Task-owned
  Rust formatting and every executable compile/test/package gate passed.
