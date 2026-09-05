# Current State, Orca Evidence, and Trellis Drift Audit

## Scope

This note records the repository evidence behind the global jump palette and
automatic worktree discovery plan. It also audits whether the archived Orca
runtime task already delivered these behaviors.

## Orca Evidence

The reference checkout is `/home/leo/orca` at commit
`5aa02ead59a4f34a186c3e8814558b5795260ee9`.

- `src/renderer/src/components/WorktreeJumpPalette.tsx` owns the modal lifecycle.
- `worktree-jump-palette-surface.tsx` renders a roughly 900 px top-centered
  command dialog with the search field, Filter affordance, bounded list, and
  keyboard footer.
- `use-worktree-jump-palette-list-entries.ts` renders empty-query sections as
  `Recent Chats & Terminals` and `Recent Worktrees`; queried results add
  worktrees, projects/groups, open tabs, settings, and actions.
- `use-worktree-jump-palette-filter.ts` builds host/project-aware filters and
  reconciles stale selections when the catalog changes.
- `use-worktree-jump-palette-selection-actions.ts` re-resolves a target before
  activation and handles worktree, tab, settings, and action targets through
  type-specific activation paths.
- `use-worktree-jump-palette-selection-lifecycle.ts` snapshots the previous
  focus target, resets query/filter state on open, and restores focus only when
  the selection did not intentionally navigate elsewhere.
- `use-worktree-jump-palette-worktrees.ts` and
  `lib/recent-workspace-tab-rows.ts` keep host-qualified worktree identity and
  rank attention before recency.
- `components/cmd-j/palette-results.ts` bounds query size and ranks settings and
  actions separately from worktree/open-tab matches.

The mini-term implementation should copy these interaction contracts, not
Orca's React/Tailwind structure or its unrelated browser, simulator, plugin,
emoji, and worktree-creation features.

## mini-term Confirmed State

### Existing search surfaces

- `crates/mt-app/src/search_modal.rs` is current-project file name/content
  search. It is opened by `Ctrl+Shift+F` and must remain a separate feature.
- `crates/mt-app/src/project_switcher.rs` is a project-only fuzzy switcher
  opened by the `switchProject` action (`Ctrl+Shift+P`). It already contains
  useful GPUI patterns for Dialog ownership, input focus, cursor movement, and
  Enter handling, but its result model is too narrow.
- `crates/mt-app/src/orca_sidebar.rs:736` currently sends the sidebar Search row
  to the file-search modal, so it does not implement the approved Quick Open
  behavior.
- `crates/mt-app/src/overlay.rs` already has guarded modal ownership and focus
  restoration. A new palette must stay inside this overlay contract.

### Existing stable activation data

- `AppStore::agent_target_views()` provides immutable live Agent targets with
  `AgentRunId`, host/worktree/tab/pane/session/incarnation route data, status,
  attention, and receipt time.
- `AppStore::activate_agent_run()` re-resolves all stable route identities,
  focuses only an existing live pane, reveals the terminal page, and
  acknowledges only after successful focus.
- `ProjectState.panels`, `ProjectPanel.tab_id`, `PaneState.pane_key`,
  `TerminalSessionId`, and optional `TerminalIncarnationId` provide enough data
  to construct exact terminal targets.
- `activate_existing_pane` is currently store-private. The palette needs a
  public exact terminal-target boundary rather than reproducing pane routing in
  the UI.

### Existing worktree catalog

- `mt_project::worktree::scan` already uses
  `git worktree list --porcelain -z`, strict byte parsing, text fallback only
  for the supported-option failure, bounded output, timeout cleanup,
  single-flight caching, mutation generations, and last-known degradation.
- `crates/mt-app/src/orca_sidebar.rs` already scans top-level local projects,
  merges authoritative Git rows with configured compatibility children, keeps
  prior rows on failure, and materializes an unregistered local worktree only
  when the user selects it.
- The same file explicitly excludes SSH projects from `scan_targets`, and
  `build_project_rows` returns exactly one configured row for every remote
  project. The test `remote_project_has_exactly_one_configured_worktree_row`
  freezes this limitation.
- WSL projects currently travel through the non-SSH local branch. The new
  catalog should route WSL Git commands through the WSL execution backend so
  the path is interpreted by its owning host.
- `execution_host.rs` already provides bounded structured command execution for
  Local, WSL, and authenticated SSH backends, including observed SSH epochs.
- `AppStore::project_execution_snapshot()` provides the host-qualified root,
  canonical path, backend signature, and connection fingerprint/epoch needed
  to fence background work.

### Registration limitation

- `register_or_activate_project` correctly deduplicates local and SSH
  canonical locations, prepares worktree identity, persists the project, and
  returns the exact project/worktree pair.
- New records created through that boundary are always top-level projects.
  `add_project_at(..., Some(parent))` can create a local compatibility child,
  but it is local-only and bypasses the unified SSH registration contract.
- Automatic discovery therefore needs one centralized child-worktree
  registration path that validates the parent/host relationship and reuses the
  existing transaction instead of adding a second raw insertion helper.

## Trellis Progress and Drift Audit

The archived task records are mechanically synchronized:

- `09-02-worktree-catalog-v2` is completed and its parser/catalog acceptance
  criteria are backed by implementation and later GitHub Actions evidence.
- `09-02-orca-project-worktree-runtime` is archived as 10/10 children complete,
  with its integration and later Actions-only validation recorded.
- The latest unified project-onboarding task is also completed, archived, and
  tied to successful CI and Windows packaging runs.

There are nevertheless two product-scope gaps against the earlier approved
Orca research:

1. The research required Search to open Quick Open across worktrees, open tabs,
   and live Agent targets. No implementation child owned that deliverable;
   mini-term still has only file search plus a project-only switcher.
2. The research placed the worktree catalog on the owning execution host. The
   catalog child explicitly deferred SSH/WSL transport, and no later child
   completed automatic remote worktree discovery. The current sidebar source
   confirms the remote one-row fallback.
