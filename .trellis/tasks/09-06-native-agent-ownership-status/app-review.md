# Combined Agent App Review

Status: combined source review and approved conditional-clear follow-up complete.
No remaining implementation blocker identified in the assigned slice. Main's
independent lower follow-up and Actions/native acceptance remain pending.
The clarified later-episode policy required no runtime-policy rewrite.
NOT a passed quality gate.
No build, Cargo metadata, tests, fixtures, probes, lint, format, whitespace
checks, code generation, app launches, automation, staging, or commits run.
Main owns Actions, integration, specs, and matching-artifact acceptance.

## Follow-Up Handoff: Weak Alias API

NEW FOLLOW-UP API FOR MAIN / INDEPENDENT LOWER REVIEW:
`SessionTracker::clear_ai_session_if_episode(pty_id, expected) -> bool` is now
implemented in the narrowly released `mt-ai/src/tracker.rs`. It acquires the
existing `ai_sessions` outer mutation lock, checks exact published episode and
rejects any different pending episode, and performs cleanup while retaining
that lock. Input parsing/minting/publication, echo promotion and pane purge now
take the same lock before auxiliary tables. Plain clear and input-exit cleanup
share a private locked helper; there is no compare-then-call-plain-clear gap and
no reentrant session lock. `None` succeeds only without published/pending
episodes; a matched idempotent clear returns true. Published episode survives
successful clear; pending echo state does not. No Hook/perception/export changes.
The app retains `inventory.weak_episode` before consuming the inventory, and
calls this conditional API only after accepted retirement and the existing
no-surviving-owner/no-Hook guards. This follow-up edits only tracker.rs,
store/remote_agents.rs, the policy regression in store/ai.rs, and this report.
Main may start the independent lower review; all new tests remain UNRUN.
Lock audit: input holds the outer session guard before input-state and episode
tables; it releases the input-state guard before calling locked exit cleanup.
Echo checks its window and publishes under the same session guard. Mark, clear,
purge and the monitor snapshot also acquire session before auxiliary locks.
Public episode reads take only the episode table and never acquire session
afterward. Perception calls Hook interrupt handling only after tracker input
returns. No Hook code executes while the tracker mutation guard is held.

Update: main approved the episode contract and narrow branch-family caller scope.
The completed API at the top of lower-layer-review.md has been read; app field
forwarding and registry-owned supersession consumption are implemented.
Main also released `mt-app/src/ai.rs` only for source-event bridge/test fields.

The conditional-clear question is implemented in the newly approved narrow
tracker scope above. Both recognized input B and pending-before-echo B survive
completion of an older inventory captured at A; the retired process remains
retired and the later detection can publish its own Unknown fallback.

The different-provider question is RESOLVED BY MAIN'S CAUSALITY POLICY, not by
changing runtime matching: a genuine later episode B > A is not A's alias.
An older Hook capture cannot cover B merely by remaining live. Keep B as weak
Unknown alongside the proved/Hook run until covering accepted source evidence
or actual retirement. Same-episode fallback must be superseded; same-provider
ambiguity may still reject when no unique identity exists. There is no route-
wide hide-every-weak policy, provider/cwd inference or guessed process exit.

The ORIGINAL weak-alias API request is resolved: source-minted episodes,
`is_superseded_weak_alias`, the accepted supersession record and new-RunId
relaunch policy are present and consumed. The record's episode is the accepted
invalidating SOURCE capture; the retained audit run keeps its original episode.
Normal first-PTY/first-Hook ordering and strong-end persistence no longer rely
on an app suppression set, inferred exit or receipt timestamp.

The P2 caller request is also resolved in the explicitly approved scope:
`branch_family.rs` captures `SessionHistoryOwner` before its scan and retains it
through the deferred jump; Sessions retains the same owner with loaded/cached
rows and previews. No further history caller release is needed.

## Findings (Fixed)

- Files: `mt-ai/src/tracker.rs`, `mt-app/src/store/remote_agents.rs`.
  Issue: an old accepted empty inventory could erase a newer recognized or
  pending detection episode before its status event reached the app.
  Fix: atomic source-owned conditional clear with one outer session lock across
  validation and cleanup. Input detection, echo publication, ordinary clear and
  purge participate in that lock order. The app uses the scheduled inventory
  episode; accepted retirement/hysteresis/Hook guards are unchanged. Matching
  clears close the echo window without erasing the published episode. No new
  export, perception plumbing, Hook authority or runtime matching rule added.
  Tracker and app production-helper regressions cover recognized B, pending B,
  matching/idempotent/None clears and concurrent input/echo completion.
- File: `store/ai.rs`, policy regression only.
  Main clarified that later different-provider B is eligible weak Unknown, not
  a duplicate of A. The regression distinguishes a same-episode duplicate from
  genuine B, preserves B's exact state through a later receipt of older Hook
  capture A and A's actual Hook exit, then supersedes B only with covering
  accepted episode-B Hook evidence. No production route-wide suppression added.

- Files: `store/ai.rs`, `store/context.rs`, `store/remote_agents.rs`, narrow
  `ai.rs` bridge fixtures. Follow-up app consumption now forwards SOURCE
  `weak_episode` fields from status/session events and captures remote inventory
  episodes when scheduling. Delivery no longer re-reads the tracker provider.
  The shared live-run iterator, accepted legacy projection, provider fallback,
  display status, runtime title cardinality and exact target resolver exclude
  only registry-owned superseded aliases. Feed/sidebar/Runtime/Quick Open counts
  and activation inherit the exact target filter; close/provider consumers use
  the shared accepted projection. Audit rows remain intact; no synthetic exit,
  app suppression set, receipt heuristic or provider-based identity merge added.
  Later source episodes retain the lower new-RunId policy. Conditional tracker
  retirement and the different-provider policy question are resolved above.
- Files: `session_panel.rs`, approved `branch_family.rs` owner boundary,
  `store/context.rs` target epoch plus owned target fixtures. Follow-up P2
  history routing now returns NoTarget, ExactTarget, Ambiguous or SourceMismatch.
  Rows retain their scan owner across host/WSL loads and per-worktree caches;
  source includes backend, host, worktree, configured/canonical/root paths,
  project, optional WSL history source and SSH fingerprint/epoch. Badges and
  labels never select a first sorted match. Click/menu/resume and post-CWD
  continuation revalidate the captured owner; ambiguous/mismatched selections
  cannot create a terminal. Local no-target resume remains available on a
  matching source. Preview reloads also retain their original source. Family
  scans capture before I/O, reject nonlocal/mismatched paths, and carry the same
  owner through menu close/defer; family layout/menu/fork logic is unchanged.
  Agent targets now retain their accepted connection epoch so current history
  cannot bind to pre-reconnect runs merely sharing a route/session ID.

- File: `crates/mt-app/src/store/remote_agents.rs`, `take_title_after_sample`.
  Issue: a request already in flight at title capture could supply the second
  foreground sample, even when its remote snapshot preceded that capture.
  Fix: retain request scheduling time and require a request scheduled strictly
  after the original title timestamp. Same-owner earlier/same-millisecond
  samples retain the pending original; changed/absent owners, expiry, unsupported
  probes and rejected inventories discard it. No poll timestamp replaces the
  capture timestamp; no lower API changed.
- File: `crates/mt-app/src/store/ai.rs`, `activity_for_session_identity`.
  Issue: after cross-Hook matching became exact-only, session identity could
  copy an unbound process's semantic Working into a new Hook run. That promoted
  unrelated, expiring title evidence into permanent Hook authority.
  Fix: the unbound fallback retains only a unique already-Hook-owned state;
  exact provider-session matches remain eligible. An unbound process supplies
  Unknown, not borrowed task semantics, to a new Hook identity.
- File: `crates/mt-app/src/store/context.rs`, `RuntimeTitleOwner`.
  Issue: session-title requests captured route/provider/session but not the
  run's connection epoch, so a retained run could acquire a replacement source's
  metadata or accept a completion after its own epoch changed.
  Fix: capture epoch, require matching host/worktree/backend epoch, request new
  metadata only for live connectivity, and revalidate the complete owner on
  completion/display. Cached exact metadata remains presentation, not liveness.
- Files: `agent_activity.rs`, `orca_sidebar.rs`, `session_panel.rs`, narrow
  `main.rs` Agent row and `jump_palette.rs` Agent candidate regions.
  Issue: Sessions history and global feed text omitted semantic freshness. Quick
  Open's activity search term also omitted freshness, and its candidate subtitle
  had no activity qualifier (it did not previously show a Working/Live badge).
  Fix: one small `activity_label_with_freshness` helper supplies the last-known
  qualifier across existing sidebar/Runtime/history/feed text and Quick Open's
  Agent subtitle/search fields. Liveness, grouping, selection and navigation
  remain independent. Main explicitly released those two additional regions;
  peer module-registration lines are preserved.
- Files: `store/ai.rs`, `store/remote_agents.rs`, regression fixtures.
  Issue: three assertions still assumed old lower behavior: unbound weak-Hook
  acceptance, provider-only weak Hook exit acceptance, and partial application
  of a stale whole-route inventory.
  Fix: use an exact process in the accepted weak fixture, assert unbound
  rejection before projection, and require OutOfOrder with every run unchanged
  for stale inventory. Add independent same-provider liveness coverage.

## Findings (Not Fixed)

- No remaining implementation blocker identified in the approved source slice.
  Main requested an independent lower follow-up on the new tracker locking/API;
  that review and Actions/native acceptance are not claimed here.

- Residual lower detection precision: an exit/relaunch never recognized by
  input, Hook or an exact process sample remains unknown. Source episodes now
  handle recognized later input even if compatibility status is unchanged, but
  do not turn output/silence into activity or process proof. The previously
  reported test-only `HookState::status_age` cleanup is now done by its owner.

## Combined Source Review

- Re-read the completed lower review and current runtime/semantic/probe/login/
  monitor/tracker source, including final episode fields, supersession queries
  and `SupersededWeakEpisode`. Current `foreground`, Unknown inventory activity,
  `observe_semantic` and `activity_freshness` contracts remain in use. No
  lower files other than the newly authorized narrow tracker follow-up were
  edited. The actual production login fixture is now authored
  by its lower owner, superseding that earlier coverage gap; it is still UNRUN.
- Registry ordering uses accepted event/receipt identity in feed watermarks.
  Unchanged weak polls do not renew unread; identical connectivity is filtered
  before app event allocation. Runtime process-only state stays Unknown, while
  Done/Failed are task states and do not imply process exit. Existing accepted
  projection precedes pane/attention/Git-watcher/notification effects.
  Legacy events with no runtime route still retain their existing None-outcome
  projection. None-episode ambiguous Hook/weak matching keeps its existing
  AmbiguousRun result; captured superseded episodes reject as SupersededWeakEpisode.
- The pump and registered-route scheduler cover background projects. Launch
  ownership relies on route/incarnation plus managed-root ancestry/TTY evidence,
  not provider/cwd; unsupported probes and two-empty retirement remain separate.
  Exact target activation, flat terminal owners, close/fork pending fences,
  shared ContextPanel selection, and no-extra-hydration behavior were preserved.
- OSC capture uses actual output events and the existing VTE parser. Cold
  snapshot/config/resize titles do not enter semantic capture. Hosted replay is
  not a new remote title source: current semantic title attestation is SSH-only,
  and SSH terminals use the existing legacy transport. No claim is made that a
  future local semantic path may treat replay bytes as new observations.
- Automatic catalog scans remain quiet; manual queue/in-flight progress stays
  separate from catalog authority. Existing errors and last-known states remain.
  No native refresh cadence or UI acceptance was executed.

## Actual Review Edits

Source files changed by this reviewer, relative to Rawls's handoff:

- `crates/mt-app/src/agent_activity.rs`
- `crates/mt-app/src/orca_sidebar.rs`
- `crates/mt-app/src/session_panel.rs`
- `crates/mt-app/src/store/ai.rs`
- `crates/mt-app/src/store/context.rs`
- `crates/mt-app/src/store/remote_agents.rs`
- `crates/mt-app/src/main.rs` (Agent row label only)
- `crates/mt-app/src/jump_palette.rs` (Agent subtitle/search label only)
- `crates/mt-app/src/branch_family.rs` (approved history owner boundary)
- `crates/mt-app/src/ai.rs` (approved source-event bridge fixtures only)
- `crates/mt-ai/src/tracker.rs` (newly approved atomic conditional-clear API,
  participating lock boundaries and focused tests only)
- This explicitly requested `app-review.md` report.

Rawls's remaining pane/titlebar/catalog/store initialization changes were reviewed
but not rewritten here. No specs, workflow, task metadata, other lower-layer files,
configuration/layout/UI modules, staging or commits were changed by this reviewer.
The Git worktree cleanup module/registration/export, location reuse visibility
and TerminalCloseRequest accessor regions remain reserved to their owner.
This final bounded pass changed only `crates/mt-ai/src/tracker.rs`,
`crates/mt-app/src/store/remote_agents.rs`, `crates/mt-app/src/store/ai.rs`,
and this report. The complete list above includes earlier review fixes.

## Authored Tests, UNRUN

- `conditional_clear_matches_owned_episode_and_preserves_legacy_none`
- `conditional_clear_preserves_a_later_recognized_input_episode`
- `conditional_clear_preserves_pending_input_before_echo_publication`
- `conditional_clear_serializes_with_input_and_echo_promotion`
- `old_inventory_retirement_preserves_a_later_recognized_input`
- `old_inventory_retirement_preserves_pending_input_before_echo`
- `later_different_provider_episode_survives_older_hook_capture_and_retirement`

- `weak_fallback_projection_is_sticky_after_hook_end_but_not_a_later_launch`
- `weak_supersession_never_hides_proved_same_provider_runs_or_another_route`
- `terminal_title_cardinality_ignores_superseded_alias_and_drops_ended_hook_title`
- `queued_bridge_events_keep_the_source_episode_after_a_later_input`
- `history_distinguishes_no_target_exact_and_ambiguous_routes`
- `history_source_matrix_never_attaches_local_wsl_or_ssh_rows_to_each_other`
- `history_captured_owner_rejects_source_changes_before_resume_or_title_matching`

- `title_confirmation_requires_a_poll_scheduled_after_original_capture`
- `pending_title_is_discarded_on_changed_owner_or_expired_capture`
- `session_identity_does_not_promote_unbound_process_semantics_into_hook_authority`
- `session_identity_can_retain_a_unique_unbound_hook_activity`
- `unbound_hook_and_two_same_provider_processes_keep_independent_app_liveness`
- `title_metadata_cannot_bind_a_retained_run_to_a_replacement_epoch`
- `activity_labels_keep_semantic_freshness_separate_from_liveness`
- Updated existing exact weak-projection, unbound weak-exit, stale inventory,
  title-owner epoch, observer teardown and constructor regressions. Normal
  PTY-before-Hook and Hook-before-PTY now have app production-helper coverage,
  including Done remaining live, strong exit, rejected old polls, new RunId on
  a later source episode, independent proved runs, legacy cleanup and title
  cardinality. The two former open sequences now have focused regressions under
  the approved conditional-clear API and clarified causal episode policy.
  The existing confirmed-retirement fixture now captures the tracker episode
  when scheduling each represented inventory, matching production ownership.

## Spec Requests For Main

- Sync the app contracts for strictly post-capture confirmation polls,
  source/epoch-owned title metadata, and shared freshness-qualified text.
- Add the atomic conditional-clear signature/return semantics, pending-episode
  fence and outer session lock contract. Preserve the causal distinction between
  same-episode fallback and a genuinely later source episode beside older Hook
  evidence. Main reports the episode/title/history specs are otherwise prepared.
- Keep authored tests, source review, Actions evidence and matching native
  artifact acceptance distinct. Existing navigation CI is not Agent validation.

## Verification

- Lint: UNRUN, Actions-only.
- TypeCheck: UNRUN, Actions-only.
- Tests: UNRUN, Actions-only. Source review is not passing CI or native evidence.
- Main must run the exact integrated SHA through existing Actions Linux tests,
  check/Clippy/format/whitespace gates and Windows compilation/package gates,
  recording run URL/ID, headSha, job conclusions and artifact identity. Native
  acceptance must use the matching Actions-produced artifact.
