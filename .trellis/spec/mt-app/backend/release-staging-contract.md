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
