# WSL2 Actions Review

Status: SOURCE REVIEW COMPLETE / RELEASED / FROZEN. No concrete findings or
source blockers in Newton's final two-file CI-only patch. Main owns the scoped
commit and exact-SHA Actions run. This patch remains UNRUN; this is not WSL2
acceptance evidence.

## Scope

Reviewed the current `.github/scripts/tasks_wsl_fixture.mjs` and
`.github/workflows/ci.yml`, including the released implementation handoff and
the explicit WSL2 follow-up contract. No Rust, production policy, fixture-suite,
dependency, workflow or script edits were made by this reviewer. Only this
report was written; unrelated workspace changes were preserved.

## Source Conclusions

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
