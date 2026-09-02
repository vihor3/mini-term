# Remote Runtime Foundation

## Goal

Establish authenticated remote runtime identity, multiplexed transport, inventory, reconnect epochs, and authoritative worktree reconciliation for SSH execution hosts.

## Requirements

1. Extend the existing `mt-ssh` pooled `russh` session instead of creating a
   second SSH implementation. The verified server-key SHA-256 fingerprint must
   be captured by the connection handler and exposed only after authentication.
2. Assign every newly authenticated pooled session a process-monotonic
   `ConnectionEpoch`. Reusing the same healthy session preserves the epoch;
   reconnecting creates a higher epoch. Connection IDs and display names are
   never execution-host identity.
3. Bootstrap a versioned remote runtime identity in the authenticated user's
   canonical home. Persist one canonical `HostInstallId` with exclusive-create
   race handling; reject malformed/symlinked state rather than overwriting it.
4. Derive `ExecutionHostId` from verified host-key fingerprint plus remote
   install ID. A host-key change continues to fail at known-host verification;
   an install-ID change produces a different host identity and must not silently
   take over an existing live workbench.
5. Provide a host-neutral runtime snapshot containing protocol version,
   execution-host identity, connection epoch, canonical home/worktree path,
   canonical Git common dir when present, stable repo/worktree IDs, and bounded
   tool capabilities. All remote output is size bounded and strictly parsed.
6. Use independent SFTP/exec channels on the pooled authenticated SSH session
   for bootstrap, heartbeat, and inventory. Timeout/cancellation closes only the
   affected channel; transport failure evicts only the exact stale session and
   retries at most once.
7. Add AppStore runtime state with project-scoped request generations. Late
   results from a prior path, connection configuration, connection epoch, or
   project lifetime cannot update current state or bindings.
8. Upgrade provisional SSH project bindings transactionally through the
   existing layout reconciliation path. Activate a changed authoritative
   `WorktreeId` only before PTY hydration; if live panes already exist, defer
   rebinding instead of retagging them.
9. Preserve current SFTP, SSH terminal, session scanning, CLI daemon, and
   compatibility identities. `MINI_TERM_REMOTE_RUNTIME=0` disables probing and
   keeps the provisional path as the rollback control.
10. Never persist or log SSH passwords, private-key contents, command output
    beyond bounded parsed inventory, environment variables, or prompts.
11. Run formatting, compilation, tests, Clippy, and Windows MSVC checks only in
    Docker; do not create host Rust state or a repository-local target.

## Acceptance Criteria

- [ ] A verified host key produces a canonical SHA-256 fingerprint, and a
      mismatch still fails before runtime bootstrap or inventory runs.
- [ ] Concurrent first bootstrap converges on one valid remote install ID;
      malformed or symlinked state fails closed without replacement.
- [ ] The same host key and install ID derive the same `ExecutionHostId` across
      reconnects while the connection epoch increases.
- [ ] Healthy pooled reuse keeps one epoch and supports bounded heartbeat,
      SFTP, and exec channels without serializing long-lived channel work.
- [ ] Remote Git and non-Git folders return canonical authoritative identities;
      branch/display/connection names do not affect IDs.
- [ ] A stale or superseded runtime result cannot overwrite current project
      status or binding, and a live compatibility PTY blocks identity rebinding.
- [ ] Reconciliation copies prior layout state to the authoritative worktree
      without deleting the provisional source row or corrupting stable pane and
      terminal IDs.
- [ ] Existing remote file/session/terminal paths remain functional when the
      runtime is disabled or unavailable.
- [ ] Docker Linux tests, affected-package Clippy, and Windows MSVC checks pass;
      host `target`, `~/.cargo`, and `~/.rustup` remain absent.

## Notes

- Depends on stable worktree identity plus terminal warm/cold recovery.
- This child establishes runtime identity and transport. Provider-specific
  Agent event normalization and replay belong to the next child.
