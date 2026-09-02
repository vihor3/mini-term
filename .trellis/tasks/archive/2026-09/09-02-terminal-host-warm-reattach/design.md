# Technical Design

## Boundary

This child moves live local PTY ownership out of `mt-app`. It does not add disk checkpoints, cold restore, provider resume, or a remote host daemon. Existing SSH-project panes stay on the in-process compatibility path until the remote runtime phase.

The behavior gap is ownership: today `TerminalPane` owns `PtySession`, and `Drop` kills the child. The new owner is a per-user `mt-terminal-host` process. `TerminalPane` owns an attachment that detaches on drop and kills only through an explicit close command.

## Crate And Process Shape

Add `crates/mt-terminal-host` as a workspace library plus binary:

```text
mt-app
  -> mt-terminal-host client API
       -> local IPC
mt-terminal-host binary
  -> mt-terminal-host server
       -> mt-pty::PtySession
```

The library owns protocol types, endpoint helpers, client connection logic, server state, and lifecycle tests. The binary is a thin `serve` entrypoint. `mt-pty` remains the only layer that opens and drives a native PTY.

## Endpoint And Startup

- Unix: a Unix domain socket under `$XDG_RUNTIME_DIR/mini-term` when available, otherwise the existing per-user application data directory. The parent directory and socket are owner-only; stale recovery uses a lock plus liveness probe and inode recheck before unlink.
- Windows: a per-user named pipe with a protected DACL that grants the current SID only.
- `MINITERM_TERMINAL_HOST_ENDPOINT` overrides the endpoint for tests and diagnostics.
- `MINITERM_TERMINAL_HOST_BIN` overrides binary discovery. Production resolves `mt-terminal-host[.exe]` beside `mini-term`.
- Clients first probe the endpoint, then detached-spawn the binary and retry with bounded backoff. Endpoint binding is the single-winner convergence point.
- The host exits after an idle window only when it owns no live sessions. A client disconnect never ends a session.

## Protocol

Every connection starts with a host hello carrying binary version, protocol version, pid, and live-session count. Requests are newline-delimited JSON; output bytes use base64.

```text
create(session, expected_absent, worktree, spawn_spec)
attach(session, expected_incarnation, after_sequence)
write(session, expected_incarnation, bytes)
resize(session, expected_incarnation, rows, cols)
arm_autofill(session, expected_incarnation, password, disarm_on_input)
kill(session, expected_incarnation)
list()
status()
shutdown_if_idle()
```

`create` is atomic. The server rejects an existing session, generates a new `TerminalIncarnationId`, overwrites the internal incarnation environment field, starts the PTY, and only then publishes the session.

`attach` is attach-only. Missing session, incarnation mismatch, exited session, and replay gap are distinct errors and never imply create.

## Hosted Session State

Each live session stores:

```text
TerminalSessionId
TerminalIncarnationId
WorktreeId
child process id
PtySession
last terminal size
next output sequence
retained output chunks
subscriber senders
exit state/code
```

PTY output is serialized under one session stream lock:

1. allocate the next sequence;
2. append the raw chunk to retained replay;
3. publish the same `(sequence, bytes)` to subscribers.

Attach takes that same lock, validates the requested sequence against the retained prefix, copies replay chunks, registers the live subscriber, and then releases the lock. This makes replay-to-live handoff gap-free.

Phase 3 keeps a bounded in-memory replay window. Once old chunks are evicted, an attach requesting an earlier sequence returns `replay_gap`; it never skips forward. Phase 4 replaces restart-from-zero replay with snapshots/checkpoints.

## Client Session

The client exposes a `HostedTerminalSession` with `write`, `resize_if_changed`, `arm_ssh_autofill`, `kill`, descriptor access, and a cancellation-backed output reader. Dropping it only closes the attach stream. The server session and child remain alive.

The output reader invokes the existing `TerminalPane` callbacks, so `TerminalEmulator`, AI perception, Git output observation, and GPUI wakeup behavior remain in the GUI. No VT parser is duplicated in this phase.

## Application Integration

`TerminalPane` replaces `Option<PtySession>` with a transport enum:

```text
Hosted(HostedTerminalSession)
Legacy(PtySession)
```

The transport presents the current write/resize/autofill/kill surface. Its drop semantics differ intentionally: hosted detaches, legacy drops and kills.

`AppStore::hydrate_project` carries the persisted incarnation into startup. For a local/WSL pane it first calls attach-only. On success it keeps the persisted incarnation. On missing/gap/host failure it runs an explicit fresh-or-legacy policy, rotates incarnation, persists the result, and exposes a recovery notice. New panes call create directly.

The host generates the incarnation. `AppStore` inserts the returned value into `PaneState`, `TerminalRoute`, and persistence only after the create/attach result is known. Old route events remain fenced by the expected route already captured in the pane subscription.

SSH-project panes retain the existing `PtySession` path in this child. Their remote authority moves in the remote runtime phase.

## Exit And Close Semantics

- GUI/window/application teardown: `HostedTerminalSession` is dropped, which cancels only its subscriber.
- Terminal pane/tab close: existing store disposal calls `TerminalPane::shutdown`, which issues `kill(session, expected_incarnation)` before dropping the attachment.
- Project removal: hosted panes detach; they are not killed implicitly. A later diagnostics UI can offer explicit cleanup.
- Host process exit/crash: native `PtySession` drop kills children. Layout identity remains intact for the next explicit recovery decision.

## Compatibility And Rollback

`MINI_TERM_TERMINAL_HOST=0` disables hosted creation and attach. Connection, handshake, or spawn failures fall back to the legacy in-process backend for fresh terminals. Restored terminals are marked cold-only so fallback is not misrepresented as warm attach.

No persisted field is removed. The stable identity schema from the previous child remains readable if this code is reverted.

## Packaging

- Build the new binary in desktop release jobs.
- Stage it beside `mini-term`, `miniterm-hook`, `mt-ssh-cli`, and `mt-ssh-mcp`.
- Add it to NSIS install, upgrade kill list, uninstall, portable bundle, and package validation.
- Windows cross compilation continues in Docker and uses the same target/toolchain as the main application.

## Failure Handling

- Protocol/version mismatch never sends terminal mutations.
- Unknown session and stale incarnation are typed failures.
- Replay subscriber lag becomes a sequence-gap failure and forces a fresh attach request.
- Malformed frames close only the offending connection.
- Host bind/runtime failures are visible and do not mutate layout rows.
- Sensitive SSH autofill data is accepted only over the current-user endpoint and is never logged or persisted by the host.
