# Source-Owned Git Host Contract

## 1. Scope / Trigger

Use `git_backend` for the native Git panel, Changes, History, Diff and Worktree
Management on Local, WSL and SSH projects. The domain's machine formats and
literal path rules remain in `mt-project/backend/git-cli-contract.md`. This
adapter owns captured execution authority, filesystem inspection, dispatch and
write reconciliation; GPUI still owns presentation and dialog generations.

## 2. Signatures

```rust
GitBackend::connect(ProjectExecutionSnapshot, GitLifetime) -> GitResult<GitBackend>;
GitBackend::discover(&self) -> GitResult<Vec<GitRepository>>;
GitBackend::repository_for_file(&self, &str) -> GitResult<(GitRepository, String)>;
GitRepository::request(&self, GitRead) -> GitResult<GitReadRequest>;
GitRepository::prepare_write(&self, GitWrite) -> GitResult<PreparedGitWrite>;
PreparedGitWrite::execute(self) -> GitWriteOutcome;
GitRepository::busy(&self) -> Option<GitBusy>;
GitRepository::review_uncertain(&self, u64, UncertainReview) -> GitResult<GitReconciliation>;
```

`GitWriteState` is `NotDispatched | Completed | Uncertain`. A write outcome
retains its operation ID, original repository/operation, error, optional
reconciliation, reconciliation error and `lease_retained`. Reconciliation
returns status, branches, worktrees and a typed `GitPostcondition`: `Refreshed`,
`WorktreeCreated`, `WorktreeRemoved` or `WorktreesPruned`.

## 3. Contracts

- All blocking adapter calls run on a background executor. Capture project,
  root project, worktree, execution host/backend, canonical path, fingerprint
  and epoch before work. A readiness read may establish an absent SSH epoch;
  publish using its returned snapshot. It must not retag a prepared write.
- Canonical project anchor, worktree root, Git directory and common directory
  must still describe the captured repository before and after reads. UI
  consumers additionally fence parameters, request IDs, worktree generations
  and dialog lifetimes. An A-to-B-to-A view is not the old request owner.
- Local authority/status/ref/history/tree/index/blob reads use libgit2 through
  `mt_project::git::local`. WSL/SSH use only their captured host. Never accept a
  same-spelling client path or local fallback for an unavailable remote source.
  Native Windows paths are not passed through the remote POSIX path parser.
- Pinned git2 0.19 status flags omit the unreadable bit and truncate unknown
  bits. Keep `include_unreadable(true)` and reject typed
  `StatusEntry::index_to_workdir().status() == Delta::Unreadable` before flag
  mapping or clean-entry skipping. Do not guess a raw bit, remove the rejection
  or treat truncated CURRENT/staged-only flags as proof of readability.
- Repository discovery checks the project and at most five ancestors before
  bounded descendant discovery. FileTree diff resolution is different: start
  at the literal file's parent and walk upward to the project's fifth ancestor,
  preferring the nearest nested repository and allowing missing file parents.
  Reject special/directory targets, symlink parents and escapes. Preserve leaf
  symlinks as literal link content and POSIX backslashes/newlines exactly.
- Capture `PreparedGitWrite` before confirmation, including selected paths,
  branch/base/target/Force and baseline HEAD/index/status/file facts. Consume it
  once. Revalidate at dispatch; a changed target must not widen the operation.
  Stage All is explicitly repo-wide. Untracked discard removes one leaf, not a
  directory or recursive clean. Native mutations also prove actual CLI
  authority matches libgit2, rejecting Git environment retargeting.
- Conflicting writes share a process-owned lease keyed by execution-host ID,
  backend class (and WSL distro) and canonical common directory. Do not include
  project/worktree/view, SSH configuration ID, credentials or epoch in that
  exclusion key. The slot separately retains the original source snapshot.
- Once dispatch is possible, worker ownership outlives view/dialog disposal.
  Failed/lost replies and failed reconciliation can leave an uncertain lease.
  No timer, reconnect, view switch, automatic replay or plausible inventory
  releases it. Explicit exact-ID review requires
  `UserConfirmedOriginalOperationStopped` and re-reads the original repository.
  It does not authorize replay, registration or configuration cleanup.
- A typed WSL process receipt with exit `-1` is launcher loss, not a completed
  Git command or proof that guest work stopped. Classify it as `Uncertain`
  even without timeout, preserving the receipt and original write lease after
  successful reconciliation. Native `-1`, SSH handling and ordinary positive
  WSL exits (including 255) keep their existing classification. Do not infer
  launcher loss from diagnostic text or broaden it to unproven negative codes.
- Reconciliation may establish a new read-only SSH epoch without rewriting the
  original write receipt. If the original worktree was removed, a proven
  survivor can transport recovery facts only; that handle cannot authorize
  ordinary discovery, reads or writes as the removed project.
- Only a normal successful outcome with the exact typed postcondition may
  authorize the next worktree UI step. Uncertain `WorktreeCreated/Removed`
  facts are not registration, PTY disposal or configuration-removal authority.
  UI/store must independently retain dirty-document, terminal and alias guards.
- Git keeps host hooks, signing, configuration and credentials. It never reads
  Tasks' selected account or adds that account's authentication environment.
- Capability errors remain explicit: WSL filesystem inspection needs Python
  3.10+, SSH literal symlink reads need `readlink -n --`. Removing the last
  non-bare linked worktree without a verified survivor is currently refused.
  Unsupported branch shorthand, directory/gitlink/special-node file actions
  and unsafe nested-repository Force removals must not silently retarget.
- Filesystem checks are not an atomic inode transaction; SFTP v3 has no openat
  ownership primitive. Native libgit2 inventories and SFTP readdir materialize
  before projection bounds; native filesystem calls are not preemptible. The
  write lease is process-local, not a persistent restart recovery journal.

## 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Stale source, epoch, anchor or repository | Reject; no replacement-source dispatch |
| Dialog invalidated before dispatch | NotDispatched; retain current UI ownership |
| Different project/SSH alias opens same common directory | Existing lease still blocks conflicting write |
| HEAD/index/selection changed after confirmation | Changed error; no implicit fresh confirmation |
| Write may have run, reply/cleanup uncertain | Quarantine and reconcile original source; no retry |
| Typed WSL receipt has exit -1 without timeout | Uncertain; successful reconciliation alone cannot release its exact write lease |
| Exact-ID explicit stopped-operation review | Re-read original authority; no cleanup inference |
| Deleted file inside a nested repository | Nearest existing repository, exact relative path |
| Remote path happens to exist locally | Ignore local state; only captured host is relevant |
| Native index differs only by case | Exact entry bytes, not core.ignorecase lookup alias |

## 5. Good / Base / Bad

- Good: close a Changes dialog after dispatch while its worker retains the
  repository lease and reconciles only the original source.
- Base: a failed readiness read leaves remote Git unavailable with last-known
  data, not an empty local repository.
- Bad: reconnect through another SSH configuration to bypass an uncertain
  operation, or register a worktree solely because a timed-out add later appears.

## 6. Tests Required

Actions must cover actual native libgit2 compatibility, exact index bytes,
commondir CRLF/ASCII suffixes, detached labels, nearest/deleted/nested file
resolution, no host-command native reads and fake-host path isolation. Exercise
production write leases across project/worktree/credential/SSH alias/epoch
changes. Real authenticated loopback exec/SFTP tests separately prove dispatch,
containment, epoch rejection and uncertain review; synthetic transports do not.
Exercise the production process classifier with native versus WSL `-1`, other
negative and ordinary positive exits, absent exits and timeout. Feed a WSL
launcher-loss receipt through the real write coordinator: successful original
source reconciliation retains the exact lease, conflicting writes stay blocked,
and only successful exact-ID stopped-operation review releases it without replay.

Full UI/store review and matching-SHA Windows/WSL/SSH/native artifact gates
remain required for confirmations, stale result publication, drafts, worktree
registration and exact terminal/document cleanup. All execution is Actions-only.
Authored tests and source review alone are not passing verification evidence.

## 7. Wrong vs Correct

Wrong: close the dialog, drop its busy flag, then retry a timed-out remote write
against whichever project and connection are now selected.

Correct: retain the original repository and common-directory operation lease,
reconcile its outcome independently of UI lifetime, and require exact-source
explicit review when the original effect cannot be established.

## Scenario: Worktree UI And Destructive Cleanup

### 1. Scope / Trigger

Use this boundary for every reachable Worktree Management action. Git filesystem
effects, central project registration and exact terminal/configuration cleanup
are separate authorities; no one successful read substitutes for all three.

### 2. Signatures

```text
git_worktree::open_repository(GitRepository, on_changed, window, cx)
AppStore::project_ids_for_location(&ProjectLocationKey) -> Vec<String>
AppStore::prepare_git_worktree_removal(&ProjectLocationKey, &App)
    -> Result<GitWorktreeRemovalGuard, String>
AppStore::git_worktree_removal_is_current(&GitWorktreeRemovalGuard, &App) -> bool
AppStore::close_git_worktree_terminals(
    GitWorktreeRemovalGuard, ProjectExecutionSnapshot, GitLifetime, cx)
    -> Task<Result<GitWorktreeRemovalGuard, String>>
AppStore::finish_git_worktree_removal(GitWorktreeRemovalGuard, cx)
    -> Result<(), String>
```

The cloneable guard is opaque. `matches_location` and `terminals_empty` expose
only the checks needed by the modal; UI cannot manufacture or advance authority.
Legacy `git_worktree::open` and public path/branch helpers remain available.

### 3. Contracts

- Retain the captured repository/source and modal instance through inventory,
  branch/base choice, picker, confirmation and completion. Initial SSH None
  epoch bootstrap is allowed only through `host_ui::read_source_matches`; an
  already pinned epoch cannot be replaced. Cached Loading is never restored.
- Failed/LastKnown inventory retains unavailable rows, not row-action authority.
  Fresh native Libgit2Fallback can remain readable without authorizing mutation.
  Existing/new branch and base choices are exact; an ambiguous shorthand does
  not select the first colliding local/remote branch.
- The shared picker captures the execution host and current form owner. Edits,
  mode changes, selection changes, refresh, write and close invalidate it.
  Remote suggestions and paths use host rules, never the client's native join.
- Create registers only exact normal WorktreeCreated postconditions on a live
  owner. Open/Add/Switch re-read exact inventory and use central
  `ProjectLocationKey` plus `ProjectPlacement::ChildWorktree`. No broad project
  lookup by path, local remote-path existence test or late active-root fallback.
- Remove captures both prepared Git intent and the store guard before showing
  confirmation. Force/Review Target is a new preflight, never a flag toggled on
  an old prepared request. The guard freezes every matching alias's config,
  binding, source, saved layout, exact terminal target and close request.
- Refuse dirty documents, changed/new/divergent aliases, missing saved runtime
  state, orphan routes, unrelated same-WorktreeId bindings and configured child
  dependencies. Do not hydrate a missing/dormant owner to inspect or close it.
- Reuse the exact existing terminal close path. Its bool means focus handoff,
  not success. After await, prove target absence and unchanged retained facts
  before advancing the guard. The close worker outlives modal disposal; a lost
  host reply cannot become successful cleanup.
- The detached close worker retains the exact confirmation lifetime and
  original repository snapshot. It finishes a dispatched close, but rechecks
  lifetime/source before each additional Kill. Cancelling a removal must not
  continue closing the rest of its terminals just because the worker is detached.
  Destructive alias selection additionally proves each trusted binding owns
  the target location; configured-path matching alone is not close authority.
  Capture `ProjectConfig::saved_layout` explicitly because normal config
  serialization omits that field.
- Close background records before the selected terminal to avoid neighbor
  restoration. An identical dormant alias may follow a proved exact logical
  session removal only after full route/incarnation checks; no second Kill and
  no adoption of unrelated concurrent alias changes. Revalidate each projection.
- Before Git dispatch, require the updated empty guard. Before configuration
  finalization, require the same live UI owner, normal verified Git removal and
  the still-current empty group. No await may divide final group validation and
  its bounded removal. Uncertain effects keep configuration, even if a later
  inventory looks removed. Explicit uncertainty review is not cleanup consent.
- Prune never closes terminals. It can finalize only backend-proven absent
  paths with already-empty captured guards. Hidden/invalid/offline presentation
  is not absence authority. Manual worktree visibility remains independent.
- Right-tool selection remains window-global; Git source/cache/branch/history
  and draft ownership remain worktree-specific. Retained user draft recovery
  after an epoch change is explicit and limited to matching repository identity
  and an empty current draft; it cannot revive old remote data or replay writes.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Dialog source/epoch/form changed before confirmation | Reject captured callback; do not retarget |
| Any dirty document or unexpected terminal/alias | Keep configuration; require fresh review |
| Close bool is false, exact terminal is absent | Judge captured postconditions, not the focus hint |
| Close returns but original terminal is retained/uncertain | No Git removal or configuration finalization |
| Source record removed but identical dormant alias remains | Prove exact route before bounded alias projection |
| Pruned path still has terminal records | No automatic closure or configuration deletion |
| User edits a commit/create draft while request runs | Late completion cannot clear the new revision |

### 5. Good / Base / Bad

- Good: close the captured background terminals, verify their unchanged alias
  group, remove the captured worktree and then finalize only the empty group.
- Base: an offline inventory shows retained unavailable rows without deletion.
- Bad: treat `close_terminal_target` returning false as failure/success itself,
  or run `dispose_project_terminals` on a project ID looked up after confirmation.

### 6. Tests Required

Actions must exercise host-qualified locations, raw remote paths, request/epoch
ABA, normal-vs-uncertain postconditions, fallback capability, exact guard changes,
selected-last ordering, dormant aliases and no-extra-hydration behavior. Source
tests of helpers do not establish actual GPUI timing, host Kill or native dirty
document behavior. Independent review and exact-SHA Windows/SSH/WSL/native
artifact acceptance remain required for the integrated workflow.

### 7. Wrong vs Correct

Wrong: remove remote worktree, then find projects with the same path and dispose
all their terminals/configuration from whichever UI context is now active.

Correct: capture host-qualified repository and complete cleanup guard before
confirmation, retain exact close ownership, verify normal Git postconditions,
and finalize only the still-current empty captured group.

## Scenario: WSL Captured Directory

### 1. Scope / Trigger

Noninteractive registered-project and pre-project WSL commands must enter the
captured Linux directory without depending on Windows WSL launcher path mapping.
The actual Actions marker matrix isolated that launch boundary; it did not
prove a deeper operating-system cause. A subsequent actual argv matrix isolated
empty Windows-launcher arguments on that runner: every nonempty comparison
passed, while the original list and its Empty row failed. Interactive PTY launch
is a separate contract and is not changed by this fix.

### 2. Signatures

`plan_host_command` and `plan_pre_project_local_command` retain their public
signatures and share private `plan_wsl_command(distro, cwd, &CommandPlan)`.
The resulting Windows argv contains no empty values:

```text
wsl.exe --distribution <distro> --cd / --exec /bin/sh -c <one-command-string>
```

Build that string only as `CDPATH= cd -P <quoted-cwd> && exec <quoted-argv>`.
Use the existing `posix_quote(cwd)` and `serialize_posix_argv(plan.display_argv())`;
never insert raw data or add a second handwritten quoting implementation.

### 3. Contracts

- Keep the source snapshot, distro identity and Windows `current_dir: None`.
  `/` is the launcher location only, never fallback project authority.
- Validate nonempty/NUL-free distro, absolute/NUL-free Linux cwd and existing
  program/argv constraints before dispatch. Reject a program beginning with
  `-` as `Rejected`; explicit `./-name` or `/path/-name` remains literal.
- `CDPATH=` and physical `cd -P` prevent inherited lookup/output or logical-PWD
  state from altering directory entry. Failed entry must stop before `exec`.
  Resolve PATH and relative executables only after successful captured `cd`.
- Encode original empty values as POSIX empty arguments inside the single
  nonempty command string. Preserve their positions and count, including
  consecutive/leading/trailing empties; never drop or replace them with spaces.
- Keep ordinary Null stdin, suspended/no-window creation, exact root attachment,
  bounded capture, deadlines and the backend-specific cleanup contract below.
  No user/default-distro,
  interop, automount, global environment or credential-policy change.
- Tasks privately builds only `exec <serialized-envelope-argv>` from the same
  fixed launch root. Its existing Python `os.chdir(cwd)` remains the sole cwd
  entry before all account operations. An outer shell cd would incorrectly
  map a missing cwd to HostHelperUnavailable; retain Account(CommandFailed).
  Never route credential capture through this ordinary public-result runner.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Empty/NUL distro or nonabsolute/NUL cwd | Planning `Io`; no dispatch |
| Program begins with an exec option prefix | Planning `Rejected`; require explicit path |
| Missing/non-directory Linux cwd | Nonzero command result; target command never runs |
| Literal quotes, whitespace, newline or metacharacters | Preserve exact argument bytes |
| Original empty/consecutive/leading/trailing arguments | Nonempty launcher fields; exact Linux argv cardinality |
| Relative executable | Resolve inside captured directory, not launcher root |
| WSL unavailable or directory entry fails | No Windows/local/root-directory fallback |

### 5. Good / Base / Bad

Good: encode a quoted-name worktree with the existing serializer, enter it in Linux,
then execute the original argv. Base: a deleted directory stays unavailable.
Bad: replace the snapshot path with `/` or retry its Git write from another cwd.

### 6. Tests Required

Actions planner tests assert registered/pre-project parity, unchanged source
identity, invalid-context and exec-option rejection, and literal paths/argv.
Assert a fixed nonempty Windows argument list and exact serialized empty values.
The actual owned WSL fixture must prove exact `pwd -P`, absolute/PATH/relative
executable behavior, byte-exact arguments and zero target dispatch for missing
and non-directory cwd. Independently prove the private Tasks envelope's captured
directory through fixture evidence and missing-directory rejection. Retain all
existing account secrecy, cancellation, descendant cleanup and marker guards.

### 7. Wrong vs Correct

Wrong: `--cd <project>` failure leads to executing the command from `/`.
Correct: always launch at `/`, then require exact Linux directory entry before
the original command, without changing captured request authority. Encode empty
arguments for the Linux shell instead of passing empty launcher fields or
silently deleting values that the original command requires.

## Scenario: WSL Client Containment

### 1. Scope / Trigger

Actions retirement-timing evidence for f1fde4d showed an overlapping public peer
fail after immediate short-command Job retirement, but complete normally when
the same boundary waited until peer return. This supports retirement interference
on the owned WSL1 fixture, not identification of a kernel process or WSL2 proof.
The production correction must use typed WSL ownership, never the diagnostic wait.

### 2. Signatures

Keep public execution/planner APIs unchanged. A crate-private strict-tree versus
WSL-client-root policy reaches ProcessTree configuration through the captured
ProjectExecutionSnapshot backend, PreProjectLocalContext, or private Tasks
run_wsl entry. The default configure path remains strict for existing callers.

### 3. Contracts

- Strict/native Windows Jobs retain only KILL_ON_JOB_CLOSE. Typed WSL client
  Jobs additionally allow SILENT_BREAKAWAY_OK so retiring one client does not
  adopt and terminate shared WSL instance descendants. Do not infer the policy
  from executable name, cwd, environment or cooperative-stdin mode.
- Preserve CREATE_SUSPENDED | CREATE_NO_WINDOW, exact root assignment before
  resume, fallible attachment cleanup, bounded direct-root reap, explicit
  TerminateJobObject and final owned-handle closure. The root remains managed.
  No CREATE_BREAKAWAY_FROM_JOB launch flag, production wait/retry, shared
  per-distro Job, distro shutdown, configuration change or new dependency.
- Silent breakaway applies to eligible descendants generally, not selectively
  to WSL infrastructure. Windows relay/interop descendants may escape the
  client Job; ancestor Job policy can further constrain inheritance. Do not
  promise full Windows descendant containment for this WSL-only path.
- Ordinary WSL execution has no positive Linux guest-stop acknowledgement.
  Timeout, missing reply, executor failure or the known launcher-loss receipt
  retains existing uncertain Git write ownership. Bounded caller return is not
  guest quiescence; no automatic replay or reconciliation-only lease release.
- Private Tasks keeps its separate host envelope, control channel, process-group
  cleanup and strict acknowledgement. Never route private bytes through this
  ordinary runner. Native, SSH and interactive PTY behavior is unchanged.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Typed registered/pre-project WSL command | WSL client-root policy; exact captured source |
| Native/default caller, even with a WSL-looking name | Existing strict-tree policy |
| WSL peer outlives short command completion or timeout | Peer completes normally; no production hold |
| Private WSL cancellation/timeout with concurrent peer | Strict acknowledgement and own descendant cleanup; peer survives |
| Assignment/resume/retirement failure | Preserve existing error and owned-root cleanup |
| Ordinary WSL guest lifetime not established | No guest-stop or safe-replay claim |

### 5. Good / Base / Bad

Good: retain the exact suspended WSL root while allowing shared descendants to
outlive that client. Base: launcher loss keeps a Git mutation uncertain. Bad:
relax all native Jobs or treat closing a WSL launcher as Linux cleanup proof.

### 6. Tests Required

Actions pins exact strict versus WSL Job flags and typed routing for both
ordinary entry points and private capture. Preserve native descendant, root
assignment/resume/failure and cleanup coverage. Keep every original actual WSL
account/lifecycle/descendant assertion unchanged; append bounded owned-fixture
concurrent-peer regressions for ordinary completion/timeout and private
cancel/timeout, without activating the diagnostic hold. Run the real Git lease
classification/review tests. WSL2, enabled interop, nested Jobs and real-device
native acceptance remain separate from the isolated WSL1 gate.

The appended fixture uses eight client-first rows after the untouched original
lifecycle loop. It requires own-marker readiness and pending workers before
retirement, then a still-pending peer and exact final public markers afterward.
Use twelve fresh no-reuse UUID directories; ordinary short calls have 5s
deadlines, fixed 2s/10s bodies, and each fixed 15s peer has a 20s deadline.
Private cancellation/timeout controls remain 15s/5s with strict stop replies
and the existing descendant postchecks. Sixteen readiness loops have absolute
10s dispatch budgets, 50ms intervals and 200-call caps; with forty fixed calls,
at most 3240 adapter calls are added. Captures remain bounded, bodies never log,
and unwind aborts pending private work before joining. Existing final-command
and cleanup grace still apply; OS scheduling is not a hard wall-clock promise.
Client-first ordering does not force cold instance startup after case setup.

### 7. Wrong vs Correct

Wrong: make a WSL cancellation appear successful by dropping cleanup evidence.
Correct: preserve the private host acknowledgement and exact write uncertainty
while changing only which descendants inherit the per-client Windows Job.
