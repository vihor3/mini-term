# Research: WSL Instance Retirement

- Query: Map ordinary/private process ownership and propose the smallest public causal A/B for per-command Job retirement versus unrelated WSL early exit.
- Scope: mixed; local source/contracts and Microsoft Job API semantics. Upstream WSL investigation and Actions inspection remain Main's work.
- Date: 2026-09-06
- Status: RELEASED TO MAIN / FROZEN. Main selected the bounded on-stack deferral; Main owns the narrow exception and implementation dispatch. No code changed or diagnostic executed. Noether separately owns conditional production-guarantee review.

## Findings

### Recommendation

Main selects the **two-row, single-probe, bounded on-stack pre-retirement wait**.
A retires normally; B retains ONLY its completed public readiness ProcessTree
on that thread's existing stack until its fixed producer returns, or the fixed
deadline. Root exit and pipe drain already happened; explicit termination and
fallback Drop/kill-on-close have not. This directly tests retirement timing
without changing Job containment. No flag contrast is included in this slice.

Both public rows use exactly one probe, eligible for B on normal exit 0 OR 1.
Deferring only the final successful READY probe in a polling loop is insufficient:
an earlier false probe may already retire the instance. One probe per row avoids
that confound and any retained-Job collection. All original private polling,
control, deadlines, assertions and production behavior remain unchanged.

| Option | Value And Limitation |
| --- | --- |
| Bounded final-retirement deferral, SELECTED | Separates observed root exit from final retirement while preserving attachment, limits and descendant containment. One on-stack guard, fixed deadline, unchanged final termination/Drop; both public rows share the same single-probe harness. |
| Silent breakaway, NOT SELECTED | Less wait coordination, but changes which descendants retirement can reach. It is not selective for startup services and can weaken command/interop cleanup. Keep it only as a conditional later production design question after causal proof and Noether's guarantee review, not a flag experiment or approved production fix in this dispatch. |

### Evidence And Interpretation

Main supplies the exact-head evidence for `35fe653`,
[run 34026742549 / job 101469109594](https://github.com/vihor3/mini-term/actions/runs/34026742549/job/101469109594):

- LookupCancel's first readiness return: 103183 us; retirement: 103113..103145 us;
  Job Count(8), private root NotInJob/Alive.
- Private exit: 113517 us, code -1; return: 119686 us, HostHelperUnavailable;
  cancellation only at 236160 us, false acknowledgement. Private bytes were
  never logged and must remain private.
- The isolated public producer also exited -1, with start but no end marker and
  the strictly decoded public text `The Windows Subsystem for Linux instance has terminated.\r\r\n`.
  Its readiness probe exited 0 at 103400 us. This is not a credential-dependent
  reproduction and already uses null stdin.
- DataCancel and LookupTimeout passed earlier by source order. The failed-case
  descendant assertion was not reached. Exact owned-distro cleanup passed;
  neither observation turns the full WSL gate green.

Main's separate primary-source finding is a stronger lifetime hypothesis:
`LxssInstance.cpp:776` opens the calling process with PROCESS_CREATE_PROCESS and
SYNCHRONIZE and supplies it to LxssClientInstanceStart;
`common/lxssclient.cpp:174` describes ParentProcessHandle as the instance parent
and forwards StartParentProcessHandle to LXBUS_IOCTL_SET_INSTANCE_STATE.
This note accepts that supplied finding without repeating upstream research.
It does NOT establish kernel Job inheritance, which process was affected, or
that every private failure has the same cause. NotInJob for one private root
does not exclude disruption through instance infrastructure; Count(8) is not
an inventory or Linux descendant proof.

### Files Found And Ownership Patterns

Paths below are relative to the repository; line numbers describe source read
during this research and can move with concurrent edits.

| File / Pattern | Ownership And Consequence |
| --- | --- |
| `crates/mt-app/src/execution_host.rs:279`, `:337`, `:368` | Ordinary registered/pre-project WSL plans use explicit distro, fixed launcher root and encoded captured cwd; both converge on generic process execution. |
| `crates/mt-app/src/execution_host.rs:1277`, `:1308` | `run_process` chooses null stdin; each invocation configures, spawns and attaches its own ProcessTree and owns its output readers. Public mkdir/producer/probe are separate invocations. |
| `crates/mt-app/src/execution_host.rs:936`, `:985`, `:1014`, `:1083` | Windows creates an owned kill-on-close Job; child starts suspended/no-window, is assigned, then resumed. `terminate` explicitly calls TerminateJobObject(job, 1); successful termination sets `terminated`. Drop retries if needed, then the owned handle closes. |
| `crates/mt-app/src/execution_host.rs:1382`, `:1414`, `:1420` | Normal return requires observed child exit and finished/joined readers, THEN explicit Job termination, even after exit 0 or 1. Timeout/error paths separately call cleanup, with existing bounded reap/drain waits. Holding a duplicate handle alone cannot prevent this explicit retirement. |
| `crates/mt-app/src/tasks_account_executor/process.rs:40`, `:644`, `:669`, `:708` | PrivateBytes zeroizes and is non-Debug/non-Clone. `run_wsl` builds/sanitizes its own command and calls private capture, NOT generic run_process. OwnedChild owns its Child plus a separate instance of the same ProcessTree type. |
| `crates/mt-app/src/tasks_account_executor/process.rs:801`, `:827`, `:854`, `:903` | Cooperative capture keeps piped control stdin; a latched stop writes one byte. After observed exit it retires the owned tree before bounded drain. A stop without valid cleanup acknowledgement remains CleanupFailed. Unstopped nonzero transport exit maps to HostHelperUnavailable at `:663`, not later cancellation. |
| `crates/mt-app/src/tasks_account_executor/host_envelope.py:66`, `:79`, `:104` | The private Linux envelope gives gh a new session, owns process-group cleanup and bounded wait, and treats control byte/EOF as cancellation. A local Job result is not this host cleanup acknowledgement. |
| `crates/mt-app/src/tasks_account_executor/tests.rs:2068`, `:2089`, `:2154`, `:2182` | PublicCase checks attested distro/root, creates a fresh UUID without mkdir reuse, constructs fixed commands, and currently polls readiness while the caller independently runs producer then joins readiness. |
| `crates/mt-app/src/tasks_account_executor/tests.rs:2245`, `:2278`, `:2328`, `:2357` | Exact-once typed trigger, producer-only preview ownership, full both-stream UTF8/UTF16 sentinel scan, strict decoding and bounded escaped JSON. Preserve these boundaries. |
| `crates/mt-app/src/tasks_account_executor/tests.rs:3444`, `:3482`, `:3509` | Private result/timestamps and canceller join precede the public attempt; original readiness/result/descendant assertions remain afterward. No diagnostic result may replace them. |

### Selected Exact Plan

1. Keep the current exact-once trigger after unexpected pre-cancellation
   DataCancel/LookupCancel HostHelperUnavailable, after private result/timestamps
   and readiness join are captured. One diagnostic attempt contains A then B,
   once each, never retries-until-success. Each row uses its own fresh UUID under
   the existing attested PublicCase/mkdir boundary. At most six host commands:
   two mkdir, two fixed producer, two fixed readiness. No extra ownership reads.
2. Preserve the immutable SCRIPT, null stdin, captured cwd, argv, creation/Job
   flags, strict attachment/resume and readiness-before-producer thread dispatch
   order. EACH public row executes exactly one `/usr/bin/test -f ready`, using
   the existing typed recorder once. The deliberate public harness reduction is
   identical in A/B; original PRIVATE 50-ms polling and 10-second readiness stay
   untouched. No startup barrier, warmup, restart, script change or extra sleep.
3. Arm a one-shot Windows-test-only scope around only the exact public readiness
   dispatch. After the existing generic runner observes child exit and joins
   both output readers, record that completion boundary. Only B waits, and only
   on a complete, nontruncated, empty-stream normal exit 0 OR 1. Spawn, attach,
   read, timeout and other error paths retain immediate existing cleanup and
   make the comparison inconclusive. Producer/setup/private paths never wait.
4. The caller records producer return and signals `Returned` BEFORE joining
   readiness. A producer-completion RAII guard signals `Aborted` on unwind;
   readiness ownership joins normal/error/unwind paths without a second panic.
   Producer does not depend on readiness, so there is no feedback cycle. Use a
   timed completion-state wait, never an unbounded receive.
5. The absolute B release deadline is `pair_started + 10 seconds`, not extended
   by wakeups. On producer return, owner abort/drop or deadline, ALWAYS proceed
   to the SAME existing `process_tree.terminate()?`, then ordinary Drop/owned
   handle closure. Do not mark terminated early or return a provisional result.
   Keep the entire guard on-stack: neither a duplicate handle alone nor skipping
   explicit termination while immediately dropping the guard would work.
6. At most ONE Job is delayed, with fixed-size synchronization and observations.
   All original 5-second/4096-byte command bounds and 2-second cleanup waits stay.
   Timeout/abort are typed inconclusive outcomes, not passing evidence. On any
   return/unwind the original guard remains cleanup authority; explicit API
   failure retains its error and existing Drop retry/last-handle closure. No
   termination override, Job-limit change, guard transfer or indefinite handle.
7. Reuse existing typed retirement traces and public preview boundary. Record
   row, probe start/completion/return, producer start/return, release reason and
   actual retirement times/count/result. Distinguish completed probe from delayed
   B return. Keep both reports within the existing 4096-byte total diagnostic cap
   and full-both-stream sentinel suppression before previews. Never accept any
   private bytes or replace the original private readiness/result/descendant
   assertions. Existing exact owned-distro cleanup remains mandatory.

### Exact Code Boundaries

- `crates/mt-app/src/execution_host.rs`: a `cfg(all(test, windows))` one-shot
  scoped wait/observation immediately before normal final retirement near `:1420`,
  after root exit and joined readers. Inactive/non-test paths are unchanged.
  Leave ProcessTree configure/attach/terminate/Drop and error cleanup unchanged;
  no new general runner, public API, runtime switch or process policy.
- `crates/mt-app/src/tasks_account_executor/tests.rs`, nested
  `wsl_public_comparison`: fixed A/B single-probe rows, completion/join RAII,
  normal-exit 0/1 eligibility and bounded combined reporting. Attested constructor
  and fixed plans remain the only entry. Keep private lifecycle assertions and
  WslFixture helper behavior unchanged.
- Focused Actions-only checks in these modules: inactive-scope parity; two-row/
  one-probe bounds; exit 0 AND 1; error bypass; return-before-join; deadline and
  producer/probe unwind; TLS reset/isolation; successful final retirement and
  failure cleanup; unchanged results and preview privacy/caps. No CI, private
  capture, envelope, fixture executable, dependency or other product changes.

### Causal And Cleanup Limits

- **Positive support:** A reproduces the public instance-terminated failure with
  overlapping completed probe and normal retirement; B reaches that completion
  boundary before producer return, waits, and producer exits 0 with exact start/
  end markers before release and successful final retirement. This distinguishes
  root exit from immediate final Job retirement in the owned fixture. It does
  not separate explicit TerminateJobObject from kill-on-close: both are delayed.
- **Against the held probe's retirement as initiator:** B's producer still exits
  early while an eligible overlapping probe Job is held and no earlier probe
  exists. That probe's final retirement has not yet caused the exit; this does
  not exclude other instance-start/retirement causes.
- **Inconclusive:** A fails to reproduce, no overlap, probe transport/output
  error, deadline/abort, cleanup failure or materially different startup order.
  Exit 1 is eligible absence metadata, not readiness success. A first-false row
  without producer start evidence is not automatically the supplied start-then-
  terminated failure. No A/B outcome can manufacture private readiness evidence.
- A/B controls code and command shape, not the post-failure distro's startup
  state or scheduling. One differential result is causal support, not proof of
  kernel inheritance, service identity, a production fix or cleanup guarantees.
  The guarantee is that no ordinary return/unwind skips final retirement/closure;
  successful OS cleanup cannot be guaranteed after API failure or runner loss.
- Keep exact-SHA nonempty ordinary Windows/Tasks tests, the original actual
  ignored WSL gate and exact owned cleanup. A successful candidate cannot pass
  the original failing private assertion, establish its unreached descendant
  check, or validate WSL2, native accounts, user interop or UI acceptance.

### Conditional Production Integration Points

Only if causal evidence supports WSL-specific lifetime containment. Noether owns
the separate production-guarantee review; these are integration locations only:

- Ordinary commands: distinguish WSL while `execute_host_command` still has
  `snapshot.backend` (`execution_host.rs:383`) and while
  `execute_pre_project_local_command` still has PreProjectLocalContext (`:343`).
  Both currently erase WSL identity into a generic Process plan. A small internal
  WSL execution path can preserve distro identity and reuse current planning,
  quoting, capture bounds and request cleanup. Do not infer WSL from program text
  inside generic ProcessTree or change native/SSH behavior.
- Private commands: enter the same WSL-specific lifetime boundary from
  `tasks_account_executor/process.rs:644`, before its existing private capture.
  Keep private buffers, sanitized environment, control pipe, host envelope and
  acknowledgement parser separate. A change only to ordinary run_process cannot
  cover private WSL capture; a global ProcessTree change would affect native
  credential subprocesses as well.
- The design question is separating instance-start lifetime from short request
  cleanup while retaining provable request-descendant retirement. Do not turn the
  diagnostic wait into production delays, indefinite retained Jobs, a blanket
  no-Job mode or shared per-distro kill-on-close policy: user terminals/background
  work may share that distro. Exact containment mechanism needs separate review;
  no UI/Git domain rewrite, account remap, blind breakaway flag or distro-wide
  termination is justified by this A/B.

Silent breakaway remains conditional: it can spare instance-start infrastructure
but is not service-specific, and owned-distro cleanup is not proof of escaped
Windows-child or Linux-request retirement. Disabled fixture interop cannot prove
safety for user interop. No such production flag or guarantee is approved here.

### External References And Related Specs

- Microsoft documents explicit Job termination as affecting associated processes
  and nested child Jobs; keeping another handle does not prevent that API call.
  [TerminateJobObject](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-terminatejobobject).
- Separately, KILL_ON_JOB_CLOSE terminates associated processes when the last Job
  handle closes. Delaying only explicit termination and then dropping the guard
  is therefore insufficient. The same reference defines SILENT_BREAKAWAY_OK and
  the constraints imposed by nested ancestor Jobs; it does not promise which
  WSL implementation processes inherit or escape.
  [JOBOBJECT_BASIC_LIMIT_INFORMATION](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information).
- Local binding reference: `Cargo.lock:8873` pins windows 0.61.3; the proposed
  diagnostic requires no new Windows API, binding feature or dependency.
- Read `.trellis/workflow.md`, `.agents/skills/trellis-before-dev/SKILL.md`, active
  `prd.md`/`design.md`/`implement.md`, mt-app index/quality/error/logging guidance
  and shared guide context. Quality guidance makes every check Actions-only.
- Relevant contracts: `.trellis/spec/mt-app/backend/release-staging-contract.md`
  (actual fixtures, private lifecycle, public comparison);
  `github-project-tasks-contract.md` (private account/cleanup boundaries);
  `git-host-contract.md` (WSL captured directory and guarded launch).
- Child review: `.trellis/tasks/09-06-native-tasks-gh-accounts/review.md:9`
  documents the approved public-only boundary; its `ci-review.md` documents
  exact discovery, attestation and owned cleanup. Existing contracts forbid
  retained Jobs/peer waits outside approved diagnostics. Main selected this
  exact bounded exception and owns contract coordination/implementation dispatch;
  this researcher edits no spec or source. No breakaway exception is selected.

## Caveats / Not Found

- No code, CI, spec or Git operation occurred; only this research file was written.
  No local compilation, metadata, tests, fixtures, lint, formatting, probes,
  transport/credential operations or app launch. Verification remains unrun.
- Only implement/check JSONL manifests exist at the active task; they were not
  loaded because researcher role isolation forbids those manifests. Named task
  artifacts and relevant contracts/reviews were read directly instead.
- HEAD/run facts and upstream instance-parent findings above are explicitly
  supplied by Main, not independently reverified here. Concurrent edits were
  preserved; reread current semantic locations before implementing.
- WSL Job inheritance, identity of instance infrastructure and the failed
  private case's descendant cleanup remain unproven. No PID/member inventory,
  raw private logs, weakened cancellation remaps or removed assertions proposed.
  No flag experiment or production correction is included. Selected diagnostic
  is unimplemented/unrun here. Report is released and frozen; no further research
  or CI wait is required for Main's narrow dispatch.

## Main Post-Fix Analysis: WSL Client Ownership

This addendum follows the historical research release above. Main read the
completed Actions logs: the f1fde4d timing contrast supported retirement
interference, and the production correction on f4ee0f9 passed the exact actual
WSL1 gate, including every original assertion and eight appended peer rows.
It ran once in149.54s and cleaned its owned distro. Final Linux/Windows CI,
all five actual SSH fixtures and packaging also passed on that product commit.

### 1. Root Cause Category

- E, Implicit Assumption: a command's Windows descendants were treated as
  exclusively command-owned, although WSL can involve shared instance lifetime.
- B, Cross-Layer Contract: owning/retiring the Windows client is not equivalent
  to owning or proving termination of Linux guest work.
- D, Test Coverage Gap: isolated command/root checks do not establish that
  another concurrent WSL client survives retirement. Root nonmembership also
  cannot rule out indirect interference through shared infrastructure.

### 2. Why Earlier Changes Were Insufficient

Captured-directory and empty-launcher-argument corrections addressed separate
observed entry failures, not concurrent retirement. Native error rendering and
root-membership observations were useful bounded evidence but did not isolate
cleanup timing. A fixed non-credential producer and a single-probe timing
contrast separated normal client exit from final Job retirement without logging
private bytes, serializing production or substituting successful diagnostics.

### 3. Prevention Mechanisms

| Priority | Mechanism | Action / Status |
| --- | --- | --- |
| P0 | Typed ownership | Strict native default; explicit WSL policy in both ordinary entries and private capture, committed |
| P0 | Concurrent integration coverage | Original actual WSL assertions plus eight client-first/overlap/peer-survival rows, passed on f4ee0f9 |
| P0 | Honest uncertain writes | Known WSL launcher loss retains exact Git lease after reconciliation; exact-candidate Linux/Windows regressions passed |
| P0 | Private cleanup authority | Preserve Linux group cleanup and strict acknowledgement; original and appended actual WSL checks passed |
| P1 | Compatibility limits | WSL2, interop, nested Jobs and real native acceptance remain separately open |

### 4. Systematic Expansion

The registered, pre-project and private background paths were audited together.
Interactive PTY and SSH behavior was deliberately not expanded. Silent breakaway
does not selectively exempt infrastructure; escaped relay/interop descendants
remain an explicit limitation. No new ordinary guest-stop acknowledgement,
global distro lifetime manager, retry or universal compatibility claim follows.

### 5. Knowledge Capture

- Updated and committed git-host-contract.md for typed client ownership and
  launcher-loss uncertainty, plus github-project-tasks-contract.md for private
  cleanup boundaries. The release contract retains the narrow diagnostic rules.
- Captured exact fixture budgets and cold-start/OS-scheduling limits in the
  production contract and wsl-containment-fixtures.md.
- Kept unrelated dirty shared guides untouched. This native application has no
  matching Trellis template-source tree to synchronize; no template was invented.

Evidence: [timing contrast](https://github.com/vihor3/mini-term/actions/runs/34030386551/job/101478843292)
and [actual production WSL1 gate](https://github.com/vihor3/mini-term/actions/runs/34032734882/job/101485351593).
