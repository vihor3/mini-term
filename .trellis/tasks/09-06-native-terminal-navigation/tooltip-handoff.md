# Shared Icon Tooltip Handoff

Sidebar/hover implementer owns `mt-ui/src/icon_tooltip.rs`, its lib export and
shared bubble styling in `tooltip.rs`, `activity_bar.rs` timing extraction,
`orca_sidebar.rs`, and the small anchored-menu addition in `menu.rs`. Core may integrate
the API below now; source implementation is complete, with the same signatures.
No local execution is allowed.

## Integration API

```rust
use mt_ui::icon_tooltip::IconTooltips;

// One retained entity per contiguous icon group, created with the owner.
icon_tooltips: Entity<IconTooltips>,

// In the owner's constructor; no Window argument is needed here.
let icon_tooltips = cx.new(|_| IconTooltips::default());

// In render. Keep the existing button's ID, geometry, icon and click handler.
// The tooltip key must be stable and unique in this view.
let add = IconTooltips::button(
    &self.icon_tooltips,
    "terminal-add-description",
    t("terminalArea", "newTerminal"),
    div().id("terminal-add").child(add_icon).on_click(add_handler),
    window,
    cx,
);

// Wrap the entire group, including gaps, so leaving it resets the sequence.
let toolbar = IconTooltips::group(
    &self.icon_tooltips,
    div().id("terminal-tools").child(add),
    window,
    cx,
);
```

Both wrappers return `Stateful<Div>`. `button` accepts `key: impl Into<ElementId>`
and `description: impl Into<SharedString>`. `group` accepts an existing
`Stateful<Div>`. Neither wrapper changes dimensions or takes focus. Do not add
GPUI `.tooltip(...)` or a second `.on_hover(...)` on those same elements.
Ordinary click/key handlers remain owned by the caller.

The shared implementation retains the Activity Bar's 500 ms first-hover delay
and immediate next-item behavior while inside the group. It owns the timer,
generation checks, anchor lifetime, focus/window-deactivation reset, and
non-intercepting window-level tooltip rendering. Clicks and scrolling reset it.
There is no second `.tooltip` delay and no app-wide default-tooltip change.

For an explicit owner transition or opening a menu programmatically:

```rust
IconTooltips::reset(&self.icon_tooltips, window, cx);
```

`activity_bar::{HoverSession, HoverEnter, HOVER_SHOW_DELAY}` stays source-compatible
through a re-export of the shared timing reducer. Existing Activity Bar consumers
do not need edits.

## Mobile Event

Add this arm in the core-owned `main.rs` sidebar-event match:

```rust
OrcaSidebarEvent::OpenMobile => crate::mobile_panel::open(window, cx),
```

The sidebar adds the lower-left Mobile icon and removes Mobile from its old top
overflow menu. No translation or `main.rs` edit is made by the sidebar owner.

## Verification

Pure timing/lifetime/placement tests are authored in `mt-ui/src/icon_tooltip.rs`;
project trigger, footer mapping, and anchor-position regressions are authored in
`orca_sidebar.rs` and `menu.rs`. Existing Activity Bar reducer tests are retained.
Compilation, formatting, tests and native interaction evidence remain unrun
locally and must be provided by GitHub Actions for the integrated commit.

Native acceptance must cover keyboard focus revealing the project ellipsis,
moving from its trigger into the open menu, cancellation/focus restoration,
rapid icon movement and removed anchors, and bottom-edge/narrow/high-DPI tooltip
placement. The new path submits `AnyTooltip` directly; only bubble styling is
shared with ordinary `Tooltip`, whose existing default/instant delays are unchanged.
