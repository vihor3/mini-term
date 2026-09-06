# Tasks Accounts Independent Review

Date: 2026-09-06. Assigned child: `09-06-native-tasks-gh-accounts`; the active
pointer still identifies the Git child. Source review and explicitly authorized
Actions log inspection. No passing build, lint, type-check, formatting, test,
transport, or native acceptance is claimed for the current literal-argv diagnostics.

## WSL Literal-Argv Diagnostics: Source Released To Main

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
