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
pub enum ProjectPlacement<'a> {
    TopLevel { target_group: Option<&'a str> },
    ChildWorktree { root_project_id: &'a str },
}

pub fn register_or_activate_project(
    &mut self,
    location: ProjectLocationKey,
    canonical_path: &str,
    suggested_name: Option<&str>,
    target_group: Option<&str>,
    cx: &mut Context<AppStore>,
) -> Result<ProjectRegistrationOutcome, String>;

pub fn register_or_activate_project_with_placement(
    &mut self,
    location: ProjectLocationKey,
    canonical_path: &str,
    suggested_name: Option<&str>,
    placement: ProjectPlacement<'_>,
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
- Local Git-root classification may use normalized path equality as a fast
  path, but Windows must not treat lexical inequality as proof of nesting.
  After both directories are canonicalized, compare their opened directory
  identity by volume serial number plus file index when long-name, 8.3, or
  equivalent path aliases still differ. Equal identity means the selected
  directory is the exact repository root. Failure to read either identity is
  a `GitFailure`; it must not silently become `NestedInRepository` or
  `NotGit`.
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
- Existing onboarding calls the top-level wrapper. Automatic worktree
  discovery calls `register_or_activate_project_with_placement` with
  `ProjectPlacement::ChildWorktree` and the catalog's exact root project ID.
- Child placement requires an existing top-level root and matching execution
  host class. SSH children use the same saved connection ID; WSL children use
  the same distribution; Local, WSL, and SSH paths cannot cross categories.
- Location validation and canonical host/path dedupe run before insertion. If
  an existing top-level alias already owns the location, activate it without
  silently reparenting user configuration.
- A newly inserted child sets `parent_project_id`, never enters `projectTree`,
  prepares its normal worktree identity, persists through the same transaction,
  and returns the exact `ProjectId + WorktreeId` used for reactivation.
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
| Windows canonical paths differ only by long/8.3 alias | Compare directory file identity; equal identity is exact root; identity read failure is Git failure |
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
| Child root is missing or is itself a child | Registration error; no insertion |
| Local/WSL/SSH child host does not match the root | Registration error; no insertion |
| SSH child uses another connection or WSL child another distribution | Registration error; no insertion |
| Child location matches an existing top-level alias | Activate the alias; do not reparent it |
| Valid new child worktree | Set `parent_project_id`, keep it out of `projectTree`, and return exact project/worktree IDs |
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
- Good: classify `C:\Users\RunnerAdmin\repo` and the same directory reached
  through `C:\Users\RUNNER~1\repo` as one exact repository root when their
  filesystem identities match.
- Bad: classify every normalized-string mismatch on Windows as a nested root.
- Good: selecting an unconfigured SSH worktree registers one child under the
  root that owns the same connection and activates the returned worktree ID.
- Bad: insert a discovered child with `add_project_at`, infer a missing parent,
  or reparent an existing top-level project solely because paths match.

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
- Child registration tests must cover Local, WSL, and SSH success, missing or
  non-top-level roots, host/connection/distribution mismatch, repeated
  selection, canonical aliases, existing top-level alias preservation, and
  absence from `projectTree`.
- Windows focused tests must exercise the directory-identity fallback rather
  than only its lexical fast path, verify the Win32 information layout used by
  the FFI, keep distinct sibling directories unequal, and run real clone/new
  folder postconditions through exact-root classification.
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

Catalog-owned child registration uses the explicit placement:

```rust
let outcome = store.register_or_activate_project_with_placement(
    location,
    canonical_path,
    Some(suggested_name),
    ProjectPlacement::ChildWorktree { root_project_id },
    cx,
)?;
reactivate_active_page(&outcome.project_id, &outcome.worktree_id, window, cx);
```

## Scenario: Host-Aware Directory Browser

### 1. Scope / Trigger

Use the shared `remote_directory_picker` for every onboarding folder selection,
including native Local, Windows drive/UNC, WSL and authenticated SSH paths.
Browsing is a read-only precursor to the existing registration authority.

### 2. Signatures

```rust
struct DirectoryPickerOptions {
    host: ProjectHostSelection,
    initial_path: String,
    canonical_home: Option<String>,
    expected_connection_epoch: Option<u64>,
}

remote_directory_picker::open(
    options,
    is_current: impl Fn(&App) -> bool + 'static,
    on_select: impl Fn(String, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut Window, &mut App) + 'static,
    window,
    cx,
);

remote_ssh::browse_directory(&RemoteProjectContext, absolute_path)
    -> Result<RemoteDirectoryListing, RemoteDirectoryBrowseError>;
```

`DirectoryLocation` pairs a native/WSL/SSH `DirectorySource` with its host path.
SSH source includes connection ID, fingerprint and authenticated epoch. Returned
`RemoteDirectoryListing` carries canonical path, directories, producing epoch
and fingerprint; it is not an unqualified entries vector.

### 3. Contracts

- Retain separate current location, successful listing/selection, filter input
  and monotonically checked request identity. Typing filters loaded rows;
  explicit submission navigates. Home/up/breadcrumb/location changes issue a
  new owned request and clear the old selection until successful completion.
- Both listing publication and Select require the live modal, parent-form owner,
  latest request, exact source and current SSH epoch. Back, cancel, new request,
  page/mode/host change or starting an operation invalidates old callbacks.
  Counter overflow permanently exhausts that modal instance.
- Select captures `DirectorySelection` at render, including its request and
  selected location; it cannot reread a successor selection at delayed click.
  Go/row/location callbacks likewise retain the rendered request. Cancel/close
  is idempotent and cannot close a later picker instance with the same kind.
  Onboarding folder buttons capture parent form context before request allocation.
- Local uses native filesystem rules; WSL keeps case-sensitive POSIX locations
  and converts to its owning distribution's host-visible UNC only at the
  selection/native-I/O boundary. Drive/UNC roots cannot be normalized as SSH.
  WSL home lookup uses the structured execution-host command API and its
  [captured-directory contract](./git-host-contract.md#scenario-wsl-captured-directory).
  Pre-project and registered-project WSL commands share fixed-root launch plus
  Linux-side exact-directory entry; missing directories do not permit fallback.
  SSH uses only the home returned by its authenticated host probe, never the
  client home.
- WSL mapping validates the distro and each POSIX component before native I/O;
  do not Path::join unchecked text onto a Windows UNC root. Prefix injection,
  literal backslash/colon and components the existing registration boundary
  cannot faithfully preserve are rejected, not reinterpreted. Explicit parent
  traversal resolves through structured `pwd -P` on that captured distribution
  before Win32 normalization. Preserve strict Unicode and record terminators.
- `probe_connection` obtains SSH Home using `canonicalize(".")` on its actual
  acquired session with pre/post session guards. It does not read or populate
  the legacy connection-ID-only `home_cache`, which can contain an older
  authenticated session's path. SFTP initial cwd is not inferred from SetEnv HOME.
- The SSH browser validates the captured context before I/O and after completion
  using the same acquired session. Reconnect cannot lend a new epoch to an old
  listing. No retry may silently replace its source.
- List one level, including hidden folders and browsable directory symlinks;
  natural-sort results without applying project ignore rules. SSH listing has
  a 30-second deadline and 20,000-entry acceptance limit. Existing SFTP readdir
  materializes entries before this guard, so it is not a streaming memory cap.
- Keep one bounded list scroller, nonshrinking rows and a reserved scrollbar
  gutter. Paths and host remain inspectable, with shared icon tooltip timing.
- Select returns the host-visible path to `apply_picker_selection`; the original
  directory-only probe, operation ownership and central registration still run.
  Browsing itself never initializes Git, mutates files or persists projects.

### 4. Validation & Error Matrix

| Condition | Required behavior |
| --- | --- |
| Local PermissionDenied | Dedicated permission state |
| Proven missing/non-directory/invalid path | InvalidPath |
| Opaque SFTP error | Unavailable with bounded detail; no localized-text inference |
| Empty successful folder | Selectable current location, empty list |
| Pending/failed/superseded listing | No stale Select authority |
| Delayed Select/Cancel after picker replacement | Cannot select or close the new instance |
| New SSH epoch or endpoint fingerprint | Reject old result/selection |
| Poisoned connection-ID Home cache | Probe the current authenticated session directly |
| WSL prefix injection/unrepresentable POSIX name | Reject before native path inspection |
| Local/WSL/SSH source mismatch | Fail closed; never fall back to another filesystem |
| Entry limit exceeded or deadline elapsed | Unavailable, not a partial authoritative listing |

### 5. Good / Base / Bad

- Good: the selected SSH directory is re-probed on the original form/host before
  registration, even though its browser listing was successful.
- Base: selecting an ordinary non-Git folder works without Git installed.
- Bad: stamp a late directory result with the current epoch or register directly
  from the browser callback.

### 6. Tests Required

Actions exercises request replacement/ABA, parent-form invalidation, returned
source/epoch, overflow, hidden/empty/symlink directories, filtering versus path
submission, native drives/UNC and WSL conversion. Windows focused browser and
onboarding tests execute, not only compile. Exact Actions artifacts still need
native last-row wheel/thumb, long-path/high-DPI, keyboard, cancellation and SSH
reconnect acceptance. Authored fixtures and source review are not those results.
The actual authenticated browser fixture independently reads SFTP initial cwd,
poisons the legacy home cache, replaces the pooled session, and rejects stale
epochs. Geometry-helper coverage at 400px and above is not proof for shorter
viewports, native nested-dialog offsets, IME or actual scrollbar interaction.

### 7. Wrong vs Correct

Wrong: use a connection-only listing, then attach the current epoch at selection.

Correct: preserve the producing epoch/fingerprint through the listing, validate
the exact modal/form/request/source, then delegate to authoritative onboarding.
