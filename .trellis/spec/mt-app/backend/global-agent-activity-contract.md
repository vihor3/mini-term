# Global Agent Activity Contract

## Scenario: Exact-run global Agent feed and acknowledgement

### 1. Scope / Trigger

Use this contract when the global Orca Agents entry projects live Agent activity,
renders the anchored feed, counts its badge, activates a row, or changes feed
acknowledgement. It applies to local, WSL, and SSH runs, duplicate provider
sessions, same-path worktrees, terminal teardown, overlay focus return, and the
global feed rollback switch.

### 2. Signatures

```rust
pub struct AgentTargetView {
    pub run_id: AgentRunId,
    pub last_event_id: AgentEventId,
    pub project_id: String,
    pub project_name: String,
    pub root_project_name: String,
    pub worktree_name: String,
    pub host_label: String,
    pub pane_id: String,
    pub pane_label: String,
    pub route: AgentRoute,
    pub provider: AgentProvider,
    pub provider_session_id: Option<String>,
    pub activity: AgentActivity,
    pub connectivity: AgentConnectivity,
    pub evidence: AgentEvidence,
    pub received_at_unix_ms: i64,
    pub attention: bool,
    pub unread: bool,
}

pub fn agent_target_views(&self) -> Vec<AgentTargetView>;

pub fn activate_agent_run(
    store: &Entity<AppStore>,
    run_id: &AgentRunId,
    window: &mut Window,
    cx: &mut App,
) -> bool;
```

Presentation projection:

```rust
pub(crate) fn build_agent_activity_feed(
    targets: Vec<AgentTargetView>,
    recent_limit: usize,
) -> AgentActivityFeed;
```

Rollback environment:

```text
MINI_TERM_GLOBAL_AGENT_ACTIVITY=0
```

### 3. Contracts

- The global feed and badge consume only `AppStore::agent_target_views()`.
  Historical session files, project status summaries, tray completion state,
  paths, and provider names cannot manufacture a live row.
- Row identity is `AgentRunId`. Current unread identity is
  `AgentRunId -> AgentEventId`; the watermark is process-local and intentionally
  separate from the legacy pane-level `DoneTracker`.
- Opening or closing the overlay never changes a watermark. Window focus may
  clear legacy tray completion state but cannot acknowledge the global feed.
- A target is unread only when its current event differs from the run watermark
  and the current state is pane attention, Blocked, Failed, Done, or Waiting.
  A later accepted event on the same run differs from the stored event and is
  unread again without an invalidation callback.
- Needs You contains pane attention, Blocked, Failed, and unread Done/Waiting.
  Working contains Live Starting/Working. Recent contains acknowledged
  Done/Waiting, Interrupted, Exited, Unknown, and all remaining Stale or
  Disconnected rows. Connectivity never rewrites activity.
- Each section sorts newest receipt first, then stable project, execution host,
  worktree, pane, provider, and run identities. Needs You and Working are
  unbounded; Recent rendering is capped.
- Activation submits only `AgentRunId`. The store re-resolves the exact current
  execution host, worktree, tab, pane, terminal session, incarnation, PTY route,
  and terminal entity before navigation. It acknowledges the resolved current
  event and clears exact pane attention only after the destination is active.
- A stale or missing target returns `false` before navigation, does not create,
  hydrate, resume, or focus a terminal, does not change a watermark, and leaves
  the overlay open. Workbench-page failure after exact focus also leaves the
  event unacknowledged.
- Terminal kill removes all runtime runs on the route and their acknowledgement
  watermarks. A GUI-only detach keeps runtime and watermark state so warm
  reattach does not make an already handled event unread again.
- The overlay remains fixed, anchored, non-modal, viewport-clamped, and
  list-scroll-only. Escape, outside click, close, and repeated Agents toggle use
  one close path with focus restoration. Successful row navigation closes
  without restoring the previous focus.
- Only exact value `0` hides and disables the global entry/overlay. Inline
  worktree Agent rows, Sessions, runtime reconciliation, and exact activation do
  not consult this gate.

### 4. Validation & Error Matrix

| Condition | Required behavior |
|-----------|-------------------|
| Historical session exists without an exact target | No global feed row |
| Same provider has multiple current runs | Render separate `AgentRunId` rows |
| Same path exists on different hosts/worktrees | Keep rows and routes distinct |
| Current event equals watermark | Mark acknowledged; do not classify Done/Waiting as unread |
| Same run accepts a later event | Mark the new event unread |
| Window becomes focused | Preserve feed watermarks |
| Working activity is disconnected | Display Working plus Offline in Recent; never call it Done |
| Blocked/Failed run is disconnected | Keep it in Needs You and show connectivity separately |
| Route, incarnation, PTY owner, or terminal entity is missing | Activation fails closed; overlay and watermark remain |
| Exact destination activates | Focus exact pane, acknowledge current event, then close overlay |
| Runtime route is killed | Remove its orphaned watermarks |
| Rollback variable equals `0` | Hide global entry and refuse overlay open; preserve inline/session/runtime behavior |

### 5. Good / Base / Bad Cases

- Good: Two Codex runs in sibling worktrees appear as separate rows and each
  focuses its own terminal incarnation.
- Good: A Done event is activated and acknowledged; a later Waiting event on
  the same run becomes unread again.
- Base: No authoritative runs produces an empty overlay without changing the
  current workbench or focus-return target.
- Bad: Use project ID or provider session ID as the acknowledgement key. One
  row then clears another run.
- Bad: Treat Disconnected as completion or use session history to add a live
  row.
- Bad: Close the overlay before exact activation reports success.

### 6. Tests Required

- Projection tests cover all three groups, connectivity/activity separation,
  deterministic equal-timestamp ordering, duplicate provider runs, same-path
  multi-host routes, and Recent bounding without limiting active rows.
- Watermark tests assert exact run/event acknowledgement, no unread for ordinary
  Working state, later-event renewal, and selective route-prune behavior.
- Route tests vary execution host, worktree, tab, pane, terminal session,
  incarnation, PTY route, and terminal entity and require failure for every
  mismatch.
- Overlay tests preserve viewport bounds and one close action for Escape,
  outside click, close control, and repeated toggle; opening causes no
  project/workbench mutation.
- Rollback parsing accepts only exact `0`; inline Agent and Sessions tests remain
  unchanged and passing.
- Run focused tests, workspace check, Clippy, and Windows MSVC check in Docker.

### 7. Wrong vs Correct

#### Wrong

```rust
let project = store.ai_projects(DoneScope::All).entries.first()?;
store.set_active_project(&project.id, cx);
store.clear_unread_done(cx);
```

This collapses multiple runs to a project, mixes tray acknowledgement with the
feed, and cannot validate a terminal incarnation.

#### Correct

```rust
let run_id = store
    .read(cx)
    .agent_target_views()
    .into_iter()
    .find(|target| target.run_id == run_id)?
    .run_id;

if AppStore::activate_agent_run(&store, &run_id, window, cx) {
    close_agents_without_focus_restore();
}
```

The view originates from authoritative runtime state, while the action
re-resolves every routing boundary and acknowledges only after exact focus.
