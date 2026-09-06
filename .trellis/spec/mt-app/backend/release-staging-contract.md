# Release Staging Contract

## Scenario: Build and verify desktop release payloads

### 1. Scope / Trigger

Use this contract when GitHub Actions CI/release workflows invoke
`scripts/stage-sidecars.mjs` to build or stage the GPUI application,
root-workspace helpers, sidecar-workspace binaries, portable ConPTY, or a
Windows installer. The staging directory is executable release state and must
never contain a partially validated mixture of architectures or dependency
graphs.

### 2. Signatures

```text
node scripts/stage-sidecars.mjs [--release] [--target <triple>]
node scripts/stage-sidecars.mjs --verify-only --release --target <triple>

CARGO_TARGET_DIR=<GitHub Actions job-owned build root>
MINI_TERM_STAGE_DIR=<GitHub Actions job-owned runnable staging root>
```

Owned Cargo gates use both lockfiles:

```text
cargo metadata --locked ...
cargo build|check|test|clippy --locked ...
cargo metadata|build|check|test --manifest-path sidecars/Cargo.toml --locked ...
```

### 3. Contracts

- The root workspace and `sidecars/` workspace are independent locked graphs.
  Every owned CI/release command that resolves either graph passes `--locked`;
  lock drift fails before packaging.
- Compile, rustfmt, Clippy, tests, staging, Windows cross-compilation, NSIS,
  extraction, and payload verification execute only in GitHub Actions. A local
  workstation may edit files, inspect diffs, perform Git operations, and clean
  residual artifacts; it must not run those executable validation commands or
  Docker CI as a substitute.
- Action job workspaces and caches own all transient output. Uploaded workflow
  artifacts own distributable installers and validation manifests; a local
  artifact is never release evidence.
- `swatinem/rust-cache` target mappings are relative to each declared workspace:
  use `. -> target` and `sidecars -> target`. Writing
  `sidecars -> sidecars/target` caches an unused nested directory.
- `mt-terminal-host` links in an isolated root-helper target namespace and
  sidecars link in their own namespace. Neither build output is the live stage.
  Dev copy failure preserves the existing runnable file with a warning; release
  copy failure is fatal.
- Supported Windows staging is currently only `x86_64-pc-windows-msvc`.
  Unsupported Windows triples are rejected while planning, before Cargo runs or
  any workspace-local/stage output is created.
- Every built executable must be a non-empty regular file and, for Windows x64,
  report PE machine `0x8664` before any artifact is copied. A release failure
  removes staged sidecars, root helpers, and portable ConPTY so stale payloads
  cannot be packaged.
- Portable ConPTY validation requires x64 `conpty.dll`, x64
  `portable-conpty/x64/OpenConsole.exe`, arm64
  `portable-conpty/arm64/OpenConsole.exe`, and official hashes in release mode.
  The arm64 OpenConsole helper is intentional portable-ConPTY content; all
  application/sidecar executables remain x64.
- After the application build, `--verify-only` validates the complete stage,
  including `mini-term`, before NSIS or archive collection runs. Verification
  is read-only and never builds or repairs missing artifacts.
- A Windows package is accepted only after extraction proves the expected
  payload set and each extracted payload hash exactly equals its staged source.
  PE machine, application resources/version, and required integration markers
  are additional release evidence, not substitutes for hash equality.

### 4. Validation & Error Matrix

| Condition | Required behavior |
|-----------|-------------------|
| Root or sidecar lockfile is stale | Fail the locked metadata/build gate |
| A workstation validation or packaging command is proposed | Stop and push or dispatch the owning GitHub Actions workflow |
| Sidecar cache maps to `sidecars/target` from the `sidecars` workspace | Reject the workflow; map that workspace to `target` |
| Windows target is not x86_64 MSVC | Reject before Cargo and before creating stage output |
| Built helper is missing, empty, or wrong PE machine | Reject before copy; clean release stage |
| Dev destination is locked by a running helper | Keep the prior runnable artifact and warn |
| Release destination copy fails | Fail and remove task-owned staged payloads |
| Portable ConPTY layout/hash is wrong | Fail release verification before installer build |
| Complete stage omits `mini-term` or a helper | `--verify-only` fails before NSIS/archive collection |
| Extracted installer payload differs from stage | Reject the installer and report the differing hash |

### 5. Good / Base / Bad Cases

- Good: A GitHub Actions runner builds root and sidecar graphs into isolated
  namespaces, validates PE machines, stages once, verifies the complete payload,
  then packages and hash-compares the extracted installer before upload.
- Base: A non-Windows GitHub Actions build stages helpers without portable
  ConPTY and keeps the isolated-link/copy boundary.
- Bad: Link `mt-terminal-host.exe` directly into a directory from which an old
  copy may be running; Windows can reject or corrupt the replacement step.
- Bad: Copy artifacts first and inspect architecture afterward; stale valid
  files may combine with one wrong-machine binary into a plausible package.

### 6. Tests Required

- Staging-plan tests assert job-owned target/stage/cache inputs, isolated root
  and sidecar namespaces, locked dependency graphs, and collision-free staging.
- Unsupported-target tests assert zero Cargo invocations and zero stage output.
- Copy tests assert dev failure preserves the old file and release failure is
  fatal.
- PE tests reject wrong architecture before copy and in complete verify-only
  mode; release failure removes stale stage payloads.
- Static workflow tests assert locked root/sidecar metadata/build/check/test,
  exact `. -> target` / `sidecars -> target` cache mappings, and complete-stage
  verification between app build and installer.
- GitHub Actions Windows jobs compile every affected payload. Package
  validation extracts the installer and compares exact hashes, machines,
  resources, versions, and feature markers before the artifact is uploaded.
- Workflow run URLs, job conclusions, artifact identity, and generated hashes
  are the acceptance evidence. Local retries or previews cannot replace them.

### 7. Wrong vs Correct

#### Wrong

```text
cargo build helper into live stage
copy remaining files
build installer
inspect a few filenames
```

This permits dependency drift, locked-file replacement failures, mixed
architectures, and stale payload reuse.

#### Correct

```text
locked root + sidecar graphs on GitHub Actions
-> isolated job-owned build roots
-> validate built files and PE machines
-> stage/copy
-> verify complete stage and ConPTY hashes
-> build installer
-> extract and compare every payload hash
```

Each transition consumes a fully validated predecessor and leaves no ambiguous
partial release state after failure.

## Scenario: Actions Formatting Diagnostics

### 1. Scope / Trigger

Use this boundary when the pre-release CI formatter reports changed-line
violations and uploads a patch for source repair. Diagnostic relevance and a
safe source transformation are different contracts.

### 2. Signatures

Runner-only entry points:

```text
node .github/scripts/check_changed_rustfmt.mjs <base-sha>
node --test tests/changedRustfmt.test.cjs
RUSTFMT_PATCH_PATH=changed-rustfmt.patch
RUSTFMT_FULL_PATCH_PATH=full-rustfmt.patch
```

### 3. Contracts

- Changed-line intersection controls failure; historical formatting entirely
  outside changed lines remains non-failing. Baseline-only and untouched files
  are not included in an applicable artifact.
- Once a file has a relevant formatting violation, BOTH patch paths contain
  its complete `diff -U3` transformation to rustfmt output. Retain every hunk
  in that file, including historical ones. The legacy `changed-rustfmt.patch`
  name does not mean individual hunks may be discarded.
- Selected `-U0` hunks are console diagnostics only. They may omit half an
  import move and retain offsets assuming omitted edits occurred. They must
  never be presented or applied as an independent source repair.
- Produce and test artifacts only in Actions. Source repair may mechanically
  apply downloaded complete patches, preserving unrelated work. A follow-up
  source commit needs fresh Actions evidence. Never regenerate/check locally
  or use `--unidiff-zero` to relax artifact application.
- Fixing the producer does not repair old incomplete applications. Compare
  affected committed source with the original complete artifact, restore lost
  content through its current owner, and keep concurrent feature edits separate.

### 4. Validation & Error Matrix

| Condition | Required behavior |
| --- | --- |
| Changed import is moved onto baseline lines | Artifact retains deletion and insertion |
| Earlier historical formatting changes later hunk offsets | Include the full file transformation with context |
| Only baseline formatting differs | Exit zero and produce no applicable patch |
| Two files have relevant changes, another is baseline-only | Complete patches for the two affected files only |
| Patch fails normal contextual application | Stop and reconcile source ownership; do not force offsets |
| Old partial patch compiled incorrectly or lost content | Restore from original source/full artifact; rerun all affected gates |

### 5. Good / Base / Bad

- Good: a moved import and its unchanged-line insertion travel in one complete
  artifact, which reproduces the formatter's full file output.
- Base: a changed comment does not force unrelated historical formatting.
- Bad: discard nonintersecting hunks and assume the remainder preserves Rust
  semantics because the original full transformation came from rustfmt.

### 6. Tests Required

The four production-script fixtures exercise import moves, split declaration
hunks after ignored offsets, unchanged baseline gating, and multiple files.
Both public artifacts must apply with ordinary `git apply`, reproduce the full
expected formatter output, and leave excluded files unchanged. Run after Setup
Rust in Actions. Compilation/lint/tests and exact-SHA package verification are
still required; patch-application tests alone do not validate the product.

### 7. Wrong vs Correct

Wrong: upload independent changed-line `-U0` hunks and apply with relaxed context.

Correct: use changed lines to select violations/files, upload complete per-file
`-U3` patches, apply them without relaxing context, then validate the resulting
commit through Actions.

## Scenario: Actual Execution-Host Fixtures

### 1. Scope / Trigger

Use the CI-owned loopback SSH and disposable WSL fixtures when validating host
transport, account secrecy, cancellation and exact-source filesystem behavior.
Pure parsers, synthetic adapters and Linux Python-envelope tests do not prove
an actual Windows-to-WSL or authenticated SSH pipeline.

### 2. Signatures

```text
node .github/scripts/tasks_wsl_fixture.mjs prepare|import|test|cleanup

MT_TEST_SSH_ROOT, MT_TEST_SSH_PORT, MT_TEST_SSH_USER, MT_TEST_SSH_KEY
MT_TEST_WSL_DISTRO=mt-tasks-<GITHUB_RUN_ID>-<GITHUB_RUN_ATTEMPT>
MT_TEST_WSL_MARKER=/mini-term-fixture/owner.json
MT_TEST_WSL_GH_SHA256=<same-run Linux synthetic gh ELF SHA-256>
```

WSL guest owner JSON contains exactly numeric `schema: 1`,
`kind: "mini-term-tasks-wsl"`, and string `run_id`, `run_attempt`, `repository`,
`sha`. The artifact manifest changes kind to `mini-term-tasks-wsl-artifact` and
adds `source_rootfs_url`, `source_rootfs_sha256`, `rootfs_sha256`, `gh_sha256`.
Credentials are not fields of either object.

### 3. Contracts

- All setup, compilation, import, transport commands, fixtures, cleanup and
  verification execute only in GitHub Actions. Never inspect real device gh
  credentials or use a user's existing distro/SSH session as a fixture.
- Linux prepares a Canonical amd64 Ubuntu 22.04 rootfs with a source-controlled
  SHA-256 pin. Verify the download before extraction; a changed upstream file
  fails pending explicit review, never automatic pin replacement. Compile the
  checked-in std-only `gh_fixture.rs` on Linux and install that ELF plus the
  checked-in WSL Python launcher. Guest Python remains `/usr/bin/python3`.
- Upload rootfs and manifest as a same-run, short-retention artifact. Windows
  validates run/attempt/repository/commit, source pin and downloaded hash before
  import. Guest tests also check bounded marker output, exact gh/Python lookup
  and ELF hash before invoking any account API.
- Use a unique run/attempt-owned distro on the Windows 2022 WSL 1 runner.
  Refuse a pre-existing name or import ownership state. Persist fixture import
  ownership before dispatch so interrupted imports still have guarded cleanup.
  Never set a default distro/version or issue global `wsl --shutdown`.
- Imported guest automount and Windows interop/PATH append are disabled.
  Synthetic cases and HOME live below `/mini-term-fixture`; account fixtures
  explicitly seed conflicting synthetic auth/debug variables so sanitation is
  tested. No real gh binary/configuration or token store is imported.
- The Windows transport test must be discovered exactly once, then explicitly
  run with `--ignored --exact --test-threads=1`; zero matches cannot pass.
  Timeouts fail the gate. An `always()` step cleans only the recorded exact
  owned distro, including partial import or an already stopped guest. Missing
  WSL capability is a failure, not fallback to Linux-envelope evidence.
- SSH setup uses only 127.0.0.1 and an unprivileged port, disposable keys and
  homes below RUNNER_TEMP, a marker and an early cleanup trap. Public-key-only
  sshd may use PAM to avoid the portable non-PAM locked-account gate; password,
  keyboard authentication, forwarding and user rc/environment files remain off.
  Do not unlock accounts or modify real SSH configuration.
- The shared test-only `loopback_ssh_fixture` validates Actions inputs and
  containment; it does not create servers. Actual Git, Tasks and Files tests
  consume that single setup. Keep transport failures distinct from authoritative
  absence, and verify execution counts for exact ignored fixture filters.
- Runner logs/artifact manifests are execution evidence for their exact product
  SHA. These fixtures do not establish native UI geometry, every mid-dispatch
  fault, a user's installed CLI capability or secure-store accessibility.

Source references for setup, not runtime proof:
[Canonical checksum](https://cloud-images.ubuntu.com/wsl/jammy/current/SHA256SUMS),
[Windows 2022 WSL feature](https://github.com/actions/runner-images/blob/main/images/windows/Windows2022-Readme.md),
[Microsoft import/cleanup commands](https://learn.microsoft.com/en-us/windows/wsl/basic-commands),
[same-run artifact download](https://github.com/actions/download-artifact).

### 4. Validation & Error Matrix

| Condition | Required behavior |
| --- | --- |
| Wrong run, commit, marker, rootfs or ELF checksum | Fail before account requests |
| Existing distro or foreign cleanup state | Refuse import/destruction |
| WSL capability absent or exact test missing | Fail explicitly, no silent skip |
| Test cancellation/timeout or partial import | Preserve failure and run owned cleanup |
| Another distro exists/runs | Leave it untouched |
| SSH key/root/home escapes RUNNER_TEMP fixture | Refuse fixture configuration |
| Fake transport passes | Do not claim authenticated SSH/actual WSL coverage |

### 5. Good / Base / Bad

- Good: same-run manifest and guest provenance match, synthetic account requests
  run through production `wsl.exe`, and the owned distro alone is unregistered.
- Base: a missing WSL component fails setup without touching another distro.
- Bad: run a Linux shell envelope and mark Windows cancellation coverage passed,
  or use the runner's default distro because import is inconvenient.

### 6. Tests Required

Assert selected-account list/detail identity, both stdout/stderr secrecy,
same-captured credential after lookup rotation, lookup/data cancellation and
timeout, real descendant readiness/retirement, missing helper/malformed reply,
SSH epoch replacement, and no global account change. Fixture setup itself must
execute in the exact-SHA workflow and complete guarded cleanup. Native Windows
process ownership, pure account/config tests and ordinary Linux tests remain
separate gates, not replacements for actual transport execution.

### 7. Wrong vs Correct

Wrong: accept any available WSL distro and infer a test ran from Cargo exit zero.

Correct: validate same-run ownership and fixture bytes, import a unique guest,
require exact test discovery and execution, then clean only that recorded guest.

## Scenario: WSL Marker Launch Diagnostics

### 1. Scope / Trigger

An Actions-owned WSL fixture may import successfully but fail the production
ReadOwner launch before any account request. Diagnose that boundary without
changing production credentials, stdin behavior or process cleanup guarantees.

### 2. Signatures

Windows test builds alone expose `tasks_wsl_marker_probe(snapshot, cwd, user,
stdin) -> Result<HostCommandResult, CommandExecutionError>`. The selectors are
`TasksWslProbeCwd::{Captured, Root}`, `TasksWslProbeUser::{Default, Root}` and
`TasksWslProbeStdin::{Null, ClosedPipe}`. Private `run_process` retains its
existing signature and production Null selection; the factored runner has no
production ClosedPipe variant.

Private Windows tests also use `WslCwdProgram::{Absolute, Path, Relative}`,
static cwd stages, and `wsl_require_literal_argv` for failure-only literal-argv
diagnostics. `WslArgvDiscriminator` selects ten fixed synthetic printf plans;
it is not a public arbitrary-command or account-execution API.

### 3. Contracts

- Require Actions, numeric run/attempt, the matching `mt-tasks-<run>-<attempt>`
  distro, exact marker path and captured `/mini-term-fixture` source. The helper
  runs only `/bin/cat /mini-term-fixture/owner.json`; it accepts no arbitrary
  command, environment map, credential or alternate marker.
- The helper must match current production planning. After the reviewed
  fixed-root correction, Captured/Root select the requested Linux directory
  inside the encoded directory wrapper, not different Windows-side `--cd` values.
  Every row launches with `--cd /`; source identity remains captured and the
  original baseline still uses the ordinary production command path.
- Keep suspended creation, strict Job attachment, no-window, bounded output,
  timeouts and cleanup paths. A diagnostic pipe writer closes after attachment;
  no Job bypass or breakaway flag is permitted.
- Reuse the original failed baseline and collect seven alternative rows for
  the three binary selectors. Each launch is bounded to five seconds and 4096
  bytes per stream. Baseline success starts no diagnostic launch.
- Log only static row/stage/error classes, numeric status, capture flags and
  typed `marker_matches`. Never print raw output, marker JSON, argv, environment
  or exception text. Compare system-message classes by text, not output length.
- A failed literal-argv baseline may collect ten fixed `/usr/bin/printf`
  comparisons on the same already-owned fixture and public project/pre-project
  dispatch path: ASCII, ASCII with NUL formatting, each original synthetic
  argument separately, and the original list without its empty argument.
  Preserve the full original list and absolute/PATH/relative executable checks.
  Each comparison keeps the five-second/4096-byte bounds and existing cleanup.
- Literal diagnostics log static mode/program/stage/row labels and typed
  `output_matches` only beside the existing numeric/class metadata. A complete
  matching baseline launches no probes; every failed baseline remains rejected
  even if all comparisons succeed. No account stage runs after that failure.

### 4. Validation & Error Matrix

| Condition | Required behavior |
| --- | --- |
| Foreign source, marker or run ownership | Reject before dispatch |
| Original complete marker matches exact owner | Continue ordinary pre-auth checks |
| Original fails, every alternative succeeds | Still fail original baseline |
| Wrong/partial/malformed marker or capture | Report false match, never account consent |
| Any diagnostic launch fails | Record its static failure and retain baseline rejection |
| Literal comparison succeeds after baseline failure | Diagnostic evidence only; never fallback success |

### 5. Good / Base / Bad

Good: identify a launch-condition difference through the same guarded runner.
Base: report unclassified output without copying it to logs. Bad: adopt a
successful alternate row as production recovery or infer the cause from bytes.

### 6. Tests Required

Windows `execution_host::` discovery must be nonempty and its ordinary tests
must execute. Cover fixed argv, original-snapshot preservation, all selector
combinations and rejection before launch. Tasks matrix tests assert baseline
reuse, seven unique alternatives, no fallback, zero probes after success, exact
owner/completeness checks and secret-safe diagnostics. Preserve native process
cleanup and actual WSL gates; all execution remains Actions-only.
The actual owned WSL test also executes registered/pre-project commands in a
directory containing spaces, quotes and shell metacharacters, checks literal
argv and relative/PATH/absolute executables, and proves missing/non-directory
cwd cannot dispatch a marker command. The private account envelope must prove
its own captured cwd through fixture evidence and reject a missing cwd before
account access. Passing matrix units alone is not this transport evidence.
Literal diagnostic regressions must cover all fixed plans, intact empty/newline
cases, exactly ten comparisons after failure, zero comparisons after success,
failed-baseline retention, complete byte matching, dispatch/capture failures and
synthetic-secret suppression. Their Windows filter must actually execute them.

### 7. Wrong vs Correct

Wrong: return success when the explicit-root or closed-pipe diagnostic works.
Correct: fail the original path, record the bounded difference, and require a
separately reviewed production or fixture fix with fresh Actions evidence.

## Scenario: Private Capture Lifecycle Diagnostics

### 1. Scope / Trigger

Use bounded test-only observations when an actual host cancellation fails but
the error alone cannot distinguish exit, stop, pipe draining and cleanup order.

### 2. Signatures

Private `cfg(test)` `trace_capture(started: Instant, action: impl FnOnce() -> T)
-> (T, CaptureDiagnostics)` stores six thread-local stage slots: Attached,
StopLatched, ControlWrite, Exited, TreeRetired and Drained. The actual WSL fixture
uses fixed lifecycle case labels and `WslReadinessObservation` on the same clock.

Windows test builds also expose `tasks_wsl_retirement_trace(started, action)
-> (T, TasksWslRetirementTrace)`, retaining first/last numeric retirement records
and a saturating call count. `TasksWslActiveProcesses` distinguishes Count from
QueryFailed; an absent retirement record is not either of those observations.
`tasks_wsl_root_scope(&TasksWslRoots, TasksWslRootRole, action)` supplies per-case
Private/Readiness root identity to the same observer. Its record contains only
typed membership/liveness, never the underlying process references.

### 3. Contracts

- Retain only numeric elapsed/exit/byte-count metadata, typed stop/control
  errors, fixed stage/case labels and boolean write/cleanup acknowledgements.
  No private pipe bytes, account identity, paths, argv, environment or exception
  text enters the diagnostic record. Format only failing actual assertions.
- Reset thread-local capture scope on success and unwind; reject nested traces.
  Preserve the action's result. Observations must not change production control,
  error mapping, Job ownership, stdin handling, deadlines or cleanup decisions.
- Readiness records the first probe start, final probe start/return, saturating
  numeric probe count and cancellation time using the capture's same Instant.
  Keep fixed-size storage and existing probe cadence; no new retry or fallback.
- A local tree retirement is not proof of Linux descendant cleanup. Cooperative
  cancellation still requires the existing typed host cleanup acknowledgement;
  missing/invalid/cleanup-failed replies remain CleanupFailed. A later cancel
  flag cannot relabel an already returned unstopped nonzero transport exit.
- Preserve every original lifecycle assertion and final descendant check.
  A failing result does not prove its later descendant check ran. Observed
  timing overlap alone does not establish cross-Job interference or OS cause.
- Only an active private test trace with a nonzero transport exit and false
  cleanup acknowledgement may classify native output. Decode at most 4096 bytes
  as valid UTF-8/UTF-16. Compare the COMPLETE message, allowing only BOM/newline
  framing, against fixed system-message IDs: base Win32 0..=1999, WinSock
  10000..=11004 and ten named standard HRESULTs, at most 4096 candidates.
  FormatMessageW uses FROM_SYSTEM | IGNORE_INSERTS | MAX_WIDTH_MASK and a fixed
  512-unit buffer. Match WSL's trusted resource rendering flags; outer CR/LF
  trimming does not replace its treatment of embedded regular line breaks.
  Do not adopt ALLOCATE_BUFFER or normalize the private input to force a match.
  The locked windows 0.61.3 Debug module does not export the width-mask macro.
  Use the private SDK_FORMAT_MESSAGE_MAX_WIDTH_MASK value 0x000000ff from
  WinBase.h; the independent flag regression pins the same literal value.
  Do not add a broad binding feature or rely on transitive feature unification.
  Retain Unknown or SystemMessageId with the matched numeric identifier only;
  no substrings, output-derived labels, guessed codes from length or raw text.
  A matching message identifier is not an independently observed OS return code.
- The shared Windows test observer queries JobObjectBasicAccountingInformation
  only in an active trace, records active-process Count or QueryFailed and
  same-clock times around the EXISTING TerminateJobObject call. Preserve its
  exactly-once invocation, arguments, result/error mapping and ownership state.
  No retained Job handles, process IDs/names, extra termination or breakaway policy.
- Root registration occurs only in an active Windows test scope, after the
  exact Child attaches to its guard. Duplicate its handle with only query-limited
  and synchronization rights; retain at most two non-inheritable OwnedHandles
  per case, replacing the previous same-role reference. Never reopen by PID,
  enumerate members or retain Job objects. Clear TLS on return/unwind.
- Immediately before the existing readiness retirement, query these exact roots
  against that exact Job with IsProcessInJob and zero-time WaitForSingleObject.
  Keep membership InJob/NotInJob/QueryFailed/Unavailable separate from liveness
  Alive/Exited/QueryFailed/Unavailable. A busy registry is Busy, a poisoned one
  QueryFailed; a missed registration invalidates the preceding reference.
  No peer waits, barriers or cadence/deadline changes. Per-case Arc ownership
  must end with the case; query references must never affect Job lifetime.
- First/final readiness records retain probe start AND return, plus their
  corresponding fixed retirement traces. Intermediate calls are counted, not
  accumulated in an unbounded list. A count of zero, failed query and missing
  observation remain distinct. No count establishes Linux cleanup by itself.

### 4. Validation & Error Matrix

| Condition | Required behavior |
| --- | --- |
| Cancel byte received, valid typed acknowledgement | Preserve Cancelled |
| Stop without acknowledgement or with cleanup-failed | Preserve CleanupFailed |
| Unstopped exit followed by later cancellation | Preserve original transport error |
| Earlier false readiness probe, later ready probe | Retain first timestamp and full bounded count |
| Action panics or another thread observes capture | Clear scope on unwind; isolate other thread |
| Payload contains a system message plus private text | Unknown; no substring classification |
| No active trace, successful exit or valid cleanup reply | Do not classify private output |
| Job count query fails or no retirement call occurred | Explicit QueryFailed or absent record, never invented zero |
| Root missing, registration missed or registry busy | Explicit Unavailable/Busy, never invented non-membership |
| Exact root belongs to another Job | NotInJob and an independent liveness observation; no Linux cleanup inference |

### 5. Good / Base / Bad

Good: same-clock typed observations distinguish unlatched exit from acknowledged
stop. Base: unknown cause remains unknown. Bad: make a failing gate green by
remapping nonzero exit to cancellation without host cleanup confirmation.

### 6. Tests Required

Actions ordinary Windows/Linux tests cover fixed metadata bounds, secret
suppression, result preservation, unwind/thread isolation, and compiled synthetic
child modes for acknowledged/unacknowledged cancellation and unstopped exit.
Windows tests cover first false-probe retention and counter saturation. Rebuild
the same-run synthetic gh fixture and execute the exact actual WSL test; unit
diagnostics alone do not prove the actual cancellation/descendant gate.
Windows coverage also compares complete system messages in supported encodings,
rejects malformed/oversized/embedded private payloads, preserves classification
guards, and covers retirement result preservation, unwind/thread isolation and
one real guarded nonzero child exit. First-probe retirement evidence must survive
later probes in fixed storage. The actual WSL scenario stays concurrent and
retains the original expected error and descendant-retirement assertions.
Root coverage uses Actions-only guarded native children to distinguish known
same/different Jobs and live/exited roots, preserve termination results and
prove bounded ownership, missing/busy/query-failed states and TLS isolation.
Catalogue coverage proves fixed order and cap, representative messages inside
and outside the former 26-ID list, and complete framing. Do not round-trip
every catalogue ID through a whole-catalogue scan or cache private payloads.
Pin the exact trusted rendering flags and independently compare representative
system messages using those flags, without assuming every locale has soft
line breaks. This renderer parity does not establish an actual transport cause.

### 7. Wrong vs Correct

Wrong: infer a cancellation race from elapsed time and accept absent cleanup.
Correct: retain the failure, record bounded ordering evidence, then review a
specific production correction and require fresh exact-SHA Actions evidence.
