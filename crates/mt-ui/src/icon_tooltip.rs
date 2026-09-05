//! Opt-in icon descriptions: one delay per contiguous toolbar hover sequence.
//!
//! Keep one `Entity<IconTooltips>` in the owner. Wrap its icon divs with
//! `IconTooltips::button` and their containing div with `IconTooltips::group`.
//! Do not also attach GPUI's `.tooltip` or `.on_hover` to those same divs.

use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyTooltip, App, AppContext, AvailableSpace, Bounds, Context, Div, ElementId, Entity, EntityId,
    FocusHandle, Hitbox, HitboxBehavior, IntoElement, KeyDownEvent, MouseDownEvent, MouseExitEvent,
    ParentElement, Pixels, Point, Render, ScrollWheelEvent, SharedString, Size, Stateful,
    StatefulInteractiveElement, Styled, Subscription, Task, WeakEntity, Window, canvas, div, point,
    px,
};

pub const HOVER_SHOW_DELAY: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverEnter {
    Unchanged,
    Delay(u64),
    ShowNow,
}

/// Clock-free reducer shared with the legacy Activity Bar. Timer cancellation
/// and generation validation are both required, including after anchor removal.
#[derive(Debug)]
pub struct HoverSession<K = &'static str> {
    hovered: Option<K>,
    visible: Option<K>,
    warmed: bool,
    generation: u64,
}

impl<K> Default for HoverSession<K> {
    fn default() -> Self {
        Self {
            hovered: None,
            visible: None,
            warmed: false,
            generation: 0,
        }
    }
}

impl<K: Clone + PartialEq> HoverSession<K> {
    pub fn enter(&mut self, key: K) -> HoverEnter {
        if self.hovered.as_ref() == Some(&key) {
            return HoverEnter::Unchanged;
        }
        self.generation = self.generation.wrapping_add(1);
        self.hovered = Some(key.clone());
        if self.warmed {
            self.visible = Some(key);
            HoverEnter::ShowNow
        } else {
            self.visible = None;
            HoverEnter::Delay(self.generation)
        }
    }

    pub fn leave(&mut self, key: K) -> bool {
        if self.hovered.as_ref() != Some(&key) {
            return false;
        }
        self.generation = self.generation.wrapping_add(1);
        self.hovered = None;
        self.visible = None;
        true
    }

    pub fn on_delay_elapsed(&mut self, generation: u64, key: K) -> bool {
        if self.warmed || self.generation != generation || self.hovered.as_ref() != Some(&key) {
            return false;
        }
        self.warmed = true;
        self.visible = Some(key);
        true
    }

    pub fn reset(&mut self) -> bool {
        let changed = self.hovered.is_some() || self.visible.is_some() || self.warmed;
        self.generation = self.generation.wrapping_add(1);
        self.hovered = None;
        self.visible = None;
        self.warmed = false;
        changed
    }

    pub fn is_visible(&self, key: K) -> bool {
        self.visible.as_ref() == Some(&key)
    }

    fn remove(&mut self, key: K) -> bool {
        if self.hovered.as_ref() == Some(&key) || self.visible.as_ref() == Some(&key) {
            self.reset()
        } else {
            false
        }
    }

    fn reject_delay(&mut self, generation: u64, key: K) -> bool {
        if self.generation == generation && self.hovered.as_ref() == Some(&key) && !self.warmed {
            self.reset()
        } else {
            false
        }
    }
}

#[derive(Default)]
pub struct IconTooltips {
    session: HoverSession<EntityId>,
    delay: Option<Task<()>>,
    focus: Option<FocusHandle>,
    focus_out: Option<Subscription>,
}

struct TooltipAnchor {
    hitbox: Option<Hitbox>,
}

struct TooltipScope {
    _activation: Subscription,
}

impl IconTooltips {
    /// Preserve the caller's button geometry and handlers. The key identifies a
    /// mounted anchor, not its caption; use distinct keys for distinct commands.
    pub fn button(
        owner: &Entity<Self>,
        key: impl Into<ElementId>,
        description: impl Into<SharedString>,
        button: Stateful<Div>,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        let group = owner.downgrade();
        let anchor = window.with_id(
            SharedString::from(format!("icon-tooltip-anchors-{:?}", owner.entity_id())),
            |window| {
                window.use_keyed_state(key, cx, |window, cx| {
                    let key = cx.entity().entity_id();
                    cx.on_release_in(window, move |_: &mut TooltipAnchor, window, cx| {
                        let _ = group.update(cx, |group, cx| {
                            if group.session.remove(key) {
                                group.cancel_delay();
                                window.refresh();
                                cx.notify();
                            }
                        });
                    })
                    .detach();
                    TooltipAnchor { hitbox: None }
                })
            },
        );
        let key = anchor.entity_id();
        let hover_anchor = anchor.downgrade();
        let hover_owner = owner.downgrade();
        let paint_owner = owner.downgrade();
        let description = description.into();

        button
            .on_hover(move |hovered, window, cx| {
                let _ = hover_owner.update(cx, |group, cx| {
                    group.on_hover(key, hover_anchor.clone(), *hovered, window, cx);
                });
            })
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
                        let clipped = bounds.intersect(&window.content_mask().bounds);
                        anchor.update(cx, |anchor, _| anchor.hitbox = Some(hitbox));
                        let Some(owner) = paint_owner.upgrade() else {
                            return;
                        };
                        let visible = owner.read(cx).session.is_visible(key);
                        if !visible {
                            return;
                        }
                        if !anchor_contains_pointer(clipped, window)
                            || !owner.read(cx).focus_matches(window, cx)
                        {
                            Self::reset(&owner, window, cx);
                            return;
                        }

                        // Direct submission bypasses GPUI's private per-div
                        // delay and escapes the sidebar/toolbar content mask.
                        let view = cx.new(|_| IconDescription(description.clone()));
                        let tooltip_size = view.clone().into_any_element().layout_as_root(
                            AvailableSpace::min_size(),
                            window,
                            cx,
                        );
                        let origin =
                            description_position(clipped, tooltip_size, window.viewport_size());
                        let group = owner.downgrade();
                        window.set_tooltip(AnyTooltip {
                            view: view.into(),
                            // GPUI adds one pixel to this point. Position from
                            // the measured anchor, including when flipping above.
                            mouse_position: origin - point(px(1.0), px(1.0)),
                            check_visible_and_update: Rc::new(move |_, window, cx| {
                                group.upgrade().is_some_and(|group| {
                                    let group = group.read(cx);
                                    group.session.is_visible(key)
                                        && group.focus_matches(window, cx)
                                        && anchor_contains_pointer(clipped, window)
                                })
                            }),
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
    }

    /// Include the gaps between buttons in this element. Leaving it, unmounting
    /// it, changing focus, clicking, or scrolling ends the warm hover sequence.
    pub fn group(
        owner: &Entity<Self>,
        group: Stateful<Div>,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        let weak_owner = owner.downgrade();
        let scope = window.use_keyed_state(
            SharedString::from(format!("icon-tooltip-scope-{:?}", owner.entity_id())),
            cx,
            |window, cx| {
                let release_owner = weak_owner.clone();
                cx.on_release_in(window, move |_: &mut TooltipScope, window, cx| {
                    if let Some(owner) = release_owner.upgrade() {
                        Self::reset(&owner, window, cx);
                    }
                })
                .detach();
                let activation = cx.observe_window_activation(window, move |_, window, cx| {
                    if !window.is_window_active()
                        && let Some(owner) = weak_owner.upgrade()
                    {
                        Self::reset(&owner, window, cx);
                    }
                });
                TooltipScope {
                    _activation: activation,
                }
            },
        );
        let hover_owner = owner.downgrade();
        let layout_owner = owner.downgrade();
        let paint_owner = owner.downgrade();
        group
            .on_hover(move |hovered, window, cx| {
                if !*hovered && let Some(owner) = hover_owner.upgrade() {
                    Self::reset(&owner, window, cx);
                }
            })
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
                        let clipped = bounds.intersect(&window.content_mask().bounds);
                        if let Some(owner) = layout_owner.upgrade()
                            && (!anchor_contains_pointer(clipped, window)
                                || !owner.read(cx).focus_matches(window, cx))
                        {
                            Self::reset(&owner, window, cx);
                        }
                        hitbox
                    },
                    move |_, hitbox, window, cx| {
                        // Keep the mount lease through paint, without a timer
                        // or callback retaining it after the group is removed.
                        let _ = &scope;
                        // Hit testing includes deferred overlays only after
                        // prepaint. Occlusion must also cool a gap-only session.
                        if !hitbox.is_hovered(window)
                            && let Some(owner) = paint_owner.upgrade()
                        {
                            Self::reset(&owner, window, cx);
                        }
                        let owner = paint_owner.clone();
                        window.on_mouse_event(move |_: &MouseExitEvent, phase, window, cx| {
                            if phase.capture()
                                && let Some(owner) = owner.upgrade()
                            {
                                Self::reset(&owner, window, cx);
                            }
                        });
                        let owner = paint_owner.clone();
                        window.on_mouse_event(move |_: &MouseDownEvent, phase, window, cx| {
                            if phase.capture()
                                && let Some(owner) = owner.upgrade()
                            {
                                Self::reset(&owner, window, cx);
                            }
                        });
                        let owner = paint_owner.clone();
                        window.on_mouse_event(move |_: &ScrollWheelEvent, phase, window, cx| {
                            if phase.capture()
                                && let Some(owner) = owner.upgrade()
                            {
                                Self::reset(&owner, window, cx);
                            }
                        });
                        let owner = paint_owner.clone();
                        window.on_key_event(move |_: &KeyDownEvent, phase, window, cx| {
                            if phase.capture()
                                && let Some(owner) = owner.upgrade()
                            {
                                Self::reset(&owner, window, cx);
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
    }

    pub fn reset(owner: &Entity<Self>, window: &mut Window, cx: &mut App) {
        owner.update(cx, |group, cx| {
            if group.session.reset() {
                group.cancel_delay();
                cx.notify();
                // Scope validation also runs during drawing, where refresh is
                // ignored. Request the clearing frame after the draw completes.
                window.defer(cx, |window, _| window.refresh());
            }
        });
    }

    fn cancel_delay(&mut self) {
        self.delay = None;
        self.focus_out = None;
        self.focus = None;
    }

    fn focus_matches(&self, window: &Window, cx: &App) -> bool {
        window.is_window_active() && self.focus == window.focused(cx)
    }

    fn on_hover(
        &mut self,
        key: EntityId,
        anchor: WeakEntity<TooltipAnchor>,
        hovered: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !hovered {
            if self.session.leave(key) {
                self.delay = None;
                cx.notify();
                window.refresh();
            }
            return;
        }
        if !window.is_window_active() {
            return;
        }
        if !self.focus_matches(window, cx) {
            self.session.reset();
            self.cancel_delay();
        }
        self.focus = window.focused(cx);
        if self.focus_out.is_none()
            && let Some(focus) = &self.focus
        {
            let owner = cx.weak_entity();
            self.focus_out = Some(window.on_focus_out(focus, cx, move |_, window, cx| {
                if let Some(owner) = owner.upgrade() {
                    Self::reset(&owner, window, cx);
                }
            }));
        }

        match self.session.enter(key) {
            HoverEnter::Unchanged => return,
            HoverEnter::ShowNow => self.delay = None,
            HoverEnter::Delay(generation) => {
                let owner = cx.weak_entity();
                self.delay = Some(window.spawn(cx, async move |cx| {
                    cx.background_executor().timer(HOVER_SHOW_DELAY).await;
                    let _ = cx.update(|window, cx| {
                        let Some(owner) = owner.upgrade() else {
                            return;
                        };
                        let anchor_hovered = window.is_window_hovered()
                            && anchor.upgrade().is_some_and(|anchor| {
                                anchor
                                    .read(cx)
                                    .hitbox
                                    .as_ref()
                                    .is_some_and(|hitbox| hitbox.is_hovered(window))
                            });
                        owner.update(cx, |group, cx| {
                            if anchor_hovered && group.focus_matches(window, cx) {
                                if !group.session.on_delay_elapsed(generation, key) {
                                    return;
                                }
                                group.delay = None;
                            } else if group.session.reject_delay(generation, key) {
                                group.cancel_delay();
                            } else {
                                return;
                            }
                            cx.notify();
                            window.refresh();
                        });
                    });
                }));
            }
        }
        cx.notify();
        window.refresh();
    }
}

fn anchor_contains_pointer(bounds: Bounds<Pixels>, window: &Window) -> bool {
    window.is_window_active()
        && window.is_window_hovered()
        && !bounds.is_empty()
        && bounds.contains(&window.mouse_position())
}

fn description_max_width(viewport_width: Pixels) -> Pixels {
    (viewport_width - px(8.0)).max(px(1.0)).min(px(360.0))
}

fn description_position(
    anchor: Bounds<Pixels>,
    description: Size<Pixels>,
    viewport: Size<Pixels>,
) -> Point<Pixels> {
    let margin = px(4.0);
    let max_x = (viewport.width - description.width - margin).max(margin);
    let max_y = (viewport.height - description.height - margin).max(margin);
    let below = anchor.bottom() + margin;
    let y = if below + description.height <= viewport.height - margin {
        below
    } else {
        anchor.top() - description.height - margin
    };
    point(
        anchor.left().max(margin).min(max_x),
        y.max(margin).min(max_y),
    )
}

struct IconDescription(SharedString);

impl Render for IconDescription {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // No occluding hitbox or listeners: the description never steals a
        // toolbar click, and its bounds never participate in the hover group.
        crate::tooltip::surface(cx)
            .max_w(description_max_width(window.viewport_size().width))
            .max_h((window.viewport_size().height - px(8.0)).max(px(1.0)))
            .overflow_hidden()
            .child(div().min_w_0().child(self.0.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delay(session: &mut HoverSession, key: &'static str) -> u64 {
        let HoverEnter::Delay(generation) = session.enter(key) else {
            panic!("expected a cold hover delay");
        };
        generation
    }

    #[test]
    fn first_description_waits_and_following_icons_are_prompt() {
        let mut session = HoverSession::default();
        let generation = delay(&mut session, "usage");
        assert!(!session.is_visible("usage"));
        assert!(session.on_delay_elapsed(generation, "usage"));
        assert!(session.leave("usage"));
        assert_eq!(session.enter("settings"), HoverEnter::ShowNow);
        assert!(session.is_visible("settings"));
        assert!(!session.is_visible("usage"));
        assert_eq!(HOVER_SHOW_DELAY, Duration::from_millis(500));
    }

    #[test]
    fn stale_leave_and_timer_cannot_replace_a_new_anchor() {
        let mut session = HoverSession::default();
        let first = delay(&mut session, "usage");
        let second = delay(&mut session, "settings");
        assert!(!session.leave("usage"));
        assert!(!session.on_delay_elapsed(first, "usage"));
        assert!(session.on_delay_elapsed(second, "settings"));
        assert!(!session.on_delay_elapsed(second, "settings"));
        assert!(session.is_visible("settings"));
    }

    #[test]
    fn reset_cancels_pending_and_warm_sequences() {
        let mut session = HoverSession::default();
        let pending = delay(&mut session, "usage");
        assert!(session.reset());
        assert!(!session.on_delay_elapsed(pending, "usage"));
        let pending = delay(&mut session, "usage");
        assert!(session.on_delay_elapsed(pending, "usage"));
        assert!(session.reset());
        delay(&mut session, "settings");
    }

    #[test]
    fn reset_invalidates_a_warm_sequence_even_between_icons() {
        let mut session = HoverSession::default();
        let pending = delay(&mut session, "usage");
        assert!(session.on_delay_elapsed(pending, "usage"));
        assert!(session.leave("usage"));
        assert!(session.hovered.is_none());
        assert!(session.visible.is_none());
        assert!(session.reset(), "an empty gap still owns the warm sequence");
        assert!(!session.on_delay_elapsed(pending, "usage"));
        delay(&mut session, "settings");
    }

    #[test]
    fn anchor_removal_resets_only_its_own_sequence() {
        let mut session = HoverSession::default();
        let removed = delay(&mut session, "removed");
        assert!(session.remove("removed"));
        assert!(!session.on_delay_elapsed(removed, "removed"));
        let current = delay(&mut session, "current");
        assert!(!session.remove("removed"));
        assert!(session.on_delay_elapsed(current, "current"));
        assert!(session.remove("current"));
        delay(&mut session, "replacement");
    }

    #[test]
    fn leaving_before_the_delay_never_warms_the_group() {
        let mut session = HoverSession::default();
        let generation = delay(&mut session, "usage");
        assert!(session.leave("usage"));
        assert!(!session.on_delay_elapsed(generation, "usage"));
        delay(&mut session, "mobile");
    }

    #[test]
    fn descriptions_fit_narrow_and_wide_viewports() {
        assert_eq!(description_max_width(px(1920.0)), px(360.0));
        assert_eq!(description_max_width(px(160.0)), px(152.0));
        assert_eq!(description_max_width(px(8.0)), px(1.0));
    }

    #[test]
    fn a_replacement_anchor_cannot_receive_a_removed_anchors_delay_or_release() {
        let mut session = HoverSession::<u64>::default();
        let HoverEnter::Delay(old_generation) = session.enter(1) else {
            panic!("expected a delay");
        };
        let HoverEnter::Delay(current_generation) = session.enter(2) else {
            panic!("expected a replacement delay");
        };
        assert!(!session.remove(1));
        assert!(!session.on_delay_elapsed(old_generation, 1));
        assert!(session.on_delay_elapsed(current_generation, 2));
        assert!(session.is_visible(2));
    }

    #[test]
    fn hidden_or_unfocused_anchor_cancels_only_its_own_pending_delay() {
        let mut session = HoverSession::default();
        let old = delay(&mut session, "old");
        let current = delay(&mut session, "current");
        assert!(!session.reject_delay(old, "old"));
        assert!(session.reject_delay(current, "current"));
        assert!(!session.on_delay_elapsed(current, "current"));
        delay(&mut session, "replacement");
    }

    #[test]
    fn footer_descriptions_flip_above_without_covering_the_icon() {
        let anchor = Bounds::new(point(px(8.0), px(552.0)), gpui::size(px(32.0), px(32.0)));
        let description = gpui::size(px(140.0), px(26.0));
        let viewport = gpui::size(px(800.0), px(600.0));
        let origin = description_position(anchor, description, viewport);
        assert_eq!(origin, point(px(8.0), px(522.0)));
        assert!(origin.y + description.height < anchor.top());
    }

    #[test]
    fn titlebar_descriptions_stay_below_and_inside_a_narrow_window() {
        let anchor = Bounds::new(point(px(130.0), px(4.0)), gpui::size(px(24.0), px(24.0)));
        let viewport = gpui::size(px(160.0), px(300.0));
        let description = gpui::size(description_max_width(viewport.width), px(40.0));
        let origin = description_position(anchor, description, viewport);
        assert_eq!(origin, point(px(4.0), px(32.0)));
        assert!(origin.x + description.width <= viewport.width - px(4.0));
        assert!(origin.y > anchor.bottom());
    }
}
