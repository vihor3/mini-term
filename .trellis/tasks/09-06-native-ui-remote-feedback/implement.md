# Integration Execution Plan

## Approval Gate

- [x] Main presents the converged parent scope and latest layout clarification.
- [x] A subsequent user message explicitly approves that final summary (2026-09-06).
- [x] Curated child implementation/check manifests and designs are read before
  activation. The current old task pointer is not approval of any new child.
- [x] Activate only the owning child; every dispatch starts with the exact path
  returned by `task.py current`. Prefer native context injection, with child-side
  role/manifests/PRD/design/implement loading as fallback.

## Ordered Delivery

1. Terminal navigation: flat individual tabs, single display, no split entry
   points, retained route identity, global right-tool choice, shared tooltips,
   anchored project settings, and footer icons.
2. Agent ownership/status: refine liveness/activity evidence and duplicate
   identity, quiet catalog refresh, and exact Runtime titles against the new
   terminal projection.
3. Files/onboarding: full directory browsing, constrained scrolling, contextual
   creation, and drag-only upload. Preserve the preceding navigation contracts.
4. Remote Git: enumerate and migrate the complete existing action surface to
   the execution host; add bounded read and mutation reconciliation coverage.
5. Tasks accounts: add device-account discovery, independent selected identity,
   secret-safe request execution, cache invalidation, and Tasks selector.
6. Integrated source review, exact-commit Actions, packaged-artifact acceptance,
   affected spec updates, scoped commits, and session wrap-up.

Steps are serialized by default because `mt-app` store/main/terminal context and
execution-host code are shared. A child may be independently reviewed, but final
acceptance explicitly includes all five and R17. Do not archive the earlier
sidebar tasks simply because the new task tree exists.

## Actions-Only Validation

The following are runner commands/workflow steps, NEVER local shell instructions:

| Gate | Actions execution |
| --- | --- |
| Focused regressions | Child-specified tests under existing workspace test jobs |
| Locked graphs | Existing root/sidecars `cargo metadata --locked` steps |
| Linux compilation | `cargo check --locked --workspace --all-targets` |
| Rust regressions | `cargo test --locked --workspace --all-targets --no-fail-fast` |
| Lint/format | Existing changed-line rustfmt and Clippy workflow steps |
| i18n | Existing generator and generated-diff diagnostic artifact step |
| Sidecars | Existing staging/ConPTY and sidecar check/test steps |
| Windows | Existing `windows-msvc` job and `windows-package.yml` installer job |
| Whitespace | Existing Actions `git diff --check` step |

Use `.github/workflows/ci.yml` and `windows-package.yml` on the working fork
`vihor3/mini-term`, not the unrelated upstream CI. Main may push approved scoped
commits, inspect GitHub run logs, and download patches/artifacts. Formatting and
generator corrections are produced in Actions, then applied narrowly; never run
the same tool locally to save time. Do not alter workflow coverage merely to
turn a failure green. No workflows are dispatched during this planning turn.

## Evidence and Finish

- [ ] Implement/check sub-agents perform only allowed local source review;
  execution gates are Actions-only. No recursive implement/check dispatch.
- [ ] Record each exact product SHA, run/job result, and regression coverage;
  a follow-up source fix requires fresh matching Actions evidence.
- [ ] Obtain an Actions-produced Windows installer and its validation manifest.
  Ask the user to verify the reported native interaction cases with that artifact;
  do not launch a local build/probe as an acceptance substitute.
- [ ] Acceptance covers background Agents, old terminal layouts, worktree/tool
  switches, file scrolling/menus/dragging, remote Git, and independent gh accounts.
- [ ] Main updates affected specs with the implemented contracts and caveats.
- [ ] Stage only explicitly owned paths. Do not include unrelated dirty Trellis,
  hooks, config, keys, or source files; no blanket staging or destructive resets.
- [ ] Leave tasks open when native acceptance is outstanding and distinguish
  code/CI completion from observed resolution of the user's screenshots.
