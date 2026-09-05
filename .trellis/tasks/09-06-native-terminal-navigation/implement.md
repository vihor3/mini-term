# Terminal Navigation Execution Plan

First implementation child after final parent approval. Main activates this
child and dispatches implementation/check separately; neither child role spawns
other agents. Inherit every parent Actions-only and scoped-commit requirement.

## Implementation Order

- [x] Read the full compatibility report, identity/context/workbench/layout
  contracts, and parent constraints before changing any terminal state.
- [x] Add optional singular selected PaneKey and presentation-order fields with
  lossless layout restore/salvage/normalization/dual-write handling.
- [x] Add the current-worktree flat target projection and centralize select,
  cycle/index, reorder, new-terminal selection and close-neighbor behavior.
  Preserve original panel route owners and exact-live activation.
- [x] Reuse one selected terminal body; remove split/leaf tabs/exit-tree rendering
  while keeping search, IME, reconnect, file drops, and background PTY lifetime.
- [x] Move/enlarge tabs into the titlebar, remove the project dropdown/status and
  terminal-panel strip, and preserve document/detail page access without a second
  terminal tab row. Keep native drag/window-control hit regions separate.
- [x] Remove every split/group shortcut/menu/drag creation path. Redirect fork
  through a source-fenced new terminal and make per-tab close truly single-pane.
- [x] Anchor project settings, add hover/focus/open-menu trigger rules, move footer
  icons, and share delayed-first/prompt-following description behavior.
- [x] Lock global right-tool selection with independent worktree data; do not
  replace the existing ContextPanel owner with per-project saved selection.
- [x] Add regressions, coordinate review, and hand the retained flat target
  contract to the Agent-status child before it modifies overlapping store files.

Source reviews and follow-up lifecycle/tools corrections are complete in
`final-review.md` and the linked handoffs. The flat interface is ready for the
Agent child, which remains unactivated at this checkpoint. Actions validation
and native artifact acceptance are still outstanding. Windows CI now also runs
the terminal-host package's all-target regression suite.

## Actions-Only Cases

Checked implementation items above mean source implementation is handed off;
they do not assert completed source review, Actions success, or native acceptance.

- Multiple legacy panels, nested splits, several panes per leaf, dormant/exited
  records, and empty layout: complete inventory, one rendered terminal, no old
  live terminal mounted by transitions, unchanged routes/entities/PTYS.
- Select a non-first leaf/panel, visit other worktrees/documents and return, then
  save/restore. Cover malformed/absent order/selection, alias dirty-owner policy,
  stable fallback, and old JSON compatibility without dropping valid sessions.
- Tab clicks, Quick Open, Agent activation, keyboard cycle/index, reorder,
  new terminal, background Mobile StartSession, and close selected/background/last.
- X/menu/Ctrl+Shift+W respect one-terminal confirmation and never close hidden
  siblings; stale prompts and cancelled confirmations do nothing.
- Removed split tools/bindings/dispatch/body drag/maximize routes cannot recreate
  splits. Fork source closes/rebinds while awaiting CWD: no substitute or focus theft.
- File and Tasks detail pages retain previews/drafts/focus; terminal/file drops,
  selected reconnect/search/markers, raw terminal keys and IME remain usable.
- Long/duplicate titles, overflow, narrow/wide/high-DPI windows, native controls,
  tab drag versus window drag, footer/project-menu keyboard and pointer behavior.
- Tooltip initial delay, prompt next item, reset, removed anchor and late timer;
  right tool stays selected across A/worktree A -> A/worktree B -> B for all tools.

## Risky Surfaces and Rollback

`main`, `title_bar`, `terminal_area`, `workbench_area`, `hotkeys`, `pane_actions`,
store pane/context/layout/identity paths, `mt-config` saved layout fields and
`mt-layout` salvage. Keep deprecated tree readers, original route IDs, and
recoverable payloads. Do not migrate the entire project-state map or terminal host.
Adjust localized split/fork copy at source; generation and formatting run only
in Actions. Record code/CI evidence separately from native visual acceptance.
