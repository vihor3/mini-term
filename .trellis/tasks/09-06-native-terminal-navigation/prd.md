# Consolidate terminal navigation and shared icon tooltips

## Goal

Give each worktree one terminal display with independent top tabs, preserve the
selected right-side tool across projects/worktrees, and simplify navigation and
shared icon tooltips without losing terminal sessions.

## Requirements

- Own parent requirements R4 through R10 and R17. Inherit all constraints and
  confirmed decisions in [the parent PRD](../09-06-native-ui-remote-feedback/prd.md).
- Remove the right vertical terminal switcher and titlebar project/Agent
  dropdown; move and enlarge terminal tabs in the upper titlebar area.
- One top tab represents one terminal of the current worktree. Render only its
  terminal on the single display surface; background terminals continue running.
  There is no user-facing panel/group layer, split rendering, split action,
  split drag target, or split shortcut. Fork/new-terminal workflows open a new
  terminal tab instead of creating a split.
- Keep all existing terminals, their stable runtime identities, close
  confirmations, and saved terminal records. Legacy grouped/split terminals
  appear in the flat tab inventory and must not be dropped, respawned, or have
  live routing identities silently reassigned just to simplify the UI.
- Project Settings belongs in the project-row ellipsis menu. Reveal the trigger
  on row hover/focus and keep an open menu usable; preserve worktree settings.
- Lower-left Settings, Usage, and Mobile are icons with descriptions. Toolbars
  use the same delayed-first, prompt-following tooltip interaction.
- Keep interactive tabs and controls out of the window-drag hit region. Long
  titles, overflow, and narrow windows must not overlap controls or resize rows.
- Preserve ordinary terminal-tab selection, ordering, keyboard cycling and
  single-terminal close without making internal group boundaries user-visible.
- Keep the right-side Files/Git/Tasks/Sessions tool set and selection at window
  scope. Project/worktree switches change only the source and contents; they do
  not reset the selected tool or merge its per-worktree data/account preferences.
  This right context sidebar stays; only the terminal-panel switcher is removed.

## Evidence

- `crates/mt-app/src/tree.rs:209` owns project panels with complete split trees;
  `:276` owns leaf terminal tabs. `terminals_panel.rs:119` switches whole panels.
- `crates/mt-app/src/title_bar.rs:883` inserts the existing project switcher;
  `:910` owns the center drag region.
- `crates/mt-ui/src/tooltip.rs` adds a local delay on top of GPUI's hover delay.
  Its instant mode alone does not implement a shared warm hover sequence.
- `crates/mt-app/src/main.rs:520` already holds the right context selection on
  `Workspace`; `:965` changes selection and `:1195` chooses the content view.
  Preserve this boundary rather than adding per-project selected-tool storage.
- `crates/mt-app/src/terminal_area.rs:626` exposes split context commands;
  `:2100` renders split tools. `crates/mt-app/src/pane_actions.rs:376` also forks
  into a split. All creation paths, not just the visible toolbar, need alignment.
- [Compatibility research](../09-05-sidebar-agent-status/research/09-06-flat-terminal-compatibility.md)
  identifies retained route owners, singular saved-selection gaps, leaf-wide
  close, dormant hydration, and hidden split/transition entry points.

## Acceptance Criteria

- [ ] Each worktree has one display surface and one flat row of individual
  terminal tabs. No terminal group or split navigation remains (R4/R8/R9).
- [ ] Old grouped/split terminal records all remain reachable from top tabs;
  switching tabs preserves the exact terminal and background Agent ownership (R4/R9).
- [ ] Menus, shortcuts, drag operations, and fork/new-terminal actions cannot
  recreate splits; retained creation workflows produce individual tabs (R4/R9).
- [ ] Tabs are larger, fit in the titlebar, support overflow, and do not interfere
  with dragging/resizing/window controls (R9).
- [ ] Tab selection/order survives worktree switches and restart; close/cycle
  and reorder use the same complete terminal inventory without route changes (R9).
- [ ] Project settings opens at the project menu; hidden triggers remain
  accessible and worktree visibility behavior is unchanged (R6).
- [ ] Footer entries are icons in the requested location and retain their
  commands; hover descriptions follow the shared timing rule (R5/R7/R10).
- [ ] Select Files on A/worktree A, switch to A/worktree B and project B, and
  remain in Files with each target's contents. Repeat for Git, Tasks, Sessions;
  an unavailable target produces a state in the same tool, not a reset (R17).

## Out of Scope

Visible split/group terminal workflows, replacing the terminal host or Agent
route schema, migrating the entire project-state map, destructive layout rewrite,
and redesigning document/Tasks detail storage or adding pane-title persistence.

## Risks

Legacy owner TabIds remain runtime correlation facts even though they disappear
from UI. Old exit transitions and leaf-wide close can violate the single-terminal
model. Existing dormant hydration is separate from visibility and is not replaced
with a new lifecycle policy by this task.

## Planning Status

The previous split-workspace-tab recommendation is superseded by the user's
single-terminal clarification. The user approved the final parent plan on
2026-09-06. PRD, compatibility research, design, execution plan and curated context
are reviewed for this first implementation child. All compilation, tests, UI
harnesses, format/lint and packaging remain GitHub Actions-only.
