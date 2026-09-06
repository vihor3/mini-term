# WSL2 Actions Review

Status: FLAGS REPLACEMENT SOURCE REVIEW COMPLETE / RELEASED / FROZEN.
No concrete findings or source blockers in the final `validateWslVersion`
replacement. The initial API-generation verdict below remains superseded.
The replacement is UNRUN; Main owns the next exact-SHA Actions run. No WSL2
acceptance is claimed.

## Scope

Reviewed the current `.github/scripts/tasks_wsl_fixture.mjs` and
`.github/workflows/ci.yml`, including the released implementation handoff and
the explicit WSL2 follow-up contract. No Rust, production policy, fixture-suite,
dependency, workflow or script edits were made by this reviewer. Only this
report was written; unrelated workspace changes were preserved.

## Initial Source Conclusions (Generation Verdict Superseded)

- The matrix explicitly pairs Windows 2022 with WSL1 and Windows 2025 with WSL2.
  `fail-fast: false` preserves the sibling row after a row failure. Independent
  runners isolate the unchanged run-owned distro name and paths; both consume
  the same exact-run rootfs artifact and retain its provenance/hash checks.
- Import accepts only `MT_TEST_WSL_VERSION=1|2`, records the numeric generation
  with exact owner/name/install-path state before dispatch, and supplies explicit
  `--version`. No alternate generation, default distro, installation, update,
  global shutdown or feature-enable fallback was added.
- The P/Invoke declaration matches the documented UTF16 name, HRESULT, ULONG
  outputs, flags, environment pointer array and count. System32-only DLL lookup
  is explicit. The `finally` block frees every returned environment string
  pointer and then the array with `Marshal.FreeCoTaskMem`, without decoding or
  emitting their values. These obligations and the configured-generation
  interpretation follow the [Microsoft API reference](https://learn.microsoft.com/en-us/windows/win32/api/wslapi/nf-wslapi-wslgetdistributionconfiguration).
- The API subprocess is bounded to 60 seconds and 8192 output bytes. Its strict
  UTF8/JSON result contains only integer `hresult` and `version`; success and an
  exact generation match are required. WSL2 also requires a bounded, allowlisted
  kernel release from `/usr/bin/uname -r` in the exact owned guest, ending in
  `-microsoft-standard-WSL2`. API/kernel attestation runs after import and again
  after discovery/build, immediately before the unchanged test invocation.
- Discovery still requires exactly one matching ignored Rust test, followed by
  exact execution with one test thread. No original account, private lifecycle
  or eight-peer-row assertion was changed. A missing test or failed capability,
  import, attestation or execution fails the gate rather than passing by skip.
- Cleanup remains `always()` and uses the recorded exact ownership state. It
  does not require successful attestation, a readable guest marker or the
  current version environment, so partial imports and generation mismatches
  remain cleanable. Unregister failure retains the state; no other distro is
  targeted. Existing command/test/cleanup bounds and private-output handling
  remain intact; new logs contain only bounded version/kernel metadata and
  public host capability information.

## Validation And Release

No local execution, automated check, probe, format, test, Git operation or child
dispatch was performed. No new automated tests were authored for this CI-only
patch. Prior green WSL1 production evidence does not establish WSL2 support;
the runner manifest and package version also do not prove guest availability.
Main must obtain actual requested-generation import, kernel attestation, exactly
one passing transport test and owned cleanup evidence for each Actions row.
No additional source change or review loop is required before that run.

## Follow-Up: Generation Field Correction

Main supplied Actions evidence for `e82d77c`, run `34035781785`: both actual
rows imported, but the new API probe returned `E_ACCESSDENIED` (`-2147024891`)
before any Rust test ran. WSL1 job `101493611077` and WSL2 job `101493611084`
both completed owned cleanup successfully. This report does not establish the
internal cause of that access failure.

Independent source review confirms a separate defect in the initial generation
interpretation. Microsoft's `LxssUserSession` distribution enumeration selects
WSL1/2 from `configuration.Flags` and `LXSS_DISTRO_FLAGS_VM_MODE`; configuration
lookup instead returns `configuration.Version`, also used in filesystem handling.
The earlier API-generation conclusion above is withdrawn. Reading the registry
`Version` as a generation would preserve this defect, not correct it.
[Microsoft WSL service source](https://github.com/microsoft/WSL/blob/master/src/windows/service/exe/LxssUserSession.cpp)

The replacement review must require read-only access to the exact current-user
registration, exactly one owned `DistributionName` match, a genuine `Flags`
DWORD, and the source-backed VM_MODE bit `0x8` mapping to WSL1/2. Main confirmed
the constant in `wslservice.idl` and the exclusion of VM_MODE from global flag
overrides in `DistributionRegistration.cpp`. No registry `Version`
or API fallback is acceptable. Separate WSL2 kernel boot proof, guest owner,
recorded import state and failure-independent owned cleanup remain required.
Newton retained source ownership during that preliminary review. The replacement
was not yet present then; the final verdict follows.

## Final Flags Replacement Review

Reviewed only the now-present `validateWslVersion` function delta in
`.github/scripts/tasks_wsl_fixture.mjs`. No concrete findings remain in this
replacement; released to Main for integration and Actions validation.

- `RegistryKey.OpenSubKey(..., false)` opens the fixed current-user Lxss root
  and registrations read-only. Exactly one ordinal `DistributionName` match to
  the generated owned distro is required. Missing, duplicate or unreadable
  registrations fail closed. Name lookup does not expand environment strings.
- Only the matched registration's `Flags` is read for generation. `DWord` kind
  and an `Int32` value are required; `BitConverter` preserves the full DWORD
  bit pattern when converting to `UInt32`, including the high bit. Nested
  `finally` blocks dispose registration handles on continue/error paths and
  dispose the parent handle. No registry values are changed.
- The sole emitted object is `{flags}`. The existing 60-second/8192-byte capture
  remains, followed by strict UTF8 and a one-field non-array JSON object check;
  its flags must be an integer within the unsigned 32-bit range. Registry names,
  environment values and arbitrary subprocess failure output are not logged.
- Generation is derived exclusively from the source-backed `0x8` VM_MODE bit.
  JavaScript's signed bitwise conversion preserves that low bit for all accepted
  DWORD values. No registry `Version`, API or expected-generation fallback
  remains. A mismatch fails before the test; separate WSL2 kernel boot proof
  and its existing bounded allowlisted output remain unchanged.
- This function replacement does not change import ownership state, cleanup,
  matrix routing, guest configuration or Rust test execution. The prior failed
  API run remains setup-failure evidence, not a guest/runtime regression or a
  successful generation/test result.

Only this report was edited by the reviewer. No local execution, syntax check,
formatting, automated test, probe, Git operation or child dispatch occurred.
No new tests were authored. The configured-generation observation, WSL2 kernel,
exact Rust scenario and owned cleanup must pass on the next Actions run; source
review does not substitute for that evidence. Review writes are now frozen.
