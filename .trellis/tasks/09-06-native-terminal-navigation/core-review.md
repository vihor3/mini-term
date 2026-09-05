# Core State Review

Source-only review of the state/lifecycle/persistence slice on 2026-09-06.
No agent was spawned. No builds, Cargo metadata, tests, fixtures, probes,
lint, formatting, code generation, whitespace checks, CI dispatch, staging,
or commits were performed. Source inspection is not passing CI evidence.

## Findings (fixed)

- File: `crates/mt-app/src/store/mod.rs:486`
  Issue: The round-trip regression did not assert a second snapshot's complete
  serialized stability, and its unrelated second state did not exercise a
  worktree switch despite the test name.
  Fix: Assert byte-identical second serialization, retain the no-shell record
  preservation assertions, and name the test for the behavior actually covered.
- File: `crates/mt-app/src/store/mod.rs:526`
  Issue: Selecting a non-active sibling in a multi-pane legacy leaf was not
  covered by the new single-pane-leaf selection tests.
  Fix: Add a regression for compatibility-pointer updates before snapshot,
  unchanged process identities/order, invalid-selection no-op, and background
  append preserving that selected leaf sibling.
- File: `crates/mt-app/src/store/identity.rs:1115`
  Issue: The authoritative rebind regression did not detect stale flat
  selection/order retained from the provisional state.
  Fix: Seed both old preferences and a distinct destination owner/selection/
  order; assert the destination preferences and next snapshot replace the old
  ones before hydration.
- File: `crates/mt-layout/src/lib.rs:2535`
  Issue: Existing flat-selection fixtures had one pane per leaf, so they could
  not detect failure to synchronize a selected leaf's compatibility pointer.
  Fix: Add two regressions covering a non-active sibling in a non-first owner,
  unchanged remaining trees/records/CWD/AI metadata, idempotent normalization,
  and absent/stale selection preferring the legacy owner over presentation order.
- File: `crates/mt-app/src/store/panes.rs:381`
  Issue: Hydration comments still referred to the removed `set_active_panel`
  entrypoint and implied that GUI restart means the PTY no longer exists.
  Fix: Clarify the retained original-panel dormant recovery policy and the
  separate exact-live no-hydration path. No hydration behavior change remains.

## Findings (not fixed)

### P1: Dormant hosted close can leave an unrepresented live process

- Evidence: `persist.rs:133` restores a saved session/incarnation with no GUI
  `pty_id`. `store/context.rs:557` accepts that exact dormant target.
  `pane_actions.rs:223` revalidates it, then `store/panes.rs:147` disposes only
  when `pty_id` exists and otherwise removes the saved record immediately.
  Nothing in that branch calls the host's fenced close.
- Scenario: With terminal hosting enabled, restart the GUI while a background
  legacy panel's hosted session remains alive. Close its flat tab before that
  panel has been hydrated. The tab and saved record disappear while the host
  still owns the process/history. This is a pre-existing dormant-close gap made
  directly reachable through the complete flat inventory.
- Recommendation: Coordinate an exact dormant-close path using the existing
  session/incarnation-fenced host operation (`mt-terminal-host/src/client.rs:464`),
  without spawning or hydrating merely to close. Decide failure/missing-session
  handling before deleting the record, and preserve binding/alias fences across
  any asynchronous completion. Add an Actions-host lifecycle regression.
- Why not fixed: This crosses the terminal-host lifecycle boundary and requires
  coordinated completion/error semantics, not a mechanical navigation edit.

### P2: Shellless reconnect leaves a record the new close guard rejects

- Evidence: `store/ssh.rs:324` disposes the old attachment before
  `resolve_shell(...)?` at line 358. When all shells have been removed, it returns
  without clearing the pane's old `pty_id`. `store/context.rs:557` rejects a
  `Some(pty_id)` whose entity/route no longer exists. Both close checks in
  `pane_actions.rs:195` and `:223` use this navigation resolver.
- Scenario: Remove all configured shells, then reconnect an exited remote pane.
  The failed reconnect leaves its flat tab visible but unable to open or complete
  the close confirmation. Ordinary dormant hydration also skips the stale
  nonempty attachment handle.
- Recommendation: Resolve the replacement shell before disposing the current
  attachment in `store/ssh.rs`, and cover the failed-reconnect/close path in
  Actions. Do not weaken full-target/live-route validation to mask stale state.
- Why not fixed: The root lifecycle mutation is in `store/ssh.rs`, outside this
  reviewer's allocated files. Main should coordinate the scoped correction.

## Hydration Correction

Main briefly requested selected-only hydration, then explicitly retracted it in
favor of the approved design. The reviewer had applied that request; it was
subsequently undone using `apply_patch` only. The added
`pending_terminal_hydration` helper, selected-only pending inventory, import, and
two associated tests are removed. Avicenna's implementation and the independent
persistence regressions remain.

The retained contract is:

- Ordinary dormant activation, including close into a dormant neighbor, selects
  the original owner first and may recover every eligible dormant record in that
  owner's panel. The existing remote deferral/reconciliation flow is unchanged.
- Exact-live terminal/Agent activation uses existing entities and does not call
  hydration. Inventory reads and background close do not hydrate siblings.
- One-visible-body rendering does not imply selected-only process hydration.
  Main owns the corresponding spec synchronization.

## Other Source Checks

- Flat inventory/order and save normalization retain original owner TabIds,
  PaneKeys, session/incarnation records, full trees, and legacy title metadata.
- Exact terminal activation revalidates the complete target before and after
  project activation; reorder validates both captured targets and edits only
  presentation order. Creation selects before the scheduled snapshot.
- Fork revalidates source identity/session/shell/CWD, selected pane and actual
  focus after CWD lookup, then registers pending lineage before command write.
- Mobile uses background append and retains its SSH/WSL eligibility guards.
  The pure append regressions are not proof of actual desktop focus behavior.
- Latest-dirty-alias ownership and transactional worktree/mirror writes remain
  in their existing owners; no separate flat-navigation persistence writer was
  introduced.
- Source search found no callers of the retained `set_split_sizes`,
  `live_node_ids`, `terminals_panel_visible`, or `toggle_terminals_panel` helpers.
  No obsolete runtime split/group API was reintroduced. Initial focus and other
  view integration remain with the view owner; none of those files was edited.

## Changed Files

This reviewer changed only:

- `crates/mt-app/src/store/mod.rs`: one new test and one strengthened/renamed test.
- `crates/mt-app/src/store/identity.rs`: strengthen the rebind test.
- `crates/mt-layout/src/lib.rs`: two new tests.
- `crates/mt-app/src/store/panes.rs`: hydration-comment clarification only.
- `.trellis/tasks/09-06-native-terminal-navigation/core-review.md`: this handoff.

## Verification

- Lint: UNRUN, GitHub Actions only.
- TypeCheck/build/Cargo metadata: UNRUN, GitHub Actions only.
- Tests: UNRUN. Three new regressions and two strengthened regressions were
  authored; neither these nor the implementer's existing regressions were run.
- Format/codegen/whitespace/native acceptance: UNRUN, GitHub Actions and
  matching Actions-produced artifacts only.
- Remaining execution coverage includes host lifetime, confirmed close/cancel/
  reconnect races, exact-live spawn counts, real worktree/document switches,
  deferred fork write ordering, Mobile focus, and remote epoch behavior. The
  pure state tests do not substitute for those integrated Actions cases.

Main owns lifecycle decisions, integration, specs, Actions evidence for the exact
product commit, and commits. This review does not claim the quality gate passed.
