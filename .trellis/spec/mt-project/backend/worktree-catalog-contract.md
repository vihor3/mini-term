# Worktree Catalog Contract

## Scenario: Authoritative worktree inventory

### 1. Scope / Trigger

Use this contract whenever `mt-project` or `mt-app` reads Git worktree facts,
projects legacy worktree rows, invalidates worktree state after mutation, or
removes persisted/UI state because a worktree appears absent.

The trigger is safety-critical: a partial libgit2 result, failed command, stale
async completion, or path-normalization collision must never become proof that
a registered worktree disappeared.

### 2. Signatures

```rust
pub fn scan(repo_path: &Path) -> anyhow::Result<WorktreeScan>;
pub fn invalidate(repo_path: &Path);
pub fn current_generation(repo_path: &Path) -> u64;

pub struct WorktreeScan {
    pub generation: u64,
    pub source: WorktreeScanSource,
    pub authoritative: bool,
    pub worktrees: Vec<WorktreeFact>,
    pub warning: Option<String>,
}
```

Compatibility callers continue to use `mt_project::git::WorktreeInfo`,
`list_worktrees`, `get_worktree_branches`, `add_worktree`,
`remove_worktree`, and `prune_worktrees`. These APIs delegate to or invalidate
the catalog; they must not introduce a second parser or authority model.

### 3. Contracts

- Run structured argv `git worktree list --porcelain -z` and parse raw bytes
  before any lossy conversion.
- Retry text porcelain only when `-z` is unsupported (currently exit code 129).
  Ordinary Git failures do not change parser mode.
- Successful complete NUL/text porcelain scans are authoritative. Last-known
  and libgit2 fallback scans are non-authoritative and carry a warning.
- Preserve full branch refs, HEAD, detached/bare/sparse flags, locked/prunable
  markers and optional reasons, path state, and first-row/main identity.
- Main and linked worktrees share the canonical Git common-dir cache key.
  Concurrent scans coalesce per repository generation.
- Successful add/remove/prune increments the repository generation. A result
  started before invalidation is downgraded and cannot become current authority.
- App consumers fence completion by both request ownership and catalog
  generation. Only a current authoritative scan may prove destructive absence.
- Windows path comparison normalizes separators and case. POSIX comparison
  preserves case and backslashes; display projection must not strip a legal
  POSIX trailing backslash.

### 4. Validation & Error Matrix

| Condition | Required result |
|---|---|
| Valid `--porcelain -z` output | `PorcelainZ`, authoritative |
| Exit 129 for `-z`, valid text output | `PorcelainText`, authoritative |
| Non-129 Git failure | No text retry; last-known/fallback or error |
| Invalid UTF-8, malformed C quote, conflicting field, missing worktree path | Reject the whole authoritative parse |
| Command/parse failure with last authoritative snapshot | `LastKnown`, non-authoritative, warning |
| No snapshot but libgit2 succeeds | `Libgit2Fallback`, non-authoritative, warning |
| Mutation races with an in-flight scan | Downgrade stale result; current generation wins |
| Non-authoritative empty result in app | Preserve last-known rows/badges |
| Filesystem path missing but Git still registers it | Keep persisted project; absence is not proven |
| Current authoritative inventory omits registered child path | Cleanup may remove the legacy child project |

### 5. Good / Base / Bad Cases

- Good: a prunable row keeps `refs/heads/feature`, so the create UI still treats
  that branch as occupied while the row remains registered.
- Base: a detached or bare row has no local branch but remains representable in
  rich facts and the compatibility projection.
- Bad: a transient Git error returns an empty fallback and the project list
  deletes worktree children. This is forbidden because fallback absence is not
  authoritative.

### 6. Tests Required

- Parser fixtures: spaces, newlines, detached, bare, sparse, locked/prunable
  with and without reasons, unknown fields, invalid UTF-8, malformed quoting,
  conflicting duplicates, and missing worktree path.
- Catalog tests: unsupported `-z`, ordinary failure without retry, last-known
  fallback, common-dir singleflight, mutation generation fencing, timeout
  kill/wait, and a real-Git linked/detached/locked/prunable smoke test.
- App tests: stale request rejection, degraded-result preservation,
  authoritative-only cleanup, branch projection, and POSIX case/backslash
  distinctions.
- Verification runs only through `scripts/docker-ci.sh`: the focused `worktree`
  suite covers `mt-project`, linked `mt-app` checks/tests, and Clippy; `fmt` runs
  the changed-line rustfmt gate. The host retains no Rust toolchain or repository
  `target` directory. Cargo state is isolated in the documented Docker cache.

### 7. Wrong vs Correct

#### Wrong

```rust
if !Path::new(&child.path).exists() {
    store.remove_project(&child.id, cx);
}
```

Filesystem absence can mean a disconnected volume, stale mount, permission
error, or a still-registered prunable worktree.

#### Correct

```rust
if scan.authoritative
    && scan.generation == current_generation(&parent.path)
    && !scan.worktrees.iter().any(|row| paths_equal(&row.path, &child.path))
{
    store.remove_project(&child.id, cx);
}
```

The caller applies absence only when the exact current authoritative inventory
owns the decision.
