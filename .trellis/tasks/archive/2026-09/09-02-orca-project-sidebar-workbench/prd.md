# Orca Project Shell Visible Slice

## Goal

Replace the legacy mini-term shell with the already approved Orca-aligned visible experience so a Windows build unmistakably shows `Project -> Worktree`, an active-worktree workbench, a docked context sidebar, and the global Agents overlay entry.

This child is the first visible delivery from the parent architecture. It consumes the completed authoritative local worktree catalog and existing project-scoped terminal/layout compatibility model without claiming that detached PTY warm reattach, remote runtime identity, or GitHub task fetching is complete.

## Requirements

1. The left sidebar contains only global Search and Agents actions, a `Projects` section, project rows with nested worktree rows, and a footer containing Usage and Settings.
2. Workspace/status grouping controls, the legacy 44px activity bar, the left-side file tree, and persistent local-runtime connection copy are absent from the new shell.
3. Local top-level projects scan through `mt_project::worktree`; authoritative facts populate main and linked worktree rows. A degraded scan keeps configured project/worktree rows visible.
4. Selecting a configured worktree activates its existing independent project state. Selecting an unregistered catalog worktree materializes it as a child project and then activates it, giving it independent terminal layout, open files, and file-tree state through the existing compatibility model.
5. Remote projects remain usable and render as a single worktree row until the remote catalog child is implemented.
6. The central workbench keeps the existing terminal/file surface. Opening a file creates one replaceable preview tab for the active worktree compatibility scope; opening another file replaces it in place. Editing, explicit promotion, or double-clicking the preview tab makes it permanent.
7. Double-clicking a file row continues to mean rename. It must not pin the preview tab.
8. The right sidebar is docked and always exposes top tabs in the exact order `Files / Git / Tasks / Sessions`. Files, Git, and Sessions reuse the existing active-project entities; switching worktrees therefore switches their roots and data scope.
9. Tasks has a visible, honest read-only placeholder in this child. It must not open a browser, execute `gh`, or imply authentication/data loading before the GitHub Tasks child exists.
10. Agents opens a fixed non-modal floating panel over the workbench. It does not replace or unmount the active workbench or right sidebar. The first slice may project current mini-term live pane status and must label the feed as live activity rather than historical sessions.
11. Existing settings, usage, add-project, worktree-management, terminal, file, Git, session, SSH-project, and mobile/SSH settings paths remain reachable from the new shell or Settings.
12. All compilation, formatting, linting, tests, UI smoke checks, and Windows packaging run in Docker. No Rust toolchain, target directory, or build cache is created on the host.

## Compatibility

- Existing project IDs remain the workbench bucket during this visible slice. Stable pane/session identities and terminal-host ownership remain separate children.
- Existing config and layout databases remain readable. The presentation change must not delete project groups or legacy layout fields even though the new sidebar does not render group hierarchy.
- The old shell remains in source only where needed for rollback while the shipped default uses the Orca shell.

## Acceptance Criteria

- [x] A clean launch visibly shows the new three-part shell without the legacy activity bar or left-side Files panel.
- [x] One local Git project with two worktrees shows one Project row and two Worktree rows; both can be selected.
- [x] Selecting an unregistered linked worktree creates one child project record, does not duplicate it on repeated activation, and opens that worktree root.
- [x] Two worktrees retain different terminal layouts and open file tabs when switching back and forth through the sidebar.
- [x] The right sidebar top tabs remain ordered `Files / Git / Tasks / Sessions`; Files/Git/Sessions track the active worktree compatibility scope.
- [x] Repeated single-click file opens replace one preview tab in place; double-clicking the preview tab or editing it preserves it when another file opens.
- [x] File-row double-click still enters rename and never promotes a preview.
- [x] Agents opens as a floating non-modal panel and closing it leaves the active worktree/tab intact.
- [x] Usage and Settings are the only fixed footer actions and both open their existing surfaces.
- [x] Tasks displays a truthful deferred state and performs no browser or `gh` side effect.
- [x] Docker-only format, clippy, focused tests, and workspace checks pass.
- [x] A replacement Windows installer is built only after the visible shell is compiled and verified.

## Out Of Scope

- Stable `PaneKey`, `TerminalSessionId`, and terminal incarnation migration.
- Detached terminal host, warm reattach, cold terminal history restore, and remote runtime transport.
- Authoritative remote worktree discovery and remote Agent identity protocol.
- Real GitHub Issues/PR fetching, details, authentication probing, or writes.
- Full global Agent feed history, unread acknowledgement protocol, and exact remote pane routing.
- Pixel-for-pixel Orca branding or its Electron/Tailwind implementation.
