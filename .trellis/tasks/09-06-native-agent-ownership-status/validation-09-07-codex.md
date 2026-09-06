# Codex Native State Regression Validation

## Reported Failure

The user supplied a native screenshot of an owned remote Codex terminal showing
its Working status while the exact Mini-Term sidebar row remained Unknown.
Baseline package: `Mini-Term_1.2.2-ci.70_windows-x64`, product
`f90993afce7c04f3d6a359fdf5bee0a9016d4de9`. Its passing build and layout evidence
does not validate the newly reported Agent symptom.

## Root Cause

This is a missing producer and positive integration-test gap. Earlier corrections
properly stopped treating process liveness and generic terminal output as task
activity. Remote Codex has no forwarded local Hook channel and its conversation
title is not a task-state marker. No producer supplied the visible native status
to the otherwise correctly fenced semantic registry.

The correction uses parsed, bounded live-grid Codex status/composer evidence,
not a sidebar label override. It preserves exact foreground ownership, original
capture time, connection epoch, Hook priority and stale last-known semantics.
Orca research and pinned official Codex source show why composer availability or
a disappearing Working row must not be treated as Waiting or completion.

## Verification Status

- Source implementation: complete; six product files changed and 18 focused
  regression tests authored, not yet run.
- Independent source review: approved for exact-commit Actions. All reported
  freshness/provenance findings resolved in source; native acceptance is separate.
- New Windows gate: nonempty discovery/execution of Agent semantics/runtime,
  pane producer and remote reconciliation suites, plus terminal library tests.
- Linux: existing full workspace, Clippy, SSH and sidecar gates unchanged.
- Exact-commit Actions and matching installer: pending.
- Native acceptance of the corrected package: pending user observation.

## Review Follow-up

The early independent review identified two freshness risks in the draft:
split ANSI completion losing its pre-frame baseline, and structural/footer or
spinner-only changes renewing an unchanged semantic marker. The implementer is
correcting both and adding VT-to-registry regressions. The second review
confirmed those corrections and identified two additional provenance gaps:
cursor-only output could make an unchanged marker newly eligible, and unbinding
retained unfinished-frame authority. Both require source corrections and
negative VT-to-registry tests before approval for Actions. Final review confirmed
both fixes and their positive counter controls, with no remaining in-scope
findings. Executed test results are still required; authored tests alone do not
establish passing verification.

## Actions Iterations

- Initial product commit: `150ac8c84a712d7a26d8b420bdf8ba427b6f5018`.
  CI https://github.com/vihor3/mini-term/actions/runs/34051663557 and package
  https://github.com/vihor3/mini-term/actions/runs/34051663551 started on it.
- Linux job `101536269004` failed changed-line rustfmt before compilation or
  tests. The complete 42,347-byte formatter patch was downloaded from all three
  numbered gzip/base64 API annotations and applied as one contextual artifact
  to four owned Rust files. No local formatter or test was run. Windows
  compilation and both WSL imports remain pending before a replacement push.

All compilation, tests, fixtures, formatting, lint, syntax/whitespace checks,
packaging and automated verification remain GitHub Actions-only. No local app,
installer, user-device probe, credential/configuration change or check was run.
The approved native layout and other parent acceptance items remain unchanged.
