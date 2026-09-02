# Terminal Host Contract

## Scenario: Detach and warm-reattach a local terminal

### 1. Scope / Trigger

Use this contract when a local or WSL-backed terminal is created, attached,
written, resized, detached, closed, or restored after the GUI process exits.
SSH-project terminals remain on their compatibility transport until the remote
runtime owns the remote PTY.

`mt-terminal-host` owns the native `PtySession`. `mt-app` owns only a client
attachment and the terminal emulator. Dropping an attachment must not imply
killing the child process.

### 2. Process and API Boundary

```text
mt-app -> mt-terminal-host client -> current-user IPC
                                      |
                                      v
                           mt-terminal-host server -> mt-pty
```

The protocol is newline-delimited JSON with base64 byte payloads. Every
connection receives a version hello before one request. Long-lived `attach`
connections then receive ordered output and exit frames.

Required operations:

```text
create, attach, write, resize, arm_autofill, kill, detach,
list, status, shutdown_if_idle
```

### 3. Identity and Mutation Contracts

- `TerminalSessionId` identifies the logical terminal and survives GUI restart.
- The host creates `TerminalIncarnationId` only after it starts a new PTY.
- Every mutation is fenced by session ID plus expected incarnation ID.
- `attach` is attach-only. It cannot create a missing session or rotate an
  incarnation as a side effect.
- A stale incarnation must fail before write, resize, autofill, or kill reaches
  the current PTY.
- Host-returned identity is persisted and installed into `TerminalRoute` only
  after create or attach succeeds.
- `u32 pty_id` remains a process-local GUI attachment handle, never a persisted
  terminal identity.

### 4. Replay Contract

Each session serializes PTY output under one stream lock:

1. allocate a monotonically increasing sequence;
2. retain the exact bytes in a bounded replay buffer;
3. publish the same sequence and bytes to live attachments.

Attach validates `after_sequence`, snapshots retained chunks, subscribes to
live events while holding the same lock, and then releases it. Therefore the
replay-to-live handoff has no gap or duplicate. If the requested prefix has
already been evicted, return `ReplayGap`; never skip to the oldest retained
chunk silently.

The warm-reattach phase keeps at most 64 MiB of raw output per live session.
Disk snapshots and dead-process recovery belong to the cold-restore contract.

### 5. Lifecycle Contract

| Trigger | Hosted terminal | Compatibility terminal |
|---------|-----------------|------------------------|
| GUI/window/entity drop | Detach only | Drop legacy PTY |
| Explicit pane/tab close | Fenced kill | Kill legacy PTY |
| Project registration removal | Detach only | Drop legacy PTY |
| Worktree deletion | Explicitly kill first | Explicitly kill first |
| Host crash | Host PTY drops; next start is cold recovery | Not applicable |

`shutdown_if_idle` succeeds only when no live sessions remain. The host may
exit after its idle window only when it has no live sessions and no active
connections.

### 6. Endpoint and Startup Contract

- Unix uses an owner-only directory (`0700`) and socket (`0600`). Stale recovery
  requires a nonblocking endpoint lock, hello probe, and inode recheck before
  unlinking the socket.
- Windows uses a per-user named pipe whose DACL grants the current SID only.
- Concurrent auto-start attempts converge at endpoint ownership. A contender
  must report an already-running host and must not replace the live owner.
- `MINITERM_TERMINAL_HOST_ENDPOINT` overrides the endpoint for tests.
- `MINITERM_TERMINAL_HOST_BIN` overrides packaged-binary discovery.
- `MINI_TERM_TERMINAL_HOST=0` forces the compatibility backend.
- Protocol mismatch must not send any terminal mutation.

### 7. Application Recovery Matrix

| Attach result | Required application behavior |
|---------------|-------------------------------|
| Exact session and incarnation found | Quiet warm attach; preserve process and incarnation |
| Session missing or exited | Create a new incarnation and show a cold recovery notice |
| Replay gap | Explicitly end the unusable incarnation, create fresh, show replay notice |
| Incarnation mismatch | Fail closed; do not take over the host session |
| Protocol mismatch or existing-session conflict | Fail closed; do not mutate either session |
| Host unavailable for a fresh terminal | Use compatibility backend with visible notice |

Warm attach is true only when the returned child PID and incarnation belong to
the persisted live session. Merely reusing a session ID is not warm attach.

### 8. Packaging Contract

`mt-terminal-host[.exe]` is installed beside `mini-term` and the three existing
sidecars on every desktop release path. Windows upgrade must terminate the old
host before replacing files; uninstall must remove it. A package validation is
incomplete unless the extracted installer contains the host binary and its hash
matches the staged source.

### 9. Required Tests

- Protocol serde and binary payload round trips.
- Missing attach never creates.
- Detach and reattach preserve PID and incarnation.
- Detached output replays exactly once and live output continues in order.
- Stale write, resize, autofill, and kill all fail closed.
- Explicit kill removes the child; idle shutdown refuses while a child is live.
- Unix endpoint modes, stale socket recovery, and live-owner contention.
- Windows MSVC target compilation of current-SID named-pipe security code.
- Application routing keeps returned incarnation and fences old callbacks.
- Extracted Windows installer contains `mt-terminal-host.exe`.

### 10. Forbidden Patterns

- Do not let `TerminalPane::drop` call hosted `kill`.
- Do not create a fresh shell from inside `attach`.
- Do not accept a mutation by session ID alone.
- Do not present compatibility fallback or a new process as warm attach.
- Do not perform host IPC round trips synchronously on the GPUI input/render
  path; enqueue ordered commands on the client runtime.
- Do not merge terminal-host protocol state with the SSH CLI daemon.
