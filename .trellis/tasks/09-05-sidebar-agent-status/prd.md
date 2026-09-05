# Stabilize Remote Project Agent Status

## Goal

Recognize remote Agents accurately and prevent misleading persistent work or
flashing indicators at startup, reconnect, quiet periods, and exit, while
preserving genuine work, waiting, errors, and attention.

## Background

The user explicitly reports remote projects after mini-term startup/connection.
The decoded reference,
`/home/leo/.cache/tmp/orca-paste-1788582261130-8a62636b-8a65-498a-9726-4e96fb932681.png`,
shows `Refreshing`, `last known`, and yellow worktree dots, not an inline Agent
row. It does not establish a spinner or its cadence.

[Research](../09-05-project-sidebar-discovery-status/research/remote-agent-status.md)
records the confirmed defects, current guards, possible lifecycle gaps, and
native-runtime caveats. The exact startup behavior remains unreproduced.

## Requirements and Evidence

- A1: Trace recognition, ownership, aggregation, and marker provenance.
  `crates/mt-app/src/orca_sidebar.rs:486` reads configured project status and
  attention; this alone is not root-cause proof. Ordinary refresh demotion at
  `crates/mt-app/src/worktree_catalog.rs:420` drives the warning path at
  `crates/mt-app/src/orca_sidebar.rs:541`; the worktree child owns that fix.
- A2: Prevent false persistent activity while preserving genuine work,
  waiting, error, and attention; scope lifecycle corrections to demonstrated
  failures and current evidence authority.
- A3: Preserve exact run/pane/worktree/host/incarnation ownership and rejection
  of stale observations/completions.
- A4: Keep connectivity, liveness, activity, and attention distinct. SSH
  connection, polling, transport traffic, and restored session metadata alone
  must not establish ongoing Agent work.
- A5: Correct status independently of visibility. Hiding affected rows or
  disabling all animation is not a fix.
- A6: Restore exact-route Linux process enumeration. The confirmed probe defect
  is `crates/mt-ssh/src/agent.rs:359` disabling globbing before
  `/proc/[0-9]*` at `:367`, yielding a valid but empty inventory. Preserve
  safe argv/stat splitting, bounds, matching, and data minimization; validate
  actual generated-script execution, not only parser/string fixtures.
- A7: Reject late events before any legacy status, attention, or completion
  effect. The confirmed ordering gap is `crates/mt-app/src/store/ai.rs:489`
  mutating legacy state before registry submission at `:505`. Project the
  accepted current state rather than unchecked incoming status.
- A8: All CI, compilation, tests, generated-probe/fixture execution, lint,
  format, generation, packaging, and automated validation run only in GitHub
  Actions. Do not run a local or manually SSH-dispatched reproduction harness.

## Acceptance Criteria

- [ ] A no-input startup or restored idle remote session does not manufacture
  a live-working Agent or repeated false activity transitions (A1/A2/A4).
- [ ] A controlled Linux process is found by the generated probe; mismatched
  routes, wildcard-looking input, invalid framing, and unsafe output fail the
  applicable matching/protocol checks (A3/A6).
- [ ] Genuine activity and quiet/waiting/attention/error transitions remain
  observable on only the owning worktree (A2/A3/A4).
- [ ] A delayed Working event after accepted newer Waiting changes no legacy
  status, attention, completion notification, or accepted rich state (A7).
- [ ] Confirmed disappearance settles attested activity; later ordinary shell
  output does not resurrect its contradicted weak tracker state (A2/A3).
- [ ] Natural terminal exit stops local observation/poll updates without
  falsely declaring remote semantic completion (A2/A3/A4).
- [ ] Reconnect and failed/late probes preserve ownership and known semantic
  state with correct connectivity, not false live-working presentation (A3/A4).
- [ ] Visibility changes do not conceal status defects; a genuine working
  indicator still animates, and catalog freshness is independently tested
  through the other child (A1/A5).
- [ ] Focused deterministic regressions, exact-commit CI, and a native
  startup/quiet/exit/reconnect trace verify the implemented correction.
- [ ] Automated evidence and the binary used for any manual acceptance are
  produced by Actions for the exact product commit (A8).

## Dependencies and Out of Scope

Process/event tests are independent of the worktree child. Execute status
integration after that child's shared sidebar changes to avoid overlap.
This child owns semantic Agent presentation; the other owns catalog freshness.
Parent startup validation explicitly requires both results.

No replacement Agent runtime, remote Hook-secret forwarding, universal
heuristic timeout, global animation disabling, unrelated UI redesign, or
unbounded unread/provider-classifier rewrite.

## Risks and Deferred Validation

The design resolves accepted-event ordering and bounded lifecycle policy.
Keep existing two-empty confirmation, Hook priority, unsupported/failure
fallback, and heuristic-only behavior without prior attestation. Presence alone
cannot prove exact task completion, permission waiting, or continuous work.

Native flashing cadence and running-binary/source correspondence are validation
risks, not already-proven runtime reproduction. No app launch, authenticated
remote probe, or compiler/test gate ran during planning. All product decisions
are resolved. The user approved the final parent summary on 2026-09-05; start
this child according to the serialized integration order.
