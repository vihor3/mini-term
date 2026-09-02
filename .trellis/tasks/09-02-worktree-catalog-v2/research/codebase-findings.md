# Worktree Catalog V2 Codebase Findings

Audit date: 2026-09-02.

## Current Implementation

- `crates/mt-project/src/git.rs` exposes one six-field `WorktreeInfo` and `list_worktrees(&Path)` backed by libgit2.
- Linked worktree main-repo discovery walks two parents from the Git directory, which assumes a specific on-disk layout instead of asking Git.
- Partial enumeration, skipped metadata, and a complete list are indistinguishable. Bare, sparse, detached, lock/prunable reasons, HEAD OID, and scan authority are absent.
- Invalid/prunable rows lose branch occupancy because the current code only reads the branch when `path.exists() && validate()` succeeds.
- The existing CLI runner returns lossy `String`, rejects bare repositories through a `.git` existence check, and cannot classify unsupported `-z` separately from ordinary failures.

## Current UI Risks

- `crates/mt-app/src/git_worktree.rs` has no load generation; an older background result can replace a newer refresh.
- Repository grouping lowercases all paths, which can merge distinct POSIX paths.
- `crates/mt-app/src/project_list.rs` badge probes can clear state after transient failure and have no request generation.
- Legacy worktree child reconciliation deletes projects from filesystem absence alone; it has no authoritative Git proof.

## Orca Evidence

The archived research and Orca source show the required pattern:

- Prefer `git worktree list --porcelain -z` for newline-safe fields.
- Fall back to text porcelain only when old Git rejects `-z`.
- Preserve `bare`, `sparse`, `locked`, `prunable`, reasons, HEAD, branch ref, and main ordering.
- On old Git, existence-probe only eligible linked rows to recover prunable state.
- Treat source/authority as a safety boundary; degraded scans can preserve display but cannot prove deletion.

Normative research: `.trellis/tasks/archive/2026-09/09-01-orca-worktree-terminal-research/research/orca-worktree-terminal-agent-architecture.md`.

## Identity Boundary

No existing durable type can safely serve as `ExecutionHostId`, `RepoId`, or `WorktreeId`:

- `SshConnection.id` is a mutable saved-profile key.
- current connection fingerprints are process-random and include credentials.
- `DefaultHasher`/`u64` uses are temporary UI/runtime keys, not persisted contracts.
- lowercased paths, branch names, display names, and project IDs are compatibility projections.

The dependent identity task should introduce shared strong ID types, domain-separated SHA-256 derivation, UUID-v4 host-install seeds, and separate preserved mappings. This child deliberately stops at authoritative Git facts and path contracts because the remote runtime does not yet expose a trustworthy host-install identity.
