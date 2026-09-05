# Approved Scoped Commits

The user confirmed this exact two-batch plan on 2026-09-05 with "可以".
Authorization includes committing the scoped changes, pushing to the existing
fork branch, and submitting scoped Actions diagnostic correction commits.
Execution and verification evidence is recorded in `validation.md`.

## 1. Worktree Sidebar Configuration

Message: `feat: add per-project worktree visibility`

This batch includes the shared sidebar's UI presentation changes and tests;
the underlying Agent recognition/lifecycle correction is the second batch.

- `crates/mt-config/src/config.rs`
- `crates/mt-config/src/db.rs`
- `crates/mt-config/src/lib.rs`
- `crates/mt-app/src/project_settings.rs`
- `crates/mt-app/src/worktree_visibility.rs`
- `crates/mt-app/src/worktree_catalog.rs`
- `crates/mt-app/src/orca_sidebar.rs`
- `crates/mt-app/src/store/projects.rs`
- `crates/mt-app/src/main.rs`
- `crates/mt-app/src/overlay.rs`
- `crates/mt-app/src/i18n.rs`
- `crates/mt-app/src/mobile_relay.rs`
- `crates/mt-app/src/project_list.rs`
- `crates/mt-app/src/project_tree.rs`
- `crates/mt-app/src/ssh_conn.rs`
- `crates/mt-app/src/store/identity.rs`
- `crates/mt-app/src/store/pure.rs`
- `crates/mt-layout/src/lib.rs`
- `crates/mt-i18n/locales/worktree.ts`
- `crates/mt-i18n/tests/consistency.rs`
- `.trellis/spec/mt-app/backend/navigation-catalog-contract.md`
- `.trellis/tasks/09-05-sidebar-worktree-discovery/`

Stage only this session's Actions-only constraint hunk from
`.trellis/spec/mt-app/backend/quality-guidelines.md`; the pre-existing Windows
API section and any other earlier dirty content must not be silently included.

## 2. Remote Agent State

Message: `fix: stabilize remote agent status`

- `crates/mt-ssh/src/agent.rs`
- `crates/mt-ai/src/agent_runtime.rs`
- `crates/mt-app/src/ai.rs`
- `crates/mt-app/src/pane.rs`
- `crates/mt-app/src/store/ai.rs`
- `crates/mt-app/src/store/remote_agents.rs`
- `.trellis/spec/mt-ssh/backend/remote-agent-inventory-contract.md`
- `.trellis/spec/mt-ai/backend/agent-runtime-contract.md`
- `.trellis/spec/mt-app/backend/remote-agent-reconciliation-contract.md`
- `.trellis/tasks/09-05-sidebar-agent-status/`
- `.trellis/tasks/09-05-project-sidebar-discovery-status/`

## Actions Follow-Up

Following the recorded confirmation, push to the existing tracking remote branch
`fork/feat/remote-file-management` (`vihor3/mini-term`). Existing branch-push
workflows run CI and Windows packaging. Do not create a release tag.

Actions must generate the i18n dictionary patch for the 14 added keys and any
formatting diagnostic patches. Apply only scoped generated/source corrections
and submit follow-up commits. Verify exact final product SHA, all required job
conclusions, and artifact correspondence. Never run local checks or generators.

## Excluded Dirty Work

Preserve all unrelated baseline changes, including `.agents/`, `.claude/`,
Trellis bootstrap/runtime/platform helpers and generic specs, older archived
tasks, `.trellis/workspace/` journals, `AGENTS.md`, `.gitattributes`, untracked
images, and key files. Do not blanket-stage the repository or `.trellis/`.
Both Cargo lockfiles, sidecars, and workflows are outside the current write
scope. Task archival and journal auto-commits remain blocked by unmet CI/native
acceptance; they must not sweep in earlier dirty journal/archive changes.
