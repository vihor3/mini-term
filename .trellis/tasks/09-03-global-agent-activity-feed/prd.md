# Global Agent Activity Feed

## Goal

Replace the Orca Agents overlay placeholder aggregation with a fixed anchored,
non-modal feed of exact live Agent runs. Users can see which worktree needs them,
which Agents are working, and which runs completed recently, then jump to the
exact current terminal without changing or unloading the active workbench until
they deliberately select a row.

## Requirements

- Consume only authoritative `AgentTargetView` projections from the runtime
  registry. Historical session files and project-level status summaries must not
  create feed rows or live claims.
- Keep the existing left-sidebar Agents entry and fixed overlay behavior: anchor
  to its right, remain inside the viewport, scroll only the list, and do not add
  dragging, resizing, or geometry persistence.
- Opening the overlay must not change active project/worktree, terminal/file/task
  tab, split layout, context tab, drafts, PTY ownership, or terminal output.
- Escape, outside click, close icon, and pressing Agents again share one close
  action. Non-navigation close restores the previously focused terminal or
  active workbench page; missing targets fall back safely.
- Group exact runs as `Needs You`, `Working`, and `Recent`. Provider identity,
  activity, connectivity, project/worktree, pane label, and relative receipt time
  remain distinct visible fields.
- Row identity is `AgentRunId`; route identity remains execution host,
  `WorktreeId`, `TabId`, `PaneKey`, terminal session, and incarnation.
- Clicking a row calls the shared exact activation action. It closes the overlay
  only after successful activation; a stale/missing route stays inert and keeps
  the row available for diagnosis.
- Track feed acknowledgement by `AgentRunId + last AgentEventId`, not by project
  or path. Opening the overlay does not acknowledge anything. A successful exact
  activation acknowledges only that event; a later accepted event on the same
  run becomes unread again automatically.
- Do not let window-focus clearing of the legacy tray completion counter clear
  feed acknowledgement. The two sources have separate compatibility semantics.
- Connectivity is not activity. Disconnected/Stale does not become Done, and a
  failed or blocked live run remains in Needs You.
- Limit or virtualize Recent presentation so large historical runtime sets do
  not rebuild the shell or move overlay geometry.
- `MINI_TERM_GLOBAL_AGENT_ACTIVITY=0` disables the global entry/overlay while
  preserving inline Agent rows, Sessions, runtime state, and exact activation.

## Acceptance Criteria

- [ ] Needs You, Working, and Recent ordering is deterministic across projects, hosts, providers, and equal timestamps.
- [ ] Same-path worktrees and multiple runs from one provider render distinct rows and activate distinct terminal incarnations.
- [ ] Opening/closing by all four routes preserves workbench state and restores focus correctly.
- [ ] Successful row activation focuses the exact project/tab/split pane and then acknowledges only that run event.
- [ ] Failed stale-route activation does not close the overlay, create/resume a terminal, or acknowledge the row.
- [ ] Opening the overlay does not clear unread; a new accepted event after acknowledgement makes the run unread again.
- [ ] Replay/duplicate/out-of-order events remain deduplicated by the runtime registry and do not create duplicate feed rows.
- [ ] Disconnected/stale connectivity is displayed independently from activity and never fabricates completion.
- [ ] The overlay remains inside narrow and normal viewports with the close control always reachable.
- [ ] Docker-only focused tests, workspace check, Clippy, and Windows MSVC check pass.

## Out Of Scope

- Historical transcript browsing or provider resume from the global feed.
- A central Agents page, Kanban dashboard, modal dialog, draggable window, or
  persisted overlay geometry.
- Tracking Agents launched outside mini-term-owned terminals.
- Changing the existing provider event protocol or remote inventory transport.
