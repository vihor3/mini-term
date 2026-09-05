# Icon Tooltip Contract

## 1. Scope / Trigger

Use this opt-in boundary for contiguous icon tools that need one initial hover
delay and prompt descriptions on subsequent icons. Existing ordinary `Tooltip`
timings are not changed globally. Native hit testing and visual acceptance remain
separate from pure reducer tests.

## 2. Signatures

`mt_ui::icon_tooltip` exports `IconTooltips`, generic `HoverSession<K>`,
`HoverEnter`, and `HOVER_SHOW_DELAY` (500 ms). The app's Activity Bar reexports
the reducer for its existing consumers.

```rust
pub fn button(
    owner: &Entity<IconTooltips>,
    key: impl Into<ElementId>,
    description: impl Into<SharedString>,
    button: Stateful<Div>,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div>;
pub fn group(
    owner: &Entity<IconTooltips>,
    group: Stateful<Div>,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div>;
pub fn reset(owner: &Entity<IconTooltips>, window: &mut Window, cx: &mut App);
```

## 3. Contracts

- Retain one `Entity<IconTooltips>` per contiguous group in the owning view. Use
  stable distinct anchor keys; descriptions are not identities.
- Wrap each existing icon button and the containing group, including its gaps.
  Preserve command handlers, dimensions and focus behavior. Do not add a second
  `.on_hover` or GPUI `.tooltip` on a wrapped button or group.
- The current anchor canvas uses absolute full-size placement without explicit
  insets. A wrapper around a child `Button` must use centered flex alignment,
  not default block layout: the latter can place the canvas after the in-flow
  child, so its hitbox misses the visible control. `TerminalSearchBar`'s private
  `tip_anchor` builder records this production style contract.
- When explicitly tracking a `FocusHandle`, set its `tab_stop(true)` during
  creation if it is keyboard reachable. GPUI's Div `tab_index` configures an
  implicit handle, not a supplied handle's tab-stop state.
- First hover waits 500 ms. Only a completed valid timer warms the group; moving
  to another icon before it fires starts another cold timer. Once warmed, the
  next icon's description is immediate, including movement through group gaps.
- Leaving/unmounting the group, losing its anchor, changing focus, deactivating
  the window, clicking, keyboard input or scrolling invalidates pending work.
  Timer generation, live mount and current hit-test ownership must all match
  before displaying. Reset transient descriptions on explicit owner switches or
  programmatic menu opening.
- Hit testing after prepaint includes deferred occluding overlays. Invalidate
  warm gap sessions on window exit/occlusion as well as icon mouse movement.
  Draw-time resets defer the clearing refresh; GPUI ignores immediate refresh
  while drawing.
- Submit an `AnyTooltip` at the window level, bypassing GPUI's additional
  per-div delay. Clamp/flip placement inside the viewport. The description must
  not occlude interactive hit testing, take focus or change layout dimensions.
- Share ordinary tooltip bubble styling only; existing default/instant tooltip
  delays and unrelated Activity Bar consumers retain their previous behavior.

## 4. Validation & Error Matrix

| Condition | Required behavior |
| --- | --- |
| First icon in a cold group | Wait for a valid 500 ms timer |
| Next icon after a description appeared | Show promptly without a second timer |
| Pointer leaves an icon into a group gap | Hide its text but retain warm state |
| Old timer returns after another icon/owner took over | Ignore it |
| Anchor/group disappears or an overlay occludes it | Invalidate pending/visible description |
| Focus or active-window state changes | Reset the hover sequence |
| Icon is near an edge or toolbar is clipped | Use visible anchor bounds and viewport-constrained text |

## 5. Good / Base / Bad Cases

- Good: Usage waits once, then Settings and Mobile describe promptly while the
  pointer stays in the lower-left group.
- Base: Leaving the toolbar and returning starts a new delay.
- Bad: Recreate the owner every render or use the caption as the anchor key.
- Bad: Layer ordinary GPUI tooltip timing over the shared hover reducer.

## 6. Tests Required

Author reducer tests for cold/warm movement, gaps, stale leave/timer, reset and
mount removal; placement tests for edge, narrow and oversized cases. Run them,
formatting, compilation and lint only in GitHub Actions. Packaged native
acceptance must check real hover/click/overlay/focus/window-control hit testing,
high-DPI placement and removed anchors; pure tests do not prove those behaviors.

## 7. Wrong vs Correct

Wrong: call `.tooltip(...)` on every icon and only shorten its inner view delay;
GPUI can still impose a second private hover timer.

Correct: retain one group owner, wrap buttons and group through `IconTooltips`,
and use generation-fenced window-level submission with explicit lifecycle reset.
