# Independent Worktree Review

Status: bounded SOURCE REVIEW COMPLETE. Worktree Management and its cleanup
source are RELEASED to main. Every reachable worktree body uses the captured
execution-host backend; this routing gate permits main/Pauli to coordinate the
remote panel guard. No remaining implementation/API blocker in this assigned
slice. This is NOT a passing build, test, native acceptance or full R14 gate.

No local compilation, Cargo metadata, tests, fixtures, probes, lint, formatting,
syntax/whitespace checks, codegen, application execution, automation or Git
writes. No child agents. Main owns Actions, specs and staging. Agent lifecycle
work, four Git UI modules, backend/domain, Files/Tasks and navigation policy
were not edited by this checker.

## Findings Fixed

- P1, `store/git_worktree_cleanup.rs::close_git_worktree_terminals`: the
  detached loop retains dispatched close ownership, but has no confirmation
  lifetime. Cancel during the first dormant host Kill prevents the later Git
  write while still permitting subsequent terminal Kills. The detached worker
  now retains the exact form lifetime and original execution snapshot. It
  finishes an already dispatched close and its exactly observed dormant-alias
  projection, but checks source/lifetime before every additional close. No
  replacement owner is recaptured and no dispatched close task is cancelled.
- P1, `prepare_git_worktree_removal`: the registration lookup intentionally
  matches configured OR trusted canonical path. Destructive cleanup must also
  prove that each matched binding actually owns the target location. A project
  reconfigured to the target while its retained live binding names another
  worktree could supply unrelated terminal-close authority. Each candidate now
  additionally requires a trusted canonical binding at the exact location and
  matching binding project ID. Missing/stale provenance fails closed. The
  central location matcher is reused; ordinary registration is unchanged.
- P1, WSL target projection: `wsl_host_visible_path` alone accepts literal POSIX
  backslashes and rewrites them as UNC separators. A Git target such as
  `/srv/repo\child` could therefore register/close the distinct `/srv/repo/child`
  project. The owned projection and binding predicate now consume the existing
  checked `DirectoryLocation::host_path` boundary; unrepresentable native names
  are explicit capability errors before registration/terminal cleanup. No shared
  execution-host/browser edits and no new path parser.
- P2, cleanup snapshots: `ProjectConfig` skips `saved_layout` during serde, so
  serializing the whole configuration did not actually fence that field. The
  guard now separately retains runtime layout and configured saved layout.
  `expected_alias_removal` projects only the exact removal into both, preserving
  other records and selection. Every awaited close/projection is compared
  against that retained result; false close hints cannot authorize cleanup.
- P2, `actions.rs::register`: Open/Add/Switch and add-after-create previously
  resolved existing aliases only at completion. They now capture matching alias
  inventory, configuration order and execution source before yielding. Changed,
  new or reordered aliases and replaced sources cannot become that activation
  target. Running/reconciling/uncertain common-directory writes also block
  registration, so a fresh row cannot bypass uncertainty via Open/Add/Switch.
- P2, create completion: inputs remain editable while Git runs. Successful
  creation could register and close the manager over a newer draft. The
  submitted draft now independently owns automatic registration/close; newer
  drafts remain visible with the verified created path and an explicit skipped
  registration result. No automatic mutation replay was introduced.
- P2, legacy discovery and branch/base menus: discovery ignored a stale caller
  folder when `discover_repos=true`, and an old branch menu could apply to a
  refreshed or edited form. The legacy entry now compares its selector to the
  current configured host path before opening. Menu choices retain refresh
  generation and picker/form request at both menu-open and item-click boundaries.
- P2, manager close: the old kind-only handler could close a successor dialog.
  Following Pauli's published private pattern, the manager now separates source
  lifetime from `view_lifetime`. Source invalidation leaves the visible manager
  closable; close/on_close/Drop dispose both. Programmatic close checks the exact
  instance token and top overlay, then invalidates before removal. A nested
  overlay leaves the tokens untouched. No shared `host_ui` signature changed.

## Reachable Action Pass

| Surface | Source closure |
| --- | --- |
| Legacy / selected-repository open | Snapshot at entry, retained selector or exact authority, independent modal lifetime; no delayed active-project fallback |
| Inventory / Refresh / branches | Detached host reads, producing request and modal generation, pinned source/epoch; failed/LastKnown rows inert, native fallback read-only |
| Existing/new branch / base | Captured local ref and selected base object per repository; occupied branches and ambiguous local/remote names refused; no checkout |
| Destination / browser | Captured form/source/request; Local, owning WSL distro or exact SSH context; no remote client filesystem probe |
| Create / multi-create | Prepared single-use writes per original repository; exact normal WorktreeCreated proof; captured aliases/draft for registration; collisions/errors retained |
| Open Terminal / Add / Switch | Fresh owned inventory, exact row, captured aliases and central ChildWorktree registration under original root; returned project/worktree pair and host CWD |
| Remove / Force / Review Target | Git intent and full store guard before confirmation; force/review explicitly starts a new preflight; exact close requests and source/lifetime revalidation |
| Prune | Backend authoritative inventory and host absence; only captured empty guards for exact proved paths; no terminal close or client absence helper |
| Uncertain review | Exact original operation ID plus explicit stopped-process acknowledgement; read-only reconciliation, no replay/registration/configuration cleanup |

Read the Git check.jsonl, PRD/design/implement, backend-review, ui-handoff and
worktree-handoff plus Git host, worktree/workbench identity, onboarding, catalog,
quality and domain contracts. Reviewed backend dispatch/reconciliation, existing
terminal close/pending-token/navigation and layout-save code read-only to check
the cross-layer assumptions.

Cleanup preserves complete terminal identities. It deduplicates only exact
same-session alias closure, prefers the attachment and closes selected last.
Dormant aliases follow only an exactly observed expected source-record removal,
never a second host Kill for history. New aliases, other attachments, divergent
layouts, dirty documents, pending closes, orphan routes, missing runtime buckets
or configured children outside the deletion group refuse cleanup. Current empty
guards plus normal typed Git postconditions are required before synchronous
configuration removal. Uncertain or changed-owner completions cannot finalize.

Ordinary central project activation keeps its existing hydration behavior.
Removal adds no hydration, broad PTY disposal or panes/layout/navigation policy
change. A dispatched close may complete after Cancel, but no later close or Git
write is authorized by that cancelled form. Existing effects are not rolled back.

## Final Cleanup API

```rust
pub(crate) fn close_git_worktree_terminals(
    &mut self,
    guard: GitWorktreeRemovalGuard,
    source: ProjectExecutionSnapshot,
    lifetime: GitLifetime,
    cx: &mut Context<Self>,
) -> Task<Result<GitWorktreeRemovalGuard, String>>;
```

The sole owned caller supplies
`prepared.repository().backend().snapshot().clone()` and the exact confirmation
form's lifetime. Other guard/location APIs and public worktree entry/path/branch
helpers keep their signatures. `store/projects.rs` only makes the existing
matcher visible to its sibling cleanup module; the trusted-binding predicate is
private to destructive cleanup. No panes accessor or shared host_ui change.

## Actual Changed Files

Checker edits in this pass:

- `crates/mt-app/src/git_worktree.rs`
- `crates/mt-app/src/git_worktree/actions.rs`
- `crates/mt-app/src/store/git_worktree_cleanup.rs`
- `crates/mt-app/src/store/projects.rs`: matcher visibility only
- `.trellis/tasks/09-06-native-remote-git/worktree-review.md`

Mill's pre-existing location lookup and `store/mod.rs` module/export are retained
unchanged. Agent fields/test initializers and all peers' UI/backend/module-line
changes are preserved. No `panes.rs`, shared host_ui, specs, workflows, task
metadata or main edits; no staging/commits.

## Tests Authored, All UNRUN

Ten new production-helper regressions:

- `cleanup_requires_the_bound_location_not_a_repointed_configured_alias`
- `cleanup_bound_wsl_path_keeps_distribution_case_and_native_host_separate`
- `cancelled_or_changed_removal_owner_cannot_dispatch_the_next_close`
- `predicted_close_updates_runtime_and_skipped_config_layout_without_mutating_inventory`
- `creation_completion_cannot_register_or_close_over_a_newer_draft`
- `registration_keeps_captured_alias_order_source_and_uncertain_exclusion`
- `source_invalid_manager_can_close_but_disposed_or_covered_instance_cannot`
- `discovery_entry_rejects_repointed_project_and_foreign_wsl_selector`
- `worktree_registration_rejects_wsl_names_that_would_retarget_native_paths`
- `branch_menu_choice_cannot_adopt_a_refreshed_or_edited_form`

The existing guard-mismatch test now includes the separately captured saved
layout. Mill's eleven new tests and existing path/branch/group tests remain.
These source assertions are not GPUI scheduling, actual host Kill, dirty-document,
SSH or WSL execution evidence.

## Remaining Findings And Gates

- Main separately owns Pauli's backend-connect finding: captured Some(epoch)
  must be rejected before connect's replacement-session anchor read. Worktree
  checks returned source before discovery/publication, but cannot undo that
  earlier backend read. No backend edits here; this report does not certify the
  separately coordinated correction.
- Conservative cases: a native prepared create path canonicalized to a different
  captured registration location retains the verified created path without
  automatic registration. Missing binding authority, conflicting aliases and
  retained terminal history require explicit reconciliation, not guessed
  absence. Unrepresentable WSL UNC names are explicit capability errors.
- Existing backend limits remain: original-source recovery, bare-survivor and
  nested-repository Force restrictions, WSL/SSH inspection capabilities,
  process-local leases and external filesystem/Git races. No inode-atomic
  transaction, persistent restart journal or guaranteed remote rollback is
  claimed. The legacy path/project entry first captures its full host at
  invocation; no earlier caller/menu source API was introduced.
- Main: exact-head Actions compilation, Clippy, formatting, whitespace and
  focused/full tests, including Windows branches and relevant locked root/sidecar
  graphs. Prior domain-only runs do not cover this uncommitted app/store delta.
  No verification command was run locally.
- Actions/native acceptance: multi-terminal cancellation during a dormant Kill;
  failure-to-success rejection; alias projection/shared-save conflicts;
  selected-last/no-neighbor hydration; dirty docs introduced during preflight,
  close or Git; replacement PTYs/config/bindings; repository/project A-B-A;
  close/reopen/nested overlays; epoch loss before/after dispatch; force/remove/
  prune with unknown absence; and uncertainty across SSH aliases/common dirs.
  Use disposable Actions fixtures and matching-SHA Windows/WSL/SSH artifacts,
  not user repositories.
