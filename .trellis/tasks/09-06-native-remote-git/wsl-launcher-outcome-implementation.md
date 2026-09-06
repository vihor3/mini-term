# WSL Launcher Outcome Correction

Status: RELEASED TO MAIN / FROZEN. Source review complete; all automated
verification UNRUN. No child agents, Git writes or local execution checks.

## Changed Paths

- `crates/mt-app/src/git_backend/host.rs`
- `crates/mt-app/src/git_backend/tests.rs`
- `.trellis/tasks/09-06-native-remote-git/wsl-launcher-outcome-implementation.md`

The existing Git backend test layout is `git_backend/tests.rs` plus inline
lease tests in `git_backend/write.rs`. Only the former needed changes.
`write.rs` is unchanged; no broader coordinator source correction was needed.

## Behavior And Source Review

The Local/WSL adapter now calls the small `process_dispatch` classifier using
its captured typed `ExecutionBackend` and existing bounded `CommandOutput`.
Only WSL `exit_code == Some(-1)` newly becomes `Dispatch::Uncertain`: the
reported launcher loss is not a Linux Git completion or guest-stop proof.
No program, cwd, message-text or cooperative-stdin inference is used.

The helper borrows the receipt. Existing stdout/stderr clipping, output bytes,
exit code, timeout/truncation flags and error construction stay in place.
Native -1, ordinary WSL exits including 255, timeouts, absent exits and executor
errors retain their previous handling. ProgramNotFound still means
NotDispatched; other executor errors remain Uncertain. SSH uses its unchanged
separate branch; no source identity, epoch, cleanup or cancellation mapping is
altered.

Source review traced the real adapter into `Attempt::checked`,
`PreparedGitWrite::execute`, reconciliation, retained lease state and
`review_uncertain`. The existing coordinator already quarantines an uncertain
receipt after successful read-only reconciliation. Correcting this receipt is
sufficient; there is no retry, rollback claim or new release mechanism.

## Authored Tests: UNRUN

Both tests live in the existing `crates/mt-app/src/git_backend/tests.rs`:

- `git_backend::tests::process_dispatch_distinguishes_wsl_launcher_loss_from_native_and_git_exits`
- `git_backend::tests::wsl_launcher_loss_retains_exact_lease_after_reconciliation_until_explicit_review`

The classifier matrix covers Local and WSL with -1, another negative exit,
success, ordinary failures 1/128/255, timeout with present exit, and absent
exit with/without timeout. It calls the exact production classifier.

The coordinator regression extends the existing scripted FakeHost with an
optional raw mutation output, classified by that same helper rather than a
preselected Uncertain dispatch. Its -1 receipt has `timed_out: false` and
untruncated streams. It asserts successful original-source reconciliation,
retained exact operation/source/project/repository ownership and Uncertain
phase, refusal of queued/new conflicting writes without additional commands,
rejection of another operation ID, retention after an exact-ID review fails
current authority, and release only through the existing explicit
`UserConfirmedOriginalOperationStopped` review after authority is restored.
Mutation and commit command counts remain exactly one through all reviews.
Existing tests and gates are preserved.

## Verification And Remaining Gates

Performed only source/contract reading and manual read-only Git status/diff
review. Compilation, metadata, tests/fixtures, lint, formatting, automated
whitespace checks, probes and application launches were NOT run locally.
No Actions run or passing result is claimed for this patch. Main owns Git,
exact-product-SHA Actions validation and any independent source review.

This is an independently justified receipt safety correction, not a fix or
causal conclusion for the WSL Job lifecycle issue. It does not prove guest
termination, WSL1/WSL2 compatibility, transport cleanup or native UI acceptance.
The lease remains process-local and explicit review still requires the user's
confirmation that the original operation stopped. Newton's execution-host and
Tasks diagnostics, private capture, Job policy, dependencies, CI, UI, and
Main-owned specs/docs outside this report were not edited.
