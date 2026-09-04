# Research: Host-scoped project filesystem, Git, and persistence operations

- Query: Research host-scoped filesystem/Git operations and project persistence for the unified local/SSH add-project flow, including execution-host selection, canonical paths, Git discovery/init/clone, deduplication, rollback, and opening the resulting project.
- Scope: internal
- Date: 2026-09-04

## Findings

### Confirmed conclusions

1. `ProjectExecutionSnapshot` cannot be the onboarding root abstraction because it requires an already registered project, worktree, and execution-host identity (`crates/mt-app/src/execution_host.rs:76-111`, `crates/mt-app/src/store/identity.rs:277-359`). Onboarding needs a smaller pre-project host context.
2. The local process runner already has the required structured argv, bounded output, timeout, and process-tree cleanup behavior (`crates/mt-app/src/execution_host.rs:157-200`, `crates/mt-app/src/execution_host.rs:665-798`). Reuse that machinery for local `git clone`/`git init` rather than the older Git helper whose timeout does not terminate the child (`crates/mt-project/src/git.rs:1124-1189`).
3. SSH mutations must preserve `BoundedExecState`. The current generic `execute_host_command` collapses uncertain SSH states into a generic disconnected error (`crates/mt-app/src/execution_host.rs:202-250`), while the underlying SSH API distinguishes safe-before-dispatch from may-have-started outcomes (`crates/mt-ssh/src/pool.rs:694-765`). Clone/init cannot safely retry after that distinction is lost.
4. Add Existing Folder requires a new pure remote path/Git probe. `runtime_snapshot` may create `~/.mini-term/runtime-v1/install-id`, so it violates the PRD's no-mutation rule if used merely to validate a folder (`crates/mt-ssh/src/runtime.rs:177-197`; `.trellis/tasks/09-04-unified-add-project-flow/prd.md:31-38`).
5. Current project persistence cannot enforce AC9: local add APIs use two different dedupe rules, and remote add always creates a new ID (`crates/mt-app/src/store/projects.rs:105-168`, `crates/mt-app/src/store/projects.rs:188-237`, `crates/mt-app/src/store/ssh.rs:201-247`). Dedupe must move to one host-qualified store boundary.
6. Filesystem success and durable project registration are separate outcomes. `save_config_now` only enqueues a snapshot and writer failures are logged, not returned (`crates/mt-app/src/store/layout.rs:670-709`, `crates/mt-app/src/store/config_writer.rs:199-213`, `crates/mt-app/src/store/config_writer.rs:281-296`). A persistence failure must never trigger deletion of a successfully cloned or initialized repository.

### Files found

| File | Relevant responsibility |
|---|---|
| `.trellis/tasks/09-04-unified-add-project-flow/prd.md` | Source requirements for pure add, host-local clone/init, dedupe, stale-result fencing, rollback, and activation. |
| `crates/mt-app/src/execution_host.rs` | Project-bound local/WSL/SSH command planning and bounded local process execution. |
| `crates/mt-app/src/remote_ssh/mod.rs` | Saved-connection lookup, pooled session acquisition, SFTP facade, bounded SSH exec, and runtime snapshots. |
| `crates/mt-app/src/remote_ssh/dirs.rs` | Remote directory validation/browsing and project-root-scoped creation. |
| `crates/mt-app/src/remote_ssh/delete.rs` | Existing same-session SFTP/shell proof and ambiguous-mutation verification pattern. |
| `crates/mt-app/src/remote_ssh/paths.rs` | POSIX path joining, normalization, basename validation, and containment helpers. |
| `crates/mt-ssh/src/sftp.rs` | Canonicalization, lstat-style type probes, exclusive/single-level creation, and empty/recursive removal. |
| `crates/mt-ssh/src/pool.rs` | Exact SSH dispatch and cleanup state required for safe mutation decisions. |
| `crates/mt-ssh/src/runtime.rs` | Authenticated host identity, current remote Git common-dir discovery, and runtime bootstrap mutation. |
| `crates/mt-project/src/worktree/identity.rs` | Host-owned local canonicalization and local/provisional SSH worktree identity. |
| `crates/mt-project/src/git.rs` | Existing local Git discovery and CLI helpers; no production clone/init API. |
| `crates/mt-project/src/fs.rs` | Local collision-rejecting creation and recursive deletion behavior. |
| `crates/mt-app/src/store/projects.rs` | Local registration, inconsistent local dedupe, activation, and hydration entry point. |
| `crates/mt-app/src/store/ssh.rs` | Remote registration that currently always creates a project. |
| `crates/mt-app/src/store/identity.rs` | Project/worktree bindings, authoritative remote installation, and execution snapshots. |
| `crates/mt-config/src/db.rs` | Config persistence keyed only by project ID. |
| `crates/mt-layout/src/lib.rs` | Project-to-worktree bindings; aliases are intentionally representable. |
| `crates/mt-app/src/store/remote_runtime.rs` | Existing generation/fingerprint/epoch fencing and deferred SSH hydration. |
| `crates/mt-app/src/store/panes.rs` | Existing activation path's remote-runtime gate and pane hydration. |

### Existing host and command boundaries

- `ExecutionBackendSignature` already represents Local, WSL, and SSH with connection ID, configuration fingerprint, and optional connection epoch (`crates/mt-app/src/execution_host.rs:30-74`). This is suitable as a completion-fencing component, but the surrounding `ExecutionSourceSignature` is project/worktree-specific (`crates/mt-app/src/execution_host.rs:76-97`).
- Command plans retain structured program/argv locally and under WSL; only SSH serializes POSIX argv (`crates/mt-app/src/execution_host.rs:139-200`, `crates/mt-app/src/execution_host.rs:253-293`). Preserve this rule in onboarding.
- The local runner owns a process group on Unix and a kill-on-close Job Object on Windows, drains bounded stdout/stderr, and terminates descendants on timeout (`crates/mt-app/src/execution_host.rs:302-364`, `crates/mt-app/src/execution_host.rs:486-635`, `crates/mt-app/src/execution_host.rs:665-798`).
- The SSH facade acquires an authenticated pooled session, records its epoch, and rejects a result superseded by a newer session (`crates/mt-app/src/remote_ssh/mod.rs:344-370`, `crates/mt-app/src/remote_ssh/mod.rs:426-466`). Public facades are blocking and must run on GPUI's background executor (`crates/mt-app/src/remote_ssh/mod.rs:14-27`).
- `BoundedExecState::{NotDispatched, ExecEnqueueTimedOut, Rejected}` can permit fallback; `ExecReplyUnknown` and `Started` may have executed; uncertain channel cleanup requires exact-session retirement (`crates/mt-ssh/src/pool.rs:694-765`). The SSH error contract forbids retry/fallback until post-state is verified when a command may have started (`.trellis/spec/mt-ssh/backend/error-handling.md:19-36`, `.trellis/spec/mt-ssh/backend/error-handling.md:40-53`).
- Remote delete already demonstrates the correct shape: prove SFTP and shell see the same canonical parent, retain the exact exec result, probe post-state, and only fallback when dispatch state or verified absence makes it safe (`crates/mt-app/src/remote_ssh/delete.rs:90-123`, `crates/mt-app/src/remote_ssh/delete.rs:413-474`). Clone/init should follow this orchestration model.
- `mt-terminal-host` is not the filesystem/Git execution layer. It owns local/WSL PTY lifecycle; SSH terminals remain a compatibility transport (`.trellis/spec/mt-terminal-host/backend/terminal-host-contract.md:5-14`).

### Path canonicalization and collision checks

- Host-owned canonicalization is normative: local paths are canonicalized and required to be directories; SSH provisional paths are normalized absolute POSIX paths, with authenticated canonical paths replacing provisional aliases later (`crates/mt-project/src/worktree/identity.rs:79-134`, `crates/mt-project/src/worktree/identity.rs:182-223`; `.trellis/spec/mt-project/backend/worktree-identity-contract.md:56-89`).
- Windows local comparison normalizes separators/case; POSIX comparison preserves case and backslashes (`crates/mt-project/src/worktree/mod.rs:70-92`). Do not apply Windows comparison rules to remote POSIX paths.
- Remote validation/browsing already uses SFTP `canonicalize` and directory stat (`crates/mt-app/src/remote_ssh/dirs.rs:214-310`). New onboarding probes should return this canonical path, not the user's spelling.
- Collision-sensitive code must use `try_node_kind`, which uses lstat semantics and distinguishes absence from protocol failure (`crates/mt-ssh/src/sftp.rs:263-302`). Do not use `exists()`: it converts all I/O errors to `false` (`crates/mt-ssh/src/sftp.rs:258-260`).
- `SftpHandle::create_dir` is a single-level creation whose parent must exist (`crates/mt-ssh/src/sftp.rs:554-560`). `create_dir_all` has mkdir-p semantics and treats an existing final directory as success, so it is not valid for New Folder collision rejection (`crates/mt-ssh/src/sftp.rs:470-513`).
- Local `mt_project::fs::create_directory` rejects an existing entry and uses single-level `fs::create_dir` (`crates/mt-project/src/fs.rs:414-420`). Its surrounding API is project-root-scoped, so onboarding may need a small parent-directory primitive rather than pretending the destination parent is an existing project.
- Folder-name validation must require one basename: non-empty, not `.`/`..`, no separators or NUL, plus host-specific invalid-name rules. The existing remote helper enforces a stricter basename policy including `\` and `:` (`crates/mt-app/src/remote_ssh/paths.rs:51-74`).

### Git discovery and mutation support

- Current remote discovery runs `git -C <path> rev-parse --path-format=absolute --git-common-dir`, with a legacy fallback, and strictly parses one path (`crates/mt-ssh/src/runtime.rs:446-511`). It does not return `--show-toplevel`, so it cannot distinguish an exact repository root from a directory nested inside a containing worktree.
- The required probe should execute, on the selected host, the equivalent of:

  ```text
  git -C <canonical-path> rev-parse --path-format=absolute --show-toplevel --git-common-dir
  ```

  Parse exactly two bounded paths, canonicalize both on the owning host, and classify `NotGit`, `RepositoryRoot`, or `NestedInRepository`. Retry a legacy command only for a proven unsupported-option result, not for ordinary Git failure. Prior Orca research identified the same two-path command (`.trellis/tasks/archive/2026-09/09-01-orca-worktree-terminal-research/research/orca-worktree-terminal-agent-architecture.md:47-50`).
- If a `.git` marker exists but discovery fails, fail closed rather than classify the directory as non-Git (`crates/mt-project/src/worktree/identity.rs:108-131`, `crates/mt-ssh/src/runtime.rs:219-230`; `.trellis/spec/mt-project/backend/worktree-identity-contract.md:91-107`).
- Local Git discovery elsewhere can search parent directories and expose repository roots, but it is panel-oriented and not a strict onboarding classification API (`crates/mt-project/src/git.rs:270-320`, `crates/mt-project/src/git.rs:358-443`, `crates/mt-project/src/git.rs:514-538`).
- No production clone/init API was found. `Repository::init` occurs only in tests, and no production `git clone` implementation exists (`crates/mt-project/src/git.rs:1791`). The existing CLI helper is restricted to an existing `.git` path and leaves a timed-out child running (`crates/mt-project/src/git.rs:1124-1189`), so it must not be extended unchanged for onboarding.
- Git must execute on the selected host so the host's credential manager, SSH agent, hooks, and environment remain authoritative; the codebase already documents this reason for CLI network operations (`crates/mt-project/src/git.rs:10-14`; `.trellis/tasks/09-04-unified-add-project-flow/prd.md:40-48`).

### Project persistence, deduplication, and opening

- `ProjectConfig` stores the configured path and optional `ssh_connection_id`; an SSH path is an absolute remote POSIX path (`crates/mt-config/src/config.rs:419-464`).
- `config.db` has only `projects.id` as a uniqueness key; there is no host/path uniqueness constraint (`crates/mt-config/src/db.rs:69-88`).
- `layout.db` keys project bindings by `project_id` and has only a non-unique index on `worktree_id`, so multiple project aliases for one worktree are supported (`crates/mt-layout/src/lib.rs:57-88`, `crates/mt-layout/src/lib.rs:991-1038`). Do not add a database uniqueness constraint to implement onboarding dedupe.
- `add_project_at` uses normalized local paths and returns an existing ID, while `add_project` compares exact strings; both register identity after inserting in-memory config (`crates/mt-app/src/store/projects.rs:105-168`, `crates/mt-app/src/store/projects.rs:188-237`).
- `add_remote_project` always generates a new ID and explicitly excludes remote projects from local path dedupe (`crates/mt-app/src/store/ssh.rs:201-247`).
- Identity registration can silently return on resolution failure; layout binding persistence failure is logged and an in-memory binding is retained (`crates/mt-app/src/store/identity.rs:419-462`). A new registration API should return an explicit result rather than hide these failures.
- The strongest existing alias lookup is `project_id_for_worktree`, which prefers the active alias and otherwise follows config order (`crates/mt-app/src/store/identity.rs:371-383`). This can seed dedupe behavior once a verified `WorktreeId` is available.
- Existing opening semantics must be retained: `set_active_project` updates active project/worktree, invokes `hydrate_project`, records last active project, and notifies (`crates/mt-app/src/store/projects.rs:73-103`). SSH hydration first passes through the remote-runtime reconciliation gate (`crates/mt-app/src/store/panes.rs:679-689`, `crates/mt-app/src/store/remote_runtime.rs:114-130`).

### Proposed minimal contracts

#### 1. Pre-project host selection

```rust
enum ProjectHostSelection {
    Local,
    Ssh {
        connection: SshConnection,
        connection_fingerprint: u64,
    },
}

struct ProjectHostOperationContext {
    modal_instance_id: u64,
    host_generation: u64,
    operation_id: u64,
    host: ProjectHostSelection,
}
```

- Capture an immutable context at submit time. A host switch increments `host_generation` and invalidates host-dependent paths and probes.
- On SSH acquisition, extend the result with the observed `connection_epoch`; completion is usable only if modal instance, host generation, operation ID, connection ID/fingerprint, and current epoch still match.
- Do not require `project_id`, `WorktreeId`, or `ExecutionHostId` before the operation has verified a path and registration has resolved identity.
- The PRD selector is Local plus saved SSH hosts. Existing WSL execution remains derivable from a selected local UNC path (`crates/mt-app/src/store/identity.rs:329-343`), but whether the new UI explicitly exposes WSL is outside this host-service contract.

#### 2. Pure host path/Git probe

```rust
struct HostPathProbe {
    canonical_path: String,
    node_kind: Directory,
    directory_empty: Option<bool>,
    git: GitLocation,
    observed_connection_epoch: Option<u64>,
}

enum GitLocation {
    NotGit,
    RepositoryRoot { top_level: String, common_dir: String },
    NestedInRepository { top_level: String, common_dir: String },
}
```

- The probe performs canonicalization, exact type checks, optional emptiness checks, and strict two-path Git discovery only. It must not create runtime state or project records.
- `Add Existing Folder` consumes this probe directly and accepts both `NotGit` and `RepositoryRoot`/`NestedInRepository` without mutation.
- `Initialize Existing Folder` maps exact root to `Add Project`, nested to a blocked result carrying `top_level`, and only `NotGit` to a possible `git init`.

#### 3. Mutation execution result

```rust
struct HostMutationResult {
    command: BoundedCommandOutcome,
    post_probe: Option<HostPathProbe>,
    created_target: bool,
    observed_connection_epoch: Option<u64>,
}

enum BoundedCommandOutcome {
    Completed { exit_code: i32, stdout: Vec<u8>, stderr: Vec<u8> },
    SafeBeforeDispatchFailure,
    OutcomeUncertain,
}
```

- Local execution maps bounded process-tree results into this type. SSH execution retains the full `BoundedExecOutput` internally and derives the three business states without discarding dispatch/cleanup evidence.
- `OutcomeUncertain` is never blindly retried. Retire only the exact failed session when required, then verify target state. If verification proves the intended postcondition, finish as success; otherwise preserve the target and report uncertainty.
- Return bounded, redacted errors. Git URLs can contain credentials; existing sidecar tests already redact URL userinfo for `git clone` (`sidecars/src/ssh_service.rs:1453-1484`), but the app execution layer currently has no equivalent shared redaction.

#### 4. Central dedupe/register/activate boundary

```rust
enum ProjectRegistrationDisposition {
    ActivatedExisting,
    RegisteredNew,
}

struct ProjectRegistrationOutcome {
    project_id: String,
    worktree_id: WorktreeId,
    disposition: ProjectRegistrationDisposition,
}

fn register_or_activate_project(
    host: VerifiedProjectHost,
    probe: HostPathProbe,
    target_group: Option<&str>,
) -> Result<ProjectRegistrationOutcome, ProjectRegistrationError>;
```

- Prefer dedupe by verified `WorktreeId`, equivalently verified execution host plus canonical worktree path. Before authenticated remote authority is available, use exact saved connection ID plus canonical SFTP path as the provisional key; never use display labels.
- Keep alias support in storage. This API prevents accidental onboarding duplicates; it does not outlaw intentional aliases elsewhere.
- Return persistence/identity errors explicitly. Once the filesystem postcondition is verified, a config/layout failure is a registration failure with retry/recovery guidance, not a filesystem rollback trigger.
- On success use the existing `set_active_project` path, allowing SSH runtime reconciliation to complete before hydration. Do not duplicate pane creation or activation logic in each subflow.

### Per-operation contract

| Operation | Execution-time precondition | Command | Required postcondition | Rollback |
|---|---|---|---|---|
| Add Existing | Canonical existing directory | None | Pure probe succeeds | None; no filesystem mutation permitted |
| Clone | Canonical parent; target absent or explicitly allowed existing empty directory; recheck immediately before exec | `git clone -- <url> <folder>` on selected host | Target probes as exact Git repository root | Always preserve a failed or uncertain target. Clone does not prove exclusive ownership of the destination path, even when preflight observed it as absent. |
| New Folder | Canonical parent; target absent by lstat/type probe | Exclusive single-level create, then `git -C <target> init` | Target probes as exact Git repository root | Remove with empty-directory-only removal only when this operation created it, no command may still be running, and it remains empty. Preserve non-empty/uncertain state. |
| Initialize Existing | Canonical existing directory; strict probe is `NotGit` | `git -C <path> init` | Same directory probes as exact Git repository root | Never delete or overwrite the pre-existing directory/files. |
| Existing Git Root | Strict probe says exact root | None | Registration receives verified root/common-dir | None |
| Nested Git Folder | Strict probe says nested | None | Block and return containing root | None |

Additional rules:

- UI preflight is advisory. Recheck collisions and path type in the operation engine because state can change after validation (`.trellis/spec/guides/cross-layer-thinking-guide.md:359-371`).
- Use `std::fs::remove_dir` locally and `SftpHandle::remove_dir` remotely for rollback. Existing recursive helpers are deliberately too strong (`crates/mt-project/src/fs.rs:953-970`, `crates/mt-ssh/src/sftp.rs:570-630`).
- A command failure with a partial non-empty clone is reported with the preserved target path. Destructive cleanup is not required by the PRD and cannot be proven safe with recursive deletion.
- No operation may fall back from SSH to local execution or to another connection.

### Failure and stale-result behavior

- Recommended error classes: `Validation`, `Collision`, `GitUnavailable`, `GitNonZero`, `Authentication`, `DisconnectedBeforeDispatch`, `RemoteOutcomeUncertain`, `PostconditionFailed`, `Persistence`.
- Operation exclusion must live above a transient page entity. Existing file operations capture project/root/backend/generation and only the owning context may clear busy state (`crates/mt-app/src/file_ops.rs:9-44`, `crates/mt-app/src/file_tree/ops.rs:19-73`). Apply the same pattern with modal instance, host generation, and operation ID.
- Existing remote-runtime requests demonstrate the required owner facts and completion checks: generation, path, connection ID, configuration fingerprint, and connection epoch (`crates/mt-app/src/store/remote_runtime.rs:25-74`, `crates/mt-app/src/store/remote_runtime.rs:243-335`).
- Closing or navigating away invalidates the form owner but must not let an old completion clear a newer form's busy/error state. If mutations may continue after close, keep job state in an app-level operation record keyed by `operation_id`; do not store the sole ownership record in the discarded page.
- Authentication/connection failure before dispatch registers nothing. Ambiguous SSH dispatch triggers post-state verification and registers only after the intended postcondition is proven.

### Likely affected files

Core operation boundary:

- `crates/mt-app/src/execution_host.rs`: expose/reuse pre-project structured local/WSL process planning and preserve mutation outcome state.
- `crates/mt-app/src/project_onboarding.rs` or a dedicated `host_project_ops.rs` (new): typed host context, pure probes, operation orchestration, stale-owner state, postcondition checks, and rollback.
- `crates/mt-app/src/remote_ssh/mod.rs` plus a possible `remote_ssh/project_ops.rs`: same-session SFTP preflight, bounded mutation exec, post-probe, epoch result, and exact-session retirement.
- `crates/mt-ssh/src/runtime.rs` or a new `mt-ssh` Git-inspection module: non-mutating `show-toplevel` + `git-common-dir` probe separate from runtime bootstrap.
- `crates/mt-project/src/git.rs` or a new focused module: strict Git-location data model/parser and local host operation plans; do not reuse the old non-killing timeout helper.

Persistence/opening:

- `crates/mt-app/src/store/projects.rs`, `crates/mt-app/src/store/ssh.rs`, `crates/mt-app/src/store/identity.rs`: one host-qualified dedupe/register outcome with explicit errors.
- `crates/mt-app/src/store/layout.rs` and `crates/mt-app/src/store/config_writer.rs`: only if onboarding requires a durable-save acknowledgement rather than current enqueue-and-log semantics.
- `crates/mt-config/src/config.rs`, `crates/mt-config/src/db.rs`, and `crates/mt-layout/src/lib.rs`: likely no schema change; verify compatibility, but preserve intentional alias support.

Integration surfaces identified by sibling UI research:

- `crates/mt-app/src/modal.rs`, `remote_project.rs`, `remote_directory_picker.rs`, `first_run.rs`, `project_list.rs`, `orca_sidebar.rs`, and `main.rs`.
- `crates/mt-i18n/locales/remoteProject.ts`, `projectList.ts`, `app.ts`, locale index/dictionary generation, and any new onboarding locale file.

### Related specs

- `.trellis/tasks/09-04-unified-add-project-flow/prd.md:22-78`: host inheritance, pure add, host-local clone/init, dedupe, state, and activation requirements.
- `.trellis/spec/mt-project/backend/worktree-identity-contract.md:56-107`: host-qualified identity, canonical paths, provisional/authoritative remote distinction, and fail-closed Git markers.
- `.trellis/spec/mt-ssh/backend/remote-runtime-contract.md:64-99`: authenticated host identity, same-session SFTP/exec, epochs, bounded output, and current-session checks.
- `.trellis/spec/mt-ssh/backend/error-handling.md:19-63`: preserve dispatch state and verify before fallback.
- `.trellis/spec/mt-app/backend/remote-runtime-reconciliation-contract.md:43-77`: request owner facts, epoch fencing, transactional authoritative binding, and bounded errors.
- `.trellis/spec/mt-app/backend/workbench-identity-contract.md:80-126`: `project_id` compatibility, WorktreeId ownership, and safe authoritative rebind timing.
- `.trellis/spec/mt-app/backend/worktree-context-contract.md:64-92`: generation and source-fact fencing for delayed results.
- `.trellis/spec/guides/cross-layer-thinking-guide.md:331-371`: operation context, execution-time recheck, staged verification, and ambiguous remote mutation rules.
- `.trellis/spec/mt-project/backend/worktree-catalog-contract.md:36-80`: structured Git command authority and refusal to infer destructive absence from failed/stale scans.

### External references

No web sources were needed. Relevant local dependency versions are `mt-ssh 0.6.5`, `russh 0.61`, `russh-sftp 2.3.0`, and Tokio 1 (`crates/mt-ssh/Cargo.toml:1-8`, `crates/mt-ssh/Cargo.toml:27-66`); local Git integration uses `git2 0.19` with vendored OpenSSL (`crates/mt-project/Cargo.toml:18-21`).

## Caveats / Not Found

- No production clone/init implementation or strict root-vs-nested Git probe exists in the inspected app/project/SSH layers.
- The remote runtime snapshot is authoritative but not pure because it may bootstrap remote state. A separate pure probe cannot itself claim authenticated `ExecutionHostId` unless the implementation also performs a non-mutating authenticated host-identity read or defers authoritative binding until registration/hydration.
- SFTP v3 cannot provide descriptor-relative `openat`/`O_NOFOLLOW`; same-account replacement races remain possible even with lstat and post-validation (`.trellis/spec/mt-ssh/backend/remote-runtime-contract.md:92-94`).
- Current config and layout persistence are separate databases and are not one cross-database transaction. The operation contract should expose partial persistence failure and retain recoverable filesystem state.
- Local/WSL behavior needs an implementation decision: preserve existing automatic WSL inference for UNC paths or explicitly exclude WSL from this MVP. The PRD names local Windows and SSH but does not add a WSL selector.
- No executable validation was run, per the task requirement that local formatting, compilation, Clippy, tests, and packaging remain GitHub Actions-only (`.trellis/tasks/09-04-unified-add-project-flow/prd.md:13-18`, `.trellis/tasks/09-04-unified-add-project-flow/prd.md:87-99`).
- No product code, specs, task manifests, or git state were modified by this research.
