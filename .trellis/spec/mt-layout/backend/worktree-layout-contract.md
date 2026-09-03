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
  identity_context        TEXT,
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
`ProjectWorktreeBinding::identity_context` is an optional opaque provenance
value. It may contain non-secret identity facts needed to decide whether a
persisted authoritative binding can be reused.

### 3. Contracts

- Schema changes are additive. A database carrying a higher schema version is
  not renamed as corrupt or destructively recreated.
- Reconciliation is one SQLite transaction. For each desired binding, source
  priority is existing destination worktree row, previous bound worktree row,
  then the compatibility `project_layout` row.
- Schema version 3 adds nullable `identity_context` with `ALTER TABLE`. Opening
  a version-2 database preserves every existing binding/layout payload and its
  timestamp; old rows read as `None`.
- An existing destination row always wins and is never overwritten by stale
  previous-binding or legacy data.
- Desired bindings are grouped by `WorktreeId`, and every group's candidate is
  selected before any destination or binding write occurs. This prevents one
  group's migration from changing another group's source candidates.
- When a source tier has multiple candidates, select the greatest
  `updated_at_ms`; break ties by ascending owner project ID and then source key.
  Input order must not affect the selected layout.
- Reconciliation never deletes source rows. Project removal deletes only the
  compatibility binding and its legacy mirror; orphan worktree rows remain
  recoverable.
- A successful normal save writes `worktree_layout` and that project's legacy
  `project_layout` mirror in the same transaction. Empty layouts delete both
  content rows while retaining the binding.
- When multiple desired project aliases resolve to one worktree, the selected
  destination is projected to every alias. A differing legacy row for an
  unselected alias is retained and logged, not silently mirrored over.
- In-memory save coalescing has one explicit latest dirty owner per
  `WorktreeId`. Only that alias may flush the shared row. Removing an older or
  ownerless alias never serializes it over newer state; removing the current
  owner flushes that owner without promoting an older snapshot. Removing the
  last alias still performs its final save.
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
| No destination and several candidates in one tier | Choose newest timestamp, then stable project/source key; ignore input order |
| Two worktree groups swap previous sources | Freeze both candidates before writing either destination |
| Destination absent, prior binding row exists | Copy and normalize the prior worktree row |
| Destination/prior row absent, legacy exists | Copy, normalize, dual-write, and bind |
| Version-2 binding table has no `identity_context` | Add the nullable column without rewriting payloads or timestamps |
| Valid JSON contains one malformed pane | Drop/regenerate only that pane and preserve valid siblings |
| Destination JSON is syntactically invalid | Keep it unchanged, return no layout for that project, continue other rows |
| Future schema version is present | Continue compatibility read/write, log it, and preserve unknown data without rebuilding the database |
| Empty layout is saved | Delete worktree content and the current alias mirror; keep binding |
| Shared alias is removed | Flush only when it owns the latest pending snapshot; keep the shared worktree row |
| Last project binding is removed | Final-save it, then delete binding and alias mirror; keep worktree content |

### 5. Good / Base / Bad Cases

- Good: Two project aliases for `/repo/shared` return the same worktree layout,
  while a conflicting second legacy row remains available for rollback review.
- Good: A moved project rebinds to a destination that already has state; the
  destination wins and the old worktree row remains untouched.
- Good: Reversing two alias records still picks the same newest legacy layout,
  and deleting the older alias cannot overwrite the latest pending snapshot.
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
- Version-2 migration adds `identity_context` while preserving binding and
  layout data plus their original timestamps.
- Destination-wins rebind retains the previous source row.
- Shared-worktree collision and reversed input order select the same newest
  candidate, return one destination to both aliases, and keep the conflicting
  legacy row unchanged.
- Cross-worktree source swaps prove all candidates are frozen before writes.
- Latest-owner save/remove tests prove older aliases cannot flush shared state,
  while the final alias still requests a save.
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
let groups = group_bindings_by_worktree(desired_bindings);
let frozen = groups
    .iter()
    .map(|group| select_candidate_by_tier_timestamp_and_stable_key(group))
    .collect::<Result<Vec<_>>>()?;

for (group, candidate) in groups.into_iter().zip(frozen) {
    reconcile_group_in_one_transaction(group, candidate)?;
}
```

All source selection is frozen before any write. Candidate ordering is explicit,
all binding/layout writes remain in one transaction, and no compatibility source
row is deleted by reconciliation.
