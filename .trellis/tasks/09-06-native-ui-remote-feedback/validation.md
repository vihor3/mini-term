# Integrated Feedback Validation

## Latest Completed Candidate

- Product commit: `5a070e149e76d289c08cc64e9cd3d05deb8259b8`.
- CI: https://github.com/vihor3/mini-term/actions/runs/34013775348
- Windows Package: https://github.com/vihor3/mini-term/actions/runs/34013775268
- State: Linux/Windows/package SUCCESS; actual WSL FAILED at a cancellation
  assertion. Complete cancellation/transport correction is not validated.
- All five implementation slices are independently source-reviewed. All
  execution, including diagnostics, formatting and fixtures, remains Actions-only.

Subsequent test-only diagnostic attempts are tracked in `progress.md`; their
running/failed gates do not inherit a pass from this completed candidate.

Current WSL job101434114219 discovered and executed exactly one test in 20.19 s.
It passed marker/shim/hash, hostile captured cwd, literal/empty argument counts
on project/pre-project routes, account enumeration/selected identity/secret
rejection and error cases by source order. It then returned HostHelperUnavailable
where cancellation expected Cancelled. Readiness passed; the failed iteration's
later descendant-retirement assertion was not reached. Owned cleanup succeeded.
The old failure does not identify data versus lookup cancellation or its cause.
The next bounded test-only trace preserves all assertions and acknowledgement
requirements; it has no passing evidence yet. Linux job101433980963 is entirely
SUCCESS, including mt-ai 244, mt-app 1211, new empty-argument regressions, the
five separately executed actual SSH fixtures, sidecars and whitespace.
Windows job101433980851 is entirely SUCCESS: onboarding 82/3, Files 49/14/5/1,
execution-host 14, Tasks executor 18 (one separate WSL test ignored), Tasks
app/config/domain 24/2/31, Git domain/app 24/6/124, terminal-host library 31.
The new empty-argument planner/private-builder tests explicitly passed on Windows.

Package job101433965140 is SUCCESS. Artifact9983573681 is
`Mini-Term_1.2.2-ci.53_windows-x64`, not downloaded or launched locally.
Artifact: https://github.com/vihor3/mini-term/actions/runs/34013775268/artifacts/9983573681
This artifact does not establish successful actual WSL or native UI acceptance.

## Proven Predecessor

At `0e141f01596787b66e3a9c90f5c94ff3e1708236`, CI34009374061:

- Linux job101422396717 SUCCESS: format, generated dictionary, staging, locked
  graphs, full compilation, Clippy, root/sidecar tests and whitespace. The root
  suite includes mt-ai 244 and mt-app 1205 passes; five host fixtures are
  explicitly ignored there and each is separately discovered/run once below.
- Authenticated SSH: both Git fixtures, Tasks account isolation, pinned Files
  mutations and folder browsing each passed, with zero ignored tests.
- Windows job101422396483 SUCCESS: onboarding 82/3, Files 49/14/5/1,
  execution-host 10, Tasks executor 12 (one separate WSL fixture ignored),
  Tasks app/config/domain 24/2/31, Git domain/app 24/6/124, terminal-host library
  31. Root/sidecar compilation passed; empty binary targets are not test passes.
- Actual WSL job101422518970 FAILED before auth. Its eight-way guarded matrix
  proved only launcher-root cwd rows could read the exact same-run marker.
  Original-baseline failure and owned-distro cleanup were preserved.
- Package34009374064 SUCCESS; artifact9982270369 is
  `Mini-Term_1.2.2-ci.47_windows-x64`. The Actions manifest passed staged/extracted
  payload verification. Downloaded only, with no local launch or verification,
  to `/home/leo/.cache/mini-term/artifacts/0e141f0-integration`.

This predecessor is Windows/SSH pre-acceptance evidence, not a passing current
WSL correction. Chronological runs, fixes and artifact hashes are in `progress.md`.

## Native Acceptance: Open

Use the matching Actions installer; do not substitute a locally built app.

- [ ] R1-R3/R16: no-Agent terminals stay quiet; only Mini-Term-owned foreground
  and background Agents appear under their owners; working/waiting/attention,
  disconnect/reconnect and Runtime titles match the exact terminal/run.
- [ ] R4/R8/R9: one display per worktree, larger individual titlebar tabs,
  no group/split/duplicate strip; old grouped terminals remain reachable and
  background PTYs survive selection, reordering, close and restart.
- [ ] R5-R7/R10: anchored project settings, hover/focus ellipsis, footer icons,
  first-delay/warm tooltips, keyboard access and narrow/high-DPI geometry.
- [ ] R17: Files/Git/Tasks/Sessions selection persists across worktree/project
  switches, while contents, drafts, selected accounts and terminal owners do not mix.
- [ ] R11-R13: full source-correct Local/WSL/SSH folder navigation, complete
  Files scrolling, row/blank menu creation targets and drag-only upload.
- [ ] R14: remote Changes/History/Diff/sync and Worktree Management use the
  remote repository, preserve confirmation/drafts and reject stale ownership.
- [ ] R15: actual device gh accounts both appear in Tasks; project choices
  survive restart independently of global gh active-account changes in either
  direction; logout/revocation/secure-store failures do not choose another account.
- [ ] Baseline worktree policy: invalid hidden, new valid visible, manual hiding
  persistent, offline inventory never treated as invalid/removal authority.

Synthetic host fixtures cannot prove real noninteractive credential-store access
or native pointer/window rendering. Keep this parent and its children open until
the applicable user-observed acceptance is recorded; CI success alone is not
resolution of every reported screenshot symptom.
