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
| native-agent-ownership-status | Source reviewed / Actions pending | Full Agent slice committed in `8ab8c27`; producer receipts and explicit lifecycle resume independently reviewed, not yet executed |
| native-file-browser | Windows regressions passed / Remote pending | Source reviewed; onboarding and Files steps passed at `37b4ef9`; actual SSH and native acceptance remain open |
| native-remote-git | Source reviewed / CI repair | CLI parity passed; integrated Windows Git filter has 123 passes and one incomplete fake-command failure; Linux Clippy follow-up released |
| native-tasks-gh-accounts | Native Windows tests passed / WSL failing | Account isolation steps passed at `37b4ef9`; actual WSL executed once and failed in its prelude, secret-safe diagnostics authored |

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
