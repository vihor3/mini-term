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

pub struct TerminalJumpTarget {
    pub project_id: String,
    pub execution_host_id: ExecutionHostId,
    pub worktree_id: WorktreeId,
    pub tab_id: TabId,
    pub pane_key: PaneKey,
    pub terminal_session_id: TerminalSessionId,
    pub terminal_incarnation_id: Option<TerminalIncarnationId>,
}

pub fn terminal_jump_views(&self) -> Vec<TerminalJumpView>;
pub fn terminal_tab_views(&self, project_id: &str) -> Vec<TerminalJumpView>;
pub fn terminal_jump_target_for_pane(
    &self,
    project_id: &str,
    pane_id: &str,
) -> Option<TerminalJumpTarget>;
pub fn activate_terminal_jump_target(
    store: &Entity<AppStore>,
    target: &TerminalJumpTarget,
    window: &mut Window,
    cx: &mut App,
) -> bool;
pub fn reorder_terminal_tabs(
    &mut self,
    source: &TerminalJumpTarget,
    target: &TerminalJumpTarget,
    after: bool,
    cx: &mut Context<Self>,
) -> bool;

// pane_actions.rs: all X/menu/keyboard close paths use the same confirmation.
pub fn close_terminal_target(
    store: Entity<AppStore>,
    target: TerminalJumpTarget,
    window: &mut Window,
    cx: &mut App,
);

// AppStore owns the retained request/token and returns true only for a removed
// currently selected terminal that needs a workbench focus handoff.
pub(crate) fn terminal_close_request(
    &self,
    target: &TerminalJumpTarget,
) -> Option<TerminalCloseRequest>;
pub(crate) fn close_terminal_target(
    &mut self,
    request: TerminalCloseRequest,
    cx: &mut Context<Self>,
) -> Task<bool>;
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
- Each worktree has one visible terminal surface. `ProjectState` retains
  `selected_terminal_pane_key: Option<PaneKey>` and `terminal_order: Vec<PaneKey>`
  beside its complete legacy route owners. A top-level visual tab represents a
  terminal `PaneKey`, not a new or replacement routing `TabId`.
- `terminal_tab_views` is an ordered read-only inventory, including dormant and
  exited records from every old owner/leaf. Rendering cannot hydrate, reparent,
  respawn, or drop them. New split/group entrypoints are not compatibility paths.
- Clicks, cycle/index, Quick Open and Agent navigation use singular selection.
  `focus_pane` is focus-only and rejects inactive/unselected panes. Reordering
  captures and validates both complete targets and changes presentation order
  only; it cannot move a live terminal between routing owners.
- New desktop terminals select before persistence. Closing the selected terminal
  selects its flat right neighbor, otherwise left, otherwise empty. Closing a
  background terminal preserves selection and never hydrates siblings. Mobile
  background creation appends the order without replacing desktop selection or
  focus. Deferred close confirmation revalidates the captured full target.
- Close confirmation also captures the current GUI attachment, binding
  provenance, configured project source and other worktree-alias snapshots.
  Installing an attachment during confirmation invalidates that old request.
  A dormant local/WSL record with a saved incarnation is removed only after
  asynchronous, session/incarnation-fenced host `Kill` returns `Ok`. No host
  error, including `SessionMissing` from an older host, proves history cleanup.
  Disabled/unavailable hosting retains the record with a bounded error because
  saved panes do not record transport provenance. SSH compatibility and records
  without a saved incarnation do not issue a host mutation.
- Before any close mutation, a source alias must agree with other aliases'
  captured saved layouts, and another alias cannot own a pending worktree save.
  After flush the pending-owner map is cleared, so absence of that owner alone
  is not proof that the source snapshot is current. Divergence refuses close
  with notification rather than writing an older inventory over another alias.
- A logical-session-keyed pending-close token excludes duplicate close and
  activation/hydration/reconnect through aliases until its owner completes.
  Existing other-alias attachments reject dispatch. Same-owner selection/order
  changes do not invalidate completion: remove the intended background record
  while retaining the new selection. Conflicting other-alias changes retain the
  exact record as runtime Error with notification, without saving over newer
  alias state. Changed original source/route/attachment is inert.
- Pending-close navigation is rejected before changing project/worktree/page
  scope, and dormant activation must honor its internal rejection result. A
  remembered global focus key cannot turn a rejected activation into success.
- Reconnect resolves its captured route, shell and CWD before disposing the
  current attachment. Missing prerequisites leave that view and close identity
  intact. Never weaken exact-route validation to work around a stale handle.
- Fork opens one new terminal, never a split. Capture source route, provider
  session, shell, CWD and selected/focused state before asynchronous CWD lookup;
  changed source/focus makes completion inert. Register lineage before writing
  the fork command. Never substitute a different source after a stale result.
- Right-side `ContextPanel` selection remains global, while workbench pages and
  panel contents stay worktree-owned; see `worktree-context-contract.md`.
- A global terminal jump captures project, execution host, worktree, tab, pane,
  logical terminal session, and the optional saved/live incarnation before the
  user selects it. No field may be derived from the later active project.
- Resolution validates the complete target, changes project without hydrating
  unrelated panes, then validates the same complete target again before focus.
  A rebind or layout change between those checks is inert.
- An exact dormant pane may run the ordinary hydration path for that saved
  pane. That compatibility path may hydrate other eligible dormant records in
  its original legacy owner; flattening presentation does not silently change
  this recovery policy. A live target uses its existing terminal entity without
  hydrating siblings. Neither path creates a replacement pane when the selected
  identity disappeared.
- `PaneKey` and `TerminalSessionId` survive save/reload, legacy split/move, reorder,
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
- A persisted authoritative SSH binding is reusable only when its opaque
  `identity_context` exactly matches the current non-secret tuple
  `("ssh-authority-v2", connection_id, host, normalized_port, user,
  normalized_configured_path)`. Display name, password, and private-key path do
  not participate. Missing legacy context or any endpoint/path mismatch fails
  closed to provisional resolution and a fresh authenticated probe.
- `identity_context` proves the configured path alias; the binding's
  `canonical_worktree_path` remains the authenticated canonical target. A
  configured symlink such as `/srv/repo-link` therefore may retain authenticated
  `/srv/repo-real` across restart without deriving a new worktree identity.
- A safe authoritative rebind with no live PTY and no open document replaces
  any non-empty provisional startup hydration with the reconciled destination
  layout. Non-empty panels alone are not proof of live ownership.

### 4. Validation & Error Matrix

| Condition | Required behavior |
|-----------|-------------------|
| Active project has no binding | Do not open, focus, save, or route worktree state |
| Project was rebound after a callback was scheduled | Reject the callback without touching either worktree |
| PTY event has an old incarnation | Reject it even when pane and session IDs still match |
| Terminal jump differs by host/worktree/tab/pane/session/incarnation | Reject it without changing the current workbench |
| Exact terminal jump points to a dormant saved pane | Activate that pane and run its ordinary hydration path |
| Terminal jump target disappears during project activation | Reject the second check; never create or focus a substitute |
| Restored stable pointer is invalid | Fall back deterministically and persist the corrected pointer |
| One project alias is removed while another keeps the worktree | Keep the worktree bucket; remove only stale clean tabs and retain dirty drafts |
| New PTY is spawned | Preserve pane/session identity and rotate incarnation |
| Pane is moved or reordered | Preserve pane key, session, incarnation, and current PTY attachment |
| A visual terminal tab is reordered across old panel boundaries | Change flat order only; keep its original `TabId` and full route |
| Close confirmation returns after reconnect or project rebind | Reject the captured target without closing a substitute terminal |
| Background terminal closes or Mobile appends a terminal | Preserve selected desktop terminal and focus |
| Fork source/focus changes while CWD lookup is pending | Reject completion before creating a terminal or sending input |
| A saved dormant hosted tab is confirmed closed | Run only the fenced host close asynchronously; remove after confirmed success |
| Host is unavailable, disabled, mismatched or cannot identify stored history | Keep the dormant record and show a bounded error |
| Another tab is selected/reordered in the same project during close | Complete the intended close while preserving current selection |
| Another alias changes its saved layout during a confirmed host close | Retain an explicit Error record without overwriting the newer alias |
| Reconnect has no usable shell | Preserve the current attachment and saved identity; leave close possible |
| Remote/WSL identity is provisional | Route consistently but never present it as verified host authority |
| Persisted authoritative SSH context exactly matches current endpoint and configured path | Reuse the authoritative IDs and authenticated canonical path |
| Persisted authoritative SSH context is missing, legacy, or mismatched | Do not reuse it; resolve provisionally and require a fresh probe |
| Configured SSH path is an alias of the authenticated canonical path | Match provenance by configured path but preserve the authenticated canonical path |
| Authenticated remote identity differs before hydration | Reconcile layout transactionally, install binding, replace cold provisional state, then hydrate |
| Authenticated remote identity differs with live PTY or open document | Defer the binding change and preserve the current route |
| Hosted session remains live after GUI restart | Attach-only and preserve its PID/incarnation |
| Hosted session is missing or has a replay gap with valid history | Explicitly restore, apply snapshot first, and rotate incarnation |
| Recovery history is missing or corrupt | Start clean with a visible unavailable notice; never label it reattached |
| Old incarnation attempts restore after recovery | Reject it before replacing or mutating the new session |

### 5. Good / Base / Bad Cases

- Good: Worktree A keeps a terminal page while worktree B keeps a file page;
  switching restores each worktree's own route.
- Good: Quick Open selects one dormant pane by its saved complete target and
  hydrates that pane without touching sibling worktrees.
- Good: Reordering a non-first legacy leaf does not rewrite its route; an Agent
  event carrying the original owner still reaches that exact terminal.
- Bad: switch projects first, then look up a pane by display label or whichever
  pane is active in the destination.
- Good: An Agents overlay opened on a document captures project and worktree;
  closing it after a project rebind does not focus the new worktree.
- Good: An SSH project configured as `/srv/repo-link` reuses its matching
  authenticated `/srv/repo-real` binding after restart; changing host, user,
  port, connection ID, or configured path makes that persisted authority inert.
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
- Terminal jump projection tests assert saved dormant panes and live panes carry
  the complete stable target and accurate state flags.
- Exact activation tests vary every identity component, cover the second
  revalidation, and prove stale selection never creates a replacement pane.
- Flat-navigation regressions cover all legacy owners/leaves, selected non-first
  restore, full-order cycle/index, selected/background/last close, Mobile append,
  retained live attachments and stale reorder/fork/confirmation targets.
- Dormant-close tests cover transport/no-incarnation decisions, retained token
  ownership, alias activation/reconnect exclusion, every captured identity and
  source field, failure-to-success rejection, harmless selection/order changes,
  conflicting aliases and shellless reconnect preflight. Host cleanup fixtures
  and real GUI timing/focus acceptance remain separate Actions/artifact gates.
- Remote runtime tests assert authoritative rebind is blocked by either live PTYs
  or open documents and that a safe rebind uses layout reconciliation before
  hydration.
- SSH persistence tests cover exact-context reuse, normalized default port,
  display/credential changes that do not affect identity, endpoint/path changes
  that do, legacy/missing context rejection, configured symlink preservation,
  and replacement of cold non-empty provisional hydration.

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
