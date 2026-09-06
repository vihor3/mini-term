# Tasks Accounts Independent Review

Date: 2026-09-06. Assigned child: `09-06-native-tasks-gh-accounts`; the active
pointer still identifies the Git child. Source review and explicitly authorized
Actions log inspection only. The current production containment, eight new unit
tests and eight appended actual WSL rows are UNRUN; source review is complete.
Main reports the predecessor's eighteen diagnostic and two Git safety tests
passed on Windows. That evidence does not validate this production candidate.
No local build, test, lint, formatting, syntax, whitespace, transport or native
acceptance is claimed. Timing evidence supports the retirement boundary only;
kernel-member identity and broader WSL behavior remain unproven.

## Final WSL Containment Check: Source Released To Main

Reviewed Newton's released wsl-containment-implementation.md and production
slice, then Tesla's wsl-containment-fixtures.md and finalized appended fixture
module. Reused the completed production/Git reviews rather than restarting the
earlier diagnostics. No remaining concrete finding or source/API blocker was
identified; this final containment check needed no source fix. Only this report
was edited by the checker. The exact check-owned paths are now released and
frozen for Main:

- `crates/mt-app/src/execution_host.rs`
- `crates/mt-app/src/tasks_account_executor/process.rs`
- `crates/mt-app/src/tasks_account_executor/tests.rs`
- `.trellis/tasks/09-06-native-tasks-gh-accounts/review.md`

Main owns specs, progress, formatter artifacts, Git, CI and the exact-SHA gate.
No other source/config/dependency files were changed. No local automated checks,
fixtures, probes, app launches, Git commands, upstream/CI fetches or children.

### Combined Source Conclusions

- The production slice retains strict/native defaults and selects WslClientRoot
  only from typed registered/pre-project ownership or private run_wsl. WSL adds
  SILENT_BREAKAWAY_OK while retaining KILL_ON_JOB_CLOSE, suspended/no-window root
  creation, exact assignment before resume, fallible cleanup and bounded reap.
  Private capture remains separate with unchanged sanitation/control/strict ack.
  No production wait, Job retention, source retargeting or result remap appears.
- The new fixture call is after the entire original actual-test lifecycle loop.
  Its original setup/account/cwd/argv/privacy checks, private 50-ms polling,
  ten-second readiness budget, result checks and descendant assertions remain
  intact. The failure-only A/B timing module is not invoked by the new rows.
- Four ordinary rows cover Project/PreProject times Complete/Timeout. Each
  starts its short client first, requires that client's own fresh short-ready,
  then starts an independent peer and requires peer-ready. Both workers must
  still be pending and the short's fixed completion/timeout window must remain
  open. Completion requires empty normal exit 0; timeout requires the actual
  timeout receipt, not a transport error accepted as timeout.
- Four private rows cover data/lookup cancellation/timeout. Private and peer
  directories are distinct fresh UUIDs. The private request starts and proves
  its existing descendant ready before the peer starts. The private deadline
  must remain open and both workers pending at peer readiness; only then is
  cancellation signalled or normal timeout awaited. Exact production Cancelled
  or TimedOut is required. The unchanged ready/release/one-second/absent-marker
  postcheck still supplies descendant evidence; no private bytes or diagnostic
  text are parsed to manufacture acknowledgement.
- Every row requires the peer pending after the earlier client returns/retire
  checks complete, before its fixed fifteen-second sleep can have elapsed.
  The peer must then return normal exit 0, exact start/end stdout and empty
  stderr. This checks survival across retirement; it is not just sequential
  success or a worker started after the affected client already finished.
- Worker owns each join handle. Private unwind signals cancellation before
  joining. The later peer carries that same cancellation handle, so peer
  startup/readiness/assertion unwind signals private cancellation before waiting
  for the peer. Spawn failure unwinds through the already-owned earlier worker;
  normal joins remove the handle without spurious cancellation. Public joins
  retain finite script/runner bounds; no manual kill or held completed Job.
- Root/distro attestation is required before any case creation. Existing case()
  performs fresh UUID mkdir without -p; collisions fail. Fixed public plans read
  only their own markers. All failure text is static/typed/numeric, with no
  stdout/stderr body, private payload, argv/environment or arbitrary error echo.
- Eight rows add at most 3,240 Windows-to-WSL calls: twelve mkdir, four ordinary
  short calls, eight peers, four private envelopes, twelve descendant postchecks
  and sixteen readiness loops capped at 200 calls each. Readiness also has a
  ten-second budget with 50-ms intervals. Ordinary short/probe/setup calls retain
  five-second/4096-byte limits; peers have fixed fifteen-second sleep and
  twenty-second runner deadlines. Private cancel/timeout limits remain 15/5
  seconds for these new rows. Final in-progress probes, cleanup grace and OS
  scheduling remain documented limits, not hidden retries or changed baselines.

### Eight New Unit Tests: Source Reviewed, UNRUN

- `execution_host::tests::process_tree_policy_follows_typed_sources_not_executable_or_path`
- `execution_host::tests::windows_process_tree_policies_set_exact_job_limits_and_keep_strict_default`
- `execution_host::tests::windows_process_tree_policies_keep_exact_root_attachment_and_retirement`
- `tasks_account_executor::process::tests::private_wsl_entry_rejects_non_wsl_sources_before_capture`
- `tasks_account_executor::process::tests::private_root_cleanup_preserves_failure_before_assignment_for_both_policies`
- `tasks_account_executor::tests::wsl_containment::containment_requires_attested_root_before_case_creation`
- `tasks_account_executor::tests::wsl_containment::containment_public_plans_and_limits_are_fixed`
- `tasks_account_executor::tests::wsl_containment::containment_worker_ownership_joins_and_cancels_on_unwind`

The first five include actual native Job-limit/root guard tests and the real
private pre-assignment failure cleanup. The last three pin fixture ownership,
literal plans/budgets and join/cancel behavior; the unwind test confirms that
cancellation is observed before joining a panicking worker without double panic.
These tests do not establish actual WSL peer survival. The eight real rows are
appended to the existing single ignored
`tasks_account_executor::tests::tasks_account_executor_wsl_sentinels_cleanup_and_foreground_host`,
not new skipped tests or an alternative passing gate.

### Evidence And Remaining Limits

Main's authoritative predecessor evidence is f1fde4d Windows job 101478701601:
all eighteen timing/public regressions and both Git receipt safety tests passed
(execution-host 25, Tasks 44 plus one ignored, Git 126). Package 101478701516
passed; Linux formatting failed. Actual WSL 101478843292 still failed its
original DataCancel assertion while showing the positive Immediate/AfterProducer
timing contrast; owned cleanup passed. None validates this new ownership/peer
fixture source based on the formatter successor 551595b.

Required next evidence remains exact-SHA Actions build/lint/format/unit gates
and the original actual WSL test executing once, including every original
assertion and all eight appended rows, followed by owned-distro cleanup.
Current production success is not claimed. Short/private-first startup and
verified overlap do not force cold instance startup: setup and earlier cases
can warm WSL. No distro restart/shutdown was added or authorized here.

Silent breakaway is not selective for shared infrastructure; escaped relays,
interop and guest daemons remain outside full containment claims. Ordinary WSL
still has no positive guest-stop acknowledgement, and the existing typed WSL -1
Git uncertainty/no-replay/exact-ID lease review is preserved. Private Linux
group/ack evidence is not Windows Job or universal guest cleanup proof. WSL2,
enabled interop, nested Jobs, real external/shared-distro clients and native UI
acceptance remain separate. No new supervisor or fault-injection framework is
needed for this release. Earlier source-release sections below are historical;
this section is the current combined containment handoff.

## Final Combined Check: Source Released To Main

Read Newton's released wsl-retirement-implementation.md, the finalized timing
contract/research and all eighteen diagnostic test functions. The prior Tesla
Git receipt review remains clean and consistent with the diagnostic boundary.
No unresolved source finding remains. The exact check-owned paths are released
and frozen:

- `crates/mt-app/src/execution_host.rs`
- `crates/mt-app/src/tasks_account_executor/tests.rs`
- `.trellis/tasks/09-06-native-tasks-gh-accounts/review.md`

The final checker changed only a test-worker cleanup guard/regression in
execution_host.rs and this report. Newton's finalized Tasks tests needed no
additional edit. No Git source, private capture/envelope, ProcessTree method,
creation/Job flag, public API, dependency, workflow or spec was changed.

### Findings Resolved

Both earlier WIP findings are resolved in Newton's release. Reply::received
scans both full public producer streams in UTF8/UTF16LE/UTF16BE before rejecting
incomplete/stale captures; only the sentinel boolean survives rejection.
Report::describe computes combined suppression before decoding either row.
Both row directions, streams, encodings, beyond-cutoff sentinels and rejected
captures are covered. The old single-row constructors, helper calls and polling
record tests have been migrated to the final two-row types.

The final check found and corrected one additional test-only ownership defect:
the shared timing tests used bare JoinHandles, so a controller assertion/unwind
before join could detach a held worker until its deadline. The local ProbeJoin
owner now aborts before joining on unwind and preserves the normal join result.
Both held synthetic and native test workers use it. The deadline-notification
worker is scoped so assertion unwind joins it too. The existing
public_timing_wait_releases_on_return_abort_and_absolute_deadline regression
now also unwinds a controller while an actual guarded native child is held,
then requires native exit 1, exactly one successful retirement, worker completion
before unwind returns and ProducerAborted release. No new test function or
production synchronization was added.

### Final Boundary Review

- Only cfg(test,windows) timing code is active. The ordinary runner's hook is
  after direct child exit and both joined readers, before the same existing
  process_tree.terminate()?. Complete empty untruncated exits 0 and 1 are
  eligible; timeout/read/dispatch errors bypass the hold and retain their result.
- Pair owns fixed coordination/metadata only. Its absolute start-plus-ten-second
  deadline cannot be extended by wakeups. No Job/process handle is exported,
  stored in the coordinator, pre-disarmed or replaced. Normal release and
  unwind retain the original guard, explicit termination and handle closure.
- Public producer return or abort is signalled before readiness join.
  ProbeOwner supplies the fallback abort/join on controller unwind, with typed
  probe/producer errors. TLS resets on return/unwind and rejects nested/reused
  scope owners. Only one public probe per row can wait.
- Rows remain Immediate then AfterProducer, each using its own attested UUID,
  no-reuse mkdir, unchanged literal producer and own-marker probe: at most six
  host commands. Command bounds remain five seconds/4096 bytes plus existing
  cleanup grace. No startup barrier or retry was added to the actual comparison.
- The final escaped diagnostic is at most 4096 bytes across both rows. Its
  compact fallback remains explicitly inconclusive; only public producer bytes
  can preview. Exact ASCII-prefix separation, strict UTF decoding and all prior
  control/bidi escaping remain. No private stream or arbitrary error is exposed.
- Original private 50-ms polling, ten-second readiness budget, control/clock
  ordering, strict acknowledgement, baseline result checks and final descendant
  assertions remain in place. Diagnostic success cannot replace any assertion.
- Fallible native cleanup was source-traced: pre-spawn Job ownership drops on
  spawn failure, post-spawn attach/read setup errors use the existing bounded
  cleanup/direct-child fallback, and the native root fixture constructs its
  owner before the fallible attachment assertion. No successful Windows cleanup
  is inferred from a test timeout, API failure or missing retirement record.
- Tesla's typed WSL Some(-1) classification only affects Git receipt uncertainty;
  this diagnostic neither calls it for public commands nor remaps private
  results. Native/SSH/positive-exit behavior and exact-ID/no-replay lease review
  remain as independently reviewed below.

### Eighteen Functions Inspected: UNRUN

Under `execution_host::tasks_wsl_public_timing::tests::`:

- `public_timing_inactive_and_exit_zero_one_eligibility`
- `public_timing_wait_releases_on_return_abort_and_absolute_deadline`
- `public_timing_scopes_reset_isolate_and_reject_nested_or_reused_owners`
- `public_timing_native_runner_preserves_output_and_retires_after_release`
- `public_timing_native_timeout_truncation_and_dispatch_errors_bypass_hold`
- `public_timing_read_failure_preserves_the_error_without_completion`

Under `tasks_account_executor::tests::wsl_public_comparison::`:

- `public_comparison_plans_are_fixed_and_require_the_attested_root`
- `public_comparison_attempt_is_exact_once_and_never_replaces_private_failure`
- `public_comparison_joins_readiness_on_producer_and_thread_errors`
- `public_comparison_two_rows_each_probe_once_even_when_absent_or_failed`
- `public_comparison_signals_before_join_on_return_error_and_producer_unwind`
- `public_comparison_owner_unwind_aborts_before_join_and_probe_panic_is_inconclusive`
- `public_previews_reject_nonproducer_incomplete_and_undecodable_sources`
- `public_previews_scan_full_both_streams_all_encodings_before_cropping`
- `public_previews_decode_only_exact_start_prefix_and_native_error_framing`
- `public_previews_and_total_json_are_bounded_and_escape_controls`
- `public_two_row_privacy_and_total_budget_keep_metadata_when_previews_are_dropped`
- `public_retirement_api_failure_and_missing_or_late_evidence_remain_inconclusive`

This is source inspection of all eighteen, not eighteen passing executions.
The final check strengthens an existing function, leaving the count unchanged.
Real native runner cases exercise normal retirement and timeout/truncation
paths; failed-retirement projection is synthetic. No actual failing Windows
termination API injection or independent handle-closure probe is claimed.

### Gates And Limits

Main's 35fe653 ordinary Linux/Windows passes validate the earlier public slice,
not these new timing/receipt changes. Its actual WSL gate still failed with a
public instance-termination reproduction; no causal conclusion is added here.
Exact-SHA Actions compilation, tests, formatting and the original nonempty actual
WSL ignored gate plus owned-distro cleanup remain pending. Both new Git safety
tests remain UNRUN as recorded in the Tesla check below.

The diagnostic remains post-failure, single-pair-per-row and scheduler-sensitive.
No overlap, abort/deadline, transport/cleanup error or differing startup order
is a passing fix. Its delay cannot identify a kernel member, prove guest
descendant cleanup or establish WSL2/interop/native UI acceptance. No production
wait or Job-flag contrast is selected. No local automated verification, probes,
Git writes, app launches or child agents occurred. Main owns all integration,
formatter artifacts, specs, Git and Actions. Earlier handoffs below are history;
this section is the current source-release status.

## Post-Failure Public Comparison: Source Released To Main

The bounded test-only slice is authored and source-reviewed, with no remaining
source/API blocker identified. Ownership is released to Main and source writes
are frozen. Only `crates/mt-app/src/tasks_account_executor/tests.rs` and this
report changed. No production, shared runner, private capture, classifier,
fixture executable, dependency, workflow or spec changes. Main owns formatter
artifacts, Git, integration and exact-SHA Actions validation.

### Trigger, Ownership And Result Preservation

The one-attempt gate requires DataCancel or LookupCancel, the original typed
HostHelperUnavailable error, and false cancellation-at-return. It runs only
after the original private result, timestamps and readiness join are captured.
The public entry accepts an attested fixture and typed metadata, never private
output buffers. Expected success, timeout, other errors and already-cancelled
returns cannot dispatch it. Public success, setup/probe/thread failure or output
never replaces the original readiness/result/descendant assertions, which remain
unchanged. No pre-auth/account reordering or original cadence/deadline changes.

WslFixture records its exact distro receipt only after the existing same-run
owner, resolved executable, synthetic ELF hash and cases-directory checks.
Those preflight commands resolve gh/Python but do not invoke an account helper.
The public comparison also requires the exact attested distro and both root
paths `/mini-term-fixture`; a private-case cwd or different backend is rejected.
Each comparison uses a new UUID under `/mini-term-fixture/cases`, with guarded
`/bin/mkdir -- <own-path>` and no `-p` or reuse. Setup errors are typed metadata.

The immutable producer is `/bin/sh -c` with this literal script:

```sh
printf '%s\n' mt-public-start && : > ready && /usr/bin/sleep 1 && printf '%s\n' mt-public-end
```

Its concurrent probe is only `/usr/bin/test -f ready` in that same fresh cwd.
Neither command reads gh/Python/accounts, user environment values, private
streams or sibling files. Both use the existing guarded host runner with a
5-second/4096-byte command bound. Probe cadence remains 50 ms with a 10-second
budget; a final in-progress probe and existing runner drain/cleanup grace can
extend the join beyond that budget. Readiness stores only saturating count,
first/final same-clock timestamps and typed metadata. All ordinary producer
result paths join readiness; thread creation/join errors remain static metadata.
No additional kill, Job handle, barrier or producer retry is introduced. Final
owned-distro cleanup remains authoritative, not proof of per-command Linux
descendant cleanup.

### Public Output Boundary

Only the nested comparison module's fixed producer can retain preview bytes.
Setup/probe replies, dispatch/read errors, unexpected epochs, incomplete or
over-bound output cannot supply them. The formatter has no exported arbitrary
buffer input. Both full bounded streams are scanned for the fixture sentinel
`fixture_credential_` in UTF8/UTF16LE/UTF16BE before any prefix separation or
cropping; a hit in either stream suppresses ALL previews. This is a fixture-only
sentinel guard, not a general secret sanitizer.

Only the exact ASCII `mt-public-start\n` stdout prefix may be separated. Its
presence is metadata, and the remaining public bytes are strictly decoded,
including native UTF16LE/BE after that prefix or without a prefix. Unsupported
framing/encoding is metadata-only; no lossy decoding or private normalization.
Each preview is at most 256 Unicode scalars before structured JSON escaping.
C0/C1 controls, U+061C, U+200E/U+200F, U+2028-U+202E and U+2066-U+2069 cannot be
emitted literally. The complete public diagnostic is capped at 4096 bytes,
dropping previews or returning fixed cap metadata if needed. Error messages,
argv, environment, process identifiers and private streams are never formatted.

### Eight New Tests: Authored UNRUN

All names below are under
`tasks_account_executor::tests::wsl_public_comparison::`:

- `public_comparison_plans_are_fixed_and_require_the_attested_root`
- `public_comparison_attempt_is_exact_once_and_never_replaces_private_failure`
- `public_comparison_joins_readiness_on_producer_and_thread_errors`
- `public_comparison_readiness_retains_own_probe_timing_and_stops_on_errors`
- `public_previews_reject_nonproducer_incomplete_and_undecodable_sources`
- `public_previews_scan_full_both_streams_all_encodings_before_cropping`
- `public_previews_decode_only_exact_start_prefix_and_native_error_framing`
- `public_previews_and_total_json_are_bounded_and_escape_controls`

These exercise structured fixed plans and ownership denial, exact trigger and
one-attempt behavior, real test-thread joining/error paths, bounded readiness
storage, both-stream suppression across all three encodings and beyond the
preview cutoff, malformed input, ASCII-start plus UTF16 body, native no-marker
errors, and JSON roundtrip/control/cap assertions. The source pass corrected a
test comparison against ExecutionBackend, which intentionally has no PartialEq,
by matching the exact WSL distro instead. The existing actual ignored test
`tasks_account_executor::tests::tasks_account_executor_wsl_sentinels_cleanup_and_foreground_host`
retains all original cwd/literal/account/concurrent-readiness/strict-ack/final
descendant assertions. No local execution or automated verification occurred.

### Evidence And Limits

The previously authorized
[682ac33 log, run 34023590512 / job 101460683298](https://github.com/vihor3/mini-term/actions/runs/34023590512/job/101460683298)
executed exactly one actual WSL test and failed in 20.26 seconds at DataCancel.
The private process exited before cancellation with HostHelperUnavailable,
false acknowledgement and Unknown native classification despite the width
mask. At the first readiness retirement the private root was NotInJob and Alive.
Its final descendant assertion was not reached; owned distro cleanup succeeded.
No inference is made from private output length. Main subsequently confirmed
all ordinary/package jobs succeeded for 682ac33; that completed candidate does
not validate this newly authored diagnostic.

The comparison runs in the post-failure disposable distro, not a clean-room
state. Its ordinary runner uses null stdin, unlike the private piped control
channel. A public reproduction would not establish Job causality, and public
success is inconclusive, never a passing fix. Actual WSL early-exit cause, full
transport acceptance and separate native hardware acceptance remain unresolved.
No children, local checks or Git writes occurred. The SDK/compiler handoff below
is historical and does not describe the current two-path release.

## SDK Width Mask Compiler Fix: Source Released To Main

The narrow compiler correction is authored and source-reviewed, with no
remaining source blocker identified. Ownership is released to Main and source
writes are frozen. Only `crates/mt-app/src/tasks_account_executor/process.rs`
and this report changed. No dependency, feature, other source, CI or spec edits.

Main reports a7b8b11 run 34022945645 / Windows job 101458780310 failed E0432 at
process.rs:139 and :1004: the locked windows 0.61.3 Debug namespace does not
export FORMAT_MESSAGE_MAX_WIDTH_MASK. This was an invalid binding import in
both renderer and regression, not evidence against the width-mask behavior.
No actual WSL execution is claimed for that head; Main owns the pending WSL,
ordinary and package conclusions. Earlier a98473e ordinary/package success
does not validate the newer renderer or this correction.

The lockfile still selects windows 0.61.3 for this dependency. Microsoft's
[SDK WinBase.h declaration](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/RecompiledIdlHeaders/um/WinBase.h#L2390)
defines FORMAT_MESSAGE_MAX_WIDTH_MASK as 0x000000FF, consistent with the
[FormatMessageW flag documentation](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-formatmessagew).
Published current bindings place this numeric mask under
[WindowsProgramming](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/WindowsProgramming/constant.FORMAT_MESSAGE_MAX_WIDTH_MASK.html),
not Debug; mt-app does not directly enable that namespace. No reliance on
transitive feature enablement or new feature is needed for this SDK value.

The invalid imports are replaced with the clearly named private test/Windows
SDK_FORMAT_MESSAGE_MAX_WIDTH_MASK = 0x000000ff, with inline provenance. The
renderer retains the exact FROM_SYSTEM | IGNORE_INSERTS | max-width bits. The
existing independent regression constructs its expected mask from the literal
SDK value via the already-used FORMAT_MESSAGE_OPTIONS type; it does not reuse
the renderer's private mask constant. Its exact-flags assertion and three fixed
trusted-system-message comparisons are retained. No test was removed and no
new test function or diagnostic behavior was added.

The 512-u16 buffer, 4096-byte input/candidate caps, 3015-ID catalogue, full-message
matching, privacy, false-ack/nonzero/trace gates and production results remain
unchanged. No Job, cancellation, private-input or resource-wrapper changes.
The correction and existing renderer regression remain UNRUN pending exact-SHA
Windows Actions. No local execution, automated checks, Git writes or child
agents occurred. WSL early-exit causality remains unconfirmed. Main owns the
remaining Actions, formatter, integration and release gates.

## Earlier WSL System Rendering Flags Handoff

Historical handoff below predates the a7b8b11 compiler failure. The SDK constant
correction above supersedes its binding-readiness statement, not its intended
rendering behavior or privacy boundary.

The narrow test-renderer correction is authored and source-reviewed, with no
unresolved implementation blocker. Ownership is released to Main and source
writes are frozen. Only `crates/mt-app/src/tasks_account_executor/process.rs`
and this report changed. Main owns specs, Git, formatter artifacts and Actions;
the ongoing ordinary/package gates are not cancelled by this work.

### Confirmed Difference And Bounded Correction

Microsoft's
[GetSystemErrorString](https://github.com/microsoft/WSL/blob/master/src/windows/common/wslutil.cpp#L1047)
uses FORMAT_MESSAGE_MAX_WIDTH_MASK together with FROM_SYSTEM and IGNORE_INSERTS.
Our trusted renderer omitted the width flag. Microsoft documents that this flag
[ignores regular resource line breaks while preserving hard-coded breaks](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-formatmessagew).
Outer BOM/CR/LF trimming alone does not make the two renderings equivalent.

The renderer now uses exactly those three flags, pinned by a private test-only
constant. It does NOT adopt WSL's ALLOCATE_BUFFER flag: the original 512-u16
buffer and length checks remain. The 4096-byte input bound, 4096-candidate cap,
3015-ID catalogue, strict UTF decoding and complete-message matching are
unchanged. No private input stripping or normalization was added. Matching still
runs only under trace for a nonzero exit with false cleanup acknowledgement,
and emits only the existing numeric SystemMessageId or Unknown.

There is no substring, resource or contextual-wrapper parser, new catalogue,
Job/launcher change, production result remapping or acknowledgement relaxation.
The confirmed difference is in trusted message rendering, NOT a confirmed cause
of the observed early WSL exit or Unknown classification. Current upstream source
also does not prove the implementation of this runner's installed WSL binary.

### One New Test: Authored UNRUN

- `tasks_account_executor::process::tests::native_windows_message_renderer_uses_exact_wsl_system_flags`

The test pins the exact flag set and independently calls FormatMessageW with
those flags for ERROR_BAD_EXE_FORMAT, ERROR_BROKEN_PIPE and
ERROR_EXE_MACHINE_TYPE_MISMATCH. Full results must equal the renderer's output;
failure diagnostics contain only static text and the fixed numeric ID. This
covers the intended rendering where a localized resource has regular breaks,
without assuming that every runner language uses multiline resource text.
All existing framing, contamination, Unknown, catalogue-bound and gating tests
remain unchanged. No all-catalogue API roundtrip or new diagnostic framework.

### Exact Evidence And Remaining Gates

The authorized
[a98473e log, run 34021441260 / job 101454796139](https://github.com/vihor3/mini-term/actions/runs/34021441260/job/101454796139)
ran exactly one actual WSL test and failed in 28.96 seconds at LookupCancel.
The private root was NotInJob and Alive at the first readiness termination;
its exit at 144475 us preceded cancellation at 298343 us. The native message
classification remained Unknown, cleanup acknowledgement was false, and owned
distro cleanup succeeded. Earlier DataCancel and LookupTimeout cases, including
their descendant assertions, completed by source ordering. This is not a passing
full WSL gate or proof of the failed case's descendant cleanup.

The new flag correction and regression are UNRUN pending Main's next exact-SHA
Actions gate. Current ordinary/package outcomes remain Main's responsibility.
No local execution, automated checks, Git writes, child agents or other source
edits occurred. WSL early-exit causality and native hardware acceptance remain
unresolved; no production correction is claimed.

## Earlier Root-Only WSL Diagnostics Handoff

Historical release status follows. The rendering-flag correction and current
evidence above supersede its pending-gate summary without reopening its scope.

The approved test-only slice is authored and source-reviewed. No unresolved
source/API blocker is identified. Ownership is released to Main and source
writes are frozen. Exact release paths: `crates/mt-app/src/execution_host.rs`,
`crates/mt-app/src/tasks_account_executor/process.rs`,
`crates/mt-app/src/tasks_account_executor/tests.rs`, and this report. Main owns
specs, CI, complete Actions formatter artifacts, Git and exact-SHA validation.

### Root Observer Boundary

One per-case Arc/Mutex registry holds at most two non-inheritable process
OwnedHandles, indexed only by the fixed Private and Readiness roles. Scoped
test/Windows TLS selects the registry and role; neither the registry nor its
handle owner implements Debug. Both existing runners register their exact Child
only after successful Job attachment. No PID lookup, member enumeration, Job
handle retention or cross-test identity registry was introduced.

[DuplicateHandle](https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-duplicatehandle)
requests only PROCESS_QUERY_LIMITED_INFORMATION plus PROCESS_SYNCHRONIZE, with
zero duplicate options and inheritance disabled. Replacement closes the old
role reference before duplicating a new one. Registration and observation use
try_lock: a missed registration invalidates the old role instead of presenting
it as current. Missing, busy and failed-query observations are explicit, not
false membership or fabricated zero counts.

Immediately before the EXISTING readiness TerminateJobObject call, the observer
queries the two exact roots against THAT Job with
[IsProcessInJob](https://learn.microsoft.com/en-us/windows/win32/api/jobapi/nf-jobapi-isprocessinjob).
Membership and independent zero-wait liveness have separate typed states.
The private root is therefore reported as InJob, NotInJob, QueryFailed or
Unavailable, independently of Alive, Exited, QueryFailed or Unavailable.
Readiness-root metadata supplies the corresponding known-root observation.
Only these states enter the existing fixed first/final retirement record;
counts, timestamps, success results and termination arguments remain unchanged.

TLS resets on return and unwind. The existing concurrent readiness thread shares
only its case's registry and clock; it is joined before the original assertions.
No peer wait, start barrier, cadence/deadline change, additional WSL command,
termination, cancellation decision or result remapping was added. The observer
does not preserve roots beyond the case or retain Job handles. Process reference
lifetime and Job counts are not Linux descendant-cleanup evidence.

### Bounded Complete-Message Catalogue

The existing windows_system_message/FormatMessageW path now considers base
Win32 IDs 0..=1999, WinSock IDs 10000..=11004 and ten named standard HRESULT IDs.
The fixed ordered catalogue has 3015 candidates and a hard iterator cap of
4096. Input remains capped at 4096 bytes and the system-message buffer at 512
u16 units. FROM_SYSTEM and IGNORE_INSERTS, strict UTF decoding and complete
BOM/newline-framed matching are unchanged. No private-byte cache was added.

Only a numeric SystemMessageId or Unknown is emitted, solely for an already
traced nonzero transport exit with false cleanup acknowledgement. The renamed
variant does not mislabel HRESULTs as Win32 errors. A matching ID identifies a
trusted catalogue message, not necessarily a unique underlying OS error when
messages alias; deterministic catalogue order selects the first match. Unknown
remains Unknown. No raw output, paths, handles, argv, environment or credentials
are logged, and byte length is not used to infer an error.

### Three New Tests: Authored UNRUN

- `execution_host::tests::tasks_wsl_root_observer_distinguishes_known_jobs_and_root_liveness`
- `execution_host::tests::tasks_wsl_root_observer_preserves_results_and_isolates_scopes_and_contention`
- `tasks_account_executor::process::tests::native_windows_message_catalogue_is_fixed_bounded_and_ordered`

The root tests use fixed guarded native cmd roots to distinguish known own/peer
Job membership and Alive/Exited states, with normal bounded guard cleanup.
They cover missing/busy/query-failed states, invalidated stale registration,
per-thread/per-case isolation, unwind/TLS release and preserved results. The
existing guarded exit-23 runner test also verifies its registered root metadata
and unchanged exactly-once retirement. No raw handle is formatted by assertions.

The existing readiness fixed-storage test retains typed root metadata through
first/final recording and saturation. Catalogue tests cover exact bounds/order,
retention of all original 26 IDs and a newly covered system ID. Five representative
messages exercise UTF-8/UTF-16 complete matches; no all-ID roundtrip API loop is
used. Existing private-payload, malformed UTF, Unknown and failure-only guards
remain, with whitespace-contamination rejection added.

### Evidence And Remaining Gates

Main reports remote c92e54a run 34017910209 Linux, Windows and packaging passed,
except actual WSL job 101445078760. That failure remained DataCancel, with
owned-distro cleanup passing but no final descendant check. Count(4), timing
correlation and the earlier Unknown message do not establish cross-Job cause.
This slice is diagnostic only, NOT a WSL root-cause correction.

All three new tests and updated observer/catalogue/fixture assertions are UNRUN.
The object-only parser in Main's local unpushed 7b9eb0f also remains UNRUN pending
the next Actions gate. Existing execution_host and Tasks filters cover this
release; no workflow, dependency, mod.rs, gh_fixture, Python/SSH, production
policy or public API change was made. Actual cwd/literal/account/readiness,
strict acknowledgement and final descendant assertions are retained. No local
execution, automated checks, Git writes or child agents occurred. Native hardware
acceptance and a passing full actual WSL gate remain separate requirements.

## Earlier Object-Only Host Reply Handoff

The parser slice below is already committed locally by Main as 7b9eb0f. It is
unchanged by this diagnostic follow-up and remains pending exact-SHA Actions.

The bounded correction is authored and source-reviewed with no implementation
blocker. Ownership is released to Main and source writes are frozen. Exact
release paths are `tasks_account_executor/mod.rs`, `tests.rs`, `gh_fixture.rs`
and this report. No public API, dependency, valid wire object, error mapping,
size bound, cancellation or credential-policy change was made. Main owns
specs, CI, formatter artifacts, Git and exact-SHA Actions validation.

### Confirmed Shape Gap And Correction

The locked Serde 1.0.229
[TaggedContentVisitor::visit_seq](https://raw.githubusercontent.com/serde-rs/serde/v1.0.229/serde/src/private/de.rs)
accepts the first sequence element as the internally tagged enum's tag. Its
[struct variant derive](https://raw.githubusercontent.com/serde-rs/serde/v1.0.229/serde_derive/src/de/struct_.rs)
also accepts sequences. Empty struct statuses therefore close extra fields but
do not enforce a top-level object: `["cancelled"]` and `["output","ok","",0]`
still have valid positional shapes. This gap is confirmed
from the locked primary source, not from running a local probe.

One private `parse_host_reply` now requires `deserialize_map`, passes the
original map through `MapAccessDeserializer` to the existing HostReply derive,
and calls `deserializer.end()`. This follows the existing structured object
boundary in `mt-github/src/accounts.rs` without adding a shared abstraction.
There is no Value normalization or string-prefix parser: original duplicate
keys remain visible, and trailing non-whitespace data is rejected.

Both decoding and cleanup acknowledgement use this parser. Valid objects,
whitespace framing, closed fields and typed Output fields keep their existing
behavior. Invalid root arrays, scalars, nested shapes or trailing data produce
Protocol at decode and false at acknowledgement. Parser errors and private
payloads are not echoed. A stopped request with an array acknowledgement still
requires CleanupFailed, never a successful cancellation result.

### Two New Tests: Authored UNRUN

- `tasks_account_executor::tests::host_reply_statuses_require_single_objects_not_positional_arrays`
- `tasks_account_executor::tests::host_reply_output_rejects_positional_nested_scalar_and_trailing_json`

These cover every status-only positional array, nested arrays/object wrappers,
scalars, the positional Output sequence, nested Output field values, trailing
JSON values and accepted object whitespace through the shared parser and both
consumers. Existing all-status valid mappings, extra/token/nested fields,
duplicate fields, required fields and wrong-type regressions remain intact.

The existing
`private_capture_diagnostics_keep_cancellation_acknowledgement_required` test
also gains fixed `cancel-array-ack` fixture mode. It receives the real private
control byte, emits only synthetic `["cancelled"]`, and must return CleanupFailed
with false acknowledgement. Original extra-field and metadata safety assertions
are unchanged. This is an updated guarded-capture test, not a third new function.

### Limits And Separate Investigation

No local executable checks, fixtures, Git writes or child agents occurred.
The broad existing Tasks test filters cover this slice; Main must run the new
tests and rebuilt synthetic fixture in Actions. No execution_host, process,
classifier, Python/SSH envelope, CI or spec edits belong to this follow-up.

Main reports c92e54a actual WSL job 101445078760 failed at DataCancel in 29.79
seconds, with owned-distro cleanup passing and no final descendant check. Its
early-exit/Job investigation remains separate and is not marked fixed here.
No full WSL transport or native hardware acceptance is claimed. Current ordinary
Actions outcomes remain Main's integration responsibility.

## Earlier Strict Reply And Retirement Handoff (Superseded)

Historical status below describes the prior c92e54a diagnostic release. The
object-only correction and current validation limits above supersede its
protocol-completeness and UNRUN statements; its observer/classifier scope is
not reopened by this follow-up.

The approved slice is authored and source-reviewed, with no implementation
blocker. Ownership is released to Main and source writes are frozen. Exact
release paths are `tasks_account_executor/mod.rs`, `process.rs`, `tests.rs`,
`gh_fixture.rs`, the narrowly authorized test-observer surface in
`execution_host.rs`, and this report. Main owns specs, CI, formatter artifacts,
staging/commits and exact-SHA Actions reruns. No local execution, automated
checks, Git writes or child agents occurred.

### Exact New Evidence

Read the authorized
[bb79421 WSL log, run 34015670072 / job 101438999280](https://github.com/vihor3/mini-term/actions/runs/34015670072/job/101438999280).
Exactly one actual test failed in 20.85 seconds, now identified as `DataCancel`.
The private API returned `HostHelperUnavailable` at 155760 us with cancellation
false. Capture observed exit -1 at 155611 us, local tree retirement at 155666 us
and drained pipes at 155727 us: no latched stop, no control cancellation, no
cleanup acknowledgement, 118 stdout bytes and zero stderr bytes. Neither
StopLatched nor ControlWrite occurred. Readiness completed two probes: first
start 2632 us, final start/return 196952/299384 us, cancellation 299386 us.
Owned `mt-tasks-34015670072-1` cleanup succeeded.

The private process/API had already exited before cancellation. Cancellation
remapping cannot fix this observed failure. An earlier false readiness command
overlapped the request, but the evidence does NOT establish cross-Job causation
or identify a native error from byte length. The failed case's Linux descendant
assertion still did not run. All original literal/cwd/empty/cardinality and
preceding account cases remain exercised by source order, not a passing full
WSL transport claim.

Also read the authorized
[bb79421 Linux log, job 101438890459](https://github.com/vihor3/mini-term/actions/runs/34015670072/job/101438890459).
mt-app had 1214 passed, one failed and five ignored. The sole failure was
`capture_diagnostics_are_bounded_payload_free_metadata`, then at process.rs:843.
The other three Linux diagnostic/lifecycle tests passed. Main reports the same
sole assertion failure on Windows job 101438890388 (Tasks 22 passed, one failed,
one ignored; execution-host 14 passed). Later SSH/sidecar/whitespace gates on
Linux and later Tasks/Git/terminal gates on Windows did not run after those
failures. Compile and Clippy passed before the failures.

Main reports package run 34015670075 succeeded for bb79421, artifact 9984115137,
`Mini-Term_1.2.2-ci.55_windows-x64`. Packaging success does not validate the
failed ordinary tests, actual WSL cancellation, or this unrun source delta.

### Confirmed Protocol Defect And Correction

The lockfile selects Serde 1.0.229. Its
[internally tagged derive branch](https://raw.githubusercontent.com/serde-rs/serde/v1.0.229/serde_derive/src/de/enum_internally.rs)
routes unit variants to a
[visitor that consumes extra map entries as IgnoredAny](https://raw.githubusercontent.com/serde-rs/serde/v1.0.229/serde/src/private/de.rs).
Consequently the synthetic `cancelled` reply with an extra `token` field was
accepted by `host_reply_confirms_cleanup` despite enum-level deny_unknown_fields.
This is the erroneous expected-false case in the failed metadata test, not a
reason to weaken that assertion. No actual token disclosure is inferred.

All twelve status-only HostReply variants are now empty struct variants under
the existing attribute. The
[struct derive path](https://raw.githubusercontent.com/serde-rs/serde/v1.0.229/serde_derive/src/de/struct_.rs)
enforces unknown-field rejection. Match arms were adjusted; valid wire objects,
existing error mappings, Output fields and cleanup-failed treatment are unchanged.
There is no custom string parser or public API change. Invalid extra/duplicate
fields and wrong field types return Protocol from decoding and false from
cleanup acknowledgement. The cooperative capture path therefore retains
CleanupFailed for such an invalid stopped reply.

The original failing metadata assertion and synthetic extra-token input remain
unchanged. A fixed `cancel-extra-field` fixture mode now reads the actual control
byte and emits a status-plus-extra-field reply; the existing guarded capture
test requires CleanupFailed, never Cancelled. It does not supply a real token.

### Passive Native Error And Retirement Evidence

The Windows-only test classifier runs solely for an actively traced, nonzero
transport exit without a valid cleanup acknowledgement. It reads only already
captured bytes, capped at 4096, supports strict UTF-8 and UTF-16 LE/BE decoding,
and ignores only BOM/newline framing. It compares the COMPLETE message against
26 fixed relevant system-message IDs. Named Win32 constants are used where the
enabled bindings provide them; named numeric RPC/socket IDs need no new feature.
The five suggested general-function/buffer/stub error IDs are included without
inferring any error from the observed 118-byte length.

[FormatMessageW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-formatmessagew)
uses only FROM_SYSTEM plus IGNORE_INSERTS, a 512-unit fixed buffer, no input text,
inserts, arbitrary IDs or allocation flag. Only `Win32(code)` from the fixed list
or `Unknown` enters diagnostics. Unknown, malformed, over-cap, JSON-contained
or prefix/suffix-contaminated messages remain Unknown. No raw private text,
argv, environment, account fields or output-derived labels leave the classifier.
Production result selection, private pipes and zeroing are unchanged.

The shared `cfg(all(test, windows))` observer scopes fixed TLS storage around
the existing readiness command. Its only runner hooks are immediately before
and after the EXISTING TerminateJobObject call. That call still executes exactly
once per original invocation with the same arguments, error conversion and
terminated-state update. Before it, an active observer may query basic Job
accounting; only the numeric ActiveProcesses count or static QueryFailed is
retained. Missing observations are None, not a fabricated zero. No Job/process
handles, IDs, names, raw errors or arguments enter the record. No extra cleanup,
termination, wait, retry, breakaway or alternate readiness transport is added.

TLS records a saturating call count and first/final typed observations only;
each has same-clock before/after times and a success boolean. It resets on
return or unwind and does not cross threads. The readiness recorder now retains
FIRST and final probe start/return times and their corresponding retirement
traces, together with its saturating probe count. All use the same started
Instant as private capture and cancellation. Existing cadence, deadlines,
concurrent transport, baseline failure and descendant assertions remain.

[Job accounting documentation](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_accounting_information)
describes ActiveProcesses as a Job association count, with reference-lifetime
effects. It is not a Linux descendant-cleanup proof. Retirement timing and a
native error match may narrow the next investigation; neither count nor timing
alone establishes a cross-Job cause. No Job, WSL launcher, Python or SSH policy
correction is claimed or implemented.

### Eight New Tests: Authored UNRUN

- `tasks_account_executor::tests::host_reply_statuses_keep_valid_mappings_and_reject_unknown_or_duplicate_fields`
- `tasks_account_executor::tests::host_reply_output_requires_typed_unique_closed_fields`
- `tasks_account_executor::process::tests::native_windows_message_matches_complete_system_messages_in_utf8_and_utf16`
- `tasks_account_executor::process::tests::native_windows_message_rejects_private_substrings_unknown_and_malformed_data`
- `tasks_account_executor::process::tests::native_windows_message_observation_is_failure_only_without_result_remapping`
- `execution_host::tests::tasks_wsl_retirement_observer_preserves_results_and_distinguishes_missing_queries`
- `execution_host::tests::tasks_wsl_retirement_observer_resets_on_unwind_and_is_thread_local`
- `execution_host::tests::tasks_wsl_retirement_observer_records_the_existing_guarded_termination`

Protocol tests cover every valid status/error mapping and Output required fields,
extra nested/token fields, duplicate status/known fields, invalid field types
and integer bounds through both decoder and acknowledgement APIs. Classifier
tests cover complete system messages across UTF encodings, private payloads
containing system text, malformed UTF, bounds, Unknown and every gating branch.
Observer tests cover preserved results, missing/query-failed/zero distinctions,
count saturation, first/final retention, unwind/thread isolation and an actual
guarded fixed cmd.exe exit 23 through the unchanged ordinary runner.

Updated tests/fixtures: the existing guarded cancellation-acknowledgement test
adds the extra-field mode; the readiness fixed-storage regression distinguishes
first/final retirement metadata and retains first return time through saturation.
The exact ignored
`tasks_account_executor::tests::tasks_account_executor_wsl_sentinels_cleanup_and_foreground_host`
keeps its original cases and assertions, with enriched failure-only metadata.
Existing Windows execution-host/Tasks filters and the workspace gate cover this
slice. No CI, manifest, rootfs-path, marker-contract or dependency change is needed.

### Remaining Gates

All eight new tests, updated fixtures, protocol correction, observer and classifier
are UNRUN. No local build/test/format/lint/syntax/whitespace verification occurred.
Main must apply complete Actions formatter output and obtain new exact-SHA
Linux/Windows tests plus the same-run actual WSL gate. The independent native
hardware/secure-store acceptance requirement remains. The WSL early-exit cause
is still unconfirmed and is NOT marked fixed.

## Earlier WSL Cancellation Diagnostics Handoff (Superseded)

Historical status below predates bb79421. The exact evidence, protocol correction
and passive retirement/classifier handoff above supersede it.

The bounded diagnostic slice and five focused regressions are authored and
source-reviewed; ownership is released to Main with no implementation blocker.
Only `tasks_account_executor/process.rs`, `tasks_account_executor/tests.rs`,
`tasks_account_executor/gh_fixture.rs`, and this report changed. No production
result remapping, shared ProcessTree/execution_host, Python/SSH envelope, public
API, credential policy, CI/spec, launcher or rootfs-layout changes. No local
execution/verification, Git writes or child agents. All current changes are
UNRUN, Actions-only. Main owns integration, same-run fixture rebuild and rerun.

### Actual Evidence And Source Limits

Read the authorized log for
[5a070e1 run 34013775348, job 101434114219](https://github.com/vihor3/mini-term/actions/runs/34013775348/job/101434114219).
Exactly one actual WSL test failed in 20.19 seconds at then-current
`tests.rs:2153`, returning `HostHelperUnavailable` instead of `Cancelled`.
Owned distro cleanup succeeded for `mt-tasks-34013775348-1`.

By source order, the original and new literal/captured-cwd/empty-cardinality
checks, capability/discovery, selected-account proofs and secret rejection,
Rotate cwd marker, missing-cwd/helper/error and large-response cases completed
before this failure. Empty-argv transport is therefore exercised successfully
on this runner. The old assertion does not identify DataCancel (`Slow`) versus
LookupCancel (`LookupSlow`), transport exit status, control-latch timing or host
cleanup acknowledgement. Readiness joined successfully, but the failed case's
`assert_retired` did not run. There is no passing full WSL transport or failed
cancellation descendant-cleanup claim.

Main now reports 5a Linux, Windows and package completed successfully, with only
the actual WSL cancellation gate failed. Linux includes mt-app 1211 tests, the
new empty tests and all five actual SSH fixtures. Those results precede this
diagnostic delta.

Source-proven ordering:

- Private `capture` checks control at its loop head, then polls `try_wait`.
  Observed exit breaks to owned-tree cleanup and bounded pipe draining without
  another production control check. A cancellation arriving after that last
  check can therefore remain unlatched through return. This is a real source
  window, not proof that it produced this runner failure.
- `run_wsl` maps an otherwise unstopped nonzero transport exit to
  `HostHelperUnavailable`; a decoded helper-unavailable reply can also produce
  that category. The old assertion alone does not establish which route ran.
- `WslFixture::cancel_after_start` calls the ordinary guarded WSL `/usr/bin/test`
  readiness command before setting cancellation. Its success includes generic
  runner pipe draining and owned Job retirement. It can overlap the private
  account capture. A successful join proves eventual readiness, not that the
  cancellation request preceded the private child's exit or API result.
- Python's existing stdin byte/EOF handling raises cancellation and retires
  credential children before emitting a typed reply. Private cooperative stop
  still requires `host_reply_confirms_cleanup`; absent/invalid or explicitly
  cleanup-failed replies remain `CleanupFailed`. No late flag, nonzero status
  or successful Windows Job retirement is accepted as a substitute.

The actual nonzero-exit cause and the role, if any, of the concurrently retired
readiness process remain unconfirmed. No Job interference, stdin failure or
Python defect is inferred from elapsed time or the error category.

### Bounded Same-Clock Observations

`cfg(test)` private `trace_capture` owns six fixed slots, scoped to the current
thread and reset on normal return or unwind. It preserves the closure's result
without decoding, substituting or adopting any alternative. Existing captures
without an active trace retain no observations. Stages are `Attached`,
`StopLatched`, `ControlWrite`, `Exited`, `TreeRetired` and `Drained`.

Each observed stage records elapsed microseconds, numeric exit status, the
already latched typed stop error, and a separate control sample. ControlWrite
records only whether the fixed byte write succeeded; Drained records typed
cleanup-acknowledgement and numeric stdout/stderr byte counts. No private bytes,
source/path, argv, environment, account identity or output-derived text enters
the record. Byte counts carry no inferred output class. `TreeRetired` means
local owned cleanup completed, not that Linux descendant cleanup is proved.

The five original lifecycle cases retain their order, timeout values and
assertions, with fixed `PipeDescendant`, `DataTimeout`, `DataCancel`,
`LookupTimeout`, `LookupCancel` labels. Capture, API return, first readiness
probe start, last probe start/return, and cancellation all use the SAME
`started: Instant`. A saturating `u32` probe count includes all completed false
probes as well as the final probe, while the first timestamp is retained even
when it is zero. This prevents an earlier false readiness execution from being
mistaken for no concurrent probe when the account child exits before the final
probe starts. Storage stays fixed-size; no event list or extra probes are added.
The cancel timestamp follows the atomic flag store; the control samples expose
the observed state. Timestamps describe observed boundaries, not exact OS exit
times. Only a failing assertion formats this metadata, including a static
readiness-thread failure if that thread panics. Original failure always remains
failure; no retries, extra WSL probes, delays, fallback or skipped cases.

The next Actions failure can distinguish an unlatched exit before the cancel
request, a late control sample during exit/drain, and an acknowledged versus
unacknowledged stop. Overlap with the readiness command is timing evidence,
not by itself proof of cross-Job process interference.

### Exact New Tests: UNRUN

- `tasks_account_executor::process::tests::capture_diagnostics_are_bounded_payload_free_metadata`
- `tasks_account_executor::process::tests::capture_diagnostics_preserve_results_and_reset_after_panics`
- `tasks_account_executor::tests::private_capture_diagnostics_keep_cancellation_acknowledgement_required`
- `tasks_account_executor::tests::private_capture_unstopped_exit_is_not_relabelled_by_later_cancellation`
- `tasks_account_executor::tests::wsl_readiness_diagnostics_retain_earlier_false_probes_in_fixed_storage`

The first two exercise bounded typed formatting with synthetic private payloads,
late control samples, unchanged results, unwind reset and thread isolation.
The next two execute the existing compiled synthetic fixture through the actual
private capture and ProcessTree path on Windows/Linux. Fixed fixture modes wait
for the real control byte and return a valid cancellation acknowledgement, no
acknowledgement, or cleanup-failed acknowledgement; expected results remain
`Cancelled`, `CleanupFailed`, `CleanupFailed`. A real exit 23 without a stop
remains `HostHelperUnavailable` when cancellation is subsequently requested.
These are guarded child/pipe lifecycle regressions, not source-string tests or
a substitute for actual Linux descendant acceptance.

The fifth test exercises the same readiness recorder used by the actual WSL
canceller: two false probes followed by a ready probe retain count three, first
start zero, final probe times and cancellation time. It also checks counter
saturation, retained first-start data, a later false result and bounded numeric
formatting. It does not execute WSL or claim a passing transport result.

Updated exact ignored test:
`tasks_account_executor::tests::tasks_account_executor_wsl_sentinels_cleanup_and_foreground_host`.
All earlier literal/empty/cwd/account/cancel/descendant assertions and failure-only
discriminators remain. Existing broad Windows Tasks and workspace test filters
cover the five new ordinary tests. No CI or fixture marker/manifest contract
change is required; the existing same-run shim build incorporates its four
additional fixed synthetic modes.

### Not Fixed / Remaining Gates

The actual WSL cancellation failure is NOT claimed fixed. Production stop/check,
exit/cleanup/drain, private pipes and zeroing, suspended/no-window Job attachment,
stdin policy, deadlines/caps, source/epoch and error categories are unchanged.
The sole non-cfg production textual change names the previously ignored control
write result so test-only code can observe its success; production still ignores
it exactly as before. Shared ProcessTree or Python/SSH changes would require a
concrete proposal and Main coordination after evidence.

Current compile/type-check, lint, formatting, ordinary tests and the exact actual
WSL gate remain UNRUN. Next Actions evidence is needed before choosing a safe
production correction. Native hardware/secure-store acceptance remains separate.

## Earlier WSL Empty-Argv Correction Handoff (Superseded)

Historical status below predates 5a070e1. The actual execution evidence and
cancellation diagnostic handoff above supersede its pending-empty-argv status.

The approved correction and focused regressions are authored and source-reviewed;
ownership is released to Main with no implementation blocker. Scope is only
`execution_host.rs`, `tasks_account_executor/process.rs`,
`tasks_account_executor/tests.rs`, and this report. No new public API, SSH/PTY,
Python envelope, credential/process policy, rootfs, CI or spec changes. No local
execution/verification, Git writes or child agents. New verification is UNRUN,
Actions-only; Main owns integration and the exact-SHA rerun.

### Definitive Runner Evidence

Read the authorized log for
[15a1f25 run 34012325033, job 101430284051](https://github.com/vihor3/mini-term/actions/runs/34012325033/job/101430284051).
Exactly one actual WSL test failed in 2.25 seconds. The original
Project/Absolute baseline and only the Empty probe failed with exit -1,
60-byte stdout classified `windows-invalid-parameter`, empty stderr, and no
timeout/truncation. Ascii, AsciiNul, Hostile, Dash, DoubleDash, Assignment,
Wildcard, Newline and WithoutEmpty all exited 0 with `output_matches=true`.
Cleanup succeeded for owned `mt-tasks-34012325033-1`.

This isolates direct empty-argument transport on that runner. It does not prove
a deeper OS implementation cause or behavior across all WSL versions. Main
reports `15a1f25` Linux and Windows jobs fully successful, including 17 Windows
Tasks tests and all four discriminator regressions. Those results precede this
production correction; the actual WSL gate remained failed.

### Agreed Production Boundary

Generic project and pre-project planning continue to share `plan_wsl_command`.
It now uses the existing `posix_quote` for the captured physical cwd and
`serialize_posix_argv(plan.display_argv())` for the original executable/args:

```text
wsl.exe --distribution <distro> --cd / --exec /bin/sh -c <one-nonempty-command>
CDPATH= cd -P <quoted-captured-cwd> && exec <quoted-original-argv>
```

Every Windows launcher argument is nonempty; each empty Linux argument is
represented by `''` inside the command string, not dropped or substituted.
Absolute-cwd/NUL checks, exec-option rejection, failure-before-target, physical
cwd, exact source identity, relative executable and external PATH lookup
semantics remain. No manual PATH search, new Python dependency or fallback.
The narrow module comment now correctly identifies WSL and SSH shell
serialization boundaries rather than claiming SSH is the only one.

Per Main's final refinement, the private Tasks builder does NOT call generic
planning or execution and does NOT clone a `/` snapshot or add an outer captured
cd. It reuses the same envelope encoder as unchanged `ssh_envelope_command`:

```text
wsl.exe --distribution <distro> --cd / --exec /bin/sh -c <one-nonempty-command>
exec <serialize_posix_argv(envelope.display_argv())>
```

Python remains the sole Tasks cwd entry via existing `os.chdir(cwd)` before
any account lookup. Missing cwd therefore retains `Account(CommandFailed)`;
missing Python retains `HostHelperUnavailable` from the existing nonzero-exit
handling. Private `wsl_command` now returns a typed planning error for invalid
distro/program/NUL input. Its leading-dash distro guard agrees with existing
`validate_context`. It carries no credential, only the nonsecret envelope argv.

`run_wsl` still calls private sanitize/capture with the same control, stdin
protocol, private byte pipes, deadline, cap and result decoder. No credential
output passes through generic `execute_host_command`. Same-token proofs,
child-only overrides, Windows suspended/no-window Job attachment, cancellation
and cleanup paths are unchanged. The SSH implementation is untouched.

### Exact Tests: Current Changes UNRUN

New ordinary tests:

- `execution_host::tests::wsl_plans_encode_empty_cardinality_without_empty_windows_arguments`
- `tasks_account_executor::process::tests::wsl_launcher_encodes_empty_fields_and_rejects_invalid_envelopes`

Updated generic project/pre-project, hostile argv, exec-option and marker plan
expectations verify the quoted command instead of raw positional WSL arguments.
Updated `wsl_launcher_preserves_private_envelope_argv_with_root_cwd` covers both
selected-account and three-empty-field envelope shapes, unchanged captured cwd,
the exec-only shell body and eight nonempty launcher arguments. The new tests
check exact single/consecutive/trailing empty encoding and malformed inputs,
not source substrings or a mock claim of actual transport success.

The same exact ignored
`tasks_account_executor::tests::tasks_account_executor_wsl_sentinels_cleanup_and_foreground_host`
now additionally checks Linux `$#` plus exact NUL-separated bytes for one empty
argument and four arguments containing consecutive and trailing empties. Both
project and pre-project APIs execute these checks in the existing hostile owned
cwd, with static `SingleEmptyArgv` / `MultipleEmptyArgv` diagnostics. Checking the
count avoids printf's missing-value/empty-value ambiguity. It uses only the
existing `/bin/sh` and `/usr/bin/printf`; no rootfs/setup change is needed.

All original full empty/hostile/newline cases, Absolute/PATH/Relative programs,
invalid-cwd target sentinels, capability/discovery/selected-account checks,
Rotate cwd marker, missing-cwd/helper error assertions and descendant/cancel
cases remain. All failure-only diagnostics still reject the original failure
regardless of alternative success; no skip, weakened assertion or fallback.
Existing broad Windows and workspace filters cover the new units; no CI edit.

### Remaining Gates

Compile/type-check, lint, formatting, ordinary tests and the actual WSL fixture
for this correction are UNRUN. The empty-argv boundary is now evidenced and
corrected in source, but passing end-to-end execution requires fresh Actions.
Native hardware/secure-store acceptance remains a separate requirement.

## Earlier WSL Literal-Argv Diagnostics Handoff (Superseded)

Historical status below predates `15a1f25`. The executed discriminator results
and production correction above supersede its unresolved-cause status.

The bounded test-only follow-up is authored and source-reviewed; ownership is
released to Main. Current edits are only `tasks_account_executor/tests.rs` and
this review. No production planner/launcher, credential/process policy, fixture
payload removal, rootfs/config, CI/spec, Git writes or child agents. No local
execution or verification. All new tests/diagnostics are UNRUN, Actions-only.

### Actual Failure And Remaining Uncertainty

Read the authorized log for
[2cfd4cf run 34010842674, job 101426406119](https://github.com/vihor3/mini-term/actions/runs/34010842674/job/101426406119).
Exactly one actual WSL test failed in 1.19 seconds at then-current
`tests.rs:1160`: `stage=literal-argv exit=Some(-1)`. Cleanup succeeded for the
owned `mt-tasks-34010842674-1` distro. Source order shows owner/pre-auth executable
resolution/hash/setup and at least one captured-cwd assertion completed first.
The shared assertion identifies neither Project/PreProject nor
Absolute/PATH/Relative, and prints no output class. No program, argument or
Windows error-message cause is inferred from the exit or elapsed time.

Source tracing retains the original argument vector through `CommandPlan::new`,
`plan_wsl_command` and `Command::args`. The fixed shell script uses positional
parameters rather than interpolated data. This establishes the application-side
construction, not what the runner's WSL parser accepted or why it returned -1.
No further production change is justified by this evidence alone. Main reports
`2cfd4cf` Linux/SSH success and Windows through Tasks success; those do not
validate this diagnostic delta or the failing actual WSL gate.

### Fixed Failure-Only Comparisons

Preserved the static mode/program/stage diagnostics for cwd, literal argv and
invalid cwd, including dispatch kind, signed/hex exit, timeout/truncation flags,
byte counts and existing bounded UTF-8/UTF-16 output classes. No raw command,
source path, argv, env, stdout/stderr or exception message is formatted.

The original full argument vector, including empty/newline/hostile data, is now
a private constant used unchanged by all three original program kinds on both
routes. A complete successful baseline with exact stdout starts no probes. A
failed/nonzero/incomplete/mismatched baseline runs these ten fixed rows once:

| Static Row | Fixed `/usr/bin/printf` Input |
| --- | --- |
| Ascii | `%s` with `synthetic-ascii` |
| AsciiNul | Original `%s\0` format with the same ASCII data |
| Hostile | `%s` with the original quotes/substitution-looking argument |
| Dash | `%s` with original `-n` |
| DoubleDash | `%s` with original `--` |
| Empty | `%s` with the original empty argument |
| Assignment | `%s` with original `NAME=value` |
| Wildcard | `%s` with original `*` |
| Newline | `%s` with the original embedded-newline argument |
| WithoutEmpty | Original format and full original list minus only empty |

Every probe uses the same captured owned fixture, route and guarded execution
API as its baseline, with a fixed absolute printf program, five-second timeout
and existing 4096-byte per-stream cap. There are at most ten extra command waits
(50 seconds plus existing bounded cleanup waits), never an open-ended retry.
Dispatch errors are collected as typed kinds so later rows still provide evidence.
An unexpected epoch remains an immediate ownership failure, not a probe retry.

Only static enum row labels, numeric metadata/output classes and boolean
`output_matches` appear. Matching requires success, complete bounded capture
and exact expected bytes. The final result always rejects the original failed
baseline even if all probes match; no probe output becomes acceptance data.
The standalone Empty row cannot prove empty-argument cardinality by output alone
because printf can default a missing value to empty; the unchanged full baseline
and WithoutEmpty comparison remain necessary. These rows are diagnostics, not
replacement product assertions or a passing transport claim.

### Exact New Tests: UNRUN

All are ordinary Windows tests under `tasks_account_executor::tests::`:

- `wsl_cwd_diagnostics_identify_route_program_and_stage_without_raw_output`
- `wsl_literal_discriminator_plans_are_fixed_and_preserve_edge_cases`
- `wsl_literal_discriminators_never_run_on_success_or_adopt_alternatives`
- `wsl_literal_discriminators_reject_incomplete_mismatched_and_dispatch_failures`

These exercise the functions used by the fixture: fixed plans and expected
bytes (including NUL), all mode/program/stage labels, exactly ten probes after
failure, zero after success, original failure despite ten successful outputs,
all typed dispatch errors, truncation/timeout/size/byte mismatch and suppression
of synthetic stdout/stderr/exception secrets. They inspect values and decisions,
not source substrings, and do not execute probes themselves.

The exact ignored WSL test/filter, all original program-kind/argument/invalid-cwd
assertions, account and cleanup cases, owned marker/hash/rootfs contract and CI
setup remain. Main's broad Windows `tasks_account_executor` filter includes the
new units as well as the prior `process::tests` builder unit; no CI edit is needed.
Main owns the exact-SHA Actions rerun. Root cause and actual WSL/account acceptance
remain pending; native hardware/secure-store acceptance is a separate gate.

## Earlier WSL Cwd Correction Handoff (Superseded)

Historical source/UNRUN status below predates `2cfd4cf`. The executed failure and
current diagnostic scope above supersede that status; no further production
change is made by this follow-up.

The authorized production correction and focused regressions are authored and
source-reviewed; ownership is released to Main. Scope is `execution_host.rs`,
`tasks_account_executor/process.rs`, `tasks_account_executor/tests.rs`, and this
review only. No public API, interactive terminal/PTY, Agent, SSH, account receipt,
CI/workflow, spec, fixture launcher or ELF shim changes. No implementation blocker.
All new/modified verification is UNRUN, Actions-only; no local execution or Git
writes were performed.

### Evidenced Boundary

Read the authorized log for
[0e141f0 run 34009374061, job 101422518970](https://github.com/vihor3/mini-term/actions/runs/34009374061/job/101422518970).
Exactly one actual WSL test failed in 0.51 seconds, retaining its original failed
baseline despite successful alternatives. Owned-distro cleanup succeeded.

- All four launcher `Captured` cwd rows: exit -1 / `0xffffffff`, 90 stdout bytes,
  exact `windows-file-not-found` class, `marker_matches=false`.
- All four launcher `Root` cwd rows: exit 0, 164 stdout bytes,
  `marker_matches=true` for the exact typed same-run owner.
- Default versus explicit root user and null versus closed-pipe stdin made no
  difference. Every row used the same guarded runner and `/bin/cat` marker read.
  No timeout, truncation or epoch occurred; stderr was empty.

This isolates the observed failure to launcher-level cwd handling, before
credentials/Python/gh. It is not evidence for changing stdin, Job guards or
authentication. The underlying WSL implementation detail is not inferred from
the message alone. Main reports the `0e141f0` Linux workspace, five actual SSH
fixtures, sidecars, whitespace and Windows gates succeeded, with package
`ci.47` successful; only the original actual WSL baseline failed. These results
precede this correction and are not a pass claim for its new source.

### Correction And Shell Review

Project and pre-project WSL branches now share private
`plan_wsl_command(distro, cwd, plan)`. The unchanged public planners validate
command argv; the helper validates distro and absolute, NUL-free cwd without
normalizing or rewriting the captured source identity. Its fixed launch is:

```text
wsl.exe --distribution <distro> --cd / --exec /bin/sh -c <static-script> mini-term-wsl <captured-cwd> <program> <args...>
```

The complete static script is:

```sh
CDPATH= cd -P "$1" && shift && exec "$@"
```

Directory, program and arguments are separate argv, never shell source. Empty
CDPATH prevents directory-search/output behavior; absolute-only cwd validation
prevents `cd` option/default-directory interpretation. Physical `cd -P` occurs
before `shift` and `exec`, and failure cannot dispatch the target in `/` or any
other directory. Bare executable names use the Linux shell's external PATH
lookup; relative executable paths are resolved after the directory change.
No eval, manual PATH search or command-name rewrite was added. Empty arguments,
quotes, substitutions, wildcard/assignment-looking data and newlines stay data.

Quoted argv does not prevent the `exec` builtin from interpreting a leading `-`
program as an option. The WSL-only helper therefore explicitly rejects such a
program with a static `Rejected` error. A literal filename can still be requested
as `./-name` or an absolute path; leading-dash arguments remain valid. This is
intentional fail-closed handling, not a change to shared `validate_argv`, Local
or SSH planning, or the existing fixed executable call sites.

Tasks uses private `wsl_command` with the fixed launcher cwd `/`, retaining its
original envelope argv. `host_envelope.py` already calls `os.chdir(cwd)` before
any gh discovery or credential lookup, so no shell wrapper or Python change is
needed there. Sanitization, private pipes, same-token proofs, cancellation,
deadlines, read bounds, suspended/no-window Job guards and cleanup are unchanged.

The diagnostic helper now follows corrected production planning. Its `Captured`
and `Root` labels select the Linux-side cwd; both launchers use `--cd /`.
Baseline plan expectations were updated, but failed-baseline rejection,
seven-alternate-only-on-failure behavior and static secret-safe output remain.

### Focused Tests And Fixture Needs

New ordinary tests, all authored UNRUN:

- `execution_host::tests::wsl_project_and_preproject_plans_keep_cwd_and_relative_argv_literal`
- `execution_host::tests::wsl_project_and_preproject_reject_invalid_launch_context`
- `execution_host::tests::wsl_rejects_exec_options_without_rejecting_literal_paths_or_arguments`
- `tasks_account_executor::process::tests::wsl_launcher_preserves_private_envelope_argv_with_root_cwd`

Existing project/pre-project and eight-row marker planner expectations are
updated. The new process test inspects the actual private command builder and
is platform-independent; the workspace gate includes it. Main confirmed the
actual Windows CI filter is broad `tasks_account_executor`, so it includes this
process test too; no CI edit is needed.
No source-string-only regression was added.

The same exact ignored
`tasks_account_executor::tests::tasks_account_executor_wsl_sentinels_cleanup_and_foreground_host`
now calls `WslFixture::assert_cwd_routing` after owner/shim/hash validation. It
executes both public project and pre-project APIs inside the same owned distro:
exact cwd with spaces, quotes, shell-looking data and newline; absolute/PATH
lookup plus a hostile `./-relative...` executable; exact NUL-separated argument
bytes; and missing/non-directory cwd where the absolute target marker must never
be created. It then proves the private account envelope writes `data-seen` in
that hostile cwd and returns `CommandFailed` for a missing cwd. Existing selected
account, secret rejection and descendant/cancel tests remain, and the test must
still discover/run exactly once with no skip/fallback.

The same rootfs additionally needs `/bin/pwd`, `/bin/ln` and `/usr/bin/printf`
(coreutils) for these assertions. All temporary paths/symlinks stay beneath the
owned `/mini-term-fixture/cases/<UUID>`; no distro setup/config change or new
fixture framework is requested. The three env vars, owner schema, ELF hash,
wrapper, exact ignored filter and guarded distro cleanup remain unchanged.

### Remaining Gates

The cwd correction has not been executed or formatted. Main owns exact-SHA
Linux/Windows compile, lint, ordinary tests and the actual WSL rerun. A passing
matrix/root marker from `0e141f0` does not validate the new shell or account path.
Native hardware/secure-store acceptance remains separate from synthetic fixture
coverage. No additional production issue is claimed in this bounded source pass.

## Earlier WSL Matrix Handoff (Superseded)

Historical status below describes the diagnostic slice before `0e141f0`; the
evidence, production correction and pending validation above supersede it.

The approved diagnostic slice and regressions are authored and source-reviewed;
ownership is released to Main. No implementation blocker remains. Scope is only
`execution_host.rs` private runner factoring and Windows-test-only marker helper,
`tasks_account_executor/tests.rs`, and this review. No launcher/ELF shim, Agent,
Tasks service/view, account executor, ProcessTree policy, CI script/workflow or
spec changed. No local verification, staging, commit, push or child agent.
All current code/tests are UNRUN, pending Main's exact-SHA Actions gates.

### Actual Actions Evidence

Latest authorized log inspected:
[run 34007314719, job 101416918481](https://github.com/vihor3/mini-term/actions/runs/34007314719/job/101416918481),
which Main identifies as `0b28475`. Exactly one actual WSL test ran and failed
in 0.10 seconds: `ReadOwner`, exit `Some(-1)` / `0xffffffff`, no timeout or
truncation, stdout 90 bytes / `unclassified`, stderr 0 bytes / `empty`.
Owned-distro cleanup succeeded. This identifies the first marker read, before
Python/gh/account APIs, but not a Windows message or root cause. No inference
is made from the 90-byte length.

The earlier inspected
[run 34005933974, job 101413098858](https://github.com/vihor3/mini-term/actions/runs/34005933974/job/101413098858)
ran exactly one test and failed in 0.06 seconds without stage/status attribution.
Main now reports the complete `aec1c72` Windows CI and package jobs succeeded.
Those results do not validate this diagnostic slice or establish passing WSL
execution/native credential acceptance for the current source.

### Authored Helper And Invariants

Approved API, only under `cfg(all(test, windows))`:

```rust
pub(crate) fn tasks_wsl_marker_probe(
    snapshot: &ProjectExecutionSnapshot,
    cwd: TasksWslProbeCwd,     // Captured | Root
    user: TasksWslProbeUser,   // Default | Root
    stdin: TasksWslProbeStdin, // Null | ClosedPipe
) -> Result<HostCommandResult, CommandExecutionError>;
```

- The private planner requires `GITHUB_ACTIONS=true`, numeric nonempty run ID
  and attempt, exact `mt-tasks-<run>-<attempt>` in both env and WSL backend,
  fixed marker env path, and both snapshot paths exactly `/mini-term-fixture`.
  Invalid/missing inputs reject before spawn with a static error. No arbitrary
  executable, argv, env map, working path, timeout or output-cap input.
- The production WSL planner builds only `/bin/cat` with the fixed
  `/mini-term-fixture/owner.json` argument. Root cwd changes a clone to `/`;
  explicit root adds structured `--user root`. No Windows current-dir change,
  executable override, Job bypass, breakaway or special launcher.
- `run_process` keeps its signature and unconditionally delegates with
  `ProcessStdin::Null`. Its privately factored body still selects exactly
  `Stdio::null()` in production. The only alternative is Windows-test
  `ClosedPipe`, whose writer drops immediately after successful attach.
- Source diff preserves all original configure/spawn/attach/resume/error,
  suspended/no-window flags, kill-on-close Job policy, reader, deadline,
  termination and cleanup paths. Each probe uses the same guarded body,
  five-second command limit and 4096-byte per-stream cap. Seven alternatives
  add at most 35 seconds of command waits plus existing bounded cleanup waits.

### Failure-Only Matrix

`WslFixture::open` still obtains the first marker read through ordinary
`execute_host_command`. Only its failed/incomplete/mismatched `ReadOwner` result
runs the seven alternatives, before helper/shim checks or account APIs.

| Cwd / User | Null Stdin | Closed-Pipe Stdin |
| --- | --- | --- |
| Captured / Default | Reuse original baseline; never rerun | Probe |
| Root / Default | Probe | Probe |
| Captured / Root | Probe | Probe |
| Root / Root | Probe | Probe |

The original failure always rejects the fixture, even if every alternative
succeeds. A valid baseline starts no diagnostic probes; later stages cannot
trigger this matrix. Each row reports only static enum/error/output classes,
numeric exit/length metadata, timeout/truncation/epoch flags and boolean
`marker_matches`. That boolean requires complete successful bounded output,
no connection epoch, and exact typed owner JSON with unknown fields rejected.
No raw output, marker, argv, env, exception message, account or credential is
formatted. The pre-auth env guard also no longer prints rejected values.

The retained classifier delta recognizes exact canonical Windows file/path
not-found and invalid-parameter messages in UTF-8/UTF-16LE with optional BOM
and CRLF. Decorated messages and unrelated 90-byte payloads remain unclassified.
The existing five pre-auth stage labels and fixed bounded output classes remain.

### Exact Tests: Current Patch UNRUN

New ordinary Windows tests under Main's `execution_host::` filter:

- `execution_host::tests::tasks_wsl_marker_probe_plans_are_fixed_and_match_production_baseline`
- `execution_host::tests::tasks_wsl_marker_probe_rejects_unowned_launch_inputs`

They exercise the actual helper planner/guard, all eight combinations, exact
argv and stdin policy, unchanged source identity, and rejection of foreign or
missing env/run/attempt/backend/marker/snapshot paths, without WSL or global env
mutation. No source-string tests.

New ordinary Windows tests under `tasks_account_executor::tests::`:

- `wsl_prelude_system_messages_require_exact_text_not_length` (retained delta)
- `wsl_marker_matrix_reuses_failed_baseline_without_success_fallback`
- `wsl_marker_matrix_does_not_probe_after_valid_owner`
- `wsl_marker_matrix_rejects_unowned_incomplete_and_dispatch_failures`

The matrix regressions exercise the same decision/diagnostic functions as the
actual fixture: exactly seven distinct probes and eight rows, no alternate
success fallback, no success-path probes, wrong/missing/extra/ill-typed owner
fields, incomplete/oversized output, unexpected epoch, every typed dispatch
error and synthetic-secret suppression. The earlier ordinary test
`wsl_prelude_diagnostics_are_bounded_stage_specific_and_secret_safe` remains.

The actual ignored test/filter and rootfs/env/marker/hash requirements below
are unchanged and still must discover/run once. No setup change is needed.
Main has added/released the Windows `execution_host::` filter, retaining native
lifecycle tests; the Linux workspace gate retains generic cleanup regressions.
All current tests, build/type-check, lint, formatting and probes remain UNRUN.

### Historical Limit Before 0e141f0

Before the matrix ran, the source already supplied `--cd /mini-term-fixture`;
simply adding another cwd argument was not justified. Node's successful marker
guard used explicit root and `/`, with a
different stdin/spawn path. Node/libuv can itself use no-window and Job guards;
it is not an unguarded control. Previously inspected primary sources:
[Node 22 Windows process spawning](https://github.com/nodejs/node/blob/v22.x/deps/uv/src/win/process.c)
and [synchronous stdin shutdown](https://github.com/nodejs/node/blob/v22.x/src/spawn_sync.cc).
The matrix varied only the approved parameters while keeping production guards.
That diagnostic slice made no production behavior fix pending Actions evidence.
The `0e141f0` result and current correction above resolve this former source
decision blocker; successful execution of the correction is still required.
Native hardware/secure-store acceptance remains separate from these fixtures.

Verification for this patch: lint, type-check/build, unit/WSL tests, formatter
and probes are all UNRUN, Actions-only. Only source/config/docs, read-only Git
diff/status and the explicitly authorized Actions log were read locally.

## Approved Follow-Up Complete: Source Handoff

Main approved the foreground-cache fix and narrowly extended editing to
`workbench_area.rs` WorkItem open/reopen/activation hooks only. Ownership is
released to Main with this source handoff. All new verification remains UNRUN.

API boundary recorded here before implementation, now authored:

- Added `GitHubWorkItemViewer::on_activated(&mut self, &mut Context<Self>)` for
  existing WorkItem opens/tab activation. It starts a new bounded detail access
  without replacing the viewer/tab or resetting scroll. Only WorkItem hook
  calls change in `workbench_area.rs`; no navigation/document/terminal refactor.
- Tasks foreground events start a new monotonic access owner. Passive
  render/service/store notifications must not turn cache presence into another
  request. Detail accesses retain their own monotonic request receipt in the
  viewer so an old completion or another cached access cannot revive one.
- Reuse the existing bounded origin/account read pipeline, last-known cache,
  cancellation, and source/auth-generation gates. No global-active fence, timer,
  arbitrary execution-host API, or shared-fetch engine is added.
- `GitHubTaskService::ensure_detail` is replaced with private `access_detail`,
  which returns the viewer's request receipt. Known-identity lists use a
  separate `list_access_id` to fence row/menu callbacks without cancelling
  independent detail accesses solely because Tasks was shown or its mode
  accessed. Actual source/auth invalidation still cancels both.
- Only two WorkItem hook blocks changed in `workbench_area.rs`: existing-item
  open and tab/page activation. New viewers access in their constructor;
  existing worktree/page reactivation already routes through the updated hook.
  No tab replacement or scroll reset is added for existing-item activation.

### WSL Fixture Contract For Main

The actual Windows ignored test and launcher were authored for Main's Actions
integration. The resumed execution evidence is recorded above. Read-only inspection of Main's setup script
and `tasks-wsl-rootfs`/`tasks-wsl` jobs found their marker, paths, environment and
exact filter aligned with this contract. CI reviewer McClintock owns any setup
or workflow fixes; this reviewer made none.

Source paths:

- `crates/mt-app/src/tasks_account_executor/wsl_fixture_launcher.sh`
- `crates/mt-app/src/tasks_account_executor/tests.rs`:
  `tasks_account_executor_wsl_sentinels_cleanup_and_foreground_host`

Required environment:

```text
MT_TEST_WSL_DISTRO=mt-tasks-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}
MT_TEST_WSL_MARKER=/mini-term-fixture/owner.json
MT_TEST_WSL_GH_SHA256=<lowercase SHA-256 of /usr/local/bin/gh in the same-run rootfs>
```

Agreed with main's rootfs proposal: require `GITHUB_ACTIONS=true`, nonempty
numeric `GITHUB_RUN_ID` and `GITHUB_RUN_ATTEMPT`, `GITHUB_REPOSITORY`, and a full
`GITHUB_SHA`. The marker must have exactly these fields (schema is numeric;
other fields are strings), filled with the job's matching values:

```json
{
  "schema": 1,
  "kind": "mini-term-tasks-wsl",
  "run_id": "<GITHUB_RUN_ID>",
  "run_attempt": "<GITHUB_RUN_ATTEMPT>",
  "repository": "<GITHUB_REPOSITORY>",
  "sha": "<GITHUB_SHA>"
}
```

Rootfs requirements: Python 3.8+ at `/usr/bin/python3`; the Actions-compiled
Linux ELF from `tasks_account_executor/gh_fixture.rs` at `/usr/local/bin/gh`;
`/bin/sh`, `/bin/cat`, `/bin/mkdir`, `/usr/bin/test`, `/usr/bin/touch`, and
`/usr/bin/sha256sum` (plus `/usr/bin/head` for Main's guard); writable empty `/mini-term-fixture/cases` and
`/mini-term-fixture/home`; no real gh/credentials. Put `/usr/local/bin` ahead
of `/usr/bin:/bin`. The separate same-run artifact manifest supplies the ELF
hash for `MT_TEST_WSL_GH_SHA256`; the owner marker need not duplicate that hash.
Use the now-authored `tasks_account_executor/wsl_fixture_launcher.sh`
as executable `/usr/local/bin/python3`; it only seeds synthetic host environment
and controlled missing-helper/malformed-reply test cases before execing the
real Python. No Windows fixture compiler is needed for this ignored test.

The launcher preserves `python3 -I -c SCRIPT CWD ...` argv and execs
`/usr/bin/python3`. The fourth argument must be `/mini-term-fixture/cases/*`.
It seeds synthetic auth/debug/host/repo, TTY/shell-startup/WSLENV overrides and
isolated HOME/config/PATH. Per-case control files are `missing-helper`,
`malformed-reply`, and `malformed-enumeration`; missing-helper attempts an absent
fixture-only executable without removing any installed Python.

The test first validates the marker (bounded read), exact `command -v gh` and
`command -v python3` paths, and the ELF checksum through the selected distro,
before calling any account API. It neither imports/unregisters distros nor
changes a default. Per-case paths are `/mini-term-fixture/cases/<UUID>`.

Exact filter, now backed by authored source:

```text
cargo test --locked --target x86_64-pc-windows-msvc -p mt-app --bin mini-term tasks_account_executor::tests::tasks_account_executor_wsl_sentinels_cleanup_and_foreground_host -- --ignored --exact --test-threads=1
```

Main owns the checksum-pinned same-run rootfs artifact, dedicated Windows 2022
Actions job, unique `wsl.exe --import ... --version 1`, explicit runner
capability failure and guarded per-distro cleanup. The test must be discovered
and run exactly once; absence is not a passing gate. No local WSL probe or
fixture execution is authorized or performed.

## Findings (fixed)

### P1: Extracted Detail Method Was Inaccessible To The Workbench

- Files: `github_tasks/service.rs`, `github_tasks.rs`, `workbench_area.rs`.
- Issue: extraction into private `github_tasks::service` made `ensure_detail`
  `pub(super)`, which permits only the `github_tasks` subtree. The unchanged
  workbench sibling still calls it, producing a private-method compile error.
  The former method in `github_tasks.rs` was public.
- Initial review restored `pub(crate)` visibility. The approved follow-up
  supersedes that temporary API with the viewer activation hook above and
  private `access_detail`. Actual sibling call sites now use the intended
  boundary; compilation remains UNRUN, not claimed passing.

### Executor Regression Coverage

- File: `crates/mt-app/src/tasks_account_executor/gh_fixture.rs` and `tests.rs`.
- Issue: `Leak` emitted the sentinel on both stdout and stderr, so removing
  stderr filtering would not fail that case. `Wrong` failed the initial proof,
  not the post-data proof. Alice/Bob both returned `[]`, so the data assertions
  did not independently establish which credential the data child received.
  No executable fixture changed named lookup between data and the final proof.
- Fix: added `LeakStderr`, `WrongAfter`, and `Rotate` fixture modes. Data now
  contains the authenticated login in parsed list/detail fields. `Rotate`
  changes the synthetic named lookup after data while the existing credential
  continues to prove the selected identity. Per-request fixture directories
  keep the phase marker isolated. Native, POSIX-envelope and shared loopback
  SSH cases exercise both list and detail, assert account-specific data, reject
  post-read mismatch/stderr-only echo, and retain the same captured credential.
- This fixture coverage does not modify a production API. Follow-up production
  changes are limited to Tasks cache/service/view ownership and the approved
  WorkItem hooks. No shared SSH/framework, execution-host, store, navigation,
  locale, workflow, or spec edit was made.
- Tests: one new test function,
  `native_post_read_proof_keeps_the_original_credential_after_lookup_changes`,
  plus stronger assertions/cases in the existing executor tests. The follow-up
  adds the actual ignored WSL test above. All UNRUN.

The pre-existing Windows pre-assignment case calls the production OwnedChild
cleanup with a genuinely suspended child and asserts it is reaped without
running its marker writer. Native/POSIX descendant cases wait for a real ready
marker before cancellation, then release the descendant and check for an orphan
marker. SSH assertions inspect actual channel stdout and stderr. These are
executable tests, not source-substring assertions, but execution remains pending.

### P2: Cached Foreground Access Bypassed Revalidation

- Files: `github_tasks.rs`, `github_tasks/model.rs`, `github_tasks/service.rs`,
  `github_tasks/tests.rs`, plus the approved two WorkItem hooks.
- Issue: Ready list and populated detail slots bypassed `read_with`, allowing
  cached reopen after external logout/origin replacement without a new proof.
  Removing the guards alone would let passive service observers refetch forever.
- Fix: Tasks hidden-to-visible, actual active-source changes, Issue/PR mode
  access, account choice and Retry explicitly arm one monotonic access.
  New/reopened/activated work items acquire their own request receipt. The
  existing bounded origin/enumeration/selected pre-data-post proof pipeline
  runs even when data is cached. Passive render/service/store notifications do
  not rearm the consumed access or dispatch another settled request.
- Cached list rows remain noninteractive while loading/error; cached detail is
  inert and visibly refreshing/last-known until its own receipt completes.
  Legitimate transient failures retain only same-identity last-known data;
  logout, replaced origin and nonretained errors cannot revive it. Scope/auth/
  source gates and request/slot ownership still run before publication.
- A completion edge case found during follow-up was also fixed: an older list/
  source error no longer invalidates a later freshly proved detail. Only the
  current completion's nontransient error can retire its new authority.
  Detail NotFound does not invalidate unrelated list readiness.
- Known-identity list access cancels older list/preparation requests, not an
  independent detail receipt. Real source/auth invalidation still cancels all
  affected work. Completed-cache reuse and loading-slot cancel/restart remain;
  no in-flight sharing subsystem or broad polling was added.

Seven new follow-up app test functions, all UNRUN:

1. `foreground_list_access_is_consumed_once_and_notifications_cannot_rearm_it`
2. `cached_list_foreground_rechecks_logout_and_origin_without_passive_retries`
3. `new_list_access_cancels_only_list_and_preparation_owners_not_detail_accesses`
4. `cached_detail_reopen_rejects_logout_through_pipeline_and_scope_invalidation`
5. `cached_detail_reopen_revalidates_origin_and_preserves_only_transient_last_known_data`
6. `revalidated_detail_does_not_inherit_an_earlier_source_failure`
7. `detail_access_receipts_reject_reopen_aba_and_never_borrow_another_access_result`

These use the actual pipeline and production service/model/view helpers for
cache mutation, invalidation and receipt projection, with a scripted host. They
are not source-substring checks or a full GPUI/physical-host acceptance claim.

## Spec Coordination

### In-Flight Sharing Finding Corrected By Main

- Files: `github_tasks/service.rs` and the main-owned Tasks contract.
- Evidence: `start_read` cancels a previous loading slot and starts another
  request. A newly prepared sibling runs its own full list pipeline. The latest
  source-read spec now describes independently proved sources, completed-cache
  reuse, and possible cancel/restart; this matches the implementation. The old
  single-fetch example is no longer a remaining finding. Main owns any further
  foreground-event/receipt documentation from this handoff.

## Findings (not fixed)

### Actual WSL Transport Execution Gate Is Still Open

- The missing source fixture is now authored: it calls exported capability,
  discovery and selected-account APIs through production `run_wsl`/`wsl.exe`.
  It covers github.com/ghe.com/GHES Alice/Bob/Rotate list/detail, broken/store/
  wrong pre- and post-proof, stdout/stderr token rejection, malformed reply/auth
  JSON, missing helper, and bounded oversized output.
- Actual distro ready/release/orphan markers exercise descendant cleanup on
  successful inherited pipes, deadlines, data cancellation and credential lookup
  cancellation/deadlines. Cancellation waits for the real ready marker. Test-only
  cooperative-capture assertions inspect actual transport stdout and stderr for
  the synthetic token on completed and stopped requests.
- Remaining limit: the actual Actions fixture now ran once and failed in its
  prelude with an unattributed nonzero exit; see the resumed evidence above.
  WSL stdin/control/cancel/deadline behavior and Linux descendant retirement
  still need a passing exact-SHA Actions result. Missing capability
  must explicitly fail, never silently skip or pass with zero discovered tests.
- Main owns setup/workflow and CI reviewer McClintock owns their bounded fixes.
  No local WSL probe, execution, or distro mutation was performed.

## Source Audit

Read the assigned PRD/design/implementation plan/check.jsonl, all three
handoffs, account-isolation research, before-dev/check skills, package guidelines,
worktree context and main's newly updated Tasks contract. Audited the full
account domain and authored tests, native/Python/SSH executor and fixtures,
Tasks model/pipeline/service/UI/tests, config field/default/DTO/export/tests,
and read-only surrounding source/ProcessTree/workbench integration.

Observed implementation properties, not runtime claims:

- Domain enumeration uses the official host-map shape without active/show-token
  flags. Typed projection retains broken peers with valid identities; it rejects
  invalid/duplicate accounts, inherited-token rows and incomplete captures.
  Lookup preserves discovered spelling while comparisons/persistence normalize.
- Native named lookup stays in private non-Debug bounded pipes. WSL/SSH lookup,
  data, and both proofs stay inside the host's isolated Python envelope. Only
  child auth environments are overridden. No auth switch/login/config write,
  token-bearing argv or process-global environment mutation was found.
- Exported discovery returns `KnownGitHubAccounts`, not raw auth JSON. Both
  native and host-envelope discovery discard extra token fields and raw error
  strings before returning. Credential lookup failures are static categories.
  Successful selected data is screened for the exact captured token on both
  stdout and stderr. OS/process/Python-managed copies are not locked memory.
- Capability/enumeration/selected calls carry observed epochs on their result
  envelope. Authentication/protocol replies pass through the SSH current-session
  check before decoding; error ownership is also checked by the app pipeline.
  Cancellation/timeout retain their originating epoch and explicit cleanup
  failure semantics. Source/publication checks do not infer a replacement epoch.
- Stored missing/broken/invalid/duplicate choices do not fall back. The key
  includes root project, execution host, stable backend and GitHub hostname,
  while cache/presentation also retain worktree/source and runtime authority.
  Both directions of global-active synchronization are absent from Tasks.
- Dispatched reads check origin before/after success and failure, selected
  identity and request/auth generations. Scope rotation cancels owned requests,
  removes old identity-dependent caches and invalidates detail readiness. The
  ordinary origin read has the approved bounded/no-physical-cancel limitation.
- Account selection is inside Tasks for both modes. Login UI is inert Copy/Retry;
  detail bodies still pass through the existing inert Markdown sanitizer.
  The config defaults to an empty list and persists identity fields only.

Official references re-read, without running gh or looking up credentials:
[status schema](https://raw.githubusercontent.com/cli/cli/trunk/pkg/cmd/auth/status/status.go),
[named lookup](https://raw.githubusercontent.com/cli/cli/trunk/pkg/cmd/auth/token/token.go),
[config source](https://raw.githubusercontent.com/cli/cli/trunk/internal/config/config.go),
[host auth environment](https://cli.github.com/manual/gh_help_environment).
These confirm the documented approach, not any target device's installed
capabilities or credential-store accessibility.

## Verification

The counts and UNRUN entries below record the earlier source handoff. The WSL
resume above supersedes its old execution status and adds one Windows ordinary
unit test (executor source count 13; Windows ordinary count eight). No current
diagnostic-patch pass is claimed.

- Lint: UNRUN. Actions-only; no local lint, syntax or whitespace check.
- TypeCheck/build: UNRUN. No local Cargo command or metadata probe.
- Tests: UNRUN. Domain 20, app 24, config 2 and executor 12 authored functions
  across platforms after this review. Counts describe source, not passes.
- Executor platform counts: Windows has seven nonignored cases plus the actual
  ignored WSL case; Linux has nine nonignored cases plus the exact ignored
  shared-loopback SSH case. Ordinary runs do not execute either ignored gate.
- Formatting/codegen/app verification: UNRUN. Main must include complete new
  files when applying Actions formatting output, including `gh_fixture.rs`,
  which is compiled as a standalone fixture and is not a crate module.
- Actions evidence for the reviewed product SHA: none supplied/observed here.
  Main owns staging, workflows, commits and run/artifact recording.

Required existing Actions test commands (not run by this reviewer):

```text
cargo test --locked -p mt-github
cargo test --locked -p mt-app --bin mini-term github_tasks
cargo test --locked -p mt-app --bin mini-term tasks_account_executor
cargo test --locked -p mt-config tasks_account_choices
cargo test --locked --target x86_64-pc-windows-msvc -p mt-app --bin mini-term tasks_account_executor
cargo test --locked -p mt-app --bin mini-term remote_ssh::tasks_accounts::tasks_account_executor_ssh_sentinels_cleanup_and_epoch_pipeline -- --ignored --exact --test-threads=1
```

The SSH case still uses Turing's shared Actions fixture guard, isolated HOME/key,
verified synthetic gh PATH and inherited synthetic auth/debug values. No second
SSH setup framework was introduced. Keep the existing five-minute case timeout;
this review adds short list/detail scenarios, not additional long-wait cases.

## WSL Actions Setup Ownership

The current authored test/launcher contract is at the top of this review and
supersedes the earlier prospective WSL setup. Main now owns the same-run,
checksum-pinned Linux rootfs/ELF artifact and explicit unique Windows 2022
`--import --version 1` job. Default-root, disabled interop/automount, isolated
HOME/config/PATH and only-owned always-cleanup are Main's setup responsibility.
The marker has no distro/hash fields; provenance/hashes belong to the separate
artifact manifest. Neither a default distro change nor global WSL shutdown is
needed. CI reviewer McClintock owns exact-test discovery, timeout headroom and
guarded cleanup fixes. No CI file was edited here.

No source fixture gap remains, but successful Actions execution is still
required before claiming the transport gate passes. Command forms were checked
against Microsoft documentation:
[WSL commands](https://learn.microsoft.com/en-us/windows/wsl/basic-commands),
[custom distro import](https://learn.microsoft.com/en-us/windows/wsl/use-custom-distro).
These references do not prove runner capability or a successful fixture run.

## Native Acceptance Still Open

Use only an exact-SHA Actions-produced artifact for real Native/WSL/SSH UI
acceptance: two actual configured accounts, independent project choices after
restart, global-active changes in both directions, revocation/logout, actual
noninteractive credential-store access, reconnect, foreground cached reopen and narrow/
high-DPI toolbar/menu sizing. Synthetic fixtures cannot establish secure-store
compatibility or actual desktop rendering. No app was launched by this reviewer.

## Conditional WSL Root Containment Review

Read-only source proposal following Main's 35fe653 evidence. Only this addendum
was authored; no source/spec/CI/Git changes, local verification or child agents.
This is conditional on Socrates' causal A/B establishing the per-command Job
retirement mechanism. It does not authorize applying a breakaway flag now.

### Evidence Boundary

Main reports CI 34026742549 ordinary Linux/Windows FULL PASS, including all eight
public-comparison tests, while actual WSL job 101469109594 failed LookupCancel.
The private request exited before cancellation; the separately owned public
producer emitted its start but not end marker and reported instance termination
while its own readiness probe succeeded. Owned distro cleanup passed. This
establishes a public-command reproduction, not the driver/Job causal mechanism.
Private buffers remain unlogged. Main's upstream parent-process/instance-handle
findings are context, not proof of this runner's inbox WSL1 implementation.

### Smallest Conditional Boundary

Keep public command plans and snapshot identities unchanged. Select a small
crate-private StrictTree versus WslClientRoot policy while backend identity is
still available, before the plan becomes an arbitrary Command:

| Integration point | Required selection |
| --- | --- |
| execution_host.rs:343, execute_pre_project_local_command | Match PreProjectLocalContext::Wsl; Native stays strict |
| execution_host.rs:383, execute_host_command | Match snapshot.backend Wsl in the Process arm; Local stays strict and SSH stays separate |
| tasks_account_executor/process.rs:644, run_wsl | Pass explicit WSL policy into private capture; native lookup/proof/data captures stay strict |

Thread that policy only through the private process runner/capture to the Job
constructor. Keep ProcessTree::configure's default strict for existing callers;
factor its flag setup narrowly if needed. Do not infer WSL from executable text,
cwd, a cooperative-stdin boolean, or a global flag. No new public API, dependency,
WSL version probe, generic environment channel or planner representation is
needed. Native, SSH and interactive PTY paths must remain unchanged.

For WSL only, the candidate flags are KILL_ON_JOB_CLOSE together with
SILENT_BREAKAWAY_OK. Microsoft's
[Job limit contract](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information)
allows children to escape this Job without changing the explicitly assigned
root's membership. The flag does not selectively exempt only shared WSL
infrastructure. Nested ancestor Jobs can still constrain breakaway. Preserve
suspended/no-window creation, exact root assignment before resume, assignment
and resume failure cleanup, explicit existing termination, direct-child bounded
reaping and final handle closure. Add no CREATE_BREAKAWAY_FROM_JOB launch flag,
Job retention, retry, or fallback when assignment/configuration fails.

### Guarantees And Guest Helper Decision

Current ordinary execution (execution_host.rs:1277 onward) bounds waiting and
output, kills current Windows Job members on timeout/error and also retires the
Job after normal completion. It has no guest process-group/control channel or
guest cleanup acknowledgement. This is true for both WSL versions: source has
no WSL1/WSL2 branch. A Windows Job membership/count/result cannot prove Linux
descendant cleanup in either version.

The candidate retains root cleanup but deliberately gives up automatic Windows
descendant containment on WSL calls. If A/B confirms the hypothesis, the gain is
survival of shared instance infrastructure and unrelated concurrent commands.
The cost is that escaped relays, interop processes or guest work may outlive the
client, including on timeout/crash; inherited output handles can still require
the existing bounded drain error. Do not report such errors as clean success.
WSL2 compatibility is a separate required acceptance case, not implied by WSL1
success or by the absence of a version branch.

Tasks already has the necessary guest cleanup mechanism: host_envelope.py:66
kills the spawned gh process group and waits for its direct child; :79 creates
a new session, monitors control/EOF and owns cleanup in finally. Private capture
still requires the strict object-shaped host acknowledgement after a latched
stop (process.rs:902). Keep Cancelled/TimedOut versus CleanupFailed distinct and
all secret, source, deadline, pre/post-proof and descendant checks unchanged.
This does not establish containment of deliberately escaped daemon/interop
descendants outside that guest group.

A new ordinary guest helper is not inherently needed for this containment fix.
Git already treats timeout, absent exit and executor errors as uncertain
(git_backend/host.rs:193); write.rs:292 retains the original lease even when
reconciliation succeeds, and exact-ID review requires user confirmation that
the original operation stopped. Preserve those contracts; bounded caller return
is not bounded guest lifetime. A helper would be needed to promise the stronger
new guarantee of positive guest-stop acknowledgement for ordinary commands,
which is not established by the current runner and need not be invented here.

One concrete qualification needs Main's attention before relying on that safety
argument: host.rs:210 currently classifies every present non-timeout exit code
as Completed, including WSL launcher -1. write.rs can release the lease after
successful reconciliation of such a failed command. A lost launcher does not
prove guest quiescence. The conditional implementation should preserve or
establish uncertainty for proven launcher-loss outcomes rather than treating
them as ordinary Git exit status. This is a narrow result-classification review
point, not a request for a new guest supervisor or a fix authored here.

### Required Gates And Residual Risks

- Actions A/B must establish the proposed Job-retirement difference on the same
  owned fixture; the public instance-terminated message alone is insufficient.
- Test both ordinary project and pre-project WSL routing and private capture;
  retiring/timeout of one client must preserve a concurrent peer, with no probe
  serialization or changed original cadence. Keep all original actual cases.
- Run private cancellation and timeout during lookup/data, pipe descendants,
  invalid/missing acknowledgement and early launcher failure. A valid stop must
  still have host acknowledgement and descendant evidence, not merely root exit.
- Pin exact strict/native versus WSL flags and suspended attachment ordering;
  native descendant retirement, pre-assignment cleanup, attach/resume failure
  and root cleanup/result preservation must remain effective.
- Retain Git uncertain-lease/no-retry/exact-ID-review tests, adding focused
  launcher-loss coverage if that classification is corrected. No clean mutation
  or guest-stop claim may follow a transport-only exit.

The disposable fixture disables interop and tests one inbox WSL1 environment.
Installed WSL variants, WSL2, nested launcher Jobs, enabled Windows interop and
shared distros used by external clients remain compatibility/acceptance risks.
Application restart also ends the current process-local Git lease; no durable
recovery promise is introduced. No global distro shutdown/configuration change,
UI expansion or extra diagnostic framework is proposed. Main owns the causal
decision, any resulting source authorization, contract changes and Actions.

## Newton Timing Contrast: Independent Read-Only WIP Check

Read the selected Public WSL Retirement Timing Contrast release contract and
finalized Git-child research/wsl-instance-retirement.md, then the current
execution_host.rs and Tasks tests.rs WIP. Only this report changed. Newton keeps
all diagnostic source ownership; no fixes, local verification, Git operations,
children or duplicated upstream/CI investigation occurred. This is not a final
source release or an Actions pass. The selected experiment is the on-stack
pre-retirement timing contrast, NOT a Job-flag contrast.

### Concrete WIP Findings

1. Cross-row preview suppression is missing. tests.rs:2459 RowReport::value
   calls previews independently; :2487 Report::describe collects both values
   without a shared sentinel decision. A fixture sentinel in one producer's
   stream suppresses only that row, leaving the peer row's previews visible.
   The approved contract requires ALL previews suppressed across both rows.
   Compute the attempt-wide sentinel decision before decoding/cropping either
   row, covering both complete bounded streams and all three encodings. The
   decision also needs to survive discarding an incomplete/stale producer's
   bytes in Reply::received (:2052), so a bounded captured sentinel cannot be
   forgotten before the other row is rendered. No private buffers or raw error
   text should be added. Regress both row directions, streams, encodings and
   beyond-preview-cutoff input, including metadata-only producer outcomes.

2. Test migration is incomplete in this WIP snapshot. Report now contains only
   rows (:2175), but describe_output (:2536) and tests (:2642, :2791) still
   construct removed producer/failure fields. collect_pair now requires Pair
   (:2213), while existing calls at :2686/:2717/:2723 omit it. The old readiness
   test (:2732) still constructs removed Probe/record/count fields. These are
   source-visible release blockers, not a claimed compiler run; Newton may
   already be addressing them. The reviewed snapshot also lacks the new shared
   hold/deadline/unwind/TLS/real-retirement regression coverage required by the
   contract. Recheck the released version rather than treating old green tests
   as evidence for this new coordination surface.

### Boundaries Inspected

- execution_host.rs:1654 enters the cfg(test,windows) hook only after direct
  child exit and both readers have joined, and only on the existing non-timeout
  path. Empty, untruncated exits 0 and 1 are eligible (:1150); other exits/output
  do not wait. Spawn/attach/read/timeout errors keep their earlier cleanup path.
- The existing process_tree.terminate()? follows the hook at :1661. The Pair
  owns only fixed coordination/metadata, not Job/process handles. No early
  terminated state, substitute termination result or moved guard was observed.
  Configure/attach/terminate/Drop still use the existing strict Job policy.
- Pair's deadline is fixed at creation plus ten seconds (:1009). Condvar wakes
  recompute only remaining duration (:1084), and return/abort/deadline release
  the hold. State critical sections contain no command execution or peer join.
- collect_pair signals producer_returned or abort before owner.join
  (tests.rs:2231). ProbeOwner::Drop signals abort before its fallback join
  (:2202); producer panic is caught as typed failure. Ordinary producer errors
  follow the same signal/join order. The probe itself retains original command
  bounds plus cleanup grace; OS failure/runner loss remains an explicit limit.
- The scoped TLS rejects nested activation/reuse before installing a new scope
  (execution_host.rs:1109), and its Reset clears TLS on return/unwind. Pair has
  one used latch; run_rows has exactly two fixed rows, each with one mkdir,
  producer and probe. Original private polling/assertions are not replaced.
- Combined escaped output has a 4096-byte cap and metadata-only fallback
  (tests.rs:2487). The actual public entry remains typed and attested with no
  private-buffer input. This cap does not repair finding 1's cross-row policy.

No additional hold/RAII/result-preservation defect was identified in this source
snapshot. Final review must cover the completed regressions and source changes
after Newton's explicit release; all execution remains Actions-only. Main's
separate typed WSL Some(-1) uncertainty correction has its own writer and is
reserved for the post-release combined check. No broader negative/HRESULT
classification, production wait or containment policy change is implied.

## Tesla WSL Receipt Safety: Independent Source Check

Reviewed the released/frozen git_backend/host.rs and git_backend/tests.rs slice,
Tesla's wsl-launcher-outcome-implementation.md handoff, and the real write lease
and reconciliation paths. No findings in the authorized correction. Only this
review addendum changed; source remains untouched, including Newton's WIP.
No tests, build, formatter, lint, probes, Git operations or children ran locally.

### Production Boundary

host.rs:41 process_dispatch adds exactly one new uncertain receipt: a typed
ExecutionBackend::Wsl with exit_code Some(-1). Existing timeout/missing-exit
uncertainty remains. Local -1, other negative values and positive WSL statuses
retain their prior classification. The production adapter calls this helper at
:222 using the captured snapshot backend after the existing output clipping.
The helper only borrows the output; bytes, exit/timeout/truncation flags, source
identity and epoch ownership are not rewritten. SSH remains in its separate
branch; ProgramNotFound and other executor error handling at :227 is unchanged.
No message/program/cwd inference, Job policy or cleanup change was introduced.

The newly uncertain receipt reaches Attempt::checked (:68), which rejects it
with an unavailable outcome rather than treating launcher loss as a Git exit.
PreparedGitWrite::execute records that dispatch before checking the result
(write.rs:250); successful reconciliation cannot clear the uncertainty at :292.
This closes the specific source finding without requiring coordinator changes
or making a claim that guest execution stopped.

### Regression Strength

- `git_backend::tests::process_dispatch_distinguishes_wsl_launcher_loss_from_native_and_git_exits`
  directly calls the production helper for Local/WSL -1, -2, 0, 1, 128, 255,
  existing timeouts and missing exits (tests.rs:210). It pins the narrow -1
  exception rather than expanding the policy to all negative/HRESULT values.
- `git_backend::tests::wsl_launcher_loss_retains_exact_lease_after_reconciliation_until_explicit_review`
  uses fake()'s actual typed WSL snapshot (:187). Its raw non-timeout,
  untruncated -1 mutation receipt is fed to the same production classifier by
  FakeHost::run (:74), not a scripted Dispatch::Uncertain. The test then uses
  the real prepare/execute/reconcile/lease/review code (:363).

The lease test asserts successful original-source reconciliation with retained
operation ID, source signature, project and repository ownership. A previously
prepared conflicting write and a new write are both refused without extra host
commands. A different valid operation ID cannot review/release the lease.
Exact-ID review with changed current authority fails and preserves quarantine;
restored authority plus UserConfirmedOriginalOperationStopped permits the
existing read-only release. Repeating that review fails. Mutation and commit
counts remain one across every review, covering no replay. Reconciliation clones
the scripted host (write.rs:1064); fresh valid WSL backend checks do not replace
it with a real execution adapter, so the test does not secretly launch WSL.

Both new test functions remain authored UNRUN pending exact-SHA Actions. This
is synthetic receipt-to-production-classifier-to-real-coordinator coverage,
not an actual Windows/WSL transport run. Earlier ordinary CI does not validate
this patch. The lease remains process-local and explicit release still relies
on the user's stopped-operation confirmation; no durable recovery, positive
guest-stop proof or causal Job conclusion is claimed. Newton's released version
still requires its separate final/combined check when Main authorizes it.

## WSL Client Containment: Independent Read-Only WIP Review

Scope follows Main's new production-candidate authorization, the fresh curated
Git/Tasks contexts, parent PRD's Explicit WSL Follow-Up Approval, Git contract's
WSL Client Containment scenario and private Tasks ownership paragraph. The
earlier diagnostic-only/no-flag handoffs above are historical, not the authority
for this newly approved candidate. Newton owns execution_host.rs/process.rs;
Tesla separately owns appended Tasks peer-survival fixtures. Only this addendum
was written. No source edits, children, Git operations, local automated checks,
upstream investigation, CI inspection or probes occurred.

### Current Source Findings

No concrete routing, flag-selection, root-cleanup or output-contract defect was
identified in the production WIP inspected. This is provisional source review,
not writer release, compilation evidence or a passing containment gate.

- execution_host.rs:57 maps the captured backend explicitly: Local is
  StrictTree, Wsl is WslClientRoot, SSH has no process policy. The registered
  executor consumes that mapping in its Process arm (:414); the SSH branch is
  unchanged. PreProjectLocalContext has its own typed mapping (:262), passed by
  execute_pre_project_local_command (:369). No executable/path/environment
  heuristic determines policy, and public planner/result APIs stay unchanged.
- The ordinary runner still selects Null stdin. Its old private strict wrapper
  is now test-only because both production callers pass the policy explicitly;
  the shared body preserves command argv/cwd, capture limits and result fields.
  The already-attested test-only marker probe also selects WSL explicitly.
- ProcessTree::configure remains the strict default (:1587). Its factored
  Windows constructor sets exactly KILL_ON_JOB_CLOSE for strict and adds only
  SILENT_BREAKAWAY_OK for WSL (:1615). The same extended-limit structure and
  SetInformationJobObject call are used. No launch breakaway bit, shared Job,
  retained handle, production wait/retry or user-distro change appears.
- Both flags are imported from the already-enabled Win32_System_JobObjects
  namespace and combined as the existing LimitFlags type. Cargo.toml directly
  enables that feature and the selected lockfile entry is windows 0.61.3; no
  dependency/feature change is needed by this source shape. The locked generated
  bindings are not cached locally, so exact Windows compilation remains an
  Actions gate, not a source-inspection pass claim.
- CREATE_SUSPENDED/CREATE_NO_WINDOW, exact root AssignProcessToJobObject before
  resume, attached-before-resume bookkeeping, explicit TerminateJobObject,
  failure propagation, terminated-on-success only and final owned-handle Drop
  remain intact (:1602, :1648, :1677). Spawn/configuration failures retain owned
  cleanup; attach/resume/read setup errors still take the existing bounded
  cleanup/direct-child fallback. No missing attachment becomes successful.
- Unix configure_with_policy delegates to the existing dedicated process-group
  configuration (:647), preserving non-Windows behavior. The Windows exception
  does not change native/default Job limits or interactive PTY/SSH execution.
- Private run_wsl validates typed WSL first and passes WslClientRoot explicitly
  (process.rs:659), independently of its cooperative=true argument. All native
  unselected/lookup/proof/data captures explicitly pass StrictTree (:433, :456,
  :472); the cooperative synthetic envelope fixture is also explicitly strict.
  This keeps private bytes in the private capture path, never the ordinary
  runner or a new general command-result surface.
- Private sanitation, sole Python cwd entry, control pipe, latched stop/deadline,
  owned root cleanup, bounded drain and strict cleanup acknowledgement remain
  unchanged. Missing acknowledgement after a stop is still CleanupFailed
  (:922); unstopped nonzero launcher exit is still HostHelperUnavailable
  (:679). No account/source/proof fencing or error remap is altered.

### WIP Regression Inspection

Three shared-runner test functions were visible during the pass, all UNRUN:

- `execution_host::tests::process_tree_policy_follows_typed_sources_not_executable_or_path`
  checks registered/pre-project Local and WSL policy selection, misleading
  native executable/path text, shared planning and SSH separation.
- `execution_host::tests::windows_process_tree_policies_set_exact_job_limits_and_keep_strict_default`
  queries actual configured Job limits for default/explicit strict/WSL and pins
  unchanged creation flags. It does not merely compare two policy constants.
- `execution_host::tests::windows_process_tree_policies_keep_exact_root_attachment_and_retirement`
  creates an owned suspended root under both policies, verifies attachment and
  live membership, checks the actual retirement result and bounded root exit,
  and checks termination idempotence. The fixture owner exists before fallible
  attachment/assertions, preserving Drop cleanup on unwind.

These cover the inspected policy mechanics but do not substitute for Tesla's
appended actual WSL peer-survival tests or the original private account/lifecycle
assertions. Private explicit policy is source-traced here; final combined review
must check its released regressions and all original assertions. No source
ownership transfers in this WIP review.

### Evidence And Limits

Main supplies f1fde4d actual WSL job 101478843292: Immediate retired an overlapping
probe before its start-only producer failed; AfterProducer held the same timing
boundary until exact-marker producer success. Both timing issues were null,
but the original DataCancel assertion still failed and owned cleanup passed.
This supports the retirement boundary on that owned WSL1 fixture, not kernel
membership, this candidate's correctness, WSL2 or native UI acceptance.

The candidate intentionally stops promising containment of eligible escaped
Windows relay/interop descendants. Ordinary WSL still has no positive guest-stop
acknowledgement; typed WSL -1 Git receipts must retain their separately corrected
uncertain write lease. Private Linux groups/control/strict acknowledgement keep
their existing limited ownership, not a new guarantee for escaped daemons.
Nested Jobs, enabled interop, WSL2 and real shared-distro behavior remain explicit
acceptance risks. No new supervisor, fault-injection framework or requirements
expansion is proposed. Main owns final authorization, specs/Git/CI and exact-SHA
gates after Newton and Tesla release.

## Newton Containment Release: Partial Final Check

Read the finalized wsl-containment-implementation.md handoff and released
execution_host.rs/tasks_account_executor/process.rs source. No concrete defect
remains in this production slice; no source fix was needed. All five focused
tests were inspected, including the final explicit SSH fixture construction:

- `execution_host::tests::process_tree_policy_follows_typed_sources_not_executable_or_path`
- `execution_host::tests::windows_process_tree_policies_set_exact_job_limits_and_keep_strict_default`
- `execution_host::tests::windows_process_tree_policies_keep_exact_root_attachment_and_retirement`
- `tasks_account_executor::process::tests::private_wsl_entry_rejects_non_wsl_sources_before_capture`
- `tasks_account_executor::process::tests::private_root_cleanup_preserves_failure_before_assignment_for_both_policies`

The last two call the real private entry/cleanup boundaries. Local and SSH
snapshots with WSL-looking path/program data are rejected before capture. The
pre-assignment case creates an owned suspended native root under each policy,
requires CleanupFailed from the unattached Job while separately requiring
direct-root reaping, and constructs its owner before fallible assertions. The
shared limit test queries configured Job state; the root test verifies exact
membership, bounded exit and termination idempotence. These are native guard
tests and typed routing checks, not real WSL2 or guest-descendant evidence.

The provisional production conclusions above remain valid: strict is the
default, both ordinary routes retain typed WSL authority, private run_wsl
selects its policy explicitly, the marker test does so only after attestation,
and original timing hooks, private pipes/control/acknowledgement and cleanup
error/results remain intact. No additional diagnostics or policy expansion is
needed for the source reviewed.

Evidence distinction: Main reports the f1fde4d Windows job 101478701601 passed
all eighteen predecessor timing/public regressions and both Git receipt safety
tests (execution-host 25, Tasks 44 plus one ignored, Git 126); package job
101478701516 passed. Its Linux formatting failed, and original actual WSL still
failed despite positive timing evidence. Those results do not validate the
new production containment or peer-survival fixture slice. Its five focused
tests and Tesla's additions remain UNRUN pending fresh exact-SHA Actions.

This is only the completed Newton portion of the final check. Tesla retains
Tasks tests.rs ownership until its explicit release; no edit was made there or
to any source. The combined source release remains pending that fixture review.
Only this report was updated. No local automated verification, transport probes,
upstream/CI fetch, Git operation or child agent occurred.
