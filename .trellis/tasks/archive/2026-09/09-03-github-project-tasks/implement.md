# Implementation Plan

1. Add `mt-github` to the workspace with normalized repository/work-item types,
   remote URL parsing, structured Git/gh plans, bounded output parsing, and
   authentication/error classification tests.
2. Add an execution-host command boundary in `mt-app`: native bounded process,
   WSL `wsl.exe --exec`, and SSH pooled bounded exec with tested POSIX argv
   serialization and no local fallback.
3. Expose immutable project execution snapshots from `AppStore`, including root
   project grouping, stable host/worktree identity, canonical path, backend
   fingerprint/epoch, and host label.
4. Implement a shared GitHub task service with project/repository/auth cache,
   single-flight list/detail requests, last-known data, Retry generation, and
   source/repository/account stale-result fences.
5. Build `GitHubTasksPanel` with Issue/PR modes, filter, per-worktree selection
   and scroll, compact refresh/auth/empty/error states, Copy, and Retry.
6. Extend `WorkbenchArea` with worktree-scoped read-only work-item preview and
   permanent tabs; add a sanitized internal detail viewer and no URL opener.
7. Replace the Tasks placeholder in `Workspace`, wire store observation and
   lifecycle cleanup, and add `MINI_TERM_GITHUB_PROJECT_TASKS=0` rollback.
8. Add pure and interaction tests for Local/WSL/SSH routing, hostile argv/JSON,
   URL variants, auth/error states, sibling-worktree cache sharing, stale
   completions, preview isolation, and no browser/login/terminal side effects.
9. Update the GitHub Tasks contracts, run Docker-only rustfmt/focused tests,
   workspace check/Clippy, Windows MSVC check, commit, and archive.

## Validation

```text
./scripts/docker-ci.sh run cargo test --locked -p mt-github --no-fail-fast
./scripts/docker-ci.sh run cargo test --locked -p mt-app github_tasks --no-fail-fast
./scripts/docker-ci.sh run cargo test --locked -p mt-app workbench_area --no-fail-fast
./scripts/docker-ci.sh run cargo test --locked -p mt-ssh bounded_exec --no-fail-fast
./scripts/docker-ci.sh run cargo check --locked --workspace --all-targets
./scripts/docker-ci.sh run cargo clippy --locked -p mt-github -p mt-project -p mt-ssh -p mt-app --all-targets --no-deps
cargo xwin check --locked --target x86_64-pc-windows-msvc -p mt-app
```

## Rollback Point

Set `MINI_TERM_GITHUB_PROJECT_TASKS=0` or revert this child. Do not roll back
stable worktree identity, remote runtime authentication, workbench documents,
or the contextual sidebar.
