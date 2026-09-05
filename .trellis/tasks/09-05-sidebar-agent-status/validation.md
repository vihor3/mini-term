# Remote Agent Status Validation Record

Recorded: 2026-09-05 (Asia/Shanghai)

## Implementation Handoff

The status implementer handed off changes in `mt-ssh/src/agent.rs`,
`mt-ai/src/agent_runtime.rs`, and `mt-app/src/{ai.rs,pane.rs,orca_sidebar.rs,
store/ai.rs,store/remote_agents.rs}`. Independent static review is complete.
Only source/Git inspection occurred. Compilation, tests, fixtures, formatting,
generation, and native verification have not run.

The visibility/settings changes remain separately owned in their first commit.
The status edit preserved the new sidebar header controls and visibility
predicate, and begins after the first visibility reviewer released that file.

## Source Regression Coverage

- `mt-ssh agent::tests`: actual generated `/proc` probe over explicitly owned
  Linux subprocess fixtures, exact route-field rejection, provider arguments,
  wildcard-literal safety, bounded readiness/output/cleanup. The readiness
  reader continues draining stdout through libtest shutdown after signaling
  readiness; early pipe closure would cause a false fixture failure.
- `mt-ai agent_runtime::tests`: retired-route ordering cannot be bypassed by a
  queued older weak event; a later launch retains a fresh run identity.
- `mt-app ai::tests`: observer teardown is idempotent and delayed events lose
  their route eligibility.
- `mt-app store::ai::route_tests`: delayed PTY Working after accepted Waiting
  has no projection; accepted state and evidence precede compatibility effects.
- `mt-app store::remote_agents::tests`: accepted inventory/Hook state,
  confirmed-retirement weak-latch clearing, new launch, and exited-poll fences.
- `mt-app orca_sidebar::status_tests`: live work versus steady waiting,
  attention, completion/error, disconnected/stale work, and rich-state priority
  over legacy Working fallback.

These are test-source references, not passing execution evidence. The existing
Actions workspace job includes the tests. The user approved the scoped
commit/push plan on 2026-09-05, and initial source commits are pushed. Both
locked workspaces and Windows MSVC/package workflows are running; see the
parent validation record. No successful result is implied before completion.

## Review Focus

Verify cause-based Hook authority, multiple same-route processes, mixed-pane
aggregation, ordinary shell/no-rich fallback, exact sequence/epoch ownership,
and natural-exit integration. Do not generalize the fix to a new remote Hook
protocol, universal heuristic timeout, provider classifier, or unread redesign.

## Native Acceptance Pending

The exact original startup cadence remains unreproduced. A controlled native
startup/quiet/exit/reconnect trace must identify the Actions-produced binary
and show semantic activity, connection freshness, route ownership, and catalog
progress separately. No still screenshot or compiler pass substitutes for that
evidence. Unsupported/multiplexer environments and never-attested heuristic
sessions retain their existing bounded-inference limitations.

## First Static Review

- Fixed route-wide maximum evidence suppressing independent accepted runs.
  Aggregation now retains each accepted run's reconciled state.
- Fixed rich evidence for one pane discarding valid fallback work/error on
  other panes. The production helper now replaces fallback per owning pane.
- Fixed ambiguous accepted provider `None` retaining a prior inferred provider
  through the legacy updater. Explicit session identity remains separate.
- Found provider-less Hook exit selecting another run's newest provider. The
  design/spec now require a unique exact Hook owner or side-effect-free
  rejection. The correction is complete: `observe_hook_exit` proves the exact
  owner under existing matching rules, preserves session/process identity,
  and returns `Ignored(UnresolvedHookOwner)` when it cannot do so. New tests
  cover different/same-provider peers, exact-session targeting, ambiguous or
  absent owners, ordering/epoch rejection, and no projection on rejection.
- Lifecycle integration coverage is not full GPUI event delivery: the current
  first test manually evicted poll state. The follow-up factors the exited-PTY
  fence and poll removal into `retire_terminal_polling`, used by `on_pty_exit`.
  The regression now calls that production helper and checks idempotence,
  unrelated-owner preservation, and rejection of late completion facts.
  Full Pane/AppStore event delivery and connectivity delivery remain pending;
  this source coverage is not a substitute for native integration evidence.
- The bounded follow-up static review is complete with no further confirmed
  code defect reported. Tests, probes, compilation, lint, formatting, and
  native checks remain unverified pending Actions and native acceptance.

## Bug Analysis

1. Root causes: shell-option assumptions and a generated-command coverage gap;
   cross-layer ordering allowed legacy effects before registry rejection;
   lifecycle teardown and presentation fallback did not consistently follow
   accepted exact-route evidence.
2. Earlier source tests could check valid framing/strings without exercising
   enumeration. A global animation toggle would hide the symptom without
   correcting event acceptance or process discovery, so it was not used.
3. Prevention: actual generated-command fixtures in Actions, accepted-state
   projection boundaries, exact-route retirement/teardown guards, and pure
   rich-state/connectivity presentation regressions. Test execution is pending.
4. Related scope: inventory and PTY paths share the same acceptance boundary;
   ordinary catalog refresh is separately corrected by the visibility child.
5. Knowledge captured in the mt-ssh inventory, mt-ai runtime, mt-app remote
   reconciliation, and navigation contracts. No template tree exists for these
   application-owned contracts; unrelated Trellis bootstrap files stay intact.
