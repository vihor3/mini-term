# Project Onboarding Contract

## 1. Scope / Trigger

Use this contract for any UI or backend path that adds a local or SSH project,
clones a repository, creates a Git-backed folder, initializes an existing
folder, or activates an already registered host/path.

The public UI is one host-aware modal. Host-specific filesystem and Git work
must finish and prove its postcondition before project persistence begins.

## 2. Signatures

The operation layer is host-neutral:

```rust
trait ProjectHostOps {
    fn probe_existing_directory(
        &self,
        path: &str,
        include_empty: bool,
        inspect_git: bool,
    ) -> Result<HostPathProbe, OnboardingError>;
    fn probe_target(&self, parent: &str, name: &str)
        -> Result<TargetState, OnboardingError>;
    fn create_directory_exclusive(&self, target: &str)
        -> Result<(), OnboardingError>;
    fn remove_empty_directory(&self, target: &str)
        -> Result<(), OnboardingError>;
    fn run_git(&self, cwd: &str, plan: &CommandPlan)
        -> Result<HostCommandOutcome, OnboardingError>;
    fn probe_after_uncertain_dispatch(
        &self,
        path: &str,
        include_empty: bool,
        inspect_git: bool,
    ) -> Result<VerifiedUncertainPostcondition, OnboardingError>;
    fn location_key(&self, path: &str)
        -> Result<ProjectLocationKey, OnboardingError>;
}
```

Owned operation outcomes carry explicit authority rather than only an
observed epoch:

```rust
enum OperationResultProvenance {
    Normal,
    PostconditionVerifiedAfterUncertainDispatch,
}

struct OperationResultAuthority {
    observed_connection_epoch: Option<u64>,
    provenance: OperationResultProvenance,
}

struct OnboardingError {
    kind: OnboardingErrorKind,
    message: String,
    authority: Option<OperationResultAuthority>,
}
```

Registration is centralized:

```rust
fn register_or_activate_project(
    &mut self,
    location: ProjectLocationKey,
    canonical_path: &str,
    suggested_name: Option<&str>,
    target_group: Option<&str>,
    cx: &mut Context<AppStore>,
) -> Result<ProjectRegistrationOutcome, String>;
```

An async completion is owned by the exact modal, host generation, operation,
page/mode, host signature, and optional SSH connection epoch.

## 3. Contracts

- `Add Existing Folder` performs a read-only directory probe. It never creates
  files and never runs `git init`.
- Directory-only validation passes `inspect_git = false`. This is required for
  Add Existing and clone/create parent folders so ordinary project onboarding
  still works when Git is unavailable. Initialization classification and exact
  repository postconditions pass `inspect_git = true`.
- `Clone From URL` executes `git clone` on the selected host and registers only
  after the target probes as an exact Git repository root.
- Clone never deletes a failed target. A preflight absence check does not prove
  exclusive ownership of the destination path after the command starts.
- `New Folder` requires an absent target, creates one directory exclusively,
  runs `git init`, and verifies the same target as an exact repository root.
- `Initialize Existing Folder` runs `git init` only for `NotGit`. An exact Git
  root is added directly. A nested folder returns its containing root and does
  not create a nested `.git` directory.
- Clone URL validity and destination-basename validity are independent. A valid
  repository URL whose inferred final segment is not a portable basename must
  remain usable after the user supplies a valid editable destination name; the
  inferred-name failure alone must not mark the URL invalid.
- Local commands keep structured program/argv values. SSH is the only boundary
  that serializes argv into a quoted POSIX command.
- SSH results are usable only when connection ID, configuration fingerprint,
  and authenticated connection epoch still match the selected host. The host
  readiness probe returns both canonical home and the exact session epoch; the
  UI may enter `Ready` only while that returned epoch is still current.
- Every normal SSH probe, target check, create, Git exec, and cleanup call is
  pinned to the operation's originally captured authenticated epoch before it
  may inspect or mutate remote state. Acquiring a newer session makes that call
  stale and must fail before mutation.
- A changed epoch on a normal result is always stale, even when that epoch is
  now current. Epoch mismatch alone never grants reconciliation authority.
- The only epoch-reconciliation exception is a read-only repository
  postcondition probe invoked explicitly after
  `HostCommandDispatch::OutcomeUncertain`. That probe may use the fresh current
  authenticated session and must return
  `PostconditionVerifiedAfterUncertainDispatch` provenance. The UI may advance
  the same operation owner only when the saved connection fingerprint still
  matches and that returned epoch is the exact current, newer epoch. An exact
  repository postcondition can then register; a verified non-exact
  postcondition carries the same authority on the uncertainty error so the
  modal can display that owned failure after epoch reconciliation without ever
  registering the project. A failed recovery probe may carry the same
  authority only when the exact failing session remains current after the
  failure and the selected connection fingerprint still matches; otherwise the
  error has no reconciliation authority.
- `ProjectLocationKey::Local` uses normalized canonical local identity.
  `ProjectLocationKey::Ssh` uses exact saved connection ID plus normalized,
  case-sensitive absolute POSIX path. Dedupe may bridge a configured alias to a
  binding canonical path only for authoritative local sources or an
  authoritative SSH binding whose endpoint/configured-path identity context
  still matches. Provisional Local/WSL/SSH bindings must match the canonical
  form recomputed from the current configured path.
- Registration either activates the existing location or inserts one project,
  prepares its worktree identity, places it in the requested group, activates
  it, and returns the exact project/worktree pair used for workbench focus.
- If filesystem/Git work succeeds but registration fails, retain only the
  verified canonical path under the exact form context. A retry performs a new
  read-only directory probe and retries registration; it must never rerun the
  completed clone or `git init` mutation.
- Closing, going Back, changing host, or changing create mode invalidates the
  current owner. A late result may not change UI state, persist a project, or
  reactivate a workbench page.
- Folder-picker callbacks also require the latest checked picker request ID and
  an idle matching form context. Starting an operation supersedes open pickers.
- Any identity-counter overflow is terminal for that modal instance; navigation
  cannot clear it or permit more work. The user must close and reopen the modal.
- The unified modal is the only compiled onboarding surface. Do not retain the
  obsolete local folder dialog, remote-project dialog, or their raw insertion
  helpers after all entry points have migrated.

## 4. Validation & Error Matrix

| Condition | Required result |
|---|---|
| Empty/relative/invalid host path | Validation error; no mutation |
| Git unavailable during directory-only validation | Canonical directory validation still succeeds |
| Existing non-empty clone target | Collision error; no Git dispatch |
| Existing target in New Folder mode | Collision error; no mkdir/init |
| Git probe explicitly reports `not a git repository` | `NotGit` |
| Git marker exists but discovery fails | Git failure; fail closed |
| SSH failure proven before dispatch | Disconnected-before-dispatch error |
| SSH command may have started or reply is lost | Outcome uncertain; fresh post-probe only |
| SSH host probe returns an epoch that is no longer current | Host remains disconnected; do not accept its home path |
| Normal SSH result returns a different epoch | Stale result; no reconciliation or registration |
| Verified uncertain-dispatch post-probe returns the exact fresh current epoch | Reconcile that same owner to the newer epoch, then recheck before registration |
| Any failed clone | Preserve the target if present and report its path; never delete it |
| Failed init in operation-owned empty directory | Empty-directory-only cleanup is allowed |
| Failed init in a pre-existing directory | Preserve all user files and directory state |
| Duplicate canonical host/path | Activate existing project; do not insert another |
| Target group disappeared | Registration error; do not silently place elsewhere |
| Registration retry after successful clone/init | Re-probe read-only and retry registration; never repeat the mutation |
| Owner/fingerprint/epoch mismatch | Ignore completion as stale |

Authentication guidance may tell the user to run `gh auth login` on the owning
host. Project onboarding must not launch a browser or attempt account login.

## 5. Good / Base / Bad Cases

- Good: clone over SSH, lose the exec reply, retire the exact uncertain session,
  probe with a current authenticated session, and register only if the target is
  now an exact repository root.
- Base: add a non-Git local folder after canonical read-only validation.
- Bad: catch an SSH error, retry locally, recursively delete the target, or
  register from the original unchecked path.
- Good: select a nested folder for initialization, show the containing Git root,
  and let the user add that root.
- Bad: treat every exit code 128 as `NotGit` and run `git init` after dubious
  ownership, permission, or corrupt-repository errors.

## 6. Tests Required

- Reducer tests must cover A-to-B-to-A host switching, page/mode invalidation,
  duplicate submit, close/reopen, persistent generation overflow, picker supersession, and epoch mismatch.
- Operation tests must assert call order and prove that Add Existing has no
  mutation, collisions stop before dispatch, exact-root post-probes gate
  registration, and cleanup is empty-directory-only.
- Pure owner tests must reject a normal result whose epoch changed and accept a
  newer epoch only when the result carries verified uncertain-postcondition
  provenance, the fingerprint is unchanged, and the observed epoch is current.
- Remote adapter tests must prove normal probe/create/exec/cleanup paths reject
  a replacement epoch before inspection or mutation, while only the explicit
  read-only uncertain-dispatch post-probe can acquire the fresh session.
- Operation tests must assert that Add Existing and parent probes disable Git
  inspection while initialization and repository post-probes enable it.
- URL tests must separately assert URL syntax and inferred-name validity, and
  prove that a manually supplied valid destination name enables clone when the
  URL itself is valid but its inferred segment is reserved or otherwise invalid.
- URL tests must cover HTTPS, SSH/scp forms, editable folder-name inference,
  structured argv, bounded diagnostics, and credential redaction.
- Store tests must cover local-vs-SSH separation, SSH connection identity,
  POSIX case sensitivity, normalization, invalid paths, duplicate activation,
  authoritative canonical aliases, and rejection of stale provisional bindings.
- Local integration tests must preserve sentinel files across Add Existing and
  Initialize Existing, verify real clone/init postconditions, and prove nested
  folders do not acquire their own `.git`.
- Registration-retry tests must prove a verified path is scoped to the exact form
  context and produces `AddExisting`/read-only work instead of a second mutation.
- SSH host-probe tests must reject a result whose captured epoch is not the exact
  current connection epoch.
- Store transaction tests that use fixture SSH connections must not start the
  production remote-runtime hydration path or a headless GPUI application. Use
  the context-free registration state seam or an injected runtime fake so
  results never depend on UI task shutdown, DNS, or socket timeout behavior.
- Windows GitHub Actions must compile the affected packages and run focused
  onboarding and SSH project-operation tests. Linux Actions remains the full
  workspace check, Clippy, test, rustfmt, i18n, and whitespace gate.
- Installer completion requires the Windows packaging workflow and its payload
  verification; local Cargo, formatting, tests, generation, Docker, and
  packaging are not evidence for this repository.

## 7. Wrong vs Correct

### Wrong

```rust
let result = remote_git_clone(connection, url, path).await;
store.add_remote_project(path); // unchecked path, stale host, uncertain result
```

### Correct

```rust
let owner = state.begin_validation()?.ok_or(DuplicateSubmit)?;
let result = selected_host.clone_from_url(&request);
let verified = state.apply_owned_result(&owner, result)?;
let outcome = store.register_or_activate_project(
    verified.key,
    &verified.canonical_path,
    Some(&verified.suggested_name),
    target_group.as_deref(),
    cx,
)?;
reactivate_active_page(&outcome.project_id, &outcome.worktree_id, window, cx);
```

The concrete reducer method names may differ, but the ownership check,
postcondition proof, central registration, and exact activation order may not.
