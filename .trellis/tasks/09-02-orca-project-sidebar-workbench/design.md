# Technical Design

## Shell Boundary

Add a dedicated Orca sidebar entity and make `Workspace` compose three stable regions below the title bar:

```text
OrcaProjectSidebar | WorkbenchArea | WorktreeContextSidebar
```

The left and right entities observe the existing `AppStore`; the central workbench and terminal ownership remain unchanged. This keeps the visible migration reversible and avoids a second terminal/file source of truth.

## Project And Worktree Rows

`OrcaProjectSidebar` builds a pure row model from:

- top-level configured `ProjectConfig` records, flattened in saved tree order while ignoring legacy group presentation;
- configured child projects keyed by `parent_project_id`;
- per-local-project `mt_project::worktree::WorktreeScan` snapshots.

An authoritative scan supplies main/linked rows. A non-authoritative or failed scan merges, rather than removes, configured rows. Row activation resolves a normalized path to an existing configured project ID. If none exists, the action calls `AppStore::add_project_at(path, Some(parent_id))` and activates the returned ID. Therefore the current project-scoped layout/document/file state becomes the compatibility workbench bucket for that worktree.

The sidebar owns only presentation cache, scan generations, expansion state, and transient hover/overlay intent. Git facts stay in `mt-project`; project/layout ownership stays in `AppStore`.

## Workbench Preview Slot

Extend each existing `ProjectDocuments` bucket with preview metadata on `DocumentTab`.

- Opening an already-open document activates it.
- Opening a new document first checks for the current replaceable preview.
- A clean preview is replaced at the same tab index.
- A dirty preview is promoted before replacement, so user edits cannot be discarded.
- Double-clicking a preview tab promotes it.
- Closing and active-tab fallback continue to use the existing document identity and dirty confirmation path.

This child uses one tab group per compatibility workbench because mini-term does not yet have workbench-level file split groups. The future stable identity child can lift the same rule to `WorktreeId + TabGroupId` without changing the visible interaction.

## Context Sidebar

Replace the transient Sessions/Git drawer route with an application-level `ContextPanel` route:

```text
Files | Git | Tasks | Sessions
```

The sidebar is docked at the right using the existing persisted drawer width and resize handle. Existing entities are mounted one at a time:

- Files: `FileTree`
- Git: `GitPanel`
- Tasks: static deferred-state panel
- Sessions: `SessionPanel`

Panel selection changes visibility gates on Git and Sessions so background scans retain their current lifecycle rules. Switching worktrees changes `AppStore::active_project_id`; each existing panel already observes that source and applies its own request fencing.

## Agents Overlay

The left sidebar emits a `ToggleAgents` event. `Workspace` stores an `agents_open` boolean and the pane ID that held focus before opening. The overlay is drawn after the three-region shell and before modal/menu layers, with a fixed anchor immediately to the right of the project sidebar and viewport-clamped width/height.

The initial contents are a compatibility projection of current live pane statuses grouped by attention/working/recent. It is explicitly not the historical session scanner. Closing by toggle, close button, outside click, or Escape restores the previously focused live pane when possible and otherwise falls back to the active pane.

## Compatibility And Rollback

- Preserve `ProjectList` and legacy ActivityBar code until this slice has shipped and been validated; `Workspace` stops composing them in the default shell.
- Do not mutate or delete saved project groups. The new row builder only ignores their visual grouping.
- Do not change terminal ownership or recovery claims.
- Reverting the shell composition restores the previous presentation while all project/layout data remains valid.

## Risks

- A catalog scan may be slow or fail. The sidebar must render configured fallback rows immediately and apply generation checks to late results.
- Creating a project while a scan completes can produce duplicates unless normalized path lookup happens again inside the store update.
- Reparenting a top-level configured linked worktree is a migration decision and is not performed automatically in this child.
- Existing FileTree headers include their own title/actions. The outer top tabs must not create nested card chrome or remove those actions.
