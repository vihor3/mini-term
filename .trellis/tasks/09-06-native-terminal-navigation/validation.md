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
- Exact-commit CI, packaging and native artifact acceptance are pending. The
  historical candidates below do not validate this new native source.

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

## Executed Gates

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

## Package And Native Acceptance

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
