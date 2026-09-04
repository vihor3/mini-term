# Implementation Plan

## 1. Establish Testable Domain Types

- Add pure onboarding page, host, form, operation-owner, readiness, error, path-probe, Git-relationship, and registration-outcome types.
- Add checked generation/form-ID helpers that fail closed on overflow.
- Add reducer tests for host/page/mode transitions, stale completion rejection, duplicate submit, owner-only phase clearing, close/reopen, and A-to-B-to-A switching.
- Add pure clone URL name derivation and cross-platform target-path preview tests.

## 2. Add Pre-Project Host Operations

- Extract/reuse the bounded local process runner from `execution_host.rs` without weakening existing project-scoped command behavior.
- Add Local/automatic-WSL path resolution and structured command planning.
- Add a strict local directory and Git relationship probe.
- Add `remote_ssh::project_ops` using the exact pooled session, SFTP canonicalization/type checks, bounded exec state, connection fingerprint/epoch, and exact-session retirement.
- Add fake adapters and table-driven orchestration tests before wiring UI.

## 3. Implement Operation Orchestration

- Add Existing Folder: pure canonical directory probe followed by registration.
- Clone From URL: derive/edit target name, recheck parent/target, execute on selected host, verify exact Git root, preserve every failed or uncertain target, then register.
- New Folder: validate basename, exclusive create, `git init`, exact-root verification, and non-recursive empty rollback only for the owned directory.
- Initialize Existing: classify non-Git/root/nested, mutate only non-Git, skip root initialization, and return containing root for nested folders.
- Preserve SSH uncertain-dispatch evidence and post-probe before accepting success or enabling retry.
- Add credential redaction and bounded actionable error formatting.

## 4. Centralize Project Registration

- Add host-qualified canonical `ProjectLocationKey` lookup.
- Replace raw-string local duplicate checks and always-new remote onboarding insertion with one register-or-activate path.
- Preserve intentional alias support outside onboarding and avoid a database uniqueness migration.
- Return the exact project/worktree identity and created/existing disposition.
- Preserve group placement, project-tree ordering, config save scheduling, active-project hydration, and workbench focus restoration.

## 5. Build the Unified GPUI Modal

- Add one modal entity with stable chrome, Home/Clone/Create pages, Back/Close, and host identity on every page.
- Add the compact host selector using existing SSH grouping/summary visuals and lazy Connect state.
- Reuse native local folder selection and the remote directory picker with parent-owner plus latest-request fencing; operation start supersedes every open picker.
- Add Home actions: Add Existing Folder, Clone From URL, Create New Project.
- Add Clone fields: URL, parent folder, editable inferred folder name, final target preview, progress/error state, and dynamic submit readiness.
- Add Create segmented control: New Folder and Initialize Existing Folder, with mode-specific fields, repository classification, containing-root action, and dynamic action labels.
- Keep stable dimensions, constrained long paths, vector icons, keyboard focus, and no persistent overlay banner.

## 6. Migrate Entry Points and SSH Handoff

- Route Orca sidebar, first-run, legacy root, and group-context add actions through the unified open API.
- Retire or reduce the old local and remote onboarding functions to delegates until no caller remains.
- Use one overlay kind for onboarding; preserve nested remote picker and SSH editor stacking.
- Add an SSH editor completion callback that refreshes/selects the newly saved host without duplicating credential UI.

## 7. Localization and Visual Assets

- Add paired Chinese/English onboarding locale keys at the TypeScript source dictionaries.
- Use or extend the existing vector icon system for Folder, Clone, Git, Host, Back, Close, Chevron, and status affordances.
- Let GitHub Actions regenerate and verify `crates/mt-i18n/src/dict.rs`; do not generate it locally.

## 8. Focused Verification

Add or extend tests for:

- modal reducer and stale-result ownership;
- local/SSH path locator dedupe;
- exact-root vs nested Git classification;
- no-mutation Add Existing flow;
- clone target collision and partial-state preservation;
- New Folder ownership rollback;
- Existing Folder sentinel preservation;
- structured argv and URL credential redaction;
- SSH uncertainty matrix and epoch/fingerprint fencing;
- one registration/tree entry and exact activation handoff;
- all entry points using the unified modal;
- paired localization keys.

## 9. GitHub Actions Gates

After coherent implementation commits are pushed, require:

- changed-line rustfmt and generated i18n checks;
- locked root/sidecar metadata;
- full root/sidecar check, Clippy, and tests;
- focused Windows Rust tests for new onboarding/path logic;
- Windows MSVC affected-package checks;
- Windows installer build, extraction, payload verification, and artifact upload;
- final whitespace check.

Do not run Cargo, rustfmt, Clippy, tests, i18n generation, staging, Docker, or packaging locally. Local verification is limited to code inspection, Git diff/status, and non-executable whitespace checks.

## 10. Completion Review

- Run a Trellis implementation review against every acceptance criterion and the risk checklist in `research/validation.md`.
- Update an existing spec or add a focused onboarding contract for reusable host-operation, dedupe, and stale-owner rules learned during implementation.
- Record GitHub Actions run IDs, Windows artifact metadata, residual visual risk, and any deferred persistence/cancellation issue in task validation.
- Commit only current-task product/spec/task changes and preserve unrelated bootstrap/settings/journal dirt.
- Archive the task after the final Actions evidence and user-visible behavior are complete.

## Expected File Areas

- `crates/mt-app/src/project_onboarding/**` (new)
- `crates/mt-app/src/remote_ssh/project_ops.rs` (new)
- `crates/mt-app/src/execution_host.rs`
- `crates/mt-app/src/store/{projects,ssh,identity}.rs`
- `crates/mt-app/src/{main,orca_sidebar,first_run,project_list,modal,remote_project,remote_directory_picker,overlay,ssh_panel,ui}.rs`
- `crates/mt-project/src/**` only for shared pure/local Git-path logic where the existing crate boundary fits
- `crates/mt-ssh/src/**` only if exact-session primitives cannot remain inside the app facade
- `crates/mt-ui/src/icons/**`
- `crates/mt-i18n/locales/**`
- `.github/workflows/ci.yml`

## Rollback Points

- Keep old onboarding public functions as delegates until every entry point compiles against the new surface.
- Land host operations and store registration behind pure tests before enabling the new UI.
- Do not remove old overlay kinds or modules until the unified path is wired and Actions passes.
- Filesystem mutations are never rolled back recursively; preserved paths are reported for manual recovery.
- A failed implementation batch can be reverted without changing persisted schemas or existing project records.
