# Implementation Progress

## Approval and Baseline

- Final parent scope approved by the user on 2026-09-06 after the no-split and
  global right-tool clarification. Implementation is authorized.
- Initial branch: `feat/remote-file-management`; HEAD `2e6660e`.
- Product baseline is clean under `crates/`; index initially empty. There are
  many unrelated dirty/untracked Trellis, hook, config, image and key paths.
  Preserve them; never blanket-stage the repository or `.trellis`.
- Actions owner: `vihor3/mini-term` (`fork` remote), not upstream `origin`.
  CI workflow ID `343499026`; Windows Package workflow ID `349566481`.

## Delivery Status

| Child | State | Evidence |
| --- | --- | --- |
| native-terminal-navigation | Actions corrections | `d50f616` format/i18n diagnostics applied; Windows menu type error under correction |
| native-agent-ownership-status | Implementing | Activated; lower-layer implementation dispatched without overlapping navigation files |
| native-file-browser | Implementing | Activated for disjoint onboarding/browser slice while Agent integration continues |
| native-remote-git | Pending | Approved, not activated |
| native-tasks-gh-accounts | Pending | Approved, not activated |

## Current Dispatch

Main active task: `.trellis/tasks/09-06-native-file-browser`.
Navigation remains open pending Actions and native acceptance, not archived.
Agent implementation remains in progress under the explicit owners below.
The default serialized delivery order is relaxed only for disjoint onboarding
files; shared store/main/execution-host ownership and integrated review remain
serialized. This changes scheduling, not the approved product scope.

Current lower-layer implementer: `01a07372-7fd4-7420-9760-88ef3afa4b70`
(Epicurus), owning `mt-ai/src` plus focused tests, `mt-ssh/src/agent.rs` and
narrow exports/tests, and `mt-pty/src/ssh.rs` plus needed exports/tests. It may
not edit mt-app/config/layout/ui, workflows or task metadata. It must hand off
bounded ownership/activity APIs in the Agent child's `lower-layer-handoff.md`.
App implementer: `01a07379-8615-7cd1-a4a2-68701d4cbb35` (Rawls), owning
`store/{remote_agents,ai,context}.rs`, catalog/sidebar/Sessions/remote_ssh and
narrow store declarations. It coordinates lower-layer APIs directly with
Epicurus and records `app-handoff.md`. Navigation formatting was staged before
this dispatch, keeping its CI correction separate from new Agent changes.
Resumed navigation checker James owns only `menu.rs` and the unused search-bar
import for the current compiler diagnostic; there is no write overlap.
That focused correction is now source-complete and James is closed again;
`ci-followup.md` records the explicit SharedString comparison and import fix.

Onboarding implementer `01a0737c-6d5f-7152-9936-5707a1867101` (Raman) owns
`remote_directory_picker.rs` and `project_onboarding/` plus narrowly scoped
locale sources, recording `onboarding-handoff.md`. It may not edit FileTree,
store/main, remote_ssh, execution-host code, specs or workflows. Agent owners
were notified that their captured task/scope remains unchanged by this pointer.

### Completed Navigation Dispatches

- State implementer `01a0730b-46a5-7752-91f2-bcac7a659d63` (Avicenna) implemented flat
  terminal layout/persistence, selection/order, store lifecycle and fork/close
  source fencing in `pane_actions.rs`. Main narrowed its scope before UI files
  changed; it never edited titlebar/body/main/hotkeys/workbench files. Its slice
  is complete and the implementer is closed; fourteen regressions are authored,
  unrun. Reviewer `01a0732b-cdfe-7931-917c-d1c57e990b59` (Zeno) now owns only
  those state/lifecycle/persistence files and records `core-review.md`.
  That review is complete and the reviewer is closed. Three new regressions and
  two strengthened ones are authored, unrun. It found dormant-hosted-close and
  shellless-reconnect gaps requiring the follow-up owner below.
- UI implementer `01a0730b-473f-72a1-b4cb-2b3099dad015` (Archimedes) owns
  `orca_sidebar.rs`, shared tooltip support, optional `activity_bar.rs` extraction,
  and narrowly needed menu anchoring. It does not edit main/titlebar/terminal
  area/store. Its tooltip/Mobile event API is handed to the core implementer.
  Implementation is now complete and that agent is closed. Fourteen pure
  regressions are authored, unrun. Reviewer `01a07327-8943-7030-85bc-3bbcd8f33159`
  (James) completed source review/local fixes and is closed. It added explicit
  focus-handle tab stops, window-exit/occlusion tooltip resets and deferred
  draw-time clearing; fifteen total pure cases are authored, unrun. Its titlebar
  tab-stop finding has been assigned to Franklin. Automated evidence remains
  pending Actions.
- Tooltip handoff is in the navigation task's `tooltip-handoff.md`: one retained
  `Entity<mt_ui::icon_tooltip::IconTooltips>` per icon group, `button`/`group`
  wrappers and explicit `reset`. Core has been sent the API and the
  `OrcaSidebarEvent::OpenMobile` main-wiring requirement.
- Terminal-view implementer `01a07316-cd58-78b1-ba3b-2458db236ce8` (Ramanujan)
  implemented main/titlebar/terminal area,
  workbench/hotkeys/dnd/pane UI and necessary terminal/fork locale sources. It
  consumes state and tooltip APIs without editing those owners' files. The
  state API handoff is `core-handoff.md`; view owner records `view-handoff.md`.
  It is complete/closed with four new regressions authored, unrun. Reviewer
  `01a07333-5c49-7582-8e58-5c853081645e` (Franklin) now owns that view slice and
  full-navigation source integration review, recording `view-review.md`.
  Main also authorized a narrowly scoped `modal.rs::open_rename_pane` deferred
  target fence and the obsolete `tree.rs` DropZone doc link; no other reviewer
  owns those files. Main's startup first-leaf focus finding was fixed by the
  implementer before handoff.
  Franklin completed/closed after the full-navigation source review. It fixed
  modal rename target fencing, titlebar tab stops and drag focus, invalid numeric
  shortcut page switching, search-bar click propagation and stale comments, and
  authored one additional bilingual regression (unrun). The two lifecycle
  findings remain pending, plus the tool/settings follow-up below.
- Main briefly proposed narrowing ordinary dormant hydration, then retracted it
  after rereading the approved compatibility design. Preserve ordinary original-
  owner recovery; only exact-live activation, inventory and background close have
  the no-additional-hydration rule. Both reviewers received the correction.
- Lifecycle implementer `01a0733f-c26c-7443-b2cb-d5d0f63d19a1` (Halley) owns
  `pane_actions.rs`, `store/{panes,ssh}.rs`, narrowly `store/mod.rs`, and any
  needed exact-cold-close change/tests in `mt-terminal-host/{server,history}`.
  It addresses dormant close without hydrating and shell resolution before
  reconnect teardown; no hydration-policy change. Existing Kill's SessionMissing
  path does not purge cold history, so main relayed that evidence and authorized
  a narrow host-side fix if needed without changing the wire protocol casually.
  Main also authorized the one-field store/projects.rs test initializer update.
  Additional review notes require same-session closing reservations through
  host cleanup and distinguish harmless same-owner selection/order changes
  from actual route or conflicting-alias changes during asynchronous close.
  Handoff: `lifecycle-handoff.md`.
  Implementation is source-complete and Halley is closed. Fourteen new tests
  and two updated restore-race tests are authored, unrun. Main accepted the
  conservative disabled-host/unknown-history policy and documented the new
  app/host boundaries. Resumed Zeno now owns the final full-navigation source
  check, primarily the lifecycle changes, and records `final-review.md`.
  Zeno completed and is closed. It fixed cleanup-error reservations, a
  pre-existing divergent-alias close overwrite, and pending-close navigation
  before scope mutation. Three more tests and one strengthened test are authored,
  unrun. The final report finds no remaining concrete scoped blocker.
- Tool follow-up implementer `01a07341-8bf6-7910-8873-4161cce1d91e` (Lovelace)
  owns only `mt-ui/src/terminal/search_bar.rs` and
  `mt-app/src/settings/pages_terminal.rs`: adopt shared icon hover behavior in
  search controls and remove the obsolete transition toggle while preserving
  saved configuration. Handoff: `tools-followup-handoff.md`.
  It is complete/closed. Resumed reviewer James completed the focused review
  and is closed again: fixed search-button wrappers to centered flex so the
  absolute tooltip anchor covers each child Button, with one new pure style
  regression. `tools-followup-review.md` records no further scoped defect;
  all tests and native behavior remain unrun/pending Actions and artifacts.
- `core-handoff.md` is available and has been relayed. It exposes
  `terminal_tab_views`, full-target activation/reorder and
  `pane_actions::close_terminal_target`; UI callers must retain captured targets.
- All navigation implementation/check agents listed above are closed. Only its
  Actions jobs remain active. Main owns specs, metadata, CI fixes/review dispatch,
  scoped commits and Actions while lower-layer Agent work proceeds separately.

## Execution Constraint

No local build, Cargo metadata, test, fixture/probe, formatter, linter, generator,
whitespace check, app launch, or automated verification. Author tests locally,
execute only in Actions. No user credential or real remote Git mutation probes.
Source review is not CI evidence. Do not mark native acceptance complete without
the matching Actions artifact and the user's observed result.

## Validation

First scoped candidate: `d50f616034f8d17b5100be68e804c66f0af90edc`, committed and
pushed to `fork/feat/remote-file-management`. Git hooks were disabled per command
to prevent local verification. No unrelated initial dirty files were staged.

- CI: https://github.com/vihor3/mini-term/actions/runs/33992637822
- Windows Package: https://github.com/vihor3/mini-term/actions/runs/33992637938
- Both refer to `d50f616`; no result/artifact acceptance is claimed yet. Earlier
  green runs and the old `1.2.2-ci.30` installer are not evidence for these changes.

The first CI completed with failures. Linux stopped at changed-line rustfmt and
generated i18n gates, so Linux compilation/tests did not execute. Windows reached
the affected-package compile and failed one `menu.rs` anchor comparison type
error. Main downloaded and mechanically applied the Actions diagnostic patches
under `~/.cache/mini-term/actions/33992637822/`; the i18n patch changes four
split/pane captions to terminal captions. No formatter or generator ran locally.
The menu error and directly related unused search import are assigned to James.
Windows packaging is still running on the first candidate, not accepted.
Main has updated the existing workbench identity and worktree layout specs for
the implemented flat navigation boundary. Reviewer/source follow-ups may refine
them; documentation is not execution evidence.
The terminal-host and shared-tooltip contracts are also updated. Main added a
Windows terminal-host all-target test step to existing CI after confirming the
unit fixtures choose `cmd.exe` on Windows; Unix IPC integration stays Unix-only.
No workflow, test or fixture was executed locally. Apply only Actions-produced
formatting/i18n diagnostic patches and coordinated source fixes, then obtain
fresh exact-commit evidence before claiming validation.
