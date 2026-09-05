# Core State Handoff

State/lifecycle/persistence slice is complete for source handoff. No UI edits
landed from this implementer; the attempted terminal-area replacement was
rejected before write. No builds, tests, format, generators, probes, or automated
checks were run. Static review is not Actions evidence.

## Current APIs

- `ProjectState::selected_terminal_pane_key: Option<PaneKey>` and
  `terminal_order: Vec<PaneKey>` retain flat selection/order beside original
  `panels` and their unchanged owner `TabId`s/trees.
- `SavedProjectLayout` adds optional `selected_terminal_pane_key` and
  `terminal_order: Option<Vec<PaneKey>>` (`selectedTerminalPaneKey`,
  `terminalOrder` JSON). Missing/malformed preferences cannot drop terminals.
- `ProjectState::ordered_terminal_panes()` includes all panes across all legacy
  owners; `selected_terminal()` resolves the singular display selection.
- `AppStore::terminal_tab_views(project_id) -> Vec<TerminalJumpView>` is the
  ordered, read-only exact project/worktree inventory. It includes dormant and
  exited records; never hydrate while rendering. Widget keys use `PaneKey`.
- `terminal_jump_target_for_pane(project_id, pane_id)` captures original full
  project/host/worktree/tab/pane/session/incarnation identity.
- `AppStore::activate_terminal_jump_target(&store, &target, window, cx) -> bool`
  performs exact revalidation, live no-hydration selection, and workbench page
  handoff. Use this for titlebar clicks; never assign global focus alone.
- `AppStore::reorder_terminal_tabs(&source, &target, after, cx) -> bool` fences
  both complete targets and changes presentation order only; no reparenting.
- Existing `cycle_pane` and `select_pane_by_index` now consume the same complete
  flat order and exact focus boundary. Their caller must retain/reveal the
  terminal workbench page as appropriate.
- `focus_pane` is focus-only: it rejects an inactive project or an unselected
  terminal. Navigation must use activation, not focus-only calls.
- `pane_actions::close_terminal_target(store, captured_target, window, cx)` is
  the single-terminal confirmation path. `close_pane` captures a target and
  delegates. Confirmation completion revalidates all identity components.

## Integration Still Needed By View Owner

1. Consume `terminal_tab_views` in the enlarged titlebar, retaining captured
   targets for click, reorder, X and menus. No native Drag hitbox on controls.
2. Render only the selected terminal's existing entity/body. Remove ALL old
   split, collapsed-leaf, leaf-tab, exit-terminal and old-panel layers.
3. Remove split/group/directional-focus/maximize menus, dispatch, bindings and
   pane-body merge/split drag targets; preserve file-path drops separately.
4. Route Ctrl+Shift+W to `pane_actions::close_pane`, never a leaf/group close.
5. Remove the vertical panel switcher and generic inner terminal-page tab while
   retaining document/detail state and globally selected right ContextPanel.
6. Integrate the separate tooltip handoff and sidebar OpenMobile event; update
   fork copy in locale sources only (generation is Actions-only).

## Removed Runtime APIs

Runtime split/group creation, movement, maximize, whole-leaf close, panel switch,
new-panel, panel close/rename APIs have now been removed from store/pane_actions.
Remove the old `terminals_panel` module declaration together with its render
entrypoints so its obsolete callers are not compiled. The saved tree operations
and panel owner records remain. `terminal_tab_views`, exact activation, reorder,
and single-terminal close signatures above are unchanged.

Removed names include `split_pane`, `split_pane_with_cwd`, `move_pane`,
`move_pane_to_tab`, `maximized_pane_id`, `toggle_maximized_leaf`,
`close_leaf_of_pane`, `close_leaf`, `set_active_panel`,
`set_active_panel_without_hydration`, `switch_panel`, `new_panel`,
`new_panel_from_launcher`, `close_panel`, and `rename_panel`. These are not
compatibility UI shims. Saved SplitNode readers and original owner records stay.

## Completed State Behavior

- Selection is updated before creation persistence; explicit selection syncs
  original owner and leaf compatibility pointers without changing route IDs.
- Closing a background pane preserves selection and does not hydrate. Closing
  the selected pane chooses the flat right neighbor, otherwise left, otherwise
  empty. Only a newly selected dormant neighbor may use ordinary hydration.
- Mobile background append preserves selection and existing leaf pointers;
  it appends presentation order without changing desktop focus or active project.
- Fork captures full route, source session/shell/CWD, selected pane and actual
  focus before async CWD lookup. Changed source/focus is inert. Successful fork
  creates one new terminal, registers lineage before command write, and reveals
  the terminal page. It never creates a split or substitutes a missing source.
- Restore keeps saved records even when no shell is configured; hydration can
  report the unavailable shell. The view should show the existing error/empty
  body state for a selected record without an entity, not remove its tab.
- Exact Agent activation now recognizes a selected non-first legacy leaf.
- Existing latest-dirty-alias and transactional worktree/mirror save ownership
  remains intact. ContextPanel selection storage was not changed.

## Authored Regressions

14 new tests across store/mod, store/layout, store/context, pane_actions,
mt-config and mt-layout cover complete nested inventory, non-first owner/leaf
round trips, missing shells, duplicate/stale/absent preferences, idempotence,
reorder/cycle/index ownership, single-close neighbors, background append,
source/incarnation rejection, fork fencing, salvage, dual writes and alias saves.
All are authored only and unrun locally.

## Owned Paths And Gate

Changed product files: `crates/mt-app/src/pane_actions.rs`, `persist.rs`,
`store/{mod,panes,layout,identity,context}.rs`,
`crates/mt-config/src/config.rs`, `crates/mt-layout/src/lib.rs`.
SavedProjectLayout literals in these files were updated; no required literal
changes were found in the view owner's files. No tree.rs changes were needed.

View owner/reviewer must finish the six integration steps above, remove all
callers of deleted APIs, and retain complete captured targets through deferred
menu/drop actions. The tooltip handoff belongs to the separate UI owner.
No additional state API change or known source blocker remains. Main owns
Actions formatting/type/tests/generated corrections and native artifact
acceptance. PTY/focus/IME integration and exact-live no-hydration behavior still
require the integrated Actions/native gate; pure tests do not prove those UI cases.
