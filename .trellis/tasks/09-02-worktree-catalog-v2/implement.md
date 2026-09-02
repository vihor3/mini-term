# Implementation Plan

## Step 1: Pure Parser And Types

- Add `mt_project::worktree` public types and parser module.
- Implement strict NUL/text parsing and Git C-quote decoding.
- Add table-driven raw-byte fixtures for normal, edge, forward-compatible, and malformed inputs.

Validation:

```bash
scripts/docker-ci.sh run cargo test -p mt-project worktree::porcelain
```

## Step 2: Raw Command Runner And Catalog

- Add a structured raw-output Git runner for worktree reads.
- Implement verified `-z` capability fallback, path-state enrichment, per-repo single-flight, last-authoritative cache, source/authority semantics, and generation invalidation.
- Add fake-runner tests for unsupported `-z`, ordinary failures, malformed output, concurrency, fallback, and mutation fencing.
- Add one real-Git temporary-repository smoke test for main/linked/detached/locked/prunable behavior when supported by the installed Git.

Validation:

```bash
scripts/docker-ci.sh run cargo test -p mt-project worktree::catalog
```

## Step 3: Legacy API Projection

- Export `pub mod worktree` from `mt-project`.
- Keep `git::WorktreeInfo` and existing function signatures, delegating list/branch probes to the catalog.
- Invalidate catalog generation after successful add/remove/prune alongside the existing repository cache.
- Verify prunable branches remain occupied and bare/detached rows are represented safely.

Validation:

```bash
scripts/docker-ci.sh run cargo test -p mt-project
```

## Step 4: Existing App Fencing

- Add modal and project-list request generations.
- Consume rich scan authority where cleanup decisions are made.
- Preserve last-known rows/badges on degraded scans.
- Replace unconditional lowercase grouping with platform-correct path comparison.
- Add focused tests for late completion, non-authoritative empty, authoritative disappearance, and case-sensitive paths.

Validation:

```bash
scripts/docker-ci.sh run cargo test -p mt-app git_worktree
scripts/docker-ci.sh run cargo test -p mt-app project_list
```

## Step 5: Full Check

```bash
scripts/docker-ci.sh build
scripts/docker-ci.sh worktree
scripts/docker-ci.sh fmt <base-sha>
```

After validation, assert that `target/`, `~/.cargo`, and `~/.rustup` are absent on the host. Docker-only Cargo and target caches may remain under `~/.cache/mini-term/docker-ci` and can be removed with `scripts/docker-ci.sh clean`.

## Validation Results (2026-09-02)

- `scripts/docker-ci.sh worktree`: passed. `mt-project` 119/119, `git_worktree` 8/8, and `project_list` 22/22; `mt-app --tests` check and affected-package Clippy completed successfully.
- `scripts/docker-ci.sh check`: `cargo check --workspace --all-targets` passed.
- `scripts/docker-ci.sh fmt HEAD^`: 52 baseline formatting hunks ignored, 0 changed-line formatting hunks.
- CI changed-line Clippy gate on commit `3f386f2`: 129 baseline warnings ignored, 0 changed-line warnings.
- Host verification: repository `target/`, `~/.cargo`, `~/.rustup`, and shell Cargo/Rust commands are absent. Docker cache remains isolated under `~/.cache/mini-term/docker-ci`.

## Risk And Rollback

- Parser strictness can hide all rows if the format contract is wrong; fixtures and real-Git smoke tests must land before caller migration.
- App reconciliation is destructive; authoritative-only tests are mandatory before enabling it.
- Keep legacy libgit2 enumeration as the non-authoritative fallback. Because this child adds no persisted schema and the repository has no shared runtime feature-gate facility, rollback is a code revert through the unchanged legacy public API rather than a one-off `worktree_catalog_v2` switch.
- Do not add stable ID persistence, remote execution, or unrelated worktree mutation changes in this child.
