# Workbench Identity Contract

## Scenario: Stable worktree, pane, and terminal routing

### 1. Scope / Trigger

Use this contract when code creates, restores, moves, reconnects, persists, or
routes a terminal/document workbench. It also applies to deferred callbacks
that can focus or close a page after the active project binding may have
changed.

This contract establishes stable routing identities. Local PTY ownership, warm
reattach, and output replay are layered through the
`mt-terminal-host/backend/terminal-host-contract.md` contract. Authenticated
SSH runtime identity and pre-hydration rebinding are defined by
`mt-app/backend/remote-runtime-reconciliation-contract.md` and
`mt-ssh/backend/remote-runtime-contract.md`.

### 2. Signatures

Shared opaque identities from `mt-identity`:

```rust
pub struct HostInstallId(String);
pub struct ExecutionHostId(String);
pub struct RepoId(String);
pub struct WorktreeId(String);
pub struct TabId(String);
pub struct PaneKey(String);
pub struct TerminalSessionId(String);
pub struct TerminalIncarnationId(String);
```

AppStore routing boundary:

```rust
pub fn active_worktree_id(&self) -> Option<&WorktreeId>;
pub fn worktree_id_for_project(&self, project_id: &str) -> Option<&WorktreeId>;
pub fn terminal_binding_matches(
    &self,
    worktree_id: &WorktreeId,
    tab_id: &TabId,
    pane_key: &PaneKey,
    terminal_session_id: &TerminalSessionId,
    terminal_incarnation_id: &TerminalIncarnationId,
) -> bool;
```

Deferred workbench handoff boundary:

```rust
pub fn close_document_source(
    expected_worktree_id: WorktreeId,
    source: DocumentSource,
    window: &mut Window,
    cx: &mut App,
);

pub fn is_document_active(
    expected_worktree_id: &WorktreeId,
    source: &DocumentSource,
    cx: &App,
) -> bool;

pub fn reactivate_active_document(
    expected_project_id: &str,
    expected_worktree_id: &WorktreeId,
    window: &mut Window,
    cx: &mut App,
);

pub fn reactivate_active_page(
    expected_project_id: &str,
    expected_worktree_id: &WorktreeId,
    window: &mut Window,
    cx: &mut App,
);
```

### 3. Contracts

- Serialized identities are the complete routing representation. Random IDs
  use canonical UUID v4 payloads; derived IDs use SHA-256 with versioned,
  length-prefixed domains. Callers never concatenate or parse payloads.
- `project_id` remains a compatibility/configuration key. `WorktreeId` owns
  workbench layout, document bucket, preview slot, and active-page state.
- `PaneKey` and `TerminalSessionId` survive save/reload, split, move, reorder,
  rename, and worktree switches. A successful warm attach keeps both session
  and incarnation. A new PTY spawn or explicit reconnect keeps the session ID
  and mints a new `TerminalIncarnationId`.
- Cold restore validates the persisted worktree and previous incarnation,
  reconstructs the terminal at its recorded source size, applies the snapshot,
  and then attaches the new process output. It preserves pane/session identity
  but always mints a new incarnation.
- `RestoredHistory` is not `Reattached`. Provider resume remains a separate
  post-restore action; only a true warm reattach suppresses duplicate resume.
- The process-local `u32 pty_id` is an attachment handle only. It is not a
  persisted logical terminal identity.
- A terminal event is accepted only when project binding, pane key, logical
  session, and expected incarnation all match the current route.
- Every deferred close/focus/search callback captures its originating
  `WorktreeId` before yielding. The callback revalidates project-to-worktree
  binding and fails closed after a rebind.
- Local child processes receive these routing fields:
  `MINITERM_PTY_ID`, `MINITERM_TAB_ID`, `MINITERM_PANE_KEY`,
  `MINITERM_TERMINAL_SESSION_ID`, `MINITERM_TERMINAL_INCARNATION_ID`,
  `MINITERM_EXECUTION_HOST_ID`, and `MINITERM_WORKTREE_ID`.
- Identity environment values are correlation/fencing keys, not credentials
  or remote attestation.
- A provisional SSH binding may be replaced with an authoritative remote binding
  only before PTY hydration and while no document from that project is open.
  Existing panes or documents force a visible deferred-rebind state; they are
  never silently retagged.

### 4. Validation & Error Matrix

| Condition | Required behavior |
|-----------|-------------------|
| Active project has no binding | Do not open, focus, save, or route worktree state |
| Project was rebound after a callback was scheduled | Reject the callback without touching either worktree |
| PTY event has an old incarnation | Reject it even when pane and session IDs still match |
| Restored stable pointer is invalid | Fall back deterministically and persist the corrected pointer |
| One project alias is removed while another keeps the worktree | Keep the worktree bucket; remove only stale clean tabs and retain dirty drafts |
| New PTY is spawned | Preserve pane/session identity and rotate incarnation |
| Pane is moved or reordered | Preserve pane key, session, incarnation, and current PTY attachment |
| Remote/WSL identity is provisional | Route consistently but never present it as verified host authority |
| Authenticated remote identity differs before hydration | Reconcile layout transactionally, install binding, then hydrate |
| Authenticated remote identity differs with live PTY or open document | Defer the binding change and preserve the current route |
| Hosted session remains live after GUI restart | Attach-only and preserve its PID/incarnation |
| Hosted session is missing or has a replay gap with valid history | Explicitly restore, apply snapshot first, and rotate incarnation |
| Recovery history is missing or corrupt | Start clean with a visible unavailable notice; never label it reattached |
| Old incarnation attempts restore after recovery | Reject it before replacing or mutating the new session |

### 5. Good / Base / Bad Cases

- Good: Worktree A keeps a terminal page while worktree B keeps a file page;
  switching restores each worktree's own route.
- Good: An Agents overlay opened on a document captures project and worktree;
  closing it after a project rebind does not focus the new worktree.
- Base: A GUI restart warm-attaches when the host still owns the exact
  incarnation; otherwise cold recovery creates a new incarnation while
  preserving the logical terminal session and pane identity.
- Bad: Recompute `WorktreeId` from whichever project is active when an async
  callback completes.
- Bad: Treat `pty_id` as stable across process restart.
- Bad: Interpret provisional SSH identity as authenticated remote-device
  identity.

### 6. Tests Required

- Parse/serde tests reject wrong prefixes, uppercase digests, and non-v4 UUIDs.
- Golden derivation tests freeze domain separation and length framing.
- Layout round trips assert tab, pane, session, incarnation, active tab, and
  active pane identities.
- Split/move/reorder tests assert stable identities and PTY attachment survive.
- Reconnect tests assert session preservation and incarnation rotation.
- Route tests assert the prior incarnation cannot match the current binding.
- Cold-restore tests assert snapshot-before-attach ordering, source-size
  restoration, new-incarnation fencing, corruption fallback, and provider
  resume only after the snapshot is installed.
- Workbench tests assert worktree-isolated previews and stale callback rejection.
- Search/overlay tests assert same project/path with a different `WorktreeId`
  cannot receive deferred focus.
- Remote runtime tests assert authoritative rebind is blocked by either live PTYs
  or open documents and that a safe rebind uses layout reconciliation before
  hydration.

### 7. Wrong vs Correct

#### Wrong

```rust
window.defer(cx, move |window, cx| {
    let project_id = store.read(cx).active_project_id.clone().unwrap();
    reactivate_active_page(&project_id, window, cx);
});
```

The callback derives ownership after the user may have switched or rebound the
project.

#### Correct

```rust
let project_id = project.id.clone();
let worktree_id = store.worktree_id_for_project(&project_id)?.clone();
window.defer(cx, move |window, cx| {
    reactivate_active_page(&project_id, &worktree_id, window, cx);
});
```

The originating scope is captured before yielding and the workbench validates
both identities again before focus changes.
