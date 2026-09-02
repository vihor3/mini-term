# Technical Design

## Architecture Source

The approved `.trellis/tasks/archive/2026-09/09-01-orca-worktree-terminal-research/design.md` is the normative architecture. This parent records only the cross-child integration contracts and release order.

## Cross-Child Contracts

```text
ExecutionHostId
  -> RepoId
    -> WorktreeId
      -> WorktreeWorkbenchState
        -> TabId / PaneKey
          -> TerminalSessionId / TerminalIncarnationId
            -> AgentRunId / ProviderSessionId
```

- IDs are stable ownership keys; display names, branches, connection IDs, runtime PTY numbers, and UI indexes are projections only.
- Git/worktree facts originate on the execution host. Only authoritative generations may remove previously known rows.
- Long-lived PTYs belong to a terminal host, not the GUI. GUI close is detach; terminal-tab close is kill.
- Agent Hook/replay events update state only when host, worktree, pane, terminal session/incarnation, Agent run, and generation fences match.
- Files/Git/Sessions UI state is worktree-scoped. Tasks fetch/cache is execution-host/project/repository/auth-generation scoped, while its selection is worktree-scoped.
- Existing panels and persistence schemas remain readable during migration. Where a shared rollout-control mechanism exists, it switches ownership/presentation only after the new source is verified; schema-free foundational children may instead use unchanged compatibility APIs plus code revert.

## Integration Strategy

1. Establish Git/worktree facts and stable identities.
2. Persist worktree-scoped workbench and terminal bindings.
3. Move PTY ownership out of the GUI and add warm reattach.
4. Add bounded terminal history and explicit cold restore.
5. Establish authenticated remote runtime identity/transport.
6. Normalize remote Agent identity/status and replay.
7. Replace the application shell with the Orca-aligned Project sidebar/workbench.
8. Move worktree context into the right sidebar and add recovery diagnostics.
9. Add execution-host GitHub Tasks.
10. Add the global Agents overlay and run final integration checks.

Each child owns its schema/API migrations and an explicit rollback mechanism appropriate to its layer. A later child may depend on an earlier contract but must not silently redefine it.

## Release Safety

- New persisted fields are additive and salvageable until the owning child proves migration coverage.
- Stale async results fail closed and cannot delete, replace, or relabel current state.
- Remote host identity changes require explicit trust/reconciliation rather than path-based continuity.
- Rollback gates preserve newer persisted IDs even when presentation falls back to an older surface.
