# Worktree Context Sidebar

## Goal

Complete the Orca-style active-worktree context surface: keep the top-level
`Files / Git / Tasks / Sessions` route stable, isolate Files/Git/Sessions view
state and async ownership by `WorktreeId`, expose exact live Agent rows, and
show terminal recovery/connectivity diagnostics without treating history as
live evidence.

## Requirements

- Keep `ContextPanel` application-scoped so switching worktrees does not change
  the selected Files/Git/Tasks/Sessions tab.
- Use authoritative `WorktreeId` plus source generation for Files, Git, and
  Sessions caches, selection, scroll handles, and delayed results.
- FileTree must unwatch the old worktree before activating the new scope,
  restore last-known rows/selection/scroll when returning, and continue to use
  the existing exact source-signature fence for local and SSH I/O.
- GitPanel must preserve selected repository, viewed branch, section expansion,
  ratio, and cached metadata per worktree. Old repository/branch/mutation
  completions must not update a different worktree.
- SessionPanel must preserve history rows, view mode, selected preview,
  pagination, and scroll per worktree. Its scan path is the active worktree
  canonical path and every completion is fenced by worktree plus generation.
- Session history is read-only evidence. Live/stale/disconnected state comes
  only from `AgentRuntimeRegistry` and exact current terminal routes.
- Provide a shared read-only Agent target model and one exact activation action
  keyed by `AgentRunId -> WorktreeId + TabId + PaneKey + terminal incarnation`.
  Orca worktree inline rows and later global Agents must use it.
- Show terminal recovery state and bounded diagnostics for reattached, restored
  history, compatibility, unavailable, exited, stale, and disconnected panes.
  Never expose environment, raw argv, credentials, Hook secrets, or tokens.
- Worktree inline rows rank needs-you, working, done/waiting, then unknown;
  connectivity is a separate visible axis and disconnect never creates done.
- Preserve existing file preview semantics: one replaceable preview per
  worktree, row double-click renames, preview-tab double-click/edit pins.
- Keep Tasks as an independent placeholder for the next child.
- `MINI_TERM_ORCA_WORKTREE_CONTEXT=0` disables new inline/diagnostic overlays
  while preserving the Orca shell and existing panels.

## Acceptance Criteria

- Switching between two worktrees while Files, Git, or Sessions is selected
  keeps the tab type and restores each worktree's cached UI state on return.
- Old local/remote directory, Git, and session completions cannot overwrite the
  newly active worktree.
- File watchers are active only for the current local worktree.
- All historical session rows can show authoritative live/stale/disconnected
  badges without history creating a live run.
- A worktree can render multiple Agent rows; clicking one activates the exact
  project, terminal panel, split pane, and current incarnation or fails closed.
- Warm reattach, cold history restore, compatibility fallback, recovery
  unavailable, and disconnect are distinguishable in the Sessions diagnostics.
- Existing file preview and rename interaction tests remain green.
- Docker Linux tests/check, changed-line rustfmt, and Windows MSVC check pass.

## Out Of Scope

- GitHub Tasks implementation and authentication UI.
- A draggable or persistent global Agents window.
- New Git mutations or a replacement Git data model.
- Promoting historical transcripts into live Agent state.
