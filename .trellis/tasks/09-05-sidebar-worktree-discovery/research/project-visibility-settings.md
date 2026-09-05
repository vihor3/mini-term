# Research: Project Worktree Visibility Settings

- Query: Add per-project selection of sidebar worktrees, hiding invalid entries
  by default.
- Scope: Internal source inspection; implementation remains unapproved.
- Date: 2026-09-05

## Confirmed Request

The user explicitly requests invalid entries hidden by default, a project
configuration entry for selecting individual worktrees, and a separate fix for
remote Agent status flashing. A blanket hide-external toggle alone does not
satisfy individual selection. The reference is
`/home/leo/.cache/tmp/orca-paste-1788610690574-105422f2-fedb-43be-9b19-075c39de865e.png`.
Copying every other Orca menu command is not required by this request.

## Menu and Form Integration

- `crates/mt-app/src/orca_sidebar.rs:359`, `render_project_row`, is the active
  project header and currently has no per-project options menu. Its
  collection-wide menu at `:310` is a different target.
- `crates/mt-app/src/project_list.rs:819`, `project_menu`, is an older project
  action surface, not the Orca-style header renderer.
- `crates/mt-app/src/menu.rs` owns anchored menus and focus/close behavior.
  Capture the clicked root project ID before opening settings; do not resolve
  the target from whatever project is active when a later callback runs.
- `crates/mt-app/src/env_vars.rs`, `open`, demonstrates an entity-backed
  project form using `prompt::open_guarded` and explicit save/cancel behavior.
- `crates/mt-app/src/ui.rs:825`, `checkbox`, is the existing row checkbox.

Proposed UI: project ellipsis -> Project Settings -> a bounded scrollable
worktree list with checkbox, branch/name, host/path, and state. Local, WSL,
and SSH use the same surface. Save applies the draft; Cancel discards it.
The settings entry remains reachable even when ordinary sidebar rows are
hidden. Do not expose local-only filesystem actions for remote paths.

## Discovery Must Remain Complete

- `crates/mt-app/src/worktree_catalog.rs:284`, `groups`, provides the shared
  inventory; `resolve_target` at `:288` also depends on this inventory.
- `build_groups` at `:1070` projects all facts, and `row_from_fact` at `:1190`
  already carries prunable, path, authority, and last-known state.
- `crates/mt-app/src/orca_sidebar.rs:816` renders all group rows today.
- `crates/mt-app/src/jump_palette.rs:605` also consumes the shared groups.

Derive sidebar visibility without removing raw discovery or making hidden
targets unresolvable. Settings/management need the full inventory. Scope
this change to sidebar presentation, not global search, unless the user
expands the request. Changing a checkbox must not prune Git, remove projects,
close terminals, stop Agents, or start another Git/SSH scan.

Invalid means a prunable registration or a positively established missing
path. An SSH outage, failed refresh, unknown path state, or non-authoritative
scan is not evidence of invalidity. Preserve last-known valid rows. An invalid
row stays out of default sidebar navigation even if an old visibility choice
would otherwise include it; a scan must not erase the saved choice.

## Persistence and Identity

- `crates/mt-config/src/config.rs:421`, `ProjectConfig`, has no visibility
  preferences. Add a backward-compatible typed field with serde defaults once
  the normal-worktree default is approved.
- `crates/mt-config/src/db.rs:27` documents JSON-per-project storage in
  `config.db`; no new SQL table/manual field map is needed. The sidecar
  `config.json` projection is not the owner of UI settings.
- AppStore project setters live in `crates/mt-app/src/store/projects.rs`.
  `crates/mt-app/src/store/layout.rs:693`, `save_config_now`, enqueues the
  existing writer. The old synchronous-save comment in `env_vars.rs` does not
  describe current behavior. Do not bypass the writer or promise a disk
  acknowledgement the API does not supply.
- `crates/mt-app/src/worktree_catalog.rs:995`, `row_key`, combines host,
  backend/connection, and normalized path, but unavailable fallback keys are
  different. Do not blindly persist temporary unavailable keys. Define a
  durable source/path preference key with existing identity helpers, not
  branch names or row indices; identical paths on different hosts must not
  share settings.
- `root_config_key` at `:813` captures root ID/path/SSH connection. Visibility
  changes should not invalidate scan ownership. Root removal/reconfiguration
  while the dialog is open must reject a stale save.

## Validation and Specs

Required regression coverage: old config deserialization and database
round-trip; save/cancel/restart/refresh; branch renaming and host isolation;
invalid versus offline/last-known state; clicked-project ownership and stale
saves; hidden-row recovery; long-list scrolling; hiding an active worktree
without closing sessions; unchanged raw discovery and exact activation.
Keep the v26.019 and AICOS inventories as distinct fixtures.

Related specs: `.trellis/spec/mt-app/backend/navigation-catalog-contract.md`,
`workbench-identity-contract.md`, `worktree-context-contract.md`,
`project-onboarding-contract.md`, and the worktree catalog/identity contracts
under `.trellis/spec/mt-project/backend/`. The existing navigation spec calls
for rendering invalid rows, so its presentation contract must be revised when
the new policy is approved; discovery and authority contracts stay intact.

Rust compilation/tests, Clippy, formatting, generated i18n, and Windows checks
remain GitHub Actions-only. None ran during this planning research.

## Resolved Product Decision

The user explicitly chose default-show for newly discovered valid worktrees,
with unwanted worktrees hidden manually. Implement an explicit hidden set,
not an opt-in visible list or an Orca ownership-based filter. Missing old
preferences mean no manual exclusions. Invalid entries still stay out of the
default sidebar. The earlier default-hide recommendation is superseded.
