# Git Validation Evidence

## Scope: CLI Domain Only

Commit `2c8d1f1408f04b204c0b87d00208edb384a20b0f` includes the independently
reviewed CLI plans/parsers, directory-status projection and history-body parity
correction. It does NOT include the uncommitted app host adapter, native helper,
Git UI/worktree cleanup, Agent, Files or Tasks-account implementation.

- [CI 34000949862](https://github.com/vihor3/mini-term/actions/runs/34000949862):
  completed SUCCESS on that exact head SHA. Linux job `101399624211` and Windows
  MSVC job `101399624267` both succeeded.
- Linux formatting diagnostics, generated i18n, locked root/sidecar graphs,
  compile, changed-line Clippy, root/sidecar tests and whitespace gates passed.
  `mt-project`: 172 passed, zero failed. The original all-parent history/full
  DTO parity fixture and both new body-whitespace regressions explicitly passed.
- [Windows package 34000949832](https://github.com/vihor3/mini-term/actions/runs/34000949832):
  completed SUCCESS on the same SHA, including staged/extracted installer
  validation. Artifact `9979775690`, `Mini-Term_1.2.2-ci.40_windows-x64`.
- No local build, formatter, test, fixture, hash check or app launch is evidence.
  Exact-SHA adapter/UI/transport/native gates remain open. A prior package is
  not the full requested feature delivery, and this child is not archived.
