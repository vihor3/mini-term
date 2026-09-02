# Stable Worktree Workbench Identity

## Goal

Replace process-local project/pane/PTY routing with stable worktree-scoped identities so mini-term can restore the correct workbench, pane, and logical terminal across application restarts without confusing two worktrees or accepting stale terminal events.

This child establishes the identity and persistence foundation required by detached terminal reattach, remote runtime reconciliation, exact Agent routing, and the Worktree-scoped right sidebar. It does not claim that those later runtime features are complete.

## Confirmed Facts

- `layout.db` currently stores one JSON layout row keyed only by `project_id`; a malformed row is skipped as a whole (`crates/mt-layout/src/lib.rs:59`, `crates/mt-layout/src/lib.rs:312`).
- Saved terminal panes contain shell, cwd, and provider-session metadata, but no stable tab, pane, logical-terminal, or incarnation identity (`crates/mt-config/src/config.rs:328`, `crates/mt-config/src/config.rs:375`, `crates/mt-config/src/config.rs:383`).
- Runtime pane, terminal-panel, leaf, and split IDs are generated from one process-local counter; the PTY handle is a process-local `u32` (`crates/mt-app/src/tree.rs:31`, `crates/mt-app/src/tree.rs:103`, `crates/mt-app/src/tree.rs:183`).
- Restore currently creates new runtime pane/panel/node IDs and restores the first pane in each leaf as active (`crates/mt-app/src/persist.rs:69`).
- `AppStore` and the document workbench remain keyed by compatibility project IDs (`crates/mt-app/src/store/mod.rs:336`, `crates/mt-app/src/workbench_area.rs:37`, `crates/mt-app/src/workbench_area.rs:380`).
- The local Git worktree catalog already provides authoritative porcelain facts and generation fencing, but it does not yet expose stable host-qualified repository/worktree identities (`crates/mt-project/src/worktree/mod.rs:1`).
- All Rust formatting, compilation, linting, tests, and Windows packaging must run in Docker. The host must not gain a Rust toolchain, repository `target`, or project Cargo cache.

## Requirements

1. Define shared opaque strong types for `HostInstallId`, `ExecutionHostId`, `RepoId`, `WorktreeId`, `TabId`, `PaneKey`, `TerminalSessionId`, and `TerminalIncarnationId`. Serialized values must be versioned/domain-prefixed and stable across restarts.
2. Persist one local `HostInstallId` in `layout.db`. Derive the local `ExecutionHostId` from that install ID and a local-host marker. Display names, project IDs, branch names, and SSH connection names must not participate in authoritative identity.
3. Derive local Git `RepoId` from `ExecutionHostId + canonical Git common dir` and `WorktreeId` from `RepoId + canonical worktree path`. Main and linked worktrees in one repository must share a `RepoId` and have different `WorktreeId` values.
4. Give local non-Git folders and current WSL/SSH projects stable compatibility bindings so existing projects remain usable. These bindings must carry an explicit source/authority marker and be replaceable transactionally when a later authoritative runtime identity becomes available.
5. Add an additive `project_id -> execution host/repo/worktree` binding and make `active_worktree_id` available from `AppStore`. Existing project IDs remain a compatibility presentation/configuration key, not the workbench identity.
6. Persist terminal layouts by `WorktreeId`. Keep the legacy `project_layout` row as a rollback mirror and dual-write it while this compatibility period is active. A newer worktree row must never be overwritten by stale legacy data.
7. Persist `TabId`, `PaneKey`, `TerminalSessionId`, the expected `TerminalIncarnationId`, active tab identity, and active pane identity. The runtime `u32 pty_id` remains only a current-process attachment handle and must never be serialized as the logical terminal identity.
8. Preserve `PaneKey` and `TerminalSessionId` through split, move, reorder, rename, project/worktree switches, and application restart. Mint a new `TerminalIncarnationId` only when a new PTY is actually created or explicitly reconnected.
9. Migrate old layouts additively and atomically. Missing or invalid stable IDs are regenerated per affected object and written back once. A valid JSON layout with one malformed pane must salvage unaffected panes/tabs instead of dropping the entire worktree.
10. Rekey the current in-memory document/preview bucket and deferred focus checks to `WorktreeId` while retaining the existing project/backend/path document source needed for local and SSH I/O.
11. Create one central AppStore identity/binding boundary used by startup, project creation/removal, worktree materialization, layout save, document routing, and future async request fencing. Callers must not independently derive worktree identity from whichever project is active at completion time.
12. Preserve existing shell, file, Git, session, SSH, mobile, and Orca-shell behavior. This child must not silently claim warm PTY reattach, remote host authority, terminal-history restore, or exact live Agent status.
13. Run all validation and Windows packaging in Docker, then remove task-created containers and caches and verify the host remains free of Rust build state.

## Acceptance Criteria

- [ ] The same local installation, Git common dir, and worktree path produce the same `ExecutionHostId`, `RepoId`, and `WorktreeId` across two clean application starts.
- [ ] Two linked worktrees in one repository share `RepoId` but have distinct `WorktreeId` values and independent terminal/document workbench buckets.
- [ ] Recreating a compatibility project record for the same local worktree resolves the existing `WorktreeId` layout rather than creating an empty workbench.
- [ ] A migrated tab/pane keeps its `TabId`, `PaneKey`, and `TerminalSessionId` after save/reload, split, move, reorder, rename, and worktree switching.
- [ ] Cold process creation and explicit reconnect preserve `TerminalSessionId` but replace `TerminalIncarnationId`; late data addressed to the prior incarnation cannot match the current binding.
- [ ] Active tab and active pane restore by stable identity. Invalid saved pointers fall back deterministically without discarding valid siblings.
- [ ] A legacy `project_layout` row is copied to the bound `worktree_layout`, normalized, and written back once. A second load performs no identity churn.
- [ ] One malformed pane object in otherwise valid layout JSON is skipped/regenerated according to the salvage rules while unaffected panes and tabs remain available.
- [ ] Legacy `project_layout` stays readable and mirrors subsequent layout changes, allowing the stable-identity path to be rolled back without losing the latest terminal layout.
- [ ] WSL/SSH projects keep stable provisional bindings across restart and are clearly marked non-authoritative for later remote-runtime reconciliation.
- [ ] Document preview/active-page state is isolated by `WorktreeId`; a delayed callback from worktree A cannot focus, replace, or close a tab in worktree B.
- [ ] Docker-only focused tests, workspace check, task-package Clippy, and formatting pass; an updated Windows x64 installer is produced from the verified source state.
- [ ] After validation/packaging, no host `cargo`, `rustc`, `target`, `~/.cargo`, or `~/.rustup` state was created by this task.

## Out Of Scope

- Detached terminal-host ownership, warm attach to the same live OS process, output sequencing, terminal snapshots, and cold visual replay.
- Authoritative remote `HostInstallId`, SSH host-key binding, remote inventory, reconnect epochs, or WSL/SSH runtime deployment.
- `AgentRunId`, provider Hook envelopes, remote Agent status, unread acknowledgement, and global Agent feed routing.
- Persisting file/diff/transcript tab contents, editor buffers, selections, scroll positions, or right-sidebar view state. This child only changes their current worktree identity boundary.
- GitHub Issues/PR fetching or authentication.
- Further Orca visual redesign beyond any identity labels/diagnostics required to keep compatibility behavior honest.

## Technical Constraints

- Schema changes must be additive. Unknown newer fields/tables remain untouched on downgrade.
- Migration writes must use SQLite transactions and never delete the legacy row before the replacement row is durable.
- Canonicalization runs in the owning execution-host context. Current SSH bindings are therefore provisional and must not be presented as verified remote device identity.
- Stable IDs are routing/fencing keys, not authorization tokens.
- No blocking product, scope, UX, compatibility, or risk questions remain for this child.
