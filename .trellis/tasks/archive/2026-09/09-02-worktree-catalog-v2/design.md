# Technical Design

## Change Boundary

The smallest behavior gap is that current worktree listing cannot distinguish a complete Git inventory from a partial/fallback result. The change belongs in `mt-project`, where Git facts are read and parsed, with narrow app changes only where asynchronous scan ownership or destructive cleanup currently violates that contract.

Expected production changes:

- `crates/mt-project/src/worktree/mod.rs`: public fact/result types and legacy projections.
- `crates/mt-project/src/worktree/porcelain.rs`: pure raw-byte NUL/text parsing and C-quote decoding.
- `crates/mt-project/src/worktree/catalog.rs`: command boundary, capability fallback, per-repo cache/single-flight, generation, and invalidation.
- `crates/mt-project/src/lib.rs`: export the domain module.
- `crates/mt-project/src/git.rs`: delegate legacy APIs and invalidate the catalog after mutations.
- `crates/mt-app/src/git_worktree.rs`: request generation, rich scan consumption, and case-correct grouping.
- `crates/mt-app/src/project_list.rs`: request generation, last-known preservation, and authoritative-only reconciliation.

This child explicitly does not introduce the final stable-ID crate or persistence schema.

Build and test isolation is owned by `docker/ci/Dockerfile`, `docker-compose.ci.yml`, and `scripts/docker-ci.sh`. The repository is bind-mounted read-only at `/workspace`; `/cargo` and `/target` are bind-mounted from the Docker-only cache under `~/.cache/mini-term/docker-ci`. The host UID/GID owns cache files, and no host Rust installation is part of the supported workflow.

## Domain Types

```rust
pub enum WorktreeScanSource {
    PorcelainZ,
    PorcelainText,
    LastKnown,
    Libgit2Fallback,
}

pub struct GitAnnotation {
    pub reason: Option<String>,
}

pub enum WorktreePathState {
    Present,
    Missing,
    Unknown,
}

pub struct WorktreeFact {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch_ref: Option<String>,
    pub is_main: bool,
    pub is_detached: bool,
    pub is_bare: bool,
    pub is_sparse: bool,
    pub locked: Option<GitAnnotation>,
    pub prunable: Option<GitAnnotation>,
    pub path_state: WorktreePathState,
}

pub struct WorktreeScan {
    pub generation: u64,
    pub source: WorktreeScanSource,
    pub authoritative: bool,
    pub worktrees: Vec<WorktreeFact>,
    pub warning: Option<String>,
}
```

`Option<GitAnnotation>` preserves all three states: absent, present without reason, and present with reason. `branch_ref` keeps `refs/heads/...`; display projections remove the prefix at the boundary.

## Parser Contract

### NUL Mode

- Input is `&[u8]` from successful `git worktree list --porcelain -z`.
- NUL separates fields and an empty field closes a record. A final complete record may be accepted without an extra empty delimiter.
- Field prefixes are decoded only after structural splitting. Path/reason values must be valid UTF-8 for the current cross-platform model; invalid bytes reject the complete scan.
- Unknown tokens are ignored. Duplicate identical fields are tolerated; conflicting duplicates reject the complete scan.

### Text Mode

- Blank lines separate records; CRLF and LF are accepted.
- Git C-style quoted path and reason values are decoded, including escaped control characters and octal byte sequences.
- Malformed quoting or decoded invalid UTF-8 rejects the complete scan.

Only records containing a worktree path are valid. Missing optional facts remain `None`/false; a `detached` token sets `is_detached` even when no branch is present.

## Command And Fallback Flow

```text
scan(repo, requested_generation)
  -> per-repo single-flight gate
  -> run raw argv: git worktree list --porcelain -z
     -> success + strict parse: PorcelainZ, authoritative
     -> verified unsupported -z: run --porcelain
        -> success + strict parse: PorcelainText, authoritative
     -> other command/parse failure:
        -> last authoritative cache: LastKnown, non-authoritative + warning
        -> otherwise libgit2 projection: Libgit2Fallback, non-authoritative + warning
        -> otherwise return error
```

The raw command result includes exit status, stdout bytes, and stderr bytes. Arguments are structured and never shell-concatenated. Repository validation asks Git rather than requiring a `.git` directory, so bare repositories remain valid inputs.

For old text-mode Git, existence enrichment is bounded and skips main, bare, locked, and already-prunable rows. An IO error other than not-found leaves `path_state = Unknown`; it does not prove pruning.

## Generation And Cache

- Each repository key owns a mutation generation and last authoritative snapshot.
- Concurrent callers for the same generation share one in-flight scan; different repositories do not block one another.
- Successful add/remove/prune increments the affected repository generation and clears current scan freshness while retaining last authoritative data for degraded display.
- A scan result carries the generation captured before command execution. App consumers compare both their request generation and the catalog generation before applying it.
- Non-authoritative results may update diagnostics/last-known display but never produce destructive absence.

## Compatibility Projection

`mt_project::git::WorktreeInfo` remains source-compatible. Its fields project as follows:

- `name`: final path component, falling back to `main`.
- `path`: display string with trailing separators removed.
- `branch`: short local branch from `branch_ref`; prunable rows retain it.
- `is_main`: `WorktreeFact::is_main`.
- `is_valid`: not prunable and path state is not `Missing`.
- `is_locked`: locked marker present.

Legacy APIs never become a second authority. They call the default catalog and project its result.

## Existing UI Ownership

- `WorktreeModal` increments `load_generation` before every load. Completion applies only when it matches; mutation-triggered reloads therefore fence older scans.
- Group keys compare native/canonical paths with platform-appropriate case rules; they never lowercase all paths.
- `ProjectList` badge probes preserve the old map on failure/non-authoritative results and apply only the latest request generation.
- Reconciliation groups legacy child projects by parent repository, requests an authoritative catalog, and removes a child only when its registered path is absent from that authoritative result. Missing directories without Git evidence remain visible/stale.

## Rollback

The legacy public APIs remain available and the old libgit2 enumerator is retained as a non-authoritative fallback. This child introduces no persisted schema and the repository has no general runtime feature-gate facility, so rollback is a code revert to the legacy implementation while preserving the unchanged public API; no synthetic one-off gate is added.
