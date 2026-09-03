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
  connectivity only after matching an authoritative `AgentTargetView` by
  normalized provider plus exact provider session ID.
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
  match history badges only on exact normalized provider plus session ID.
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
let target = store
    .agent_target_views_for_worktree(&worktree_id)
    .into_iter()
    .find(|target| {
        target.provider == normalized_provider
            && target.provider_session_id.as_deref() == Some(history.session_id.as_str())
    });

if let Some(target) = target {
    show_activity(target.activity);
    show_connectivity(target.connectivity);
}
```

The history row consumes an immutable authoritative projection, and activation
passes only `target.run_id` back to the store for complete route revalidation.
