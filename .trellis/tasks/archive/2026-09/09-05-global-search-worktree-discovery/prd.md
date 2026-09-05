# Global Search and Automatic Worktree Discovery

## Goal

Deliver the missing Orca-style navigation loop in mini-term:

1. A global jump palette that quickly finds chats, terminals, worktrees,
   settings, and common actions without replacing the current workbench.
2. A project sidebar that automatically discovers and displays every Git
   worktree belonging to each configured project on Local, WSL, or SSH hosts.

The result must preserve mini-term's stable worktree/terminal identities so a
selection always opens the intended worktree, tab, pane, or Agent run.

## Background

- The approved Orca research already defined Search as Quick Open and the left
  hierarchy as `Project -> Worktree`.
- `search_modal.rs` is file name/content search and remains bound to
  `Ctrl+Shift+F`.
- `project_switcher.rs` is a project-only switcher on `Ctrl+Shift+P`; it does
  not search terminals, Agent targets, settings, or actions.
- The Orca sidebar currently auto-discovers local worktrees but deliberately
  emits one configured row for SSH projects. WSL discovery also does not
  consistently execute Git inside the owning distribution.
- The archived Trellis records are status/validation synchronized, but the
  original Orca architecture lost these two deliverables between research and
  implementation. This task is a corrective increment; historical archived
  records remain unchanged.
- Detailed evidence is in `research/current-state-and-orca.md`.

## Requirements

### R1. Global Jump Palette Entry and Layout

- The Orca sidebar Search row and the existing `switchProject` shortcut open
  the same global jump palette.
- Preserve the configured `switchProject` hotkey ID and its default
  `Ctrl+Shift+P` binding so existing user overrides continue to work.
- Preserve `Ctrl+Shift+F` and the existing file name/content search as a
  separate current-worktree feature.
- Render the palette as a large, top-centered modal over the current workbench,
  with a search field, functional Filter control, sectioned results, bounded
  scrolling, and a keyboard-help footer matching the supplied Orca reference.
- Opening the palette must not switch worktree, alter the right contextual tab,
  stop terminals, or acknowledge Agent attention.

### R2. Searchable Targets

- With an empty query, show `Recent Chats & Terminals` followed by
  `Recent Worktrees`.
- With a query, search these result families:
  - live/known Agent chat targets owned by mini-term;
  - terminal tabs and panes across configured worktrees;
  - discovered worktrees, with project, branch/path, and host keywords;
  - existing settings destinations and an explicit allowlist of common actions.
- Initial actions include Settings, Usage, Add Project, New Terminal, and the
  existing current-worktree file search. Unavailable actions are omitted or
  visibly disabled; selecting one must not silently do nothing.
- A terminal pane with a current Agent target appears as a chat result instead
  of a duplicate plain-terminal row in the same section.
- Search is case-insensitive, Unicode-safe, bounded to 2 KiB of input, and uses
  only in-memory projections while the user types. Typing must not start Git,
  filesystem, SSH, or session-history scans.
- Filter state is temporary and resets when the palette closes. It can narrow
  results by result family, execution host, and project.

### R3. Navigation and Focus

- Support pointer selection, `Up`/`Down`, `Enter`, `Esc`, `Tab` for Filter, and
  `Ctrl+1` through `Ctrl+9` for the first nine selectable rows.
- Every result carries its complete stable target before activation. The UI
  must not switch project first and then infer the target from whichever state
  is active afterward.
- Agent results reuse the existing exact `AgentRunId` activation boundary and
  never create, hydrate, or resume a stale Agent target.
- Terminal results revalidate project/worktree/tab/pane/session identity and
  may perform the normal terminal-tab hydration path only for that exact
  still-existing terminal. They must never create a replacement target when
  the selected pane disappeared.
- Worktree results revalidate the catalog owner before activating or
  registering the row.
- `Esc` and backdrop close return focus to the previously focused terminal/file
  surface when it still exists. A repeated open request keeps one palette
  instance and must not create a second dialog or steal focus into a hidden
  input. Successful navigation transfers focus to the selected target instead.
- A stale or removed result fails closed with a concise user-visible message
  and leaves the current workbench unchanged.

### R4. Automatic Worktree Discovery

- The exact folder added as a top-level project is the scan anchor. Run Git
  from that folder on its owning Local, WSL, or SSH execution host and display
  only the worktrees Git reports for the repository containing that folder.
- The app must not recursively search the project directory, enumerate every
  repository on the execution host, or include unrelated nested/sibling Git
  repositories. This is repository worktree discovery, not filesystem
  discovery.
- When the added folder is either the main worktree or a linked worktree, Git
  should resolve the same repository and return its related main/linked
  worktrees.
- A non-Git project folder remains a valid configured project and shows only
  its configured row; a `not a git repository` result must not trigger a wider
  host or parent-directory scan.
- Configured child worktrees are merged under their top-level project and do
  not independently start another repository scan.
- The sidebar renders main and linked worktrees beneath the project without
  requiring the user to add each linked worktree manually.
- Each row shows a stable status column, worktree/branch label, and host/project
  context consistent with the supplied Orca reference. Main, sparse, detached,
  locked, prunable, disconnected, and unavailable states remain distinguishable
  where the source provides them.
- Configured child worktrees and newly discovered Git rows are merged without
  duplicates using host-qualified canonical location, never display name alone.
- A valid discovered but unconfigured worktree is selectable. Selection
  registers it as a child of the owning root project through the centralized
  project-registration transaction, then activates the exact returned
  `ProjectId + WorktreeId`.
- Discovery is read-only. It must not create, prune, repair, remove, or delete a
  Git worktree or project registration.

### R5. Catalog Authority and Refresh

- Git porcelain remains the source of truth. Prefer
  `git worktree list --porcelain -z`; use text porcelain only for the verified
  unsupported-option case.
- Timed-out, truncated, malformed, disconnected, non-zero, or stale results are
  non-authoritative and cannot clear known rows or authorize destructive state
  changes.
- Preserve configured fallback and last-known rows while a refresh is running
  or fails; the sidebar and palette must not flash to an empty list.
- A completion is accepted only when its root project, execution host,
  canonical source, backend fingerprint, request generation, and observed SSH
  connection epoch still match.
- Refresh on startup, relevant project/config changes, window focus regain,
  successful worktree mutation, and a bounded foreground polling interval for
  remote/WSL hosts. Do not overlap scans for the same target, and cap global
  scan concurrency.
- Sidebar and palette consume the same catalog snapshots. They must not own
  separate scanners or disagree about which worktrees exist.

### R6. Isolation and Compatibility

- Activating two worktrees from one project continues to restore independent
  terminal tabs, splits, open files, preview state, Git state, and Sessions
  state through the existing `WorktreeId` ownership model.
- Local paths remain Windows-aware; POSIX paths remain case-sensitive. The same
  path spelling on different execution hosts remains distinct.
- Existing configured projects, child worktrees, layouts, user hotkey
  overrides, file search, Agents overlay, and right-sidebar behavior remain
  compatible. No data migration is required for this MVP.
- Empty-query recency is process-local: attention and current targets lead,
  followed by observed activation recency and deterministic sidebar/layout
  order. Do not fabricate timestamps or add a persistence schema solely for
  the palette.

### R7. Verification Policy

- All formatting, compilation, Clippy, tests, Windows MSVC checks, and Windows
  installer packaging run only in GitHub Actions.
- Do not run Cargo, repository test suites, packaging, generated-code checks,
  or Docker CI locally. Local verification is limited to document/source
  inspection, Trellis validation, Git status/diff, and `git diff --check`.

## Acceptance Criteria

- [x] Sidebar Search and `Ctrl+Shift+P` open one Orca-style jump palette;
      `Ctrl+Shift+F` still opens current-worktree file search.
- [x] Empty query shows recent chat/terminal and worktree sections with stable,
      deterministic ordering and no fabricated age labels.
- [x] Typed queries return matching Agent chats, terminal panes, worktrees,
      settings, and supported actions; 2 KiB and Unicode edge cases are tested.
- [x] Filter can narrow by type, host, and project, reconciles removed options,
      and resets on close.
- [x] Keyboard, pointer, direct `Ctrl+1..9`, backdrop/Esc close, selection reset,
      and focus restoration are covered by interaction tests.
- [x] Agent selection uses `activate_agent_run`; every mismatched stable route
      component or stale run is inert and does not clear unread state.
- [x] Terminal selection revalidates worktree/tab/pane/session identity and
      never opens a different pane when the original target disappeared.
- [x] Adding either the main folder or one linked-worktree folder discovers the
      same repository worktree inventory; configured/unconfigured rows merge
      without duplicates.
- [x] A non-Git project remains a single configured row, and unrelated nested,
      sibling, parent, and host-wide repositories are never included.
- [x] WSL Git discovery executes in the owning distribution and converts
      discovered POSIX paths back to valid host-visible project paths only at
      the registration boundary.
- [x] SSH discovery uses the selected connection, preserves POSIX case, and
      rejects a result from an old fingerprint, path, request generation, or
      connection epoch.
- [x] Timeout, output truncation, malformed porcelain, unsupported `-z`,
      disconnect, and stale completion tests preserve last-known rows and
      authority semantics.
- [x] Selecting an unconfigured local, WSL, or SSH worktree registers one child
      project under the correct root, deduplicates repeated selection, and
      activates the returned stable worktree.
- [x] Sidebar and palette read one shared catalog, and a catalog update changes
      both projections without a second Git scan.
- [x] Switching between two discovered worktrees restores their independent
      terminal and open-file workbench state.
- [x] GitHub Actions `CI` succeeds for the final product commit, including
      Linux workspace gates and Windows MSVC checks.
- [x] GitHub Actions `Windows Package` succeeds for that same product commit and
      publishes a verified installer artifact.

## Out Of Scope

- Filesystem-wide file lookup inside the jump palette. Existing file
  name/content search remains the file-search surface.
- Global scanning or full-text search of historical Agent transcripts. History
  remains in the active worktree's Sessions panel.
- Worktree create/delete/prune UX, project grouping redesign, browser pages,
  simulators, plugins, emoji commands, URL-to-task actions, or Kanban views.
- Persisting palette query, filters, open state, or MRU timestamps across app
  restarts.
- Changing terminal recovery semantics, remote Agent detection, GitHub Tasks,
  or the right contextual sidebar beyond consuming the selected worktree.
- Rewriting archived Trellis task history.
