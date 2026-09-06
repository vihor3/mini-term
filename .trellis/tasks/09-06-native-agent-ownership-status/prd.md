# Correct owned terminal Agent status and runtime titles

## Goal

Resolve feedback items 1, 2, 3, and 16: quiet routine refresh, semantic Agent activity, Mini-Term terminal ownership, and exact runtime titles.

## Requirements

- Own parent requirements R1, R2, R3, and R16. Inherit all constraints and scope
  decisions in [the parent PRD](../09-06-native-ui-remote-feedback/prd.md).
- Keep healthy automatic catalog refresh visually quiet, separate from manual
  refresh and actual error/connectivity reporting.
- Establish logical Agent ownership and evidence priority before projecting
  activity. Do not assign the same terminal-output heuristic to every process
  as if it were task-semantic evidence.
- Monitor Mini-Term-owned terminals across foreground and background projects;
  exclude Agents from unrelated external terminals. Preserve exact route,
  incarnation, and lifecycle rejection rules.
- Runtime titles must come from the owning pane/run or exact session identity,
  not the newest unrelated conversation or an undifferentiated SSH host label.

## Evidence

### 2026-09-07 Native Regression

The user reports a Mini-Term-owned SSH Codex terminal displaying its native
`Working (2s ... esc to interrupt)` status while the exact sidebar Agent stays
`Unknown` in the Actions-built native layout package. Fix this within R2; keep
the approved layout and ownership scope unchanged. Process attestation alone
must remain Unknown, but reliable live Codex task-state evidence must reach the
existing owned semantic observation boundary.

[Source-only research](../09-05-sidebar-agent-status/research/09-06-native-feedback.md)
records the static refresh glyph, terminal-wide output recency, route-filtered
process inventory, and current Runtime label fallback. Orca's managed PTY,
foreground, semantic evidence, and exact-title lookup are references; its
additional orchestration rows are not automatically in scope.

## Acceptance Criteria

- [ ] Successful automatic scans with no Agent do not flash the header (R1).
- [ ] Quiet input prompts, unrelated shell output, redraws, real work, and
  attention states do not collapse into the same Working state (R2).
- [ ] Wrapper/native-child duplicates and independent sessions are distinguished
  by ownership evidence, not provider/cwd guesses (R2/R3).
- [ ] Switching projects retains monitoring of owned background terminals;
  external terminals and stale incarnations remain excluded (R3).
- [ ] Runtime rows use exact available titles with safe, distinguishable
  fallbacks; legacy split terminals and duplicate SSH labels do not share titles (R16).

## Out of Scope

Device-wide external Agent feeds, unowned history-title inference, blanket
animation disabling, a replacement runtime, or new remote Hook-secret forwarding.

## Risks

Some providers expose no reliable semantic activity; unknown must remain distinct
from Working. Process-only fixtures cannot establish actual native UI cadence.

## Execution Status

The user explicitly approved the final parent scope on 2026-09-06. This child is
now activated after navigation's source-complete `d50f616` handoff. Lower-layer
implementation starts while that navigation candidate runs in Actions; shared
app files remain reserved until its diagnostics and API handoff are coordinated.
No native reproduction or automated checks are claimed; all compilation,
tests/probes, lint/format, and packaging remain Actions-only.

The 2026-09-07 Codex follow-up is implemented, independently reviewed and
Actions-validated on product `5186575`. The matching `1.2.2-ci.72` installer is
downloaded. See `validation-09-07-codex.md` for all five successful CI jobs,
18 passing new Linux/Windows regressions and exact package identity. Native
acceptance remains open; the parent scope and approved layout are unchanged.
