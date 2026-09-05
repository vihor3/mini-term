# Remote Agent Status Design

## Scope and Evidence

Fix exact-route remote process recognition, accepted-event projection, and
startup/exit/reconnect status stability. The research distinguishes confirmed
source defects from the still-unreproduced native startup symptom. Its
globbing defect and legacy-before-registry ordering are in scope. Routine
catalog-refresh warning churn is owned by the worktree child.

Reuse the current registry and evidence order: Hook > ProcessAttested > PTY >
history. Preserve all route, sequence, generation, incarnation, and SSH epoch
fences. Do not add remote Hook-secret forwarding, a new provider protocol,
network service, dependency, or replacement status engine.

## Process Probe

`build_probe_command` currently disables globbing before enumerating
`/proc/[0-9]*`. Allow expansion only for this fixed trusted enumeration and
retain glob suppression before splitting remote argv/stat text. Preserve
exact environment-route checks, provider normalization, PID/start ticks,
bounded framing, authenticated session ownership, and error classification.

Add a Linux test executing the actual generated command against a controlled
fixture process with exact public route values and deterministic readiness/
shutdown. Assert positive discovery and route-field mismatches, not merely
command-string presence. Never use or kill a user's Agent as the fixture.
Exercise wildcard-looking arguments so the fix cannot regress shell safety.

## Accept Before Projecting

For routed Agent observations, resolve ownership/provider and apply the
registry event before changing pane status, project aggregate, attention,
Git-watcher Agent flags, or completion notifications. Propagate the existing
`AgentApplyOutcome` instead of discarding it:

- Applied: derive UI state from the accepted registry state, including stronger
  evidence that may have overridden the weak incoming observation.
- Ignored: do not execute downstream legacy/attention/completion side effects.
- Not an Agent observation: preserve ordinary shell/terminal and legacy
  no-route behavior. Missing provider is not permission to discard all shell
  status changes.

For process inventory, project current accepted route state rather than the
raw request's recent-output classification. `apply_process_inventory` can
skip individual observations even when the overall result is successful.
Do not choose an arbitrary process as a stronger semantic owner of the pane.
Test queued Working sequence 10 after accepted Waiting sequence 11, epoch
changes, Hook authority, and multiple exact-route processes.

Static review additionally found that a provider-less Hook exit selected the
newest route provider, which can belong to an independent process-attested
run. Correct this at the existing acceptance boundary: resolve an unambiguous
current Hook-owned run on the exact route, carry its available session/process
identity into acceptance, and reject unresolved or ambiguous ownership without
legacy side effects. Do not use receipt recency or a different process's
provider. Preserve the registry's generic matching behavior; if its fallback
could select another same-provider run, require a provably exact target or
reject the observation. This is an ownership correction, not a new Hook
protocol. Ordinary shell/no-route fallback remains separate.

## Liveness and Negative Evidence

Keep the existing two-successful-empty confirmation after prior attested
process evidence, including poll-map recreation. A single empty, unsupported
probe, timeout, reconnect, or inaccessible environment is not a semantic exit.
Keep diagnostics/connectivity separate from process activity.

After accepted confirmed absence retires the last attested Agent on the exact
route, clear only the corresponding weak tracker latch when no stronger live
Hook/run evidence remains. Reuse `SessionTracker::clear_ai_session`; do not
unregister the terminal itself. Test that later ordinary shell output cannot
resurrect a retired Agent, and a subsequent genuine launch can create a fresh
run. Preserve Hook-owned approval/completion and other live same-pane runs.

Heuristic-only sessions that have never been process-attested keep the existing
fallback policy; no new universal timeout or negative-evidence grace period is
introduced. Linux presence plus PTY activity cannot determine exact provider
task completion or distinguish all quiet reasoning from an idle prompt. These
remain bounded inference, not a promise of Hook-level remote semantics.

On natural PTY exit, retire that incarnation's local observation sources and
poll eligibility, reject its queued events/completions, and clear local weak
tracking. Preserve the remote run's last semantic activity with disconnected/
stale connectivity: terminal transport exit is not proof the remote process
finished. Make cleanup idempotent with explicit close/detach. Do not change
warm-attach preservation, credentials, or remote process lifecycle.

## Sidebar Presentation

Derive a small pure presentation from exact-run activity, connectivity, and
attention, aggregated only over the configured worktree's current targets.
Use existing rich runtime views and status/icon helpers, not branch-name or
directory guessing. Keep legacy four-state compatibility mapping unchanged
for unrelated consumers.

- Live Starting/Working may animate when no higher-priority attention/error
  presentation applies.
- Waiting, completion, approval/Blocked, and error have distinct steady
  presentations; preserve meaningful attention and existing acknowledgements.
- Stale/disconnected Working retains its last semantic activity but does not
  present a live-working spinner. Show connectivity separately.
- No live Agent evidence yields ordinary Idle, not a Working state inferred
  from SSH connectivity, inventory polling, or restored session metadata.
- Local/no-rich-route fallback keeps its existing status behavior. Catalog
  warning/progress belongs to the worktree child and must not feed activity.

Use stable indicator dimensions. Do not disable `StatusKind::spins` or global
motion to conceal a semantic error. Broader unread-event deduplication and
provider-command classifier hardening are deferred unless a scoped regression
demonstrates they prevent these acceptance criteria; return to planning for a
materially broader correction.

## Files and Dependencies

Primary boundaries: `mt-ssh/src/agent.rs`, `mt-app/src/store/ai.rs`,
`store/remote_agents.rs`, `ai.rs`, and natural-exit handling in `pane.rs` or its
AppStore owner. `mt-ai` changes are limited to necessary accepted-state/lifecycle
helpers with focused tests; preserve existing empty-input, redraw cooldown,
Hook stall, and exact-connectivity no-op guards.

Coordinate only the status-lane portion of `orca_sidebar.rs` with the visibility
child. Logical process/event tests do not depend on visibility; execution is
serialized after that child to avoid shared-file conflicts. Integrated startup
acceptance requires both results, not just a passing process parser.

## Validation, Risk, and Rollback

Use deterministic fixture processes, explicit sequence/completion ordering,
and controlled timestamps. Cover no-input startup, live/quiet transitions,
confirmed disappearance followed by shell output, natural terminal exit,
reconnects, rejected late events, multiple processes, and Hook authority.

All CI, compilation, tests and fixture/probe execution, lint/format/whitespace,
generation, packaging, and automated verification run only in GitHub Actions
as a hard user constraint. No local/SSH/container test substitute is permitted.
Any manual native acceptance uses an Actions-produced artifact and records the
tested binary/source revision and bounded non-secret diagnostics: catalog
freshness, route/epoch, probe capability/count, event ordering, activity,
connectivity, and legacy status. A still screenshot is not proof of cadence;
record a short startup/quiet/exit/reconnect trace or video for final acceptance.

Exact startup behavior remains a runtime validation risk, not a planning
blocker or already-proven reproduction. Unsupported/multiplexer environments
retain fallback and are not promised exact authenticated process discovery.
Preserve the existing `MINI_TERM_REMOTE_AGENT_STATUS=0` rollback, local Hook/PTY
behavior, and both workspace lockfiles. Never roll back by deleting user
runtime records or terminating remote Agents.
