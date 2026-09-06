# CI Integration Source Review

Status: bounded SOURCE REVIEW COMPLETE; CI script/workflow writes RELEASED
to main. Backend/domain/CLI/local files remain RELEASED. Reviewed and edited
only `.github/scripts/tasks_wsl_fixture.mjs`, `.github/workflows/ci.yml` and
this report. No local checks, syntax, tests, builds, fixtures, WSL/SSH probes,
automation, staging, commits or other Git writes.

## Findings (fixed)

- P1, `tasks_wsl_fixture.mjs:185`: failed/stopped/partial-distro termination
  aborted cleanup before unregister. Termination is now bounded to 30 seconds
  and best effort; unregister is still attempted only for the state-owned
  unique name. Failed unregister or failed absence verification preserves
  install/state evidence and fails the step. No broad WSL shutdown or cleanup.
- P2, `tasks_wsl_fixture.mjs:72`: enumeration now uses `--all --quiet` for both
  the pre-import exclusion and cleanup, including partial registration states.
  UTF-8/UTF-16 decoding is fail-closed rather than silently lossy. The same
  run/attempt-derived name, canonical RUNNER_TEMP install path and exact owner
  fields are checked; state is exclusively created after proving the name and
  install location absent, before import dispatch. Cleanup does not need to
  boot a partial guest to read its marker and never chooses a pre-existing name.
- P2, `tasks_wsl_fixture.mjs:170`, `ci.yml:233`: listing and execution both use
  `--ignored --exact`; discovery requires exactly the named test once. This
  prevents an absent filter or removed ignore annotation becoming a green
  zero-test gate. Preserved all five main-authored SSH test names, including R11.
- P2, `ci.yml:222`: `SetEnv HOME` does not change SFTP's initial directory;
  OpenSSH first uses passwd home. Set `internal-sftp -d <fixture>/remote-home`
  so the browser's actual SFTP home probe also remains in the fixture. This is
  supported by [OpenSSH sftp-server -d](https://man.openbsd.org/sftp-server.8#d)
  and [pinned session setup](https://github.com/openssh/openssh-portable/blob/V_8_9_P1/session.c#L1523).
  UsePAM yes and password/kbd-interactive no remain unchanged. No account unlock.
- P2, `ci.yml:382`, `:406`: reserved cleanup headroom with a 100-minute WSL
  job and 10-minute cleanup, retaining the 60-minute test step. Cleanup's normal
  command bounds are two-minute enumeration, 30-second terminate, five-minute
  unregister and two-minute verification. The SSH EXIT trap now continues
  removing private fixture files after an earlier cleanup error; failed file
  removal makes the step fail rather than claim cleanup success.
- `tasks_wsl_fixture.mjs:78`, `:100`: guest setup reads explicitly use `--cd /`
  instead of inheriting a Windows workspace cwd with automount disabled. The
  built synthetic executable is checked as a regular ELF64 little-endian x64
  file before install/upload; runtime gh hash/path validation remains required.

## Provenance And Contract Review

- Read assigned Tasks check context, PRD/design/implement/review, current WSL
  fixture contract, package/guides indexes, release-staging and quality
  contracts. Read the complete CI script/workflow, launcher, production envelope
  argv, synthetic executable setup and the five SSH test source locations.
- The pinned input hash
  `1483cc5c1dce13064f774834cbffdff226559fd522a67a381a8ea77d63fb4109`
  matches [Canonical's published SHA256SUMS](https://cloud-images.ubuntu.com/wsl/jammy/current/SHA256SUMS).
  The builder verifies it before extraction. The `current` URL may move; a new
  image must fail the pin until explicitly reviewed, not silently update it.
  The [package manifest](https://cloud-images.ubuntu.com/wsl/jammy/current/ubuntu-jammy-wsl-amd64-wsl.manifest)
  includes Python 3.10; the fixture contract requires 3.8+.
- The Linux x64 Ubuntu 22.04 job compiles only the standalone stdlib synthetic
  `gh_fixture.rs`, installs it at `/usr/local/bin/gh`, and installs the checked-in
  launcher at `/usr/local/bin/python3`. Real Python stays at `/usr/bin/python3`.
  The rootfs contains the marker and isolated cases/home, fixture PATH, disabled
  Windows automount/interop and root default only inside this new import.
  No runner gh config, keys, user account credentials or credential files are
  copied into the artifact. The executable uses synthetic sentinel identities.
- `tasks_wsl_fixture.mjs:144`: manifest validation requires exact repository,
  run, attempt, SHA, schema/kind, pinned source URL/hash and rootfs/gh hashes.
  The downloaded tar hash is checked before import; guest owner marker is
  bounded and matched before environment publication and before test discovery.
- `ci.yml:356`, `:376`: the Windows job depends on the Linux producer, downloads
  the run/attempt-qualified artifact, and passes no foreign repository/run or
  credential override. [download-artifact v8](https://github.com/actions/download-artifact/blob/v8/action.yml)
  defaults to this run/repository. New Cargo invocations retain `--locked`;
  existing independent root/sidecar gates and workspace cache mappings remain.
- [Windows 2022 runner inventory](https://github.com/actions/runner-images/blob/main/images/windows/Windows2022-Readme.md)
  lists WSLv1 enabled (reviewed image 20260824.284.2). The workflow uses that
  label and [explicit import version 1](https://learn.microsoft.com/en-us/windows/wsl/basic-commands#import-a-distribution).
  Capability/import failures fail CI; there is no WSL2 conversion, installation
  into an existing distro, default switch or POSIX/local substitute.
- Shared loopback retains 127.0.0.1, private run-owned directory, key-only auth,
  isolated client/remote HOME, fixture-owned sshd config/PID/key paths and early
  cleanup. The new fifth browser test exists at `remote_ssh/dirs.rs:602`; it
  compares real authenticated home facts and deliberately poisons only its test
  connection's cache. It does not require SFTP home to equal the browser root.

## Other Owners

Noether's launcher and actual Windows ignored test are now source-available
and were checked read-only against this CI contract. The launcher's `$4` matches
the production envelope's captured cwd argument, it seeds only synthetic
environment, and it execs `/usr/bin/python3` with unchanged args. The exact
filter now exists at `tasks_account_executor/tests.rs:651`:

`tasks_account_executor::tests::tasks_account_executor_wsl_sentinels_cleanup_and_foreground_host`

The three published environment inputs and six-field owner marker match the
current review contract. `WslFixture::open` checks Actions/run/attempt/repository/
SHA, the exact owned distro and marker path, bounded marker JSON with unknown
fields denied, exact gh/Python lookup and ELF checksum before account APIs.
Setup reads use production `execute_host_command` on the selected WSL source;
case creation and ready/release/orphan checks stay in that distro below
`/mini-term-fixture/cases/<UUID>`. There is no native fixture compiler, local
fallback, distro import/unregister, default switch or real credential lookup
inside this test.

The ignored test calls production capability, discovery and selected-account
APIs, which reach `run_wsl`/`wsl.exe`. Authored assertions cover account-specific
list/detail data, GitHub/GHES hosts, same credential after lookup rotation,
wrong pre/post proof, credential/store errors, stdout/stderr token echo,
missing helper, malformed reply/enumeration, output limits, lookup/data timeout
and cancellation, and Linux descendant readiness/retirement. This resolves the
previous missing-test source dependency, not its execution gate. The name does
not independently prove Tasks foreground-cache or UI lifecycle behavior.

Main's appended `release-staging-contract.md:221` actual-host-fixture contract
was read and agrees with this producer/consumer boundary. Main approved all CI
corrections, including explicit SFTP start directory; the browser fixture reads
its authenticated SFTP initial cwd independently, so the change is compatible.
No further spec/owner API request was found. No launcher, executor, test or spec
source was edited by this checker.

## Findings (not fixed)

- No unresolved CI source blocker found in this bounded review. Main must stage
  the now-authored Windows test and launcher with the matching CI integration;
  keep exact ignored discovery so a later missing/renamed test cannot pass.
- No source inspection can establish real import, interpreter/ELF startup,
  WSL1 control-pipe delivery, cleanup after timeout/partial import, or actual
  authenticated SSH behavior. All require the exact-SHA Actions gates below.
- `always()` and an EXIT trap cannot guarantee completion after runner loss or
  forcible process/job termination. Command cleanup errors preserve ownership
  evidence where the script is running; do not report teardown as proven from
  source alone. Tests/negative-path fixtures for this CI script are not authored
  in this narrow review; no new framework or out-of-scope test file was added.

## Verification

- Lint / Syntax / Format / Whitespace / Build / Metadata / Tests: UNRUN.
- WSL rootfs preparation/import/test/cleanup and all five SSH tests: UNRUN.
- Published checksum, runner WSL1 capability, CLI syntax and same-run artifact
  defaults are primary-source evidence only, not executed fixture evidence.
- Require Actions records for the staged head SHA: actual runner image, input
  rootfs hash, same-run artifact identity/manifest hashes, one discovered and
  executed WSL test, all five discovered/executed SSH tests, and cleanup outcome.
- Additional Actions negative gates: stopped and partial owned import still
  reaches unregister; foreign/mismatched state performs no destructive command;
  unregister/verification failure retains state; existing-name collision aborts;
  mismatched provenance/tar/ELF rejects before account execution; absent or
  no-longer-ignored exact test fails discovery. No such fixture was run locally.
- Actual WSL1 fixture success will not establish WSL2 or real secure-store
  compatibility. Native account/UI acceptance remains tied to matching Actions
  artifacts and separate user-account acceptance, never this synthetic rootfs.

## Release

Both CI files and this source-review handoff are RELEASED to main for integration
and staging. Main owns the remaining tests/specs/run evidence. All unrelated
dirty paths were preserved; no tests/executor/backend/domain/CLI/local source
or other workflow was edited by this reviewer.
