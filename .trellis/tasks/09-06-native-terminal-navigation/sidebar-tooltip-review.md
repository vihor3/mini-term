# Sidebar and Tooltip Review

Scope: R5/R6/R7/R10, source review only. No agents dispatched, Git mutations,
local verification, app launch, CI dispatch, or task/spec metadata edits.

## Findings (fixed)

- `crates/mt-app/src/orca_sidebar.rs:1177`: Explicitly tracked project-row and
  ellipsis focus handles defaulted to non-tab-stops. GPUI 0.2.2 applies a Div's
  `tab_index`/`tab_stop` settings only when creating an implicit handle, so these
  controls were skipped by keyboard navigation. Both retained handles now set
  `tab_stop(true)` at creation, preserving the hover/focus/open-menu policy.
- `crates/mt-ui/src/icon_tooltip.rs:268`: Group reset depended on GPUI's
  mouse-move-only `on_hover`. Window exit, a moved/clipped group, or occlusion
  without another mouse move could retain a warm sequence, especially in an
  inter-icon gap. Added a nonblocking group hitbox, prepaint bounds/focus checks,
  paint-time occlusion checks, and capture-phase `MouseExitEvent` reset. Existing
  generation guards, weak anchor ownership, and unmount subscriptions remain.
- `crates/mt-ui/src/icon_tooltip.rs:321`: Draw-time resets called `refresh`, which
  GPUI ignores while drawing. Reset now defers the clearing refresh until the
  effect cycle completes. Added a pure warm-gap reset case at line 509; this
  checks reducer state, not native event delivery.

## Findings (not fixed)

- P2, core-owned `crates/mt-app/src/title_bar.rs`, `render_tab`: The retained
  `tab_focus` handles still use plain `cx.focus_handle()` followed by
  `.track_focus(&focus).tab_index(0)`. This has the same non-tab-stop defect as
  the sidebar. The core owner should set `tab_stop(true)` on those handles and
  cover keyboard tab navigation. Left untouched because another agent owns it.
- Native wiring coverage remains pending. Pure cases cannot establish keyboard
  reveal/activation, mount-release callbacks, window-exit delivery, draw-time
  clearing, or measured long-label/high-DPI placement. Main should add/coordinate
  Actions-only GPUI integration coverage and matching artifact acceptance.
  No test-support dependency or other public interface was changed here.

## Source Handoff

- Reviewer changed only `orca_sidebar.rs`, `icon_tooltip.rs`, and this report.
  Reviewed `activity_bar.rs`, `menu.rs`, `tooltip.rs`, and `mt-ui/src/lib.rs`
  without further edits. Other owners' ongoing changes were retained.
- `IconTooltips::{button, group, reset}` signatures are unchanged. Direct
  `AnyTooltip` submission adds no second GPUI delay. Initial delay remains 500 ms;
  ordinary `Tooltip` keeps its existing 700 ms extra delay and instant semantics.
- ActivityBar reducer re-export remains source-compatible. Project Settings
  captures the root target and its existing open path revalidates it. Menu
  identity keeps the trigger visible while open; bounds anchor at its bottom.
  Footer commands remain Usage/Settings/Mobile with stable icon sizes; Mobile
  is absent from the top menu. The core's `main.rs` OpenMobile arm is present.
- GPUI 0.2.2 public APIs and callback phases were read from the published pinned
  source archive because the local cached source directories contained no files:
  https://static.crates.io/crates/gpui/gpui-0.2.2.crate
  Relevant source: `window.rs` focus handles, keyed state, tooltip prepaint,
  hit testing, refresh/defer; `elements/div.rs` explicit focus and hover handlers;
  `elements/canvas.rs`, `app/context.rs`, `app.rs`, and `interactive.rs` callbacks.
- Main owns spec synchronization: document explicit tracked-handle tab stops,
  window-exit handling, and deferred draw-time invalidation if adopted as shared
  conventions. No spec edits were made by this reviewer.

## Verification

- Lint: NOT RUN; GitHub Actions only.
- TypeCheck/build: NOT RUN; GitHub Actions only.
- Tests: NOT RUN; this slice has 15 newly authored pure cases including the one
  added here, plus retained existing cases. None were executed by this reviewer.
- Formatting, whitespace checks, codegen, UI fixtures, and native acceptance:
  NOT RUN. Main must provide exact integrated-commit Actions evidence and use
  its produced artifact for acceptance. Static source review is not passing CI.
