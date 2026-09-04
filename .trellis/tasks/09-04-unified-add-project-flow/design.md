# Technical Design

## Overview

Implement one GPUI onboarding surface that owns the selected host, page, form drafts, validation, and operation ownership. Keep filesystem and Git work behind a host-scoped service that exists before project registration. Commit the resulting canonical host/path to `AppStore` only after the requested filesystem postcondition is verified.

The design follows Orca's stable dialog shell and host-token fencing while preserving mini-term's existing project/worktree identity, SSH pool, SFTP browser, overlay stack, persistence, and activation paths.

## Evidence

- `research/ui-onboarding.md` maps the existing GPUI entry points, modal patterns, picker, store, and activation flow.
- `research/host-operations.md` defines the pre-project host boundary, strict Git relationship probe, mutation uncertainty, dedupe, and rollback constraints.
- `research/validation.md` defines the test seams, acceptance-criteria map, SSH failure matrix, and Actions-only validation plan.
- `research/orca-reference.md` records the Orca interaction and transaction patterns being adapted.

## Module Boundaries

### `project_onboarding`

Add `crates/mt-app/src/project_onboarding/` with:

- `mod.rs`: public `open` API and modal entity lifecycle.
- `model.rs`: pure page, form, host, operation-owner, reducer, and readiness types.
- `view.rs`: GPUI rendering for modal chrome, host selector, home actions, clone form, create form, and inline status.
- `ops.rs`: host-neutral onboarding orchestration and injected adapter trait.
- `local.rs`: local/automatic-WSL filesystem and bounded Git command adapter.

The UI never calls Git, SFTP, persistence, or activation directly. It emits typed effects to the orchestration layer and applies results only through the reducer.

### `remote_ssh::project_ops`

Add a focused remote adapter inside `crates/mt-app/src/remote_ssh/` so it can reuse the existing process-level runtime, pool, exact cached session, SFTP handle, connection epoch, bounded exec states, and exact-session retirement.

It provides:

- selected-host probe/connect;
- canonical directory and target-state probes;
- strict repository relationship discovery;
- exclusive directory creation and empty-directory rollback;
- bounded clone/init execution with post-state verification.

It must not call remote runtime bootstrap or create `~/.mini-term` state during pure folder validation.

### Store Registration Boundary

Add one `AppStore::register_or_activate_project` boundary used by every onboarding path. It accepts a verified canonical location and returns:

```rust
enum ProjectRegistrationDisposition {
    RegisteredNew,
    ActivatedExisting,
}

struct ProjectRegistrationOutcome {
    project_id: String,
    worktree_id: WorktreeId,
    disposition: ProjectRegistrationDisposition,
}
```

The method owns host-qualified dedupe, group placement, config insertion, worktree identity registration, save scheduling, active-project selection, and the exact project/worktree pair used for focus restoration.

No database uniqueness migration is required. Intentional project aliases remain supported outside this onboarding path.

## UI State Model

```rust
enum OnboardingPage {
    Home,
    Clone,
    Create,
}

enum CreateMode {
    NewFolder,
    InitializeExisting,
}

enum ProjectHostSelection {
    Local,
    Ssh {
        connection: SshConnection,
        connection_fingerprint: u64,
    },
}

enum HostStatus {
    Ready { observed_epoch: Option<u64> },
    Connecting,
    NotConnected,
    Error(String),
}

enum OperationPhase {
    Idle,
    Validating,
    Running,
    Failure(OnboardingError),
}

struct OperationOwner {
    form_instance_id: u64,
    host_generation: u64,
    operation_id: u64,
    page: OnboardingPage,
    create_mode: Option<CreateMode>,
    host_signature: HostSignature,
}
```

IDs and generations use checked increments. Overflow enters a terminal form failure that remains visible and starts no further work until the modal is closed and reopened.

Opening the modal mints a new `form_instance_id`. Host switch, Back, Close, mode switch, and new submission invalidate prior ownership. Only an exact owner may apply a result or clear the active phase.

## Modal and Navigation

- Use one guarded overlay kind for all onboarding entry points.
- Keep a stable floating shell sized near the existing Orca-inspired modal, with Back on subpages and Close in the top-right.
- Home displays the Host row, a prominent Add Existing Folder action, then Clone From URL and Create New Project rows.
- The selected host remains visible on subpages as a compact read-only row. Back returns to Home and invalidates the current form generation.
- Local and remote folder browsing reuse the native picker and `remote_directory_picker`; a checked monotonic picker request ID plus parent-form owner facts ensure only the latest idle request may update inputs, and starting an operation supersedes every open picker.
- Entry points in the Orca sidebar, first-run screen, legacy project list, and group context all delegate to one `project_onboarding::open` function.
- A group-originated flow preserves `target_group`, but successful registration always activates and opens the resulting project.

## Host Selector

- Options are Local plus saved SSH connections, ordered through existing SSH grouping helpers.
- Local is always ready.
- SSH status is lazy. Do not connect every saved host when the menu opens.
- A saved SSH host without a current known session is shown as not connected and remains visible with a Connect action.
- Selecting or connecting an SSH host performs the existing authenticated `~`/SFTP probe and records the observed connection epoch in modal state.
- Add Remote Host opens the existing SSH editor as a nested overlay. On successful save, refresh the host list and select the saved connection when a callback can be added without duplicating the credential form.
- Host selection never falls back to Local or another SSH connection after an operation starts.

## Local and WSL Routing

The visible selector has one Local option. A chosen Windows UNC/WSL path is inspected by the local adapter and routed through `wsl.exe` using the existing structured argv convention. No separate WSL host row is introduced.

Native local paths use the bounded local process runner extracted from `execution_host.rs`. Both routes retain process-tree cleanup, wall-clock timeout, and bounded output.

## Path and Repository Probe

Return a strict typed probe:

```rust
struct HostPathProbe {
    canonical_path: String,
    directory_empty: Option<bool>,
    git: GitRelationship,
    observed_connection_epoch: Option<u64>,
}

enum GitRelationship {
    NotGit,
    RepositoryRoot { top_level: String, common_dir: String },
    NestedInRepository { top_level: String, common_dir: String },
}
```

For existing paths, canonicalize and verify directory type on the owning host. For new targets, canonicalize the parent and join one validated basename without following the absent leaf.

Git discovery uses structured equivalents of:

```text
git -C <path> rev-parse --path-format=absolute --show-toplevel --git-common-dir
```

A compatibility fallback is allowed only when the Git version proves that `--path-format` is unsupported. Parse exactly two bounded paths and canonicalize them on the same host. A present `.git` marker with failed discovery is an error, not `NotGit`.

## Operation Semantics

| Flow | Execution boundary | Success condition | Failure and cleanup |
|---|---|---|---|
| Add Existing Folder | Canonical existing directory probe only | Register the exact selected canonical folder, Git or non-Git | Never create, initialize, or delete anything |
| Clone From URL | Canonical parent, derived/editable folder name, execution-time collision recheck | `git clone -- <url> <target-name>` exits successfully and target probes as an exact Git root | Existing non-empty target is rejected; every failed or uncertain target is preserved because clone never proves exclusive ownership of the destination path |
| New Folder | Canonical parent and absent target rechecked at execution | Exclusive directory create, `git -C <target> init`, exact-root post-probe | Remove only the operation-created directory with non-recursive empty removal; preserve non-empty or uncertain state |
| Initialize Existing | Canonical existing directory and relationship probe | `NotGit` runs `git init` and verifies exact root; exact root skips mutation | Existing files are never removed; nested relationship returns containing root and performs no mutation |

Initialization does not create README/license files, choose a branch, create a remote, or create an initial commit.

## Clone Target Derivation

- Parse HTTPS, SSH URL, and SCP-like Git URL forms without treating user text as shell syntax.
- Infer a folder name from the final repository segment, strip one `.git` suffix, and validate it as one host-compatible basename.
- The inferred name remains editable.
- Render the complete final target path before submission.
- Redact URL userinfo and credentials from errors, logs, task state, and persisted config.

## SSH Mutation Safety

Remote mutation results retain the underlying dispatch state, output bounds, timeout, cleanup evidence, and observed epoch.

- A proven pre-dispatch rejection can return a normal retryable error after a fresh path probe.
- A command that may have started is never blindly retried.
- Every normal remote path probe, target check, create, command, post-probe, and cleanup call is pinned to the operation's captured authenticated epoch before remote state is inspected or mutated. A replacement epoch makes the call stale before mutation.
- For uncertain clone/init, retire only the exact uncertain session when required, then use one explicitly typed read-only recovery post-probe through the current host generation. Only that probe returns provenance authorizing the same operation owner to reconcile to a newer exact-current epoch after the connection fingerprint is revalidated. Exact-root recovery may register; a verified non-exact recovery carries the same authority on the uncertainty error so the modal can show the owned failure without registering. A failed recovery probe carries authority only when its exact failing session is still current and the selected connection fingerprint still matches; otherwise its error cannot reconcile the owner.
- Normal results never reconcile across epochs. A changed epoch is stale even when it is now the current session; the UI must consume explicit recovery provenance rather than infer permission from the mismatch.
- If state cannot be proven, preserve filesystem contents and show an uncertainty error containing the target path and recovery guidance.
- Stale owner, changed connection fingerprint, or changed epoch prevents UI/store mutation even if the command succeeded.

## Registration, Deduplication, and Activation

Define a canonical onboarding locator:

```rust
enum ProjectLocationKey {
    Local { normalized_canonical_path: String },
    Ssh { connection_id: String, normalized_posix_path: String },
}
```

Local comparison follows the existing platform path-normalization contract. SSH comparison is case-sensitive POSIX and includes the saved connection ID until authenticated worktree identity can prove a stronger equivalence.

Registration sequence:

1. Revalidate the operation owner and verified postcondition.
2. Find an existing project by `ProjectLocationKey`; if found, do not append config/tree state.
3. Otherwise stage the `ProjectConfig`, register identity, place it in the requested group, and schedule persistence.
4. Call the existing active-project path.
5. Close onboarding and call `reactivate_active_page` with the captured project/worktree pair.

Filesystem success is not rolled back because config/layout persistence later fails. Preserve the repository and expose a retryable registration error. The existing asynchronous writer remains the persistence mechanism; adding a cross-database durability transaction is deferred.

## Errors and Operation State

Use bounded typed errors: Validation, Collision, GitUnavailable, GitFailure, Authentication, DisconnectedBeforeDispatch, RemoteOutcomeUncertain, PostconditionFailed, and Registration.

- Inline field errors own validation copy.
- The primary button shows the running state and cannot be submitted twice.
- Errors include the selected host label and relevant safe path, but never credentials or raw unbounded output.
- GitHub credential failures may instruct the user to open a terminal on the selected host and run `gh auth login`; the app does not launch an authentication browser automatically.
- Cleanup errors append to the primary error instead of replacing it.

## Compatibility and Migration

- No persisted schema migration is planned.
- Existing local and remote projects continue to load unchanged.
- The old public local/remote onboarding functions become narrow delegates during migration, then are removed once all call sites use the unified surface.
- Keep the existing SSH editor, remote browser, project identity, workbench activation, and rollback shell behavior.
- Do not alter terminal, Files, Git, Tasks, or Agent-session behavior after project activation.

## Validation Design

- Pure reducer tests cover form ID, host generation, page/mode, fingerprint, epoch, duplicate submit, owner-only phase clearing, close/reopen, A-to-B-to-A switching, and overflow.
- Fake host-adapter tests cover exact call ordering, selected-host routing, collision rechecks, structured argv, every failure point, ambiguous SSH outcomes, and registration-last behavior.
- Store tests cover local/SSH canonical dedupe, group placement, one tree entry, identity registration, and activation.
- Temp-directory integration tests cover sentinel preservation, real local Git init/clone probes, nested detection, spaces/quotes/Unicode, and non-recursive rollback.
- Existing SSH pool tests remain the transport oracle; CI does not require a real SSH endpoint.
- All Rust formatting, compilation, Clippy, tests, i18n generation, Windows checks, and packaging run in GitHub Actions only.
- Add focused Windows Rust test execution for the new path/state modules in addition to the existing MSVC compile gate.

## Risks and Deferred Items

- No GPUI screenshot/E2E harness exists; visual acceptance requires review of the resulting Windows package.
- Config and layout persistence are not one acknowledged transaction. This task centralizes registration but does not redesign both databases.
- Same physical SSH host configured under two connection IDs remains distinct during onboarding until stronger authenticated identity is available.
- SFTP cannot provide descriptor-relative race-free filesystem operations; execution-time rechecks and post-probes reduce but cannot eliminate same-account replacement races.
- Clone cancellation is not required for MVP. Close/Back invalidates UI ownership, but a possibly running remote mutation must still finish or reach timeout and be post-probed safely.
