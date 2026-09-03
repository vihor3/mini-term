# Technical Design

## Shared Scope

`AppStore` exposes canonical active-worktree path data and immutable view
models for current terminal/Agent ownership. UI components never reconstruct
routes from project names, paths, PTY IDs, or session files.

```text
AgentRuntimeState + terminal_routes + ProjectState
                    |
                    v
              AgentTargetView
                    |
         +----------+-----------+
         |                      |
Orca inline rows        Sessions diagnostics
         |                      |
         +------ activate_agent_run(run_id) ------> exact project/tab/pane
```

Activation re-reads the current run and route, resolves its worktree project,
checks panel `TabId`, `PaneKey`, `TerminalSessionId`, and incarnation, switches
project/panel, then focuses the pane. Missing or stale ownership returns false
and does not create or resume a terminal.

## Panel State

Each panel keeps an in-memory map keyed by `WorktreeId` and one current scope.
The global context tab remains in `Workspace`.

- FileTree cache: rows, chain ownership, Git status, selected path, root error,
  and one `ScrollHandle`. Watchers/loading ownership remain active-scope only.
- Git cache: repository metadata, selected repo, branch list/view branch,
  section open flags/ratio, last sync state, and generation. Existing child
  views are re-synchronized after restore.
- Sessions cache: host/WSL/SSH rows, lineage, pagination, view mode, preview,
  diagnostics selection, and one `ScrollHandle`.

A scope switch first saves stable last-known presentation, increments the
component generation/request counters, cancels or invalidates active handles,
then restores the target bucket before scheduling refresh. Background closures
capture the exact worktree ID and generation in addition to existing source
facts.

## Canonical Path

`AppStore::canonical_worktree_path_for_project` reads the persisted binding's
canonical path and falls back to configured project path only when no canonical
value exists. Files and Sessions use this boundary. SSH sources still carry
connection identity/fingerprint and never fall back to local filesystem scans.

## Diagnostics

`TerminalDiagnosticView` contains only stable public IDs, display label,
recovery enum, exited flag, bounded backend notice, and optional remote probe
capability/connectivity/error. `AgentTargetView` contains run ID, project and
pane display data, route, provider, activity, connectivity, evidence, last
receipt, and attention.

Terminal recovery and Agent connectivity remain separate. A restored transcript
can be cold while its resumed Agent is live; a disconnected Agent retains its
last activity. UI labels are derived from enums and do not mutate runtime state.

## Rollback

`MINI_TERM_ORCA_WORKTREE_CONTEXT=0` hides inline Agent rows and recovery
diagnostics and leaves existing FileTree/GitPanel/SessionPanel behavior in
place. Stable identities, terminal-host records, and Agent runtime state are
not removed or rewritten.
