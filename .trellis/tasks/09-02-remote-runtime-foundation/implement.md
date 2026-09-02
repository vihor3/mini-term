# Implementation Plan

1. Capture accepted SSH server-key SHA-256 fingerprints in `CachedSession` and
   assign immutable process-monotonic connection epochs.
2. Add `mt-ssh::runtime` protocol types, SFTP install-ID bootstrap, bounded
   heartbeat, tool probing, Git common-dir discovery, and stable identity
   derivation.
3. Expose one retrying runtime-snapshot service through the existing
   `remote_ssh` process runtime and exact-session eviction boundary.
4. Add project-scoped AppStore runtime state and generation/path/connection
   fences; probe remote projects before PTY hydration.
5. Reconcile authoritative remote bindings through `mt-layout`, deferring any
   changed binding while a project owns live PTYs.
6. Add unit/integration coverage for fingerprint/epoch rules, install-ID
   parsing and race decisions, bounded output parsing, Git/non-Git identity,
   stale generation rejection, and safe layout rebinding.
7. Update `mt-ssh`, workbench identity, and remote runtime specs.
8. Run Docker-only rustfmt, `mt-ssh`/layout/app tests, workspace check, Clippy,
   Windows MSVC checks, diff checks, then commit and archive.

## Validation Commands

```text
./scripts/docker-ci.sh run cargo test --locked -p mt-ssh
./scripts/docker-ci.sh run cargo test --locked -p mt-layout remote
./scripts/docker-ci.sh run cargo test --locked -p mt-app remote_runtime
./scripts/docker-ci.sh run cargo check --locked -p mt-app --tests
./scripts/docker-ci.sh run cargo clippy --locked --no-deps -p mt-ssh -p mt-layout -p mt-app --all-targets
cargo xwin check --locked --target x86_64-pc-windows-msvc -p mt-ssh -p mt-app
```

## Rollback Point

The archived cold-restore task ends at commits `c89250e` and its archive commit.
Set `MINI_TERM_REMOTE_RUNTIME=0` to retain the existing provisional SSH identity
and compatibility transport without removing newer persisted IDs.
