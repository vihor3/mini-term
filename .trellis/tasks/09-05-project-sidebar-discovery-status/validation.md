# Integration Validation Record

Recorded: 2026-09-05; updated: 2026-09-06 (Asia/Shanghai)

## Current State

Both child implementations and their bounded independent static reviews are
complete. Exact-product CI and Windows packaging passed for
`1ee49b8a4504ccf24b24f891bf8f7020420195cc`. The matching installer and runner
validation report are downloaded. Native interaction/startup acceptance remains
pending; neither child nor the integration parent is archived.

## Baseline and Ownership

- Branch: `feat/remote-file-management`.
- Baseline: `b166bd2b65549ef14bf0d6c3b9e1b31afb4f70e8`.
- Tracking remote: `fork`, `https://github.com/vihor3/mini-term.git`.
- `origin` is the upstream repository, not the current branch's CI owner.
- Product paths were clean before the first implementation dispatch:
  `crates`, `sidecars`, `.github`, `Cargo.toml`, and `Cargo.lock`.
- Pre-existing Trellis/platform/archive/journal changes and untracked files
  are unrelated baseline work. Do not stage or revert them as part of this task.
- Visibility/configuration/catalog freshness is implemented first. Remote
  Agent recognition and status follows its shared-sidebar handoff.

## Actions-Only Verification

All compilation, tests/fixtures, lint/format/whitespace checks, generation,
packaging, and automated verification are restricted to GitHub Actions.
Local operations are source/document editing, static source/Git inspection,
Trellis task bookkeeping, GitHub API operations, and artifact downloads.

The existing `CI` workflow covers root/sidecar locked checks and tests,
changed-line rustfmt/Clippy, generated i18n, whitespace, and Windows MSVC.
`Windows Package` builds and validates a non-release installer on branch push.
Do not create a release tag to obtain a test build. Record exact product SHA,
run IDs/URLs, conclusions, diagnostic correction commits, and final artifact.

## Commit and Push Consent

The coordinator proposed two scoped work commits, `feat: add per-project
worktree visibility` and `fix: stabilize remote agent status`, excluding
unrelated dirty work. A one-shot user confirmation was requested before
committing/pushing to `fork/feat/remote-file-management` and submitting scoped
Actions diagnostic correction commits. The user explicitly replied "可以" on
2026-09-05, approving that plan. This is user authorization, not a sub-agent
completion notification. Commits and Actions verification are now proceeding;
no passing Actions evidence is claimed before a matching run completes.
The exact file grouping and dirty-work exclusions are in `commit-plan.md`.

## Initial Actions Submission

- `02adf2d`: `feat: add per-project worktree visibility`.
- `8ccba04469b61e682b242393d5881cd6c63ec2a0`:
  `fix: stabilize remote agent status`.
- Both commits pushed to `fork/feat/remote-file-management` after approval.
- CI: <https://github.com/vihor3/mini-term/actions/runs/33976393533>.
- Windows Package:
  <https://github.com/vihor3/mini-term/actions/runs/33976393554>.
- Both runs target `8ccba04469b61e682b242393d5881cd6c63ec2a0` and were
  in progress at submission. No successful executable result is implied.
- Only the new Actions-only hunk of the mixed quality-guidelines file was
  staged. The pre-existing Windows section and all unrelated dirty work
  remain outside the commits. Git hooks were disabled for these Git
  operations so no local verification could be invoked implicitly.

### First Diagnostic Correction

The initial Linux job failed changed-line rustfmt and generated i18n checks;
its subsequent compile/test steps were skipped. Applied the runner-produced
`changed-rustfmt.patch` (not the full historical formatting patch) and
`generated-i18n.patch` locally without invoking any formatter or generator.
Artifact IDs are `9972444604` (`rustfmt-diagnostics-33976393533`) and
`9972444868` (`generated-i18n-33976393533`). The dictionary now contains the
14 source keys and reports 952 entries per language. This patch application
is not a passing validation result; the correction requires another run.

### Second Actions Submission

- `1d2ea0dd634756dcf52ad05f2ba34ee55b369db8`:
  `fix: apply sidebar CI diagnostics`.
- CI: <https://github.com/vihor3/mini-term/actions/runs/33976617320>.
- Windows Package:
  <https://github.com/vihor3/mini-term/actions/runs/33976617345>.
- The newer push cancelled the remaining initial Windows jobs through the
  existing concurrency policy. Initial run-level cancellation does not turn
  its failed Linux checks into a pass.
- Second-run formatting, generated i18n, staging tests, and locked graph
  checks passed. Linux/Windows compilation and remaining gates were still
  in progress at this checkpoint; no full green run is claimed yet.

### Second-Run Compiler Diagnostics

CI `33976617320` failed on both Linux and Windows. The two reported E0599
errors were new regression test calls to the nonexistent
`SessionTracker::track_input` at `store/remote_agents.rs:1010` and `:1061`.
Clippy and tests were not reached. A bounded Trellis checker is correcting
the calls against the existing tracker API; no production compatibility API
or relaxed assertion is authorized as a workaround. Local checks remain
prohibited. Windows packaging does not compile these tests and its result
cannot replace the failed all-targets gate.

The scoped correction uses the existing
`track_input_with_line_snapshot(pty_id, "codex\r", None)` at both sites. It
retains launch inference through the normal raw-input fallback; production
code and all regression assertions are unchanged. Executable confirmation
requires the next Actions run.

The checker completed its bounded static handoff. Root cause:
`track_input` is an mt-ai-local `#[cfg(test)]` wrapper, so dependency builds
used by mt-app tests cannot call it. The correction matches that wrapper's
delegation exactly, and the cross-crate testing contract now records this
boundary. No local checks ran.

### Third Actions Submission

- Product SHA: `1ee49b8a4504ccf24b24f891bf8f7020420195cc`.
- Commit: `fix: use public tracker input API in regression tests`.
- CI: <https://github.com/vihor3/mini-term/actions/runs/33977857810>.
- Windows Package:
  <https://github.com/vihor3/mini-term/actions/runs/33977857790>, run number 30.
- Two HTTPS push attempts failed with transport-only TLS errors. The retry
  succeeded and the GitHub branch API confirmed the exact SHA above.
- Formatting, generated i18n, staging tests, locked graph checks, Linux and
  Windows all-target compilation, sidecar compilation, and affected Clippy
  passed. Complete test suites and installer verification remain in progress
  at this checkpoint. The previous package run was cancelled by the newer
  push and is not evidence for this product SHA.

## Final Actions Evidence

Product SHA: `1ee49b8a4504ccf24b24f891bf8f7020420195cc`.
Any subsequent evidence-only documentation commit does not change this tested
product tree or claim a different binary SHA.

| Workflow/job | Run/job ID | Conclusion |
| --- | --- | --- |
| CI | `33977857810` | success |
| Rust workspace | `101337624846` | success |
| Windows MSVC check | `101337624768` | success |
| Windows Package | `33977857790` | success |
| Build and verify Windows installer | `101337641881` | success |

- CI: <https://github.com/vihor3/mini-term/actions/runs/33977857810>.
- Package: <https://github.com/vihor3/mini-term/actions/runs/33977857790>.
- Both workflow `headSha` values and the package report's `commit` match the
  exact product SHA. Both jobs of CI passed, including root/sidecar locked
  graphs and checks, affected Clippy, formatting/i18n, staging tests, full
  root/sidecar tests, whitespace, Windows checks, and focused Windows tests.
- Windows focused tests report 72 onboarding tests and 3 SSH project-operation
  tests passed, including all nine visibility onboarding regressions.
- Linux mt-app: 1027 passed, zero failed/ignored. Logs explicitly confirm
  visibility/settings/database persistence, refresh fencing, accepted-state
  projection, Hook exit ownership, retirement/teardown, and sidebar cases.
- Linux mt-ai: 200 passed, zero failed/ignored. Linux mt-ssh: 63 passed,
  zero failed, one intentionally ignored subprocess entry point. All three
  generated-command parent tests executed and passed, including exact route
  matching, provider arguments, and literal wildcard arguments. The ignored
  entry point is explicitly launched by those parent tests, not omitted probe
  coverage. A separate pre-existing mt-ui debug preview is also ignored.
- No local compile, test, fixture, formatter, generator, lint, whitespace
  check, installer verification, or automated native harness was run.

## Downloaded Artifact

- Name: `Mini-Term_1.2.2-ci.30_windows-x64`.
- GitHub artifact ID: `9973159120`; archive size: 18367455 bytes.
- GitHub-reported archive digest:
  `sha256:c205c529a9945270c51266126e02f5c39fb6cea9a74652390605ce1f852d505e`.
- Target: `x86_64-pc-windows-msvc`; package version: `1.2.2-ci.30`.
- Download directory:
  `/home/leo/Downloads/Mini-Term_1.2.2-ci.30_windows-x64/`.
- Installer: `Mini-Term_1.2.2-ci.30_x64-setup.exe` (18376829 bytes).
- Actions-reported installer SHA-256:
  `b1147e246cf8139ffc6a49362e9a0926fc08f55876d97fa352533730c80bfda6`.
- `windows-package-validation.json` reports `status: passed`, the exact
  product commit/run, valid resources and expected machine types, all eight
  staged/extracted payload hash matches, and the remote Agent feature marker.
- Local work downloaded and read this runner-produced report; it did not
  recalculate hashes, extract/verify the installer, or launch the native app.

## Static Integration Outcome

- Worktree settings use typed canonical/configured exclusions, retain new
  valid default-show behavior, and filter only the sidebar. WSL aliases and
  saved-only cleanup are covered by regression source without alias inference.
- Healthy refresh preserves successful presentation while keeping effective
  registration fences. Raw catalog and Quick Open remain complete.
- Remote process enumeration, accepted-state projection, weak-latch retirement,
  observer/poll cleanup, mixed-run/mixed-pane indicators, inferred-provider
  clearing, and provider-less Hook exit ownership were statically reviewed.
- The production cleanup helper is now referenced by lifecycle regression
  source. Full Pane/AppStore event-loop delivery remains a concrete test gap,
  not a passing end-to-end result.
- No new dependency, lockfile, sidecar, workflow, remote Hook protocol,
  repository pruning, terminal closure, or runtime identity migration was added.

## Native Acceptance

The repository has no GPUI end-to-end window automation harness. Source review,
unit tests, compilation, and installer extraction are not a native startup
cadence reproduction. Manual acceptance must use an Actions-produced binary
and retain its source/artifact correspondence. A Windows runtime check is
still required for the original startup/quiet/exit/reconnect symptom and the
settings hide/restart/unhide interaction; leave these explicitly pending if
no matching native trace can be obtained. Do not archive this integration task
on compiler evidence alone.
