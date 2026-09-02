# Worktree Layout Contract

## Scenario: Additive worktree layout persistence and migration

### 1. Scope / Trigger

Use this contract for every `layout.db` schema change, project/worktree binding
reconciliation, saved-layout migration, layout save, or compatibility rollback
write. Layout data is first-party user state and must never be rebuilt merely
because a schema or one JSON row is unfamiliar.

### 2. Signatures

Additive schema owned by `mt-layout`:

```sql
CREATE TABLE IF NOT EXISTS project_worktree_binding (
  project_id              TEXT PRIMARY KEY,
  execution_host_id       TEXT NOT NULL,
  repo_id                 TEXT NOT NULL,
  worktree_id             TEXT NOT NULL,
  identity_source         TEXT NOT NULL,
  canonical_worktree_path TEXT,
  updated_at_ms           INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_project_worktree_binding_worktree
  ON project_worktree_binding(worktree_id);

CREATE TABLE IF NOT EXISTS worktree_layout (
  worktree_id    TEXT PRIMARY KEY,
  layout_json   TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL
);
```

The `meta.local_host_install_id` key stores one canonical `HostInstallId`.

```rust
pub fn local_host_install_id(&self) -> Result<HostInstallId>;
pub fn load_project_bindings(&self) -> Result<HashMap<String, ProjectWorktreeBinding>>;
pub fn reconcile_worktree_layouts(
    &self,
    desired_bindings: &[ProjectWorktreeBinding],
    now_ms: i64,
) -> Result<ReconciledProjectLayouts>;
pub fn save_worktree_layout(
    &self,
    binding: &ProjectWorktreeBinding,
    layout: &SavedProjectLayout,
    now_ms: i64,
) -> Result<()>;
pub fn delete_project_binding(&self, project_id: &str) -> Result<()>;
pub fn retain_project_bindings(&self, live_project_ids: &HashSet<String>) -> Result<()>;
```

Saved JSON adds optional `worktreeId`, `activeTabId`, `tabId`,
`activePaneKey`, `paneKey`, `terminalSessionId`, and
`terminalIncarnationId` fields. Missing fields remain readable.

### 3. Contracts

- Schema changes are additive. A database carrying a higher schema version is
  not renamed as corrupt or destructively recreated.
- Reconciliation is one SQLite transaction. For each desired binding, source
  priority is existing destination worktree row, previous bound worktree row,
  then the compatibility `project_layout` row.
- An existing destination row always wins and is never overwritten by stale
  previous-binding or legacy data.
- Reconciliation never deletes source rows. Project removal deletes only the
  compatibility binding and its legacy mirror; orphan worktree rows remain
  recoverable.
- A successful normal save writes `worktree_layout` and that project's legacy
  `project_layout` mirror in the same transaction. Empty layouts delete both
  content rows while retaining the binding.
- When multiple desired project aliases resolve to one worktree, the first
  created/existing worktree destination is authoritative. A differing legacy
  row for another alias is retained and logged, not silently mirrored over.
- Valid JSON is salvaged per tab, split child, and pane. Invalid stable IDs are
  regenerated only for the affected object; surviving siblings remain.
- Syntactically invalid destination JSON is isolated to that worktree. It does
  not fall back to legacy data and does not block healthy rows.
- Normalization writes only when JSON content changed so a second startup does
  not churn stable identities or timestamps.

### 4. Validation & Error Matrix

| Condition | Required behavior |
|-----------|-------------------|
| Duplicate project ID in one reconcile request | Abort before opening the transaction |
| Existing destination and stale legacy row | Use destination; mirror only when it is not a shared-worktree conflict |
| Two aliases have different legacy layouts for one worktree | Use the destination for both, retain/log the conflicting legacy row |
| Destination absent, prior binding row exists | Copy and normalize the prior worktree row |
| Destination/prior row absent, legacy exists | Copy, normalize, dual-write, and bind |
| Valid JSON contains one malformed pane | Drop/regenerate only that pane and preserve valid siblings |
| Destination JSON is syntactically invalid | Keep it unchanged, return no layout for that project, continue other rows |
| Future schema version is present | Return compatibility error without moving or deleting the database |
| Empty layout is saved | Delete worktree content and the current alias mirror; keep binding |
| Project binding is removed | Delete binding and alias mirror; keep worktree content |

### 5. Good / Base / Bad Cases

- Good: Two project aliases for `/repo/shared` return the same worktree layout,
  while a conflicting second legacy row remains available for rollback review.
- Good: A moved project rebinds to a destination that already has state; the
  destination wins and the old worktree row remains untouched.
- Base: A legacy layout gains stable IDs on first load and performs no second
  write when reopened.
- Bad: Loop over project rows and copy each legacy layout into the same
  worktree destination; ordering would silently destroy one user's state.
- Bad: Delete all layout tables when schema version differs.
- Bad: Deserialize a whole worktree row with one `serde_json::from_str` failure
  and discard all valid sibling panes.

### 6. Tests Required

- Host install ID remains stable across two database opens.
- Legacy migration writes a normalized worktree row and mirror once, with no
  timestamp churn on second reconcile.
- Destination-wins rebind retains the previous source row.
- Shared-worktree collision returns one destination to both aliases and keeps
  the conflicting legacy row unchanged.
- Salvage tests assert bad pane removal, split collapse, equal-size repair, and
  stable active-pointer fallback.
- Invalid JSON isolation tests assert healthy bindings still reconcile.
- Future schema tests assert the database file and unknown payload survive.
- Delete/retain tests assert orphan worktree rows are preserved.

### 7. Wrong vs Correct

#### Wrong

```rust
for binding in desired_bindings {
    if let Some(legacy) = load_legacy(binding.project_id)? {
        upsert_worktree(binding.worktree_id, legacy)?;
    }
}
```

This lets a later compatibility alias overwrite an already selected worktree
destination.

#### Correct

```rust
let destination = load_worktree_layout(&binding.worktree_id)?;
let candidate = destination
    .map(|row| (Destination, row))
    .or_else(|| load_previous_binding_row(&binding).transpose())
    .or_else(|| load_legacy_row(&binding.project_id).transpose());

// Existing destination wins. Shared conflicting legacy rows are logged and
// retained rather than mirrored over.
reconcile_candidate_in_one_transaction(candidate, binding)?;
```

Source selection and all binding/layout writes belong to one transaction, and
no compatibility source row is deleted by reconciliation.
