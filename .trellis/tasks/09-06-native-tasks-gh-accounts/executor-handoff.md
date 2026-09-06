# Tasks Account Executor Handoff

Executor slice is SOURCE-COMPLETE, NOT VERIFIED. Final source reading and the
dispatched sentinel fixture authoring are complete. Ownership is released for
main's independent review and integration. All compilation, tests, formatting,
lint, whitespace checks, probes and fixture execution are UNRUN and Actions-only.
The previously published app API signatures and error variants are unchanged.

## Shared Helper Resolved

Turing exposed the existing `ProcessTree` type and `configure`, `attach`, and
`terminate` methods as `pub(crate)` on Unix and Windows. This executor reuses
that dedicated process group / suspended-spawn Windows Job ownership while
capturing credential stdout/stderr in private non-Debug buffers. No ordinary
`run_process` / `CommandOutput` path handles named credential lookup. This slice
did not edit `execution_host.rs`, any mt-ssh library, or any Git owner source.

Only new `mod tasks_account_executor;` beside the `terminal_area` registration
in `main.rs`, and new `mod tasks_accounts;` plus its named export in
`remote_ssh/mod.rs`, are reserved for this slice. Turing's Git module lines
will be preserved.

## Finalized App API

- `AccountCancellation`: clonable cancellation signal, explicitly cancelled
  when the UI invalidates a request. Worker cancellation does not mutate UI.
- `AccountExecutionControl::new(timeout: Duration, cancellation:
  AccountCancellation, expected_connection_epoch: Option<u64>) ->
  Result<Self, AccountExecutionError>` validates a 100ms..120s bound. None is
  only for the pipeline's first observed connection; pass each observed epoch
  into every subsequent capability/enumeration/selected request.
- `AccountHostResult<T> { result: Result<T, AccountExecutionError>,
  observed_connection_epoch: Option<u64> }` preserves epoch on errors too.
- `probe_account_capability(&ProjectExecutionSnapshot, AccountCapability,
  &AccountExecutionControl) -> AccountHostResult<()>` returns a verified
  capability result with its epoch, not help text or credential output.
- `discover_accounts(&ProjectExecutionSnapshot, &str, &AccountExecutionControl)
  -> AccountHostResult<KnownGitHubAccounts>` returns parsed
  `KnownGitHubAccounts` with its epoch and sanitized enumeration environment.
- `execute_selected_account(&ProjectExecutionSnapshot, &SelectedAccountRequestPlan,
  &AccountExecutionControl) -> AccountHostResult<CommandOutput>` returns only the data
  command's nonsecret result with its epoch. Domain `SelectedAccountRequestPlan`
  is the only accepted selected request input; there is no arbitrary command or
  secret/environment-map argument.
- Execution failures are static categories; no raw credential errors or SSH
  acquisition diagnostics are retained or displayed. App retains exact source,
  repository, account/auth generations, request IDs, and publication fences.

Native credential lookup uses Rust-private bounded pipes. WSL/SSH lookup and
apply the credential wholly inside a host-side envelope, with
explicit capability failure if the required isolated host helper is absent.
No global auth switch/login, local fallback, or credential file is permitted.
WSL/SSH require Python 3.8+ (stdlib only, isolated `-I -c` invocation, no file
installation). The app gets an explicit `HostHelperUnavailable` category when
it is absent. The envelope monitors its control pipe, owns child process groups,
and has its own deadline; the client sends a cancellation byte before closing.
Native uses the shared ProcessTree owner. List/detail perform pre/post identity
proof using the SAME captured credential, not separate ambient-auth requests.

Main approved the visibility-only helper request; Turing's Unix/Windows exports
are now present and reused without editing `execution_host.rs` in this slice.
The domain is source-complete and released for dependent review (20 UNRUN tests);
see `domain-handoff.md`. Source completion is not verification evidence.

## Files Written

- NEW `crates/mt-app/src/tasks_account_executor/mod.rs`: stable purpose-specific
  API, backend dispatch, validated context, host envelope plan and bounded
  allowlisted reply protocol. Public plans/results do not carry tokens/env maps.
- NEW `crates/mt-app/src/tasks_account_executor/process.rs`: Rust-private secret
  capture, child-only auth environment, pre/data/post proof with one credential,
  bounded output, explicit cancellation, ProcessTree cleanup and WSL transport.
- NEW `crates/mt-app/src/tasks_account_executor/host_envelope.py`: embedded
  Python 3.8+ POSIX host credential owner. Never installed or persisted by the
  feature; credentials remain on the WSL/SSH execution host.
- NEW `crates/mt-app/src/tasks_account_executor/gh_fixture.rs`: standalone,
  std-only synthetic gh executable compiled by Actions tests, not a crate module.
- NEW `crates/mt-app/src/tasks_account_executor/tests.rs`: executable native and
  POSIX fixtures, protocol coverage, and the shared SSH fixture exercise.
- NEW `crates/mt-app/src/remote_ssh/tasks_accounts.rs`: authenticated pooled SSH
  channel, same-pipeline epoch fencing, cancellation byte/cleanup acknowledgement,
  bounded stdout/stderr consumption, exact-session retirement and ignored SSH test.
- `main.rs`: only `mod tasks_account_executor;` belongs to this slice.
- `remote_ssh/mod.rs`: only `mod tasks_accounts;` and its named executor export
  belong to this slice. All Turing/Agent/other-owner edits remain intact.
- Child `domain-handoff.md` and this `executor-handoff.md`. No Tasks UI/config,
  store, manifests/locks, specs, workflows, staging, commits or rollback edits.

Native and host-envelope discovery project only validated account identity,
active/state and static diagnostic categories. Extra token fields and original
account error text do not enter returned enumeration data or SSH/WSL replies.
Selected lookup keeps exact discovered login spelling. GH_TOKEN is used only
for github.com and subdomains of ghe.com; other hosts use GH_ENTERPRISE_TOKEN.
Inherited auth/host/debug/tracing overrides are removed for capabilities,
enumeration, lookup and data children. No switch/login or active-user fallback.
Credential lookup failures distinguish named-user capability/store evidence
from ambiguous lookup failure without retaining raw credential errors.

## Executable Fixture Setup (Actions Only)

`tasks_account_executor::tests` includes four native process tests without a
Unix-only cfg. They execute the production private capture / ProcessTree path
on Windows as well as Linux, including successful descendant retirement,
timeouts, cancellation, two selected accounts, inherited auth/debug overrides,
credential lookup errors, wrong identity proof, and data-stage token echo.
An additional Windows-only test configures a real suspended child/Job and
exercises pre-assignment failure cleanup, proving the unassigned child never
resumes and is reaped. Descendant tests wait for an actual ready marker before
cancelling, then release a marker-writing descendant to detect cleanup failures;
both credential-lookup and authenticated-data descendants are covered.

The tests compile `src/tasks_account_executor/gh_fixture.rs` as a standalone
synthetic `gh`/`gh.exe` with the runner's `RUSTC` (or `rustc`) into an Actions
temporary directory. No additional helper build workflow step is required.
The fixture constructor refuses execution outside `GITHUB_ACTIONS=true`.
It never invokes an installed real gh or reads real credentials. Run via:

```text
cargo test --locked --target x86_64-pc-windows-msvc -p mt-app --bin mini-term tasks_account_executor
```

Linux workspace tests also execute three POSIX Python-envelope tests with the Rust gh
shim first in the child PATH, shell tracing initially enabled, inherited auth
and debug variables, credential/data failures, and descendant cancellation.
Those envelope fixtures require the Actions runner's Python 3.8+ and sh. They
cover actual tracing, extra token fields, categorized errors, absent helper,
and inherited-pipe descendant cleanup, but do not execute `wsl.exe` or a WSL
distro. The actual authenticated SSH fixture below is a separate required gate.
All fixture execution, fixture compilation, formatting, lint, and checks are
UNRUN here and must remain in Actions.

## SSH Fixture Coordination (Required Actions Step)

The Tasks fixture reuses Turing's read-only `remote_ssh::loopback_ssh_fixture`
configuration (implemented in `git_ops.rs`), without modifying its
source or `execution_host.rs`. Main must run the targeted ignored integration
test explicitly after preparing the same disposable sshd, isolated client HOME,
key, fixture marker and `MT_TEST_SSH_{ROOT,KEY,PORT,USER}` variables:

```text
cargo test --locked -p mt-app --bin mini-term remote_ssh::tasks_accounts::tasks_account_executor_ssh_sentinels_cleanup_and_epoch_pipeline -- --ignored --exact --test-threads=1
```

The fixture compiles the synthetic Rust gh itself and installs it ONLY into
`$MT_TEST_SSH_ROOT/bin/gh` on the runner. The loopback sshd must put that bin
directory first in its server-side PATH (followed by `/usr/bin:/bin`, including
Python 3.8+), and should use `SetEnv` to seed `GH_TOKEN=inherited_fixture_secret`,
`GH_DEBUG=api`, `DEBUG=1`, `GH_HOST=wrong.invalid`, and `GH_REPO=wrong/repo`.
A nonsecret `command -v gh` check must prove the exact fixture path before any
account command. No real GitHub CLI or credential is invoked by these tests.
The actual SSH test runs capability probes, mixed valid/broken enumeration,
selected A/B and GHES routing, static lookup/proof/echo errors, hostile worktree
quoting, a session retired while data is active, rejection of the replacement
epoch, data timeout/cancel and credential-stage cancel. Test instrumentation
inspects actual channel stdout AND stderr for the synthetic sentinel.
It uses the production exported executor API and host envelope, not a second
transport framework. Standard requests are bounded at 15s, timeout cases at 5s,
descendant readiness at 10s, control writes/channel close at 500ms, cancellation
acknowledgement at 2s, and marker observation at 1s. Main confirmed a 5-minute
Actions timeout for this exact test.

This test is intentionally ignored in the ordinary workspace run because sshd
setup is external; running only the workspace suite is NOT the SSH gate. Main
confirmed the isolated server HOME/PATH/SetEnv and key setup in the draft CI.
Preserve CARGO_HOME/RUSTUP_HOME before changing client HOME (as the draft does),
or set RUSTC to the runner's absolute compiler. The test compiles the Rust shim
itself; no additional helper-build step or side-effect dependency install exists
in the feature. Ten test functions are authored across platforms: Windows runs
six nonignored cases; Linux runs eight nonignored cases plus this ignored case.

## Remaining Gates And Limits

1. Independent source/security review, then exact-SHA Actions compilation,
   complete-file formatting, lint and the native/SSH fixture commands above.
   Domain's 20 tests are still UNRUN. No passing workflow evidence is claimed.
2. WSL's actual `wsl.exe` stdin/exit/process-retirement behavior still requires
   an Actions-owned disposable-distro acceptance case; current POSIX fixtures
   prove the host-envelope component only. Do not infer a WSL transport pass.
3. App/config owner integrates selected-account state, origin/source/request
   generations, cancellation on invalidation and UI error states using the
   unchanged API. Every stage after first connection observation passes that
   epoch, including capability/enumeration errors. Never retry on another user.
4. Native needs no Python. Missing/old/non-POSIX host Python yields explicit
   HostHelperUnavailable for WSL/SSH, with no local fallback or automatic install.
   Real noninteractive secure-store compatibility is not established by shims.
5. Credentials are excluded from owned logs/results/argv/persistence. Rust
   private buffers are cleared on drop; std process environments and Python
   strings have OS/runtime-managed copies, not a guaranteed locked/zeroized heap.
   The installed host gh/Python/OS remain trusted; arbitrary malicious executables
   and privileged process inspection are not a supported secrecy boundary.

Pinned russh channel semantics were read from its
[0.61.2 channel source](https://raw.githubusercontent.com/Eugeny/russh/v0.61.2/russh/src/channels/mod.rs)
and [writer source](https://raw.githubusercontent.com/Eugeny/russh/v0.61.2/russh/src/channels/io/tx.rs).
No local executable/check/probe, real gh account command, stage or commit ran.
