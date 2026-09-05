# Final Navigation Source Review

Bounded source review on 2026-09-06 by the existing trellis-check reviewer.
No agents were spawned. Local activity was source reading, apply_patch edits,
and read-only Git inspection only. All automated checks are UNRUN. This report
is not compiler, CI, fixture, native-acceptance, or quality-gate proof.

## Scope And Context

- Affected package layers: mt-app, mt-config, mt-layout, mt-ui, mt-i18n, and
  mt-terminal-host. The lack of configured package metadata does not reduce
  this six-crate scope. No package-discovery command was run locally.
- Read the current PRD/design/implementation plan/check context, lifecycle
  handoff, core review, sidebar/tooltip review, full view review, and tools
  follow-up handoff/review. Loaded the terminal-host spec index and contract in
  addition to the identity, layout, context, file-workbench, tooltip and quality
  contracts. The earlier core and view reviews remain part of this source gate.
- This follow-up concentrated on Halley's confirmed-close, dormant host cleanup,
  logical-session reservation and shellless-reconnect changes. Representative
  current titlebar, terminal-body, workbench and global-context wiring was also
  read against the completed view/tool reviews.

## Findings (fixed)

### P1: Cleanup errors released the host close reservation

- Evidence: `crates/mt-terminal-host/src/server.rs:1000` previously removed
  `Registry::closing` regardless of the close result. A registered close may
  already mark its lifecycle terminated before returning a kill error. Releasing
  the fence then lets a retry infer success from that terminated state.
- The cancelled cold-restore branch at `server.rs:915` similarly recorded purge
  failure only when a registered/uncommitted session existed. An identity-checked
  cold-history mutation error could therefore release its reservation too.
- Fix: Registered-session cleanup errors and cold mutation `IoFailed` retain the
  closing fence until host restart, including cancelled cold-restore cleanup.
  Non-mutating metadata/identity refusals still release it and remain retryable.
  Successful cleanup removes the registered session only after quiescence/purge.
- Authored two regressions: `failed_cold_cleanup_keeps_the_close_fence_until_restart`
  covers direct close and cancelled restore, rejects subsequent close/create/
  restore/attach lookup, and permits cleanup only after simulated restart;
  `unidentified_cold_history_refusal_does_not_reserve_a_mutation` distinguishes a
  repairable pre-mutation refusal. Both are UNRUN.

### P1: Pre-existing newer alias data could be overwritten by close

- Main's additional audit point is confirmed. `store/layout.rs:360` unconditionally
  makes the saving project the latest dirty worktree owner; `:431` clears that
  owner map on flush. The old close snapshot compared other aliases only across
  the request, not against the source before the request began.
- A Mobile append in B before A captures its close request can therefore be
  invisible to the old conflict check. If the shared source stays dormant in B,
  its attachment guard also does not detect B's different new terminal. Closing
  A then schedules A's older inventory over B's shared layout.
- Fix: `store/panes.rs:53` captures the source layout independently from route
  identity and requires it to agree with the captured alias layouts. Another
  pending save owner also refuses close. The guard runs before disposal, host
  dispatch or record-only removal; async completion rechecks the pending owner
  alongside the existing alias-change fence. Refusal retains an explicit runtime
  Error record and bounded notification without saving over the other alias.
- Same-owner selection/order changes after capture remain allowed: dispatch
  compares route/source/attachment plus other-alias snapshots, not the evolving
  source navigation snapshot. Successful background removal keeps current
  selection and does not hydrate siblings.
- This deliberately fails closed for pre-existing divergent aliases even if the
  source might be newer. After flush there is no retained revision proof here.
  No alias synchronization, new persistence writer, public API, or wire protocol
  was introduced, and other alias operations were not redesigned.
- Authored `preexisting_alias_append_blocks_close_before_and_after_layout_flush`
  using the production background-append and close-authorization helpers.
  Strengthened the existing selection/order test with matching alias baselines,
  changed source snapshots, and competing pending-owner rejection. All UNRUN;
  the pure tests do not execute AppStore scheduling or disk flushes.

### P2: Pending-close terminal jumps changed scope before rejection

- Evidence: `crates/mt-app/src/store/context.rs:615` resolved a pending target
  without a pending-close check, then called `set_active_project_without_hydration`
  before `activate_pane_inner` could reject it. `store/projects.rs:93` changes active
  project/worktree and the remembered project without clearing terminal focus.
  The dormant branch's void `activate_pane` call did not reflect that rejection.
- A jump to a pending-close terminal in another project could thus switch scope
  even when it ultimately returned false. If the remembered focused PaneKey still
  matched, its final identity/focus comparison could also return true and let the
  outer wrapper reveal the terminal page despite rejected activation.
- Fix, after main's explicit narrow authorization: `context.rs:621` returns false
  for a pending logical session before any project/worktree mutation. The outer
  activation wrapper therefore never hands off the page for that rejected jump,
  even when remembered focus matches. No await separates the new guard from the
  existing full-target checks and activation dispatch.
- The shared `resolve_terminal_jump_target` remains unchanged and still resolves
  pending identities for close's own snapshots. Close token ownership/release,
  completion snapshot ordering, pre/post-switch full identity checks, exact-live
  behavior, and ordinary original-owner dormant hydration are unchanged. No
  public method signature or test-support dependency was changed.
- Existing pure token/alias ownership and exact-target regressions remain
  authored/unrun. These context/close fixtures do not exercise a GPUI Window;
  no artificial helper or source-string test was added just to claim coverage.
  Actions/native coverage must still establish no project/worktree/page/focus
  mutation for pending same-project and other-project/alias jumps, successful
  navigation after the owning token releases, and unaffected close completion.

## Findings (not fixed)

No remaining concrete source blocker was identified in the bounded scope after
the authorized navigation-entry fix. Automated and native gates remain UNRUN;
the absence of another source finding is not passing verification evidence.

## Lifecycle And Navigation Handoff

- The earlier dormant-close P1 is now addressed in source: confirmation captures
  the full target and GUI attachment; only the background executor calls the
  existing host Kill client for dormant local records. Dispatch and completion
  revalidate route, binding, configured source, incarnation and aliases. Every
  host error, including old-host SessionMissing, retains the record. No new
  failure-to-success mapping, endpoint fallback or GUI history deletion exists.
- Session-keyed AppStore reservations cover duplicate close, navigation before
  project/page handoff, pane activation, eligible hydration and reconnect across
  aliases. Existing other-alias attachments fail closed before Kill.
- Disabled/unavailable hosting with a saved local incarnation deliberately
  retains the record, per main's approved boundary. SSH and no-incarnation
  records keep record-only close. Existing attached disposal was not redesigned.
- Host Kill validates bounded cold metadata and expected incarnation without
  spawning. Closing/creating reservations cover cleanup; matching restore
  cancellation reports HostBusy until cleanup ends. Unvalidated stale cold
  cancellation cannot authorize purging a different incarnation. History
  invalidation and its state lock fence writers before purge.
- The earlier reconnect P2 is addressed at `store/ssh.rs:351`: complete source/
  route, shell and CWD preflight occurs before disposal. A missing shell or route
  preserves the attachment and saved identity. Pure preflight regressions are
  authored, not real GUI/transport proof.
- Approved hydration policy is unchanged: ordinary dormant activation and a
  selected dormant neighbor may recover eligible records in the original owner.
  Exact-live activation, inventory rendering and background close do not hydrate
  siblings. Pending-close sessions are excluded, not a selected-only policy.
- The prior complete view review remains applicable: one selected live body;
  original TabId/PaneKey route owners; complete flat inventory/selection/order;
  optional preference salvage and idempotent persistence; exact close/fork/
  reorder callbacks; pending fork lineage before write; Mobile background append;
  global ContextPanel; shared tooltip/footer/menu behavior. Startup focus uses
  singular selection. Search tools use the corrected flex tooltip anchors, and
  the obsolete animation setting row is gone while saved compatibility remains.
- Known unused compatibility helpers from the view report were not refactored.
  No removed split/merge/group API or runtime caller was reintroduced. Prior
  implementer/reviewer changes were preserved.

## Changed Files

This final follow-up changed only:

- `crates/mt-terminal-host/src/server.rs`: cleanup-error reservations, comment,
  and two new tests.
- `crates/mt-app/src/store/panes.rs`: close-only alias authorization, shared
  runtime conflict reporting, one new test and one strengthened test.
- `crates/mt-app/src/store/context.rs`: authorized pending-close rejection before
  navigation mutates project/worktree, without changing shared identity resolution.
- `.trellis/tasks/09-06-native-terminal-navigation/final-review.md`: this report.

Earlier review changes remain documented in core-review.md. No specs, parent
documents, task metadata, generated dictionary, dependency manifest, protocol,
workflow, staging, commit, push, reset, or CI dispatch was changed/performed here.

## Verification And Remaining Gates

- Lint/Clippy: UNRUN, GitHub Actions only.
- Build/type-check/Cargo metadata: UNRUN, GitHub Actions only.
- Tests/fixtures/probes: UNRUN. Halley's 14 new tests and two race updates remain
  unexecuted here. This follow-up adds three new tests and strengthens one. Prior
  core/view/tooltip/search tests remain authored/unrun, not passing checks.
- Format/whitespace/codegen/i18n: UNRUN. Main must use Actions diagnostics for
  changed-line rustfmt and generated dictionary patches; none was generated or
  checked locally. App launch and native acceptance are also UNRUN.

Main's precise remaining execution gates:

1. Obtain exact integrated commit evidence from `.github/workflows/ci.yml`,
   including the authorized navigation-entry fix: changed Rust formatting,
   generated i18n dictionary, staging tests, locked root/sidecar Cargo graphs,
   workspace all-target check/test, affected-package Clippy, sidecar check/test,
   whitespace, and Windows MSVC affected-package checks. Include the new host
   cleanup-error and alias-before-request regressions in those test results.
2. Exercise host cleanup/race fixtures on the required native platforms in
   Actions. The existing Windows job's focused onboarding tests are not terminal
   host race execution. Cover live/cold reservations through real writer cleanup,
   cancel-before/after-spawn, uncertain cleanup, old-host errors, and malformed/
   symlink metadata. Linux-only results do not establish Windows behavior.
3. Use `windows-package.yml` or the applicable `release.yml` for matching packaged
   app/terminal-host/sidecars/ConPTY payload and installer verification. Record
   run URL/ID, headSha, job conclusions and exact artifact identity; unrelated
   green runs do not validate this source.
4. Using matching Actions-produced artifacts, cover confirmed close/cancel after
   switch/rebind/reconnect, alias changes both before and during close, pending
   navigation, no focus theft on background completion, selected/neighbor/last
   close, GUI restart with dormant hosted sessions, shellless reconnect and
   continued background output. Pure planners do not prove those callbacks.
5. Retain the view/tool acceptance matrix: exact-live no-hydration, ordinary
   original-owner recovery, non-first-owner restart, fork CWD/lineage/write races,
   Mobile focus, keyboard/tab overflow, drag cancellation, document/search focus,
   file drops and IME; every right ContextPanel across project/worktree switches;
   all shared tooltip icons/gaps, 500 ms first/warm-next behavior, close/reopen/
   refocus/unmount/deactivation/occlusion, long labels, narrow/high-DPI geometry
   and Windows native controls. Automated harnesses and fixture processes remain
   Actions-only; no local app was launched for this review.

Bounded source review is complete, including the authorized cross-file P2 fix,
and ready for main's Actions candidate. This handoff does not wait for CI and
does not mark the quality gate passed.
