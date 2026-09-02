# Implementation Plan

## 1. Protocol And Endpoint

- Add `mt-terminal-host` with serde protocol types and explicit error codes.
- Implement per-user Unix socket and Windows named-pipe helpers, liveness probing, stale endpoint recovery, and version handshake.
- Add protocol round-trip and endpoint ownership tests.

## 2. Host Session Runtime

- Add the server registry keyed by `TerminalSessionId`.
- Implement atomic create, host-generated incarnation injection, output sequencing, bounded replay, subscriber handoff, write, resize, autofill, kill, list, and idle shutdown.
- Extend `mt-pty` only where the host needs stable child pid and launch metadata.
- Add real-process tests for create, detach, same-PID attach, background output replay, explicit kill, and host crash containment.

## 3. Client Attachment

- Implement endpoint probe, detached auto-start, handshake, one-shot commands, and long-lived attach stream.
- Expose a synchronous facade suitable for current GPUI call sites while all socket I/O and output streaming run off the UI thread.
- Ensure `HostedTerminalSession::drop` detaches without kill and explicit `kill` is incarnation-fenced.

## 4. Application Transport Boundary

- Replace direct `PtySession` ownership in `TerminalPane` with hosted/legacy transports.
- Keep existing emulator, AI perception, Git observation, redraw, keyboard, autofill, and exit event behavior behind the transport.
- Add a visible recovery/backend notice only for legacy, missing, or replay-gap outcomes.

## 5. Restore And Routing

- Carry persisted incarnation through `hydrate_project`.
- Attach-only before any fresh spawn for eligible local/WSL panes.
- Update `PaneState`, `TerminalRoute`, and persistence from the host-returned incarnation.
- Keep SSH-project panes on the compatibility backend and preserve old route fencing.

## 6. Close And Teardown

- Keep existing explicit pane/tab close on the kill path.
- Make entity/application drop detach hosted sessions.
- Ensure project removal does not implicitly kill hosted children.
- Add store/pane tests for close versus drop behavior.

## 7. Packaging

- Build and stage `mt-terminal-host` in sidecar/release scripts.
- Include it in NSIS install/uninstall/upgrade process termination and all portable artifacts.
- Update Windows package inspection expectations.

## 8. Docker Validation

Run Rust commands only through Docker:

```bash
./scripts/docker-ci.sh test -p mt-pty
./scripts/docker-ci.sh test -p mt-terminal-host
./scripts/docker-ci.sh test -p mt-app terminal
./scripts/docker-ci.sh check
./scripts/docker-ci.sh run cargo clippy --no-deps -p mt-pty -p mt-terminal-host -p mt-app --all-targets
```

Run the real lifecycle integration test in Docker:

```text
spawn shell -> print pid/marker -> detach -> print while detached -> attach ->
assert same pid/incarnation and ordered markers -> stale mutation rejection -> kill
```

Then run workspace tests and changed-line rustfmt/Clippy gates. Record the existing unrelated DnD failure separately if it remains the only broad-suite failure.

## 9. Windows Package

- Rebuild the main executable, existing sidecars, and `mt-terminal-host` in Docker.
- Produce an installer with a new warm-reattach version marker.
- Extract and verify all payload hashes, PE architectures, identity markers, and the added host binary.
- Remove task-created containers and caches while preserving the installer.

## Rollback Points

- Set `MINI_TERM_TERMINAL_HOST=0` to force legacy in-process PTY creation.
- On protocol/startup failure, use legacy for fresh terminals and retain persisted stable IDs.
- Reverting application integration leaves additive identity fields and layouts readable.

## High-Risk Files

- `crates/mt-terminal-host/`
- `crates/mt-pty/src/lib.rs`
- `crates/mt-app/src/pane.rs`
- `crates/mt-app/src/store/panes.rs`
- `crates/mt-app/src/store/projects.rs`
- `scripts/stage-sidecars.mjs`
- `scripts/windows-installer.nsi`
- release packaging workflows

## Completion Review

- Confirm GUI drop never sends kill for hosted sessions.
- Confirm attach-only cannot create and stale incarnation cannot mutate.
- Confirm replay/live handoff is sequence-complete.
- Confirm no result is described as warm attach unless pid and incarnation match.
- Confirm no disk snapshot/cold restore or remote authority claim leaked into this child.
