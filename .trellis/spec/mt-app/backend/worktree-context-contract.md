# Worktree Context Contract

## Scenario: Orca-style contextual panels and exact Agent routing

### 1. Scope / Trigger

Use this contract when Files, Git, Sessions, inline Agent rows, or terminal
runtime diagnostics read or mutate UI state for the active worktree. It applies
to local and SSH projects, same-path worktrees, delayed filesystem/Git/session
results, terminal recovery projections, and every action that focuses an Agent.

### 2. Signatures

```rust
pub fn orca_worktree_context_enabled() -> bool;

pub fn canonical_worktree_path_for_project(
    &self,
    project_id: &str,
) -> Option<&str>;

pub fn agent_target_views(&self) -> Vec<AgentTargetView>;
pub fn agent_target_views_for_worktree(
    &self,
    worktree_id: &WorktreeId,
) -> Vec<AgentTargetView>;

pub fn activate_agent_run(
    store: &Entity<AppStore>,
    run_id: &AgentRunId,
    window: &mut Window,
    cx: &mut App,
) -> bool;

pub fn terminal_diagnostics_for_worktree(
    &self,
    worktree_id: &WorktreeId,
    cx: &App,
) -> Vec<TerminalDiagnosticView>;

pub struct FsChange {
    pub project_path: String,
    pub source_key: Option<String>,
    pub path: PathBuf,
    pub kind: String,
}

pub fn watch_scoped(
    &self,
    path: &Path,
    project_path: &str,
    source_key: impl Into<String>,
) -> Result<()>;
```

Panel-owned presentation state is keyed by `WorktreeId`:

```text
Files    -> entries, Git labels, selection, root error, scroll
Git      -> repositories, branch view, section layout, drafts, history, scroll
Sessions -> host/WSL/SSH rows, lineage, pagination, preview, view mode, scroll
```

Every delayed completion additionally captures the panel generation and its
source-specific facts such as repository path, connection fingerprint, branch,
or provider session ID.

### 3. Contracts

- `ContextPanel` owns the selected `Files / Git / Tasks / Sessions` tab at the
  application level. A worktree switch changes panel contents, not the selected
  tab type.
- Presentation caches are keyed by stable `WorktreeId`, never by project name or
  path. Same-path worktrees remain independent.
- Files and Sessions read the persisted canonical worktree path. The configured
  project path is only a compatibility fallback when no canonical path exists.
- A scope switch saves stable last-known presentation, increments request and
  generation fences, cancels or invalidates active work, restores the target
  bucket, then refreshes it. File watchers are removed before the new scope is
  activated. Each queued filesystem event retains the registration-time opaque
  source key and project root; the consumer rejects events from an old source.
- A delayed result mutates UI only when `WorktreeId`, panel generation, request
  token, and source-specific facts still match. A path match alone is never
  sufficient.
- Git mutation state is not blindly cached. An in-flight pull, push, commit, or
  file mutation becomes `refresh_needed` when its scope is suspended; a stale
  `Loading` value must not be restored after its completion was fenced out.
  Completed pull/push result badges may be restored, but must receive a new
  bounded clear timer. When a mutation finishes after an A-to-B-to-A switch,
  exact generation ownership still rejects the old callback, then stable
  WorktreeId plus repository identity triggers an active reload or marks the
  inactive cache for refresh.
- A loading Sessions preview whose task is cancelled by a scope switch or list
  refresh is restored as presentation plus an explicit restart requirement.
  The replacement request is launched only after the shared task set has been
  cleared, and it carries worktree, generation, and source-signature fences.
- Session files are historical evidence only. A row may show activity and
  connectivity only after matching a unique authoritative `AgentTargetView` by
  captured `SessionHistoryOwner`, execution host/worktree/backend/epoch,
  normalized provider and exact provider session ID. `NoTarget`, `ExactTarget`,
  `Ambiguous` and `SourceMismatch` are distinct results. The latter two are
  inert: never select the first candidate or fall through to terminal resume.
- When one run route matches several configured aliases of the same
  `WorktreeId`, target projection selects the active exact alias when present;
  otherwise it selects the lexicographically smallest project ID. The Orca
  sidebar consumes the global projection, groups by that selected project, and
  renders each `AgentRunId` at most once.
- Agent activation starts from `AgentRunId`, re-resolves the current route, and
  verifies execution host, worktree, tab, pane, terminal session, incarnation,
  current PTY route, and terminal entity before focus. Exact live navigation
  switches project/panel without hydration, reveals the terminal workbench, then
  acknowledges the selected event. A stale target is inert and never creates or
  resumes a terminal. Feed grouping and watermark details are normative in
  `global-agent-activity-contract.md`.
- Terminal recovery, Agent activity, and Agent connectivity are independent
  axes. `RestoredHistory` is not `Reattached`; `Disconnected` does not imply
  `Done`; an exited terminal is displayed separately from both.
- Diagnostic text is bounded and must not expose environment values, argv,
  credentials, hook secrets, or tokens.
- Only the exact environment value `MINI_TERM_ORCA_WORKTREE_CONTEXT=0` disables
  inline Agent rows, worktree-scoped context caches, exact history badges, and
  runtime diagnostics. Missing or any other value enables the new path.

### 4. Validation & Error Matrix

| Condition | Required behavior |
|-----------|-------------------|
| Same path, different `WorktreeId` | Restore separate selection, drafts, pagination, previews, and scroll |
| Old directory/Git/session result returns after a switch | Reject it without clearing or replacing current state; successful stale Git mutations schedule reconciliation |
| Queued watcher event arrives after source switch | Reject unless registration source key and project root both match |
| Loading preview is restored after its task was cancelled | Restart after list-task cancellation, preserving selected preview identity |
| SSH connection fingerprint changes | Reject old work and refresh using the new source identity |
| Pull/push is running when the user switches | Cache `refresh_needed`, not permanent `Loading`; refresh on return |
| Historical session has no authoritative run | Show history only; do not claim live, stale, or offline state |
| Provider matches but provider session ID differs | Do not attach an Agent badge or route |
| Several exact live routes share a history session ID | Mark resolution ambiguous; no first-match activation or resume |
| History owner or backend/epoch changed after scan | Reject badges, title attachment and deferred jumps |
| Shared worktree route matches multiple project aliases | Prefer the active exact alias; otherwise choose stable smallest project ID and render one run row |
| Agent route incarnation or PTY owner changed | `activate_agent_run` returns false and leaves focus unchanged |
| Cold transcript is restored and Agent is live | Show `Restored history` and Agent connectivity independently |
| Remote process probe is unsupported | Show unsupported capability; do not infer that the Agent is done |
| Rollback variable is `0` | Use legacy panel behavior and hide the new inline/diagnostic overlays |

### 5. Good / Base / Bad Cases

- Good: Worktree A retains a Git commit draft and Sessions preview while
  Worktree B retains different rows and scroll offsets at the same filesystem
  spelling.
- Good: A remote Agent row focuses the exact current terminal incarnation; a
  late row from a previous SSH epoch fails closed.
- Base: A fresh local terminal has no Agent run and appears only in Runtime with
  `Fresh` recovery.
- Bad: Find a session ID in a transcript and treat it as proof that a process is
  still running.
- Bad: Save `SyncState::Loading` into a worktree cache after invalidating the
  only callback that could clear it.
- Bad: Fence a Git or filesystem callback by repository path without also
  checking `WorktreeId` and generation.

### 6. Tests Required

- Rollback parsing accepts only exact `0` as disabled.
- Agent target ordering keeps activity priority separate from connectivity.
- Shared-alias tests reverse candidate order, prefer the active exact project,
  fall back to stable project-ID ordering, and render one row per `AgentRunId`.
- Exact route tests vary execution host, worktree, tab, pane, terminal session,
  and incarnation one at a time and reject every mismatch.
- FileTree tests switch between same-path worktree IDs and assert independent
  rows, Git labels, selection, root warning, and scroll.
- Git tests assert independent repository/branch/section state, commit drafts,
  history pagination, and scroll; owner tests reject old worktree, generation,
  repository, and branch facts.
- Git sync tests assert suspended `Loading` becomes a refresh requirement, an
  A-to-B-to-A completion reconciles by stable worktree/repository identity, and
  a newer loading operation is not cleared by an older timer.
- File watcher tests assert scoped events retain the registration owner and the
  FileTree rejects an old source key or project root.
- Sessions tests reject old scope generations and source signatures, restart a
  cancelled loading preview, preserve preview/list scroll, and
  match history badges only on a unique exact owned provider/session. Assert
  Local/WSL/SSH isolation, duplicate live routes and captured-owner changes do
  not fall through to resume. Family-panel callers capture the same owner
  before scanning rather than capturing a replacement at click time.
- Recovery label tests cover Fresh, Reattached, RestoredHistory, Compatibility,
  Unavailable, exited, Live, Stale, Offline, Linux process probing, detecting,
  and unsupported probing.
- Run Linux tests/check/Clippy and Windows MSVC checks only in GitHub Actions.

### 7. Wrong vs Correct

#### Wrong

```rust
let live = store.find_live_session_pane(&history.session_id);
if live.is_some() {
    show_live_badge();
}
```

This lets historical or compatibility pane metadata manufacture current Agent
state and does not validate the terminal incarnation.

#### Correct

```rust
match session_agent_target(&session, captured_owner, current_owner, &targets) {
    SessionAgentResolution::ExactTarget(target) => show_exact_agent(target),
    SessionAgentResolution::NoTarget => show_history_only(),
    SessionAgentResolution::Ambiguous | SessionAgentResolution::SourceMismatch => {
        show_inert_history();
    }
}
```

## Scenario: Files Listing And Operation Ownership

### 1. Scope / Trigger

Applies to Files rows, cached directories, context menus, creation, rename,
delete, clipboard, drag-only upload and download across project/worktree or
connection switches. Document editor save authority remains separate.

### 2. Signatures

```text
FileTreeSource = FileOperationContext + WorktreeId + connection_epoch
DirectoryListingOwner = exact FileTreeSource + listing request_id
FileTreeContextTarget = source + row-or-blank + captured listing proof
FileTreeOperationOwner = monotonic operation_id + exact target/source
FileTreeOperationPhase = Preflight -> Choice -> Running

remote_ssh::list_directory_at_epoch(conn, Option<epoch>, path, root, refresh)
    -> Result<RemoteFileListing, String>
RemoteFileListing = entries + connection_id/fingerprint/producing_epoch/root/directory
```

FileTree uses opt-in `create_entry_at_epoch`, `rename_entry_at_epoch`,
`delete_entry_at_epoch`, `copy_entry_keep_both_at_epoch`,
`upload_conflicts_at_epoch`, `upload_paths_at_epoch`, and
`download_entries_at_epoch`. All mutations require a captured `u64` epoch;
legacy service signatures retain their previous policy for other consumers.

### 3. Contracts

- Directory results carry the actual producing source, not the connection epoch
  observed later on the UI thread. Only read-only bootstrap may start with no
  epoch, then adopt the producing epoch. Caches retain listing provenance;
  stale rows are muted/inert until fresh owned reads complete.
- Root/worktree/backend/fingerprint/epoch/generation/request changes invalidate
  row actionability and pending listings. Refresh stale descendants parent-first.
  An old response cannot repopulate actionable rows after mutation reconciliation.
- Header contains only Refresh, using the shared tooltip owner. Every file or
  folder row retains its applicable full menu plus New File/New Folder. File
  creation targets its parent, folder creation that folder. Blank menus contain
  exactly those two creation commands against the captured root.
- Upload starts only from external-path drop. Internal file-to-terminal drag
  remains independent. Preserve conflict choices, errors and progress; no upload
  menu, toolbar command or file chooser is reintroduced.
- A shrinkable full-height flex shell owns one bounded scrolling list. Rows and
  status strips cannot shrink; the scrollbar overlay occupies only its gutter.
  Header/status areas do not become competing scroll owners.
- Clipboard and deferred prompts retain both target identity and listing proof.
  Validate both clipboard ends. POSIX remote paths use exact checked UTF-8;
  neither lossy Windows path conversion nor client filesystem fallback is valid.
- Preflight, conflict choice and dispatch retain one operation reservation with
  no release gap. Source switches do not clear it. Only that operation ID and
  matching phase may release it; stale cancellation cannot release Running.
- Remote acquisition and write/commit checks use the captured connection/epoch
  and exact pooled session. Recursion/cleanup retain the original SFTP handle.
  Pinned deletion never reacquires/replays on a replacement connection, even
  after a nominal pre-dispatch error.
- Busy ownership and presentation ownership are distinct. Completion may
  release only its reservation; expansion/collapse/alerts/reload additionally
  need the exact current target and generation. An ABA return permits only
  original stable-source cache invalidation or a fresh read, not old UI effects.
- Pin checks do not atomically interrupt an already dispatched SFTP request or
  shell deletion. Preserve partial counts and cleanup errors on the original
  session; do not claim rollback or replay. Deterministic mid-write interruption
  requires separate transport evidence, not a pure source-owner test.

### 4. Validation & Error Matrix

| Condition | Required behavior |
| --- | --- |
| Same cached path, new SSH epoch | Inert old rows; fresh producing provenance required |
| Connection changes during menu/prompt/conflict choice | Reject captured operation; no replacement dispatch |
| Source switches while operation runs | Retain busy owner, never adopt new source |
| Old preflight cancellation arrives after Running | Do not release the running reservation |
| Old completion returns after A-B-A | Reconcile original source without replaying expansion/alerts |
| Remote path is not exact UTF-8 | Fail before remote Files I/O |
| Transfer partially completed or cleanup failed | Preserve counts/error; no silent success/retry |
| Right-click blank area | Only New File/New Folder |

### 5. Good / Base / Bad

- Good: a cached remote directory remains visible but inert after reconnect;
  a new listing proves its source before a creation menu becomes actionable.
- Base: a local file row creates a sibling; a directory row creates a child.
- Bad: stamp old rows with the current epoch or release a running upload merely
  because another project became active.

### 6. Tests Required

- Exercise actual source/request/operation helpers for producing provenance,
  bootstrap adoption, stale descendants, ABA, busy transitions, cancellation,
  clipboard ownership and menu/layout builders.
- Windows covers drive/UNC behavior, exact remote path text, Files owner helpers
  and transfer/delete epoch predicates. Local creation fixtures retain exclusive
  creation and symlink-parent checks.
- Run the ignored authenticated loopback SFTP fixture explicitly in Actions:
  `remote_ssh::transfer::epoch_tests::actual_loopback_sftp_files_pins_mutations_and_containment`.
  It covers real operations/conflicts/containment and replacement refusal with
  unchanged destination assertions, not dispatched-transfer interruption.
- Native final-row wheel/thumb access, narrow/high-DPI sizing, tooltip timing,
  row/blank/drop routing and conflict dialogs need matching Actions artifacts.
  Authored helpers or source inspection do not establish native acceptance.

### 7. Wrong vs Correct

```text
Wrong: cached row -> read current epoch -> dispatch -> refresh active project
Correct: producing listing proof -> captured target -> one operation reservation
         -> exact-session dispatch -> original-source reconciliation
         -> exact-generation-only UI effects
```

The history row consumes an immutable authoritative projection, and activation
passes only `target.run_id` back to the store for complete route revalidation.
