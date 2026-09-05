# Fix Project Sidebar Visibility and Agent Status

## Goal

Make the project sidebar configurable and accurate: automatically show new
valid worktrees, let users hide unwanted rows, exclude invalid entries, and
correct remote Agent recognition and misleading startup status flashing.

## Background

The user approved task creation on 2026-09-05, not implementation. The latest
decision explicitly supersedes the proposed Orca-style default-hide policy:
new valid worktrees show automatically, and hiding is manual.

The reference screenshots are:

- Orca v26.019 worktrees:
  `/home/leo/.cache/tmp/orca-paste-1788582316064-c5b9f242-47ba-495f-999e-f31dfa8d453a.png`.
- Mini-term AICOS worktrees, including a prunable row:
  `/home/leo/.cache/tmp/orca-paste-1788610151597-b1aebdbd-ae15-4afb-9bcc-1c88e16692e4.png`.
- Orca project-settings menu:
  `/home/leo/.cache/tmp/orca-paste-1788610690574-105422f2-fedb-43be-9b19-075c39de865e.png`.
- Reported status flashing:
  `/home/leo/.cache/tmp/orca-paste-1788582261130-8a62636b-8a65-498a-9726-4e96fb932681.png`.

All images are decoded. The status image shows `Refreshing`/`last known`
with yellow dots; a still frame does not prove Agent animation or its cadence.
The v26.019 and AICOS directories share an upstream URL but have separate
local Git inventories. The v26.019 branch count is not the AICOS/upstream count.

[Worktree research](../09-05-sidebar-worktree-discovery/research/orca-visibility.md)
records inventory and visibility evidence.
[Status research](research/remote-agent-status.md) confirms a Linux probe
globbing defect and legacy/rich-event ordering gap, and traces the catalog
warning path. The exact running application's startup symptom is not yet
reproduced or claimed fixed.

## Requirements

- R1: Separate Git branch refs, discovered worktrees, and sidebar presentation.
  Use Orca as a reference without a fixed three-row limit.
- R2: Correct remote process recognition, accepted-state projection, and
  misleading catalog/status changes while preserving genuine working, idle,
  waiting, error, attention, and connectivity information.
- R3: Preserve project/worktree identity, Local/WSL/SSH ownership, configured
  projects, terminal sessions, and unrelated user changes.
- R4: Complete planning and obtain review approval before implementation.
- R5: Exclude invalid entries by default and provide persistent individual
  worktree visibility through project settings. Offline is not invalid.
- R6: Show newly discovered valid worktrees automatically unless manually
  hidden. Do not implement ownership-based or Agent-directory default hiding.
- R7: All CI, compilation, tests/fixtures, lint/format checks, code generation,
  packaging, and automated verification run exclusively in GitHub Actions.
  This is a hard constraint for the main agent and all sub-agents; no local,
  container, or manually SSH-dispatched substitute is allowed.

## Task Map and Dependencies

- `09-05-sidebar-worktree-discovery` owns R1/R5/R6, project settings, and the
  catalog freshness portion of R2.
- `09-05-sidebar-agent-status` owns R2's process recognition, semantic activity,
  reconciliation, lifecycle, and status presentation.
- This parent owns R3/R4/R7, source requirements, and final integrated
  acceptance. Both children inherit the R7 execution constraint.

Each child is independently testable. Execute visibility first, then status,
to serialize shared sidebar edits; this is not a logical data dependency.
Integrated startup/refresh validation explicitly depends on both children's
results. The parent is not a separate product implementation target.

## Acceptance Criteria

- [ ] Only worktree inventory supplies rows; no branch enumeration or numeric
  cap is introduced (R1).
- [ ] Invalid entries are excluded; project settings hide/unhide individual
  rows and preserve choices across restart/refresh (R5).
- [ ] New valid rows appear without importing; hiding one does not suppress
  later discoveries (R6).
- [ ] Idle or ended Agents do not remain falsely live-working; genuine work,
  attention/error, and remote connectivity remain distinguishable (R2).
- [ ] Healthy refresh does not create a false warning/Agent transition, while
  actual failures and stale targets remain handled safely (R2/R3).
- [ ] Worktree visibility and Agent navigation preserve exact runtime ownership
  and do not close sessions or delete user data (R3).
- [ ] Both children pass scoped checks, exact-commit CI, and combined native
  startup/settings verification before completion (R2/R3/R4).
- [ ] Every automated validation/build result comes from Actions for the exact
  product commit; any manual acceptance uses its produced artifact (R7).

## Out of Scope

Orca source/private metadata changes; Git creation, checkout, deletion or
pruning to alter counts; unrelated sidebar/global settings/search redesign;
copying every Orca menu command; a replacement Agent runtime or remote Hook
protocol; unrelated metadata churn.

## Risks and Deferred Validation

The original flashing cadence requires a native trace tied to the tested
binary. Presence plus PTY heuristics is not universal provider-semantic
telemetry. The technical design defines bounded accepted-event and lifecycle
corrections while preserving unsupported/heuristic fallbacks.

No unresolved product decision remains. The user explicitly approved the final
planning summary on 2026-09-05. Design and execution details are in `design.md`
and `implement.md`; children are the implementation targets.
