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
pub enum WorktreePorcelainMode {
    Nul,
    Text,
}

pub enum WorktreePathSemantics {
    Native,
    Posix,
}

pub fn parse_porcelain(
    mode: WorktreePorcelainMode,
    bytes: &[u8],
) -> anyhow::Result<Vec<WorktreeFact>>;

pub fn parse_porcelain_with_path_semantics(
    mode: WorktreePorcelainMode,
    bytes: &[u8],
    path_semantics: WorktreePathSemantics,
) -> anyhow::Result<Vec<WorktreeFact>>;

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
- Host-specific command runners pass complete captured stdout through the
  public parser boundary. They do not duplicate field parsing, C-quote
  decoding, duplicate checks, unknown-field handling, or first-row/main rules.
- `parse_porcelain` uses native path comparison. A WSL or SSH capture must call
  `parse_porcelain_with_path_semantics(..., WorktreePathSemantics::Posix)` so
  POSIX case remains distinct even when the GUI process runs on Windows.
- Capture stdout and stderr independently with a 16 MiB retained-byte limit per
  stream. Continue draining after the limit so the child cannot block, then
  reject the scan instead of parsing a truncated authoritative inventory.
- The command timeout is an end-to-end deadline covering child execution and
  output-pipe drain. On Unix the command owns a dedicated process group; on
  Windows it is created suspended, assigned to a kill-on-close Job Object, and
  only then resumed. Timeout/error/success cleanup terminates descendants and
  bounds reader shutdown; inherited pipes cannot block the caller indefinitely.
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
| Either output stream exceeds 16 MiB | Drain/terminate safely and reject the scan; never parse truncated output |
| Leader exits but descendant holds a pipe | Deadline expires, terminate the complete process tree, and return a bounded error |
| Reader or process cleanup exceeds its bound | Return cleanup context; never wait indefinitely |
| Non-authoritative empty result in app | Preserve last-known rows/badges |
| Filesystem path missing but Git still registers it | Keep persisted project; absence is not proven |
| WSL/SSH output differs only by POSIX path case | Keep both rows; do not apply Windows case folding |
| Current authoritative inventory omits registered child path | Navigation may omit the row, but never deletes persistence; only a dedicated, explicitly destructive cleanup path may remove it |

### 5. Good / Base / Bad Cases

- Good: a prunable row keeps `refs/heads/feature`, so the create UI still treats
  that branch as occupied while the row remains registered.
- Base: a detached or bare row has no local branch but remains representable in
  rich facts and the compatibility projection.
- Bad: a transient Git error returns an empty fallback and the project list
  deletes worktree children. This is forbidden because fallback absence is not
  authoritative.
- Bad: call `read_to_end`, kill only the direct child, and then unconditionally
  join readers. A descendant that inherited stdout/stderr can retain the caller
  forever and output can consume unbounded memory.

### 6. Tests Required

- Public parser tests assert NUL/text parity, explicit Native/POSIX comparison,
  malformed UTF-8/C quoting, conflicting fields, unknown fields, and duplicate
  paths. Parser fixtures also cover spaces, newlines, detached, bare, sparse,
  locked/prunable with and without reasons, unknown fields, invalid UTF-8,
  malformed quoting, conflicting duplicates, and missing worktree path.
- Catalog tests: unsupported `-z`, ordinary failure without retry, last-known
  fallback, common-dir singleflight, mutation generation fencing, per-stream
  output overflow, timeout process-tree cleanup, successful leader exit with a
  descendant holding pipes, and a real-Git linked/detached/locked/prunable smoke
  test.
- App tests: stale request rejection, degraded-result preservation,
  authoritative-only cleanup, branch projection, and POSIX case/backslash
  distinctions.
- Verification runs only in GitHub Actions. The focused catalog job covers
  `mt-project`, linked `mt-app` checks/tests, changed-line rustfmt, and Clippy;
  Windows MSVC checks cover both process-tree implementations. The local
  workstation must not invoke Rust, test, staging, packaging, or Docker CI jobs.

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
