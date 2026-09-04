# Validation Report

Recorded: 2026-09-05 (Asia/Shanghai)
GitHub Actions completion date: 2026-09-04 (UTC)

## Outcome

The unified local and SSH add-project flow is complete at product commit
`2112aec0e2ffc2cb7c4127e02a3d37338a99cf2d`. Acceptance criteria AC1-AC11
are satisfied. The PRD, design, implementation/check manifests, reusable
onboarding specification, and task metadata describe the delivered behavior;
the final task audit found no scope drift.

## Delivered Behavior

- One Orca-style modal selects Local or a saved SSH host and supports adding an
  existing folder, cloning a repository, creating a new Git-backed folder, and
  initializing an existing non-Git folder.
- Existing folders are registered without mutation; existing repository roots
  skip initialization; nested selections resolve to the containing repository
  without creating a nested `.git` directory.
- Host, page, form, request generation, SSH fingerprint, and SSH session epoch
  fence stale async results.
- Canonical host-qualified locations deduplicate projects and activate existing
  records. A registration retry is read-only and cannot repeat clone/init work.
- Failed and uncertain remote mutations fail closed, preserve evidence, redact
  credentials, and avoid registering partial projects.
- Windows exact-root classification handles long-name and 8.3 aliases by
  comparing opened directory identity when canonical strings differ.

## Validated Change Range

The final executable validation covered `86838d2..2112aec`. The last hardening
commits added canonical-path regression coverage, Windows directory-identity
comparison, ABI/layout coverage, formatting cleanup, and removal of the
task-introduced obsolete SSH store import. Unrelated dirty workspace files were
not included in the task commits or validation scope.

## GitHub Actions CI

- Workflow run: `33910845940`
- URL: https://github.com/vihor3/mini-term/actions/runs/33910845940
- Product SHA: `2112aec0e2ffc2cb7c4127e02a3d37338a99cf2d`
- Result: success
- Rust workspace job: `101146633240`
- Windows MSVC job: `101146633455`
- Root tests: 1,976 passed, 1 ignored, 0 failed
- Sidecar tests: 94 passed, 0 failed
- Node staging tests: 10 passed, 0 failed
- Windows focused tests: 63 passed, 0 failed
- Changed-line rustfmt diagnostics: 0 hunks
- Changed-line Clippy diagnostics: 0 warnings
- Task-scope Windows warnings: 0

The Windows job explicitly passed the directory-alias fallback, Win32
file-information layout, real clone exact-root, real new-folder exact-root, and
focused remote SSH project-operation tests. Pre-existing warnings outside the
task's changed lines remain baseline issues and are not part of this task.

## Windows Package

- Workflow run: `33910846030`
- URL: https://github.com/vihor3/mini-term/actions/runs/33910846030
- Result: success
- Artifact ID: `9952065376`
- Artifact: `Mini-Term_1.2.2-ci.23_windows-x64`
- Artifact digest: `sha256:97de278d74d22a793ebb58fa2fc7ab98073782a15e6e10b7960e0aa620a9b384`
- Artifact expiry: `2026-09-18T19:48:04Z`
- Installer: `Mini-Term_1.2.2-ci.23_x64-setup.exe`
- Installer size: 18,217,228 bytes
- Installer SHA-256: `8c1b5daadcc3ec284842920c24f8c2785ddeaa35e2698e4c58d0378cd1e8b102`
- Installer machine: `0x014c`
- Validation: 7 feature markers, 8 payloads, and 13 extracted files verified
- Local download: `/home/leo/Downloads/Mini-Term_1.2.2-ci.23_windows-x64/`

## Trellis Synchronization

- AC1-AC11 are checked in `prd.md`.
- `design.md` and the reusable onboarding contract record the Windows
  file-identity fallback and fail-closed behavior.
- `implement.jsonl` and `check.jsonl` each contain 18 valid entries.
- `task.json` records the final product SHA, CI/package evidence, execution
  policy, residual risks, and synchronized status.
- The implementation remained inside the planned onboarding, host operation,
  registration, localization, test, and workflow boundaries.

## Execution Policy And Residual Risk

No Cargo build, rustfmt, Clippy, test, i18n generation, Node staging, Docker, or
package command was run locally. Executable validation and packaging ran only
in GitHub Actions; local work was limited to source/document inspection, Git
metadata, task validation, artifact download, and checksum inspection.

Residual risks:

- A physical Windows UI smoke test was not performed.
- SFTP cannot fully eliminate a same-account path replacement race without
  descriptor-relative remote filesystem operations.
- Some UNC/network filesystem providers may expose incomplete or
  provider-specific directory identity metadata.
