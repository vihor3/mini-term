# WSL Client Containment Implementation

Status: RELEASED TO MAIN / FROZEN for independent check. All automated
verification is UNRUN. No local build, metadata, test, fixture, probe, formatter,
lint, whitespace check, code generation, application launch or Git operation
was performed. Main owns contracts, CI/Git and integrated exact-SHA evidence;
Tesla owns the separate actual-fixture changes in Tasks tests.rs.

## Scope And Evidence

This implements Main's newly authorized production policy, not the earlier
timing-only diagnostic authorization. The source base supplied by Main is
551595b, the Actions-generated formatting successor of f1fde4d. Existing
formatting and unrelated edits were not reverted.

Main supplied positive timing evidence from CI 34030386551, actual WSL job
101478843292 on f1fde4d: Immediate's eligible empty probe completed at 135056us,
retired at 135091..135116us with Count(4), and its producer returned at 143955us
with exit 9 and only the start marker. AfterProducer's equivalent probe
completed at 134642us; its producer returned at 1086362us with exit 0 and exact
markers, followed by release at 1086394us and retirement at 1086405..1086410us
with Count(0). Both timing_issue values were null. Original DataCancel still
failed; owned cleanup passed at 11:40:53Z. This evidence was supplied by Main,
not independently rerun here. It supports retirement interference, not exact
kernel-member identity, WSL2 behavior or validation of this production patch.

## Changed Files And API

- `crates/mt-app/src/execution_host.rs`
- `crates/mt-app/src/tasks_account_executor/process.rs`
- This implementation report. No other files were edited in this slice.

Shared crate-private API:

```rust
enum ProcessTreePolicy {
    StrictTree,
    WslClientRoot,
}

ProcessTree::configure_with_policy(&mut Command, ProcessTreePolicy)
    -> Result<ProcessTree, CommandExecutionError>
```

`ProcessTree::configure` remains strict. On Windows, StrictTree configures only
JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE; WslClientRoot adds exactly
JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK. Unix configuration continues using the
unchanged process-group path. No binding feature or dependency was changed;
the new constant uses the existing pinned windows JobObjects namespace.

## Routing And Preserved Behavior

- Ordinary registered execution selects policy from the captured backend in
  the Process arm. SSH has no process policy and keeps its separate transport.
  Pre-project execution selects from PreProjectLocalContext. Executable/path
  text cannot select WSL policy. Public planner/execution signatures are intact.
- The attested test-only WSL marker launcher also passes the WSL policy after
  its existing typed ownership validation. The ordinary strict runner wrapper
  remains available to existing tests. The A/B timing module and wait hook were
  not changed or enabled in production; no diagnostic scope is armed here.
- Private run_wsl passes WslClientRoot explicitly to its own capture. Native
  unselected, lookup, identity-proof and selected-data captures pass StrictTree.
  The cooperative native envelope fixture also passes StrictTree: cooperative
  stdin is not policy authority. Tasks tests.rs was not edited.
- Suspended/no-window flags, exact assignment before resume, attachment failure
  cleanup, bounded direct-child reap, explicit TerminateJobObject, disarming and
  Drop/owned-handle closure are unchanged. No launch breakaway flag, extra kill,
  retained Job, production wait, retry, distro shutdown or configuration change.
- Private buffers, sanitization, envelope, control byte/EOF handling and cleanup
  acknowledgement checks remain separate and unchanged. Missing cleanup proof
  remains CleanupFailed; no error/result remapping was added.

## Authored Regressions

All five tests are authored UNRUN, alongside unchanged existing coverage:

- `execution_host::tests::process_tree_policy_follows_typed_sources_not_executable_or_path`
- `execution_host::tests::windows_process_tree_policies_set_exact_job_limits_and_keep_strict_default`
- `execution_host::tests::windows_process_tree_policies_keep_exact_root_attachment_and_retirement`
- `tasks_account_executor::process::tests::private_wsl_entry_rejects_non_wsl_sources_before_capture`
- `tasks_account_executor::process::tests::private_root_cleanup_preserves_failure_before_assignment_for_both_policies`

The Job-limit test queries the actual configured Job for default/strict/WSL
limits and pins the existing creation mask. The root test uses the existing
guarded native fixture ownership to inspect exact membership/liveness and
bounded retirement under both policies. Private cleanup tests keep failed
pre-assignment cleanup observable while confirming direct-root reaping.
These are not actual WSL peer-survival results. Private policy propagation was
also source-reviewed at every capture call; Tesla owns the actual host gate.

## Limits And Pending Gates

SILENT_BREAKAWAY_OK can let eligible Windows relay/interop descendants escape;
it is not selective for WSL infrastructure, and ancestor Jobs may constrain it.
This WSL path promises managed client-root cleanup, not complete Windows or
Linux descendant containment. Escaped pipe holders can still cause the existing
bounded drain error. Ordinary guest-stop acknowledgement is not introduced.
Existing Git timeout/loss uncertainty, exact-ID lease review and no-replay
behavior are untouched. Private Tasks still relies on its guest process group
and strict host acknowledgement, not the Windows Job, for guest cleanup proof.

Main must obtain a fresh independent check, exact-SHA Actions compilation,
formatting/lint/tests and Tesla's bounded ordinary/private concurrent WSL peer
fixtures, preserving every original lifecycle/descendant assertion and owned
distro cleanup. WSL2, enabled interop, nested Jobs and real-device/UI acceptance
remain separate risks; no passing production validation is claimed here.
