# Integration Validation Record

Recorded: 2026-09-05 (Asia/Shanghai)

## Current State

Both child implementations and their bounded independent static reviews are
complete at source level. No passing test, build, package, or native acceptance
result is claimed for this task yet. Commit/push authorization was received;
exact-commit Actions and native acceptance remain pending. Tasks are not archived.

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
