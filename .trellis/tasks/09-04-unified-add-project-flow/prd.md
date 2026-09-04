# Unified Local and Remote Add Project Flow

## Goal

Replace the separate local and SSH project-onboarding surfaces with one Orca-style floating modal. A user selects the execution host once, then can add an existing folder without mutation, clone a repository, create a new Git-backed folder, or initialize an existing folder as a Git repository.

## User Value

- Project onboarding uses the same predictable interaction model on local Windows and saved SSH hosts.
- Destructive or state-changing Git actions are explicit instead of being hidden behind folder selection.
- The selected host, target path, and final operation remain visible before execution, reducing accidental work on the wrong machine or directory.

## Background

- The current UI has separate local and remote project-add flows.
- Local folder selection, remote SSH connection management, remote folder browsing, project persistence, and project activation already exist, but are split across different surfaces.
- The requested visual direction follows Orca's compact floating modal: a host selector at the top, primary onboarding actions on the first page, and focused subpages with Back and Close controls.
- Local compilation, Rust formatting, Clippy, tests, packaging, and CI validation must not run on the workstation. Executable validation is performed by GitHub Actions.

## Requirements

### R1. Unified Modal and Host Context

- Replace the separate local/remote add-project entry points with one floating modal.
- Keep a host selector visible at the top-level entry page.
- The host selector must list the local machine, saved SSH hosts, connection state, and an entry for adding a remote host.
- The selected host must be inherited by every onboarding subflow.
- Subpages must keep enough host identity visible that the user can verify where an operation will run.
- Switching hosts must revalidate host-dependent paths and operation readiness. Stale asynchronous results from the previous host must not update the current form.

### R2. Add Existing Folder

- Provide an `Add Existing Folder` action on the first page.
- Local hosts use the native local folder chooser.
- SSH hosts use the remote directory browser.
- Adding an existing folder must never create files, run `git init`, or otherwise mutate the selected folder.
- Git and non-Git folders are both valid projects.
- If the selected host/path already exists in project persistence, activate the existing project instead of creating a duplicate.

### R3. Clone From URL

- Provide a `Clone From URL` action on the first page.
- The subpage must collect a Git URL and destination parent directory.
- Infer an editable destination folder name from the URL and show the final target path before execution.
- Run `git clone` on the selected host so that local and remote credentials remain owned by that host.
- Reject an existing non-empty target directory without overwriting it.
- Allow an existing empty target directory, but treat it as user-owned and never remove it if cloning fails.
- Preserve every failed or uncertain clone target. The clone command does not provide exclusive destination ownership, so onboarding never deletes its target automatically.
- On success, persist and activate the cloned project.
- On failure, show an actionable error and do not register a project.

### R4. Create New Project

- Provide a `Create New Project` action on the first page.
- The subpage must use a segmented mode control with `New Folder` and `Initialize Existing Folder` modes.

#### R4.1 New Folder Mode

- Collect a project name and parent directory.
- Show the final target path before execution.
- Create the directory, initialize it with `git init`, then persist and activate the project.
- Reject collisions instead of silently reusing or overwriting an existing directory.
- If initialization fails after this operation created the directory, remove only that newly created directory and only when it remains empty.

#### R4.2 Initialize Existing Folder Mode

- Allow selecting an existing directory on the chosen host.
- If the directory is not a Git repository, run `git init`, then persist and activate it.
- If the directory is already a Git repository root, skip initialization and present the action as `Add Project`.
- Never delete, overwrite, or modify pre-existing user files beyond the explicit Git initialization.
- If the directory is nested inside another Git worktree, block nested initialization by default, show the detected repository root, and offer to add that root instead.

### R5. Operation State and Safety

- Each subflow must expose idle, validating, running, success, and failure states without duplicate submission.
- Closing or navigating away from a running form must not allow its eventual callback to mutate a different host/form instance.
- Connection loss and SSH authentication failures must be reported in the modal without registering a partial project.
- Host/path identity must be canonical enough to deduplicate local and remote projects consistently with existing project persistence contracts.
- Successful operations must refresh the project list and open the resulting project using the existing activation path.

### R6. Visual and Interaction Direction

- Use the established Orca-inspired dark floating-modal treatment already adopted by the application.
- Preserve stable modal dimensions while moving between the entry page and subpages.
- Use icons for folder, clone/network, Git repository, Back, Close, and host affordances through the existing icon system.
- Primary action labels must reflect the operation, including `Clone`, `Create and Initialize`, `Initialize and Add`, and `Add Project`.
- Disabled states must explain readiness through field-level validation or status copy, not through an always-on warning banner over content.

## Acceptance Criteria

- [ ] AC1: One add-project modal can target either the local machine or any saved SSH host without opening the old separate remote-project modal.
- [ ] AC2: Adding an existing local or remote folder registers and opens it without changing its filesystem contents.
- [ ] AC3: Cloning on either host shows the final destination, executes on that host, and only registers the project after success.
- [ ] AC4: New Folder mode creates a previously absent directory, initializes Git, registers it, and opens it.
- [ ] AC5: Initialize Existing Folder mode initializes a non-Git directory without changing existing user files, then registers and opens it.
- [ ] AC6: Selecting an existing Git repository root skips `git init` and adds the project directly.
- [ ] AC7: Selecting a directory nested inside another Git worktree does not create a nested `.git`; the UI identifies and can add the containing repository root.
- [ ] AC8: Switching hosts or forms cannot apply stale validation/operation results to the current form.
- [ ] AC9: Duplicate host/path selections activate the existing project and do not add another persisted record.
- [ ] AC10: Failed clone, folder creation, Git initialization, authentication, or connection operations show an actionable error and do not register a partial project.
- [ ] AC11: Relevant unit/integration coverage and the Windows GitHub Actions workflows pass; no executable validation is performed locally.

## Out of Scope

- Account login UI for Git providers; host-side Git credentials remain authoritative.
- Automatically running `gh auth login`; authentication guidance remains user-driven.
- Arbitrary nested Git repositories or submodule creation.
- Repository templates, README/license generation, initial commits, branch naming configuration, or remote creation.
- Editing saved SSH host credentials inside the add-project form beyond opening the existing add-host flow.
- Changing project/worktree terminal, file-tab, Git panel, task panel, or agent-session semantics after the project is opened.

## Product Decisions

- Folder browsing is a pure add operation and never implicitly initializes Git.
- Git mutation is limited to explicit Clone or Create New Project flows.
- Existing Git roots are added directly instead of being reinitialized.
- Nested initialization is blocked by default and redirects toward the detected repository root.
- Local and remote flows share one information architecture and differ only in host-specific path picking and execution.
