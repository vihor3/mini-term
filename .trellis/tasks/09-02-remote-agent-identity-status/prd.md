# Remote Agent Identity And Status

## Goal

Give mini-term a stable, execution-host-scoped model for identifying which AI
agent is running in each owned terminal and for tracking its activity without
confusing reconnects, reused PTY handles, historical session files, or stale
remote observations.

The result must preserve the existing local Hook behavior and four-state pane
projection while exposing richer run state for the later worktree sidebar and
global Agents feed.

## Background

- Stable execution-host, worktree, tab, pane, terminal-session, and terminal-
  incarnation identities already exist.
- Local Claude/Codex/Grok Hooks are the highest-confidence live signal. Input
  and PTY-output tracking is the compatibility fallback for providers or hosts
  without Hooks.
- SSH terminals currently spawn a local `ssh` process. Their remote shell does
  not receive the complete stable terminal route, so local process inspection
  cannot authoritatively identify the remote provider.
- Authenticated SSH sessions already expose stable remote runtime identity and
  a monotonic connection epoch.
- Historical provider session files are useful for browsing and resume, but are
  not evidence that a process is currently alive.

## Requirements

1. Add opaque canonical `AgentRunId` and `AgentEventId` identities. A run ID is
   allocated when mini-term first binds a live provider process/session and is
   never substituted with provider session IDs, PIDs, PTY IDs, or display
   names.
2. Define a host-neutral agent runtime model with normalized provider,
   activity, connectivity, confirmation, evidence, provider session identity,
   exact terminal route, optional remote process identity, connection epoch,
   and monotonic observation sequence.
3. Reconcile observations only when execution host, worktree, tab, pane,
   terminal session, and terminal incarnation all match. Reject old connection
   epochs, non-increasing sequences, ended-run replay, and observations for a
   replaced route.
4. Preserve local Hook semantics, completion/attention notifications, provider
   resume identity, mobile status strings, and the existing
   `idle/ai-idle/ai-working/error` projection. Enriching status must not create
   a second competing pane-state source.
5. Capture the exact stable terminal route when local AI events enter the
   background channel. A reused process-local `pty_id` must not deliver an old
   event to a new terminal incarnation.
6. Before an SSH compatibility terminal starts, allocate its incarnation and
   inject only public mini-term route identifiers plus protocol version into
   the remote login shell. Passwords, keys, tokens, arbitrary user variables,
   and local-only attachment handles must never be forwarded.
7. Add an authenticated, bounded SSH process inventory for Linux `/proc`. It
   may report only processes that inherited every exact route identifier for
   the target terminal. Output is limited to normalized provider, PID, and
   process start ticks; full command lines and environment values are never
   returned, persisted, or logged.
8. Recognize Claude, Codex, OpenCode, Pi, and Grok using a strict normalized
   provider vocabulary. Multiple matching processes may be returned and must
   remain independently identifiable by PID plus start ticks.
9. Poll only SSH terminals whose authoritative remote runtime identity matches
   the terminal route. Every completion is fenced by request generation,
   current route, connection configuration, and exact connection epoch.
10. Treat remote process presence as process-liveness evidence, not a provider
    Hook. Recent PTY output may project a live process as working; a quiet live
    process is waiting/quiet. Without Hook evidence mini-term must not claim a
    semantic blocked or successful-done state.
11. A failed or unsupported probe changes connectivity/capability only. It must
    not mark a previously observed run done or exited. A successful supported
    empty inventory may mark previously process-attested runs exited.
12. History/session-file scanning must never update live runtime activity or
    connectivity. It remains a separate source for the later Sessions UI.
13. Exact environment value `0` for `MINI_TERM_REMOTE_AGENT_STATUS` disables
    remote route injection, inventory polling, and remote process reconciliation
    while preserving existing local Hook/input-output behavior.
14. Run all Rust formatting, compilation, tests, linting, and Windows MSVC
    validation in Docker. Do not create host Cargo/Rust state or a repository-
    local `target` directory.

## Acceptance Criteria

- [ ] Agent identity parsing/serde accepts canonical UUID-v4 values and rejects
      wrong prefixes, versions, variants, or noncanonical forms.
- [ ] Pure reconciliation tests reject wrong routes, old incarnations, old
      epochs, duplicate/out-of-order sequences, and ended-run replay.
- [ ] A Hook or process observation creates one stable run; later stronger
      evidence upgrades that run instead of producing a duplicate.
- [ ] Local queued events carry their originating stable route and cannot update
      a new terminal that reused the same `pty_id`.
- [ ] SSH remote shell argv contains the exact execution-host/worktree/tab/pane/
      terminal-session/incarnation route with shell-safe quoting, and the pane
      installs the same preallocated incarnation.
- [ ] The remote parser accepts bounded valid inventories, recognizes all five
      providers, and rejects malformed, duplicate, oversized, or schema-
      incomplete output.
- [ ] The Linux probe only reports exact-route descendants and never emits raw
      environment or command-line text; unsupported hosts return a capability
      result rather than a false empty inventory.
- [ ] Reconnect tests prove that an old epoch result cannot overwrite a newer
      connection, and disconnect does not rewrite the last known activity.
- [ ] Existing local Hook, attention, completion, session identity, remote
      terminal, SFTP, and runtime-foundation tests continue to pass.
- [ ] Docker workspace checks, affected-package tests/Clippy, and Windows MSVC
      checks pass while host Rust state remains absent.

## Out Of Scope

- Installing or rewriting provider Hook configuration on a remote host.
- Claiming semantic `blocked`, `done`, or provider-specific tool state from
  process inspection alone.
- Tracking AI processes outside mini-term-owned terminal routes.
- Treating session-history files as live status.
- Persisting or displaying the later global Agents overlay or right-sidebar
  Sessions UI in this child.
- Supporting non-Linux remote process inventory beyond explicit capability
  fallback in this child.
