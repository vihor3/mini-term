# Navigation Validation

## Approved Layout Follow-up

The user approved the corrected `preview/index.html` and requested native
implementation. Euclid implemented the five-file native change and Pasteur
independently approved the source for Actions validation on 2026-09-07.

- Shared `ShellGeometry` drives caption/body boundaries and live right width.
  Caption height is 42px; tabs are 200px and consume available space before
  window-drag fill. Native control/separator space is accounted for.
- Sidebar toggles retain window-global tool selection and existing per-worktree
  owners. Narrow overlays support dismissal and keep the caption rail compact.
- Readable exact-target captions omit generated identity suffixes; runtime
  diagnostics, manual brackets and route validation remain unchanged.
- Eleven new regressions and revised tab-reveal coverage were authored. The
  Windows workflow discovers and executes geometry, titlebar, sidebar-navigation
  and store-context suites. No local build, test, formatter or app ran.
- Main review found and implementation corrected narrow-overlay branding in the
  caption rail, separator space at minimum control widths, and equal-flex
  allocation unnecessarily truncating tabs beside a large drag region.
- Nonblocking coverage gap: new title tests exercise the owned projection and
  existing target-resolution tests, not the complete `terminal_tab_title()`
  entry point with provider fallback mapping.
- Exact-commit CI and packaging passed; the installer is downloaded. Native
  visual/pointer acceptance remains open. Earlier candidates below are historical.

### Validated Candidate

- Product commit: `f90993afce7c04f3d6a359fdf5bee0a9016d4de9`.
- CI: https://github.com/vihor3/mini-term/actions/runs/34046725805
  All five jobs completed SUCCESS on this exact commit.
- Linux job `101523006968` passed formatting/dictionary/staging/locked-graph
  checks, workspace/sidecar compilation, Clippy, full workspace and sidecar tests,
  authenticated loopback SSH fixtures and whitespace. Completed `17:12:43Z`.
- Windows job `101523007135` passed compilation and all regression steps.
  The four nonempty native navigation suites were discovered and executed;
  that step passed at `17:11:08Z`. Onboarding, Files, Tasks, Git and terminal-host
  regressions also passed. The job completed `17:17:08Z`.
- Same-run rootfs job `101523007104` passed. Actual WSL1 job `101523147769`
  and WSL2 job `101523147759` passed transport execution and owned cleanup,
  completing at `17:05:14Z` and `17:06:18Z` respectively. All times here are UTC
  on 2026-09-06 (2026-09-07 in the developer's timezone).
- Windows Package: https://github.com/vihor3/mini-term/actions/runs/34046725815
  Job `101522992431` completed SUCCESS at `17:16:21Z` on the same commit.
  Artifact `9993700175`: `Mini-Term_1.2.2-ci.70_windows-x64`.
  https://github.com/vihor3/mini-term/actions/runs/34046725815/artifacts/9993700175
- Downloaded installer and `windows-package-validation.json` to
  `/home/leo/Downloads/mini-term-1.2.2-ci.70/`. The Actions manifest reports
  `status=passed`, the exact commit/run, and all eight staged/extracted payloads
  matching. Installer size: 18,874,602 bytes; Actions SHA-256:
  `ef0a4abfdea9831db38ec0151c6d5dfaad41b13c878aa3ae41fe5b48ceb8b523`.
- Final evidence is the GitHub job/step API and downloaded Actions manifest.
  Raw job-log downloads remained intermittent, so no new per-test totals or
  guest-kernel values are inferred from older runs. No local hash check, build,
  test, formatter, installer execution or application launch was performed.
- Native acceptance remains OPEN for caption/body alignment during resize,
  collapse/overlays, actual window hitboxes, keyboard/pointer focus, tooltips,
  terminal activation/close/reorder and retained worktree tool content. Source
  review and pure tests do not establish native screenshot/interaction acceptance.

### Actions Iterations

Chronological intermediate observations below are superseded by the validated
candidate above, not separate claims of final status.

- Native source commit: `80245911fadb7c3ace75aa61d93875ac188ef9bb`.
  CI34044489937 failed changed-line formatting; Windows affected-package
  compilation passed before its running navigation tests were superseded.
  Both WSL imports completed before the replacement push, and both cancelled
  jobs completed their owned-distro cleanup successfully. No full test pass is
  claimed for this cancelled run.
- Repeated TLS failures prevented artifact and job-log downloads from GitHub's
  backing storage, while the GitHub API remained available. CI-only commit
  `dad94854b63adfcb767fd459de700742febb641e` publishes the whole existing
  formatting artifact as a bounded base64 check annotation, without truncating
  or selecting hunks. The original artifacts remain unchanged. CI34045003168
  is the diagnostic follow-up, not a source correction or acceptance result.
- That run demonstrated a runner-side 4096-character annotation limit. No
  partial patch was applied. `5ea48da36c5358acdd8df7685b561688645a98f0` changed
  only the transport: gzip the whole artifact, number JSON-framed base64 chunks
  below 4096 characters, and publish all parts or none. Pasteur approved this
  bounded source change. CI34045191525 delivered the complete 7011-byte patch
  in one annotation. Main downloaded/decompressed that API payload and applied
  the whole contextual patch to three owned Rust files, with no local formatter.
  The prior diagnostic run was cancelled only after both imports completed;
  both owned WSL cleanups succeeded.
- Formatting-only source correction: `e3463c7c2a15aeb83b8d3a90a2005e62197e11ef`.
  CI34045396180 passed formatting, dictionary/staging/locked-graph gates and
  Linux/Windows affected compilation, then failed the changed-line Clippy gate.
  Tests were not complete. Both superseded WSL jobs cleaned their owned distro.
  CI-only `a8708a1ba326444ddbbece4d605c99fc0de625a4` adds standard source/line
  annotations to the unchanged warning matcher and exit-status gate, since
  backing-storage job-log downloads still failed. Pasteur approved the scoped
  diagnostic source change; CI34046038785 will identify the warnings. The
  corresponding WindowsPackage34045396159 is still running, not yet validated.
- The diagnostic iteration identified exactly one changed-line warning:
  `clippy::assertions_on_constants` in the new collapsed-footer regression at
  `orca_sidebar.rs:1381`. Euclid is converting that width invariant to an
  inline const assertion, preserving the check without a suppression or UI
  behavior change. The standard annotation reached the GitHub API as intended.
- Euclid completed that one-line test-only correction and Pasteur approved it.
  Candidate `f90993afce7c04f3d6a359fdf5bee0a9016d4de9` starts CI34046725805
  and WindowsPackage34046725815 (package run70). Formatting is already passing;
  full checks/tests/package remain pending. Both superseded diagnostic WSL jobs
  completed their owned cleanup after their imports had finished.
- On `f90993a`, both compilation gates and Clippy passed. Actual WSL2 job
  `101523147759` and WSL1 job `101523147769` both completed SUCCESS, including
  exact transport execution and owned cleanup. Linux full tests, Windows
  navigation/regression suites and installer validation are still pending.

## Historical Candidate

- Commit: `f7b8a7e74dc901203b1fd4606310a8ad78f3503f`.
- Branch: `vihor3/mini-term`, `feat/remote-file-management`.
- CI: https://github.com/vihor3/mini-term/actions/runs/33997336854
- CI conclusion: SUCCESS, both jobs completed.
- Linux job: `101390060323`, completed `2026-09-05T23:13:21Z`.
- Windows job: `101390060222`, completed `2026-09-05T23:21:14Z`.

This is the navigation candidate only. Agent, full folder-browser, remote Git
and Tasks account feature changes remain outside this committed candidate.
Passing old baseline tests in those modules does not validate their new slices.

## Historical Gates

- Linux: all four real formatting-artifact fixtures, changed-line rustfmt,
  generated i18n, staging tests, locked graphs, workspace/sidecar compilation,
  affected-package Clippy, workspace all-target tests, sidecar tests and
  whitespace passed.
- Windows: locked graphs, affected-package all-target MSVC compilation,
  focused onboarding and existing FileTree/picker tests, sidecar compilation
  and terminal-host all-target regressions passed.
- The formatter tests applied both complete contextual artifacts to disposable
  Actions repositories and compared against full formatter output. No local
  formatter, code generator, test, fixture or automated verification ran.

## Historical Package

- Windows Package: https://github.com/vihor3/mini-term/actions/runs/33997336860
- Packaging job: `101390058223`, SUCCESS at `2026-09-05T23:21:50Z`.
- Artifact: `Mini-Term_1.2.2-ci.36_windows-x64`, ID `9978757862`.
- Artifact page: https://github.com/vihor3/mini-term/actions/runs/33997336860/artifacts/9978757862
- Downloaded installer and Actions-generated `windows-package-validation.json`
  to `/home/leo/.cache/mini-term/artifacts/f7b8a7e-navigation/`.
- Manifest reports `status=passed`, the exact candidate/run above, all eight
  staged/extracted payload hashes matching, required PE architectures/resources
  and feature markers. Installer: `Mini-Term_1.2.2-ci.36_x64-setup.exe`, 18,388,759
  bytes, Actions SHA-256
  `aa75305521da775aa3c4da44fc9fcec207154456627cdcc41a2fff67dabcbe06`.
- Artifact identity and runner payload validation are accepted. No local hash
  check, installer execution or application launch was performed.
- Native acceptance remains OPEN: titlebar geometry/overflow/drag controls,
  one-terminal rendering and preserved background PTYs, exact close/fork/
  reconnect, footer/project-menu pointer and keyboard behavior, warm tooltips,
  global right-tool selection, and compatibility with existing document pages.
- Native observations must use the matching Actions-produced artifact. Source
  review and CI success do not establish screenshot/interaction acceptance.

## Earlier Corrections

The initial selected-hunk formatter artifact was unsafe and damaged two source
locations. `4947f55` restored the import and declaration order; `8f234c0` changed
both machine-applicable diagnostics to complete U3 patches and added execution
fixtures. `f7b8a7e` fixes three Clippy diagnostics without staging concurrent
Agent code. See `formatter-followup.md` and the parent's chronological progress.

Earlier failed/cancelled runs and older installers are not substitute evidence.
