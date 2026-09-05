# Technical Design

## Design Goals

- One host-aware worktree catalog feeds both the Orca sidebar and global jump
  palette.
- Every navigation result carries stable ownership and revalidates it before
  changing the workbench.
- Local, WSL, and SSH discovery use the same Git fact/parser contract while
  keeping execution and path conversion at the correct host boundary.
- The palette feels like the supplied Orca reference without importing Orca's
  unrelated features or creating a second command/search architecture.
- All changes remain additive to existing project/layout persistence and can be
  reverted without data migration.

## Architecture

```text
AppStore project/workbench/Agent state
        | immutable projections / exact activation APIs
        +---------------------------+
        v                           v
 Workspace-owned WorktreeCatalog  Workspace-owned JumpRecency
        ^                           |
        | Local scan / WSL / SSH    | process-local MRU snapshots
        |                           v
 mt-project porcelain parser   GlobalJumpPalette
        |                           ^
        +----> OrcaProjectSidebar --+
                    |
                    +---- Workspace command routing
```

`Workspace` owns one `Entity<WorktreeCatalog>` and passes it to the sidebar and
every palette instance. Moving scan ownership out of `OrcaProjectSidebar`
prevents duplicate Git calls and makes catalog updates visible to both
surfaces in the same GPUI notification cycle.

## Shared Worktree Catalog

### Ownership

Add `crates/mt-app/src/worktree_catalog.rs` with a presentation-independent
entity. It owns:

```text
root project id -> CatalogSnapshot
root project id -> in-flight request owner
global request generation
foreground refresh timer
```

The catalog observes `AppStore` for project/config/focus changes. The sidebar
owns only collapse/hover presentation; the palette owns only query/filter/
selection presentation.

### Scan Target

For each added top-level project, use that project's exact configured folder as
the scan anchor and capture its `AppStore::project_execution_snapshot`. The
catalog asks Git for the worktree inventory of the repository owning that
folder. It never walks the directory tree or enumerates other repositories on
the host. The target contains:

```text
root_project_id
source_signature: ExecutionSourceSignature
configured host-visible root path
execution-host canonical root path
backend: Local | Wsl | Ssh
host label
local catalog generation when applicable
request generation
```

The anchor may itself be the main worktree or any linked worktree; Git must
return the same related repository inventory in either case. A definite
`not a git repository` result projects only the configured folder and never
falls back to a host-wide, parent-directory, or recursive search.

Child compatibility projects never start their own scan. Their top-level
project relation is resolved before target construction, so one repository
cannot be scanned once per configured child.

### Command and Parser Boundaries

Native Local passes the configured project folder directly to the existing
`mt_project::worktree::scan(Path)` API. This retains its canonical common-dir
cache key, single-flight behavior, mutation generation, process-tree cleanup, and last-known fallback.

WSL and SSH use `execution_host::execute_host_command` with structured:

```text
git worktree list --porcelain -z
```

The command uses the added project's execution-host folder as cwd; it does not
run a filesystem discovery command. It retains the catalog contract's 16 MiB
per-stream cap and bounded timeout. If and only if `-z` exits with the
verified unsupported-option status, retry:

```text
git worktree list --porcelain
```

Promote the existing strict byte parsers in `mt-project::worktree` through one
small public capture API, for example:

```rust
pub enum WorktreePorcelainMode { Nul, Text }
pub fn parse_porcelain(
    mode: WorktreePorcelainMode,
    bytes: &[u8],
) -> anyhow::Result<Vec<WorktreeFact>>;
```

The parser remains owned by `mt-project`; `mt-app` must not copy field parsing,
C-quote handling, duplicate checks, or main-row rules.

For WSL/SSH facts, `path_state` remains `Unknown` unless Git itself marks the
row prunable. The UI does not probe every path separately merely to paint a
row.

### Snapshot Contract

The app-owned snapshot is:

```text
CatalogSnapshot
  owner: exact captured source signature
  observed_ssh_epoch: optional
  revision: monotonic request generation
  scan: WorktreeScan
  refreshed_at: process-local timestamp
  warning: bounded diagnostic
```

A WSL/SSH scan is authoritative only when:

1. the command completed before the deadline;
2. neither stream was truncated;
3. the exit status is success;
4. the complete output parsed successfully;
5. the current root project still resolves to the same host, path, backend
   fingerprint, and root identity;
6. for SSH, the observed command epoch is the exact current epoch.

Because discovery is read-only, a command that acquired a fresh authenticated
SSH session may be accepted only after replacing the captured epoch with the
observed epoch on both the captured and current signatures and proving every
other field unchanged. Mutation paths do not inherit this reconciliation rule.

Before a refresh, the previous snapshot becomes last-known/non-authoritative.
A failed or stale completion leaves it in place. No consumer interprets absent
or degraded rows as destructive proof.

### Refresh Policy

- Immediate refresh at catalog construction.
- Refresh when the root project set, root source signature, configured path, or
  local catalog generation changes.
- Forced refresh when the window regains focus and after existing worktree
  mutation callbacks.
- While the window is focused, poll WSL/SSH roots every 10 seconds so worktrees
  created outside mini-term appear without reconnecting or changing projects.
- Keep one in-flight request per root and cap global blocking scans at four.
  A requested refresh while busy marks the root dirty and runs once more after
  the current request finishes.
- Stop polling while the window is unfocused; retain last-known rows.

### Row Projection

Expose immutable project/row models from the catalog instead of letting the
sidebar and palette repeat merge rules:

```text
ProjectWorktreeGroup
  root project id/name
  host label/backend/connectivity
  rows: WorktreeCatalogRow[]

WorktreeCatalogRow
  stable row key (execution host + normalized canonical path)
  configured project id, if registered
  host-visible project path
  execution-host path
  branch/head and Git flags
  main/selectable/authoritative/last-known state
  source owner used for click revalidation
```

Match configured rows with host-qualified location:

- Native Local: canonical local path using the existing Windows/POSIX rules.
- WSL: distro plus case-sensitive POSIX execution path; convert to
  `\\wsl.localhost\<distro>\...` only for project registration/storage.
- SSH: saved connection ID plus normalized case-sensitive absolute POSIX path.

Configured fallback rows are present before the first scan and while a Git
snapshot is degraded. Git facts lead the order (main first, then Git order),
with configured fallbacks appended only when the snapshot is absent or
non-authoritative. An authoritative Git snapshot renders its Git facts plus the
configured root fallback only if the root cannot be matched; it does not append
configured children omitted by Git. A definite non-Git result renders only the
configured root. Rendering never deletes a project registration.

### Registering a Discovered Worktree

Extend the centralized registration transaction with an explicit placement
instead of calling a raw insertion helper:

```rust
pub enum ProjectPlacement<'a> {
    TopLevel { target_group: Option<&'a str> },
    ChildWorktree { root_project_id: &'a str },
}
```

Existing onboarding uses `TopLevel`; discovery uses `ChildWorktree`. The child
path verifies:

- the root still exists and is top-level;
- Local/WSL versus SSH ownership matches the root;
- an SSH child uses the same connection ID;
- a WSL child uses the same distribution;
- the catalog owner/source signature is still current;
- canonical host/path dedupe is checked before insertion.

New children set `parent_project_id`, do not enter `projectTree`, and reuse the
existing identity preparation, layout creation, config persistence, activation,
and remote runtime reconciliation. A duplicate activates the existing record
without silently reparenting a user-managed top-level project.

## Global Jump Palette

### Opening and Workspace Commands

Replace the project-only implementation with `jump_palette.rs`. Keep the
`SwitchProject` action and `switchProject` hotkey ID as compatibility names,
but change the visible label to global Quick Open. Keep the existing
`PROJECT_SWITCHER` overlay key so user input and guarded modal behavior do not
gain a parallel overlay path.

The sidebar Search row emits `OrcaSidebarEvent::OpenJumpPalette`; `Workspace`
centralizes opening for both the event and shortcut. The palette emits typed
workspace commands for Settings, Usage, Add Project, New Terminal, and file
search. `Workspace` executes the existing entrypoints after the palette closes.

### Result Model

Use an exhaustive enum; no row carries an arbitrary callback:

```text
JumpItem
  Agent(AgentRunId + display snapshot)
  Terminal(TerminalJumpTarget)
  Worktree(WorktreeCatalogTarget)
  Setting(SettingsTarget)
  Action(JumpAction)
```

`TerminalJumpTarget` contains project ID, `WorktreeId`, `TabId`, `PaneKey`,
`TerminalSessionId`, and expected optional incarnation. Result IDs include the
full host/worktree/type identity so equal pane strings on different hosts
cannot collide.

Build Agent rows from `agent_target_views()`. When a pane has a current Agent
row, suppress the duplicate plain terminal row. Historical transcript/session
files are not searched globally.

### Search, Ranking, and Recency

Normalize query text once with Unicode lowercase and whitespace tokenization.
Reject input beyond 2 KiB before ranking. Each item exposes bounded searchable
fields; display markup uses character indices, never byte slicing.

Ranking order:

1. exact title/command intent;
2. title prefix;
3. token coverage across title, project, worktree, branch/path, host, and
   provider;
4. stable source order as the final tie-breaker.

The empty-query model is frozen when the palette opens so live status updates
do not reorder a row under the cursor. Attention rows lead. Add a small
workspace-owned `JumpRecency` entity that observes the store and records
worktree and terminal activations only when the active stable target changes;
it is capped and not persisted. Missing MRU falls back to active target,
sidebar order, and panel/pane layout order. Relative age text is shown only
when a real Agent receipt or MRU timestamp exists.

Filtering is palette-local and reconciled against current catalog options. The
Filter button or `Tab` expands an inline panel under the input with multi-select
result-family, host, and project options. Keeping it inside the Dialog avoids a
second nested overlay/focus stack.

### Activation Boundaries

- Agent: call `AppStore::activate_agent_run` with `AgentRunId`.
- Terminal: add a public store boundary that revalidates project-to-worktree,
  tab, pane, logical session, and expected incarnation before using the normal
  exact pane activation path and revealing the terminal page.
- Configured worktree: revalidate the catalog target, activate the configured
  project, capture its current `WorktreeId`, then call
  `reactivate_active_page` with both identities.
- Unconfigured worktree: revalidate, run child registration, and activate the
  exact returned project/worktree pair.
- Settings/action: close first, suppress focus restoration, then emit the typed
  command to `Workspace`.

Missing or mismatched targets return a typed failure. Keep the palette open so
the user can choose another current result; a toast explains the stale target
without mutating the current workbench. Close only after intentional successful
navigation or a workspace command handoff.

### UI and Keyboard

- Width: up to 900 px and at most 96% of viewport; top offset about 10%.
- Height: content-driven with a bounded list, no nested cards, and stable row
  dimensions.
- Input row: search icon, placeholder, Filter button.
- Sections: recent/open targets, worktrees, settings/actions as applicable.
- Row: fixed icon/status column, primary title, optional real age, and trailing
  project/branch/host badges; first nine selectable rows show `Ctrl + N`.
- Footer: Enter/Open, Esc/Close, arrows/Move, Tab/Filter.
- `Up`/`Down` bindings use the existing same-depth GPUI input-action pattern.
- Query/filter changes reset selection to the first selectable row and scroll
  to the top. Section headers and overflow hints are not selectable.
- Opening captures the prior focus target. Cancel restores it if still valid;
  successful navigation suppresses restoration.

## Compatibility and Migration

- No config or layout schema migration.
- Existing child project records remain valid and are merged into the catalog.
- Existing `switchProject` user keybindings remain valid.
- Existing file search, Agents overlay, workbench persistence, terminal host,
  GitHub Tasks, and context panels retain their public behavior.
- Remove sidebar-owned scanning only after the shared catalog has equivalent
  local regression coverage.

## Failure and Rollback

- If a scan fails, show last-known/configured rows and bounded host-specific
  warning state; do not clear the project group.
- If a palette target becomes stale, leave the workbench unchanged and show a
  concise toast.
- If the shared catalog causes a presentation regression, the rollback point is
  the commit that moves scan ownership out of `orca_sidebar.rs`; persisted
  projects/layouts require no rollback.
- If palette integration regresses navigation, restore the old project switcher
  call while keeping catalog work independently usable. No database downgrade
  is needed.

## Validation Design

Pure tests cover parser exposure, query bounds, Unicode matching, ranking,
filter reconciliation, row dedupe/order, WSL conversion, source-signature
comparison, recency freezing, and direct-key selection.

Store tests cover exact terminal activation, every identity mismatch,
top-level versus child registration, local/WSL/SSH dedupe, parent/host
validation, and repeated activation.

Catalog tests use injected command results for NUL/text success, exit-129
fallback, ordinary failure, timeout, truncation, malformed output, disconnect,
epoch replacement, A-to-B-to-A source changes, last-known preservation,
single-flight, queued refresh, and concurrency caps.

UI interaction tests cover opening from both entrypoints, sections, keyboard,
filter panel, focus restoration, stale-result messaging, and shared catalog
updates. Final executable evidence comes only from GitHub Actions `CI` and
`Windows Package` on the same product commit.
