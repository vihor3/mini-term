# WSL Containment Fixtures

Status: RELEASED TO MAIN / FROZEN. Source review complete; all automation UNRUN.

## Exact Changed Paths

- `crates/mt-app/src/tasks_account_executor/tests.rs`
- `.trellis/tasks/09-06-native-remote-git/wsl-containment-fixtures.md`

No production, Git safety, private-capture, envelope, fixture executable,
rootfs, dependency, workflow, or spec files changed in this dispatch. Newton's
`execution_host.rs` and Tasks `process.rs` remain exclusively his source scope.
No children, Git commands, local checks, fixtures or app launches were run.
Existing formatting from the supplied `551595b` HEAD was left intact.

## Gate And Ordering

The existing single ignored gate remains:

`tasks_account_executor::tests::tasks_account_executor_wsl_sentinels_cleanup_and_foreground_host`

One call to `wsl_containment::assert_peer_survival` follows its entire original
lifecycle loop. The original owner/marker/hash/cwd/argv/account/privacy checks,
private 50ms/10s polling, lifecycle results and descendant assertions must all
pass first. Their bodies and the failure-only exact-once public A/B module are
unchanged. The new cases do not invoke any diagnostic timing/retirement scope.

The four ordinary rows cover Project and PreProject, each with Complete and
Timeout. Both case creation and commands retain the attested WSL source.
They now dispatch the retiring short worker FIRST, require its own fresh
`short-ready`, then dispatch the independent public peer and require
`peer-ready`. Before joining the short worker, both workers must still be
pending; elapsed time must also be below the short's fixed completion sleep
or timeout. Complete requires a normal empty-output exit 0; Timeout requires
the runner's actual timeout receipt, not an arbitrary transport error.

The four private rows cover data and lookup cancellation/timeout. They create
separate fresh UUID directories for the private request and public peer,
dispatch the private request FIRST, require its existing descendant `ready`,
then start the peer. At peer readiness, both workers must still be pending
and the private deadline must not have elapsed. Only then is cancellation
signalled, or the normal timeout awaited. The exact production Cancelled or
TimedOut result is required, followed by the unchanged `assert_retired`:
ready exists, release marker, one-second wait, absent orphan marker. Actual
readiness excludes pre-dispatch stops; the production executor's strict
host-reply acknowledgement remains the authority for dispatched stop results.
No acknowledgement is inferred by parsing diagnostic text or private bytes.

Every row then requires its peer still pending, before the peer's fixed
15-second sleep can have elapsed, and finally a complete non-timeout exit 0
with exact start/end stdout and empty stderr. This guards overlap both before
and after the earlier client's retirement. Public diagnostics contain only
static labels, typed errors, numeric exits/flags and byte counts, never bodies.

`Worker` is a small fixture-local join owner shared by the two worker types.
Unwind cancels pending private work before joining; the later peer also retains
that abort handle so a peer readiness failure cannot wait for the peer before
signalling private cancellation. Public execution has no cancellation API:
its immutable finite sleep and existing runner deadline bound the join.
Drop ignores a joined worker's panic to avoid a second panic during unwind.
No completed ProcessTree/Job guard is retained or artificially held.

## Exact Additional Bounds

Eight rows run once in order, with no case retry or successful substitute.
Every directory uses existing `WslFixture::case`, fresh UUID and mkdir without
`-p`; a collision fails rather than reusing state. Root ownership requires
matching attested distro and both original fixture-root paths before creation.

| Work | Count / Bound |
| --- | --- |
| Case mkdir | 12 calls, existing 5s command deadline |
| Ordinary short client | 4 calls, 5s deadline each |
| Complete short body | Ready marker plus fixed 2s sleep; peer overlap before 2s |
| Timeout short body | Ready marker plus fixed 10s sleep; peer overlap before 5s |
| Independent public peer | 8 calls, fixed 15s sleep and 20s runner deadline |
| Private request | 4 single-envelope calls; cancellation controls 15s, timeout controls 5s |
| Readiness | 16 loops, each at most 200 calls, 50ms interval and absolute 10s dispatch budget |
| Each readiness command | Existing 5s/4096-byte bounded fixture call |
| Private descendant postchecks | 12 calls total with existing 5s deadline, plus four 1s waits |
| Public capture | 4096 bytes per stream on all ordinary/setup/probe/peer calls |
| Private capture | Existing production wire/output caps, sanitation and strict acknowledgements |

At most 3,240 Windows-to-WSL adapter invocations are added: 3,200 readiness
probes plus 40 fixed calls. Four are private envelope calls; their synthetic
Linux subprocesses retain the existing bounded fixture/envelope behavior.
The independent readiness deadlines also limit actual probe counts. Ordinary
rows have at most 403 calls each; private rows at most 407 calls each.

Each readiness deadline can include one final in-progress 5s command, existing
cleanup grace and at most one final 50ms sleep. Existing ordinary 2s reap/drain
and private 2s control/cleanup/drain waits remain unchanged. The eight fixed
peer sleeps contribute about two minutes on a healthy runner, plus setup,
initial readiness and normal runner overhead. These are application budgets,
not a hard wall-clock guarantee over OS spawn, scheduling or API failures.
No workflow timeout was changed, no private baseline deadline was extended,
and no ordinary timeout is promoted to clean guest-stop proof.

## Authored Unit Tests: UNRUN

New ordinary Windows test names, under
`tasks_account_executor::tests::wsl_containment::`:

- `containment_requires_attested_root_before_case_creation`
- `containment_public_plans_and_limits_are_fixed`
- `containment_worker_ownership_joins_and_cancels_on_unwind`

They pin ownership refusal, immutable plans/caps/deadlines and join/cancel
ownership on normal and panic paths. The ownership test starts three bounded
synthetic threads, no processes/WSL calls; its sole polling loop has a 1s
deadline and 1ms interval. Unit coverage is not actual transport evidence.

## Review And Remaining Gates

Manual source review traced both typed ordinary APIs and the selected-account
executor, the before/after overlap assertions, strict result gate, existing
descendant checks, bounded joins, public-only diagnostics and the final call's
position after the original loop. No build, metadata, test, lint, formatting,
whitespace check, runtime probe or Git command was executed. All new cases
and tests remain UNRUN; Main owns independent review and exact-SHA Actions.

The supplied `f1fde4d` Actions A/B establishes the reported retirement-timing
difference for that owned run, not proof that these new cases pass. Creating
case directories and running prior tests can warm WSL; client-first ordering
does NOT prove cold startup or instance-parent ownership in any row. No distro
restart/shutdown is introduced. Kernel member identity, WSL2, external clients,
enabled interop, and escaped guest/Windows descendants remain separate risks.
Original DataCancel failure must be corrected and its original gate passed;
neither new fixture success nor owned-distro cleanup can waive that assertion.
