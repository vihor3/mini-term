# Remote Runtime Reconciliation Contract

## Scenario: Probe and install authoritative SSH project identity

### 1. Scope / Trigger

Use this contract when an SSH project is activated, restored, or asked to spawn
a terminal. The authenticated runtime probe must finish or explicitly fall back
before terminal hydration so compatibility identities are never silently
retagged after processes or documents exist.

### 2. Signatures

```rust
pub enum RemoteRuntimePhase {
    Connecting,
    Ready,
    CompatibilityFallback,
    RebindDeferred,
}

pub struct RemoteRuntimeProjectState {
    pub phase: RemoteRuntimePhase,
    pub snapshot: Option<RemoteRuntimeSnapshot>,
    pub error: Option<String>,
    // private request owner facts
}

pub fn remote_runtime_enabled() -> bool;
pub fn remote_runtime_state(
    &self,
    project_id: &str,
) -> Option<&RemoteRuntimeProjectState>;
pub fn retry_remote_runtime(&mut self, project_id: &str, cx: &mut Context<Self>);
```

Rollback environment:

```text
MINI_TERM_REMOTE_RUNTIME=0
```

### 3. Contracts

- Runtime state is keyed by compatibility `project_id`, but each in-flight
  request captures a process-monotonic generation, project path, connection ID,
  and process-local connection-configuration fingerprint.
- Completion applies only while every captured owner fact still matches, the
  stored phase remains `Connecting`, and the snapshot epoch equals the latest
  authenticated epoch observed for that connection. Project removal, path edits, credential or
  endpoint edits, and superseding probes make prior results stale.
- Remote activation and terminal spawn call the same deferral gate. `Connecting`
  stops hydration; successful completion installs/reconciles the authoritative
  binding first and only then resumes saved-layout hydration.
- A changed authoritative `WorktreeId` is installed only while the project has
  no live PTY and no open document. Otherwise phase becomes `RebindDeferred`
  with the authenticated snapshot retained for a visible retry action.
- Reconciliation uses `LayoutStore::reconcile_worktree_layouts` in one
  transaction. Existing destination state wins; the provisional source row is
  preserved. In-memory bindings update only after persistence succeeds.
- Probe/transport failure enters `CompatibilityFallback` and preserves existing
  SFTP, SSH terminal, session scan, and provisional identity behavior.
- Exact environment value `0` disables probing. Missing, `1`, `false`, and other
  values do not disable it.
- Every successful pool acquisition records a monotonic connection epoch. Exact
  session eviction clears only that same epoch; an older task cannot erase a
  newer observation. Project removal clears related runtime state. Late
  completions cannot recreate cleared state because generation and owner facts
  are revalidated.
- An SSH connection identity edit invalidates its pool, Agent polls, and runtime
  states, then immediately starts a forced fresh runtime request for each still-
  connected project. The replacement request preserves the prior
  `hydrate_after` intent. Removing the connection instead resumes pending
  hydration through compatibility fallback and leaves non-pending projects
  inert until configuration is repaired.
- Errors shown to UI are bounded summaries. Credentials, private-key contents,
  prompts, environment, and raw command output are never persisted.

### 4. Validation & Error Matrix

| Condition | Required behavior |
|-----------|-------------------|
| Local or WSL project | Skip remote probe and hydrate normally |
| SSH project begins probe | Store `Connecting`; defer hydration/spawn |
| Same request asks to hydrate again | Reuse request and retain `hydrate_after=true` |
| Path, endpoint, user, credential, or connection ID changes | Reject old completion; an available edited connection starts a forced new generation immediately |
| Edited connection had `hydrate_after=true` | Carry that intent to the replacement request and hydrate only after its result/fallback |
| Connection is removed while hydration is pending | Clear stale authority and resume compatibility hydration |
| Connection is removed without pending hydration | Clear stale authority and wait for explicit configuration/retry |
| Snapshot epoch differs from the current observed epoch | Reject it before binding reconciliation and enter compatibility fallback |
| Authenticated binding is unchanged | Enter `Ready`, then hydrate if requested |
| Binding changes with no PTY/documents | Reconcile transactionally, enter `Ready`, then hydrate |
| Binding changes with live PTY | Enter `RebindDeferred`; do not retag or spawn |
| Binding changes with open document | Enter `RebindDeferred`; do not move its workbench bucket |
| Reconciliation persistence fails | Enter fallback; do not install in-memory authoritative binding |
| Probe fails or connection is missing | Preserve compatibility behavior and expose bounded error |
| `MINI_TERM_REMOTE_RUNTIME=0` | Remove process runtime state and use provisional path immediately |
| Generation counter overflows | Fail closed into compatibility fallback |

### 5. Good / Base / Bad Cases

- Good: Startup probes an SSH worktree, migrates its saved layout to the
  authenticated `WorktreeId`, then creates/restores its terminal panes.
- Good: The user edits SSH credentials while a probe is running; the late
  completion is ignored and a forced replacement probe starts with the prior
  hydration intent.
- Base: An offline host enters compatibility fallback and existing remote file
  and terminal paths remain usable.
- Bad: Hydrate a PTY under a provisional binding and change `WorktreeId` when
  the probe returns; callbacks and recovery metadata would describe a process
  that was spawned under different identity facts.
- Bad: Delete the provisional layout row after migration; it is required for
  rollback and recovery review.

### 6. Tests Required

- Environment gate disables only exact `0`.
- Generation allocation is monotonic and fails on overflow.
- Owner-fact tests independently change generation, path, connection ID, and
  connection fingerprint and assert stale rejection.
- Epoch tests assert exact equality is required, observations never regress, and
  exact eviction cannot clear a newer epoch.
- Connection invalidation tests cover available-edit refresh, preservation of
  `hydrate_after`, removal-time fallback hydration, and no spurious hydration
  when no request was pending.
- Rebind decision tests independently assert live PTY and open-document blocks.
- Layout tests assert destination-wins migration and preservation of the prior
  worktree row.
- Integration checks cover Linux and Windows compilation of the deferral path.

### 7. Wrong vs Correct

#### Wrong

```rust
spawn_terminal(project);
probe_remote(project, move |snapshot| {
    project.binding = binding_from(snapshot);
});
```

The terminal has already been created under compatibility identity and cannot
be safely relabeled.

#### Correct

```rust
if store.defer_remote_hydration(project_id, cx) {
    return;
}
store.hydrate_project(project_id, cx);
```

The probe owns a generation-fenced pre-hydration gate. Completion reconciles the
binding before resuming hydration, or remains visibly deferred.
