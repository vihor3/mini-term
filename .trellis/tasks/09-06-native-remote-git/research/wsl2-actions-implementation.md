# WSL2 Actions Implementation

Status: SOURCE RELEASED / FROZEN. Noether reviewed the Flags correction and Main
validated both actual WSL generations on Actions commit30dce5371fcbdd4936d174c12adfe7dd196807c3,
run34036603468. WSL1job101495870630 and WSL2job101495870628 each discovered and
executed the exact gate once with1 pass/0 failures/0 ignored and owned cleanup.
WSL2 attested kernel6.18.33.2-microsoft-standard-WSL2 and Flags15; WSL1 Flags7.
See the parent validation record for current ordinary-workflow status and limits.

At source handoff, automated verification was UNRUN and the first API-query
candidate had failed before either Rust test. No local script interpretation,
compilation, tests, probes, formatting, lint, whitespace checks or application
launches ran; all validation remained Actions-only.

## Changed Files

- `.github/scripts/tasks_wsl_fixture.mjs`
- `.github/workflows/ci.yml`
- This handoff. No Rust, fixture-suite, dependency, user configuration or other
  source edits; unrelated dirty paths were not reverted.

The Flags correction changes only the script and this handoff. The released
workflow matrix and public capability step are unchanged.

## Matrix And Ownership

The existing actual gate now has explicit Windows 2022 / WSL1 and Windows 2025 /
WSL2 rows with fail-fast false. Only those Windows rows set MT_TEST_WSL_VERSION
to exactly 1 or 2. Both consume the unchanged same-run shared rootfs artifact;
its provenance/owner/checksum checks and guest owner schema remain intact.
Separate runners retain the required mt-tasks-run-attempt name without collision.

Import validates the requested version before mutation, records it numerically
in `{owner,distro,installPath,version}` before dispatch, and passes it explicitly
to `wsl.exe --import ... --version`. Version/boot/owner failure cannot export the
test environment or select another distro/version. Cleanup validates only the
recorded owner/name/path and valid recorded version. It does not depend on the
current requested version, registry/kernel success or a readable guest marker, so
partial imports and actual-version mismatches retain exact unregister cleanup.

## Exact Attestation

The bounded PowerShell query uses read-only .NET RegistryKey APIs:

```text
Registry.CurrentUser.OpenSubKey(
  "Software\Microsoft\Windows\CurrentVersion\Lxss", false)
RegistryKey.OpenSubKey(subkeyName, false)
RegistryKey.GetValue("DistributionName", null, DoNotExpandEnvironmentNames)
RegistryKey.GetValueKind("Flags") == RegistryValueKind.DWord
RegistryKey.GetValue("Flags", null)
```

Require exactly one ordinal-exact owned DistributionName match. Only that
registration's Flags is read, with DWORD kind and signed Int32 representation
required, then bit-preserved into UInt32. Every opened registry key closes in
finally, including on missing/duplicate matches, invalid values and read errors.
No registry Version or environment value is queried, expanded or logged.

Output is only `{ "flags": <uint32> }`, bounded to 8192 bytes and 60 seconds.
JavaScript requires strict UTF8/JSON, exactly that one field, and an integer in
0..4294967295. It derives `(flags & 0x8) !== 0 ? 2 : 1` and requires the expected
generation; only the validated numeric flags/generation may be logged.
[Microsoft's VM_MODE constant](https://github.com/microsoft/WSL/blob/master/src/windows/service/inc/wslservice.idl)
and [registration flag handling](https://github.com/microsoft/WSL/blob/master/src/windows/service/exe/DistributionRegistration.cpp)
define this generation bit and preserve it across global flag overrides.
Registry Version and WslGetDistributionConfiguration's distributionVersion
describe filesystem/distro format, not WSL generation. The earlier P/Invoke
query is removed, with no COM retry or alternate-query fallback.

WSL2 additionally boots the exact named/root-owned guest for `/usr/bin/uname -r`.
Its 1024-byte bounded output must be one strict UTF8 release string, at most 128
allowlisted ASCII characters, ending in `-microsoft-standard-WSL2` case-insensitively.
Only validated kernel/generation/flags metadata is logged. Rootfs preparation verifies
uname exists without altering the shared rootfs configuration. Attestation runs
after import and again after Cargo discovery/build, immediately before the exact
test invocation. A registry, generation or kernel mismatch fails the gate.

## Preserved Gate And Limits

The unchanged Rust test remains exactly
`tasks_account_executor::tests::tasks_account_executor_wsl_sentinels_cleanup_and_foreground_host`,
discovered once and run with `--ignored --exact --test-threads=1`. Its original
lifecycle assertions and eight concurrent-peer rows are untouched. Existing
build/test/import/cleanup deadlines, output rules, strict guest marker/hash
checks and always-run exact cleanup remain. No new test framework or Rust test.

A WSL2-only workflow step logs public `wsl.exe --version` output and read-only
VirtualMachinePlatform feature name/state. No installation, update, feature
enable, reboot, default version/distro change, global shutdown or fallback.
The selected hosted image advertises WSL2 per Main's research, but nested
virtualization is not officially guaranteed. Actual import, guest boot, exact
suite success and cleanup must establish feasibility. Prior f4ee0f9 green WSL1
is not WSL2 evidence; Main must report the new matrix jobs independently.

## Main Post-Fix Analysis

### 1. Root Cause Category

Cross-layer contract and implicit-assumption errors in CI attestation, not a
new application regression. The initial managed API was callable but returned
E_ACCESSDENIED in both hosted jobs. Its version field was also misinterpreted:
Microsoft's source distinguishes distro/filesystem format from WSL generation.
The exact managed security cause is unproven and is not needed for the scoped
read-only registration fix.

### 2. Why The Initial Attempt Failed

The initial source review validated the ABI and memory handling but accepted
the terse API documentation's version description without checking the actual
enumeration implementation. Both imports succeeded, then attestation blocked
the unchanged Rust test before discovery. Querying registry Version instead
would have retained the semantic defect; Main caught that before committing it.

### 3. Prevention Mechanisms

Use the VM_MODE bit read from the exact owned registration through structured
read-only APIs, keep the independent WSL2 kernel proof, and require both actual
generation rows in Actions. Missing/malformed metadata and wrong generation
fail closed. Preserve the failed setup result, exact cleanup, and distinction
between environment attestation and completed test execution.

### 4. Systematic Expansion

The source search finds no production WslGetDistributionConfiguration caller
to change. No COM security changes, generic WSL subsystem redesign, registered
environment reads or new fixture framework are warranted. Native mini-term has
no matching Trellis source-template tree to synchronize.

### 5. Knowledge Capture

The release-staging contract now records exact-name/DWORD/UInt32 validation,
the fixed flags payload, VM_MODE semantics, kernel and output bounds, and the
prohibition on generation inference from Version. The parent progress file
retains both failed first-run job IDs and successful owned cleanups. The second
Actions run is the source of current validation evidence; source review alone
cannot mark WSL2 or native acceptance complete.
