# Validation Record

Date: 2026-09-02

## Scope

- Stable host, repository, worktree, tab, pane, terminal session, and terminal incarnation identities.
- Additive layout persistence and migration with legacy project-layout compatibility.
- Worktree-scoped document, preview, search, and deferred focus/close routing.
- Docker-only Rust validation and Docker-only Windows x64 packaging.
- Detached PTY reattachment and authoritative remote-agent status remain outside this child task.

## Focused Docker Checks

All Rust commands ran inside the repository Docker harness or the dedicated Windows build container.

- `mt-layout --lib`: 20 passed.
- `mt-identity --lib`: 5 passed.
- `mt-project worktree`: 24 passed.
- `mt-config --lib`: 64 passed.
- `mt-app persist`: 14 passed.
- `mt-app tree`: 108 passed.
- `mt-app store::identity`: 2 passed.
- `mt-app search_modal`: 7 passed.
- `mt-app workbench_area`: 12 passed.
- `scripts/docker-ci.sh check`: passed.
- Sidecar check: passed; only the existing `sanitize_tag` dead-code warning remained.
- Clippy for `mt-identity`, `mt-config`, `mt-layout`, `mt-project`, and `mt-app` completed; no warning remained on task-owned changed lines.
- Explicit Docker rustfmt checks passed for the task-owned identity/layout/workbench files.
- `git diff --check`: passed.

## Broad Docker Checks

- `cargo test -p mt-app --no-fail-fast`: 810 passed, 1 failed.
- `cargo test --workspace --all-targets --no-fail-fast`: all other package suites passed.
- The only failure in both broad runs was the pre-existing unrelated test `dnd::tests::重复判定走路径归一`.
- Four terminal smoke tests passed.

## Windows Package

- Main target: `x86_64-pc-windows-msvc`.
- Main application and all three sidecars were built with `cargo-xwin` in Docker.
- NSIS package version: `1.2.2-stable-identity-20260902`.
- Numeric file version: `1.2.2.902`.
- Artifact: `dist/Mini-Term_1.2.2-stable-identity-20260902_x64-setup.exe`.
- Size: 17,067,728 bytes.
- SHA-256: `a1a482d6fb51499cc212656230c9ee8a8c05894057c0fc5ea998f034ddc246c0`.
- The installer is unsigned.

## Package Inspection

The NSIS installer was extracted in Docker and all seven runtime payloads matched the staging files byte-for-byte.

- `mini-term.exe`: x64 (`0x8664`).
- `miniterm-hook.exe`: x64 (`0x8664`).
- `mt-ssh-cli.exe`: x64 (`0x8664`).
- `mt-ssh-mcp.exe`: x64 (`0x8664`).
- `portable-conpty/conpty.dll`: x64 (`0x8664`).
- `portable-conpty/x64/OpenConsole.exe`: x64 (`0x8664`).
- `portable-conpty/arm64/OpenConsole.exe`: ARM64 (`0xaa64`), as required by the portable ConPTY package layout.
- Main executable resource IDs: `3`, `14`, `16`, and `24`.
- Main executable version strings matched the package marker.
- The main executable contains `MINITERM_WORKTREE_ID`, `MINITERM_TERMINAL_SESSION_ID`, `MINITERM_TERMINAL_INCARNATION_ID`, and `worktree-v1:` markers.
- Official portable ConPTY SHA-256 values and PE machines were verified before packaging.

## Residual Validation Limits

- The unsigned installer was structurally inspected but not interactively installed on a physical Windows host in this environment.
- Same-process terminal runtime continuity remains compatibility behavior; this child does not claim detached host ownership or warm reattachment after process exit.
- Remote worktree identities created without remote authority remain provisional and are not claimed as authoritative agent identity.
