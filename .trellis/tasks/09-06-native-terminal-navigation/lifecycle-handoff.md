# Terminal Lifecycle Handoff

Source-complete lifecycle slice for focused review and Actions diagnostics.
Source edits only; all automated execution remains GitHub Actions-only. No
local tests, probes, builds, formatting, whitespace checks, app launches, Git
mutations, or CI dispatch. No agents were spawned.

## Fixes

- P1: `pane_actions` captures an exact close request before confirmation,
  including the GUI attachment handle. Confirmation dispatch and asynchronous
  completion validate the original project/worktree/host/TabId/PaneKey/session/
  incarnation, binding provenance, configured source, and alias snapshots.
  A warm attachment installed during confirmation makes the old request inert.
- Dormant hosted close calls only the existing client's fenced `kill` on the
  background executor. It does not create a terminal entity, attach, recover,
  spawn a shell, or hydrate siblings merely to close. Only `Ok` authorizes saved
  record removal; every host error, including old-host `SessionMissing` and
  `SessionExited`, preserves the record and produces bounded deduplicated UX.
- AppStore retains a logical-session-keyed close token. Duplicate close,
  activation, ordinary hydration and reconnect cannot attach or replace that
  session through another alias until its owning completion releases the token.
  Existing alias attachments reject close before host dispatch.
- Same-project selection/order are not part of the close snapshot. Selecting
  another tab during close permits completion, removes only the intended
  background record, preserves current selection and skips neighbor hydration.
  Alias snapshots include already-flushed layout changes; a confirmed close
  with a conflicting alias mutation retains the exact record as runtime Error
  and notifies without persisting over the newer alias. Changed original
  source/route/attachment is inert, never reinterpreted as a replacement target.
- Host `Kill` now reserves the logical session across live or cold cleanup.
  Competing close/create/restore/attach fails closed until history invalidation,
  writer quiescence and purge finish. The registered session is not forgotten
  before its successful close. Cold cleanup validates bounded metadata and
  expected incarnation; no directory is idempotent success, but an unidentified
  or malformed existing history directory is not absence.
- Close racing restore marks that restore cancelled and returns HostBusy,
  not premature success. The restore reservation remains through cleanup of
  old/uncommitted sessions and history; only a later retry can confirm absence.
  Cancelling a cold restore before its own identity validation does not grant
  permission to purge history belonging to another incarnation.
- P2: reconnect captures its complete original route, shell and CWD before
  disposal. Missing shells or missing routes leave the current attachment,
  view and saved identity untouched; the exact close guard is not weakened.

## Boundary Choice

- Keep the existing session/incarnation-fenced `Kill` protocol. Extend
  its host-side record handling: absence requires absence of the entire
  history directory, while an existing directory requires valid bounded
  session/incarnation metadata before invalidate/purge. Exclude create/restore
  during that operation. A registry `SessionMissing` error is not close success.
- Dormant local/WSL records carrying an incarnation require asynchronous host
  confirmation before removal. Disabled/unavailable hosting is not proof of
  absence, because saved panes do not record the transport kind; retain the
  record and report a bounded error. SSH compatibility and unsaved/no-incarnation
  records remain record-only and never issue host kill. Main accepted this
  conservative boundary explicitly during implementation.
- Pending close ownership is retained in AppStore and keyed by logical session
  to exclude duplicate closes and alias hydration/reconnect. Complete target,
  attachment, project source and alias state are revalidated on completion.
- Keep ordinary eligible original-panel hydration. Do not hydrate just to close.

## Files Changed

- `crates/mt-app/src/pane_actions.rs`: confirmed asynchronous close handoff.
- `crates/mt-app/src/store/panes.rs`: close snapshots, decisions, tokens, fences,
  errors, completion and focused pure tests.
- `crates/mt-app/src/store/ssh.rs`: read-only reconnect preflight and tests.
- `crates/mt-app/src/store/mod.rs`: retained close state and initialization only.
- `crates/mt-app/src/store/projects.rs`: one authorized test initializer field.
- `crates/mt-terminal-host/src/server.rs`: fenced cold close, closing/restore
  reservations, focused lifecycle tests and existing race assertion updates.
- `crates/mt-terminal-host/src/history.rs`: bounded stored-identity reader and
  metadata/symlink regressions.
- This handoff. Existing edits in the shared worktree are preserved; no view,
  locale, spec, task-metadata, protocol/client, PTY or SSH transport file changed.

## Authored Tests

14 new tests plus two existing restore-race tests updated, all UNRUN:

- `store/panes.rs` (6): Local/WSL/SSH/no-host decisions; session-keyed ownership
  and stale completion; missing/exited/transport/protocol error rejection;
  every captured route/source/attachment component; same-owner tab selection
  and reorder during close versus alias conflict; competing alias attachments.
- `store/ssh.rs` (2): missing-shell preflight preserves saved/current identity
  and attachment; missing-pane preflight never invokes shell resolution.
- `server.rs` (4 new): cold close is fenced/idempotent/no-spawn; both live and
  cold close exclude concurrent kill/create/restore/attach; an uncommitted
  create cannot lose its history; stale cold-restore cancellation cannot purge
  another incarnation. Two existing cancel-before/after-spawn races now expect
  HostBusy until cleanup completes, then require an explicit successful retry.
- `history.rs` (2): distinguish no directory from missing/corrupt/oversized/
  wrong-session metadata, and reject symlinked directories/metadata.

## Verification And Limits

- Performed source/API reading and read-only Git diff review only. No claimed
  compiler, lint, format, test, fixture, whitespace or native UI result.
- Main owns focused review and the existing Actions gates for the exact product
  commit, including Linux tests and Windows compilation/package verification.
  Pure app tests do not prove real GUI task scheduling, live entity retention,
  focus restoration, project rebinds, or actual alias/IPC timing. Host fixtures
  and existing lifecycle integration tests still need execution in Actions.
- Original eligible original-panel dormant hydration remains unchanged except
  temporarily excluding sessions with pending explicit close. Exact-live
  activation/inventory/background close still cannot hydrate siblings.
- Disabled/unavailable hosting deliberately prevents deleting an incarnation-
  bearing dormant local record. The saved format has no transport provenance
  that could safely distinguish a prior compatibility session from a surviving
  hosted session. No endpoint discovery fallback or GUI-side history deletion.
- Corrupt/missing identity metadata in an existing history directory remains
  fail-closed; this slice does not invent destructive recovery or forced purge.
- An uncertain cancelled-restore cleanup leaves a host-side closing reservation
  rather than allowing another request to report success from missing registry
  state. That exceptional reservation requires host restart before further
  recovery; no automatic host shutdown or destructive fallback was added.
- Existing attached-terminal close/reconnect disposal is retained. This slice
  does not redesign its pre-existing synchronous transport shutdown path.
- Error messages use fixed bounded text through the existing toast mechanism;
  no raw host error payload, launch argument, environment value or credential
  reaches the notification. Locale-source changes are outside this allocation.

No source integration blocker remains. This handoff is not passing CI evidence.
