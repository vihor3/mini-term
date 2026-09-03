# Implementation Plan

## Child Task Map

| Order | Child | Dependency | Independent acceptance |
|---|---|---|---|
| 1 | `worktree-catalog-v2` | none | authoritative porcelain catalog, source semantics, and safe legacy-UI fencing |
| 2 | `stable-worktree-workbench-identity` | child 1 Git fact/path contract | shared strong host/repo/worktree/tab/pane/terminal IDs, persisted mappings, and migration |
| 3 | `terminal-host-warm-reattach` | child 2 | detached PTY ownership and same-process reattach |
| 4 | `terminal-snapshot-cold-restore` | child 3 | bounded snapshot/replay and explicit cold recovery |
| 5 | `remote-runtime-foundation` | child 2; terminal persistence integrates with 3/4 | host identity, authenticated mux, inventory, reconnect |
| 6 | `remote-agent-identity-status` | children 2, 5; precise PTY binding uses 3 | Hook adapters, replay, fencing, status model |
| 7 | `orca-project-sidebar-workbench` | children 1, 2 | approved Project -> Worktree shell and independent workbenches |
| 8 | `worktree-context-sidebar` | child 7; live status uses 6; recovery uses 3/4 | Files/Git/Sessions and diagnostics |
| 9 | `github-project-tasks` | children 1, 7, 8; SSH command path uses 5 | execution-host `gh`, internal read-only details, manual auth remediation |
| 10 | `global-agent-activity-feed` | children 6, 7, 8 | anchored Agents overlay and exact pane routing |

Children are created just in time so their PRD/design/implementation context reflects completed predecessor contracts. Dependency ordering is recorded in each child, not inferred only from this table.

## Parent Integration Check

After all children complete:

1. [x] Run full workspace formatting, lint, and tests.
2. [x] Run multi-worktree local integration coverage for independent terminals/files/context state.
3. [x] Run detach/reattach and cold-restore process tests.
4. [x] Run fake SSH reconnect/replay/old-generation tests and at least one real remote smoke test.
5. [x] Run GPUI interaction/geometry checks against the approved v2 baseline on normal and compact desktop sizes; attempt the Windows screenshot smoke and record the environment limit when Wine cannot supply required Windows system DLLs.
6. [x] Verify GitHub Tasks against local and remote fake command runners, including manual `gh auth login` remediation.
7. [x] Review rollout/rollback controls and persisted-schema compatibility before parent completion.
