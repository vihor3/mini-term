# Orca Project Worktree Runtime

## Goal

Implement the approved Orca-aligned Project -> Worktree architecture across catalog identity, workbench state, detached terminal recovery, remote runtime/agent status, contextual sidebar, GitHub Tasks, and global Agents feed.

This parent task turns the approved research in `.trellis/tasks/archive/2026-09/09-01-orca-worktree-terminal-research` into independently verifiable implementation children while preserving one identity, ownership, and recovery model across all phases.

## Source Decisions

- The user-facing hierarchy is `Project -> Worktree`; there is no Workspace or Status grouping mode.
- Each worktree owns independent terminal tabs, splits, open files, preview slots, view state, and Agent session history.
- The center workbench uses a unified terminal/file/diff/detail tab strip. The right sidebar keeps `Files / Git / Tasks / Sessions` at the top.
- The global Agents entry opens a fixed anchored non-modal overlay and never replaces the active workbench route.
- Files single-click uses a replaceable preview slot per `WorktreeId + TabGroupId`; file-row double-click renames, while preview-tab double-click/Pin/edit makes it permanent.
- Terminal warm reattach means the same live PTY. Cold visual restore and provider resume are separate, explicitly labeled fallbacks.
- Remote Agent identity prioritizes launch attestation and provider Hooks; connectivity and activity are separate state axes.
- GitHub Tasks are read-only, use the project execution host's `gh`, render list/details inside mini-term, and never fall back from remote `gh` to local credentials.
- GitHub auth-required UI only shows the target host, `gh auth login --hostname <host>`, Copy, and Retry. mini-term does not execute login, create a terminal, or open a browser.

## Requirements

1. Deliver each major architecture slice as a child task with explicit dependencies, acceptance criteria, tests, rollout control where the repository supports one, and an explicit rollback point.
2. Preserve stable `ExecutionHostId`, `RepoId`, `WorktreeId`, `PaneKey`, `TerminalSessionId`, `TerminalIncarnationId`, and `AgentRunId` boundaries across child tasks.
3. Keep asynchronous results fenced by the identity and generation that owned the request; switching worktrees, hosts, accounts, or terminal incarnations must not accept stale results.
4. Keep compatibility projections for the existing project list, terminal layout, Hook status, Git/file/session panels, and SSH paths until their replacement child is independently verified.
5. Do not let presentation changes become a second source of truth for Git facts, PTY ownership, Agent status, session history, usage totals, or GitHub auth.

## Acceptance Criteria

- [x] All planned child tasks are implemented, checked, committed, and archived with their dependencies satisfied.
- [x] The final integration check demonstrates two worktrees in one project with independent terminal/file state and correctly scoped right-sidebar data.
- [x] Closing and reopening the GUI reattaches surviving terminal sessions without changing their process/incarnation; cold recovery is visibly distinct.
- [x] Local, WSL, and SSH Agent state remains bound to the correct host/worktree/pane through disconnect, replay, and reconnect.
- [x] GitHub Tasks use the correct execution-host `gh`, show internal read-only details, and only display manual auth remediation when unauthenticated.
- [x] The approved v2 Orca-aligned layout and interaction rules are covered by interaction/snapshot tests without reintroducing discarded UI variants.
- [x] Every rollout or rollback control can restore the prior presentation/runtime slice without corrupting newer persisted identities/schema.

## Out Of Scope

- Pixel-for-pixel Orca branding or Electron/Tailwind implementation.
- GitHub issue/PR writes, merge, comments, Projects board management, Linear/Jira, automations, or an embedded browser.
- Tracking arbitrary system-wide Agent processes outside mini-term-owned terminals.
- Claiming to revive a dead OS process after machine restart or terminal-host data loss.
- A separate Kanban Agent dashboard in the MVP.

## Notes

- Normative research and detailed architecture remain in `.trellis/tasks/archive/2026-09/09-01-orca-worktree-terminal-research`.
- This parent normally stays planning/integration-only; implementation runs in child tasks.
