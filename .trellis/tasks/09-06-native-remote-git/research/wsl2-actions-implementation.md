# WSL2 Actions Implementation

Status: RELEASED / FROZEN for Noether's source check and Main's Actions run.
No blocker remains in this source slice. No local script interpretation,
compilation, tests, probes, formatting, lint, whitespace checks, Git operations
or application launches ran. All automated verification remains UNRUN.

## Changed Files

- `.github/scripts/tasks_wsl_fixture.mjs`
- `.github/workflows/ci.yml`
- This handoff. No Rust, fixture-suite, dependency, user configuration or other
  source edits; unrelated dirty paths were not reverted.

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
current requested version, API/kernel success or a readable guest marker, so
partial imports and actual-version mismatches retain exact unregister cleanup.

## Exact Attestation

The bounded PowerShell/C# query uses System32-only `wslapi.dll` with this API:

```text
HRESULT WslGetDistributionConfiguration(
  PCWSTR distributionName, ULONG *distributionVersion, ULONG *defaultUID,
  WSL_DISTRIBUTION_FLAGS *flags, PSTR **environment, ULONG *count)
```

P/Invoke maps HRESULT to int, ULONG/flags to uint, and the returned environment
array to IntPtr. A finally block frees every returned string pointer and the
array with Marshal.FreeCoTaskMem without decoding/logging the values. Output is
only `{ "hresult": <integer>, "version": <integer> }`, bounded to 8192 bytes and
60 seconds. Strict UTF8/JSON framing, HRESULT zero, and exact expected version
are required. No localized version-table parsing or registry fallback exists.
The signature, configured-version semantics and freeing obligation follow
[Microsoft's API reference](https://learn.microsoft.com/en-us/windows/win32/api/wslapi/nf-wslapi-wslgetdistributionconfiguration).

WSL2 additionally boots the exact named/root-owned guest for `/usr/bin/uname -r`.
Its 1024-byte bounded output must be one strict UTF8 release string, at most 128
allowlisted ASCII characters, ending in `-microsoft-standard-WSL2` case-insensitively.
Only validated kernel/version metadata is logged. Rootfs preparation verifies
uname exists without altering the shared rootfs configuration. Attestation runs
after import and again after Cargo discovery/build, immediately before the exact
test invocation. An API, generation or kernel mismatch fails the gate.

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
