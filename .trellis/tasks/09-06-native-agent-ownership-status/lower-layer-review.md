# Agent Lower-Layer Review

## Conditional-Clear Independent Follow-Up

Current pass: SOURCE COMPLETE for the approved conditional-clear and Hook
producer/bridge follow-up; released to main's independent producer review.
Both reported producer P1s are fixed in source. This is NOT Agent acceptance:
the same-ID rich-resume API limitation below remains not fixed, and every build,
test, syntax/format/lint/probe/native gate is UNRUN here. Main owns Actions,
artifacts and spec updates. No staging, commits, push or child agents.
Runtime matching, semantic parsing, probes and other owners' slices stay closed.

P1 FIXED: the last-Hook `SessionEnd` producer used plain tracker clear and
then reread the current episode. Delayed A could therefore clear recognized or
pending different-provider B and emit A's exit with B's receipt. Repeated old
non-end events could also borrow B or block its echo with unconditional marking.
The registry-only later-provider test bypassed this producer.

Chosen internal API/ownership contract (recorded before producer edits):

- Extend the existing bounded active-session entries with an immutable optional
  `HookDetectionReceipt`, captured once at original non-end recognition. The
  receipt contains an optional published episode; absent receipt is UNKNOWN,
  distinct from a captured source with no episode. The receipt is crate-private;
  it introduces no wire field.
- Tracker capture and conditional marking use the existing outer `ai_sessions`
  lock, and reject conflicting published/pending input. A first explicit
  `SessionStart` can capture a compatible published weak launch only when there
  is no competing Hook lifecycle. Provider normalization is a mismatch guard,
  never a session/process identity link. An unseen mid-session event, conflicting
  provider, pending-before-echo input or competing lifecycle has no such proof:
  keep the receipt unknown, never borrow the newer launch. With no detection or
  pending input, a captured empty source can support Hook-only input tracking.
- Every subsequent event for that active sid reuses the original receipt,
  including repeated SessionStart and provider corrections. A conditional mark
  may heal input misdetection only while that exact source boundary still matches;
  it cannot steal a later published or pending episode. Unknown receipts cannot
  mark or clear unrelated input detection.
- End/tombstone only the supplied sid. EVERY known session end, including
  nonlast and `/clear`, emits its exact internal session identity and original
  receipt so the rich registry retires that run, not a provider-only fallback.
  Only a last, non-clear end removes pane Hook state. A non-clear end can
  conditionally clear its own captured source when no other active lifecycle
  holds that same receipt; unknown receipts cannot authorize cleanup. This also
  closes the old latch when its owning outer Hook ends before an unbound inner
  Hook. Unknown/duplicate/missing-sid ends cannot perform pane teardown
  or invent a rich exit. Never capture on SessionEnd. Explicit same-ID
  SessionStart after an actual end remains a new producer lifecycle.
- Minimal internal event API: add `StatusChange::hook_session:
  Option<HookSessionId>` with `#[serde(skip)]`. Hook producer statuses and ends
  carry the captured identity, and `weak_episode` remains the original receipt.
  The emitter includes exact session identity in Hook dedup; polling and legacy
  callers retain their existing API. The channel bridge forwards the field
  unchanged. App status ingestion passes its provider/session to the existing
  registry `observe`, with exact-known-owner preflight for exit; only legacy
  ownerless ends use `observe_hook_exit`. No runtime matching redesign.
- Extract a production `handle_hook_payload` function without altering HTTP/payload/
  port/route behavior. Tests call that handler with public tracker input for A,
  recognized/pending B, repeated old events/end, sole exit, clear, nonlast,
  unknown/late sid, provider mismatch and explicit same-ID resume. Its narrow
  callable boundary also permits cross-crate production -> ChannelSink ->
  ingestion-helper -> registry tests without a real server or fabricated ends.

App/API handoff: the internal-only optional status field above requires literal
constructor updates (None for legacy/monitor fixtures); external serialized
status fields and incoming Hook protocol are unchanged. Existing captured episode
forwarding and the accepted conditional-inventory consumer stay intact. A first
Hook whose lifecycle cannot cover current detection remains authoritative Hook
evidence but carries no weak receipt; it must not hide an independently eligible
weak run. There is no route-wide suppression or provider-only registry merge.

Main's additional P1 FIXED in this same pass: tombstone-only nonlast exit
left the inner rich run live, then made last exit ambiguous. Exact session status
and exit ingestion are now implemented, including both end orders with later
recognized/pending B. All new regressions remain UNRUN.

P1 NOT FIXED, residual API coordination for main: source `SessionStart` revives an ended
sid and captures a fresh lifecycle receipt, but the existing rich registry's
`observe` rejects a non-ended observation matching any ended provider/session
(`agent_runtime.rs`, ended-run guard). Exact status plumbing cannot legitimately
change that rule, fabricate a new provider session, or remove the whole route.
The producer same-ID resume regression therefore proves source tracking/emission,
not a new actionable rich run. A narrow explicit lifecycle-start decision is
needed if rich same-ID resume is an acceptance gate; no runtime rewrite is made
under this pass's approved scope. Incoming payload also cannot distinguish a
duplicate end from an older same-ID lifecycle after an explicit resume; receipt
time/provider guesses cannot supply that missing generation proof.

Source review result and focused gates:

- `tracker.rs`: `clear_ai_session_if_episode` keeps Bohr's accepted comparison
  semantics. Capture/conditional mark share that comparison under `ai_sessions`.
  Input, echo, ordinary clear, purge and snapshot participate in the same outer
  lock; no tracker method calls Hook handling while holding it. Active-session
  capture takes Hook storage then tracker; callbacks occur after both guards
  are released. No inverse lock path was found in source.
- `store/remote_agents.rs`: accepted retirement still passes the scheduling-time
  receipt, after retirement and existing Hook/live-owner guards. Added the missing
  populated-latch rejected-inventory/Hook-enabled regression; no production
  retirement or unrelated cleanup change was needed in this pass.
- UNRUN tracker gates: `episode_mutations_and_snapshot_share_the_outer_session_lock`
  (input, echo, conditional/plain clear, purge, mark, input exit, snapshot,
  capture and conditional mark),
  `hook_receipt_capture_and_mark_refuse_unowned_or_later_detection`, plus Bohr's
  matching/None, recognized/pending and concurrent conditional-clear cases.
- UNRUN producer gates: `producer_old_hook_status_and_exit_preserve_recognized_or_pending_input`,
  `producer_sole_exit_and_same_id_resume_keep_distinct_receipts`,
  `producer_nonlast_and_clear_end_emit_exact_exits_without_pane_teardown`,
  `producer_unknown_missing_and_mismatched_first_session_never_borrow_input`,
  `producer_unseen_mid_session_event_cannot_claim_a_same_provider_launch`,
  `producer_unknown_ends_cannot_tombstone_or_clear_the_current_session`,
  `producer_outer_end_clears_only_its_unshared_receipt`, serialization and
  bounded active/tombstone storage tests.
- UNRUN real bridge/ingestion gates:
  `queued_bridge_events_keep_the_source_episode_after_a_later_input` now invokes
  the actual producer; `producer_bridge_exact_nonlast_and_last_hook_exits_preserve_later_input`
  covers both end orders with recognized/pending B;
  `producer_bridge_all_hook_exits_allow_new_input_without_old_alias_resurrection`
  covers both end orders, repeated idle, later public input and queued old episode;
  `exact_hook_exit_requires_existing_unique_session_before_side_effects` covers
  unknown identity rejection before epoch changes. Cross-crate tests use public
  tracker/perception input, real handler, ChannelSink and production ingestion
  helpers, not fabricated end observations or mt-ai-only test helpers.

Exact files changed by this follow-up (peer changes preserved):

- `crates/mt-ai/src/tracker.rs`: narrow receipt helpers/shared comparison and tests.
- `crates/mt-ai/src/hook_server.rs`: bounded lifecycle receipts, real payload
  handler, conditional marking/cleanup, exact status/end production and tests.
- `crates/mt-ai/src/monitor.rs`: serde-skipped internal identity field and
  session-aware dedup only; no polling/semantic policy changes in this pass.
- `crates/mt-app/src/ai.rs`: existing bridge forwards the enriched struct;
  constructor update, production-source regression and cfg(test) channel fixture.
- `crates/mt-app/src/store/ai.rs`: exact status ingestion/exit preflight, extracted
  unchanged identity ingestion for production-backed tests, constructors/tests.
- `crates/mt-app/src/store/remote_agents.rs`: populated-latch guard regression only.
- `.trellis/tasks/09-06-native-agent-ownership-status/lower-layer-review.md`.

Spec requests for main, not edits here: document immutable per-Hook lifecycle
receipts versus unknown ownership, conditional marking as well as clearing,
unshared-receipt cleanup on nonlast end, exact internal status/end identity and
dedup, and source-backed bridge regressions. Decide the explicit rich same-ID
lifecycle-start contract without weakening ordinary ended-run rejection. First
unseen/mismatched/competing Hook evidence cannot silently consume a newer weak
episode; unknown ownership remains a visible precision limitation. Existing
provider corrections and bounded-storage eviction do not create identity proof.

## Earlier Lower-Layer Closure

Historical status before the follow-up above: lower source fixes and regressions
were complete, including combined lifecycle
P1 episode/supersession follow-up; app integration and Actions acceptance remain
required. NOT a passed quality gate.
All builds, metadata, lint/format, tests, Python checks, probes, fixtures and
native automation remain UNRUN here. Main owns Actions, artifacts and specs.
That earlier pass made no app edits; the newly approved narrow app edits are
listed above. No spec, workflow, staging, commit or push changes here.
Follow-up authorization includes monitor cleanup, its directly affected
perception test, a dedicated mt-pty login fixture, and the approved Hook-server
test-only accessor/comment cleanup. Main subsequently approved the narrow
tracker/monitor/Hook source-episode plumbing for the combined lifecycle blocker.
Both formerly deferred findings and the combined lower P1 are implemented.
Bohr/main: the former P1 matching finding is no longer unfixed. The remaining
alias behavior below is explicit conservative policy, not UI deduplication.
Earlier source closure is superseded by the current residual API note above.
Main confirms Bohr has
forwarded captured event/scheduled-request episodes and integrated alias queries;
that app work is owned by Bohr, not written here. Main has updated the successor
specs from source. The approved lower slice is ready for main's Actions handoff;
no local validation, staging or commits were performed by this reviewer.

## Immediate API Contract For Bohr

- REQUIRED FIELD REFRESH, source implementation complete:
  exported opaque `AgentWeakEpisode` (Copy/Eq/Ord/Hash, source-minted, not a
  receipt timestamp). `SessionTracker::weak_detection_episode(pty_id)` returns
  the latest detected episode, retained across clear and removed on pane purge.
  Real input detection creates a new episode; Hook marking/polls/output recency
  do not. Echo detection may only reuse an episode captured at its real input.
- Add `weak_episode: Option<AgentWeakEpisode>` to `StatusChange`,
  `SessionIdentity`, `AgentObservation`, `AgentRuntimeState`, and
  `AgentProcessInventoryObservation`. App must forward the SOURCE field for
  status/session events; do not re-read the tracker on channel delivery.
  Remote inventory callers must capture it at request scheduling and preserve
  it through completion; generic/history fixtures may use None.
- Add `is_superseded_weak_alias(&AgentRunId) -> bool` beside the detailed query
  below. `AgentFallbackSupersession` also records `weak_episode` for provenance.
  Newer receipt/event sequence/epoch cannot revive a superseded episode. A
  genuinely new source-owned episode can create a new fallback, never reactivate
  the old ID. Legacy None-episode input cannot relaunch a superseded fallback.
- Implemented combined lifecycle fix in the lower runtime:
  `AgentRuntimeRegistry::fallback_supersession(&AgentRunId)` returns
  `Option<&AgentFallbackSupersession>`. The exported record has `event_id`,
  `evidence`, `sequence`, `connection_epoch`, and `weak_episode` for the accepted
  invalidating exact-route source observation. It is NOT an identity link or
  process exit. The record episode is the SOURCE capture; the audit run retains
  its own original episode.
- Only non-ended, LiveConfirmed PtyActivity with no process AND no provider
  session can be superseded. Accepted live Hook/process source capture covers
  only the same or older input episodes on the complete same route. A later
  real-input episode also invalidates older unbound detection episodes, without
  claiming any process exit. Provider/cwd/receipt time do not select an owner.
  Singleton PTY/process identity upgrades remain intact when both preflight
  candidate sets are unique and captured episode metadata is compatible.
- `runs()`, `runs_for_worktree()` and `run()` retain audit records. App consumers
  must exclude runs with `fallback_supersession(...).is_some()` from liveness,
  pane/provider projection, close warnings, Agent targets/feed/Runtime rows,
  title-owner cardinality and weak-owner fallback selection. The runtime's
  `active_run_for_route()` and matching will exclude them directly.
- Supersession is sticky until exact-route removal, including after Hook exit,
  disconnect and empty inventories. The old weak run is never reactivated. A
  genuinely later source-owned input episode after observed lifecycle may create
  a NEW weak run ID; queued older episodes may not. Event sequence alone is NOT
  launch proof. Main explicitly approved this narrow tracker/event API expansion.
- Superseded/older/closed weak episodes return
  `Ignored(AgentObservationIgnored::SupersededWeakEpisode)` before epoch or state
  side effects, even when delivered with a newer sequence/receipt/epoch.
  `StatusEmitter::emit_if_changed` stays compatible; production paths use
  `emit_if_changed_with_episode`, deduplicating unchanged polls but emitting a
  newly captured input episode even when the compatibility status is unchanged.
- The earlier row-only workaround is replaced by this explicit evidence
  contract. Authored production regression exercises public tracker input,
  polling/emitter dedup, Hook identity/status/exit, repeated idle and later input.

## Findings (Fixed)

- P1, `crates/mt-ai/src/agent_runtime.rs`, `validate_event` / `observe`:
  an otherwise rejected ambiguous or ended-run observation with a newer epoch
  advanced the route epoch and disconnected unrelated runs before rejection.
  Validation is now read-only; `accept_epoch` runs after ownership/order checks.
  Regressions snapshot all runs, epochs and owner timestamps and prove a rejected
  newer epoch does not block a subsequent valid event on the old epoch.
- P1, same file, `apply_observation`: unchanged Process/PTY liveness updated
  `last_event_id` and `received_at_unix_ms`, repeatedly renewing Waiting/Done
  unread state and recency. It now advances `last_sequence` and remembers the
  input event for replay rejection, but preserves presentation identity/time
  unless accepted provider/session/process/activity/connectivity/confirmation/
  evidence/epoch facts change. Hook and owned semantic events remain observable.
  This addresses Rawls's request in `app-handoff.md`; no extra app watermark is
  needed. Tests assert retained Working/Waiting/Blocked/Done/Failed semantics,
  original event identity/time, freshness expiry, and changed-fact publication.
- P1, same file, `process_owner_changed`: an accepted weak PTY event carrying a
  newer epoch but no PID bypassed the process-owner timestamp reset. Existing
  attested processes now advance that fence on any accepted epoch change;
  old title captures cannot regain Fresh status through this path. Semantic
  capture epoch is retained privately beside original time, so even an epoch
  change within the same millisecond cannot keep an old title Fresh.
- P1, `crates/mt-ssh/src/agent.rs`, `read_nul_file` / `classify`:
  `head | tr; printf` masked failed/partial reads and accepted unterminated
  argv/env captures; empty argv could then become supported-empty. The bounded
  capture now carries read status through the pipe, checks the size and final
  NUL, and rejects incomplete captures. Executable/argv samples are compared
  before classification, all buffered process identities are rechecked, and a
  dead managed root cannot publish a successful frame. No raw data is emitted.
- P2, same file, `next_arg`: whitespace splitting discarded empty argv fields,
  letting `codex -c "" --help` consume `--help` as an option value. Argument
  iteration now preserves empty NUL-delimited values and keeps globs literal.
- P2, `crates/mt-ssh/src/agent_probe_tests.py` and Rust probe tests: old mismatched
  requests only proved missing-root behavior, not candidate filtering under a
  valid root. Added all seven candidate-route mismatches beside a valid child,
  bounded argv/env overflow, partial/failed/unterminated reads, empty-argument
  helper exclusion, executable/start-tick/foreground races, and owned exit
  during capture. Tool fault injection targets only explicitly spawned fixture
  PIDs; start-tick and foreground mutations are simulated captures, not claims
  of forced kernel PID reuse. Fixtures still execute the actual generated POSIX
  probe on a disposable PTY. Readiness records are size-bounded; outer cleanup
  now has a kill fallback and cannot skip the root if decoy cleanup fails.
  Parser regressions additionally cover numeric overflow, zero start ticks,
  duplicate footers, CR/NUL and missing final newline.
- P1 follow-up, `agent_runtime.rs`, `find_run` / `apply_process_inventory`:
  removed provider-only cross-evidence Hook correlation. Exact process identity
  or an already bound, valid provider/session identity is required. An explicitly
  session-proved process can fill a missing Hook process identity without
  replacing Hook semantics or weakening evidence. Inventory matches are computed
  before mutations; provider candidate counts and pre-existing process facts
  prevent arbitrary singleton upgrades. A weak PTY alias upgrades only when
  both sides are uniquely one. Proven independent processes remain separate.
  Public-input regressions cover Hook-without-PID with and without a session,
  both process orders, repeated reversed inventories, competing weak aliases,
  singleton upgrades, direct observations and exact cross-evidence proofs.
  Provider-less Hook exit uses the same Hook-only fallback family; multiple
  Hook owners remain rejected before effects.
- P2 follow-up, `mt-pty/src/ssh.rs` and `ssh_login_tests.py`: added a Linux
  Actions-only test of the actual `build_remote_login_command_with_env` result.
  It executes under a disposable controlling PTY/session, verifies all seven
  route fields and a hostile cwd/shell path, rejects inherited wrong root facts,
  and checks PID/start ticks/TTY/session/foreground preservation across exec.
  Only the explicit child and `/proc/self` are inspected. Readiness/output and
  cleanup have bounds; no SSH server, credentials, product API or dependency is
  introduced. Execution remains UNRUN.
- P1 follow-up, `mt-ai/src/monitor.rs`: removed `stall_settle_target`,
  `settle_stalled_ai`, Stall/StallExit generation and the silence-driven Hook
  writeback. The production `poll_panes` helper is read-only with respect to
  Hook/tracker authority. Unhooked input detection still emits compatibility
  liveness (`ai-idle` plus provider), but output no longer invents Working or
  Waiting transitions; the rich runtime normalizes this weak signal to Unknown.
  Poll cadence remains 500ms for active panes and 2s for no panes. A production-
  helper regression crosses the former ten-second window in Actions, verifies
  no Hook state/time/cause mutation after silence or weak exit input, and keeps
  genuine Stop and SessionEnd observable. Other regressions retain no-Hook
  input/provider liveness, deduplication, attention and explicit teardown. The
  directly affected perception output test and lib.rs successor documentation
  were updated. The subsequent approved episode plumbing below is independent
  of provider semantics and does not restore silence-driven Hook writes.
- P2 follow-up, `mt-ai/src/hook_server.rs`, `status_age`: main approved the
  remaining mechanical cleanup. The accessor and Duration import are test-only;
  obsolete stall-consumer comments were removed. Hook activity mapping is
  unchanged; the later source-episode field addition is described above.
- P1 combined follow-up, `agent_runtime.rs`, `tracker.rs`, `monitor.rs`,
  `hook_server.rs`: first local weak input followed by Hook SessionIdentity,
  Working and SessionEnd left a phantom live weak run because subsequent idle
  was correctly deduplicated. Source-owned detection episodes and durable
  supersession now make that fallback audit-only, without identity merge or
  invented process exit. New real input gets a new eligible run ID; queued old
  episodes cannot reappear after Hook exit, empty inventory or reconnect.
  Hook/process and session/PID-bound weak runs are never superseded. Inventory
  matching and all upgrades finish before leftover fallback invalidation, so
  no preflight upgrade can become a hidden proved process due to iteration order.
  Tracker clear atomically closes the pending echo window under the session
  lock; output can reuse a captured Enter episode, never mint one from recency.
  Poll episode metadata alone does not renew task-event identity/freshness.
- P2 mechanical, `perception.rs`: the adjusted output regression needed its
  `Duration` import; added during source review. No local compiler was run.

## Findings (Not Fixed)

- App integration/acceptance remains Bohr/main-owned: every count, target/feed,
  Runtime row, title-owner cardinality, provider choice, close warning and
  accepted legacy projection must consume the same supersession query. Forward
  captured episodes, not delivery-time tracker values. No app writes here.
- Residual detection precision: a lifecycle/launch not actually detected by the
  input path and not reported by Hook/process evidence remains unknown. Newly
  recognized input episodes are now distinguishable even when status is the
  same between polls; this does not make weak command detection exact process
  identity or solve unobserved/ambiguous launches. No output-based Working or
  invented Codex semantic precision was added.
- Multiple candidates still require whole-inventory preflight. Direct observe
  only knows current facts and may perform an allowed singleton upgrade before
  a later independent process arrives. Use `apply_process_inventory` for one
  simultaneous capture. No proved process is dropped to force a row count.

## Rawls API Refresh

- Refresh the episode fields and supersession queries listed above. Keep using
  `RemoteAgentProcess.foreground`, `AgentProcessObservation { activity: Unknown,
  ... }`, `observe_semantic`, and `activity_freshness` from the prior handoff.
- `Applied` does not necessarily mean a new presentation event. Unchanged weak
  liveness can advance ordering while retaining the run's event ID and receipt.
  Render/acknowledge the accepted state, never the inventory event ID or raw hint.
- Continue submitting original title capture time with exact foreground/session
  ownership and current route/epoch. Do not re-stamp retained titles on polls.
  The 15-second semantic timer is separate from liveness; Unknown/Done do not
  imply process exit. Local input detection still uses the existing public
  `track_input_with_line_snapshot` API.
- Hook matching no longer upgrades a process/PTY run by provider alone. Refresh
  tests that assumed an unbound Hook could overwrite weak state: supply an exact
  proved process or already-bound session, or expect rejection/separate identity.
  Process inventories carry no session ID, so cannot guess a Hook session.
- `Stall` and `StallExit` are no longer produced by the monitor. Keep Rawls's
  defensive cause demotion for old/queued inputs. Compatibility liveness remains
  non-semantic; do not turn `ai-idle` with no genuine Hook cause into rich Waiting.
  No app changes were made by reviewer.

## First Hook Ordering For Bohr

- Normal local input detection has provider and route, but no process/session
  identity. Weak PTY first then first Hook with session ID/no PID and its captured
  episode yields TWO audit runs but ONE eligible Agent: unchanged weak Unknown
  plus explicit supersession, and a new Hook session run. SessionIdentity starts
  Unknown; the subsequent genuine Hook status supplies Working.
- Hook with session ID/no PID first then unbound weak PTY yields ONE run: the
  unchanged Hook run. The captured old episode returns
  `Ignored(SupersededWeakEpisode)`, including if it arrives after Hook exit;
  it does not create an alias or app side effects.
- Public-input regression `first_session_hook_and_local_input_preserve_order_specific_aliases`
  uses the real tracker input API and asserts both outcomes. It also proves a
  later exact Hook exit retires only the Hook run; the pre-existing weak audit
  record stays Unknown AND superseded, so it cannot contribute liveness. The
  full `production_input_hook_exit_dedup_and_later_episode_do_not_resurrect_fallback`
  regression verifies the actual seq1/2/3/4 emitter path, repeated deduped idle,
  new input episode, and late old episode with newer sequence/epoch.
- One unbound PTY episode plus two proved processes remains three audit IDs but
  two eligible Agents after a covering source capture. Two session-bound weak
  aliases plus one process remain three eligible identities. One unbound Hook
  plus two proved processes remains three eligible identities. Exact already-
  bound session/PID upgrades retain one ID and are never hidden.
- Safe policy is source-owned evidence invalidation, not retrospective identity
  merge, process exit or UI-only suppression. Markers persist across stronger
  exit/disconnect and clear only with exact-route removal. No provider/cwd/
  receipt guessing or hiding proved processes is permitted.

## Spec Requests For Main

- Sync the mt-ai successor contract with pure rejection/no epoch side effects,
  ordering-versus-presentation event identity on liveness-only updates, and weak
  epoch changes invalidating process-owned semantic freshness.
- Document the approved exact cross-evidence Hook rule, pre-batch matching,
  two-sided singleton PTY upgrade, and retained ambiguous weak aliases. Keep
  direct-observation scope distinct from whole-inventory candidate knowledge.
- Add the approved real-input episode/source-capture contract, sticky accepted
  fallback supersession and per-route episode fences, including unseen queued
  older episodes and later real launches. New real input can invalidate only
  prior unbound detection evidence, never a proved/bound run. A stale inventory
  capture cannot borrow a later input episode from its completion-time tracker.
- The 512-byte/control-free session check currently applies to semantic owner
  input; ordinary `observe` only drops blank session IDs. Avoid a blanket session
  validation claim unless that ordinary boundary is separately hardened.
- Extend the mt-ssh contract with complete NUL framing/read-status preservation,
  exact empty argv fields and buffered identity rechecks. Keep v1 rejected and
  Unsupported distinct from confirmed process absence.
- Native title decoding is intentionally narrow. Local Orca references checked:
  `src/shared/pi-state-title-marker.ts`, `terminal-title-agent-type.ts`, and
  `agent-title-status.ts`. No transcript-word inference or claim of complete
  Codex task-state knowledge was added.
- Remove competing monitor stall/output-recency rules from successor guidance.
  Silence does not mutate Hook state, and weak input liveness is not a task state.
  Consumer review covered `AiPerception::{status_of,start_monitor}`, the app
  bridge/sink, accepted status ingestion, and defensive notification/cause rules.
  The approved adjacent Hook-server test-only accessor/doc cleanup is complete.

## Verification And Remaining Gates

- Lint: UNRUN, Actions-only.
- TypeCheck: UNRUN, Actions-only.
- Tests/fixtures/Python syntax/format/whitespace: UNRUN, Actions-only.
- Main must run the integrated exact SHA through existing Linux workspace and
  sidecar tests/check/Clippy/format gates and Windows compilation/package gates;
  record run URL/ID, headSha, job conclusions and artifact identity. Apply
  Actions-generated formatting diagnostics as needed; no local formatter ran.
- Additional fixture coverage still desirable: stat/process/depth caps and real
  native-provider installations. The actual production login fixture is now
  authored, not executed. Root TTY/
  session, copied-route same-TTY/same-cwd sibling exclusion, known launcher/native
  pairs versus independent descendants, foreground/background, helper/wildcard,
  legacy/old-root/missing-tool, parser caps and two-empty retirement paths were
  source-reviewed; authored assertions are not passing execution evidence.
- Final focused source pass confirmed that the mt-pty fixture executes the
  production login builder output with a controlling disposable PTY, exact
  owned PID inspection, bounded frame waits/cleanup and Actions guards. The
  episode production-helper regression, both delivery orderings, later launch,
  unseen/late old episodes, bound-run exclusions and unchanged semantic event
  identity were also source-reviewed. All execution remains UNRUN.
- Native visual cadence, background-project monitoring, title-event ownership,
  stale animation and duplicate-label behavior require Rawls/main acceptance
  using the exact Actions-produced artifact. Legacy roots remain Unsupported
  until actual relaunch; custom or unsupported native semantics remain Unknown.
