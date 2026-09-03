# File Review Map

Date: 2026-09-03; Actions-only policy synchronized 2026-09-03

Scope: every non-Trellis file changed by `0bc6f28..c644ae9`.

## Coverage Summary

- D1 Catalog/Identity/Persistence: 20 primary files.
- D2 Terminal Host/History/PTy: 15 primary files.
- D3 Remote/Agent/GitHub: 21 primary files.
- D4 Orca UI/Context: 17 primary files.
- D5 Actions/Windows Release: 20 primary files.
- Cross-layer second pass: 29 files.
- Total: 93 files; the generator fails if any path is unclassified.
- The four local Docker CI files remain in this frozen historical inventory because
  they were introduced in the audited range; the 2026-09-03 remediation retires them.
- Remediation also covers `.github/workflows/ci.yml`,
  `.github/workflows/windows-package.yml`, `scripts/build-windows-installer.ps1`,
  and `scripts/verify-windows-installer.ps1` in the final Actions/release pass.

## File Ownership

| Path | Primary review | Second pass | Reason |
|------|----------------|-------------|--------|
| `.github/workflows/release.yml` | D5 Actions/Windows Release | - | Build graph, CI, staging, or installer reproducibility. |
| `Cargo.lock` | D5 Actions/Windows Release | - | Build graph, CI, staging, or installer reproducibility. |
| `Cargo.toml` | D5 Actions/Windows Release | - | Build graph, CI, staging, or installer reproducibility. |
| `crates/mt-ai/Cargo.toml` | D5 Actions/Windows Release | D3 Remote/Agent/GitHub | Build graph, CI, staging, or installer reproducibility. |
| `crates/mt-ai/src/agent_runtime.rs` | D3 Remote/Agent/GitHub | - | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-ai/src/lib.rs` | D3 Remote/Agent/GitHub | - | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-app/Cargo.toml` | D5 Actions/Windows Release | D4 Orca UI/Context | Build graph, CI, staging, or installer reproducibility. |
| `crates/mt-app/src/agent_activity.rs` | D4 Orca UI/Context | - | Project/worktree presentation, panel state, focus, overlay, or event routing. |
| `crates/mt-app/src/ai.rs` | D3 Remote/Agent/GitHub | D4 Orca UI/Context | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-app/src/dnd.rs` | D4 Orca UI/Context | - | Project/worktree presentation, panel state, focus, overlay, or event routing. |
| `crates/mt-app/src/execution_host.rs` | D3 Remote/Agent/GitHub | D4 Orca UI/Context | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-app/src/file_tree/menu.rs` | D4 Orca UI/Context | - | Project/worktree presentation, panel state, focus, overlay, or event routing. |
| `crates/mt-app/src/file_tree/mod.rs` | D4 Orca UI/Context | - | Project/worktree presentation, panel state, focus, overlay, or event routing. |
| `crates/mt-app/src/file_tree/tests.rs` | D4 Orca UI/Context | - | Project/worktree presentation, panel state, focus, overlay, or event routing. |
| `crates/mt-app/src/file_viewer.rs` | D1 Catalog/Identity/Persistence | D4 Orca UI/Context | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-app/src/file_viewer_tests.rs` | D1 Catalog/Identity/Persistence | - | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-app/src/git_changes.rs` | D4 Orca UI/Context | - | Project/worktree presentation, panel state, focus, overlay, or event routing. |
| `crates/mt-app/src/git_history.rs` | D4 Orca UI/Context | - | Project/worktree presentation, panel state, focus, overlay, or event routing. |
| `crates/mt-app/src/git_panel.rs` | D4 Orca UI/Context | D1 Catalog/Identity/Persistence | Project/worktree presentation, panel state, focus, overlay, or event routing. |
| `crates/mt-app/src/git_worktree.rs` | D4 Orca UI/Context | - | Project/worktree presentation, panel state, focus, overlay, or event routing. |
| `crates/mt-app/src/github_tasks.rs` | D3 Remote/Agent/GitHub | D4 Orca UI/Context | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-app/src/main.rs` | D4 Orca UI/Context | - | Project/worktree presentation, panel state, focus, overlay, or event routing. |
| `crates/mt-app/src/orca_sidebar.rs` | D4 Orca UI/Context | - | Project/worktree presentation, panel state, focus, overlay, or event routing. |
| `crates/mt-app/src/overlay.rs` | D4 Orca UI/Context | - | Project/worktree presentation, panel state, focus, overlay, or event routing. |
| `crates/mt-app/src/pane.rs` | D2 Terminal Host/History/PTy | D1 Catalog/Identity/Persistence | PTY ownership, IPC, replay/history, snapshot, or terminal binding lifecycle. |
| `crates/mt-app/src/persist.rs` | D1 Catalog/Identity/Persistence | - | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-app/src/project_list.rs` | D4 Orca UI/Context | - | Project/worktree presentation, panel state, focus, overlay, or event routing. |
| `crates/mt-app/src/remote_ssh/delete.rs` | D3 Remote/Agent/GitHub | D4 Orca UI/Context | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-app/src/remote_ssh/mod.rs` | D3 Remote/Agent/GitHub | D4 Orca UI/Context | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-app/src/remote_ssh/tests.rs` | D3 Remote/Agent/GitHub | D4 Orca UI/Context | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-app/src/search_modal.rs` | D1 Catalog/Identity/Persistence | D4 Orca UI/Context | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-app/src/session_panel.rs` | D4 Orca UI/Context | D1 Catalog/Identity/Persistence | Project/worktree presentation, panel state, focus, overlay, or event routing. |
| `crates/mt-app/src/store/ai.rs` | D3 Remote/Agent/GitHub | D4 Orca UI/Context | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-app/src/store/context.rs` | D4 Orca UI/Context | D1 Catalog/Identity/Persistence | Project/worktree presentation, panel state, focus, overlay, or event routing. |
| `crates/mt-app/src/store/identity.rs` | D1 Catalog/Identity/Persistence | - | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-app/src/store/layout.rs` | D1 Catalog/Identity/Persistence | - | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-app/src/store/mod.rs` | D4 Orca UI/Context | - | Project/worktree presentation, panel state, focus, overlay, or event routing. |
| `crates/mt-app/src/store/panes.rs` | D2 Terminal Host/History/PTy | D1 Catalog/Identity/Persistence | PTY ownership, IPC, replay/history, snapshot, or terminal binding lifecycle. |
| `crates/mt-app/src/store/projects.rs` | D1 Catalog/Identity/Persistence | - | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-app/src/store/remote_agents.rs` | D3 Remote/Agent/GitHub | D4 Orca UI/Context | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-app/src/store/remote_runtime.rs` | D3 Remote/Agent/GitHub | D4 Orca UI/Context | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-app/src/store/ssh.rs` | D2 Terminal Host/History/PTy | D1 Catalog/Identity/Persistence | PTY ownership, IPC, replay/history, snapshot, or terminal binding lifecycle. |
| `crates/mt-app/src/tree.rs` | D1 Catalog/Identity/Persistence | D4 Orca UI/Context | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-app/src/workbench_area.rs` | D1 Catalog/Identity/Persistence | D4 Orca UI/Context | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-config/Cargo.toml` | D5 Actions/Windows Release | D1 Catalog/Identity/Persistence | Build graph, CI, staging, or installer reproducibility. |
| `crates/mt-config/src/config.rs` | D1 Catalog/Identity/Persistence | - | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-config/src/lib.rs` | D1 Catalog/Identity/Persistence | - | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-github/Cargo.toml` | D5 Actions/Windows Release | D3 Remote/Agent/GitHub | Build graph, CI, staging, or installer reproducibility. |
| `crates/mt-github/src/commands.rs` | D3 Remote/Agent/GitHub | - | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-github/src/error.rs` | D3 Remote/Agent/GitHub | - | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-github/src/lib.rs` | D3 Remote/Agent/GitHub | - | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-github/src/model.rs` | D3 Remote/Agent/GitHub | - | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-github/src/remote.rs` | D3 Remote/Agent/GitHub | - | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-identity/Cargo.toml` | D5 Actions/Windows Release | D1 Catalog/Identity/Persistence | Build graph, CI, staging, or installer reproducibility. |
| `crates/mt-identity/src/lib.rs` | D1 Catalog/Identity/Persistence | - | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-layout/Cargo.toml` | D5 Actions/Windows Release | D1 Catalog/Identity/Persistence | Build graph, CI, staging, or installer reproducibility. |
| `crates/mt-layout/src/lib.rs` | D1 Catalog/Identity/Persistence | - | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-project/Cargo.toml` | D5 Actions/Windows Release | D1 Catalog/Identity/Persistence | Build graph, CI, staging, or installer reproducibility. |
| `crates/mt-project/src/git.rs` | D1 Catalog/Identity/Persistence | - | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-project/src/lib.rs` | D1 Catalog/Identity/Persistence | - | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-project/src/watch.rs` | D1 Catalog/Identity/Persistence | - | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-project/src/worktree/catalog.rs` | D1 Catalog/Identity/Persistence | - | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-project/src/worktree/identity.rs` | D1 Catalog/Identity/Persistence | - | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-project/src/worktree/mod.rs` | D1 Catalog/Identity/Persistence | - | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-project/src/worktree/porcelain.rs` | D1 Catalog/Identity/Persistence | - | Stable identities, catalog facts, persistence, migration, or workbench state. |
| `crates/mt-pty/src/lib.rs` | D2 Terminal Host/History/PTy | - | PTY ownership, IPC, replay/history, snapshot, or terminal binding lifecycle. |
| `crates/mt-pty/src/ssh.rs` | D2 Terminal Host/History/PTy | - | PTY ownership, IPC, replay/history, snapshot, or terminal binding lifecycle. |
| `crates/mt-ssh/Cargo.toml` | D5 Actions/Windows Release | D3 Remote/Agent/GitHub | Build graph, CI, staging, or installer reproducibility. |
| `crates/mt-ssh/src/agent.rs` | D3 Remote/Agent/GitHub | - | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-ssh/src/lib.rs` | D3 Remote/Agent/GitHub | - | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-ssh/src/pool.rs` | D3 Remote/Agent/GitHub | - | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-ssh/src/runtime.rs` | D3 Remote/Agent/GitHub | - | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-ssh/src/sftp.rs` | D3 Remote/Agent/GitHub | - | Execution-host routing, authenticated SSH, Agent state, or GitHub data. |
| `crates/mt-terminal-host/Cargo.toml` | D5 Actions/Windows Release | D2 Terminal Host/History/PTy | Build graph, CI, staging, or installer reproducibility. |
| `crates/mt-terminal-host/src/client.rs` | D2 Terminal Host/History/PTy | - | PTY ownership, IPC, replay/history, snapshot, or terminal binding lifecycle. |
| `crates/mt-terminal-host/src/history.rs` | D2 Terminal Host/History/PTy | - | PTY ownership, IPC, replay/history, snapshot, or terminal binding lifecycle. |
| `crates/mt-terminal-host/src/ipc.rs` | D2 Terminal Host/History/PTy | - | PTY ownership, IPC, replay/history, snapshot, or terminal binding lifecycle. |
| `crates/mt-terminal-host/src/lib.rs` | D2 Terminal Host/History/PTy | - | PTY ownership, IPC, replay/history, snapshot, or terminal binding lifecycle. |
| `crates/mt-terminal-host/src/main.rs` | D2 Terminal Host/History/PTy | - | PTY ownership, IPC, replay/history, snapshot, or terminal binding lifecycle. |
| `crates/mt-terminal-host/src/protocol.rs` | D2 Terminal Host/History/PTy | - | PTY ownership, IPC, replay/history, snapshot, or terminal binding lifecycle. |
| `crates/mt-terminal-host/src/server.rs` | D2 Terminal Host/History/PTy | - | PTY ownership, IPC, replay/history, snapshot, or terminal binding lifecycle. |
| `crates/mt-terminal-host/tests/lifecycle.rs` | D2 Terminal Host/History/PTy | - | PTY ownership, IPC, replay/history, snapshot, or terminal binding lifecycle. |
| `crates/mt-terminal/Cargo.toml` | D5 Actions/Windows Release | D2 Terminal Host/History/PTy | Build graph, CI, staging, or installer reproducibility. |
| `crates/mt-terminal/src/lib.rs` | D2 Terminal Host/History/PTy | - | PTY ownership, IPC, replay/history, snapshot, or terminal binding lifecycle. |
| `crates/mt-terminal/src/snapshot.rs` | D2 Terminal Host/History/PTy | - | PTY ownership, IPC, replay/history, snapshot, or terminal binding lifecycle. |
| `crates/mt-ui/src/terminal/ime.rs` | D4 Orca UI/Context | - | Project/worktree presentation, panel state, focus, overlay, or event routing. |
| `docker-compose.ci.yml` | D5 Actions/Windows Release | - | Build graph, CI, staging, or installer reproducibility. |
| `docker/ci/Dockerfile` | D5 Actions/Windows Release | - | Build graph, CI, staging, or installer reproducibility. |
| `docker/ci/README.md` | D5 Actions/Windows Release | - | Build graph, CI, staging, or installer reproducibility. |
| `scripts/docker-ci.sh` | D5 Actions/Windows Release | - | Build graph, CI, staging, or installer reproducibility. |
| `scripts/stage-sidecars.mjs` | D5 Actions/Windows Release | - | Build graph, CI, staging, or installer reproducibility. |
| `scripts/windows-installer.nsi` | D5 Actions/Windows Release | - | Build graph, CI, staging, or installer reproducibility. |
| `sidecars/Cargo.lock` | D5 Actions/Windows Release | - | Build graph, CI, staging, or installer reproducibility. |

## Scope Rule

Reviewers may read surrounding baseline code, but findings require causal evidence in
`0bc6f28..c644ae9`. Adjacent baseline lines may be changed only when the minimal scoped
fix cannot be expressed otherwise.

The explicit 2026-09-03 policy change authorizes Actions workflow hardening and retirement
of the task-added local Docker harness as task remediation, without converting
unrelated baseline workflow observations into audit findings.
