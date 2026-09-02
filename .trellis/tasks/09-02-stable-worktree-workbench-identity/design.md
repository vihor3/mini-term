# Technical Design

## Architecture Boundary

Introduce one small leaf crate, `mt-identity`, shared by `mt-config`, `mt-layout`, `mt-project`, and `mt-app`:

```text
mt-identity
  opaque ID types + UUID creation + domain-separated deterministic derivation
       |             |              |
       v             v              v
  mt-config       mt-layout      mt-project
  saved JSON      DB bindings    host-path canonicalization
       \             |              /
        \            v             /
         +---------- mt-app --------+
             compatibility projection and routing
```

`mt-core` stays on its existing lightweight dependency boundary for sidecars. `mt-layout` remains the persistence owner and does not become a dependency of future terminal/remote runtimes. `mt-project` remains the owner of Git/path facts and does not write layout state.

## Identity Types And Derivation

`mt-identity` exposes transparent serde newtypes with `Display`, `AsRef<str>`, `Borrow<str>`, `FromStr`, and stable hashing/equality:

```text
HostInstallId
ExecutionHostId
RepoId
WorktreeId
TabId
PaneKey
TerminalSessionId
TerminalIncarnationId
```

Random identities use UUID v4. Deterministic identities use SHA-256 over a versioned domain tag and length-prefixed components so concatenation cannot create ambiguous inputs. Serialized forms are prefixed, for example `host-v1:...`, `repo-v1:...`, `worktree-v1:...`, and `pane-v1:<uuid>`.

Derivation rules:

```text
ExecutionHostId = digest("execution-host/v1", host fingerprint, HostInstallId)
RepoId          = digest("repo/v1", ExecutionHostId, canonical common dir)
WorktreeId      = digest("worktree/v1", RepoId, canonical worktree path, workspace instance?)
```

The MVP leaves `workspace instance` empty. This allows deleting/recreating a compatibility project record for the same host/path to recover the existing workbench. Display name, branch, project ID, connection label, and current process ID never participate.

## Identity Resolution

Add `mt_project::worktree::identity` with a pure result model and local resolver:

```rust
pub struct ResolvedWorktreeIdentity {
    pub execution_host_id: ExecutionHostId,
    pub repo_id: RepoId,
    pub worktree_id: WorktreeId,
    pub canonical_worktree_path: String,
    pub canonical_git_common_dir: Option<String>,
    pub source: WorktreeIdentitySource,
}
```

Sources are explicit:

- `AuthoritativeLocalGit`: canonical local Git common dir + worktree path.
- `LocalDirectory`: canonical local directory for a non-Git project.
- `ProvisionalWsl`: local install identity + normalized distro + host-visible canonical path.
- `ProvisionalSsh`: local install identity + stable SSH connection ID + normalized remote POSIX path.
- `PersistedFallback`: an existing binding reused because its host/path cannot currently be resolved.

Local Git resolution reuses the catalog's common-dir rules. WSL/SSH never claim remote authority in this child. Phase 5 can replace a provisional binding with authenticated host/runtime facts through the same rebind transaction.

## Layout Schema

Bump `mt-layout` schema metadata additively and create:

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

`meta.local_host_install_id` stores the local install UUID. The old `project_layout` table is retained and becomes a rollback mirror.

### Reconciliation Transaction

At startup `mt-app` resolves each configured project, then asks `mt-layout` to reconcile all bindings and layouts in one transaction:

1. Reuse a valid existing binding when resolution is unavailable.
2. Upsert the newly resolved binding when facts are available.
3. Prefer an existing `worktree_layout` row.
4. Otherwise copy the project's legacy `project_layout` row into the worktree row.
5. If a binding changed and the new worktree row is empty, copy the old bound worktree row.
6. Never overwrite an existing destination worktree row with legacy/old-bound data.
7. Parse, salvage, normalize identities, and write the normalized worktree row plus the legacy mirror before commit.
8. Return layouts keyed by compatibility project ID together with the binding registry used by `AppStore`.

If the transaction fails, the application can continue through the current legacy loader in cold-only compatibility mode. No source row is deleted.

## Saved Layout Envelope

Extend the serde model additively:

```text
SavedProjectLayout
  worktreeId?       consistency fence
  activeTabId?      stable pointer; activeTabIndex remains rollback fallback
  tabs[]
    tabId?
    splitLayout
      leaf.activePaneKey?
      panes[]
        paneKey?
        terminalSessionId?
        terminalIncarnationId?
```

Missing IDs are generated during migration. Invalid pointers fall back to the current index/first-surviving-pane rule, then the corrected stable pointer is written back. Split-node render IDs remain process-local because they are not cross-process routing targets.

## Salvage Rules

Normal serde parsing remains the fast path. If it fails but the row is valid JSON, `mt-layout` performs bounded structural salvage:

- Parse tabs independently.
- Recursively parse split children.
- Parse panes independently and skip only malformed pane objects.
- Collapse a split with one surviving child and drop a node with no surviving children.
- Rebuild invalid/missing sizes with equal percentages.
- Clamp/fallback active pointers after survivors are known.
- Preserve other worktree rows even when one row is unrecoverable.

Syntactically invalid JSON cannot be safely salvaged and remains isolated to that worktree row. Logs report counts and identities, not terminal content.

## AppStore Compatibility Projection

Add `store/identity.rs` and these fields/accessors:

```text
active_worktree_id: Option<WorktreeId>
project_worktree_bindings: HashMap<ProjectIdString, ProjectWorktreeBinding>
```

`project_states` can remain keyed by project ID during this child to limit the blast radius. Every activation updates both active IDs, and all layout/document/future async ownership obtains the stable identity through the registry. Startup, local project creation, linked-worktree materialization, remote project creation, path rebinding, and removal all use the same helper.

Layout dirty tracking remains project-facing for current callers, but flush resolves the project's current binding and writes `worktree_layout + project_layout` atomically. Removing one project binding deletes no worktree layout while another live project still references it.

## Runtime Pane And Terminal Binding

`PaneState` gains `PaneKey`, `TerminalSessionId`, and optional expected `TerminalIncarnationId`. `ProjectPanel` gains `TabId`. Existing string `id` fields remain temporary compatibility projections equal to the stable ID string, avoiding a broad GPUI element-ID rewrite in the same migration. Constructors and restore paths are the only writers and tests enforce equality.

PTY creation order changes from "spawn then create pane identity" to:

1. Create or restore `TabId + PaneKey + TerminalSessionId`.
2. Mint a fresh `TerminalIncarnationId` for this actual spawn attempt.
3. Spawn the PTY and keep its `u32 pty_id` as the process-local attachment handle.
4. Store the new incarnation on the pane and schedule layout persistence.

Explicit reconnect follows the same path and rotates only the incarnation. Split/move/reorder copies the complete pane object and therefore preserves the stable pane/session binding. The current in-process PTY map stays keyed by `u32`; Phase 3 replaces ownership without changing these stable contracts.

Local child-process environment receives the stable host/worktree/tab/pane/session/incarnation values in addition to `MINITERM_PTY_ID`. Direct SSH remains provisional because those local environment variables are not remote attestation.

## Document Workbench Scope

Change `DocumentKey` and `WorkbenchArea` buckets from project ID to `WorktreeId`. `DocumentSource` retains project ID, connection snapshot, root, and path for actual I/O. Deferred close/focus/search callbacks capture both the source and originating `WorktreeId`, then revalidate the current project-to-worktree binding before touching focus or tab state.

Open documents remain runtime-only in this child. Their disk persistence and editor view state are separate workbench-state work, but no two worktrees share the current preview slot or active page after this change.

## Compatibility And Rollback

- Old JSON readers ignore the added fields and continue reading the mirrored `project_layout` row.
- New readers prefer `worktree_layout`, fall back to `project_layout`, and never destructively rebuild the database on schema mismatch.
- Project IDs remain supported at all current UI/command call sites while stable worktree accessors are introduced.
- Provisional WSL/SSH identities are visibly distinguishable in diagnostics and can be rebound later without rewriting pane/session IDs.
- Rolling back the code path leaves the latest terminal layout in `project_layout`; new identity tables remain untouched for a future upgrade.

## Risks

- A broad direct conversion of every UI string ID to a newtype would create unnecessary GPUI churn. Compatibility string projections keep this child focused while stable routing fields become authoritative.
- Two legacy project records can resolve to one worktree but contain different old layouts. The first existing worktree row wins; conflicting legacy rows are retained and logged, never silently merged.
- Canonical paths can change after a worktree move. A project-ID-preserving rebind copies state only when the destination has no state; destination data always wins.
- Salvage must be bounded and deterministic. It never guesses terminal content or executes commands.
