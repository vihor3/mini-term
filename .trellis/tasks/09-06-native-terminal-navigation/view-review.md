# Full Navigation View Review

Source review on 2026-09-06. Reviewed the current PRD, design, execution plan,
parent constraints, curated contracts/research, affected package spec indexes,
and core/tooltip/view handoffs. Also incorporated `core-review.md` and
`sidebar-tooltip-review.md`. No agents were spawned. Only source reading,
scoped edits, and read-only Git review were performed locally.

## Findings (fixed)

- File: `crates/mt-app/src/modal.rs:55`
  Issue: Rename confirmation retained only project/pane IDs, allowing a late
  rename after the terminal incarnation or project binding changed.
  Fix: Capture and validate the complete `TerminalJumpTarget` before creating
  the input; revalidate that same target immediately before renaming. The public
  dialog signature and runtime-only title behavior are unchanged.
- File: `crates/mt-app/src/title_bar.rs:352`, `:423`
  Issue: Explicit retained tab/Add/overflow focus handles were not tab stops.
  GPUI applies Div `tab_index` defaults only to implicitly created handles.
  Fix: Set `tab_stop(true)` on all three handle creation paths, incorporating
  the sidebar reviewer's cross-owner finding.
- File: `crates/mt-app/src/title_bar.rs:467`
  Issue: GPUI automatically focuses an explicitly tracked handle on mouse down.
  Dragging a tab therefore moved focus out of the terminal/document even though
  reorder correctly preserved the selected terminal in the store.
  Fix: Prevent default left-mouse-down focus on the tab without stopping drag
  event propagation. Completed clicks still use exact activation; keyboard
  focusability is retained.
- File: `crates/mt-app/src/main.rs:1594`
  Issue: An invalid Ctrl+number request still called `activate_terminal_page`,
  leaving a document/Tasks detail page despite the store's out-of-range no-op.
  Fix: Resolve index availability against the existing complete flat inventory
  before invoking selection or handing off the page.
- File: `crates/mt-app/src/pane.rs:1912`
  Issue: Search-toolbar mouse events could arm the ancestor terminal body's
  click-to-focus callback; releasing a search-control click then focused the PTY.
  Fix: Stop left-mouse-down propagation at the search-bar wrapper. Its own input
  and controls still process the event, while the body no longer arms a click.
- File: `crates/mt-app/src/tree.rs:264`, `main.rs`, `hotkeys.rs`
  Issue: A legacy DropZone doc link referenced deleted `dnd::pane_drop_zone`;
  navigation comments still described leaf-local indexing/old titlebar scans.
  Fix: Replace the dead link with compatibility-only wording and align the
  affected navigation comments. All legacy tree readers/functions remain.
- File: `crates/mt-app/src/terminal_area.rs:749`
  Issue: Menu composition tests did not detect stale generated fork/close copy.
  Fix: Add `fork_and_close_copy_describe_individual_terminals`, covering English
  and Chinese generated labels. It requires the Actions-produced i18n update;
  no generated dictionary was edited or generated locally.

## Findings (not fixed)

Main has assigned the two lifecycle findings below to a new implementer owning
`pane_actions.rs`, `store/{panes,ssh}.rs`, and narrowly `store/mod.rs`. They are
being addressed separately, not claimed fixed or verified by this view review.
Main will coordinate a focused follow-up check; this review is complete and does
not wait for that implementation.

- P1, core lifecycle: `store/panes.rs:147` removes a dormant pane without a host
  close when it has no GUI `pty_id`. A saved hosted terminal in an unhydrated
  owner can therefore disappear from the flat inventory while its process
  remains live. Confirmed the core review's path through exact dormant-target
  resolution and close confirmation. Follow `core-review.md` for the required
  session/incarnation-fenced host close and failure handling. Not changed:
  terminal-host lifecycle/module boundary and another owner's files. Status:
  assigned to the lifecycle implementer; focused follow-up verification pending.
- P2, reconnect: `store/ssh.rs:324` disposes the attachment before resolving a
  replacement shell at `:358`. With no configured shell, the pane retains its
  now-invalid `pty_id`; the exact resolver rejects selection/close. Confirmed
  the core review's finding. Resolve the shell before disposal and add an
  Actions reconnect/close regression. Not changed: outside this write slice;
  do not weaken the view's exact-target validation to hide the broken state.
  Status: assigned to the lifecycle implementer; focused follow-up verification
  pending.
- P2, settings: `settings/pages_terminal.rs:154` still renders the
  `terminal_animations` toggle, but no terminal renderer reads that setting
  after removal of the old transitions. Its localized description at
  `locales/settings.ts:31` and `:261` still advertises panel/split/maximize
  transitions. Recommend removing the obsolete settings row while retaining
  saved compatibility data. Not changed: settings view is outside this slice;
  changing copy alone would leave a misleading no-op control.
- P2, toolbar tooltip consistency: `mt-ui/src/terminal/search_bar.rs:420`
  still wraps search controls with GPUI `.tooltip` plus `Tooltip::new`.
  `mt-ui/src/tooltip.rs:43` adds 700 ms after GPUI's initial 500 ms, and these
  controls have no shared warm hover session. This retained navigation toolbar
  does not satisfy R5. Recommend adopting `IconTooltips` in that owner and
  extending the Actions hover checks. Not changed: this mt-ui file is outside
  the assigned write slice.
- P3, obsolete state helpers: `store/ai.rs:825` still exposes the old
  `title_bar_snapshot` and its `TitleBarLight` aggregation without a runtime
  titlebar caller. The core report also records unused split-size/node and
  terminal-strip visibility helpers. Coordinate scoped cleanup with their
  owners; retain tray aggregation and required saved-tree compatibility.
  Not changed: outside this slice. No compiler warning is claimed without CI,
  and no dummy use or lint suppression was added.

## Integration Reviewed

- The titlebar reads `terminal_tab_views` for the exact current project/worktree,
  uses PaneKey widget identity, fixed 176 px tabs within a 44 px row, horizontal
  overflow/active reveal, and separately bounded native drag/window controls.
  Tab click, overflow, close/menu and both reorder endpoints retain full targets.
- `TerminalArea` mounts at most one existing terminal entity. No split/leaf tab,
  collapsed/maximized pane, or outgoing live-tree transition remains in that
  render path. Workbench document/detail pages hide rather than dispose PTYs.
- Compiled navigation no longer exposes split/group/directional-focus/body-drag
  creation paths. Old `terminals_panel.rs` and `focus_nav.rs` are unregistered;
  legacy tree operations remain for compatibility. Ctrl+Shift+W reaches the
  single-terminal confirmation boundary.
- Search/markers/reconnect and internal/external file-path drops retain their
  existing terminal operations with exact selected-target checks. Raw terminal
  keys/IME remain owned by TerminalView; the Terminal-context NoAction bindings
  still keep plain Tab/Shift+Tab out of Root focus navigation.
- Flat order, selection, complete legacy inventory and original route owners
  flow through state, restore, snapshots, preference salvage, and dual writes.
  Core tests now cover non-first owners and multi-pane leaf siblings. Startup
  focus reads the singular active pane. No alias-map migration was introduced.
- The final approved hydration policy is retained: ordinary dormant activation
  can recover eligible records in the selected original owner; exact-live
  activation, inventory rendering, and background close do not hydrate siblings.
  The brief selected-only hydration proposal was retracted, not adopted.
- Fork retains source/session/shell/CWD plus selection/focus fences across CWD
  lookup, creates an individual terminal, and registers lineage before input.
  Mobile append remains background-only; OpenMobile is wired to the lower-left
  sidebar event. Workspace still owns the one global ContextPanel selection;
  worktree content owners and their request generations are separate.
- Titlebar and selected-terminal icon groups use the shared IconTooltips API
  without a second GPUI tooltip. The final sidebar reviewer reset fixes are
  present. Ordinary label/marker text tooltips remain separate from icon groups.

## Verification

- Lint: NOT RUN; GitHub Actions only.
- TypeCheck/build/Cargo metadata: NOT RUN; GitHub Actions only.
- Tests: NOT RUN. One additional bilingual regression was authored here.
  Existing core exact-target, flat index/order/close, persistence and tooltip
  pure regressions were source-reviewed, not executed.
- Formatting, whitespace checks, generation, app launch and native acceptance:
  NOT RUN. Generated dictionary remains untouched. Main owns exact-commit
  Actions diagnostics/corrections, commits, push and artifact acceptance.

Missing boundary coverage remains explicit: actual dialog confirmation after
rebind/reconnect, keyboard tab reachability and offscreen focus, drag cancellation
without focus loss, invalid index while a detail page is active, and search
input/control focus after mouse release. The existing pure resolver/index tests
do not execute those GPUI callbacks; this review did not add a new test-support
dependency merely to fabricate a local substitute.

Actions/native acceptance must additionally cover hosted dormant close,
shellless reconnect, exact-live spawn counts and continued background output,
non-first-leaf restart, selected/background/last close, deferred fork input
ordering, Mobile focus, A/worktree-A to A/worktree-B to B for every right tool,
long/duplicate/Unicode titles, narrow/high-DPI overflow, Windows native controls,
file drops, IME, and tooltip timing/reset. Use Actions-produced artifacts only.
This source review does not mark the quality gate or native acceptance passed.

Main owns spec/task metadata synchronization. The current identity/layout specs
already describe the flat selection/order and global context boundary; no spec
file was modified by this reviewer.
