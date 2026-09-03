# Audit Orca Runtime Task Changes

## Goal

Perform a differential code review of only the product, test, CI, and packaging
changes introduced while implementing the archived Orca Project Worktree Runtime
program. Fix confirmed defects and strengthen robustness without turning the audit
into a review or refactor of unrelated pre-existing code.

## Review Boundary

- Baseline: `0bc6f28` (`chore(task): archive 09-01-orca-worktree-terminal-research`).
- Target: `c644ae9` (`fix: synchronize sidecar runtime lockfile`).
- Authoritative review diff: `git diff 0bc6f28..c644ae9`.
- The range contains 193 changed files, including 93 non-Trellis files and 73 Rust
  files. Archived task/spec documents are evidence for intended behavior; product,
  tests, Docker, CI, sidecar, and installer files are the implementation under review.
- A finding is in scope only when its causal change intersects this range. Older code
  may be read to understand contracts, but an unrelated defect that existed at the
  baseline is not an audit finding and must not be opportunistically fixed.
- A scoped fix may minimally edit adjacent pre-existing lines when necessary to repair
  the introduced behavior. It must not broaden into cleanup, redesign, or formatting
  churn outside the defect.

## Requirements

1. Build a complete changed-file inventory and assign every implementation file to a
   review domain so large or low-visibility files are not silently skipped.
2. Review the diff against the archived parent/child requirements and current code for:
   identity and generation fencing, stale async results, concurrency and cancellation,
   resource/process lifecycle, persistence and migration safety, bounded I/O and memory,
   error classification and recovery, command/path injection, credential leakage,
   platform-specific behavior, rollback controls, UI scope/focus routing, and release
   reproducibility.
3. Use independent reviewers for the five domains: catalog/identity/persistence,
   terminal host/history, remote runtime/Agent/GitHub, Orca UI/context, and GitHub
   Actions/Windows release. Consolidate duplicate findings centrally before editing.
4. Reproduce every actionable finding with a focused test or deterministic inspection.
   Add regression coverage for behavior changes and fault paths before or with the fix.
5. Fix every confirmed correctness, security, data-loss, crash, stale-routing, leak, or
   recovery defect that can be repaired within the review boundary. Low-risk local
   quality improvements are allowed only when they directly reduce complexity or make a
   reviewed invariant executable.
6. Preserve approved product behavior, persisted identity formats, compatibility
   projections, and rollback environment variables unless a confirmed defect requires a
   compatible correction.
7. Run all formatting checks, Rust compilation, linting, tests, Windows target checks,
   sidecar staging, installer builds, and package validation only in GitHub Actions.
   The local machine may edit, inspect diffs, perform Git operations, and clean artifacts,
   but must not execute build, test, lint, staging, packaging, or CI commands.
8. Preserve all unrelated dirty files and the active `00-bootstrap-guidelines` task.
   Do not stage or rewrite their content.
9. If runtime or release payload code changes, GitHub Actions must rebuild, extract,
   structurally verify, and upload a new Windows installer. Never retain or treat a
   locally built package as task evidence.
10. Record confirmed findings, fixes, tests, residual risks, and any deliberately rejected
    out-of-scope observations in the task validation artifact.

## Acceptance Criteria

- [x] Every non-Trellis file in `0bc6f28..c644ae9` is mapped to one review domain and
  reviewed at least once; high-risk cross-layer paths receive a second integration pass.
- [x] Findings include file/line evidence, severity, violated invariant, causal commit or
  diff hunk, and disposition. No finding is filed solely against baseline code.
- [x] All confirmed in-scope correctness and robustness defects are fixed with focused
  regression tests, or explicitly documented as residual risk with a concrete reason the
  scoped task cannot safely change them.
- [x] Stable host/repository/worktree/pane/terminal/Agent identities and generation fences
  remain consistent across persistence, reconnect, detach/reattach, replay, and UI routing.
- [x] Terminal and remote operations remain bounded, cancellation-safe, leak-resistant,
  and precise about retry/fallback/session-retirement behavior.
- [x] GitHub Tasks and remote probes remain execution-host scoped, injection-resistant,
  secret-safe, and stale-result fenced.
- [x] Orca Project -> Worktree UI state, preview semantics, right-sidebar scope, and global
  Agent routing remain isolated by worktree and stable under rapid switching/closing.
- [x] GitHub Actions targeted tests plus full workspace test/check/Clippy gates pass;
  sidecar locked gates and Windows checks pass for every affected release component.
- [x] GitHub Actions produces, extracts, hash-verifies, and uploads a rebuilt installer
  when payload code changes; no local package is used as evidence.
- [x] Task-related paths are committed and archived without including unrelated dirty files.

## Out Of Scope

- Reviewing or fixing defects that were already present at `0bc6f28`.
- Re-evaluating approved Orca product requirements or introducing new features.
- Broad style rewrites, repository-wide formatting, dependency upgrades without a finding,
  or speculative abstractions.
- Pixel-perfect visual redesign or claiming a physical Windows GPU smoke test from Linux.
- Completing or modifying the separate `00-bootstrap-guidelines` task.
