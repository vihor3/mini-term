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
| native-agent-ownership-status | Linux full tests passed / Native pending | At `0e141f0`, mt-ai 244 and mt-app 1205 passes include producer/lifecycle/ownership regressions; full Linux job101422396717 passed |
| native-file-browser | Linux/Windows/SSH passed / Native pending | Windows onboarding and Files steps passed at `0e141f0`; actual authenticated SSH Files/browser fixtures each ran and passed |
| native-remote-git | Linux/Windows/SSH passed / Native pending | At `0e141f0`, Windows Git app 124 passed, full Linux suite and both actual SSH Git fixtures passed |
| native-tasks-gh-accounts | Native/SSH passed / WSL cwd repair | At `0e141f0`, Windows executor 12 and execution-host 10 passed, actual SSH account pipeline passed; actual WSL matrix isolates launcher cwd before authentication |

## Current Dispatch

Main active task: `.trellis/tasks/09-06-native-remote-git`.
Navigation remains open pending native acceptance, not archived; its exact
candidate's Actions CI and packaging are complete.
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
Bohr also owns the narrow `branch_family.rs` history-source capture and `ai.rs`
source event/test consumers needed by the new episode contract. History jumps
must distinguish absent, exact, ambiguous, and mismatched owners; ambiguity
cannot select the first run or silently resume into another terminal.
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

CLI candidate `71af516` stopped at Actions formatting; its runs were superseded
after main applied the complete three-file U3 artifact and pushed `57d8196`.
CI `33998882225` and package `33998882220` own that candidate. Linux formatting,
i18n, locked graphs and compilation pass; remaining gates are pending at this
checkpoint. No local formatter or fixture executed.

Lower Agent and app reviewers are implementing the approved lifecycle follow-up:
real input mints an opaque weak-detection episode; status/session events and
scheduled inventories carry it. Accepted stronger exact-route evidence records
sticky fallback supersession, not an identity merge or invented process exit.
All app liveness/title/close/count projections must exclude superseded audit
rows. Event receipt time, fresh polling and a new SSH epoch cannot revive an old
episode; a genuine later source episode can create a new fallback identity.

Main authored an Actions-only disposable loopback SSH step, with early cleanup,
isolated client/server homes, keys below RUNNER_TEMP, synthetic gh environment,
and explicit ignored Git/Tasks transport tests. Turing owns the shared read-only
fixture configuration helper in remote_ssh/git_ops.rs; Files and Tasks consume
it without creating a second setup framework. The setup and new fixtures remain
UNRUN and unstaged with their source. Nothing connects to the user's devices.

Copernicus completed its executor source handoff (10 authored tests, including
native Windows process ownership and an ignored authenticated SSH fixture) and
released its files. Domain/executor independent review and Actions are still
open. WSL/SSH require Python 3.8+; native Windows does not. Main is releasing the
slot for Git UI integration; Tasks app/config ownership remains with Herschel.

Kepler `01a073fa-ed3f-7f42-909f-3748951f8f1b` now owns the five Git UI modules
(`git_panel`, `git_changes`, `git_history`, `git_diff`, `git_worktree`) and their
private children/tests. External legacy callers require coordinated updates,
not unannounced signature changes. Backend/domain/store/Files/Tasks/navigation
are outside its write set. It records an early `ui-handoff.md` and consumes the
actual backend API. All reachable worktree actions remain part of this delivery.

CLI CI `33998882225` Linux job `101394109884` passed formatting, compilation
and Clippy, then found one failing actual-history parity test: CLI body retains
`root body\n`, while existing libgit2 yields `root body`. mt-project reports
169 passed / 1 failed. Main authorized Turing's narrow CLI parser/test correction;
do not weaken the parity assertion. Windows and package jobs are still running.

Herschel completed the Tasks app/config source handoff (17 app and two config
tests authored, UNRUN) and is closed. Noether
`01a07400-eef7-7f23-a583-3d13a4121f25` now owns combined independent Tasks
domain/executor/app/config review and bounded fixes, including the actual SSH
fixture, with no shared execution-host/store/Git/navigation ownership. Its
report is `native-tasks-gh-accounts/review.md`; main owns specs and CI.

Pauli's FileTree follow-up is source-complete: producing listing provenance,
retained mutation/download pins and independent busy/presentation ownership.
Main added the exact ignored SFTP test and Windows delete/config filters to
draft CI. Pauli now continues independent R11 review of Raman's picker and
onboarding slice, preserving Files APIs and the browser interface now consumed
by Git UI. Report: `native-file-browser/onboarding-review.md`. Native geometry,
mid-dispatch transport interruptions and all exact-SHA Actions gates remain
separate from source review.

Turing's backend/new-local-helper source handoff is complete and its slot is
closed. Resumed McClintock `01a073bc-d8ba-77c2-9822-07fc11343632` first checks
the separately stageable two-file CLI body repair, then independently checks
the new host adapter/local helper and loopback fixture. Git UI remains Kepler's
disjoint write set. Reports: `cli-parity-review.md` and `backend-review.md`.

CI `33998882225` is completed FAILURE: Windows job `101394109787` passed;
Linux's sole reported failure is the history-body parity assertion. Package
`33998882220` completed SUCCESS, but that installer is not a passing full gate
or evidence for any uncommitted slice. The accepted navigation-only artifact
remains the previously shared f7b8a7e candidate.

Main revised the draft loopback sshd to `UsePAM yes`, keeping password and
keyboard-interactive authentication disabled. This avoids the portable OpenSSH
non-PAM locked-account gate without unlocking/mutating a runner account;
[auth.c](https://github.com/openssh/openssh-portable/blob/V_8_9_P1/auth.c#L127)
contains that condition. Explicit isolated HOME/PATH `SetEnv` is applied after
PAM environment import in
[session.c](https://github.com/openssh/openssh-portable/blob/V_8_9_P1/session.c#L1084).
The draft is still UNRUN; an authenticated fixture pass must confirm setup.

Main committed/pushed the independently reviewed two-file CLI body repair plus
its source contract/report as `68b6a60238f98290f68e20d478d8c5a1ccd965c9`.
CI `34000677142` and package `34000677088` own that narrow candidate. All new
Agent/Files/Tasks/host-adapter/Git-UI work stays unstaged. No hooks or validation
commands ran locally.

Kepler's Git UI ownership is now narrowed to panel/changes/history/diff and
their private helpers/tests. Mill `01a0740e-0933-7001-bf04-0eb98b6da1d2` took
over the already partially edited `git_worktree.rs` without resetting it, and
owns a new `store/git_worktree_cleanup.rs`, its module/export lines, narrow
project-location matcher reuse, and close-request accessor visibility. It
preserves Agent fields/test initializers and flat navigation behavior. Existing
`open_repository(GitRepository, on_changed, window, cx)` is retained; the new
opaque cleanup guard cannot treat a terminal-close focus-handoff bool as success.
It must prove exact captured target absence and empty unchanged alias inventories
before Git/config finalization. See `worktree-handoff.md`.

Leibniz's lower source review is complete and its slot is closed. Bohr's final
app pass found an additional real race: an old accepted empty inventory could
unconditionally clear a later published OR pending input detection episode.
Main authorized Bohr's narrow tracker conditional-clear API/locking and its app
consumer/tests, followed by independent lower review. Compare-and-clear must be
atomic against input/echo promotion, not a check followed by plain clear.
Main also clarified that genuinely later different-provider episode B is not
the same-episode fallback alias A: older Hook evidence cannot supersede B merely
because it remains retained. B stays Unknown until owned semantic/proof evidence;
same-provider identity ambiguity is still rejected rather than guessed.

Draft Windows CI additionally runs account-domain/config tests and Git CLI,
native-libgit2 and app Git regression filters. All new steps and transport
fixtures remain UNRUN until the corresponding integrated source is committed.

CLI follow-up `68b6a60` stopped at one Actions formatting hunk in the new
history fixture. Main inspected/applied the complete U3 diagnostic artifact,
then committed/pushed `2c8d1f1408f04b204c0b87d00208edb384a20b0f`.
CI `34000949862` passed Linux formatting/i18n/check/Clippy and Windows compile
at the latest poll; tests and package `34000949832` are still in progress.
No local formatting or test execution occurred. These runs cover the narrow
CLI candidate, not the still-uncommitted feature integration.

Bohr's app source handoff is complete and its slot is closed. Resumed Leibniz
`01a0739f-6b07-71b1-a29d-99eb0bb1f750` independently reviews only the new
conditional tracker-clear locking/API, its inventory consumer and matching
regressions. The already completed runtime matching/probe policy is not being
redesigned. Source completion is not an Actions/native acceptance claim.

McClintock completed the Git backend review and released those source files.
The new read-only `GitBackend::repository_for_file` selects the nearest actual
repository for a literal project-relative file, including nested/deleted-file
cases, without a remote-to-local fallback. Kepler consumes the API. Pauli owns
the separate FileTree caller correction that preserves POSIX backslashes.
Pauli is also authorized to make `probe_connection` obtain authenticated Home
directly from its current SFTP session, bypassing the connection-ID-only legacy
home cache without changing unrelated cache consumers. The exact authenticated
browser fixture has been added to draft CI, still UNRUN.

Noether continues the approved explicit foreground Tasks cache revalidation and
narrow WorkItem activation hooks. Main authored an isolated actual-WSL CI gate:
a checksum-pinned Canonical rootfs plus same-run synthetic gh ELF artifact,
Windows 2022 import into a unique run/attempt-owned WSL 1 distro, exact fixture
discovery/execution and per-distro cleanup. The marker contract is the CURRENT
Tasks review.md schema/kind/run/repository/sha object, not an earlier draft.
Noether owns the wrapper and actual Windows test; main owns CI integration.
McClintock now checks only that CI setup and shared loopback source, recording
`native-tasks-gh-accounts/ci-review.md`. No real credential, user distro, SSH
device or local validation command is used.

CLI candidate `2c8d1f1` is now CI and package SUCCESS: `34000949862` Linux
`101399624211` and Windows `101399624267`, plus package `34000949832`.
Linux mt-project reports 172 passes/zero failures, with the original merge-
history DTO parity and both body-whitespace regressions explicitly passing.
Artifact `9979775690` is `Mini-Term_1.2.2-ci.40_windows-x64`. Scope is recorded
in the Git child's `validation.md`; it does not cover any uncommitted slice.

Pauli released completed R11/FileTree follow-up source. Kepler released all
four Git UI modules and closed; Mill released Worktree Management plus exact
store cleanup and closed. Pauli now owns independent four-Git-UI check;
resumed Bohr owns only independent Worktree/store check. Reports are `ui-review.md`
and `worktree-review.md`. The remote guard stays until reviewed routing is
confirmed. Files APIs/source and completed backend/local helpers are released.

The Agent conditional-clear review found a second producer boundary: delayed
last-Hook SessionEnd still unconditionally cleared a later unhooked detection.
Main read the actual handler and authorized Leibniz's bounded captured Hook-
session/episode lifecycle fix and production-handler regressions. Repeated
older Hook events must not borrow the new input episode or overwrite its weak
detection either. Wire payload/route/port and runtime matching policy remain
unchanged. Agent commit is held for this concrete source blocker and subsequent
independent producer review; no passing Agent gate is claimed.

Noether released the completed Tasks cache/activation/WSL follow-up and closed.
Its report now records 24 app and 12 cross-platform executor authored functions
(not passes), plus 20 domain and two config cases. McClintock released reviewed
CI setup and closed: exact ignored discovery, isolated SFTP initial cwd,
same-run WSL provenance, ELF header and bounded only-owned cleanup. No new
fixture has run yet. Main updated the Tasks foreground-access contract.

Main's direct producer read also found nonlast Hook ends never publish an exact
rich exit, while the app only has unique-Hook fallback. Leibniz's same bounded
producer change now also owns the minimal INTERNAL Hook session identity event
and app bridge/ingestion tests needed to retire the exact captured Hook. Keep
external serialization/HTTP/route protocol unchanged, leave runtime matching
policy intact and require actual producer-to-registry lifecycle regressions.

Git source reviewers found concrete follow-ups: Pauli fences Changes row/menu
requests and separates diff close-instance ownership from read invalidation;
Bohr retains removal confirmation/source before each subsequent terminal close
and requires matched destructive bindings to own the actual target location.
Both continue in their disjoint scopes; remote enablement waits for release.

Pauli released the completed four-Git-UI review (seven additional authored
regressions, all UNRUN). Main accepts the current literal-English Git
diagnostic/review/draft style for this bounded slice, as already accepted for
Tasks; existing translations remain, and no broad localization refactor or new
Git keys is added. The combined Files/onboarding delta remains 16 keys.

Pauli found `GitBackend::connect` read the anchor on a replacement epoch before
the UI could reject it. Resumed McClintock owns only the before-anchor captured-
epoch check and narrow tests; explicit write-reconciliation reconnect remains
separate. Bohr has received Pauli's private two-lifetime dialog-close pattern,
without a shared API change. Worktree source review and Agent producer work
still block their source gates; no feature-integrated candidate is pushed yet.

Main prepared `/tmp/mini-term-agent-shared-index.patch` only for later selective
staging: it contains the four Agent hunks in main.rs/store/mod.rs/projects.rs.
After staging a reviewed non-Agent candidate's full shared files, reverse that
patch in the INDEX ONLY to keep unfinished Agent hunks out, then inspect the
cached diff. The INDEX-ONLY reverse patch has now been applied successfully;
61 reviewed non-Agent code/CI files are staged, with only the non-Agent module,
export and location-helper hunks in those shared files. Working source is
unchanged. Do not re-add those shared files wholesale, revert working files or
include the original unrelated dirty paths.

Bohr's final Worktree action matrix is source-complete and released, including
captured removal source/lifetime, trusted destructive target binding, separately
captured skipped saved_layout, registration aliases/drafts and modal disposal.
McClintock released the before-anchor epoch fix. Pauli then removed only the
obsolete private remote predicate and render guard; every reachable Git action
now uses the released host-aware path. Main is staging owned non-Agent specs,
reports and plans with that candidate. New integrated Actions/native gates are
still UNRUN, not implied by the earlier CLI success.

Leibniz released the two Agent producer fixes and exact internal Hook status/
exit bridge. Resumed Bohr independently reviews only this producer/bridge
follow-up, leaving staged Worktree and other non-Agent files alone. One concrete
remaining issue is explicit same-ID resume being rejected by the rich runtime's
ended-run guard; a minimal lifecycle-start API decision is requested before any
runtime change. Agent code remains unstaged until that source gate completes.

Reviewed non-Agent integration is committed/pushed as
`7e60d31bf164526c3eb7205becab4208252c1362`: CI `34004657463`, Windows package
`34004657434`. The first Linux job stopped at changed Rust formatting and the
16-key generated dictionary. Main downloaded and applied the complete U3
`full-rustfmt.patch` and `generated-i18n.patch` from that exact Actions run.
Both index and working source retain unfinished Agent hunks separately in shared
store files. No formatter/generator/check was executed locally. Windows compile
is still running; actual WSL fixture preparation/import succeeded and its test
step is running. These setup results are not a passed transport or feature gate.

Main accepted Bohr's concrete explicit-resume API proposal in producer-review.md:
one source-minted internal Hook lifecycle identity, source Started/Observed
identity discriminator before status delivery, and dedicated start/observe
registry entries with private exact-route/lifecycle-to-RunId bindings. A real
explicit resume creates a new RunId while preserving ended audit rows; repeated
active starts and ordinary events retain strict matching/order rules. The
incoming wire's same-ID post-resume late-end ambiguity is explicitly not solved
by internal tokens. Bohr owns this narrow producer/runtime/bridge correction and
source regressions. Main recorded the already implemented immutable receipt
contract; final lifecycle signatures will follow the released implementation.

The complete Actions formatting and generated dictionary are committed as
`a566edf`. Windows `101409559517` for `7e60d31` failed on the pinned git2 0.19
API: Status::WT_UNREADABLE is not exposed. Resumed McClintock source-reviewed
and corrected this through the typed workdir Delta::Unreadable before status
mapping/clean skipping, retaining unreadable rejection and adding one direct
production-helper regression. Its backend-review addendum records primary
pinned-version evidence. Main is committing that narrow fix with the next push;
no local execution gate ran. Agent producer work remains separate and unstaged.

Candidate `4660367306d29590b39f142104acfcf84b382e56` ran CI `34005271807`
and package `34005271799`. Linux `101411287139` passed format/dictionary/locked
graph gates, then failed application compilation on a missing existing SFTP
type root export. Windows `101411287151` reported the same three binary errors;
both test builds additionally reported seven missing-import references to the
correct picker_open_is_current helper. Pauli's bounded compiler addendum fixes
those two names and three proven unused imports, with no behavior change.
Main reviewed the exact diff and is committing only those source/report paths.

Actual WSL `101411433835` successfully prepared/imported its same-run fixture,
but test discovery/build failed on the same app compile errors, before account
assertions ran. The only-owned distro cleanup succeeded. The earlier cancelled
WSL job `101409668656` also completed its cleanup successfully. Setup/cleanup
evidence is not a passing account transport test. All new feature tests remain
blocked by compilation, and Agent producer/lifecycle changes remain unstaged.

Main committed/pushed Pauli's five-file compiler wiring fix plus reports as
`37b4ef9cebcd20dc0f90170321f35799a16f1d6d`: CI `34005933974`, package
`34005933967`. Linux `101412980006` and Windows `101412979902` now pass app
and test-target compilation. Linux stopped at 28 changed-line Clippy warnings;
177 baseline warnings were correctly ignored. Windows focused tests and actual
WSL `101413098858` continue. McClintock owns only backend dead API/DTO and
equivalent let-chain diagnostics; Pauli owns other obsolete wrappers and the
reported Git/Tasks/browser style changes. Preserve pinned behavior and proof
data; no broad lint suppression or fake reads.

Bohr released the completed Agent producer/resume review and closed. It fixes
same-ID rich resume, first explicit-start promotion with unchanged token/receipt,
and duplicate active starts resetting state/age. Nine new and two expanded
source-backed tests are authored, all UNRUN. Main reviewed producer-review.md
and recorded final lifecycle APIs/limitations in mt-ai and mt-app specs. Agent
source is ready for its separate scoped commit; the ongoing non-Agent tests
are not Agent validation. Matching full-source Actions/native gates remain open.

The complete independently reviewed Agent slice is committed locally as
`8ab8c2770e72a5751ba4002ec0830c9d029a1065` (40 scoped product/spec/report files).
It is not yet pushed or covered by any Actions result. The shared-file staging
isolation described earlier is finished; the index was emptied by this commit.
Original unrelated dirty paths remain untouched.

Non-Agent CI `34005933974` at `37b4ef9` is now complete and FAILED overall.
Windows `101412979902` passed onboarding (82 and three filtered tests), Files
(49, 14, five and one), and Tasks (seven native executor passes/one explicitly
ignored WSL fixture, 24 app, two config and 31 domain passes). Git domain filters
passed 24 and six tests; the app Git filter passed 123 and failed one:
`queued_write_rejects_same_status_changed_bytes_without_dispatch`. Its fake
host rejects the production `ls-tree -z -l --full-tree` read plan. McClintock
owns only this concrete fake/production-sequence review and focused test repair.
The terminal-host step was skipped after failure, not passed.

Actual WSL `101413098858` discovered and ran exactly one fixture, failing in
0.06 seconds at the shared fixture-command success assertion. Its import and
only-owned distro cleanup succeeded; no account transport assertion is proved.
Noether released bounded pre-auth stage/numeric-status/output-class diagnostics
and one Windows unit regression. Raw output, argv and credentials are never
logged. The existing log does not identify the failed probe or prove a runner
cause; explicit cwd is already present, and production Job/cancellation guards
are unchanged. The next exact-SHA Actions run must reproduce with diagnostics.

Pauli released all 16 nonbackend Clippy source corrections, including the sole
download-preflight test caller and retired directory-listing doc link. Backend
Clippy corrections are also released: remove only unused APIs, preserve proof
fields with documented field-level allowances, retain condition order and
source/lease fencing. New corrections and WSL diagnostics remain UNRUN.

Windows package `34005933967`, job `101412990532`, succeeded at `37b4ef9`,
including staged payload and extracted NSIS verification. This older non-Agent
installer does not validate the pending full-source candidate, overall CI,
actual remote transport or native interaction. Main will push only reviewed
follow-ups with the Agent commit; all execution remains Actions-only.

McClintock released the precise fake HEAD lookup and stronger unchanged-status/
zero-dispatch assertions. Main reviewed and committed the 19-file follow-up as
`0b284756a5ce9d1fe82771961ce82ef35e1cba0d`, then pushed it with `8ab8c27`.
This is the first candidate containing the entire reviewed Agent slice:
CI `34007314719`, package `34007314807`. Linux `101416803570` stopped at
changed Rust formatting; generated i18n is unchanged and passed. Main downloaded
and applied the complete exact-run `full-rustfmt.patch` to index and working
source (23 product files), with no local formatter or checker. Windows
`101416803507` compilation and actual WSL `101416918481` continue; the format
follow-up is held from push until their useful compile/diagnostic evidence.

The format follow-up `aec1c720f551bcd9f054553bc096c7991449aae8` is now
pushed: CI `34007899647`, package `34007899592`. Linux `101418401826`
passed format/dictionary/staging/locked graphs and is compiling all targets.
Windows `101418401984` and the new isolated WSL producer also continue.

At `0b28475`, full Windows application/test-target checking passed. Its job
`101416803507` was later CANCELLED by this push while still compiling the
onboarding test binary, so none of its focused regressions are passing evidence.
Actual WSL `101416918481` completed before the push and executed exactly one
test: FAILED in 0.10 seconds, stage ReadOwner, exit -1/0xffffffff, stdout 90
bytes classified unclassified, empty stderr, no timeout or truncation. Only-
owned distro cleanup passed. No account API was reached. Noether now traces
the precise Windows/WSL startup difference and proposes bounded pre-auth
diagnostics; no cause is guessed from output length and no production Job or
credential safeguard has been bypassed. The exact-run diagnostic report is
the Tasks child's `review.md`.

Linux `101418401826` at `aec1c72` now passed all-target root compilation and
sidecar check, then stopped at six changed-line Clippy warnings (169 baseline
warnings ignored). All earlier 28 non-Agent warnings are resolved. The six
remaining findings are tracker map-entry/equivalent nested-if style and one
Hook test type annotation. Resumed Leibniz owns only those two mt-ai source
locations and its lower-layer-review addendum; runtime/lifecycle semantics and
mutex ordering remain fixed. Root tests and authenticated SSH are still blocked
by this lint gate. Windows full compilation also passed at this exact SHA.

Leibniz released the two-file style correction with preserved tracker mutex and
episode semantics; main reviewed and committed it as `fdceead`, not yet pushed.
Windows job `101418401984` at `aec1c72` is now entirely SUCCESS. Onboarding
filters passed 82/3, Files 49/14/5/1, Tasks executor eight (including the first
secret-safe diagnostic unit) with the separate WSL test ignored as intended,
Tasks app 24, config two, domain 31, Git domain 24/6, Git app 124, and terminal
host 31. The previously failing fake-discard regression explicitly passed.
Root/sidecar checks also passed. The two terminal-host binary targets contained
zero tests; the 31 library passes are the regression evidence.

Actual WSL `101418545901` again FAILED at ReadOwner with the same -1/90-byte
unclassified-output result (0.05 seconds, exactly one executed test). Owned
cleanup succeeded. Main authorized Noether's Windows-test-only fixed-marker
matrix: captured/root cwd, default/root user, null/closed-pipe stdin, retaining
strict Job/suspended/no-window guards and original-baseline failure regardless
of alternative successes. Only private runner factoring is allowed; production
still selects Null stdin. Main added the Windows execution_host module filter;
Pauli reviewed it and added nonempty discovery without changing existing Tasks
or actual WSL gates. No production cause or passing transport is claimed.

Package `34007899592` / job `101418384070` at `aec1c72` is SUCCESS,
including staged-payload and extracted-installer checks. Artifact `9981841547`
is `Mini-Term_1.2.2-ci.45_windows-x64`; it was downloaded to the local cache
without launching or validating the binary locally. The Actions manifest records
installer size 18,851,835 bytes and SHA-256
`e3e27a52205d37935e539a2cb592958b5833ed5be020069cc5d019faca32cf80`.
This is complete Windows build/fixture/package evidence for that source, not a
passing Linux/actual WSL or native UI gate. Main reviewed the marker helper,
private Null-only production factor, diagnostic matrix and regression source,
and recorded the boundary in the release-staging contract; Noether's final
source release still precedes staging/push of that diagnostic delta.

Noether released the complete marker matrix, immutable Null production factor
and six new Windows regressions. Main reviewed the final guard/secret-safe
assertion changes, then committed/pushed the seven-file diagnostic integration
as `5a03fa60249b6162319b006b77b958911fde6198` with the Agent Clippy fix.
CI `34009289031` / package `34009289005` started. Linux `101422143667`
stopped at formatting in only the two new diagnostic source files. Main read
and applied its complete exact-run `full-rustfmt.patch` to index and source,
without local tools/checks. The immediate format follow-up restarts the gate
before waiting for more expensive test builds; this run is not transport proof.

The two-file Actions format follow-up is committed/pushed as
`0e141f01596787b66e3a9c90f5c94ff3e1708236`: CI `34009374061`, package
`34009374064`. The superseded `5a03fa6` Windows check was cancelled while
compiling; WSL `101422269400` was cancelled during cache setup, before import,
and its always-cleanup step succeeded. No marker matrix executed in that run.

At `0e141f0`, Linux `101422396717` has passed format, dictionary, staging,
locked graphs, all-target root/sidecar checks and the complete changed-line
Clippy gate. Full workspace tests are now running. Windows `101422396483`
passed full compilation and is running focused tests. Actual WSL
`101422518970` has imported its owned fixture and is in the exact test step;
package `101422381779` is building the GPUI app. All source owners are released
and closed pending concrete new Actions findings, not performing broad re-audits.

Actual WSL `101422518970` at `0e141f0` has now FAILED after exactly one
test in 0.51 seconds. Its guarded eight-way matrix is decisive at the tested
boundary: all four `--cd /mini-term-fixture` rows fail with exit -1, 90 stdout
bytes classified windows-file-not-found, empty stderr and no matching marker;
all four `--cd /` rows succeed with exit zero and the exact typed owner marker.
Default versus explicit-root user and Null versus ClosedPipe stdin do not
change either result. Every row retains suspended/no-window/Job-tree guards;
none times out or truncates. The original baseline is still rejected despite
alternative successes, and only-owned distro cleanup passed. No account API
was reached; the deeper Windows/WSL cause is not asserted.

Main resumed Noether `01a07400-eef7-7f23-a583-3d13a4121f25` for a bounded
production correction: fixed-root WSL launch, followed by fail-closed entry
into the exact captured Linux directory before the command/account lookup.
Scope is execution_host WSL planning, Tasks run_wsl, their focused tests and
the Tasks review report. Preserve literal argv/relative executable semantics,
all identity/cancellation/Job/secret guards, and all existing public APIs.
Interactive PTY launch, distro/global configuration, user defaults, credential
policy and native/SSH backends remain out of scope. Actual owned-WSL cwd and
missing-directory non-dispatch regressions must be added, never run locally.
Linux full tests and the Windows/package jobs continue at `0e141f0`; retain
their useful first integrated test evidence before superseding the run.

Linux job `101422396717` at `0e141f0` is now entirely SUCCESS: full root
workspace tests, authenticated loopback SSH, sidecar tests and whitespace all
passed after the already passing compile/Clippy/format/dictionary/staging gates.
The full suite includes mt-ai 244 passes and mt-app 1205 passes with five
explicitly ignored host fixtures. The separate authenticated SSH step discovered
and executed each of those five exact fixtures once, each with one pass and
zero ignored: Git read/write/epoch/containment (22.76 s), Git uncertain-dispatch
review (5.10 s), Tasks private account/cleanup/epoch pipeline (12.50 s), pinned
Files mutations/containment (0.52 s), and browser source/epoch (0.37 s).
This is the first complete integrated Agent and actual-SSH passing evidence,
not native UI acceptance or WSL coverage. Windows onboarding/Files also passed
at this SHA; its Tasks/Git/sidecar/terminal-host steps and package continue.

Windows job `101422396483` at `0e141f0` is now entirely SUCCESS. Onboarding
passed 82/3; Files 49/14/5/1; the nonempty execution_host filter passed 10;
Tasks executor passed 12 with the separately executed actual WSL test ignored;
Tasks app/config/domain passed 24/2/31; Git domain/app passed 24/6/124; terminal
host library passed 31. Both new Windows marker planner tests and all four new
Tasks diagnostic matrix/classifier tests explicitly passed. Sidecar check also
passed; terminal-host binary targets had zero tests, not additional regression
passes. CI `34009374061` completed FAILED solely on actual WSL.

Package `34009374064`, job `101422381779`, at the same exact SHA is SUCCESS.
Artifact `9982270369`, `Mini-Term_1.2.2-ci.47_windows-x64`, contains the installer
and passing Actions validation manifest. It was downloaded only, never launched
or checked locally, to `/home/leo/.cache/mini-term/artifacts/0e141f0-integration`.
The manifest records installer size 18,846,845 bytes and SHA-256
`8a441529dd2bcbe44e0caf56b290314ee17e63fb5453a5997a5adfbf74448301`.
Main shared this bounded Windows/SSH native-acceptance candidate with the user,
explicitly excluding the in-progress WSL fix and any completed native UI claim.

Noether released the reviewed three-file WSL correction and closed. Registered
and pre-project planning now share a private fixed-root/positional-argv helper,
requiring physical entry into the captured absolute Linux cwd before exec.
Leading-dash executable names fail closed unless requested by an explicit path.
Tasks launches its unchanged private envelope from `/`; the envelope already
enters captured cwd before account operations. Job/stdin/cancellation/credential
policy and public APIs are unchanged. Four ordinary tests and expanded actual
owned-WSL cwd/argv/relative-executable/non-dispatch/account-directory assertions
are authored, UNRUN. Main reviewed the exact diff and added concrete contracts
to Git/Tasks/onboarding/release specs. The existing broad Windows
tasks_account_executor filter includes the new process builder test, so no
workflow change is necessary. This bounded follow-up is ready for scoped push;
only matching new Actions results can validate the correction.

The WSL correction is committed/pushed as
`45fc0bb8c84bcb3825856369008f636135f0b142`: CI `34010736167`, package
`34010736172`. Linux `101425984038` stopped at five changed-format hunks in
the three correction source files. Main downloaded artifact `9982373396` and
applied its complete `full-rustfmt.patch` to index and source without local
formatter/checks. Windows `101425983973` and actual WSL `101426095403` were
still compiling/setup at this point, not passing regression evidence. The
immediate format follow-up restarts the gate before expensive test builds.

The complete three-file Actions formatting follow-up is committed/pushed as
`2cfd4cf67cac1d08cf2dd320162185b2b00b2abd`: CI `34010842674`, package
`34010842692`. Superseded WSL `101426095403` was cancelled before import and
its cleanup passed. Current Linux `101426295439` and Windows `101426295552`
passed all-target compilation, and Linux Clippy passed; ordinary tests continue.

Actual WSL `101426406119` at `2cfd4cf` FAILED after exactly one executed
test in 1.19 seconds: stage `literal-argv`, exit `Some(-1)`. Reaching this
assertion proves the original owner/shim/hash checks and the first registered-
project hostile-directory pwd assertion passed. It does not identify which
absolute/PATH/relative executable or argument failed, and no account request
was reached. Only-owned cleanup succeeded. Main resumed Noether for bounded
WSL argv/source investigation and, if needed, the smallest secret-safe static
discriminator diagnostics. No dropping hostile/empty/newline cases, skipping
the gate, changing Job/stdin/cancel/credential policy, or local probes is allowed.
Any materially new production abstraction needs Main's explicit scope approval.
The first integrated WSL-wrapper ordinary tests/package continue for useful
evidence. Parent `validation.md` now separates current and predecessor evidence
from the still-open native acceptance checklist.

Linux `101426295439` at `2cfd4cf` is now entirely SUCCESS. The full suite
passed mt-ai 244 and mt-app 1209, including all four new WSL planner/private-
builder tests. All five exact authenticated SSH fixtures ran individually and
passed again, followed by sidecars and whitespace. Windows onboarding, Files
and Tasks steps have passed and the remaining Windows/package steps continue.
Main authorized failure-only fixed synthetic printf comparisons in the actual
owned WSL test to separate argument, format/output and program-kind failures
in one diagnostic run. Each row remains bounded, secret-safe and cannot make
the original failed baseline pass. Production code/policies remain unchanged
during this diagnostic follow-up.

At `2cfd4cf`, Windows `101426295552` is now entirely SUCCESS: onboarding
82/3, Files 49/14/5/1, execution-host 13, Tasks executor 13 (one separate WSL
test ignored), Tasks app/config/domain 24/2/31, Git domain/app 24/6/124 and
terminal-host library 31. All four newly added WSL planner/builder tests passed
on Windows too. CI `34010842674` completed FAILED solely on actual WSL.
Package `34010842692`, job `101426284311`, is SUCCESS with artifact
`9982719381`, `Mini-Term_1.2.2-ci.49_windows-x64`. Download-only cache is
`/home/leo/.cache/mini-term/artifacts/2cfd4cf-integration`; the passing Actions
manifest records installer size 18,851,385 bytes and SHA-256
`0a3437bd36a2268734a0b3f5ceee405ce6090a6e848129b16f5b4590a006c0fc`.
No local installer/hash/app verification was run. The next delta remains
Windows-test-only diagnostics and must not change any production policy.

Noether released the single-test-file literal-argv diagnostic follow-up and
closed. Main reviewed all ten fixed printf plans, existing source/runner fences,
four new ordinary Windows regressions and the no-fallback/secret-safe report.
The full original argument vector and program kinds remain unchanged; only a
failed baseline collects comparisons, and it still rejects afterward. The
standalone Empty row cannot prove argv cardinality by itself, so the original
full-vector assertion remains authoritative. No production, fixture payload,
workflow, cancellation, Job or credential policy changed. Main recorded the
diagnostic contract and is staging only the single source file and four owned
spec/report/validation paths for the next exact-SHA Actions run.

The test-only discriminator follow-up is committed/pushed as
`cb8e368db6a90e462afb2eae0d4effae29e7838c`: CI `34012168519`, package
`34012168544`. Linux `101429759746` stopped at six changed-format hunks in
the single test source. Main downloaded artifact `9982804374`, read the full
patch and applied it completely to source/index, without local formatter or
checks. Windows `101429759611` and actual WSL `101429887871` were still in
compile/setup, not passing diagnostic evidence. The immediate format-only
follow-up avoids waiting for their expensive test compilation before restarting.

The complete single-file formatting correction is committed/pushed as
`15a1f25b49a3bd85deeb4ec2cd86a03141eb543f`: CI `34012325033`, package
`34012325025`. Superseded WSL `101429887871` had imported its owned fixture
before cancellation in the test step; owned cleanup succeeded. It provides no
passing diagnostic evidence. Current Linux `101430180962` has passed formatting
and entered compilation; Windows `101430180948` is compiling, actual WSL
`101430284051` imported its same-run fixture and entered the exact test step.
All source owners are released and closed until a concrete new Actions result.

Actual WSL `101430284051` at `15a1f25` executed exactly one test and FAILED
in 2.25 seconds, now with decisive parameter evidence. The original Project/
Absolute baseline and the Empty comparison alone return exit -1, stdout 60
bytes classified windows-invalid-parameter, empty stderr and false matching.
Ascii, AsciiNul, Hostile, Dash, DoubleDash, Assignment, Wildcard, Newline and
WithoutEmpty all return zero with exact matching output. No row times out or
truncates; owned cleanup succeeded. The failed baseline remains rejected and
no account request ran. This isolates direct empty-argv transport on this
runner, not an arbitrary deeper OS cause or a claim about every WSL version.

Main resumed Noether for a bounded production correction using the existing
tested POSIX quote/argv serializer. Generic project/pre-project WSL planning
must send one nonempty shell command string through the fixed-root launcher,
with encoded physical captured-cwd entry and encoded original argv; Linux must
receive all original empty values. Keep source, absolute-cwd and exec-option
guards, relative lookup, and fail-before-target on invalid cwd. Private Tasks
must separately serialize only `exec <envelope argv>` from `/`, leaving its
existing Python chdir as the sole cwd entry so missing cwd remains
Account(CommandFailed), not HostHelperUnavailable. Do not clone/rewrite source
to `/`, route credential output through the generic executor, add a public shell
API, change process/credential policy or introduce a new Python dependency.
Existing SSH envelope serialization in mod.rs is a reuse reference, not an
editable SSH scope. All original actual cases/diagnostic baseline rules remain,
with focused empty/multiple-empty regressions added. Execution stays Actions-only.

At `15a1f25`, Linux `101430180962` and Windows `101430180948` are now
entirely SUCCESS. Linux retained mt-ai 244, mt-app 1209 and all five individually
executed authenticated SSH passes, followed by sidecars/whitespace. Windows
passed onboarding 82/3, Files 49/14/5/1, execution-host 13, Tasks executor 17
(one separate WSL test ignored), Tasks app/config/domain 24/2/31, Git 24/6/124
and terminal-host library 31. All four new literal discriminator/diagnostic
units explicitly passed. CI `34012325033` is FAILED solely on actual WSL.
Package `34012325025`, job `101430166227`, is SUCCESS; artifact `9983144114`
is `Mini-Term_1.2.2-ci.51_windows-x64`. This test-only successor preserves the
last completed production baseline, not a passing empty-argument correction.

Noether released the reviewed empty-argument correction. Generic WSL planning
uses the existing POSIX cwd/argv encoders in one nonempty shell command string;
the private Tasks builder encodes only its original envelope exec and retains
Python's sole captured cwd entry and existing error distinctions. No new public
API, dependency, SSH/PTY change, credential output path or process policy was
introduced. Main reviewed the exact three-file diff and updated the relevant
Git/Tasks/launch contracts. Two ordinary tests are new; existing planner/builder
expectations are updated. Actual WSL now also checks `$#` plus NUL-separated
values for one empty argument and consecutive/leading/trailing empty arguments,
on both project/pre-project routes. All original cases and failure-only
diagnostic rules remain. New verification is UNRUN until matching Actions.

Main accepted Noether's written source release, froze/closed that owner and
committed/pushed the eight-file correction as
`26180f948845770ae1481ad775abe07d74d9a8b4`: CI `34013667988`, package
`34013667980`. Linux `101433676291` stopped at seven changed-format hunks
across the three source files. Main downloaded artifact `9983242413`, read and
applied its complete `full-rustfmt.patch` to source/index without local tools
or checks. Windows `101433676281` and actual WSL `101433777054` were still
in compile/setup, not current passing evidence. The immediate format-only
follow-up restarts the same full gates; no assertion or production policy changed.

The complete three-file Actions format correction is committed/pushed as
`5a070e149e76d289c08cc64e9cd3d05deb8259b8`: CI `34013775348`, package
`34013775268`. Superseded WSL `101433777054` imported its owned fixture before
cancellation during the test step; cleanup succeeded and no passing result is
claimed. Current Linux `101433980963` passed formatting and is compiling;
Windows `101433980851` is compiling and actual WSL `101434114219` is setting
up its same-run artifact. Product source and index are clean; original unrelated
dirty paths remain untouched. All sub-agents are closed pending concrete evidence.

Actual WSL `101434114219` at `5a070e1` executed exactly one test and FAILED
after 20.19 seconds at tests.rs:2153: actual HostHelperUnavailable versus expected
Cancelled. Source order proves all original and new cwd/literal/empty-cardinality
checks and the capability/discovery/selected-account/secret/error/Rotate/large-
response cases completed before the cancellation loop. Its failing canceller's
readiness assertion passed, but the message does not identify Slow versus
LookupSlow and the failed iteration's final descendant-retirement assertion was
not reached. Owned distro cleanup succeeded. This is real empty-argv/account
pipeline progress, not a passing entire cancellation/transport gate.

Main resumed Noether for bounded private capture/run_wsl lifecycle investigation
and focused tests/report. Trace stop checks, child exit, cleanup/drain and the
fixture's concurrent owned-WSL readiness command. Preserve Job/suspended/no-window,
source/epoch, private credential buffers, limits and distinct CleanupFailed.
Never classify an arbitrary exit as Cancelled or accept cancellation without
host cleanup acknowledgement. If source cannot prove the cause, add only bounded
static case/stage/exit/control/ack diagnostics, never raw private output. Shared
ProcessTree, Python/SSH envelope or distro-policy changes require a concrete
proposal before edits. All execution remains Actions-only; ordinary tests and
package at `5a070e1` continue for their useful evidence.

At `5a070e1`, CI34013775348 is now complete and FAILED only on actual WSL.
Linux101433980963 fully passed (mt-ai244, mt-app1211, all five actual SSH
fixtures, sidecars and final checks). Windows101433980851 fully passed:
onboarding82/3, Files49/14/5/1, execution-host14, Tasks executor18 plus one
separate ignored WSL case, Tasks app/config/domain24/2/31, Git24/6/124 and
terminal-host library31. New empty-argument tests explicitly passed in both.
Package34013775268/job101433965140 is SUCCESS; artifact9983573681 is
Mini-Term_1.2.2-ci.53_windows-x64, not downloaded or launched. Main updated
validation.md to this exact completed candidate without claiming actual WSL
or native acceptance.

Noether released/froze the bounded cancellation diagnostic slice and five new
focused tests, then closed. Main reviewed the three source files and report:
six cfg(test) fixed capture slots, same-clock case/readiness/return timestamps,
first-probe retention and saturating probe count, typed stop/control/write/ack
and numeric exit/byte metadata only. Original lifecycle cases, deadline/cadence,
readiness/result assertions and final descendant checks remain. Production
behavior is unchanged; a named discarded write result only enables test-only
write-success observation. No shared ProcessTree, launcher, Python/SSH,
credential, CI or rootfs-policy changes. Synthetic-only lifecycle modes preserve
Cancelled versus CleanupFailed and nonzero exit semantics in focused tests.
Main recorded the executable diagnostic contract. All new verification is
UNRUN until the scoped commit and same-run Actions fixture rebuild/rerun.

The released diagnostic slice is committed/pushed as
`d3766364cbe32349e7d4a82b5f540f9077352b43`: CI34015574888 and
package34015574931. Linux101438614801 stopped on four formatting hunks across
process.rs/tests.rs; generated i18n passed. Main downloaded artifact9983786019,
read and applied its complete full-rustfmt.patch to source/index. No local
formatter/check ran and no behavior changed. Windows101438614858 was compiling;
actual WSL101438728422 was setting up, with no diagnostic execution claimed.
The immediate formatting successor restarts the full gates. Main also downloaded
the already-passing ci.53 package to
`/home/leo/.cache/mini-term/artifacts/5a070e1-integration` and offered its Actions
artifact link for native pre-acceptance, explicitly retaining the failed WSL gate.
No installer launch or local artifact verification was performed.

The complete Actions formatting successor is committed/pushed as
`bb79421870083ef6c6a45c1921bd2baef0266e52`: CI34015670072 and
package34015670075. Linux101438890459 passed formatting and entered compilation;
Windows101438890388 is compiling, rootfs101438890296 passed and actual
WSL101438999280 is setting up. No current diagnostic test pass is claimed yet.
The downloaded ci.53 Actions manifest reports status passed, installer size
18,852,484 and SHA256
`22b14e7f57961ae3f95e09faee5c892599c49a52f653ac1e2975c1a6a49e2121`.
These are manifest observations, not local verification.

Superseded d376636 WSL101438728422 was cancelled DURING import, without an
Imported message. Its always-cleanup step then FAILED with static
`wsl.exe failed (4294967295)`, unlike previous successful interrupted cleanups.
No test ran and no cleanup success is claimed. The shared setup error does not
identify which wsl call failed. Main resumed Pauli only for a bounded source/log
review and proposal concerning interrupted-import cleanup; no CI/source edits
are authorized before Main reviews that proposal. Preserve exact owned-state
guards, final failure on unconfirmed removal, other/default distros and no
global shutdown/config. Current product cancellation evidence remains separate.

Pauli confirmed the superseded cleanup log cannot distinguish enumeration,
unregister or final verification. Proposed bounded cleanup retries require
additional phase evidence and cannot prove interrupted-import quiescence from
an absent listing. Main declined a retry-state-machine change without that
evidence: the existing step fails closed and retains ownership state, and this
cancelled ephemeral runner is not current product/test success. Pauli made no
edits and closed. Runner interruption/persistent service failure remains an
explicit cleanup limitation; current normal actual WSL cleanup must still pass.

Actual WSL101438999280 at bb79421 executed exactly one test and FAILED in
20.85 seconds, now with decisive ordering evidence. DataCancel returned
HostHelperUnavailable at155760us with cancelled_at_return=false. Capture attached
at2534us, observed exit-1 at155611us with latched/control both None, retired its
local tree at155666us and drained at155727us with ack=false, stdout118 bytes,
stderr0. There was no StopLatched or ControlWrite. Readiness succeeded on probe2:
first start2632us, final start196952us/end299384us, cancel299386us. Thus the API
had already returned before cancellation; relabelling a late cancellation cannot
fix this observed failure. It overlaps the earlier false ordinary-WSL readiness
probe, but that alone does not prove cross-Job interference or the OS cause.
The failed iteration's final descendant assertion was not reached. The exact
owned distro cleanup SUCCESS is independently observed.

Main resumed Noether for a bounded concrete early-exit/concurrency diagnostic
or correction proposal before edits. Do not replace the concurrent readiness
path, weaken descendant proof, expose private bytes, remap errors or relax Job
guards. Shared process/launcher/Python/SSH changes require explicit coordination.
Official Microsoft Job-object and upstream WSL transport sources were checked as
background only; no matching runner WSL1 root cause was established from them.
Linux/Windows/package bb79421 continue; compilation and Clippy passed but their
remaining tests/package are not yet complete.

Main approved Noether's next bounded TEST-ONLY slice: full-message allowlisted
Windows output classification and passive same-clock observations immediately
around the existing TerminateJobObject, including bounded active-process count
or explicit query failure. Shared execution_host/ProcessTree access is limited
to that cfg(all(test,windows)) observer; no production API/cleanup/Job-policy
change. First/final readiness start/return/retirement records remain fixed-size,
and the original concurrent scenario/result/ack/descendant assertions remain.
No raw pipe/argv/path/environment/process names or arbitrary errors may escape.

At bb79421, Linux101438890459 and Windows101438890388 are now FAILED on
the same new capture_diagnostics_are_bounded_payload_free_metadata acknowledgement
assertion (process.rs843). Linux mt-ai244 and mt-app1214 passed, with one failed
and five ignored app tests. The other three new Linux capture tests passed;
actual SSH/sidecar-test/whitespace stages were skipped after root test failure.
Windows passed onboarding82/3, Files49/14/5/1, execution-host14; Tasks had
22 passes, one failure and one separate ignored WSL case. Its other four new
diagnostic tests passed. Later Tasks/Git/sidecar/terminal stages were skipped.
Both compiler paths and Linux Clippy passed. Package is still running.

Main supplied those exact logs to Noether and requested a confirmed diagnosis
before any production protocol correction: internally tagged unit-status replies
may accept extra fields despite the enum attribute. Do not simply weaken the
new secret/ack expectation. Any mod.rs change needs Main coordination; current
approved classifier/observer diagnostics continue independently.

Noether confirmed locked Serde1.0.229's internally tagged unit-variant visitor
ignores extra fields, so a Cancelled reply with extra token content is wrongly
accepted as cleanup acknowledgement. This is a real schema hole exposed by the
new test, not a reason to weaken it. Main authorized the minimal mod.rs change
to empty struct status variants with identical valid JSON/error mappings, and
all-status valid/extra/duplicate/type regressions through decoding and cleanup
acknowledgement. The original failing assertion stays intact. Main recorded the
strict wire contract. This production protocol correction is separate from the
test-only early-exit/Job observations and still needs fresh Actions evidence.

Package34015670075/job101438873772 at bb79421 is SUCCESS, artifact9984115137
Mini-Term_1.2.2-ci.55_windows-x64. It was not downloaded or launched. CI34015670072
is fully completed and FAILED on the cross-platform new ack regression plus
the actual WSL DataCancel early exit; this package is not a full-gate pass.

Noether released/froze the five-source-file correction/diagnostic slice plus
review.md, then closed. Main reviewed the exact HostReply wire correction,
all-status/Output assertions, compiled synthetic extra-field cancellation case,
26-code complete-message classifier and passive fixed-storage retirement query.
Eight new tests are authored, with existing metadata/capture/readiness tests
retained or strengthened. Original DataCancel concurrency, limits, expected
result and final descendant checks are unchanged. Only strict malformed-host-
reply rejection changes production behavior; instrumentation is test-only and
existing termination still executes once with unchanged arguments/result.
No dependency/CI/rootfs/launcher/Python/SSH policy change. Main updated the two
affected contracts. All current validation remains UNRUN until the next exact-
SHA Actions gates; there is still no early-exit fix claim.

For any formatter-only successor, Main will avoid cancelling while WSL import
is in progress: wait until its import has completed and the exact test step is
active before superseding, then observe owned cleanup. This is coordinator
sequencing, not a workflow change or a guarantee against runner termination.

The released correction/diagnostics are committed/pushed as
`f6240b537445d3688ab22286da4db4870f756f73`: CI34017786246 and
package34017786259. Linux101444578831 stopped on26 changed-format hunks in
execution_host.rs/process.rs/tests.rs; i18n generation passed. Main downloaded
artifact9984464727, read and applied its complete full-rustfmt.patch to source/
index, without local formatting/checks. WSL101444696095 had completed import
and entered its exact test build step before the successor push; no test pass
is claimed. Windows101444578823 was still compiling. The formatting successor
restarts unchanged full gates and same-run fixtures.

The complete three-file formatting successor is committed/pushed as
`c92e54acb4f3b296801b2bb0213c585a671e7273`: CI34017910209 and
package34017910189. Superseded WSL101444696095 had completed import before
cancellation during test compilation; exact owned cleanup succeeded. Current
Linux101444951799 passed formatting and is compiling, Windows101444951923 is
compiling, rootfs101444951902 passed and actual WSL101445078760 completed import
and entered the exact test build/execution step. No current protocol regression
or early-exit/retirement result is claimed yet. All owners are closed and source
is frozen pending new Actions evidence; original unrelated dirty paths remain.

Main's additional source review found the same locked Serde visitor also accepts
sequence input for internally tagged enums/struct variants. Empty struct statuses
therefore do not alone enforce the documented top-level OBJECT shape: positional
arrays such as ["cancelled"] or ["output","ok","",0] need explicit rejection.
The new tests covered array-valued status fields, not valid-position root arrays.
Primary source checked: Serde1.0.229 private/de.rs TaggedContentVisitor::visit_seq
and serde_derive/src/de/struct_.rs internally tagged visit_seq/deserialize_any.
No local execution or current-CI array result is claimed. Main resumed Noether
only for a bounded shared object-only HostReply parser (structured map visitor,
preserved duplicate detection and trailing-data rejection), focused tests/optional
synthetic malformed-array acknowledgement and report. No shared process or
classifier edits belong to that correction.

Actual WSL101445078760 at c92e54a executed exactly one test and FAILED in
29.79s, this time at DataCancel readiness. First readiness start246us/end123389us;
its existing TerminateJobObject ran123194-123220us, Count(4), succeeded=true.
Private capture attached2653us, exited134792us at-1 without stop/control, retired
134821us, drained135339us with ack=false/stdout118/stderr0 and both native message
classes Unknown; API returned135376us cancelled=false. No StopLatched/ControlWrite
occurred. Readiness never became true over66probes; final start9988807/end10091098us,
retirement10091011-10091027us Count(1)/success, cancellation10091099us. Failed
result and failed readiness both remain rejected, and final descendant assertion
was not reached. Exact owned distro cleanup SUCCESS is independently observed.
The first retirement preceded private exit by about12ms and had four associated
active-process references, but this does not identify them or prove cross-Job
causation. Unknown native bytes remain private, not guessed from length.

Main resumed McClintock for a READ-ONLY concrete Windows/WSL ProcessTree proposal
using those exact logs/source/primary docs. No edits are authorized yet. Need
minimal ownership/membership evidence or defensible correction covering generic
WSL Git/Files commands AND private Tasks; no Job bypass, blind breakaway change,
test serialization, removed concurrent readiness, arbitrary Cancelled remapping,
indefinitely retained Jobs, retries or weakened host cleanup acknowledgement.
Noether's independent object-parser scope remains disjoint. Current Linux
compile/Clippy and Windows compile passed; their tests/package still run.

Noether released/froze the object-only parser follow-up (mod.rs/tests.rs/
gh_fixture.rs/report) and closed. Main reviewed the single shared deserialize_map
visitor, unchanged HostReply derive, preserved duplicate-key visibility and end()
framing. Two new tests cover all status/Output positional arrays, nested/scalar
forms, trailing values and valid whitespace. Existing strict-field tests remain;
compiled synthetic cancel-array-ack must still return CleanupFailed. No other
source surface, dependency, public API, valid wire mapping or process policy
changed. Main updated the Tasks contract. All follow-up validation is UNRUN.

At c92e54a, Linux101444951799 is entirely SUCCESS. mt-ai244 and mt-app1217
passed (five actual host tests separately ignored in the ordinary app suite).
The corrected metadata assertion and both strict-field protocol tests passed.
All five actual authenticated SSH fixtures were then each discovered/executed
once and passed with zero ignored cases, followed by sidecars and whitespace.
Windows101444951923 has passed execution-host/Tasks/Git filters and entered
sidecar checks; it and package101444939167 are not yet fully complete. These
results validate the preceding empty-struct correction, not the new map-only
parser. McClintock's read-only Job-scope proposal remains outstanding.

The object-only follow-up is locally committed as7b9eb0f (not pushed yet).
McClintock confirmed no production guard correction is justified by current
timing/count evidence. Main approved a bounded TEST-ONLY paired observer next:
per-case Private/Readiness exact root objects, pre-existing-termination peer
membership/liveness checks, own-Job snapshots capped32 with query-only process
handles, rechecked membership and in-memory object identity comparison. Only
fixed role/count/typed completeness/membership/liveness may be emitted; no raw
IDs/handles/names/paths/output, no Job-handle retention, no peer waits, extra
commands/termination/retries, altered cadence or production policy. Negative/
incomplete records remain inconclusive. McClintock owns only that observer's
execution_host/process/test wiring and a bounded Git-child review addendum;
Noether's mod.rs/gh_fixture/report remain frozen. Main will integrate the two
distinct deltas before the next push and preserve all original failure guards.

At c92e54a, Windows101444951923 and package101444939167 are now entirely
SUCCESS. Windows passed onboarding82/3, Files49/14/5/1, execution-host17, Tasks
executor28 plus1ignoredWSL, Tasksapp/config/domain24/2/31, Git24/6/124 and
terminal-host31. All eight newly enabled strict-field/classifier/retirement
tests passed, including the formerly failed ack assertion. CI34017910209 is
FAILED only on actualWSL. Package34017910189 artifact9984848856 is
Mini-Term_1.2.2-ci.57_windows-x64. Main updated validation.md to this exact
completed candidate, excluding the later object-only parser and membership
observer from its passing evidence.

The ci.57 package was downloaded only to
`/home/leo/.cache/mini-term/artifacts/c92e54a-integration`. Its Actions manifest
reports status passed, installer18,863,080 bytes and SHA256
`833d2fb05b16f96bf89f96e898c921c1cb24d6a860e020b1df47b0bd4dec42e2`.
No local hash/app/installer verification or launch occurred.

McClintock was paused for a concrete status after the implementation stayed at
design. He confirmed NO files/commands outstanding and no demonstrated API or
lifetime incompatibility; 32-member overlap bookkeeping remained unimplemented.
Main closed that owner and WITHDREW full member snapshots/identity overlap from
the next diagnostic scope. No new member-snapshot implementation or pass exists.

Main resumed Noether for a smaller TEST-ONLY follow-up in execution_host.rs,
process.rs, tests.rs and the Tasks report. At most two per-case query-only exact
root process references, scoped Private/Readiness roles, and pre-existing-
termination private-root membership/liveness checks are allowed. No process-ID
enumeration, member snapshots, Job-handle retention, peer waits, barriers, extra
commands/termination/retries or production-policy changes. Missing/query-failed/
busy data stays explicit, not false; process identity never enters logs.
Main also authorized broadening the formerly unknown native error match to a
FIXED BOUNDED trusted system-message catalogue: baseWin32 IDs0..=1999, WinSock
10000..=11004 and a small fixed standard-HRESULT set, at most4096 candidates.
Keep complete-message matching, existing input/buffer caps and failure-only
activation; output only numeric system-message IDs/Unknown. No raw strings,
substrings, private-data cache, length guesses, result remapping or quadratic
all-catalogue roundtrip test. Preserve existing actual assertions. All new
source/validation remains pending, and local7b9eb0f is still not pushed.

Noether released/froze the narrowed root-only observer, catalogue and focused
tests in execution_host.rs, process.rs, tests.rs and the Tasks report. Main
source-reviewed the final bounded registry, exact query-only duplicates,
try_lock/current-reference invalidation, independent membership/liveness and
unchanged existing termination. At most two per-case process references are
retained, with no Job/PID/member inventory. Three new functions cover guarded
known roots, scope/result/error isolation and the fixed3015-ID catalogue;
existing native exit/readiness/capture guards remain. No concrete source/API
blocker remains; actual WSL cause is still unconfirmed. Noether is closed.

Main updated the release contract for the root observer, fixed4096-candidate
ceiling, complete-message matching and matching-ID versus actual-error-code
distinction. The earlier object-only parser7b9eb0f and this diagnostic successor
are being integrated for their first Actions push. All new/updated test execution
and formatting remain UNRUN locally and pending exact-SHA Actions evidence.
The c92e54a/ci.57 link was offered only for native UI pre-acceptance; no user
observations or full WSL pass have been received, and tasks remain open.

The parser and diagnostic commits were pushed to the working fork as
45c813f658f834de9bb3c625818e14cb03bd69ab. CI34021304351 and Windows
Package34021304195 started for that exact head. No local hooks/checks were run.
Current outcomes are pending, including the newly authored three diagnostic
functions, two parser functions and updated guarded synthetic acknowledgement.

At45c813f, Linux101454289237 failed the changed-line format gate with22hunks;
the generated-i18n step then ran without a patch. Full Actions artifact9985587239
was downloaded/read and mechanically applied in entirety from
`/home/leo/.cache/mini-term/artifacts/45c813f-integration/rustfmt/full-rustfmt.patch`.
It touches only execution_host.rs, process.rs and tests.rs. Main is preparing a
format-only source successor; no local formatter/check ran. Rootfs101454289183
passed; Windows101454289075 and actualWSL101454407533 remain in progress.
The successor push must wait until actual WSL import has completed rather than
superseding a runner during import. No new test pass is claimed at this point.

Main observed45c813f's WSL import step SUCCESS and the exact-test step actively
building before pushing formatting successor
a98473ee81e78d5a5b0c448f7e905e3aeb108038. Superseded Windows101454289075,
WSL101454407533 and package34021304195 were cancelled, not passes. The WSL
log confirms exact owned cleanup at08:17:37Z:
`Cleaned only Actions-owned distro mt-tasks-34021304351-1`.
Successor CI34021441260 and Package34021441294 are running. Their Linux/
rootfs/Windows jobs are101454684308/101454684420/101454684440. No new actual
test result is yet available; no retry or cleanup policy changed.

At a98473e, actualWSL101454796139 discovered/executed exactly one test and
FAILED in28.96s at LookupCancel (tests.rs2627). This run passed DataCancel and
LookupTimeout, including their final descendant assertions, by source order;
the LookupCancel descendant assertion was not reached. Its first readiness
probe1883-135491us retired at135426-135451us with Count(5), success. At that
instant exact Private root was NotInJob/Alive and Readiness InJob/Exited.
Private attached2451us, exited144475us at-1 without stop/control, tree retired
144530us, drained154002us ackfalse/stdout118/stderr0 and both native classes
Unknown. API returned154039us with cancelled=false. The final second probe
185824-298343us became ready, retired298291-298308us Count(1)/success with
Private NotInJob/Exited and Readiness InJob/Exited; cancel298343us. No actual
cleanup acknowledgement existed. Exact owned distro cleanupSUCCESS08:29:28Z.

This excludes direct membership of the private Windows root in the first
readiness Job at observation time; it does not identify its other members,
rule out indirect transport effects or establish a native cause. The bounded
3015-ID catalogue still returned Unknown and private output stays undisclosed.
Linux compile/sidecar-check/Clippy and Windows compile have passed; remaining
ordinary tests/package are still running. No full current CI pass is claimed.
Main resumed Noether READ-ONLY for one constrained source-backed assessment of
WSL native-message framing/resource handling and relevant transport semantics;
no next diagnostic or production-policy edit is yet authorized. No raw output,
member enumeration, Job retention, cancellation remapping or weaker guards.

Noether's bounded read-only review found a concrete renderer difference:
Microsoft WSL GetSystemErrorString uses FORMAT_MESSAGE_MAX_WIDTH_MASK together
with FROM_SYSTEM/IGNORE_INSERTS; our test renderer omitted it. Regular embedded
resource line breaks can therefore differ despite outer CR/LF trimming. Main
confirmed the primary source and authorized only that test-renderer flag fix,
one focused regression and the report. WSL resource/contextual wrappers may
still remain Unknown; this is not a proven cause of the observed early exit.

Noether released/froze only process.rs and the Tasks report, then closed. The
three rendering flags are pinned by a test-only constant and independently
checked against three representative system messages. Fixed512buffer,
4096input/candidatecaps,3015catalogue and full private-message equality remain;
no input normalization, new wrapper parser, Job/production or control change.
Main updated the release contract. The correction/test are UNRUN and not yet
pushed; the current ordinary/package runs must complete before their successor.

At a98473e, Linux101454684308 is fully SUCCESS: mt-ai244, mt-app1219 plus
five ordinary ignored host fixtures, both new object-only parser tests and the
updated malformed-array cancellation fixture passed. Each of all five actual
authenticated SSH fixtures then executed once and passed, with zero ignored,
followed by sidecars and whitespace. Windows101454684440 has passed all primary
filters/sidecar check and is finishing terminal-host regressions.
Package34021441294/job101454669916 is fully SUCCESS. Artifact9985930270 is
Mini-Term_1.2.2-ci.59_windows-x64, downloaded only to
`/home/leo/.cache/mini-term/artifacts/a98473e-integration`.
The Actions manifest reports passed, installer18,851,649 bytes and SHA256
`7c81edca24c5b437baeec11f2f2e95ff0c4a19f1093bf13cf54dfeeccffd9232`.
No local verification, hash command or installer/app launch occurred.

Windows101454684440 has now completed entirely SUCCESS: onboarding82/3,
Files49/14/5/1, execution-host19, Tasks executor31 plus1ignored actualWSL,
Tasks app/config/domain24/2/31, Git24/6/124 and terminal-host31. The two new
root observer tests, catalogue bound test, two object-parser tests and updated
malformed-array capture assertions explicitly passed where enabled. Main
updated validation.md to this exact completed a98473e/ci.59 candidate, excluding
the later unrun renderer correction. CI34021441260 failed only actualWSL.
The read-only gh-watch session ended on a transient API EOF; subsequent API/job
logs independently confirmed all final outcomes. No local command remains from
that watcher and no workflow was cancelled for this follow-up.

The renderer correction was committed/pushed as
7bbbd4b67cd6d8d6cfe8e45ed08063b9953e67f7 only after all a98473e jobs
completed. CI34022799075 and Package34022799058 started. Linux101458357202
failed three formatting hunks, with generated i18n unchanged. Complete Actions
artifact9986067525 was downloaded/read and applied in entirety from
`/home/leo/.cache/mini-term/artifacts/7bbbd4b-integration/rustfmt/full-rustfmt.patch`;
only process.rs is affected. No local formatter/check ran. Rootfs101458357097
passed; Windows101458357237 remains compiling and actualWSL101458492666 is
importing. Main is preparing the formatting successor and will not supersede
the currently active import. No new renderer test or actual WSL pass exists.

Main observed the7bbbd4b import SUCCESS and active exact-test build before
pushing complete-format successor a7b8b11ef366c1a8dff6727ba2ac4ba8b0d0f6c1.
Superseded WSL101458492666 logs confirm import08:49:12Z and successful exact
owned cleanup08:49:53Z for mt-tasks-34022799075-1; no actual test pass occurred.
Successor CI34022945645 and Package34022945647 started. Its Linux101458780225
format gate passed; rootfs101458780123 and Windows101458780310 are running.
All renderer/actual transport outcomes remain pending. No local checks ran.

At a7b8b11 Windows101458780310 failed compilation with E0432 at process.rs139
and1004: FORMAT_MESSAGE_MAX_WIDTH_MASK is not exported from Diagnostics::Debug
by the locked windows0.61.3 bindings. No new Windows regression ran. Linux
format/i18n/compile/Clippy passed and tests are running; actualWSL101458919571
is still in its exact-test build step. Main resumed Noether only for the minimal
binding/SDK-mask correction in process.rs and the Tasks report, with no feature
or dependency expansion, removed test/flag, production/Job change or local
execution. The previous complete a98473e/ci.59 evidence stays separately scoped.

ActualWSL101458919571 also failed discovery/build on the same two E0432 imports;
NO actual test executed. Exact owned mt-tasks-34022945645-1 cleanup succeeded
09:00:51Z. No updated renderer/cancellation outcome can be inferred from this
compile failure, and the previous LookupCancel cause remains unconfirmed.

Noether released/froze the minimal process.rs/Tasks-report correction and closed.
The private SDK_FORMAT_MESSAGE_MAX_WIDTH_MASK uses the documented WinBase.h
0x000000ff value; the independent regression constructs FORMAT_MESSAGE_OPTIONS
from that literal without reusing the private constant. Existing flags, caps,
matching/gating and all original assertions remain unchanged. No dependency or
feature was added; the current published WindowsProgramming namespace is not
assumed transitively available to the app. Main reviewed the delta and updated
the release contract. This correction is UNRUN and being prepared for its next
Actions push; a7b8b11's incomplete Linux/package gates will not count as passes
if superseded. Its actual WSL import/cleanup are already complete.

The SDK-mask correction was committed/pushed as
682ac331308bc8d49278845ff9f2f0020f953062. New CI34023590512 and
Package34023590504 are running; Linux101460555838 passed format/i18n and
entered compilation, rootfs101460557128 passed, Windows101460557398 and
actualWSL101460683298 are active. Package job101460534858 is active.
Superseded a7b8b11 CI34022945645 is cancelled overall: its rootfs passed,
Windows/WSL failed compilation, Linux101458780225 and package34022945647 were
cancelled before completion. No new renderer or actual WSL pass is claimed.

At682ac33, Windows compilation and Linux compilation/Clippy passed. Actual
WSL101460683298 executed exactly one test and FAILED20.26s at DataCancel,
so the SDK correction removed the build blocker but did not solve early exit.
First readiness174-144815us retired144718-144751us Count(5), Private
NotInJob/Alive and Readiness InJob/Exited. Private attached2675us, exited
155335us(-1), retired155403us, drained163928us ackfalse/stdout118/stderr0,
returned163959us with cancelled=false. Native output remains Unknown with the
corrected max-width rendering. Second probe195067-297505us became ready,
retired297455-297477us Count(2), both roots Exited/private NotInJob; cancellation
297506us. The failed case's descendant assertion was not reached. Exact owned
mt-tasks-34023590512-1 cleanup succeeded09:16:24Z. Ordinary tests/package
remain running; no full gate or production-cause correction is claimed.

Main resumed Noether READ-ONLY for one constrained proposal for a separate
fixed public/non-credential WSL command pair in a fresh same-run-owned case.
Potential failure-only comparison must preserve the original private failure,
never read/report private account streams, invoke no gh/account/user-data input,
keep existing guarded runners and strict bounds, and treat public success as
inconclusive. No code/logging is authorized before ownership/privacy review.
No further private catalogue/resource expansion, raw private output, member
inventory, Job retention, cancellation remapping or guard relaxation is allowed.

Noether proposed a bounded failure-only public command comparison with existing
fallible WslFixture/host APIs and no implementation blocker. Main authorized
only tests.rs and the Tasks report: after captured/joined unexpected pre-cancel
HostHelperUnavailable, one fresh UUID directory, immutable marker/sleep producer
and concurrent own-marker probes, 5s/4096bytes percommand and existing50ms/10s
readiness policy plus bounded final-probe/cleanup grace. No account/helper/input
reads or original-test order/cadence changes. Setup/probe errors stay metadata;
readiness joins on ordinary returns and all original failures remain failing.
Pre-auth placement was considered but NOT selected; post-failure conditions and
null-versus-private-piped stdin remain explicit inference limits.

Only this public producer's bounded streams may supply strictly decoded, JSON-
escaped previews (256characters each, total diagnostic4096). Undecodable,
truncated or failed-read streams stay metadata-only. Full BOTH streams are
checked for fixture_credential_ in UTF8/UTF16LE/UTF16BE before any prefix removal
or cropping; any hit suppresses ALL previews. Private buffers never enter this
formatter. The exact fixed ASCII start marker may be separated from subsequent
native UTF16 output with marker-presence metadata; arbitrary framing is not
normalized. No raw controls/bytes, generic error messages, argv, environment,
Job/PID/member inventory, new private classifier or production change. Source
and focused privacy/trigger/bounds regressions are still being authored, UNRUN.

At682ac33, all ordinary jobs and packaging completed SUCCESS: Linux101460555838
mt-ai244, mt-app1219, all five individual actual SSH fixtures, sidecars and
whitespace. Windows101460557398 passed onboarding82/3, Files49/14/5/1,
execution-host19, Tasks executor32 plus1ignored actualWSL, app/config/domain
24/2/31, Git24/6/124 and terminal-host31. The new exact WSL rendering-flag
regression explicitly passed. CI34023590512 failed only actualWSL.
Main updated validation.md to this exact completed candidate, excluding the
new unrun public comparison. Artifact9986696965 from Package34023590504 is
Mini-Term_1.2.2-ci.62_windows-x64, downloaded only to
`/home/leo/.cache/mini-term/artifacts/682ac33-integration`.
Its passed Actions manifest records installer18,850,572 bytes and SHA256
`df14bd324bb9a27a21630f4ed4241ffb204a4ed48737423c0f1991587431e3ac`.
No local verification/hash/app/installer launch occurred. Main offered ci.62
for native/SSH pre-acceptance only; no user observations or full WSL pass exist.

Noether released/froze the post-failure public comparison and eight focused
Windows tests in tests.rs plus its review report. Main source-reviewed the
exact trigger/attestation/fixed plans, bounded fallible runner and readiness
join, whole-stream sentinel suppression, strict public-only decoding, Unicode
escaping and unchanged baseline assertions. The review added escaping for
U+061C, U+200E/U+200F and U+2028/U+2029; the focused test covers those scalars.
Main added the seven-part release contract. This slice remains UNRUN until
its own exact-SHA Actions jobs execute; no production guard change is included.

Read-only upstream source inspection found Microsoft WSL master
LxssConsoleManager.cpp uses ConsoleId zero for clients without a console handle
and keys session leaders by ConsoleId/elevation. This is only a hypothesis lead:
it does not establish the Actions inbox WSL1 implementation or shared-Job cause.
No creation flags, Job ownership, process inventory or cleanup policy changed.

Public-comparison commit fe25593c4e5149116f5d6e61079dead842a62e96 started
CI34026585804 and Package34026585781. Linux101468522710 failed formatting;
same-run full artifact9987251928 was downloaded/read and applied in entirety
from `/home/leo/.cache/mini-term/artifacts/fe25593-integration/rustfmt/full-rustfmt.patch`.
Only tests.rs changed in that generated patch. Rootfs101468522782 succeeded;
Windows101468522620 is compiling and actualWSL101468664122 is importing.
No local formatter, syntax/test/whitespace check or native launch occurred.
Main will push the formatting successor only after owned import completes.

Formatting successor35fe65367c122e011f12154a5da2e36530dda575 was pushed only
after fe25593's import completed. Superseded WSL101468664122 cancelled during
test compilation and cleaned only mt-tasks-34026585804-1 successfully10:11:54Z;
its actual test did not run. Successor CI34026742549/Package34026742604 started.

Actual WSL101469109594 executed exactly one test and failed25.72s at LookupCancel.
First readiness147-103183us retired103113-103145us Count8, private NotInJob/Alive.
Private attached1997us, exited113517us(-1), retired113541us, drained119651us
with false ack/stdout118/stderr0 and Unknown native classification. It returned
HostHelperUnavailable119686us before cancellation236160us. Second readiness
153336-236159us was ready, Count1/private exited. The failed-case descendant
assertion was not reached; earlier lifecycle assertions passed by source order.
Owned mt-tasks-34026742549-1 cleanup succeeded10:22:54Z.

The isolated PUBLIC pair also failed: producer start marker present/end absent,
exit-1, complete bounded capture; its single readiness probe exited0 at103400us.
The strict public-only preview reported 'The Windows Subsystem for Linux instance
has terminated.' No private bytes were reported or inferred from their length.
This establishes non-credential reproduction in the post-failure fixture, not
yet Job-retirement causality or proof about another WSL version. Linux101468976555
completed SUCCESS, including mt-ai244, mt-app1219 plus5 separately executed SSH
fixtures, sidecars and whitespace. Windows101468976679 and package101468961657
were still running at the latest check; no new ordinary-Windows pass is claimed.

The user questioned the WSL scope. Main confirmed from pre-task history that
c17b22a (2026-05-25) already supported Windows WSL UNC project roots/terminals,
but acknowledged the original screenshot feedback did not request WSL expansion.
The user then explicitly authorized solving this WSL issue together. Main added
the scope clarification to the parent PRD and dispatched a bounded code-side
causal-test proposal; no production Job policy change is approved yet.

Read-only Microsoft sources provide a stronger lifetime lead than the earlier
console-id hypothesis: LxssInstance.cpp opens the calling process with
PROCESS_CREATE_PROCESS | SYNCHRONIZE and passes it to LxssClientInstanceStart;
lxssclient.cpp describes it as the parent process for the instance and forwards
it in StartParentProcessHandle to the driver. That does not expose the driver's
Job inheritance implementation or prove the runner's precise kernel behavior.

All35fe653 ordinary/package gates have now completed SUCCESS. Windows101468976679
passed onboarding82/3, Files49/14/5/1, execution-host19, Tasks executor40 plus1
separate ignored WSL test, Tasksapp/config/domain24/2/31, Git24/6/124 and
terminal-host31. All eight new public-comparison tests explicitly passed.
Package101468961657/run34026742604 artifact9987655617 is
Mini-Term_1.2.2-ci.64_windows-x64, downloaded only to
`/home/leo/.cache/mini-term/artifacts/35fe653-integration` after one transient
archive-download EOF. The passed Actions manifest records installer18,849,232
bytes and SHA256
`67e612b341414b1abfc885f8d0c091038e57f0b2d7fc8285e0dc88e55a72b316`.
No local verification/hash/app/installer launch occurred. Main updated the
validation candidate without claiming a full WSL or native acceptance pass.

Main approved Socrates' bounded two-row SINGLE-PROBE public retirement-timing
contrast: Immediate versus an on-stack wait after normal root exit/readers,
released by producer return/abort or absolute10s deadline. No Job handles leave
the readiness stack; actual TerminateJobObject/Drop remain unchanged. Both public
rows use identical fixed commands and one probe (normal0/1 eligible), maximumsix
host commands; original PRIVATE polling and all assertions remain untouched.
Main added the narrow release-contract exception and curated context. Newton
owns implementation only in execution_host.rs/tests.rs plus its handoff. Noether
separately reviews conditional production containment guarantees; no production
flag change, dependency or shared-distro lifetime policy is approved yet.

Noether's independent containment review identified a separate Git receipt gap:
typed WSL exit -1 was classified Completed, allowing successful reconciliation
to release a possibly still-running write's lease. Main authorized the narrow
classification correction independently of the timing experiment. Tesla released
host.rs/tests.rs plus the Git-child handoff: only typed WSL Some(-1) newly becomes
Uncertain, with a production-classifier matrix and real coordinator/exact-ID
review regression. Native, SSH, positive WSL exits and write.rs remain unchanged.
Main source-reviewed the release and updated the Git host contract. Automated
verification is UNRUN; Noether now owns its independent source review.

Noether's read-only Newton WIP check found attempt-wide public preview suppression
missing and old tests still being migrated. Main relayed both findings before
release; a sentinel in either row, including bounded bytes discarded for an
incomplete receipt, must suppress BOTH rows' previews. No additional hold/RAII
or production-cleanup change was found in that snapshot. Newton still owns both
diagnostic source files; no CI run or production containment approval follows
from this WIP review.

Newton released/froze execution_host.rs, the nested public-comparison module
and the Git-child wsl-retirement-implementation.md handoff. Both WIP findings
are addressed in the authored release, including suppression carried across
discarded incomplete/stale public receipts. Eighteen focused regressions are
authored/migrated; none has executed. Main reviewed the narrow hook placement,
unchanged production ProcessTree methods and baseline private assertions, then
transferred the two source files to Noether for final combined check. Tesla's
Git correction passed independent source review without findings. No production
containment policy or new passing Actions evidence exists yet.

Noether released the final combined check with no unresolved source blocker.
It added a test-only abort-before-join owner for held workers and strengthened
the existing unwind regression, preventing failed controller assertions from
detaching a native test worker. The eighteen diagnostic functions and both Git
safety tests remain UNRUN. Main read the final report and guard, retained all
original private/production behavior, and is preparing the scoped Actions
candidate. Unrelated pre-existing dirty files remain outside this submission.

Scoped candidate f1fde4dc8e7a3e3560acc57aa3009fbef9ef0490 was committed/pushed,
starting CI34030386551 and Package34030386556. Linux101478701475 failed its
formatting gate. Main downloaded exact-run artifact9988409408, read the entire
full-rustfmt.patch and applied it mechanically to execution_host.rs, Git tests
and Tasks tests only. Rootfs101478701638 succeeded; WSL101478843292 completed
owned import and is in the actual-test step. Windows101478701601 is compiling.
The formatter successor is intentionally not pushed before the timing result;
no local checks or WSL operations occurred, and no diagnostic pass is claimed.

Main committed the complete generated formatter patch as551595b without push,
so f1fde4d's running jobs were not cancelled. ActualWSL101478843292 completed
one test, failed22.07s at the unchanged DataCancel assertion. Private returned
HostHelperUnavailable163576us, before cancellation298938us; first readiness
retired145955-145982us Count5 while the private root was NotInJob/Alive. Private
exit-1 at155006us still had no stop or valid acknowledgement; no private bytes
were exposed. Owned mt-tasks-34030386551-1 cleanup succeeded11:40:53Z.

The public timing contrast supplied positive retirement-interference evidence.
Immediate: empty probe exit0 completed135056us, retired135091-135116us Count4;
producer returned143955us exit9 with only the start marker. AfterProducer:
equivalent empty probe exit0 completed134642us and held; producer returned
1086362us exit0 with exact start/end markers, then released1086394us and retired
1086405-1086410us Count0. Both timing_issue fields were null. This single
post-failure WSL1 contrast supports the retirement boundary, not a kernel member
identity, universal causality or WSL2 compatibility. Original failure stayed red.

Main authorized the previously reviewed typed WSL-client-root policy candidate:
WSL-only KILL_ON_JOB_CLOSE | SILENT_BREAKAWAY_OK, preserving suspended exact-root
assignment/resume and all fallible root cleanup. Newton owns execution_host.rs
and private process.rs; Tesla owns appended actual peer-survival fixtures in
Tasks tests.rs. No private envelope/control/ack, original lifecycle assertions,
PTY, native/SSH, dependency, workflow or production-delay change is authorized.
Main added the production ownership contract with explicit escaped-descendant,
ordinary guest-stop and WSL2/interop/nested-Job limits. Source and tests are being
authored; this candidate has not run in Actions.

All f1fde4d jobs have now completed. Windows101478701601 succeeded: onboarding
82/3, Files49/14/5/1, execution-host25, Tasks executor44 plus1ignored actualWSL,
Tasksapp/config/domain24/2/31, Git24/6/126 and terminal-host31. All eighteen
public timing/comparison regressions and both new Git safety tests explicitly
passed. Package101478701516/run34030386556 succeeded; its installer was not
downloaded or launched. Linux remains failed at formatting and actual WSL remains
failed at the original assertion, so no full compatible candidate is claimed.
The generated formatter commit551595b stays local until the production source
and fixture slices are reviewed, avoiding an unnecessary intermediate CI run.

Noether's independent read-only production WIP check found no concrete typed
routing/flag/root-cleanup/private-capture defect in Newton's current slice.
Final check still awaits both releases. Main's fixture-contract review requested
short/private-client-first startup in Tesla's new peer cases, with marker-proven
overlap before retirement and a still-pending peer afterward. This strengthens
the cleanup regression but does not force cold WSL startup; no distro restart
or original-loop change is authorized.

Newton released/froze the production policy and five focused tests. Main read
its exact diff/handoff; Noether's partial final check found no defect. Tesla
then released/froze eight client-first peer-survival rows appended only after
the entire unchanged original lifecycle loop, plus three ownership/plan/join
tests. Main reviewed both overlap guards and the exact bounds:12 fresh cases,
8 finite15s peers/20s deadlines,4 short5s calls,4 private requests,12 descendant
postcheck commands,16 capped10s readiness loops; at most3240 adapter calls.
The healthy peer sleeps add about two minutes. No public body/private bytes log,
original assertions or diagnostic holds change, and setup may warm the instance.
Main documented these limits and transferred all three source files to Noether
for the final combined check. Current production/fixture execution is UNRUN.

Noether completed the final combined containment check with no remaining source
finding and no further source fix. It released/froze all three source files and
the current review report. Main read the final report, checked the bounded
typed-only fixture diagnostics, and is submitting the scoped production patch
together with the already committed formatter successor. All eight new units,
eight actual peer rows and the unchanged actual WSL baseline still require
fresh exact-SHA Actions proof; no native UI or broader WSL acceptance is claimed.
