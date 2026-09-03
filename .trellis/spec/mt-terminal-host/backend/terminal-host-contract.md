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
connections then receive ordered output and exit frames. One JSONL frame is
bounded to 48 MiB, one decoded terminal write to 1 MiB, and each attachment's
command queue to 32 entries.

Required operations:

```text
create, attach, restore, write, resize, arm_autofill, kill, detach,
list, status, shutdown_if_idle
```

### 3. Identity and Mutation Contracts

- `TerminalSessionId` identifies the logical terminal and survives GUI restart.
- The host creates `TerminalIncarnationId` only after it starts a new PTY.
- Every mutation is fenced by session ID plus expected incarnation ID.
- `attach` is attach-only. It cannot create a missing session or rotate an
  incarnation as a side effect.
- `restore` is explicit. It validates the persisted `WorktreeId` and previous
  incarnation, reconstructs history, and only then starts a new PTY with a new
  host-generated incarnation while preserving the logical terminal session.
  Explicit close may cancel an in-flight restore; any spawned-but-uncommitted
  replacement is killed, history-invalidated, and purged before the restore
  returns failure.
- An old incarnation cannot attach or restore over the new incarnation.
- A stale incarnation must fail before write, resize, autofill, or kill reaches
  the current PTY.
- Host-returned identity is persisted and installed into `TerminalRoute` only
  after create or attach succeeds. Restore fencing compares only stable process
  fields: session, incarnation, worktree, and process ID. Dynamic sequence
  bounds, size, recovery availability, and WSL presentation do not make the same
  process a different incarnation.
- `u32 pty_id` remains a process-local GUI attachment handle, never a persisted
  terminal identity.

### 4. Replay Contract

Each session serializes PTY output under one stream lock:

1. allocate a monotonically increasing sequence;
2. retain the exact bytes in a bounded replay buffer;
3. publish the same sequence and bytes to live attachments;
4. record the same bytes in durable history before the output callback completes.

Child exit is finalized only after the PTY output pump reports a clean drain and
all already-started output callbacks complete. The exit/error frame is therefore
strictly after accepted output. Reader failure or the bounded drain timeout
terminates the incarnation as `RecoveryUnavailable`, writes an invalidation
marker, and forbids future replay/restore from incomplete history.

Attach validates `after_sequence`, snapshots retained chunks, subscribes to
live events while holding the same lock, and then releases it. Therefore the
replay-to-live handoff has no gap or duplicate. If the requested prefix has
already been evicted, return `ReplayGap`; never skip to the oldest retained
chunk silently.

Warm reattach keeps at most 64 MiB of raw output per live session. Durable
history is stored per logical session as:

```text
meta.json | checkpoint.json | output.log
```

`meta.json` binds session, worktree, and incarnation. `checkpoint.json` is an
atomically replaced, checksummed zstd terminal snapshot. `output.log` is a
binary framed stream with magic, version, kind, generation, monotonic sequence,
payload length, payload, and CRC32. The log is capped at 8 MiB; rotation writes
a checkpoint through the latest sequence before truncation.

Recovery may truncate only a torn final frame after a valid frame prefix.
Short garbage, checksum failures, sequence gaps, invalid versions, wrong
generations, malformed complete frames, or oversized payloads fail with
`RecoveryUnavailable`. File reads enforce the byte limit on bytes actually read,
not metadata observed before the read. Invalidation acquires the history state
fence before purge so late writers cannot recreate deleted files. Launch
arguments, user environment values, autofill passwords, and other secrets are
never persisted.

### 5. Lifecycle Contract

| Trigger | Hosted terminal | Compatibility terminal |
|---------|-----------------|------------------------|
| GUI/window/entity drop | Detach only | Drop legacy PTY |
| Explicit pane/tab close | Fenced close, invalidate, quiesce, and purge even after natural exit | Kill legacy PTY |
| Project registration removal | Detach only | Drop legacy PTY |
| Worktree deletion | Explicitly kill first | Explicitly kill first |
| Host crash | Host PTY drops; explicit restore uses durable history | Not applicable |

`shutdown_if_idle` succeeds only when no live sessions remain. The host may
exit after its idle window only when it has no live sessions and no active
connections.

### 6. Endpoint and Startup Contract

- Unix uses an owner-only directory (`0700`) and socket (`0600`). Stale recovery
  requires a nonblocking endpoint lock, hello probe, and inode recheck before
  unlinking the socket.
- Windows uses a per-user named pipe whose DACL grants the current SID only.
- Concurrent auto-start attempts converge at endpoint ownership. A contender
  must report an already-running host and must not replace the live owner. Every
  spawned host child has a waiter; a startup failure/cancellation kills and
  reaps an uncommitted child.
- Client connect, hello, request write, and response read share one absolute
  10-second RPC deadline. Server request reads and frame writes are separately
  bounded. Queue overflow, oversized input, stalled peers, or transport failure
  permanently disconnect that attachment; the application clears its writable
  transport before presenting the read-only view.
- `MINITERM_TERMINAL_HOST_ENDPOINT` overrides the endpoint for tests.
- `MINITERM_TERMINAL_HOST_BIN` overrides packaged-binary discovery.
- `MINI_TERM_TERMINAL_HOST=0` forces the compatibility backend.
- Protocol mismatch must not send any terminal mutation.

### 7. Application Recovery Matrix

| Attach result | Required application behavior |
|---------------|-------------------------------|
| Exact session and incarnation found | Quiet warm attach; preserve process and incarnation |
| Session missing or exited with valid history | Explicit restore; apply snapshot; rotate incarnation |
| Replay gap with valid history | Seal and end the unusable incarnation, then explicitly restore |
| PTY output drain fails or times out | Emit recovery-unavailable after accepted output; invalidate history and never replay it |
| Explicit close races restore | Cancel restore, retire any uncommitted replacement, quiesce history, and purge |
| History missing or corrupt | Start clean with a visible recovery-unavailable notice |
| Incarnation mismatch | Fail closed; do not take over the host session |
| Protocol mismatch or existing-session conflict | Fail closed; do not mutate either session |
| Host unavailable for a fresh terminal | Use compatibility backend with visible notice |

Warm attach is true only when the returned child PID and incarnation belong to
the persisted live session. Merely reusing a session ID is not warm attach.
The application recovery state is one of `Fresh`, `Reattached`,
`RestoredHistory`, `Compatibility`, or `Unavailable`. Only `Reattached`
suppresses provider resume. A restored snapshot is applied before the new
incarnation is attached, and provider resume may run only after that restore.

### 8. Packaging Contract

General build/stage ordering, locked-graph, PE, and extracted-payload rules are
normative in `mt-app/backend/release-staging-contract.md`.

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
- Natural exit delivers final output before exactly one exit frame; explicit
  close afterward still purges registry and recovery history.
- Drain failure/timeout invalidates recovery, and attach cannot land between
  lifecycle validation and exit publication.
- Stale write, resize, autofill, and kill all fail closed.
- Explicit kill removes the child; idle shutdown refuses while a child is live.
- Unix endpoint modes, stale socket recovery, and live-owner contention.
- Windows MSVC target compilation of current-SID named-pipe security code.
- Application routing keeps returned incarnation and fences old callbacks.
- Snapshot round trips preserve grid, cursor, source size, scrollback, wide
  cells, colors, and an incomplete parser sequence.
- Host restart restores valid history into a new incarnation; stale restore,
  wrong worktree, corruption, short garbage tails, sequence gaps, and wrong
  generations fail closed.
- Restore/close races retire uncommitted replacements and prevent late history
  recreation. Stable descriptor comparison ignores dynamic output/size fields.
- JSONL, write, command-queue, RPC/read/write, history-file, and PTY drain bounds
  have oversized/stalled-peer regressions.
- Log rotation stays within the bound, explicit kill removes history, and
  persisted bytes exclude launch secrets.
- Extracted Windows installer contains `mt-terminal-host.exe`.

### 10. Forbidden Patterns

- Do not let `TerminalPane::drop` call hosted `kill`.
- Do not create a fresh shell from inside `attach`.
- Do not accept a mutation by session ID alone.
- Do not publish exit before the output pump and accepted output callbacks have
  finalized, and do not retain recovery history after an incomplete drain.
- Do not present compatibility fallback or a new process as warm attach.
- Do not apply new-incarnation replay before installing the restored snapshot;
  that would overwrite newer output with older terminal state.
- Do not perform host IPC round trips synchronously on the GPUI input/render
  path; enqueue ordered commands on the client runtime.
- Do not merge terminal-host protocol state with the SSH CLI daemon.

### 11. Wrong vs Correct

#### Wrong

```text
attach missing session -> silently create shell -> label reattached
```

This conflates live-process continuity with visual recovery and bypasses the
previous-incarnation/worktree validation boundary.

#### Correct

```text
attach missing session -> explicit restore(old incarnation, worktree)
                       -> validate history -> new incarnation + snapshot
                       -> apply snapshot -> attach new output
```

Warm attach remains pure, while cold restore is observable and fully fenced.
