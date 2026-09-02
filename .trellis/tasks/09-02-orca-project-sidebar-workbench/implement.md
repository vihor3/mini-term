# Implementation Plan

## 1. Sidebar Row Model And Entity

- Add `orca_sidebar.rs` with pure project/worktree row construction and focused tests.
- Render Search, Agents, Projects header, add-project/worktree actions, nested worktree rows, and Usage/Settings footer.
- Scan top-level local projects in background through the authoritative catalog with request generation fencing and configured-row fallback.
- Materialize an unregistered worktree through `add_project_at` before activation.

## 2. Workbench Preview Semantics

- Mark document tabs as preview/permanent.
- Replace a clean preview in place when another file opens.
- Promote on dirty state and preview-tab double-click.
- Preserve close/focus/dirty confirmation behavior and add focused pure/state tests.

## 3. Docked Context Sidebar

- Replace `DrawerPanel` with `ContextPanel` containing Files, Git, Tasks, Sessions.
- Compose a docked right region with fixed top tabs and persisted/resizable width.
- Reuse FileTree, GitPanel, and SessionPanel entities; add an honest Tasks placeholder.
- Keep Git/Session visibility lifecycle synchronized to selected context tab.

## 4. Agents Overlay Scaffold

- Wire sidebar events to `Workspace`.
- Add fixed non-modal overlay geometry and current live-status rows.
- Close through toggle, close button, outside click, and Escape without changing workbench route.
- Restore the previous pane focus when it still exists.

## 5. Legacy Reachability And Polish

- Keep add project/worktree, remote project, Usage, Settings, and low-frequency SSH/mobile management reachable.
- Remove legacy activity bar and left-side FileTree from default composition.
- Verify text truncation and stable sizing at normal and compact desktop widths.

## 6. Docker-Only Validation

Run from Docker only:

```bash
./scripts/docker-ci.sh fmt-check
./scripts/docker-ci.sh test -p mt-app orca_sidebar
./scripts/docker-ci.sh test -p mt-app workbench_area
./scripts/docker-ci.sh clippy -p mt-app --all-targets -- -D warnings
./scripts/docker-ci.sh check
```

If the repository's Docker UI harness can launch GPUI under Xvfb, capture normal and compact screenshots and inspect them. Otherwise document the limitation and at minimum run the native render/model tests in Docker.

After checks pass, build the Windows installer in Docker using the established cargo-xwin/NSIS path. Use a distinguishable artifact name so it cannot be confused with the unchanged `1.2.2` installer.

## High-Risk Files

- `crates/mt-app/src/main.rs`
- `crates/mt-app/src/orca_sidebar.rs`
- `crates/mt-app/src/workbench_area.rs`
- `crates/mt-app/src/file_tree/`
- `crates/mt-app/src/git_panel.rs`
- `crates/mt-app/src/session_panel.rs`

## Rollback Points

- Sidebar: restore composition of `ProjectList` and ActivityBar; retain all saved project data.
- Preview: remove preview metadata and return to permanent-only document tabs; dirty documents remain protected.
- Context sidebar: restore legacy floating Sessions/Git drawers and left FileTree.
- Agents: hide the compatibility overlay; no terminal/session state is owned by it.

## Completion Review

- Confirm the new shell is the default code path in the Windows artifact.
- Confirm no acceptance language claims terminal warm reattach, remote runtime identity, or live GitHub data.
- Confirm the host has no Rust toolchain, target directory, or project build cache after packaging.
