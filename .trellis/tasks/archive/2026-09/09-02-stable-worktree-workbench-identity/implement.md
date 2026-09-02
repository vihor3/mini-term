# Implementation Plan

## 1. Shared Identity Crate

- Add `crates/mt-identity` with opaque serde newtypes, UUID generation, deterministic domain-separated derivation, validation, and unit tests.
- Register the workspace dependency and add only the required package dependencies to `mt-config`, `mt-layout`, `mt-project`, and `mt-app`.
- Keep `mt-core` unchanged so existing sidecar dependency weight and direction remain intact.

## 2. Local And Provisional Worktree Resolution

- Add `mt_project::worktree::identity` for canonical local common-dir/worktree resolution and non-Git directory identity.
- Add pure provisional WSL/SSH resolution inputs without network access or authority claims.
- Reuse the catalog common-dir/path rules and add real-Git fixtures for main/linked worktrees.

## 3. Saved Layout Identity Fields

- Extend `SavedProjectLayout`, `SavedTab`, `SavedSplitNode::Leaf`, and `SavedPane` with optional stable IDs and active pointers.
- Preserve camelCase, missing-field compatibility, legacy `pane`, and `activeTabIndex` fallback.
- Add serde round-trip and old-shape compatibility tests in `mt-config`.

## 4. Additive Layout Database And Migration

- Add local host-install metadata, `project_worktree_binding`, and `worktree_layout` to `mt-layout`.
- Implement transactional binding reconciliation, legacy-row migration, rebind/collision policy, dual write, live-binding retention, and rollback fallback.
- Add bounded valid-JSON salvage and stable-ID normalization with one-time writeback.
- Cover schema upgrade/downgrade compatibility, malformed pane salvage, malformed row isolation, migration idempotence, and destination-wins rebinding.

## 5. AppStore Identity Registry

- Add `crates/mt-app/src/store/identity.rs` and initialize bindings before restoring project states.
- Track `active_worktree_id`; expose project-to-worktree/worktree-to-project lookup helpers for UI and future async owners.
- Route startup, add local project, materialize linked worktree, add remote project, project removal, and layout flush through the registry.
- Keep `project_states` and current public commands project-compatible while making worktree identity authoritative for persistence/routing.

## 6. Stable Tab, Pane, Session, And Incarnation Lifecycle

- Add stable fields to `ProjectPanel` and `PaneState`; make existing string IDs compatibility projections.
- Serialize/restore stable tab, pane, session, incarnation, active-tab, and active-pane values.
- Refactor PTY creation so pane/session identity exists before spawn; rotate incarnation on new PTY/reconnect and persist it.
- Inject stable local identity environment fields while retaining `MINITERM_PTY_ID` for the current Hook compatibility path.
- Add tests for split/move/reorder/close/reload identity preservation and old-incarnation rejection at the binding layer.

## 7. Worktree-Scoped Document Runtime

- Rekey `DocumentKey`, `ProjectDocuments`, preview state, active page, dirty-document lookup, and deferred focus/close checks to `WorktreeId`.
- Preserve `DocumentSource` project/backend/path data for I/O and revalidate the project binding before deferred actions.
- Add two-worktree and stale-callback tests without adding document disk persistence.

## 8. Contracts And Integration Checks

- Add/update the project specs for stable worktree/workbench identity, layout migration, and the file-workbench scope transition.
- Verify Orca sidebar selection still resolves the configured project while the workbench exposes the corresponding stable worktree.
- Confirm current local, WSL, SSH, mobile, Git, Sessions, preview, and terminal paths retain their compatibility behavior.

## 9. Docker-Only Validation

Run all Rust work through the existing Docker harness:

```bash
./scripts/docker-ci.sh fmt-check
./scripts/docker-ci.sh test -p mt-identity
./scripts/docker-ci.sh test -p mt-project worktree
./scripts/docker-ci.sh test -p mt-config saved_layout
./scripts/docker-ci.sh test -p mt-layout
./scripts/docker-ci.sh test -p mt-app persist
./scripts/docker-ci.sh test -p mt-app workbench_area
./scripts/docker-ci.sh test -p mt-app tree
./scripts/docker-ci.sh clippy -p mt-identity -p mt-config -p mt-layout -p mt-project -p mt-app --all-targets -- -D warnings
./scripts/docker-ci.sh check
```

Run broader workspace tests after focused suites. Record any pre-existing unrelated failure separately; no task-owned warning or failure is accepted.

## 10. Windows Package And Cleanup

- Build the Windows x64 main executable, sidecars, and NSIS installer inside Docker using the established cargo-xwin pipeline.
- Use an identity-specific artifact/version marker so it cannot be confused with the previous Orca-shell installer.
- Inspect the PE/installer payload and record size, SHA-256, architecture, and package contents.
- Remove task-created containers and Docker Cargo/target caches after verification.
- Verify the host has no repository `target`, `~/.cargo`, `~/.rustup`, `cargo`, or `rustc` state created by this task.

## High-Risk Files

- `Cargo.toml` and package `Cargo.toml` files
- `crates/mt-config/src/config.rs`
- `crates/mt-layout/src/lib.rs`
- `crates/mt-project/src/worktree/`
- `crates/mt-app/src/store/mod.rs`
- `crates/mt-app/src/store/projects.rs`
- `crates/mt-app/src/store/ssh.rs`
- `crates/mt-app/src/store/layout.rs`
- `crates/mt-app/src/store/panes.rs`
- `crates/mt-app/src/tree.rs`
- `crates/mt-app/src/persist.rs`
- `crates/mt-app/src/workbench_area.rs`

## Rollback Points

- Identity crate/types: retain opaque serialized strings; callers can return to compatibility IDs without deleting data.
- Binding/schema: disable worktree-preferred reads and continue from dual-written `project_layout`; leave additive tables untouched.
- Runtime pane identity: retain current `u32 pty_id` maps and string IDs as compatibility projections.
- Document scope: restore project-keyed runtime buckets; no document state is persisted by this child.
- Rebind failure: preserve the old binding/layout and run cold-only rather than creating a partial destination.

## Completion Review

- Confirm no code claims same-process warm reattach or authoritative remote identity.
- Confirm every persisted/routed terminal target includes `WorktreeId + TabId + PaneKey + TerminalSessionId + expected incarnation` where available.
- Confirm old config/layout fixtures load without destructive rewrite and migration is idempotent.
- Confirm the Windows artifact contains the stable-identity implementation and was built only after Docker checks passed.
