# Navigation Catalog Contract

## Scenario: Shared Quick Open and host-aware worktree discovery

### 1. Scope / Trigger

Use this contract when adding a navigation surface that lists projects,
worktrees, terminals, Agent runs, settings, or workspace actions, or when Git
worktree discovery runs outside `mt-project`'s native local command runner.
This also covers per-project sidebar visibility and the distinction between
catalog refresh progress, registration authority, and genuine degradation.

The shared catalog is a read-only navigation projection. It must not become a
filesystem crawler, a second persistence owner, or a shortcut around stable
workbench identities.

### 2. Signatures

```rust
pub struct WorktreeCatalogOwner {
    pub source: ExecutionSourceSignature,
    pub revision: u64,
}

pub struct WorktreeCatalogTarget {
    pub root_project_id: String,
    pub row_key: String,
    pub root_config_key: String,
    pub configured_project_id: Option<String>,
    pub host_visible_path: String,
    pub execution_path: String,
    pub suggested_name: String,
    pub backend: CatalogBackend,
    pub owner: Option<WorktreeCatalogOwner>,
}

impl WorktreeCatalog {
    pub fn new(store: Entity<AppStore>, cx: &mut Context<Self>) -> Self;
    pub fn force_refresh(&mut self, cx: &mut Context<Self>);
    pub fn groups(&self, cx: &App) -> Vec<ProjectWorktreeGroup>;
    pub fn resolve_target(
        &self,
        target: &WorktreeCatalogTarget,
        cx: &App,
    ) -> Option<WorktreeCatalogRow>;
}

pub fn activate_target(
    catalog: &Entity<WorktreeCatalog>,
    store: &Entity<AppStore>,
    target: &WorktreeCatalogTarget,
    window: &mut Window,
    cx: &mut App,
) -> Result<ProjectRegistrationOutcome, String>;

// mt-config persists typed exclusions in the owning ProjectConfig JSON record.
pub struct HiddenWorktree {
    pub source: WorktreeVisibilitySource,
    pub location: WorktreeVisibilityLocation,
}

// mt-app captures a settings destination before opening the guarded form.
pub struct ProjectSettingsTarget {
    pub root_project_id: String,
    pub root_config_key: String,
    pub source: Option<WorktreeVisibilitySource>,
}

pub fn sidebar_visible(row: &WorktreeCatalogRow, hidden: &[HiddenWorktree]) -> bool;

pub enum JumpCommand {
    Settings(Option<SettingsPage>),
    Usage,
    AddProject,
    NewTerminal,
    FileSearch,
}

pub fn open(
    store: Entity<AppStore>,
    catalog: Entity<WorktreeCatalog>,
    recency: Entity<JumpRecency>,
    on_command: impl Fn(JumpCommand, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
);
```

### 3. Contracts

- `Workspace` owns one `WorktreeCatalog` and one `JumpRecency`. The project
  sidebar and every Quick Open instance consume those entities; consumers do
  not start their own Git or filesystem scans.
- Build one scan target per configured top-level project. Its exact configured
  folder is the Git cwd, whether that folder is the main or a linked worktree.
  Child worktree projects never start independent scans.
- Native Local delegates to `mt_project::worktree::scan`. WSL and SSH execute
  `git worktree list --porcelain -z` on the owning host and retry text
  porcelain only for verified exit code 129 unsupported-option output.
- Remote output is bounded to 16 MiB per stream and a 30-second deadline. A
  timeout, truncation, missing status, malformed capture, ordinary non-zero
  exit, or stale completion cannot replace current authority.
- A completion must still match root configuration, full execution source
  signature, target generation, request revision, local catalog generation,
  and, for SSH, the observed current connection epoch.
- Scans are single-flight per root, globally capped at four, and coalesce one
  dirty rerun. Focus regain forces refresh; focused WSL/SSH roots poll every 10
  seconds. Unfocused windows retain their current projection.
- Starting an ordinary refresh does not turn a successful snapshot into
  `LastKnown` or create a warning. Keep refresh progress in a stable header
  lane, separate from Agent activity and warning color. Real degraded warnings
  remain visible during retries. In-flight rows still lose effective
  registration authority; unconfigured rows cannot activate until fresh facts
  restore authority. Do not weaken owner/generation/epoch validation to avoid
  presentation churn.
- Row keys combine execution-host identity with the normalized canonical path.
  Native local comparison follows platform rules; WSL/SSH execution paths use
  case-sensitive POSIX normalization. WSL conversion to a host-visible UNC path
  happens only at projection/registration boundaries.
- With no snapshot or a degraded Git snapshot, render the configured root and
  configured children as fallback rows. With an authoritative Git inventory,
  render Git facts and match configured aliases by canonical host-qualified
  location; do not append configured children omitted by Git. A definite
  non-Git result renders only the configured root. None of these projection
  choices deletes persisted projects.
- `groups()` remains the complete shared catalog. Only the sidebar applies
  `sidebar_visible`: prunable or positively missing rows are excluded; all
  other rows show unless their stable identity is manually hidden. New valid
  discoveries show automatically. No branch count cap, Orca ownership flag,
  scratch-directory policy, or upstream-URL merge belongs in this predicate.
- `ProjectConfig.hidden_worktrees` is a backward-compatible, default-empty
  exclusion list in the existing ConfigDb project JSON. Identity includes the
  execution host, normalized configured root path, backend (Local, WSL distro,
  or SSH connection and endpoint), and a typed location: canonical worktree
  path, or existing configured project ID plus normalized configured path.
  The configured variant never proves canonical identity. Do not persist
  navigation row keys, branch names, session epochs, secrets, or row indexes.
- Use a trusted binding for offline configured rows. No connection or refresh
  failure is positive path-invalidity evidence. Never invent a durable
  preference from an unresolved temporary/fallback navigation key.
- A WSL configured alias without canonical authority can use the explicit
  configured-project preference, fenced by its current source/configuration.
  Do not infer alias equality or add host commands/runtime binding mutations
  for a sidebar exclusion. A resolved row checks both its canonical and exact
  configured exclusions. Its checkbox uses the same effective visibility;
  Unhide clears both applicable choices through the draft merge. Existing
  canonical-entry JSON remains readable.
- Each root header retains its settings menu even when every child is hidden.
  The guarded settings form consumes the unfiltered catalog, shows row
  identities and validity, and keeps saved exclusions recoverable. Invalid
  rows are disabled and cannot override the invalidity filter.
- Settings edits are a draft until Save. Revalidate the captured root and
  source, merge only changed identities into the latest exclusion list, and
  persist through AppStore's existing config writer. Preserve unrelated fields
  and exclusions for unseen/removed/offline rows. Cancel changes nothing.
  Hiding the active worktree never switches, closes, deletes, or rebinds it.
- Quick Open builds Agent, terminal, worktree, setting, and allowlisted action
  candidates from in-memory projections while typing. It never scans files,
  Git, SSH, or session history in response to a query.
- Query input is Unicode-safe and limited to 2 KiB. Empty-query ordering is
  frozen per open and uses real attention/activation recency followed by stable
  source order. Filters are process-local, reconcile removed options, and reset
  when the palette closes.
- `switchProject` and `Ctrl+Shift+P` remain compatibility identifiers for Quick
  Open. `Ctrl+Shift+F` remains current-worktree file search.
- Repeated open requests reuse the guarded overlay. Cancel restores the captured
  focus when it still exists. Successful navigation or a workspace command
  suppresses restoration and transfers focus through the destination boundary.
- Agent, terminal, and worktree rows carry complete targets. Selection calls
  the exact activation API and fails closed with a visible message when the
  target no longer resolves.

### 4. Validation & Error Matrix

| Condition | Required behavior |
|---|---|
| Added folder is main or linked worktree | Show the same repository inventory |
| Added folder is not a Git repository | Show only its configured root row |
| Unrelated nested, sibling, parent, or host repository exists | Never include it |
| Authoritative Git omits a configured child | Omit it from navigation; do not delete persistence |
| Snapshot is absent or genuinely degraded | Preserve configured fallback and last-known rows |
| Healthy snapshot is refreshing | Preserve successful presentation; fence unconfigured activation |
| SSH fingerprint/path/epoch or request owner changed | Reject completion and queue current refresh |
| Prunable or positively missing fact | Keep in raw/settings inventory; exclude from sidebar and do not register |
| Bare or otherwise unsafe unconfigured fact | Preserve raw state and registration rejection; do not infer invalidity |
| New valid worktree appears | Show unless an exact stable exclusion already exists |
| Worktree is manually hidden | Filter only sidebar; keep catalog, Quick Open, and live workbench intact |
| Settings source changed before Save | Reject visibly; never apply the draft to the replacement source |
| Settings draft is canceled | Leave persisted configuration unchanged |
| Every row is hidden | Keep root header and settings/unhide entrypoint accessible |
| Configured WSL alias lacks canonical authority | Allow an exact configured-project exclusion without alias inference |
| Selected discovered row is still current | Register/activate through `ProjectPlacement::ChildWorktree` |
| Selected row became stale | Leave workbench unchanged and report failure |
| Query exceeds 2 KiB | Show the bounded validation message; do not rank candidates |
| Palette is already open | Keep one overlay and one focused input |

### 5. Good / Base / Bad Cases

- Good: a project added from `/repo-feature` shows `/repo-main` and every linked
  worktree reported by that repository, on the same Local/WSL/SSH host only.
- Base: a non-Git folder remains navigable as one configured row.
- Good: a disconnected SSH refresh keeps last-known rows but marks them
  non-authoritative and prevents unconfigured registration from stale facts.
- Good: one worktree is hidden and a later valid discovery appears; saved
  exclusions are not a default-hide allowlist.
- Bad: overwrite the root's whole settings snapshot on Save, losing unrelated
  edits made while the form was open.
- Bad: recursively scan the added directory or search parent folders for `.git`.
- Bad: derive a terminal or worktree target after first switching the active
  project.

### 6. Tests Required

- Parser/capture tests cover NUL/text parity, unsupported `-z`, malformed and
  truncated output, POSIX case, duplicate paths, and non-Git classification.
- Catalog tests cover top-level target construction, main/linked anchors,
  Local/WSL/SSH routing, source/generation/epoch fences, single-flight, dirty
  rerun, global cap, last-known preservation, and projection authority rules.
- Registration tests cover Local/WSL/SSH children, canonical aliases, repeated
  selection, top-level alias preservation, parent/host/connection/distro
  mismatch, and exact returned project/worktree identity.
- Palette tests cover Unicode and 2 KiB bounds, ranking, deterministic recency,
  dedupe, filters, result caps, keyboard precedence, direct number selection,
  stale targets, one-overlay behavior, and focus restoration.
- Sidebar/palette tests assert both read the same catalog revision and that file
  search keeps its separate shortcut and worktree scope.
- Visibility/settings tests cover default-empty config round trips, host and
  source isolation, invalid/recovered/offline rows, new default-show rows,
  draft cancel/revert/merge, retained unseen exclusions, and stale-source
  rejection. Cover WSL configured aliases, later canonical resolution, dual
  exclusion Unhide, and child reconfiguration during a draft. Assert hiding
  changes no live workbench or terminal route.
- Refresh tests assert successful presentation remains successful while
  effective activation is fenced, and genuine degraded warnings survive retry.
- Formatting, compilation, Clippy, tests, generated i18n, Windows MSVC, and
  installer verification run only in GitHub Actions.

### 7. Wrong vs Correct

#### Wrong

```rust
let path = selected_row.path.clone();
store.set_active_project(&selected_row.project_id, cx);
open_whichever_worktree_now_matches(&path, window, cx);
```

#### Correct

```rust
let target = selected_row.target.clone();
activate_target(&catalog, &store, &target, window, cx)?;
```

The target is captured before yielding, re-resolved against the shared catalog,
and activated once through the stable project/worktree boundary.
