# Worktree Catalog V2

## Goal

Replace libgit2-only worktree listing with an authoritative Git porcelain catalog that preserves Git facts, distinguishes authoritative and fallback results, and prevents stale or uncertain scans from deleting or overwriting current worktree state.

This is the first implementation child of `09-02-orca-project-worktree-runtime`. It establishes the Git fact/path contract consumed by the later stable-identity and Orca UI tasks.

## Requirements

### R1. Byte-Oriented Porcelain Parser

- Parse `git worktree list --porcelain -z` from raw bytes before any lossy string conversion.
- Preserve worktree path, HEAD OID, full branch ref, detached, bare, sparse, locked marker/reason, prunable marker/reason, and first-row/main status.
- Support valid UTF-8 paths containing spaces or newlines. Invalid UTF-8, malformed C quoting, conflicting duplicate fields, or malformed record structure must fail the whole authoritative parse rather than return a partial deletion-capable list.
- The text fallback must decode Git C-quoted path/reason fields. Unknown fields are ignored for forward compatibility.

### R2. Catalog Source And Authority

- Try `git worktree list --porcelain -z` first. Retry without `-z` only for a verified unsupported-option signal such as exit code 129; ordinary Git failures must not silently change parser mode.
- Successful NUL and complete text porcelain scans are authoritative. Cached or libgit2 compatibility results are non-authoritative.
- The catalog result exposes source, authority, generation, warning, and rich worktree facts. A marker without a reason must remain distinguishable from no marker.
- On older Git text fallback, a missing linked-worktree directory is marked prunable only when the row is not main, bare, locked, or already prunable.
- The catalog coalesces concurrent scans for the same repository and preserves the last authoritative result for degraded display.

### R3. Compatibility API

- Keep the current public `mt_project::git::WorktreeInfo`, `list_worktrees`, `get_worktree_branches`, add/remove/prune, repository discovery, and branch APIs source-compatible during this child.
- Move the new ownership into `mt_project::worktree`; legacy APIs delegate to compatibility projections instead of maintaining a second parser.
- Preserve branch occupancy for prunable rows so the create UI cannot incorrectly offer an already registered branch.
- Bare and detached rows remain representable even if the legacy projection cannot expose every rich fact.

### R4. Async Ownership In Existing UI

- Worktree modal loads and project-list badge probes carry a request generation; an older background result cannot replace a newer refresh or post-mutation result.
- A non-authoritative empty/degraded result preserves last-known worktree rows and badges instead of flashing to empty.
- Automatic removal of legacy worktree child projects requires an authoritative catalog result that proves the registration disappeared. Filesystem absence alone is insufficient.
- Path grouping must not lowercase POSIX paths or otherwise treat display normalization as repository identity.

### R5. Mutation Invalidation

- Successful worktree add/remove/prune invalidates repository discovery and the affected catalog generation.
- Results started before mutation invalidation cannot become the current authoritative scan afterward.
- Existing mutation behavior and confirmation UI remain unchanged except for catalog invalidation and stale-result fencing.

### R6. Docker-Only Verification

- The host must not require or retain a Rust toolchain, Cargo registry, or repository-local `target` directory for this task.
- All Rust compilation, checks, formatting gates, Clippy runs, and tests execute through `scripts/docker-ci.sh`.
- The source tree is mounted read-only in the CI container; Cargo registry and target data live only in the documented Docker cache directory.
- The Docker entrypoint must preserve the official Rust toolchain path for both direct commands and scripted suites.

## Acceptance Criteria

- [ ] NUL fixtures cover spaces, newline paths, detached, bare, sparse, locked/prunable with and without reasons, unknown fields, malformed UTF-8, malformed quoting, and conflicting records.
- [ ] Text fixtures produce equivalent facts for representable paths/reasons and use C-quote decoding.
- [ ] Unsupported `-z` falls back to text; unrelated exit failures do not. Both successful modes are authoritative.
- [ ] Non-authoritative empty/failure results retain last-known rows and cannot authorize deletion.
- [ ] Concurrent scans coalesce per repository, and mutation generation fences pre-mutation results.
- [ ] Existing `mt_project::git` callers compile without signature changes and receive compatibility projections from the new catalog.
- [ ] Prunable rows retain branch occupancy; bare/detached/locked rows are not silently omitted.
- [ ] Worktree modal and project-list probes reject stale generations; legacy child-project cleanup only follows authoritative Git absence.
- [ ] `scripts/docker-ci.sh worktree` and the Docker changed-line formatting gate pass without creating host Rust or repository-local build artifacts.

## Out Of Scope

- Durable `ExecutionHostId`, `RepoId`, `WorktreeId`, pane, terminal, or Agent ID types and persistence. Those belong to the dependent identity child once a real host-install identity source exists.
- Remote SSH/WSL command transport. This child defines local Git command semantics that Phase 5 will implement on remote execution hosts.
- The Orca sidebar/workbench visual migration, detached PTY host, terminal restore, Agent relay, GitHub Tasks, or Agents overlay.
- Worktree mutation redesign, force-delete semantics, new creation UX, or schema migration beyond catalog cache/generation state.

## Technical Notes

- The 2026-09-02 code audit found no reusable durable identity type. Do not use `SshConnection.id`, connection fingerprints, `DefaultHasher`, process counters, branch names, or lowercased paths as future identity inputs.
- The current string-returning Git runner is not suitable for `-z`; the worktree catalog needs a structured raw-output command boundary.
- Source research is archived under `.trellis/tasks/archive/2026-09/09-01-orca-worktree-terminal-research`.
