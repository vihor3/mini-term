# Codex Screen Review, 2026-09-07

## Review Status

Approved for GitHub Actions source verification after the second final handoff.
All six product diffs, the Windows suite gate and updated contracts were reviewed.
The original and follow-up findings below are resolved in source. No remaining
source-level blocker found in this scoped Working-evidence repair.

No product, spec, workflow, staging, commit or push changes were made by this
reviewer. Implementation owns the fixes; this review edited only the permitted
task note. Final product ownership returns to main with this report. Source
approval is not passing CI, installer or native acceptance evidence.

Loaded the full saved hook output, `check.jsonl`, its source/spec/research
references, PRD/design/implementation plan, parent PRD, applicable package
indexes and inventory contract. All findings are source reasoning, not executed
reproductions. Line numbers refer to the evolving worktree and may move.

## Findings (fixed)

None fixed by this reviewer. Schrodinger owns the following source corrections,
now independently reread with their authored (unrun) regressions:

- File: `crates/mt-app/src/pane.rs`, `crates/mt-app/src/store/remote_agents.rs`.
  Issue: completing split ANSI lost the pre-frame valid evidence and refreshed
  retained text. Fix: preserve `incomplete_baseline`; the new
  `codex_split_controls_footer_and_spinner_do_not_renew_retained_working` test
  exercises pre-owner Working text plus split SGR through the real VT/registry
  path. The original visible-cursor trace is addressed.
- File: `crates/mt-ai/src/agent_semantics.rs`, `crates/mt-app/src/pane.rs`.
  Issue: footer and spinner changes refreshed an unchanged counter. Fix:
  normalize the optional spinner prefix and compare the status marker,
  excluding footer. The same production-path regression asserts no renewal.
- File: `crates/mt-terminal/src/snapshot.rs`.
  Issue: the new parser completion method was missing. Fix: add the narrow
  `ParserState::in_ground_state` query. Real-emulator partial ANSI/UTF-8/sync
  regressions are authored; compilation has not run.
- File: `crates/mt-terminal/src/snapshot.rs`, `crates/mt-terminal/src/lib.rs`.
  Issue: the bounded parser tail reset to Ground on overflow, which the new
  query treated as completion. Fix: sticky `overflowed` prevents inspection
  until parser reset. The real-emulator regression covers oversized Sync/OSC,
  rejection after terminator, and parser-reset recovery. This resolves the
  reported P2 source boundary; the regression has not run.
- File: `crates/mt-app/src/pane.rs`, `crates/mt-terminal/src/lib.rs`,
  `crates/mt-ai/src/agent_semantics.rs`.
  Issue: showing/moving the cursor could turn an invalid before-context into
  newly timestamped retained Working. Fix: `CodexMarkerBaseline` retains raw
  normalized markers independently of cursor/composer validity; `first_row`
  and `marker_row` identify the same active-grid row for comparison. An
  uncovered before-row cannot prove change. The real VT-to-registry test
  `codex_cursor_only_validity_changes_do_not_refresh_retained_marker_cells`
  covers hidden/show, nearby cursor movement and movement outside the bounded
  window, then proves a genuinely changed counter remains accepted.
- File: `crates/mt-app/src/pane.rs`, `crates/mt-app/src/store/remote_agents.rs`.
  Issue: ownership loss retained old unfinished-frame authority across a
  same-run rebind. Fix: `bind_owner` clears pending, incomplete owner/baseline
  and frame state on identity loss/change. The production-path test
  `codex_owner_loss_invalidates_unfinished_frame_before_same_run_rebind`
  completes output while unowned, rebinds, rejects split SGR renewal and then
  accepts a new counter. Original marker non-renewal history remains separate.

## Findings (not fixed)

None remaining in the reviewed scope. All verification execution remains
pending Actions; unsupported layouts/provider state markers intentionally stay
Unknown or freshness-qualified last-known rather than inventing completion.

## Evolving Corrections Observed

The implementation retains the earliest pending Working capture through incomplete
frames and refuses publication mid-frame. The authored
`codex_partial_counter_keeps_earliest_capture_but_cannot_confirm_mid_frame` and
rapid-counter tests assert original freshness after a confirming later sample.
Absent/contradictory completed frames clear pending without fabricating another
state. The producer also retains a pre-erase marker to reject identical split
repaint. None of these tests has run.

The positive `codex_native_frame_reaches_bracketed_registry_and_accepted_projection`
regression uses actual Alacritty VT input, the production screen observer,
foreground owner resolution, inventory application, `apply_codex_screen`,
`observe_semantic`, and accepted projection. It rejects a late pre-capture
request, accepts a strictly later scheduled request, and preserves last-known
Working as stale after an idle composer instead of inventing Waiting/Done.
This is materially stronger than a direct fabricated semantic-state test.

Production integration holds the reader evidence mutex through synchronous
semantic acceptance, allocates the event sequence before that lock, and feeds
accepted projection afterward. SSH terminals use the legacy transport where
the new observer is attached; hosted replay remains outside this producer.
Background routes still come from all registered terminals, not active UI tabs.
Independent process and Hook-priority cases are authored. No added source issue
found in those ownership/acceptance paths during this follow-up.

## CI And Contracts

The new Windows gate lists each filtered mt-ai/mt-app suite and fails on empty
discovery before executing it; mt-terminal is run as the whole library suite.
No source-level workflow blocker found. Linux retains workspace all-targets
check, Clippy, tests, formatting and whitespace gates. These are configured
gates, not passing results. Main owns workflow and exact-commit evidence.
The published `AgentScreenRow`/`codex_screen_evidence` signatures and mt-app
producer API names match the reviewed code. No additional spec/gate correction
requested at this handoff.

The new runtime/reconciliation contract language preserves Hook priority,
Working-only Codex evidence, current-grid independence from display offset,
strictly later scheduled ownership confirmation, original capture time,
counter-starvation prevention and absent-marker invalidation. No layout or
device-wide ownership expansion is authorized or needed.

## Verification

- Lint: not run; Actions-only, main-owned.
- TypeCheck: not run; Actions-only, main-owned.
- Tests: not run; authored/evolving tests are not passing evidence.
- Native acceptance: not performed; requires the exact Actions-produced Windows
  artifact and user acceptance separately from source/automated review.
- Final current six-file diff: reviewed read-only after main's source-ready
  signal and second final product handoff. Approved to proceed to exact-commit
  Actions. All 18 new regression functions remain under the configured existing
  suite filters; no test execution or passing automated/native result is claimed.
