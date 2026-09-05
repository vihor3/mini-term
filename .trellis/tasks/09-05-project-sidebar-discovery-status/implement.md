# Project Sidebar Execution and Integration Plan

## Review Gate

- [x] Present the final summary covering goal, scope, exclusions, defaults,
  acceptance, risks, and artifact readiness.
- [x] Receive a subsequent explicit user approval of that summary. The default
  preference answer alone is not permission to start implementation.
- [x] Verify both children have PRD/design/implementation plans plus curated
  implement/check manifests with existing spec/research references.

The user explicitly approved the final summary with "可以" on 2026-09-05.
The Actions-only hard constraint remains mandatory. Start the owning child,
not the integration parent; no product changes preceded this approval.

## Execution Order

- [x] Capture dirty baseline and isolate the current task's write ownership.
  Preserve all unrelated changes and previous task artifacts.
- [x] Start `09-05-sidebar-worktree-discovery`; dispatch `trellis-implement`
  with native context injection (fallback pull if needed), then `trellis-check`.
- [ ] Review its scoped diff, contract updates, CI evidence, and native settings
  behavior. Hand off freshness/status boundaries before shared-file edits.
- [x] Start `09-05-sidebar-agent-status`; dispatch implementation with its own
  manifests and the visibility implementation handoff.
- [x] Dispatch the status child's independent review after implementation and
  the serialized sidebar integration.
- [x] Complete the approved scoped commits and Actions diagnostic corrections.
  CI and Windows packaging passed for product SHA `1ee49b8`; the matching
  `1.2.2-ci.30` installer is downloaded with runner validation evidence.
- [ ] Review process-probe execution, accepted-event ordering, lifecycle, and
  presentation evidence. Revisit planning if the required fix exceeds scope.
- [ ] Validate the combined startup/refresh/hide/unhide flow, because parent
  acceptance explicitly depends on both children.
- [ ] Complete spec updates, exact-commit CI review, native verification,
  scoped commit handling, task archival, and session recording via Trellis
  finish-work only after all required acceptance evidence exists.

The serialized order prevents conflicts in `orca_sidebar.rs`; it does not
imply that one child's data model depends on the other's implementation.
After the visibility implementer handed off, the status child may implement
its disjoint process/state files during the visibility static review. Its
sidebar edit is fenced until that reviewer explicitly releases the file.

## Integrated Test Matrix

| Scenario | Required result |
| --- | --- |
| AICOS default inventory | Nine valid rows shown, three prunable rows absent |
| v26.019 default inventory | Its own three valid rows, no AICOS inventory merge |
| Hide one, then discover another | Hidden row stays absent; new valid row appears |
| Save/restart/Cancel/unhide | Saved preference survives; canceled draft has no effect; row recoverable |
| Hide active remote worktree | Workbench and exact Agent route remain intact |
| Offline/retry/source change | No invalidity inferred from outage; no cross-source preference/event leakage |
| Repeated healthy catalog refresh | No false Agent/warning toggling; activation fencing preserved |
| Exact-route Agent launch/quiet/exit | Process recognized; accepted activity settles correctly |
| Late event/reconnect/PTY exit | No rejected update or old observer resurrects Working |

## Quality Gate

All CI, compilation, tests/fixtures, Clippy/lint, formatting/whitespace checks,
code generation, packaging, and automated verification are GitHub Actions-only
as a hard user constraint. Local work is editing, reading, and read-only Git
diff/status review, never a local/SSH/container test substitute. Verify
workflow `headSha` matches the exact
product commit; preserve lockfiles unless a separately justified dependency
change becomes unavoidable. Packaging follows the existing release/package
workflow when needed for the native validation build, never a local build.

Automated native harnesses run only in Actions. Manual acceptance may inspect
an Actions-produced artifact and its native trace/video or bounded diagnostics.
Do not claim Playwright covers this GPUI interface or claim a
source-only fix proves the original startup cadence resolved. Record unmet
runtime checks explicitly; do not archive merely because compiler tests pass.
