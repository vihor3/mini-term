# Public WSL Retirement Timing Implementation

Status: RELEASED TO MAIN / FROZEN for independent check. Source-authored only;
no compilation, tests, fixtures, probes, formatting or other automated checks
were run. Main owns specs, Git and exact-SHA Actions evidence.

## Changed Files

- `crates/mt-app/src/execution_host.rs`: Windows-test-only public timing scope,
  fixed-size coordinator, typed release/timestamps, normal-retirement hook and
  focused regressions. No ProcessTree method, Job limit or creation flag changed.
- `crates/mt-app/src/tasks_account_executor/tests.rs`: only the nested
  `wsl_public_comparison` module changed. Two ordered rows replace the public
  polling comparison; ownership, output privacy and focused tests are updated.
- This implementation handoff. No other files were written.

## Implemented Boundary

- The existing exact-once typed private-failure trigger now contains Immediate
  then AfterProducer. Each successful row creates a new attested UUID case with
  the original no-reuse mkdir, runs the immutable producer, and dispatches just
  one original own-ready-file probe. Maximum host calls remain two setup, two
  producers and two probes. No startup barrier, warmup, repeat probe or retry.
- Only the public readiness thread enters the one-shot scope. The generic
  runner invokes the hook after child exit and joined readers, immediately
  before its existing normal `process_tree.terminate()?`. Empty, complete,
  untruncated exits 0 and 1 are eligible. Timeout and read/dispatch failure
  paths do not enter this normal hook; ineligible normal captures do not wait.
- The coordinator contains only fixed state, row, clock/deadline and numeric
  timing metadata. It never owns a Job, ProcessTree or process reference. The
  same readiness stack retains the guard while AfterProducer waits until the
  producer returns/aborts or the absolute pair-start-plus-ten-second deadline.
- Producer return is recorded and signaled before joining readiness. Producer
  panic becomes a typed diagnostic failure; the fallback owner aborts before
  joining during unwind, discarding a probe panic without a second panic.
  Ordinary errors still join the live probe. This relies on its existing
  five-second runner and two-second cleanup waits plus the bounded hold, not
  an unbounded coordination receive. Native OS calls and scheduling cannot be
  made hard real-time by a Rust join; OS failure/runner loss remains a limit.
- Normal release falls through to the unchanged explicit termination and
  Drop/owned-handle closure. No early disarm, extra kill, changed error mapping,
  Job handle export, process inventory, private capture or production wait.
- Release reasons distinguish deadline, late producer return, abort, bypass,
  ineligible capture and rejected/failed coordination. The report is always
  explicitly inconclusive; timing issues and existing bounded retirement
  records do not establish a production fix or Linux descendant cleanup.

## Privacy And Review Findings

The Newton Timing Contrast WIP findings were addressed before release:

- Removed-field test constructors and old polling-record tests are migrated.
- `Reply::received` scans both full public producer captures for the fixture
  sentinel in UTF8, UTF16LE and UTF16BE before discarding incomplete/stale
  captures. Only a boolean survives that discard, never the rejected bytes.
  `Report::describe` resolves attempt-wide suppression before either row's
  decode/prefix removal/crop; either row's sentinel suppresses both previews.
- The unchanged strict decoder, prefix rule and Unicode escaping remain at the
  public-only boundary. Both rows together are capped at 4096 bytes. Overflow
  drops previews first, then uses explicitly limited metadata retaining rows,
  timing and command outcomes if full retirement metadata cannot fit.

## Authored Actions Tests

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

The real native runner tests author inactive output parity, actual normal
retirement after release and timeout/truncation bypass. Read failure uses the
existing bounded reader; failed-retirement projection uses typed synthetic
records. Handle closure is retained by the unchanged ownership path, not a new
handle-export/closure API test. No actual failing TerminateJobObject injection
or WSL result is claimed.

## Required Follow-Up

Independent check and exact-SHA nonempty Windows module tests, normal CI and
the original ignored
`tasks_account_executor::tests::tasks_account_executor_wsl_sentinels_cleanup_and_foreground_host`
remain pending in Actions. The private 50-ms polling, ten-second readiness,
cancellation/order, result assertions and descendant checks were not edited.
The baseline HostHelperUnavailable failure must still fail its original
assertion even if the diagnostic B producer completes. Exact owned-distro
cleanup remains mandatory. This single post-failure WSL1 contrast is neither
a production policy fix nor evidence about WSL2 or native UI acceptance.
