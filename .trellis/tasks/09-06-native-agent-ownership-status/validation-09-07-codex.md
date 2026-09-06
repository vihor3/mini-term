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

- Source implementation: complete; six product files changed and 18 new
  regression tests passed on both Linux and Windows in Actions.
- Independent source review: approved for exact-commit Actions. All reported
  freshness/provenance findings resolved in source; native acceptance is separate.
- New Windows gate: nonempty discovery/execution of Agent semantics/runtime,
  pane producer and remote reconciliation suites, plus terminal library tests.
- Linux: existing full workspace, Clippy, SSH and sidecar gates unchanged.
- Exact-commit Actions and matching installer: passed and downloaded below.
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
- Formatting correction: `51865754509fc1729920b01d5e6038489c84e355`.
  CI https://github.com/vihor3/mini-term/actions/runs/34051966889 and package
  https://github.com/vihor3/mini-term/actions/runs/34051966899 started on it.
  Changed-line formatting passed; compilation is in progress. Replacement was
  pushed only after both prior WSL imports completed. Cancelled jobs
  `101536437474` (WSL1) and `101536437496` (WSL2) both completed their owned
  disposable-distro cleanup successfully. No test pass is claimed for that
  cancelled iteration. The complete formatting correction retained independent
  source approval as mechanical-only.

All compilation, tests, fixtures, formatting, lint, syntax/whitespace checks,
packaging and automated verification remain GitHub Actions-only. No local app,
installer, user-device probe, credential/configuration change or check was run.
The approved native layout and other parent acceptance items remain unchanged.

## Validated Candidate

- Product: `51865754509fc1729920b01d5e6038489c84e355`.
- CI: https://github.com/vihor3/mini-term/actions/runs/34051966889
  All five jobs completed SUCCESS on that exact commit.
- Linux job `101537104709` passed formatting, compilation, Clippy, full workspace
  tests, all five separately executed authenticated SSH fixtures, sidecars and
  whitespace. Completed `2026-09-06T18:53:25Z`. Relevant totals: mt-ai 247 passed,
  mt-app 1,247 passed (five SSH tests ignored here, each passed separately),
  mt-terminal 19 passed. All 18 new tests appear as `ok` in the downloaded log.
- Windows job `101537104718` completed SUCCESS at `18:59:03Z`. The nonempty
  Agent gate completed at `18:54:17Z`: semantics 5, runtime 44, terminal 19,
  pane 14, remote reconciliation 29 and route projection 27 tests passed.
  All 18 new cases are present as `ok`; navigation, onboarding, Files, Tasks,
  Git, sidecars and terminal-host gates also passed.
- WSL1 job `101537231662`: one exact actual-transport test passed, zero ignored,
  147.38s; owned cleanup passed at `18:44:03Z`. WSL2 job `101537231660`: the same
  gate passed once, zero ignored, 155.19s; owned cleanup at `18:46:01Z`.
  WSL2 kernel `6.18.33.2-microsoft-standard-WSL2` was attested in the runner log.
  Rootfs preparation job `101537104561` also passed. Times are UTC on 2026-09-06.
- Windows Package: https://github.com/vihor3/mini-term/actions/runs/34051966899
  Job `101537083136` passed at `18:56:48Z` on the same product commit. Artifact
  `9995202716`: `Mini-Term_1.2.2-ci.72_windows-x64`.
  https://github.com/vihor3/mini-term/actions/runs/34051966899/artifacts/9995202716
- Downloaded installer and Actions manifest:
  `/home/leo/Downloads/mini-term-1.2.2-ci.72/`. Manifest reports `status=passed`,
  exact commit/run, and all eight payloads matching. Installer:
  `Mini-Term_1.2.2-ci.72_x64-setup.exe`, 18,889,614 bytes, Actions SHA-256
  `e58b5d2675c669639f7f5b0dee727b3f41d1dfb87ae196880457d05e16329f33`.
- Complete Linux/Windows/WSL logs are downloaded under
  `/home/leo/.cache/mini-term/artifacts/5186575-agent-codex/`. The first Linux
  log transfer ended early; it was discarded and the complete retry was used.
  No local hash check, test, formatter, probe, installer or app launch occurred.
- Native acceptance remains OPEN for the user's real remote Codex frame and
  status cadence. This candidate fixes positive Working evidence; no reliable
  marker remains Unknown or freshness-qualified last-known state, not invented
  Waiting/Done. The Agent child and parent remain open for native acceptance;
  unrelated dirty journals/tasks were not archived or auto-committed.
