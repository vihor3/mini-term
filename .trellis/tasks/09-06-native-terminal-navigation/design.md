# Flat Terminal Navigation and Shared Tools

## One Worktree, One Display

Use the [compatibility source report](../09-05-sidebar-agent-status/research/09-06-flat-terminal-compatibility.md)
as the entry-point inventory. Keep legacy `ProjectPanel` owners and saved split
trees internally; flatten presentation, not route identity. Enumerate every pane
of the current exact worktree's layout across panels/leaves, including dormant
and exited records. One visible top tab selects one terminal, never a group.

Targets reuse `TerminalJumpTarget` with project, host, worktree, original TabId,
PaneKey, logical session, and captured incarnation. Use PaneKey for widget identity,
not title, index, PTY handle, or shared internal TabId. A visual tab does not mint
a replacement runtime identity or move an attached process under a new owner.

Render only the selected pane's existing terminal body/entity, preserving search,
markers, reconnect/error state, IME, and file-path drops. Remove leaf tab bars,
split/maximize/collapsed-leaf rendering and exit layers that mount an old split
tree or a second terminal. Merely maximizing an old leaf is not single-screen
navigation. Background entities/subscriptions/Agent polling remain attached.

## Selection, Ordering, and Persistence

Add optional selected PaneKey and flat presentation-order fields to the existing
worktree layout JSON and runtime layout state. Keep `SavedTab.tabId`, complete
legacy trees, terminal/session/incarnation records and compatibility mirrors.
No new layout table or schema rebuild is needed.

- Derive absent order from saved panel order and DFS/leaf order. Normalize only
  selection/order metadata: ignore stale/duplicate keys and append every missing
  surviving pane deterministically. Invalid metadata cannot strand a terminal.
- Prefer a valid selected key. On live initialization use the current valid
  focused pane; old cold layouts fall back to the restored owner/active leaf.
  Closing the selected tab chooses a deterministic surviving flat neighbor;
  closing a background tab leaves selection unchanged. Empty stays empty.
- Selection updates its original owner and leaf compatibility pointers plus the
  singular selection before save. Reuse the exact activation boundary, including
  IME cleanup, live no-hydration path, and workbench-page handoff.
- Preserve ordinary label drag reorder through the separate flat order list,
  never by reparenting `PaneState` or rewriting live TabId. Keyboard cycle/index
  and close-neighbor selection consume the same order. Validate both drag targets
  against their captured worktree/terminal identity.
- Persist through current worktree dirty-owner and dual-write transactions;
  include new optional fields in salvage, normalization, restore and snapshots.
  Old binaries may omit the new preference fields on save, but retain terminal
  records and use deterministic fallback on re-upgrade.

Do not change the existing dormant recovery/hydration policy merely to flatten
rendering: ordinary activation may hydrate its old panel's eligible records.
Already attached exact targets must not hydrate/respawn anything. A tab inventory
read itself never starts processes. Keep new terminal creation selection updates
before persistence; background Mobile StartSession must not steal desktop focus.

## Remove Split/Group Entry Points

Remove split tools, tab/body context actions, split bindings/dispatch, directional
focus, maximize double-click, panel add/menu, and terminal-body split/merge drag
targets. Preserve terminal-file drag paths separately. Route fork to an individual
new terminal with the captured source shell/CWD and pending lineage registration
before command write. Revalidate that source after async CWD lookup; do not fall
back to whichever worktree/panel is now active.

Tab X, menu Close and Ctrl+Shift+W must close only the captured terminal using
existing Agent-aware confirmation. Do not retain a leaf-wide close behind one
flat tab. Window-wide shutdown accounting remains all-terminal, not selected-only.
Keep saved split types/readers for compatibility without exposing split actions.

## Titlebar and Existing Workbench Pages

Place the flat terminal row in the upper titlebar region freed by the project
dropdown/status removal. Use stable larger tab geometry, compact readable names,
bounded width, horizontal overflow, and active-tab reveal. Put interactive tabs,
close/add/menu tools outside window-drag hit regions and native window buttons.
Reuse mt-ui icons and current theme/type scaling; no new branding or icon library.

Document and Tasks-detail pages remain worktree-scoped with their current clean
preview/dirty/permanent behavior. Top terminal selection reveals the terminal
page without closing document tabs. Remove the redundant generic terminal-page
tab from the inner strip or replace its navigation role with the top terminal
selection; no second terminal row or per-panel workbench pages.

## Project Menu, Footer, and Tooltips

Anchor Project Settings to the project ellipsis trigger. Use row hover plus
keyboard focus/open-menu retention so an open menu does not disappear. Preserve
the existing project worktree visibility settings and only requested menu actions.
Settings, Usage, and Mobile become lower-left icon buttons with stable hit areas.

Reuse/refactor the existing `activity_bar::HoverSession` timing pattern into a
shared icon-description owner. Initial hover uses one delay; once a description
is visible, entering the next icon in the active sequence shows promptly. Leave,
removed anchors, clicks/menus, focus loss, and stale timers close/reset safely.
Do not stack mt-ui delay atop GPUI's own delay or assume `Tooltip::instant` removes
all latency. Descriptions render without intercepting toolbar clicks or moving
the controls. Include tooltip generation/race tests and narrow-window placement.

## Right Context Selection

Keep `Workspace::context_panel` and the Files/Git/Tasks/Sessions tool set shared
across project/worktree switches. Rebind only data owners/visibility/request
generations. The target worktree's files, Git drafts/history, Tasks preference,
and Sessions state stay independent. Unsupported/unavailable content stays in
the selected tool's error/empty state; it does not select Files automatically.
This right context sidebar remains; the removed bar is the terminal-panel strip.

## Risks

Live route reparenting, stale fork callbacks, leaf-wide close, transition layers,
and global-focus-only restore are the primary hazards. Preserve internal panel
titles and runtime pane labels without falsely promising new pane-title
persistence. Test multi-panel legacy layouts and worktree aliases, not only the
new simple layout. Exact geometry, focus, and tooltip timing require Actions
fixtures plus matching native artifact acceptance.
