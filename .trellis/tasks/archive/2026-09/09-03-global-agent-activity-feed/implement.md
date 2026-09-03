# Implementation Plan

1. Add per-run event acknowledgement watermarks and exact activation-then-ack
   behavior to `AppStore`; prune stale watermarks and add regression tests.
2. Extend `AgentTargetView` with current event/unread facts and add pure
   Needs You/Working/Recent grouping, ordering, and Recent bounding tests.
3. Replace the global overlay's legacy `ai_projects(DoneScope::All)` rows with
   exact target rows showing provider, project/worktree, pane, activity,
   connectivity, unread state, and receipt time.
4. Keep row activation keyed by `AgentRunId`; close only on success and leave a
   failed stale target unacknowledged and visible until authoritative removal.
5. Wire the Agents entry badge to exact Needs You/unread rows while preserving
   the existing overlay stack, anchored geometry, outside/Escape/toggle close,
   and focus return.
6. Add `MINI_TERM_GLOBAL_AGENT_ACTIVITY=0` rollback and tests that inline rows,
   Sessions, and runtime state remain unaffected.
7. Add pure/interaction tests for multi-host identity, duplicate runs, new-event
   unread renewal, stale target failure, focus restoration, viewport geometry,
   and no route/workbench mutation on open.
8. Update Agent feed contracts, run Docker-only rustfmt/focused tests, workspace
   check/Clippy, Windows MSVC check, commit, and archive.

## Validation

```text
./scripts/docker-ci.sh run cargo test --locked -p mt-app agent_activity --no-fail-fast
./scripts/docker-ci.sh run cargo test --locked -p mt-app store::context --no-fail-fast
./scripts/docker-ci.sh run cargo test --locked -p mt-ai agent_runtime --no-fail-fast
./scripts/docker-ci.sh run cargo check --locked --workspace --all-targets
./scripts/docker-ci.sh run cargo clippy --locked -p mt-ai -p mt-app --all-targets --no-deps
cargo xwin check --locked --target x86_64-pc-windows-msvc -p mt-app
```

## Rollback Point

Set `MINI_TERM_GLOBAL_AGENT_ACTIVITY=0` or revert this child. Do not roll back
remote Agent identity, worktree context, terminal recovery, or Sessions history.
