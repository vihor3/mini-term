# Address Mini-Term native UI and remote workflow feedback

## Goal

Make the reported native workflows accurate and consistent: one terminal display
per worktree, reliable owned-Agent state, complete file browsing, remote Git, and
independent gh account selection in Tasks. Switching projects preserves the
selected right-side tool without mixing their data or terminal sessions.

## Requirements

- R1: Routine project inventory refresh must not repeatedly flash a progress
  glyph when a terminal is open without an Agent. Manual progress and actual
  connectivity failures must remain distinguishable from Agent activity.
- R2: Correct false Agent task states using Orca's evidence ownership and
  semantic-state handling as references, without treating process existence or
  unrelated terminal output as proof of work.
- R3: Monitor only Agents belonging to terminals opened by Mini-Term. Continue
  monitoring its background terminals in other projects after project changes;
  show each under its owning project. Exclude external terminals' Agents.
- R4: Remove the terminal area's right-hand vertical terminal-panel switcher
  and its add button. Each worktree has one terminal display surface and no
  user-facing terminal groups or split-screen capability. Keep existing terminal
  sessions reachable as individual tabs, including terminals in old split layouts.
- R5: Toolbar icon descriptions have an initial hover delay; subsequent icons
  show descriptions promptly during the same continuous hover sequence.
- R6: Place Project Settings in the project-row ellipsis menu as illustrated.
  Hide the ellipsis when the project title row is not hovered, without making
  the menu inaccessible to keyboard focus or while the menu is open.
- R7: Present Settings and Usage as lower-left icons with R5 tooltips.
- R8: Remove the titlebar project/Agent dropdown and its redundant indicator.
- R9: Move terminal tabs into the upper titlebar area and enlarge them. One tab
  selects one terminal belonging to the current worktree, not a group or split
  workspace. Exactly one terminal is shown; the others continue in the background.
- R10: Move Mobile to the lower-left icon cluster with the same tooltip behavior.
- R11: Replace the add-project folder-selection experience with a full directory
  browser following the supplied reference: path navigation, home/up,
  breadcrumbs, path/search field, scrolling, cancel, and select folder.
- R12: Make the right Files panel scroll through its complete contents.
- R13: Keep only Refresh in the Files header. File and folder context menus
  retain applicable item actions and both allow new file/folder creation;
  creation from a file targets its parent. Blank-area menus allow only new
  file/folder. Remove upload menu/header actions; retain drag-and-drop upload.
- R14: Support the Git panel for remote projects, operating on the project's
  actual execution host rather than a local directory or unrelated repository.
  Retain the existing changes/history/diff/sync and reachable Worktree Management
  workflows, with their applicable mutation confirmations and source safeguards.
- R15: Discover accounts using the project execution device's `gh` and select
  an account inside Tasks. Keep the project's Tasks selection independent of
  the device's default active `gh` account in both directions. Use that choice
  consistently for Issues and PRs, without introducing a separate login system.
- R16: Give Runtime Agents in Sessions meaningful titles belonging to their
  exact terminal/run, with a distinguishable fallback when no title is known.
- R17: Keep the right context tools (Files, Git, Tasks, Sessions) and the selected
  tool consistent across project/worktree switches. Only the tool selection is
  shared: its contents and source-bound state follow the current worktree. If a
  chosen tool cannot load on a target, keep that tool open with its applicable
  state instead of silently switching to another tool.

## Confirmed Decisions and Constraints

- On 2026-09-06 the user requested discussion before implementation. Consent to
  organize the five work groups is not approval to implement the expanded scope.
- The user's latest confirmation accepts independent per-project Tasks account
  selection and continued monitoring of Mini-Term-owned background terminals.
  Remote credentials stay on the execution host; Mini-Term does not separately
  persist credentials. A revoked or logged-out selected account requires user
  action, not silent substitution of another account.
- The subsequent layout clarification explicitly rejects split/group tabs:
  one worktree has one screen and multiple individual terminals. It also adds
  R17. The user's example is interpreted as keeping Files selected when moving
  from project A/worktree A to A/worktree B or project B; the final wording
  "project management" refers to the same right-side function, not a new page.
- Preserve the earlier worktree policy: invalid entries are hidden by default,
  new valid worktrees appear by default, and hiding is manual and persistent.
  Offline/unreachable does not mean invalid. Project settings changes must
  preserve the existing visibility controls.
- All CI, compilation, tests, fixtures/probes, formatting/lint checks, code
  generation, packaging, and automated verification run exclusively in GitHub
  Actions. No local, container, or manually SSH-dispatched substitute is allowed.
  Source inspection and task bookkeeping are allowed locally.
- Preserve project/worktree/host identity, live PTYs, saved terminal records,
  source fences, and unrelated worktree changes. Legacy split/group metadata may
  remain internally for compatibility, but must not reinstate split/group UI or
  strand a terminal. Main coordinates planning and integration;
  implementation/check dispatch uses Trellis sub-agents only after approval of
  the final plan.

## Task Map

| Child | Feedback ownership |
| --- | --- |
| `09-06-native-agent-ownership-status` | R1, R2, R3, R16 |
| `09-06-native-terminal-navigation` | R4, R5, R6, R7, R8, R9, R10, R17 |
| `09-06-native-file-browser` | R11, R12, R13 |
| `09-06-native-remote-git` | R14 |
| `09-06-native-tasks-gh-accounts` | R15 |

This parent owns source requirements, cross-child acceptance, and the final
planning/integration review. The final scope is approved; child activation and
execution evidence are tracked in `progress.md` and each child task.
The earlier `09-05-project-sidebar-discovery-status` parent and its children
remain separate; prior green Actions runs do not establish acceptance of this
new feedback. Shared `mt-app` files need serialized or explicitly coordinated
ownership in the eventual implementation plan.

## Evidence

- User supplied ten screenshots and numbered feedback 1 through 16 on
  2026-09-06; R1-R16 preserve that mapping. R17 captures the subsequent global
  right-context-tool selection clarification.
- [Agent/status source research](../09-05-sidebar-agent-status/research/09-06-native-feedback.md)
  distinguishes source-confirmed behavior from unverified native symptoms.
- `crates/mt-app/src/tree.rs:209` defines a project panel containing a complete
  split tree; `:276` defines leaf terminal tabs. The removed vertical switcher
  currently switches panels (`crates/mt-app/src/terminals_panel.rs:119`), so it
  cannot simply be removed without exposing all its terminals in the flat tabs.
- `crates/mt-app/src/main.rs:520` stores `ContextPanel` on `Workspace`;
  `:965` switches it and `:1195` renders Files/Git/Tasks/Sessions. This is already
  window-scoped, not per-project preference. Preserve that ownership and verify
  that project/worktree changes retarget content without resetting selection.
- `crates/mt-app/src/git_panel.rs:822` currently renders a remote-not-supported
  state. [Remote Git action research](../09-05-sidebar-agent-status/research/09-06-remote-git-action-surface.md)
  records the child/dialog backend and mutation surface; this is not just auth UI.
- [Flat terminal compatibility research](../09-05-sidebar-agent-status/research/09-06-flat-terminal-compatibility.md)
  establishes a lossless projection while retaining route owners and flags
  selection, close, hidden split, hydration, and persistence pitfalls.
- `crates/mt-github/src/commands.rs:40` currently probes only the active account;
  [account-isolation research](../09-06-native-tasks-gh-accounts/research/gh-account-isolation.md)
  records a feasible request-scoped alternative, not a tested implementation.

## Acceptance Criteria

- [ ] A no-Agent terminal remains quiet across successful background refreshes;
  genuine Agent work/waiting/attention and remote failures remain accurate (R1/R2).
- [ ] Mini-Term-owned background Agents stay visible under their own projects;
  unrelated device Agents and stale terminal incarnations do not appear (R3).
- [ ] Every current-worktree terminal is an individual top tab, including old
  grouped/split terminals; one terminal is shown and background PTYs survive.
  No split controls, duplicate terminal navigation, or overlapping controls remain
  in the approved titlebar/footer interaction (R4-R10).
- [ ] Choosing Files in A/worktree A remains Files in A/worktree B and project B,
  with each target's own files. The same selection rule holds for Git, Tasks,
  and Sessions without sharing their worktree data or Tasks account choice (R17).
- [ ] Tooltip delay is paid once per hover sequence, including toolbar/footer
  movement, and resets after leaving the sequence (R5/R7/R10).
- [ ] Folder navigation, full Files scrolling, row/blank context targets, and
  drag-only upload work on the correct local/WSL/SSH source (R11-R13).
- [ ] Remote Git operations target the remote worktree and preserve existing
  local behavior, errors, and mutation confirmations (R14).
- [ ] Tasks discovers both logged-in device accounts. Projects may choose
  different accounts without changing each other or the device's active
  account; external account switching does not overwrite a Tasks choice (R15).
- [ ] Selected-account failures are explicit, stale responses cannot cross
  account/project boundaries, and remote tokens never return to the client (R15).
- [ ] Runtime titles are meaningful when exact metadata exists and never
  borrowed from another Agent, pane, or historical session (R16).
- [ ] All automated evidence and packaged binaries come from Actions for the
  exact product commit. Native acceptance uses those artifacts, not a local build.

## Out of Scope

- Reverting to default-hidden valid worktrees or imposing a branch-count limit.
- Device-wide external Agent discovery, cross-device credential synchronization,
  a Mini-Term OAuth/login system, or changing global `gh` account selection.
- Copying unrequested Orca project-menu features, changing user shell startup
  files, installing tools for screenshot warnings, or unrelated source/spec churn.
- Split-screen terminal workflows, group tabs, cross-project terminal mixing,
  or synchronizing right-tool contents merely because tool selection is shared.

## Approval Status

Parent and all five children have converged PRDs, designs, execution plans, and
curated implementation/check context for final review. Source-owned behavior and
the single-terminal/global-tool distinction are specified; no blocking product
question remains. Final review must explicitly include the retained flat-tab
ordering and existing remote Git action scope, not only the last clarification.
The user explicitly approved the final summary on 2026-09-06,
including single-terminal tabs, global right-tool choice, retained tab ordering,
owned background Agents, and existing remote Git workflows. Implementation is
authorized in the ordered child plan; activation and execution evidence are
recorded per child. This approval does not waive the Actions-only constraint.
