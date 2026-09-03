# Review Scope Inventory

Date: 2026-09-03; policy addendum 2026-09-03

## Causal Range

```text
baseline 0bc6f28  chore(task): archive 09-01-orca-worktree-terminal-research
target   c644ae9  fix: synchronize sidecar runtime lockfile
diff     git diff 0bc6f28..c644ae9
```

The range has 193 changed files, 33,345 insertions, and 1,404 deletions. It contains
93 non-Trellis files and 73 Rust files. The final parent validation/archival/journal
commits after `c644ae9` are evidence and bookkeeping, not product code under audit.

## Work Commits

- `0848513` local Docker Rust check harness (retired by the Actions-only policy).
- `3f386f2` authoritative worktree catalog.
- `5188188` Orca Project -> Worktree shell.
- `0386e6b` stable host/repo/worktree/workbench identities and persistence.
- `8e8a7dd` detached terminal host and warm reattach.
- `c89250e` bounded terminal snapshots and cold restore.
- `40714bf` authenticated remote runtime identity.
- `9e08319` authenticated remote Agent identity/status.
- `572c832` worktree-scoped Files/Git/Sessions context.
- `5fb072e` execution-host GitHub Tasks.
- `e1eacca` global Agent activity feed.
- `25430a0` integration-test portability/race fixes.
- `c644ae9` independent sidecar lock synchronization.

## Primary File Distribution

```text
mt-app            38 files
mt-terminal-host   9 files
mt-project         8 files
mt-ssh             6 files
mt-github          6 files
mt-terminal        3 files
mt-config          3 files
mt-ai              3 files
mt-pty             2 files
mt-layout          2 files
mt-identity        2 files
mt-ui              1 file
```

Additional scope includes root Cargo wiring/locks, `sidecars/Cargo.lock`, the legacy
local Docker CI harness, release workflow, sidecar staging, and NSIS packaging. The
2026-09-03 policy addendum makes GitHub Actions the sole executable validation and build
authority and retires the task-added local Docker harness.

## Exclusions

- Commits at or before `0bc6f28`, except as read-only context.
- Task archive moves, validation prose, and journal commits as implementation targets.
- Existing dirty bootstrap/spec/journal files not created by this audit.
