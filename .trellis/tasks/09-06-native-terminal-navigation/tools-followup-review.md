# Search Tools Follow-Up Review

Reviewed `tools-followup-handoff.md` and the current two-file diff. No subagents,
Git mutations, local automated verification, app launches, or CI dispatches.
Main retains ownership of specs, commits, Actions, and unrelated lifecycle work.

## Findings (fixed)

- File: `crates/mt-ui/src/terminal/search_bar.rs:419`
- Issue: `with_tip` wrapped each Button in a default block Div. The shared
  tooltip API appends a full-size absolute canvas without explicit insets.
  GPUI 0.2.2 defaults Divs to block layout; Taffy 0.9.0 places that canvas at
  the static position after the in-flow Button, below its visible bounds.
  The wrapper's hover starts the delay, but the misplaced anchor hitbox rejects
  it, preventing descriptions from appearing over the actual search controls.
- Fix: Use one private `tip_anchor` builder with non-growing/non-shrinking,
  centered flex layout. The absolute anchor overlays the child Button without
  changing its IDs, sizes, disabled logic, handlers, or focus ownership. Added
  `tooltip_anchor_uses_flex_alignment_for_the_absolute_overlay`, a pure style
  contract test for the production builder. No shared API changes.

## Findings (not fixed)

No additional concrete defects identified in the scoped source. Native event
delivery and measured geometry remain unverified, not approved by these tests.
Unrelated lifecycle findings were neither changed nor awaited.

## Handoff

- Reviewer changed only `search_bar.rs` and this report. The settings page
  required no further edits. No navigation/lifecycle or shared tooltip files
  were written.
- One tooltip owner remains constructor-created per bar; the six stable
  wrappers and their gaps share one group. Input/counter remain outside it.
  No GPUI/Button tooltip builder or second timer is installed. Close and
  programmatic input focus reset the owner; removal uses the shared mount lease.
- Pinned gpui-component 0.5.1 Button source has no occluding hitbox, preserves
  input focus on mouse-down, and rejects disabled callbacks. Normal tooltip
  hitboxes add no click handler or focus stop. The host's existing search-wrapper
  mouse-down fence still prevents terminal-body focus capture.
- Enter/Shift+Enter/Escape, input width, option selection, no-query disabled
  rules, counter/error styling, and query/options persistence are unchanged.
  Settings removes only the obsolete animation control. The optional saved
  `terminal_animations` field and serde/default handling remain in mt-config;
  shell, scrollback, and clipboard controls are retained.
- Pinned source evidence was read from published gpui 0.2.2 (`style.rs`,
  `styled.rs`), gpui-component 0.5.1 (`button/button.rs`), and Taffy 0.9.0
  (`compute/block.rs`, `compute/flexbox.rs`) archives. No dependency commands or
  layout probes were run. Main may document the flex-wrapper requirement.

## Verification

- Lint: NOT RUN; Actions only.
- TypeCheck/build: NOT RUN; Actions only.
- Tests: NOT RUN. Search-bar module now has three pure cases: the new wrapper
  style contract, retained counter cases, and the implementer's expanded
  bilingual captions. These do not simulate timers, input, or native geometry.
- Formatting, whitespace, codegen, UI fixtures, and native acceptance: NOT RUN.
  Main must provide exact integrated-commit Actions evidence and matching
  artifact acceptance for all six tooltip hitboxes, 500 ms first/warm-next
  timing, gaps, disabled clicks, input focus after release, shortcuts, IME,
  regex errors, close/reopen/refocus/unmount, window deactivation, and narrow/
  high-DPI placement. Legacy saved config should survive with no animation
  toggle displayed. No local or unrelated CI result is claimed as validation.
