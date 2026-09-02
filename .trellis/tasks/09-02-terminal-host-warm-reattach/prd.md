# Terminal Host Warm Reattach

## Goal

Move local PTY ownership into a dedicated per-user terminal host so closing the GUI detaches without killing sessions and reopening can reattach to the same live process and incarnation.

## Requirements

1. Add an independent `mt-terminal-host` process and versioned local RPC. It must not share process lifetime or protocol state with the SSH CLI daemon.
2. Keep `TerminalSessionId` stable across GUI restarts and let the host generate each `TerminalIncarnationId` when it actually spawns a PTY.
3. Support hello, create, attach-only, list, write, resize, arm-autofill, kill, detach, status, and idle shutdown operations.
4. Fence every mutating operation by `TerminalSessionId + expected TerminalIncarnationId`; stale writes, resizes, autofill changes, and kills must fail closed.
5. Closing or dropping the GUI-side pane must detach from a hosted terminal. Explicit terminal close must kill the matching hosted session.
6. A warm attach must return the same child process id and incarnation, replay retained output in sequence order, and continue with live output without a gap or duplicate.
7. If retained output no longer contains the requested sequence, attach must report a replay gap instead of silently presenting incomplete history.
8. The local endpoint must be isolated to the current user, converge concurrent auto-start attempts to one host, and recover stale Unix socket files without deleting a live endpoint.
9. The application must retain a rollback path. When `MINI_TERM_TERMINAL_HOST=0` or the host cannot be reached, new terminals use the existing in-process PTY and are visibly classified as legacy/cold-only.
10. Local and WSL-backed panes may use the local host. Existing SSH-project panes remain on the compatibility path until the authenticated remote runtime owns remote PTYs.
11. Package `mt-terminal-host` beside `mini-term` and the existing sidecars on every supported desktop release path.

## Acceptance Criteria

- [ ] Creating a hosted terminal returns a host-generated incarnation and a live child process id.
- [ ] Detaching the first client leaves the child alive; a second client attaches to the same session, incarnation, and process id.
- [ ] Output produced while no GUI client is attached is replayed exactly once and live output continues in order.
- [ ] Attach-only for a missing session never creates a new shell.
- [ ] Write, resize, autofill, and kill requests carrying an old incarnation are rejected and do not affect the current session.
- [ ] Explicit terminal close kills the hosted child; application/window teardown only detaches.
- [ ] Concurrent host startup produces one active endpoint owner, with version/protocol mismatch handled deterministically.
- [ ] Host failure or replay-gap failure leaves persisted layout data readable and falls back to an explicitly cold-only/legacy state.
- [ ] Docker tests cover protocol serde, endpoint security/recovery, lifecycle, same-PID reattach, replay ordering, stale-incarnation rejection, and application routing.
- [ ] Docker packaging checks contain `mt-terminal-host` in the Windows installer payload.

## Notes

- This child does not persist terminal snapshots or revive a dead process. Those belong to `terminal-snapshot-cold-restore`.
- This child does not claim authoritative remote device or Agent identity. Those belong to the remote runtime and remote Agent children.
- Warm attach success should be visually quiet. Persistent UI is only required for legacy fallback, replay gaps, or missing sessions.
