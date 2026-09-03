# Implementation Plan

1. Add canonical worktree path, Agent target, exact activation, and terminal
   diagnostic read APIs to `AppStore`, with pure ordering and route-validation
   tests.
2. Add the Phase 8 rollback gate and render exact Agent rows below configured
   worktrees in `OrcaProjectSidebar` using the shared activation action.
3. Add FileTree worktree scope/cache/selection/scroll ownership while retaining
   existing source-signature and remote connection fences.
4. Replace GitPanel process-global presentation cache with per-WorktreeId state;
   invalidate repository, branch, and mutation completions on scope changes.
5. Add SessionPanel per-worktree history/preview/scroll state and worktree
   generation fences; use canonical paths and merge read-only runtime badges.
6. Render bounded terminal recovery and remote Agent probe diagnostics in the
   Sessions tab, with connectivity separate from activity.
7. Add interaction/pure tests for scope restoration, stale result rejection,
   target routing, ordering, rollback, and diagnostic labels.
8. Update the worktree context spec, run Docker-only formatting, focused tests,
   workspace check/Clippy, Windows MSVC check, commit, and archive.

## Validation

```text
./scripts/docker-ci.sh run cargo test --locked -p mt-app orca_sidebar
./scripts/docker-ci.sh run cargo test --locked -p mt-app file_tree
./scripts/docker-ci.sh run cargo test --locked -p mt-app git_panel
./scripts/docker-ci.sh run cargo test --locked -p mt-app session_panel
./scripts/docker-ci.sh run cargo test --locked -p mt-app agent_target
./scripts/docker-ci.sh run cargo check --locked --workspace --all-targets
./scripts/docker-ci.sh run cargo clippy --locked -p mt-ai -p mt-layout -p mt-app --all-targets --no-deps
cargo xwin check --locked --target x86_64-pc-windows-msvc -p mt-app
```

## Rollback Point

The predecessor shell/runtime commits remain intact. Revert this child or set
`MINI_TERM_ORCA_WORKTREE_CONTEXT=0`; do not roll back identity, layout, terminal
host, snapshot, remote runtime, or Agent protocol schemas.
