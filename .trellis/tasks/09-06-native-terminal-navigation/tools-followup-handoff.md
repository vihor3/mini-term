# Search Tools Follow-Up Handoff

Scoped implementation on 2026-09-06 for the two tools/settings P2 findings in
`view-review.md`. No subagents were spawned. Main retains ownership of specs,
task metadata, commits, CI, and the focused follow-up review.

## Files Changed

- `crates/mt-ui/src/terminal/search_bar.rs`: retain one `Entity<IconTooltips>` in
  `TerminalSearchBar`, created only in its constructor. The existing `with_tip`
  wrapper now delegates to `IconTooltips::button`, with the same six stable
  anchor/button IDs and localized descriptions. One `IconTooltips::group` wraps
  all six tools and their gaps; the input and counter stay outside that group.
  There is no GPUI `.tooltip`, `Tooltip::new`, or second `.on_hover` in this path.
- `crates/mt-app/src/settings/pages_terminal.rs`: remove only the obsolete
  terminal-animation value read and toggle row, including its patch callback.
  Scrollback, shell editing, and clipboard settings are unchanged. The
  `toggle_row` import remains necessary for the clipboard controls.
- `.trellis/tasks/09-06-native-terminal-navigation/tools-followup-handoff.md`:
  this report. No other file was written by this follow-up implementer.

## Ownership and Behavior

- The shared owner supplies the existing 500 ms first hover, immediate next-icon
  descriptions after warming, gap retention, generation checks, and native
  tooltip placement. No shared API, timer, or focus-handle implementation changed.
- `close` resets descriptions before disabling search and emitting callbacks.
  `focus_input` resets before focusing/selecting input, also covering `open` and
  repeated programmatic focus requests. Shared group/anchor teardown and focus,
  window, pointer, click, key, scroll, and occlusion handling cover other resets.
- Each bar keeps its original immutable search/emulator references. It is not
  rebound to another terminal, and its tooltip owner is not shared across panes.
  Hiding/removing the bar releases the group's mount lease; dropping the bar
  drops its owner. No render-time replacement of the retained owner was added.
- Button labels, sizing, selection/disabled rules, command callbacks, input
  width, counter/error styling, and outer `TerminalSearch` key context remain.
  The new group has the same inter-button gap and adds no focus stop. Existing
  input focus, Enter/Shift+Enter/Escape handling, query/options persistence,
  read-only/preedit behavior, search events, and host callbacks are untouched.
- The saved `terminal_animations` field, serde defaults, and compatibility copy
  remain untouched. No file/project/document animation setting or implementation
  was changed. No translations or generated dictionaries were edited.

## Regression Coverage

- Extended the existing search-bar bilingual caption test with English whole
  word, regex, Previous (Shift+Enter), and Next (Enter) assertions. All six tool
  captions now have English and Chinese assertions. The existing counter test
  remains unchanged. These assertions cover captions, not native interaction.
- Source-reviewed the existing `icon_tooltip.rs` cold/warm, gap, stale timer,
  reset, anchor removal, focus rejection, and placement tests. They are reused,
  not duplicated into the search-bar module or claimed as integration coverage.
- No GPUI test-support dependency or source-string pseudo-test was added.

## Verification and Follow-Up

Only source reading, manual code editing, and read-only Git status/diff review
were performed. Builds, Cargo metadata, tests, fixtures/probes, lint, formatting,
whitespace checks, generators, app launches, and automated verification were
NOT RUN. There is no CI success or native-acceptance claim for these changes.

Main should run the existing Actions gates for the exact integrated commit and
coordinate the focused check. Actions/native coverage must exercise first-hover
timing, prompt movement across every icon and gap, leave/reentry, pending/warm
close/reopen, programmatic refocus, worktree/document switches, occluding menus,
window deactivation, and late timers after removal. Also cover mouse/keyboard
search commands, input focus after click release, read-only and IME/preedit,
invalid regex/no-query states, query retention, and narrow/high-DPI placement.
Native acceptance must use the matching Actions-produced artifact. Confirm the
settings page has no animation toggle while saved legacy configuration survives.

No scoped implementation blocker remains. Native hit testing, timing, layout,
and compilation are still unverified; the caption/reducer tests do not prove
those boundaries. Lifecycle findings and their owner's files were not changed.
