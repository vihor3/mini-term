# Integrated Feedback Validation

## Approved Native Layout

- Latest product commit: `f90993afce7c04f3d6a359fdf5bee0a9016d4de9`.
- CI: https://github.com/vihor3/mini-term/actions/runs/34046725805
  All five jobs completed SUCCESS: Linux `101523006968`, Windows `101523007135`,
  rootfs `101523007104`, actual WSL1 `101523147769` and WSL2 `101523147759`.
- The four native navigation suites were each discovered nonempty and executed
  on Windows. Full Linux/SSH/sidecar and existing Windows regression gates also
  passed. Both actual WSL jobs passed their owned-distro cleanup.
- Windows Package: https://github.com/vihor3/mini-term/actions/runs/34046725815
  Job `101522992431` completed SUCCESS on the same exact commit. Artifact
  `9993700175` is `Mini-Term_1.2.2-ci.70_windows-x64`:
  https://github.com/vihor3/mini-term/actions/runs/34046725815/artifacts/9993700175
- Installer and Actions manifest were downloaded to
  `/home/leo/Downloads/mini-term-1.2.2-ci.70/`. The manifest reports `passed`,
  all eight payloads matching, installer size 18,874,602 bytes and SHA-256
  `ef0a4abfdea9831db38ec0151c6d5dfaad41b13c878aa3ae41fe5b48ceb8b523`.

The approved preview is now implemented in the native shell: shared caption/body
boundaries, retained live sidebar widths and visibility, compact 42px caption,
200px terminal tabs and readable titles without generated identity suffixes.
No WSL/backend/Agent behavior was added or changed in this visual follow-up.
Evidence comes from exact-commit job/step conclusions and the Actions manifest;
intermittent raw-log download failures do not justify copying old per-test totals.
No local automated validation, hash check or app/installer launch occurred.

The native acceptance checklist at the end remains OPEN. Use this new installer
for layout feedback; the older candidates below cannot validate the new visuals.
See the navigation child's validation record for source review, diagnostic
iterations and remaining native interaction coverage.

## WSL Generation Extension

- CI-only commit: `30dce5371fcbdd4936d174c12adfe7dd196807c3`.
- Run: https://github.com/vihor3/mini-term/actions/runs/34036603468
- Actual WSL1 job101495870630: SUCCESS. Flags7, exact gate discovered once and
  executed with1 pass/0 failures/0 ignored,149.41s. Owned cleanup13:50:30Z.
- Actual WSL2 job101495870628: SUCCESS. Flags15 and the exact owned guest kernel
  `6.18.33.2-microsoft-standard-WSL2` attested both after import and immediately
  before execution. Exact gate discovered once and executed with1 pass/0 failures/
  0 ignored,154.11s. Owned cleanup13:51:48Z.
- Same-run rootfs job101495742885: SUCCESS. Ordinary Linux job101495742935 and
  Windows job101495742840 also completed SUCCESS. The entire workflow passed
  on this exact commit.

Both rows preserve and pass the unchanged original account, privacy, captured
cwd/argv, cancellation/timeout and descendant assertions plus all eight appended
client-first concurrent-peer rows. Each cleaned only `mt-tasks-34036603468-1`
inside its own runner. Actual images in the logs are Windows2022
`20260830.290.1` and, for the requested windows-2025 label, windows-2025-vs2026
`20260824.214.3`. The logged guest kernel and completed test, not the image
manifest or WSL package version, establish this WSL2 evidence.

This extension changes CI scripts and records only. Application/workspace source
is unchanged from the packaged candidate below; no new installer was built or
local automated verification performed. The initial e82d77c run failed only
the newly added API attestation and ran neither WSL test. Its correction and
both successful exact-owned cleanups are recorded chronologically in progress.md.
Enabled interop, external shared-distro clients, nested Jobs and native UI
acceptance remain outside these isolated transport tests.

Linux passed compilation, formatting/dictionary/staging/locked-graph/Clippy
checks, full workspace tests (mt-ai244, mt-app1223), all five separately executed
actual SSH fixtures with1 pass/0 ignored each, sidecars and whitespace. Windows
passed onboarding82/3, Files49/14/5/1, execution-host28, Tasks executor49 plus
the separate actual WSL gate, Tasks app/config/domain24/2/31, Git24/6/126 and
terminal-host31. The Job policy and WSL uncertain-write regressions passed;
empty binary targets are not counted as tests. All evidence is from Actions.

## Earlier Integrated Candidate

- Product commit: `f4ee0f9aa954142a7552e458760d9254e17701b7`.
- CI: https://github.com/vihor3/mini-term/actions/runs/34032734882
- Windows Package: https://github.com/vihor3/mini-term/actions/runs/34032734897
- State: Linux/Windows/actual WSL1/package SUCCESS on this exact product commit.
- All five implementation slices are independently source-reviewed. All
  execution, including diagnostics, formatting and fixtures, remains Actions-only.

Actual WSL job101485351593 executed the exact ignored gate once:1 passed,
0 failed,0 ignored,149.54s. All original owner/marker/hash/cwd/literal-argv,
account isolation, privacy, lifecycle and descendant assertions passed. The
eight appended client-first rows also completed: registered/pre-project normal
completion and timeout, plus private data/lookup cancellation and timeout, each
with a proven overlapping peer that survived retirement and finished normally.
No failure-only timing contrast ran. Owned mt-tasks-34032734882-1 cleanup passed
at12:33:19Z. This verifies the production correction on the isolated WSL1 fixture.

Linux job101485217634 is entirely SUCCESS: format, dictionaries/staging, locked
graphs, compilation, Clippy, full tests (including mt-ai244 and mt-app1223), all
five individually discovered/run actual SSH fixtures, sidecars and whitespace.
Those five fixtures were ignored only in the ordinary root suite; each then
ran separately with1 pass and0 ignored, covering Git, Tasks, Files and browsing.

Windows job101485217555 is entirely SUCCESS: onboarding82/3, Files49/14/5/1,
execution-host28, Tasks executor49 plus1 separately executed WSL gate, Tasks
app/config/domain24/2/31, Git24/6/126 and terminal-host31. All eight new policy,
root-ownership and fixture tests explicitly passed, as did the eighteen existing
timing/public regressions and both Git launcher-loss/retained-lease regressions.

Package job101485197913 is SUCCESS. Artifact9989567203 is
`Mini-Term_1.2.2-ci.67_windows-x64`:
https://github.com/vihor3/mini-term/actions/runs/34032734897/artifacts/9989567203
Downloaded only to `/home/leo/.cache/mini-term/artifacts/f4ee0f9-integration`.
The passed Actions manifest records installer18,851,455 bytes and SHA256
`77c5fcb8c8ba194760ca31a5181fef753fcd07504f5a54bb666d53d4f6fd25a3`.
No local installer/app launch, hash check or other automated verification ran.

WSL2 is now covered by the separate extension above. Enabled interop, nested
Jobs, real shared-distro behavior and the native acceptance checklist below
remain unverified. Chronological failure evidence,
the public timing contrast and all source/review handoffs remain in progress.md.

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

This predecessor was Windows/SSH pre-acceptance evidence only; the current WSL
correction is validated by the exact latest candidate above. Chronological runs,
fixes and artifact hashes are in `progress.md`.

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
