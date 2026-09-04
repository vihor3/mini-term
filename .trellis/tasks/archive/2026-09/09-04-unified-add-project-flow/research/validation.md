# Research: Validation strategy and robustness constraints

- Query: Validate the unified local/SSH add-project flow, emphasizing modal ownership, project persistence, host routing, async fencing, Git mutation safety, and GitHub Actions-only verification.
- Scope: internal
- Date: 2026-09-04

## Findings

### Required Safety Properties

The PRD makes these behaviors contractual:

- One modal owns local and saved-SSH onboarding; host switches must invalidate host-dependent readiness and stale callbacks (`.trellis/tasks/09-04-unified-add-project-flow/prd.md:22`, `.trellis/tasks/09-04-unified-add-project-flow/prd.md:29`).
- Add Existing is a read-only operation on the selected folder and must activate, rather than duplicate, an existing host/path (`.trellis/tasks/09-04-unified-add-project-flow/prd.md:31`, `.trellis/tasks/09-04-unified-add-project-flow/prd.md:38`).
- Clone executes on the selected host and persistence occurs only after success (`.trellis/tasks/09-04-unified-add-project-flow/prd.md:40`, `.trellis/tasks/09-04-unified-add-project-flow/prd.md:45`, `.trellis/tasks/09-04-unified-add-project-flow/prd.md:47`).
- New Folder must reject any existing target. If `git init` fails, cleanup is limited to the directory this operation created and only while it is empty (`.trellis/tasks/09-04-unified-add-project-flow/prd.md:55`, `.trellis/tasks/09-04-unified-add-project-flow/prd.md:60`, `.trellis/tasks/09-04-unified-add-project-flow/prd.md:61`).
- Initialize Existing must skip an existing repository root and block a directory nested in another worktree (`.trellis/tasks/09-04-unified-add-project-flow/prd.md:63`, `.trellis/tasks/09-04-unified-add-project-flow/prd.md:67`, `.trellis/tasks/09-04-unified-add-project-flow/prd.md:69`).
- Only the owning operation may update the form after asynchronous work. Failures must not register a partial project (`.trellis/tasks/09-04-unified-add-project-flow/prd.md:71`, `.trellis/tasks/09-04-unified-add-project-flow/prd.md:74`, `.trellis/tasks/09-04-unified-add-project-flow/prd.md:75`).
- Executable validation belongs exclusively in GitHub Actions (`.trellis/tasks/09-04-unified-add-project-flow/prd.md:18`, `.trellis/tasks/09-04-unified-add-project-flow/prd.md:99`).

The app quality contract reinforces the ownership rule: snapshot source identity and a token before background work, validate both before every UI mutation, and allow only the owner to clear shared busy state (`.trellis/spec/mt-app/backend/quality-guidelines.md:19`, `.trellis/spec/mt-app/backend/quality-guidelines.md:42`, `.trellis/spec/mt-app/backend/quality-guidelines.md:44`).

### Existing Coverage and Confirmed Gaps

| Area | Confirmed evidence | Validation consequence |
|---|---|---|
| Modal lifecycle | Dialog builders run every frame, so form state must live in an entity (`crates/mt-app/src/modal.rs:15`). The local native picker applies its late result directly to the input (`crates/mt-app/src/modal.rs:236`, `crates/mt-app/src/modal.rs:248`). | The unified form needs an explicit form/request owner around local picker results; entity lifetime alone does not prove that the same host, page, or form generation still owns the callback. |
| Overlay guard | One overlay kind cannot be pushed twice, and stack behavior has pure tests (`crates/mt-app/src/overlay.rs:153`, `crates/mt-app/src/overlay.rs:231`). Programmatic close only acts on the top overlay (`crates/mt-app/src/prompt.rs:88`). | Reuse one `ADD_PROJECT` kind. Remove or make unreachable the old `ADD_REMOTE_PROJECT` entry path so compilation, not reviewer memory, enforces AC1. |
| Current remote form | It snapshots connection/path/name/group before validation (`crates/mt-app/src/remote_project.rs:172`, `crates/mt-app/src/remote_project.rs:205`) and registers only after successful validation (`crates/mt-app/src/remote_project.rs:242`, `crates/mt-app/src/remote_project.rs:256`). On snapshot mismatch it still clears `busy` (`crates/mt-app/src/remote_project.rs:229`, `crates/mt-app/src/remote_project.rs:234`). | Value equality is not a sufficient owner token for a navigable unified modal. A stale completion must be a complete no-op, including not clearing state owned by a newer request. There are no direct tests in this file. |
| Remote picker | The picker has a request ID and connection-ID fence (`crates/mt-app/src/remote_directory_picker.rs:19`, `crates/mt-app/src/remote_directory_picker.rs:37`, `crates/mt-app/src/remote_directory_picker.rs:51`). The parent callback writes the chosen path without rechecking the parent form host (`crates/mt-app/src/remote_project.rs:587`, `crates/mt-app/src/remote_project.rs:591`). | Preserve picker-local fencing, but also capture and validate the parent form ID, host signature, and generation before applying the selected path. Replace `wrapping_add` with checked generation allocation for safety-critical ownership. |
| Entry points | First run has separate local and remote buttons (`crates/mt-app/src/first_run.rs:77`, `crates/mt-app/src/first_run.rs:105`, `crates/mt-app/src/first_run.rs:126`). Orca and the project-list footer also route to separate modal functions (`crates/mt-app/src/orca_sidebar.rs:796`, `crates/mt-app/src/orca_sidebar.rs:829`, `crates/mt-app/src/project_list.rs:2374`, `crates/mt-app/src/project_list.rs:2383`). | Route every entry through one exported open function. Existing first-run tests cover only hotkey hints (`crates/mt-app/src/first_run.rs:164`), not onboarding routing. |
| Local dedup | `find_project_by_path` uses shared normalized comparison (`crates/mt-app/src/store/projects.rs:105`). `add_project_at` uses it (`crates/mt-app/src/store/projects.rs:126`), but `add_project` uses raw string equality (`crates/mt-app/src/store/projects.rs:188`, `crates/mt-app/src/store/projects.rs:191`). | Introduce one locator comparator and one register-or-activate path. Tests must cover case/separator/trailing-slash equivalence according to platform rules. |
| Remote dedup | Remote registration explicitly bypasses local path dedup and always creates a new ID (`crates/mt-app/src/store/ssh.rs:201`, `crates/mt-app/src/store/ssh.rs:206`, `crates/mt-app/src/store/ssh.rs:218`). | AC9 requires a new remote comparator, at minimum stable connection ID plus canonical absolute POSIX path. Tests must pin whether two saved connections to the same physical host are distinct until authenticated authority proves equivalence. |
| Identity | Local resolution canonicalizes the directory and distinguishes Git from non-Git (`crates/mt-project/src/worktree/identity.rs:79`, `crates/mt-project/src/worktree/identity.rs:86`, `crates/mt-project/src/worktree/identity.rs:108`). Provisional SSH identity uses connection ID plus normalized absolute POSIX path (`crates/mt-project/src/worktree/identity.rs:182`). | Registration should consume a host-owned canonical path, not raw form text. Existing tests already cover stable local identity and provisional SSH normalization (`crates/mt-project/src/worktree/identity.rs:318`, `crates/mt-project/src/worktree/identity.rs:443`). |
| Nested repository detection | Remote runtime discovers only the Git common directory with `git rev-parse --git-common-dir` (`crates/mt-ssh/src/runtime.rs:446`, `crates/mt-ssh/src/runtime.rs:453`). No production `Repository::discover` or `git rev-parse --show-toplevel` implementation was found. | Add one host-side repository relationship probe that returns `NonGit`, `RepositoryRoot`, or `NestedInRepository { root }`. AC6 and AC7 cannot be inferred from the existing common-dir probe alone. |
| Pre-persistence execution | Host command routing currently requires a configured `ProjectExecutionSnapshot` (`crates/mt-app/src/execution_host.rs:99`, `crates/mt-app/src/store/identity.rs:277`, `crates/mt-app/src/store/identity.rs:285`). | Clone/init must not create a temporary project merely to obtain execution routing. Add a pre-persistence host snapshot containing backend, selected-host identity, cwd/path facts, SSH fingerprint, and observed epoch. |
| Structured commands | Local/WSL retain structured argv; SSH is the only serialization boundary (`crates/mt-app/src/execution_host.rs:1`, `crates/mt-app/src/execution_host.rs:157`). Tests cover structured local/WSL plans, hostile SSH text, and NUL rejection (`crates/mt-app/src/execution_host.rs:901`, `crates/mt-app/src/execution_host.rs:951`, `crates/mt-app/src/execution_host.rs:996`). | Build clone/init/probe as structured `CommandPlan` values. Never compose user URL, directory name, or path into a local shell command. |
| Local process bounds | The shared execution layer owns process-tree setup, timeout, descendant cleanup, and bounded output (`crates/mt-app/src/execution_host.rs:665`, `crates/mt-app/src/execution_host.rs:681`, `crates/mt-app/src/execution_host.rs:752`). | Reuse or extract this runner. The older Git runner returns on receiver timeout while its worker may continue running Git and explicitly lacks guaranteed termination (`crates/mt-project/src/git.rs:1124`, `crates/mt-project/src/git.rs:1164`, `crates/mt-project/src/git.rs:1175`). It is unsuitable for onboarding mutations. |
| SSH uncertainty | SSH results retain dispatch state and expose `safe_to_fallback` plus exact-session retirement (`crates/mt-ssh/src/pool.rs:735`, `crates/mt-ssh/src/pool.rs:756`, `crates/mt-ssh/src/pool.rs:762`). Tests cover the fallback matrix and forbid state downgrade after execution evidence (`crates/mt-ssh/src/pool.rs:1710`, `crates/mt-ssh/src/pool.rs:1747`). | Clone/init adapters must preserve this state. A timeout after possible dispatch is not proof of failure and must not trigger a blind retry or destructive cleanup. |
| Async owner pattern | Remote runtime captures generation, path, connection ID, and fingerprint (`crates/mt-app/src/store/remote_runtime.rs:36`, `crates/mt-app/src/store/remote_runtime.rs:259`), accepts completion only if every fact still matches (`crates/mt-app/src/store/remote_runtime.rs:295`, `crates/mt-app/src/store/remote_runtime.rs:322`), and requires exact SSH epoch equality (`crates/mt-app/src/store/remote_runtime.rs:326`). Tests independently mutate every fact and test overflow (`crates/mt-app/src/store/remote_runtime.rs:410`, `crates/mt-app/src/store/remote_runtime.rs:457`). | This is the model to copy for modal validation and operation completion. Use checked monotonic generation, immutable owner facts, exact epoch matching, and fail-closed overflow. |
| Fake runner pattern | Worktree catalog separates `GitRunner` from orchestration (`crates/mt-project/src/worktree/catalog.rs:31`), uses a scripted `FakeRunner` (`crates/mt-project/src/worktree/catalog.rs:913`), and tests mutation fencing of an in-flight scan (`crates/mt-project/src/worktree/catalog.rs:1216`). GitHub tasks similarly inject a command executor closure (`crates/mt-app/src/github_tasks.rs:232`, `crates/mt-app/src/github_tasks.rs:240`). | Use an injected onboarding host-operations interface so ordering, host routing, failures, epochs, and side effects can be tested without real SSH. |
| Persistence durability | Config rows preserve project order and deletion semantics (`crates/mt-config/src/db.rs:564`, `crates/mt-config/src/db.rs:589`). The config writer snapshots on enqueue, coalesces to the newest complete config, and has an end-to-end persistence test (`crates/mt-app/src/store/config_writer.rs:20`, `crates/mt-app/src/store/config_writer.rs:310`, `crates/mt-app/src/store/config_writer.rs:380`). | Add a focused store test for register-or-activate, then rely on existing DB/writer tests for the persistence mechanism. Do not duplicate DB serialization logic in the modal. |
| Localization | Source locale keys live in paired `projectList` and `remoteProject` dictionaries (`crates/mt-i18n/locales/projectList.ts:1`, `crates/mt-i18n/locales/remoteProject.ts:1`). Consistency tests require identical key sets, placeholders, and non-empty messages (`crates/mt-i18n/tests/consistency.rs:154`, `crates/mt-i18n/tests/consistency.rs:187`, `crates/mt-i18n/tests/consistency.rs:208`, `crates/mt-i18n/tests/consistency.rs:230`). | New labels, status copy, and errors must be added to both languages at the TS source, followed by generated dictionary validation in Actions. |

### Recommended Test Seams

#### 1. Pure modal operation reducer

Define a pure state machine with:

- `FormInstanceId` minted each time the modal opens.
- Checked monotonic `generation`; overflow enters a visible failure state and starts no work.
- `HostSignature`: local install/execution-host identity, or SSH connection ID + configuration fingerprint + captured/current connection epoch.
- `OperationOwner`: form ID, generation, host signature, page/mode, canonical input snapshot, and expected phase.
- Explicit phases: `Idle`, `Validating`, `Running`, `Success`, `Failure`.

Only an exact owner may apply validation/operation output or clear `busy`. Host switch, Back, Close, mode switch, and a new submission supersede the generation. Dropping a task is cancellation assistance; generation comparison remains authoritative.

Required reducer tests:

- Current validation success/failure applies.
- A host A result after switching to B is ignored.
- A host A result after A -> B -> A is still ignored because generation differs.
- Close/reopen with identical values rejects the prior form ID.
- Old success and old failure cannot clear a newer request's busy/error state.
- Page or New Folder/Initialize Existing mode changes invalidate prior readiness.
- Duplicate submit while validating/running emits no second effect.
- Generation overflow remains terminal for that modal instance until close/reopen.

#### 2. Pre-persistence host-operations interface

Inject a small interface instead of calling filesystem, Git, or SSH directly from render code. The fake must record host, cwd, argv, timeout, and call order and return scripted path/command/SSH-state outcomes.

Recommended operations:

- `probe_path(host, path) -> PathProbe`
- `probe_repository(host, canonical_path) -> RepositoryRelationship`
- `create_directory_exclusive(host, target)`
- `run_command(host, cwd, CommandPlan) -> HostCommandOutcome`
- `remove_empty_directory_if_owned(host, target, ownership_token)`

`HostCommandOutcome` must retain SSH `BoundedExecState`, timeout/truncation flags, exit status, observed epoch, and bounded diagnostics. Do not collapse an uncertain mutation into a plain disconnected string before orchestration decides whether a state probe is required.

#### 3. Pure operation planner/orchestrator

Test exact effects and registration boundaries:

- **Add Existing:** canonical directory probe, then register-or-activate. Assert zero create/remove/Git calls for Git and non-Git folders.
- **Clone:** revalidate parent and target immediately before execution; reject an existing non-empty target; run structured `git clone` on the selected host; verify final target/repository; register only after verified success.
- **New Folder:** require target absent; exclusive single-directory create; `git init` in that exact target; verify repository root; register. On init failure, attempt only empty-directory removal after this operation successfully performed the exclusive create.
- **Initialize Existing:** probe repository relationship. Non-Git runs `git init`; exact root skips init; nested path returns the containing root and emits no mutation; register only after final verification.

Use table-driven failures at every step and assert the persisted project count and active project are unchanged until the final commit step.

#### 4. Canonical register-or-activate helper

One helper should own dedup, insertion, group placement, activation, identity registration, and config save scheduling. Test:

- Equivalent local spellings resolve to one project according to `normalize_path_for_comparison` (`crates/mt-project/src/worktree/mod.rs:70`).
- POSIX remains case-sensitive and retains legal backslashes; Windows ignores case and separator differences (`crates/mt-project/src/worktree/mod.rs:72`, `crates/mt-project/src/worktree/mod.rs:98`).
- Same SSH connection plus equivalent canonical POSIX path activates the existing project.
- Different SSH connections remain distinct unless the design explicitly supplies equal authenticated execution-host/worktree identity.
- Duplicate selection does not append a tree node or enqueue a second project record.
- New insertion uses the existing activation/hydration path (`crates/mt-app/src/store/projects.rs:86`).

#### 5. CI filesystem/Git integration tests

Use temp directories on Actions runners for local adapters:

- Add Existing preserves a sentinel file byte-for-byte and creates no `.git`.
- New Folder creates an absent path and produces a repository root.
- Initialize Existing preserves all pre-existing sentinel files and produces `.git`.
- Existing root skips the init command, verified through the fake and through unchanged sentinel files.
- Nested child detection returns the outer root and does not create `child/.git`.
- Init failure cleanup removes only an empty directory created by the operation; a directory containing any unexpected or partial entry is preserved and the cleanup result is appended to the primary error.
- Names/paths with spaces, leading dashes, quotes, Unicode, and platform separators remain single argv/path values.

Real SSH infrastructure is not required for CI. Use fake host operations plus the existing SSH state tests to validate remote orchestration deterministically.

### SSH Mutation Failure Matrix

The SSH contract states that only `safe_to_fallback()` authorizes an immediate alternate path; possible dispatch requires verification and forbids blind retry (`.trellis/spec/mt-ssh/backend/error-handling.md:30`, `.trellis/spec/mt-ssh/backend/error-handling.md:44`, `.trellis/spec/mt-ssh/backend/error-handling.md:74`).

| Command outcome | Required onboarding behavior |
|---|---|
| `NotDispatched`, `ExecEnqueueTimedOut`, or explicit `Rejected`, with cleanup confirmed | No registration. Re-probe target before offering retry because filesystem state may also have changed independently. |
| `ChannelOpenUnknown` | Retire only the exact session, re-probe target, and report uncertainty if state cannot be established. Do not issue a second mutation automatically. |
| `ExecReplyUnknown` or `Started` with timeout/lost reply | Treat the mutation as possibly executed. Probe target and Git state; never infer failure from transport failure. |
| Nonzero exit with complete output | No registration. Preserve partial clone/init output. New Folder may remove only its exact owned directory and only if empty. |
| Exit zero but output truncated, SSH epoch changed, or owner became stale | Do not apply the completion. Verify through the current host generation before any registration. |
| Verified successful final state | Register-or-activate exactly once, then use the existing project activation path. |

Clone failure has no PRD authorization for recursive cleanup. Never use `remove_dir_all`/remote tree deletion on a failed or uncertain clone. New Folder rollback can use local `remove_dir` or SFTP empty-directory removal; the SFTP layer already exposes single-directory creation and empty-directory removal (`crates/mt-ssh/src/sftp.rs:554`, `crates/mt-ssh/src/sftp.rs:570`).

### Acceptance-Criteria Test Map

| AC | Minimum evidence |
|---|---|
| AC1 | All entry points compile against one unified open function; old remote onboarding open function/kind is private or removed. |
| AC2 | Local integration + remote fake tests prove Add Existing emits no mutating effects and accepts Git/non-Git directories. |
| AC3 | Planner tests assert final target display data, selected-host routing, structured clone argv, verification, and persistence only after success. |
| AC4 | Local integration and remote fake tests assert absent-target create -> init -> verify -> register ordering. |
| AC5 | Sentinel-file integration test plus fake call-order test for non-Git existing directories. |
| AC6 | Repository-root probe returns Add Project and emits no `git init`. |
| AC7 | Nested child probe returns containing root, emits no mutation, and the add-root action uses that canonical root. |
| AC8 | Pure owner-fact tests cover host, form ID, generation, mode, path, fingerprint, and SSH epoch independently. |
| AC9 | Store helper tests cover local and SSH canonical locators and assert one persisted/tree record. |
| AC10 | Table-driven failure at every adapter step, plus SSH uncertainty matrix, asserts zero registration and actionable bounded error. |
| AC11 | Ubuntu full suite, Windows compile plus focused Windows Rust tests, and Windows package workflow all conclude successfully. |

### CI-Only Validation Plan

No Rust build, formatting, Clippy, test, staging, packaging, or Docker validation command should run on the workstation. This is also required by the release-staging contract (`.trellis/spec/mt-app/backend/release-staging-contract.md:34`, `.trellis/spec/mt-app/backend/release-staging-contract.md:37`).

Existing `.github/workflows/ci.yml` provides:

- Pull-request, push, and manual dispatch entry points (`.github/workflows/ci.yml:3`).
- Changed-line rustfmt (`.github/workflows/ci.yml:94`).
- Generated i18n verification (`.github/workflows/ci.yml:118`).
- Locked root/sidecar metadata (`.github/workflows/ci.yml:126`).
- Root/sidecar checks, affected-package Clippy, full workspace tests, and whitespace validation (`.github/workflows/ci.yml:131`, `.github/workflows/ci.yml:137`, `.github/workflows/ci.yml:156`, `.github/workflows/ci.yml:162`).
- Windows MSVC checks for all affected root packages and sidecars (`.github/workflows/ci.yml:165`, `.github/workflows/ci.yml:192`, `.github/workflows/ci.yml:209`).

Confirmed gap: the Windows job compiles but does not execute Rust tests. Add focused Windows steps for the packages containing the new pure and filesystem tests, for example:

```powershell
cargo test --locked --target x86_64-pc-windows-msvc -p mt-project --lib --no-fail-fast
cargo test --locked --target x86_64-pc-windows-msvc -p mt-ssh --lib --no-fail-fast
cargo test --locked --target x86_64-pc-windows-msvc -p mt-app --bin mini-term --no-fail-fast
```

The Windows package workflow triggers for `crates/**` pushes and supports manual dispatch (`.github/workflows/windows-package.yml:3`, `.github/workflows/windows-package.yml:24`). It validates locked graphs, stages sidecars, builds the Windows GPUI app, verifies the staged payload, builds/extracts the installer, and uploads verified artifacts (`.github/workflows/windows-package.yml:68`, `.github/workflows/windows-package.yml:108`, `.github/workflows/windows-package.yml:116`, `.github/workflows/windows-package.yml:119`, `.github/workflows/windows-package.yml:122`, `.github/workflows/windows-package.yml:129`, `.github/workflows/windows-package.yml:142`). This is the required Windows packaging gate.

CI dispatch/observation commands, run only to start or inspect GitHub Actions:

```bash
gh workflow run ci.yml --ref <branch>
gh workflow run windows-package.yml --ref <branch>
gh run list --workflow ci.yml --branch <branch> --limit 1
gh run list --workflow windows-package.yml --branch <branch> --limit 1
gh run watch <run-id> --exit-status
```

Do not use `.github/workflows/release.yml` for feature-branch acceptance: it is tag-only (`.github/workflows/release.yml:24`).

### Risk Checklist

- [ ] Every current local/remote entry point opens the same modal and old remote onboarding cannot be reached.
- [ ] Form state is entity-owned and every async callback carries form ID, generation, host signature, and expected phase.
- [ ] Only the exact operation owner clears busy/error or changes phase.
- [ ] Host, page, mode, Back, Close, and reopen transitions invalidate prior results.
- [ ] Generation and request ID allocation fail closed rather than wrap, and overflow remains terminal until close/reopen.
- [ ] Local and remote picker results require the latest picker request ID, matching parent form owner, and idle phase before writing inputs.
- [ ] Readiness checks are repeated immediately before mutation; validation is not treated as a lock on the filesystem.
- [ ] Add Existing performs no Git/create/remove operation.
- [ ] Repository probing distinguishes root from nested path and rejects malformed `.git` state.
- [ ] Clone/init commands use structured argv and the selected host/cwd.
- [ ] User URL/name/path values cannot become shell syntax.
- [ ] Local timeout terminates the full process tree and bounds stdout/stderr.
- [ ] SSH command state, fingerprint, and epoch survive through orchestration; ambiguous mutations are probed, not retried blindly.
- [ ] Registration is a final commit after verified filesystem/Git success.
- [ ] Local and SSH dedup use one canonical locator policy; duplicate activation appends no persistence/tree record.
- [ ] New Folder cleanup requires a successful exclusive create plus an empty-directory proof immediately before non-recursive removal.
- [ ] Failed or uncertain clone targets are always preserved and are never deleted by onboarding.
- [ ] Cleanup failure augments rather than replaces the primary error, matching the project error contract (`.trellis/spec/mt-app/backend/quality-guidelines.md:136`, `.trellis/spec/mt-ssh/backend/error-handling.md:75`).
- [ ] Errors and persisted state exclude credentials, private-key material, raw unbounded command output, and shell prompts.
- [ ] Success routes through existing identity registration, list refresh, hydration, and activation.
- [ ] New i18n keys exist in both languages and generated `dict.rs` is clean in Actions.
- [ ] Ubuntu CI, focused Windows tests, Windows MSVC check, and Windows package workflow are all green before acceptance.

### External References and Versions

No external web sources were required. Repository-pinned interfaces relevant to validation are:

- Rust `1.95`, GPUI `0.2.2`, and `gpui-component` `0.5.1` (`Cargo.toml:19`, `Cargo.toml:40`, `Cargo.toml:43`).
- `git2` `0.19` with vendored OpenSSL (`crates/mt-project/Cargo.toml:18`).
- `mt-ssh` `0.6.5`, `russh` `0.61`, and `russh-sftp` `2.3.0` (`crates/mt-ssh/Cargo.toml:1`, `crates/mt-ssh/Cargo.toml:33`, `crates/mt-ssh/Cargo.toml:63`).
- Git CLI itself is not pinned; Actions integration evidence should record `git --version` if failures prove version-sensitive.

### Related Specs

- Async ownership and testing: `.trellis/spec/mt-app/backend/quality-guidelines.md:19`.
- Workbench deferred-callback identity: `.trellis/spec/mt-app/backend/workbench-identity-contract.md:99`.
- Remote generation/fingerprint/epoch fencing: `.trellis/spec/mt-app/backend/remote-runtime-reconciliation-contract.md:43`.
- Local/provisional/authoritative host-path identity: `.trellis/spec/mt-project/backend/worktree-identity-contract.md:56`.
- Structured Git execution and mutation generation fencing: `.trellis/spec/mt-project/backend/worktree-catalog-contract.md:36`.
- SSH dispatch uncertainty and exact-session eviction: `.trellis/spec/mt-ssh/backend/error-handling.md:19`.
- CI-only executable validation: `.trellis/spec/mt-app/backend/release-staging-contract.md:32`.

## Caveats / Not Found

- No direct tests were found in `modal.rs`, `remote_project.rs`, or `remote_directory_picker.rs`; current coverage is concentrated in pure overlay, identity, runtime, command, and persistence modules.
- No GPUI modal end-to-end, screenshot, Playwright, or desktop UI automation harness was found. Keep the modal thin and put behavioral guarantees in pure reducer/planner tests; visual acceptance remains review evidence unless a dedicated GPUI harness is introduced.
- No production implementation of `git clone`, `git init`, `Repository::discover`, or `git rev-parse --show-toplevel` was found. These are new adapter/probe surfaces and require focused tests.
- Current remote project persistence has no host/path dedup, and current local add paths use inconsistent comparators.
- Current command routing is project-scoped and cannot safely run onboarding mutation before persistence without a new host snapshot seam.
- No real SSH endpoint is configured in repository CI. Remote success/failure ordering must be covered with fakes; transport semantics remain covered by existing `mt-ssh` unit tests.
- Clone into an existing empty target is not stated explicitly. R3 rejects an existing non-empty target, while New Folder rejects every collision. The design and tests should pin the clone-empty-directory behavior rather than inherit Git-version-specific behavior accidentally.
