# Independent Agent Producer Review

Status: SOURCE COMPLETE / RELEASED to main for Agent integration. The mandatory
same-ID rich-resume blocker and the first-explicit-start ordering follow-up are
fixed in source under main's explicit API authorization. The two earlier producer
corrections were independently reviewed and preserved. This is NOT a passing
Actions or native acceptance result.

No local builds, Cargo metadata, tests, fixtures, probes, lint, syntax/format/
whitespace checks, native execution, automation or Git writes. No child agents.
Git/Worktree/Files/Tasks/CI/staged source, specs and task metadata were not edited.
Main owns specs, Actions, staging and commits.

## Findings and Fixes

- P1 FIXED: a source SessionStart revived an ended provider sid, but ordinary
  runtime observe matched the archived run and rejected EndedRun. Bypassing that
  guard alone would leave subsequent events ambiguous. The authorized explicit
  lifecycle API now creates a new RunId after an accepted end, keeps the real
  provider session unchanged and retains the ended audit run. Follow-on events
  route by the original source lifecycle, not newest/live provider-session order.
- P1 FIXED: first recognition through a mid-session event originally made every
  later SessionStart return started=false. Active entries now retain
  explicit_start_seen: the first actual start emits Started with the SAME token
  and immutable receipt, even when last_session has not changed. Mid-session
  alone cannot reopen an ended registry run. No receipt is recaptured on promotion.
- P2 FIXED: repeated active SessionStart could reset Working to Waiting even
  without a new lifecycle. Repeated starts no longer write/emit a start status
  or renew Hook age. Identity/provider corrections can still emit Observed;
  neither the lifecycle token nor receipt is reminted.
- Earlier P1s source-confirmed fixed: delayed A statuses/end retain A's receipt
  and cannot clear/mark recognized or pending B; every known nonlast or /clear
  end emits the exact captured session identity. No provider-only rich-exit
  fallback swallows an independent Hook, and only the permitted last non-clear
  end removes pane Hook state.

## Final API

```text
AgentHookLifecycleId: opaque internal Copy/Eq/Ord/Hash identity
    minted only inside mt-ai; not serialized

HookSessionId::lifecycle_id: Option<AgentHookLifecycleId>
SessionIdentity::hook_lifecycle: Option<HookLifecycleEvent>  // serde(skip)
HookLifecycleEvent::{Started(AgentHookLifecycleId), Observed(AgentHookLifecycleId)}

AgentRuntimeRegistry::start_hook_lifecycle(
    AgentObservation, AgentHookLifecycleId,
) -> AgentApplyOutcome
AgentRuntimeRegistry::observe_hook_lifecycle(
    AgentObservation, AgentHookLifecycleId,
) -> AgentApplyOutcome
```

The existing StatusChange::hook_session is already serde-skipped and now retains
the optional lifecycle ID too. Generic/legacy literals use None. No external
payload, provider session, persisted layout or runtime-state field was invented.
No outside-owned-path constructor update was needed.

Private registry binding is `(exact AgentRoute, AgentHookLifecycleId) -> RunId`.
The new entry points require valid exact session identity, Hook/LiveConfirmed/
Live evidence and no supplied process identity. Initial recognition can bind an
already uniquely proved live provider-session owner and retain its process. An
explicit new lifecycle may allocate after all exact matching audit runs have
ended and ordering succeeds. A competing bound live lifecycle or multiple exact
candidates is rejection, not implicit retirement or a global live/newest choice.

Bound follow-on events, including provider corrections, address only their
original run. A bound ended run rejects all such observations, including a late
exit carrying a newer delivery epoch. An unbound ordinary lifecycle observation
keeps the existing ended/ambiguity guard and cannot create an exit. Ordinary
observe and legacy ownerless-exit matching retain their previous semantics.
Exact route removal purges only its lifecycle bindings alongside its runs.

Ownership, event replay, sequence, epoch and ended-run validation are read-only
before the shared accepted-observation mutation body. Rejections leave every
registry map/set, run, sibling connectivity, weak supersession, semantic/process
freshness and event receipt unchanged. Rejected events do not consume IDs or
install bindings. App Ignored handling still precedes legacy status, attention,
notifications, session persistence and pending-fork effects.

## Source Trace

- tracker.rs reviewed read-only in this pass. Capture, conditional mark and
  conditional clear share the outer ai_sessions lock with input, echo and purge;
  both published and pending episodes participate. No compare-then-plain-clear
  or tracker-to-Hook callback under that lock was introduced.
- hook_server.rs captures receipt and lifecycle once in bounded active entries.
  End uses that entry, never tracker state at delivery. First Started identity
  is emitted synchronously before its status. Provider correction, nonlast end,
  /clear and bounded tombstones remain source-owned.
- monitor.rs retains exact HookSessionId in status dedup, so a new lifecycle
  cannot be swallowed by the preceding same-sid status/cause. Poll/legacy events
  retain their None behavior and weak episode semantics; no Stall inference or
  polling-generated lifecycle authority was introduced.
- ai.rs ChannelSink still forwards source structs unchanged, with captured
  route/event ID/sequence. It does not reread HookState or the tracker to infer
  start authority. Actual ChannelSink tests assert Started arrives before status.
- store/ai.rs dispatches Started only to the explicit start API; Observed and
  exact statuses/ends use the ordinary lifecycle API. Invalid source providers
  fail closed on the new internal path; legacy None fallback remains intact.

## Authored Tests, All UNRUN

New production/helper regressions:

- `hook_lifecycle_resume_preserves_audit_and_ordinary_ended_guards`
- `hook_lifecycle_rejections_preserve_epochs_bindings_and_supersession`
- `hook_lifecycle_initial_binding_requires_unique_proof_and_purges_exact_route`
- `producer_first_explicit_start_keeps_mid_session_token_and_unknown_receipt`
- `producer_bridge_same_id_resume_creates_new_run_for_sole_and_nonlast_sessions`
- `producer_bridge_queued_old_lifecycle_events_cannot_mutate_a_resumed_run`
- `producer_bridge_rejected_start_does_not_bind_or_consume_its_event`
- `producer_bridge_first_explicit_start_after_mid_session_recognition_can_resume`
- `producer_bridge_resumed_hook_never_borrows_later_provider_input`

Expanded existing regressions:

- `producer_sole_exit_and_same_id_resume_keep_distinct_receipts`: original/new
  lifecycle IDs, repeated active starts and exact end identity.
- `queued_bridge_events_keep_the_source_episode_after_a_later_input`: captured
  Started/status lifecycle equality and unchanged external serialization.

The bridge cases call the production Hook handler, public tracker/perception
entry points, actual ChannelSink and production app ingestion helpers. They
cover sole/nonlast resume, Hook-only and detected input, immutable old audit
rows, Done remaining live, queued old identity/status/end, newer-epoch rejection,
recognized/pending B and first explicit start after bounded tombstone eviction.
The private-registry snapshot regression compares all maps/sets on rejection.
These are authored tests, not execution evidence or a GPUI/native reproduction.
Existing lower receipt/locking/nonlast/clear/monitor gates remain required too.

## Remaining Limits and Gates

No unresolved source/API blocker remains in this authorized producer/resume
slice. The incoming wire still has NO lifecycle generation: an old same-sid
SessionEnd received by the producer AFTER a real resumed start is indistinguishable
from an end of the new lifecycle. Internal IDs fence events already captured or
queued; they cannot recover generation absent from a newly arriving wire message.
No receipt time, provider, route reset, recency or fabricated session ID hides
this protocol limitation. A missing accepted old end also cannot justify
retiring a still-live bound run to make room for a new start.

Hook active/tombstone storage remains bounded; eviction does not manufacture
identity or restart authority. Weak observations without source supersession
proof remain Unknown, and rapid no-Hook exits/relaunches entirely between samples
remain the previously documented unknown, not output-derived activity.

Main must obtain exact-Agent-SHA Actions compilation/tests/lint/format and the
matching native artifact acceptance. The non-Agent Actions commit is not Agent
verification. No execution gate was performed or marked passed here.

## Actual Changed Files

This producer-review/resume pass changed only:

- `crates/mt-ai/src/agent_runtime.rs`: lifecycle ID, private bindings, two APIs,
  common accepted mutation body and focused regressions.
- `crates/mt-ai/src/lib.rs`: narrow lifecycle type exports.
- `crates/mt-ai/src/hook_server.rs`: retained source lifecycle/start-seen flag,
  first-start emission, duplicate-start handling and focused regressions.
- `crates/mt-ai/src/monitor.rs`: internal SessionIdentity discriminator and a
  legacy fixture None field; existing status identity/dedup retained.
- `crates/mt-app/src/ai.rs`: bridge source/serialization assertions and legacy
  fixture None field; production forwarding is unchanged.
- `crates/mt-app/src/store/ai.rs`: exact lifecycle ingestion, None fixture and
  production-backed regressions.
- `.trellis/tasks/09-06-native-agent-ownership-status/producer-review.md`.

tracker.rs and store/remote_agents.rs retain prior owners' changes but were not
edited by this pass. No broader Agent UI/navigation/history review or edits.
