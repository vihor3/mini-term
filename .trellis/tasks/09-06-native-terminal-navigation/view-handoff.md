# Terminal View Handoff

Source implementation is complete for the assigned view slice. No builds,
Cargo commands, tests, probes, app launches, formatting, generators, whitespace
checks, CI dispatch, staging, commits, or automated verification ran locally.
Source reading and read-only Git diff review are not passing CI evidence.

## Changed Files

- `crates/mt-app/src/title_bar.rs`: 44px titlebar, fixed 176px individual
  terminal tabs keyed by PaneKey, complete current-worktree inventory, scroll,
  active-tab reveal, overflow menu, exact-target reorder and single close.
  Add/overflow and tab activation are keyboard-accessible. Native window hit
  regions remain separate from tabs/tools; Windows buttons have no click handler.
- `crates/mt-app/src/terminal_area.rs`: only the selected existing terminal
  entity is mounted. Removed split trees, leaf tabs, maximized/collapsed
  rendering, pane split/merge targets, and all outgoing terminal render layers.
  Retained search, markers, reconnect, error/empty/first-run states and file
  drops. File/reconnect/marker callbacks validate the captured terminal target.
- `crates/mt-app/src/main.rs`: removed TerminalsPanel and focus-nav module/entity
  registration, both layouts' vertical strip, split/directional actions and
  handlers. Ctrl+Shift+W calls single-pane confirmation. Cycle/index/new reveal
  the terminal page. Escape cancels active drags without intercepting normal
  terminal Escape. Added the OrcaSidebar OpenMobile arm. ContextPanel is unchanged.
  Startup focus now uses active_pane_id so restored non-first leaf selection
  receives focus; F2 resolves its captured pane across the project state.
- `crates/mt-app/src/workbench_area.rs`: removed the generic inner Terminal tab;
  document/detail tabs, previews, drafts and page focus logic remain. Hiding the
  terminal body clears only transient marker/tooltip UI, not terminal owners.
- `crates/mt-app/src/hotkeys.rs`: removed split and directional-focus registry
  entries and bindings; retained terminal raw Tab/Shift+Tab protection.
- `crates/mt-app/src/dnd.rs`: DragTerminalTab retains the full TerminalJumpTarget;
  tab drops select before/after only. Removed pane-body split geometry.
- `crates/mt-app/src/pane.rs`: terminal-body fork captures the full target when
  its menu opens, then revalidates before invoking the core fenced fork API.
- `crates/mt-i18n/locales/{paneGroup,settings}.ts`: fork says new terminal and
  the close shortcut says current terminal. Generated dict is deliberately
  untouched; generation belongs in Actions.

## Interfaces

- Uses final core `terminal_tab_views`, `selected_terminal`, exact activation,
  `reorder_terminal_tabs`, and `close_terminal_target`. No new state API needed.
- `TitleBar::new` now receives the WorkbenchArea entity so document/detail
  activation updates terminal-tab styling without resetting saved selection.
- `TerminalArea::suspend` clears transient UI only; WorkbenchArea invokes it
  when a non-terminal page becomes visible.
- Retained tool groups use IconTooltips with separate navigation, native-window
  and terminal-tool entities. No GPUI tooltip/on_hover is added to wrapped tools.

## Authored Tests

Four new source regressions, all unrun: flat terminal menu composition and
single-close-only actions; retired shortcut/binding absence; tab midpoint/drop
containment including empty bounds; active-tab reveal for visible, offscreen and
narrow strips. Existing native control/close-risk, file-drag and raw terminal-key
tests remain. Core owns legacy-layout/identity/lifecycle regression coverage.

## Review And Gate

- Actions must provide formatting, i18n generation, compile/lint/test evidence;
  native artifact acceptance still must cover Windows controls, overflow/drag,
  keyboard/IME, search/markers/reconnect, documents and background PTY lifetime.
- `tree.rs:264` still links to the removed `dnd::pane_drop_zone` helper in a
  legacy DropZone doc comment. Main can update this reference; tree.rs was outside
  the assigned files. Old terminals_panel.rs/focus_nav.rs stay on disk but are
  intentionally no longer module-registered.
- Existing `modal::open_rename_pane` still captures only project/pane IDs inside
  its confirmation callback. Titlebar/menu entry validates the complete target
  before opening it, but the modal itself should capture/revalidate that target
  before applying a late rename. modal.rs was outside this slice.
- Legacy project-sidebar hover previews still use pane_preview's saved tree
  snapshots outside this slice; individual terminal hover-preview helpers are
  no longer used by the new titlebar. Review any resulting dead-code warnings
  through the Actions gate without adding blanket suppression.
  The legacy preview is a non-interactive snapshot, not a second live terminal
  surface or split-creation route. No first-active-leaf display/focus lookups
  remain in the owned view modules after the startup-focus follow-up. Native
  restart acceptance must include a saved selected pane in a non-first leaf.
