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
| native-terminal-navigation | CI/package passed / Native pending | Full CI33997336854 and package33997336860 passed for `f7b8a7e`; ci.36 artifact accepted, native interaction remains open |
| native-agent-ownership-status | Reviewing | App source complete under Bohr's combined check; lower correlation/silence follow-ups underway |
| native-file-browser | Reviewing | Onboarding source-complete; FileTree review extends exact epoch/provenance through retained operations |
| native-remote-git | Implementing | Domain independently reviewed, 35 tests authored; host adapter implementing; Git UI not dispatched |
| native-tasks-gh-accounts | Implementing | Domain source-complete, 20 tests authored; dedicated executor and separate Tasks app/config integration underway |

## Current Dispatch

Main active task: `.trellis/tasks/09-06-native-remote-git`.
Navigation remains open pending Actions and native acceptance, not archived.
Agent implementation remains in progress under the explicit owners below.
The default serialized delivery order is relaxed only for explicitly disjoint
source slices. Shared store/main/execution-host ownership and integrated review
remain coordinated. This changes scheduling, not the approved product scope.

Current lower-layer implementer: `01a07372-7fd4-7420-9760-88ef3afa4b70`
(Epicurus), owning `mt-ai/src` plus focused tests, `mt-ssh/src/agent.rs` and
narrow exports/tests, and `mt-pty/src/ssh.rs` plus needed exports/tests. It may
not edit mt-app/config/layout/ui, workflows or task metadata. It must hand off
bounded ownership/activity APIs in the Agent child's `lower-layer-handoff.md`.
Epicurus is now source-complete and closed. Lower-layer checker
`01a0739f-6b07-71b1-a29d-99eb0bb1f750` (Leibniz) owns that same bounded source/
fixture scope and `lower-layer-review.md`. It must not edit app/spec/workflow;
main forwards any API/finding notes to Rawls. The implementation fixtures and
tests are authored only, with no executed results yet.
App implementer: `01a07379-8615-7cd1-a4a2-68701d4cbb35` (Rawls), owning
`store/{remote_agents,ai,context}.rs`, catalog/sidebar/Sessions/remote_ssh and
narrow store declarations. It coordinates lower-layer APIs directly with
Epicurus and records `app-handoff.md`. Navigation formatting was staged before
this dispatch, keeping its CI correction separate from new Agent changes.
Main also authorized narrow `pane_actions.rs` and `title_bar.rs` changes for
exact-owned Agent liveness/title integration, preserving all navigation fences.
After reading its draft handoff, main also authorized `agent_activity.rs`
freshness presentation/fixture, `store/projects.rs` necessary test initializer,
and a narrow `pane.rs`/`store/panes.rs` real title-event capture path. Its needed
remote title lookup lives in `remote_ssh/sessions.rs`, not general SSH exec.
Rawls completed and is closed. Combined app reviewer Bohr
`01a073cb-f945-7173-a9d6-ebcdfe8b97fc` owns that app slice and `app-review.md`.
Main additionally authorized Agent freshness rendering only in `main.rs` and
`jump_palette.rs`; module registration lines remain with their separate owners.
Bohr found a pre-title in-flight inventory incorrectly usable as a post-title
foreground sample and is adding a request-scheduling-time fence. Lower fixes
and the remaining monitor silence inference are coordinated with Leibniz.
Resumed navigation checker James owns only `menu.rs` and the unused search-bar
import for the current compiler diagnostic; there is no write overlap.
That focused correction is now source-complete and James is closed again;
`ci-followup.md` records the explicit SharedString comparison and import fix.

Onboarding implementer `01a0737c-6d5f-7152-9936-5707a1867101` (Raman) owns
`remote_directory_picker.rs` and `project_onboarding/` plus narrowly scoped
locale sources, recording `onboarding-handoff.md`. It may not edit FileTree,
store/main, remote_ssh, execution-host code, specs or workflows. Agent owners
were notified that their captured task/scope remains unchanged by this pointer.
Its draft handoff found unpinned remote browsing. Main authorized browser-only
`remote_ssh/dirs.rs` changes plus existing `RemoteProjectContext`/session-check
helper visibility in `project_ops.rs`; no mutation semantics or generic exec
changes. `pub use dirs::*` already exports the resulting API.
Raman is now source-complete and closed. Its full browser and epoch-pinned
listing have authored regression coverage only. An independent source check,
Actions gates and native acceptance remain open; its handoff explicitly records
the existing SFTP readdir materialization and opaque error limitations.

FileTree implementer `01a0737d-cad5-7f51-b3d9-8cf32bf2c7ae` (Parfit) owns
`file_tree/`, narrowly the main Files sizing chain if needed, and FileTree locale
sources. It records `file-tree-handoff.md`; no store/host/onboarding ownership.
Parfit is now complete and closed. Reviewer Pauli
`01a073b9-fd9f-77f2-9d28-fd32e7eadd70` owns those files and the narrow
`remote_ssh/transfer.rs` epoch-pinned upload variants needed by FileTree. Legacy
upload callers are preserved. It records `file-tree-review.md`.
Its first review fixed upload pins, clipboard ownership and checked remote path
text but found three P1 end-to-end gaps. Main extended its ownership to Files-only
`dirs.rs` listing/create/rename, `delete.rs`, copy/download in `transfer.rs`, and
narrow `file_ops.rs` if required. Preserve Raman's browser API and legacy callers.
Producing epoch/cache provenance, pinned retained mutations/download prompts,
and exact operation/presentation ownership through ABA are being repaired.
Switching sources must never clear a dispatched operation's busy reservation.

Git domain implementer `01a0737f-42ef-73f2-8e2a-49b1ec476efb` (Turing) owns
only `mt-project` Git plans/parsers and narrow DTO/diff exports/tests, recording
`domain-handoff.md`. It must hand off transport-free APIs early. No dependency,
app/SSH, workflow or spec edits are assigned; app integration is not dispatched.
That initial domain slice is complete and released to reviewer McClintock
`01a073bc-d8ba-77c2-9822-07fc11343632`, limited to `mt-project` Git files/tests
and `domain-review.md`. Turing now owns new `git_backend` and
`remote_ssh/git_ops.rs` modules, narrow execution-host helpers and module wiring.
It records `backend-handoff.md`; full Git UI and worktree finalization are a
later separately owned slice. No UI remote guard is enabled prematurely.
McClintock completed the domain follow-up and is closed. The new
`RepositoryStatus.untracked_directories` preserves exact trailing-slash entries
outside file-action DTOs; files/directories share duplicate/count validation.
The 35 tests remain unrun. Main relayed local-adapter projection obligations.

Tasks domain implementer `01a07384-8b00-7363-9f9b-84b36d2b9c8e` (Copernicus)
owns only transport-free `mt-github` account plans/models/parsers/errors and
focused tests, recording its child's `domain-handoff.md`. No process/credential
handling, config/app/SSH changes or real-account probes are authorized. App
selection and the dedicated secret executor are not dispatched yet.
Main interrupted/parked this owner after its source/17-test authorship handoff
to free the six-agent limit for the urgent CI review. It is closed, NOT declared
feature-complete. Resume it after the blocker review for its final source pass,
or route its explicitly unfinished handoff to a checker before app integration.
Copernicus is now resumed for its domain final source pass, then new
`tasks_account_executor` and narrowly `remote_ssh/tasks_accounts.rs` plus module
wiring. It may not edit Turing's execution_host.rs; helper requests go through
main. Config/Tasks UI/store and generic SSH libraries remain outside its scope.
The domain final pass is complete with 20 authored tests, released for later
independent review. Executor API is declared in `tasks_account_executor`; Turing
has exposed only existing `ProcessTree` cleanup visibility for its private
credential pipes. Native stays Rust-only; WSL/SSH host isolation requires
Python 3.8+ and reports missing capability rather than falling back.
Tasks app implementer Herschel `01a073d6-79fa-7052-90c0-ecec15de3cfc` now owns
`github_tasks.rs`/private children and narrow mt-config selection DTO/default/tests.
It uses existing `AppStore::patch_config`, not shared store edits, and records
Tasks `app-handoff.md`. Requests must retain selected-account/source/generation/
epoch ownership; both directions of global gh synchronization remain forbidden.

Resumed James `01a07327-8943-7030-85bc-3bbcd8f33159` now owns only the changed-
rustfmt diagnostic script, focused Node tests, the corresponding added Actions
test step, and a navigation formatter follow-up report. It audits the committed
navigation-only `d50f616..c90ff13` delta against the original full artifact for
partial-hunk damage; source fixes outside that write set are coordinated through
main. Rawls owns and has restored the two known panes.rs defects below.
James completed and is closed. Both diagnostic paths now contain complete U3
patches; four actual production-script/application fixtures and their Actions
step are authored. The bounded 26-file source audit found no further damage.
Main records the prevention contract and adds Windows Files/browser test steps.

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

Navigation diagnostic correction `e210093822c7e582346fd7de947bc3901aa0feb0` was
scoped/committed/pushed with hooks disabled, excluding the unstaged Agent work.
CI: https://github.com/vihor3/mini-term/actions/runs/33993374132
Windows Package: https://github.com/vihor3/mini-term/actions/runs/33993374111
The older package run was cancelled by the newer push. The second Linux job
passes generated i18n but still reports changed-line formatting; its patch is
being downloaded. Windows checks/package are in progress. No complete quality
gate or installer is accepted.

`c3eefa7cb6c7da4c4f99b5ef98d5824160ec30da` applies the two remaining blank-line
diagnostics from the second run. CI is
https://github.com/vihor3/mini-term/actions/runs/33993613596 and package is
https://github.com/vihor3/mini-term/actions/runs/33993613614. This push cancelled
the second run. Linux still finds one titlebar blank-line hunk because changed-
line diff alignment shifted. Main inspected its Actions-generated full patch:
only one old Polyline wrap and two nearby duplicate blank lines remain. That
three-hunk titlebar patch is mechanically applied/staged without staging Rawls's
new Agent edits in the same file. The remaining historical formatting is left
alone; no local formatter/check was run and no gate was weakened.

`c90ff131039792c5d1202749ccf3841846e9f93e` commits that exact three-hunk
Actions titlebar patch plus scoped activation/progress bookkeeping. Agent,
Files and domain edits remain unstaged. CI is
https://github.com/vihor3/mini-term/actions/runs/33993926699 and Windows Package
is https://github.com/vihor3/mini-term/actions/runs/33993926710; both are pending
results and supersede the cancelled third run. Main has started updating the
mt-ai successor contract from the implemented semantic acceptance API; final
source review may refine it, and it is not execution evidence.

The fourth candidate's format/i18n gates pass. Latest observed CI jobs
`101380936776` (Linux) and `101380936899` (Windows) are compiling. No completed
tests, full CI result or package acceptance has been reported for `c90ff13`.

Subagent messaging is unavailable inside the implementer tool sets. They write
early API/blocker notes to handoffs; main must read/forward those notes instead
of assuming peer `send_input` worked. Main forwarded lower-layer Agent APIs to
Rawls and resolved the app title/liveness and browser epoch write-scope requests.

The fourth CI completed FAILING at Linux/Windows compile. All tests/lint after
compilation remain unexecuted. Diagnostics: Linux job `101380936776`, Windows
job `101380936899`. The first partial `-U0` artifact omitted the insertion side
of a moved `use super::{AppStore, ProjectState, TerminalJumpTarget}` in panes.rs;
it also placed a lifecycle-test `states` declaration after its use. The result
produced 82/86 cascading app errors, not proof of 82 independent defects. Main
disclosed the artifact-application mistake to the user and asked Rawls to restore
both; source inspection now confirms those two fixes, still unstaged.
James must change machine-applicable diagnostics to complete full-context
rustfmt output for affected files while preserving changed-line-only gating,
and author actual patch-application regressions for Actions. Main must not apply
the old partial-hunk artifacts again. No local formatter, Node test, compilation
or automated verification has run. All currently unstaged feature work is kept.
Rawls's two corrections were manually isolated into a full-context index-only
staging patch at `~/.cache/mini-term/actions/33993926699/navigation-source-repair.patch`.
The concurrently added `PaneEvent::TitleChanged` branch is excluded. Main
committed/pushed those two restorations separately while formatter source review
continues. The fourth Windows package run failed at GPUI compilation as well.
Main has updated the existing workbench identity and worktree layout specs for
the implemented flat navigation boundary. Reviewer/source follow-ups may refine
them; documentation is not execution evidence.
The terminal-host and shared-tooltip contracts are also updated. Main added a
Windows terminal-host all-target test step to existing CI after confirming the
unit fixtures choose `cmd.exe` on Windows; Unix IPC integration stays Unix-only.
No workflow, test or fixture was executed locally. Apply only Actions-produced
formatting/i18n diagnostic patches and coordinated source fixes, then obtain
fresh exact-commit evidence before claiming validation.

`4947f5542636d4ca95c487772c941ffca53b4181` commits just the two panes.rs
source restorations. CI `33996331503` and Windows Package `33996331532` refer to
that exact SHA. Linux format/i18n, staging tests, locked graphs and workspace/
sidecar compilation pass; Windows affected-package compilation passes. Linux
Clippy job `101387417299` reports sidebar redundant closure, store tests before
production items, and one needless test borrow. Rawls owns these files and has
the exact three corrections. Later Linux tests are skipped, not passed. Latest
Windows job `101387417542` is running onboarding tests; package job
`101387417437` is building the app. No full gate or installer is accepted.

Agent lower review fixed rejection epoch side effects, unchanged weak-poll
presentation identity, semantic capture epoch, and bounded probe framing/races.
Main approved its remaining P1 correlation repair: weak process evidence cannot
borrow an unbound Hook by provider alone. Exact process/session proof is needed;
singleton weak-PTY upgrades require pre-batch uniqueness, and independent
same-provider processes must survive in either order. Leibniz continues that
bounded follow-up and authors production-login execution coverage for Actions.

Main committed the complete formatting-artifact producer, four Node fixtures and
Windows Files/browser test steps as `8f234c0`, then isolated Rawls's three Clippy
repairs as `f7b8a7e74dc901203b1fd4606310a8ad78f3503f`. The index-only staging
patch under `~/.cache/mini-term/actions/33996331503/` excluded both runtime title
maps and all new sidebar Agent behavior. The worktree retained those additions.
Both commits are pushed to the fork. New CI `33997336854` and Windows Package
`33997336860` supersede cancelled `4947f55` runs. Latest observed Linux job
`101390060323` passes all four artifact fixtures, format/i18n, staging/graphs,
workspace/sidecar compilation and Clippy, and is running workspace tests.
Windows job `101390060222` passed affected-package compilation and is running
onboarding tests. No completed overall CI or accepted installer exists yet.

Main authored combined Files locale integration: 13 browser keys plus 3 target
errors in `USED_KEYS`, expected dictionary count 952 -> 968, and a Windows
transfer epoch-test step. These remain unstaged with Files; the generated
dictionary is untouched until an Actions-produced diagnostic patch is available.
Main also updated the onboarding browser and lower Agent contracts, and added
the source-backed Git CLI contract. Source documents do not establish execution
evidence, and the remaining app/SSH boundaries are still being implemented.

Navigation CI `33997336854` is now SUCCESS for `f7b8a7e`. Linux job
`101390060323` completed all gates at `2026-09-05T23:13:21Z`; Windows job
`101390060222` completed all gates at `2026-09-05T23:21:14Z`. The navigation
child's new `validation.md` records exact scope and native gaps. Package run
`33997336860` is still pending at this checkpoint. Passing baseline Files or
Tasks tests in that candidate does not validate uncommitted new feature code.

Main committed the independently reviewed CLI domain as `71af516`, retaining all
35 authored tests and its contract/handoffs. It is not pushed yet, to avoid
cancelling the navigation package before its result. Turing now also owns narrow
new mt-project local read helpers so remote parity does not silently require
system Git for formerly libgit2-only local reads; the committed CLI domain is
otherwise released and unchanged.

The navigation package also completed SUCCESS at `2026-09-05T23:21:50Z`.
Artifact `9978757862` (`Mini-Term_1.2.2-ci.36_windows-x64`) and its manifest were
downloaded under `~/.cache/mini-term/artifacts/f7b8a7e-navigation/`. The manifest
has the exact f7b8a7e commit/run and passed staged/extracted payload validation;
main did not hash-check or launch it locally. A clearly navigation-only download
link was shared with the user; native acceptance still requires observations.
After both runs finished, main pushed `71af516e938003a678d8f532b79bb51f43aa6c60`.
CI `33998592134` and package `33998592120` now validate the reviewed CLI domain
candidate, not the uncommitted Git app adapter or other feature slices.
