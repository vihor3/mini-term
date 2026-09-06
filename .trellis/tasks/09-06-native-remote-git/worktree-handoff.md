# Worktree Integration Handoff

## Ownership And Current Status

Source implementation is COMPLETE and RELEASED to main for independent review.
Every reachable Worktree Management action now routes through the captured
execution-host backend; no old local Git mutation or filesystem absence body
remains. The `open_repository` signature and all public legacy helpers remain.
Main/Kepler can now coordinate the panel guard with the independent review gate.
This is source completeness, NOT passing R14 acceptance or CI evidence.

No compilation, Cargo metadata, test, fixture, probe, syntax/lint/format,
generation, whitespace check, application verification, Git write, or SSH
mutation was run locally. Main owns Actions, specs, staging and review. The
reported domain-only Linux Actions success does not validate this uncommitted
UI/store slice.

Write boundary: `git_worktree.rs` and private children/tests; a new
`store/git_worktree_cleanup.rs`; only its declaration/export in `store/mod.rs`;
a narrow location-matcher wrapper in `store/projects.rs`. No `store/panes.rs`
change was needed: the existing request type is visible to its sibling store
module. Bohr's fields, project test initialization and TitleChanged branch were
preserved exactly. No other owner's UI/backend/browser paths were edited.

## Actual Shared APIs Being Consumed

- Keep `open_repository(GitRepository, on_changed, window, cx)` and legacy
  `open(repo_path, discover_repos, project_id, on_changed, window, cx)` plus all
  public path/branch helpers. The legacy wrapper snapshots the project once.
- Use `GitBackend::connect/discover`, `GitRepository::request` with Worktrees
  and Branches, `prepare_write/execute`, verified postconditions, and explicit
  `review_uncertain(UserConfirmedOriginalOperationStopped)` on the captured ID.
- Use `DirectoryPickerOptions` with captured Local/WSL/SSH host and epoch.
- Use central `ProjectLocationKey` and `ChildWorktree` registration under the
  captured snapshot root. No path-only lookup or later active-project fallback.
- Implement the opaque cloneable `GitWorktreeRemovalGuard` and all five
  requested store methods from `ui-handoff.md`, plus `project_ids_for_location`.
- Initial connect uses Kepler's actual `host_ui::read_source_matches`; only
  unobserved SSH bootstrap may adopt an epoch. A pinned epoch cannot advance.

## Completed Action Paths

- Inventory and branch reads retain request IDs, host/source and modal refresh
  generation. Failed/LastKnown inventory retains visibly unavailable rows.
  Fresh Local Libgit2Fallback remains readable, never mutation authority.
- Existing/new branch selection, local/remote base tips, literal destination
  input, host-specific suggestions and the shared folder picker are routed.
  Picker requests are superseded by form edits, mode/selection changes, refresh,
  writes and close. WSL selection rejects other distributions or client paths.
- Create uses prepared writes, explicit per-repository results and exact normal
  WorktreeCreated postconditions. Changed drafts are retained; no automatic
  replay or registration after uncertainty. Multi-target collisions fail early.
- Open Terminal and Add/Switch perform a fresh captured inventory read, then
  central host-qualified ChildWorktree registration/activation. Terminal CWD
  uses the returned host-visible path, including WSL UNC launch routing.
- Remove prepares Git intent and store guard before confirmation. Force or
  explicit Review Target creates a new preflight; OK consumes the exact prepared
  request once. Closure and finalization revalidate the retained guard.
- Prune only finalizes backend-proven absent paths with captured empty guards.
  It never closes terminal records or uses a local filesystem postcheck.
- Busy/uncertain coordinator state is visible. Explicit stopped-process review
  uses the captured operation ID and never registers or cleans configuration.
  Failed/cancelled removal results refresh only the still-live original manager.

## Cleanup Design And Review Focus

The guard retains the exact host-qualified location, all matching aliases,
configuration, binding, connection source, saved layout, and terminal requests.
Dirty documents, changed/new aliases, pending close, ambiguous attachment,
changed source, or unexpected terminal changes refuse cleanup.

Reuse `terminal_close_request/close_terminal_target`; its returned bool means
focus handoff, not successful removal. Observe exact target absence after each
await and compare all other records against the retained inventory before
advancing. Close background records before the selected terminal to avoid
hydrating a dormant neighbor. An owned worker must outlive modal disposal.
Identical dormant alias records may only follow an exactly confirmed logical
session removal; never dispatch a second host kill or adopt an unexplained
alias change as an owned removal. Any such projection remains inside the new
guard module and requires independent review.

The implementation predicts only the source's exact record removal, then
compares configuration, binding, source, serialized layout and remaining full
terminal targets after the awaited close. Only an equal result permits dormant
alias projection. Every alias projection is compared again before continuing.
The original confirmed source remains the shared layout-save owner. Pending
closes, another attachment, missing runtime inventory, orphan routes, unrelated
aliases sharing WorktreeId or configured children outside the removal group
fail closed. `TerminalCloseRequest` fields were not exposed or reimplemented.

Additional opaque guard methods consumed only by this UI are
`terminals_empty()` and `matches_location(&ProjectLocationKey)`. The latter
rejects a native prepared target that canonicalized to a different location.
Normal finalization runs synchronously over the already validated empty group;
it does not reinterpret the close task's focus-handoff bool as success.

UI captures guard and prepared Git intent before confirmation, revalidates
before terminal closure and Git dispatch, and finalizes only a normal verified
postcondition for the same live dialog. Uncertain writes retain configuration
and quarantine; explicit review is not registration authority. Prune can only
finalize backend-proven absent paths whose captured terminal inventory is
already empty. No local filesystem absence check and no broad PTY disposal.

## Changed Files

- `crates/mt-app/src/git_worktree.rs`: retained scaffold, host-aware modal/read
  flow, row/form routing, literal host paths, picker fences, focused tests.
- `crates/mt-app/src/git_worktree/actions.rs`: new private action implementation,
  remove confirmation lifecycle, registration, prune/review, focused tests.
- `crates/mt-app/src/store/git_worktree_cleanup.rs`: new opaque cleanup guard,
  exact sequential close ownership, empty finalization and focused tests.
- `crates/mt-app/src/store/mod.rs`: module declaration and opaque guard export
  only, alongside untouched pre-existing Agent changes.
- `crates/mt-app/src/store/projects.rs`: location lookup wrapper reusing the
  central matcher and onboarding canonical authority, no separate normalization.
- `.trellis/tasks/09-06-native-remote-git/worktree-handoff.md`: this report.

## Tests Authored, All UNRUN

Eleven focused new tests, plus the existing public path/branch/group tests:

- `remote_path_helpers_preserve_case_and_literal_backslashes_on_every_client`
- `registration_locations_preserve_ssh_connection_and_wsl_distribution`
- `fresh_libgit2_fallback_is_readable_but_never_mutation_authority`
- `failed_inventory_retains_rows_with_explicit_error_and_no_row_authority`
- `only_normal_verified_completion_can_authorize_registration_or_cleanup`
- `directory_selection_keeps_wsl_distribution_and_local_ownership`
- `guard_rejects_location_alias_configuration_binding_layout_and_epoch_changes`
- `only_the_expected_removal_can_advance_a_retained_inventory`
- `dormant_alias_projection_rejects_every_replacement_route_field`
- `close_order_deduplicates_aliases_prefers_the_attachment_and_leaves_selected_last`
- `predicted_background_removal_preserves_selection_and_never_hydrates_records`

The existing backend's disposable worktree add/remove/prune, source-removal,
authenticated loopback SSH and uncertain-review fixtures remain the Git effect
coverage. No second fixture framework/setup was added. These pure guard tests
are not real GPUI close timing, host Kill, dirty-document or SSH evidence.

## Required Review And Gates

Main: please dispatch the independent Trellis check now. Prioritize shared-alias
projection after exact close, selected-last ordering, no cancellation of the
detached store worker, normal-vs-uncertain finalization, source removal, dialog
close/reopen, and all captured request/epoch fences. No blocker requires another
owner API change. All Actions formatting/compilation/lint/test checks and
matching-SHA artifact/native Windows/WSL/SSH acceptance remain required.

Additional review/acceptance should exercise project/repository A-B-A, modal
cancel while dormant Kill is pending, new/reconfigured aliases or documents,
replacement PTYs, missing/unavailable host history and prune with remaining
records. Ambiguous base branch display names are refused instead of selecting
the first local/remote collision. Configured child dependencies and divergent
alias layouts require user reconciliation before destructive cleanup. Remote
filesystem replacement and external Git races retain the backend's documented
limits; no atomic rollback or process-stop proof is inferred from a read.

New diagnostic/review strings currently follow the Git integration's English
error style. Main owns any additional translation keys and Actions generation.
