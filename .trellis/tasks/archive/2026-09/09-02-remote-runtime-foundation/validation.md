# Validation

Date: 2026-09-02

## Delivered

- Captured the accepted SSH server-key SHA-256 fingerprint only after known-host
  acceptance and exposed it only from an authenticated pooled session.
- Added immutable process-monotonic connection epochs, exact pool-winner checks,
  and a monotonic application registry that rejects stale epoch completions.
- Added versioned remote install-ID bootstrap with exclusive creation,
  canonical parsing, symlink/non-regular rejection, and permission hardening.
- Added bounded heartbeat, tool inventory, canonical worktree/Git discovery,
  and stable remote execution-host/repository/worktree derivation.
- Added project-scoped generation/path/connection fences plus transactional
  authoritative rebinding before terminal hydration. Live PTYs or open
  documents leave a visible deferred-rebind state.
- Preserved the provisional compatibility path behind
  `MINI_TERM_REMOTE_RUNTIME=0` and on unavailable runtime probes.

## Docker Verification

- Targeted Rust formatting and `rustfmt --check` for every task-owned Rust file:
  passed in `mini-term-ci:rust-1.95`.
- `cargo test --locked -p mt-ssh`: 56 tests passed.
- `cargo test --locked -p mt-project worktree::identity::tests`: 6 passed.
- `cargo test --locked -p mt-layout rebind`: 1 passed.
- Remote runtime, retry, epoch registry, invalidation, and authoritative rebind
  `mt-app` tests: 8 focused tests passed.
- `cargo check --locked --workspace --all-targets`: passed with only existing
  notification/tray dead-code warnings.
- Affected-package `cargo clippy --locked --no-deps ... --all-targets`: passed;
  reported warnings are in pre-existing untouched code.
- `cargo xwin check --locked --target x86_64-pc-windows-msvc` for `mt-ssh`,
  `mt-project`, `mt-layout`, and `mt-app`: passed.
- `git diff --check`: passed.

## Identity And Safety Assertions

- The same verified host key and remote install ID derive stable host/repository/
  worktree IDs while reconnects receive higher epochs.
- A replacement pool session or differing current epoch rejects the older
  runtime result before binding reconciliation.
- Exact transport eviction cannot remove or clear a newer session/epoch winner.
- Malformed, oversized, symlinked, or non-regular install state is preserved and
  rejected rather than overwritten.
- Runtime state and diagnostics contain no password, private-key bytes, prompts,
  environment values, or unbounded remote output.

## Host Hygiene

- Repository `target/`, `~/.cargo`, and `~/.rustup` are absent.
- All Rust formatting, compilation, linting, tests, and Windows checks used
  Docker-mounted caches outside the repository.

## Residual Limits

- SFTP v3 cannot provide descriptor-relative `openat`/`O_NOFOLLOW`; documented
  same-account path replacement limits remain.
- Physical-host SSH bootstrap races and interactive Windows UI behavior remain
  part of the parent task's final integration/installer smoke pass.
