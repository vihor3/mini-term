//! One logical-pixel layout for the caption and workspace in the same frame.

pub const CAPTION_HEIGHT: f32 = 42.0;
pub const TERMINAL_TAB_WIDTH: f32 = 200.0;
pub const PROJECT_WIDTH: f32 = 300.0;
pub const PROJECT_RAIL_WIDTH: f32 = 48.0;
pub const TOOL_WIDTH: f32 = 32.0;
pub const WINDOW_BUTTON_WIDTH: f32 = 46.0;
pub const CAPTION_DRAG_WIDTH: f32 = 24.0;
const MAC_TRAFFIC_LIGHT_WIDTH: f32 = 78.0;
const CONTEXT_DEFAULT_WIDTH: f64 = 284.0;
const CONTEXT_MIN_WIDTH: f64 = 240.0;
const CONTEXT_MAX_WIDTH: f64 = 720.0;
const PROJECT_OVERLAY_BREAKPOINT: f32 = 760.0;
const CONTEXT_DOCK_BREAKPOINT: f32 = 1080.0;
const MIN_DOCKED_CENTER_WIDTH: f32 = 320.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextVisibility {
    pub open: bool,
    pub overlay_open: bool,
}

impl Default for ContextVisibility {
    fn default() -> Self {
        Self {
            open: true,
            overlay_open: false,
        }
    }
}

impl ContextVisibility {
    pub fn visible(self, dockable: bool) -> bool {
        if dockable {
            self.open
        } else {
            self.overlay_open
        }
    }

    pub fn toggle(&mut self, dockable: bool) {
        if dockable {
            self.open = !self.open;
            self.overlay_open = false;
        } else {
            self.overlay_open = !self.overlay_open;
        }
    }

    pub fn show(&mut self) {
        self.open = true;
        self.overlay_open = true;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShellGeometry {
    pub orca: bool,
    pub left_width: f32,
    pub projects_width: f32,
    pub projects_expanded: bool,
    pub projects_narrow: bool,
    pub projects_overlay: bool,
    pub context_width: f32,
    pub context_dockable: bool,
    pub context_overlay: bool,
    pub titlebar_center_width: f32,
    pub titlebar_right_width: f32,
    pub body_center_width: f32,
    pub traffic_light_width: f32,
}

impl ShellGeometry {
    pub fn resolve(
        viewport_width: f32,
        is_mac: bool,
        projects_expanded: bool,
        projects_overlay_open: bool,
        context: ContextVisibility,
        preferred_context_width: Option<f64>,
    ) -> Self {
        let traffic_light_width = if is_mac { MAC_TRAFFIC_LIGHT_WIDTH } else { 0.0 };
        let projects_narrow = viewport_width < PROJECT_OVERLAY_BREAKPOINT;
        let projects_overlay = projects_narrow && projects_overlay_open;
        let left_width = if projects_expanded && !projects_narrow {
            PROJECT_WIDTH
        } else {
            PROJECT_RAIL_WIDTH + traffic_light_width
        };
        let projects_expanded = projects_overlay || (projects_expanded && !projects_narrow);
        let preferred_width = Self::preferred_context_width(preferred_context_width) as f32;
        let available = (viewport_width - left_width).max(0.0);
        let context_dockable = viewport_width >= CONTEXT_DOCK_BREAKPOINT
            && available >= preferred_width + MIN_DOCKED_CENTER_WIDTH;
        let context_visible = context.visible(context_dockable);
        let context_overlay = context_visible && !context_dockable;
        let context_width = if context_visible {
            preferred_width.min(available)
        } else {
            0.0
        };
        let docked_width = if context_overlay { 0.0 } else { context_width };
        // Include the tools region's 1px separator inside its reserved width.
        let titlebar_right_width =
            docked_width.max(Self::window_controls_width(is_mac) + TOOL_WIDTH + 1.0);
        Self {
            orca: true,
            left_width,
            projects_width: if projects_overlay {
                PROJECT_WIDTH.min(viewport_width)
            } else {
                left_width
            },
            projects_expanded,
            projects_narrow,
            projects_overlay,
            context_width,
            context_dockable,
            context_overlay,
            titlebar_center_width: (available - titlebar_right_width).max(0.0),
            titlebar_right_width,
            body_center_width: (available - docked_width).max(0.0),
            traffic_light_width,
        }
    }

    pub fn legacy(viewport_width: f32, is_mac: bool) -> Self {
        let traffic_light_width = if is_mac { MAC_TRAFFIC_LIGHT_WIDTH } else { 0.0 };
        let left_width = traffic_light_width + if viewport_width > 760.0 { 120.0 } else { 38.0 };
        let titlebar_right_width = Self::window_controls_width(is_mac);
        Self {
            orca: false,
            left_width,
            projects_width: 0.0,
            projects_expanded: false,
            projects_narrow: false,
            projects_overlay: false,
            context_width: 0.0,
            context_dockable: false,
            context_overlay: false,
            titlebar_center_width: (viewport_width - left_width - titlebar_right_width).max(0.0),
            titlebar_right_width,
            body_center_width: 0.0,
            traffic_light_width,
        }
    }

    pub fn window_controls_width(is_mac: bool) -> f32 {
        if is_mac {
            0.0
        } else {
            WINDOW_BUTTON_WIDTH * 3.0
        }
    }

    pub fn preferred_context_width(preference: Option<f64>) -> f64 {
        preference
            .filter(|width| width.is_finite())
            .unwrap_or(CONTEXT_DEFAULT_WIDTH)
            .clamp(CONTEXT_MIN_WIDTH, CONTEXT_MAX_WIDTH)
    }

    pub fn resized_context_width(self, viewport_width: f32, requested: f64) -> f64 {
        let center_reserve = if self.context_dockable {
            MIN_DOCKED_CENTER_WIDTH
        } else {
            0.0
        };
        let max_width = (viewport_width - self.left_width - center_reserve)
            .clamp(0.0, CONTEXT_MAX_WIDTH as f32) as f64;
        requested.clamp(CONTEXT_MIN_WIDTH.min(max_width), max_width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(width: f32, expanded: bool, right: Option<f64>) -> ShellGeometry {
        ShellGeometry::resolve(
            width,
            false,
            expanded,
            false,
            ContextVisibility::default(),
            right,
        )
    }

    #[test]
    fn docked_caption_and_body_share_both_boundaries_during_resize() {
        for viewport in [1280.0, 1600.0, 2560.0] {
            for expanded in [false, true] {
                for preference in [None, Some(340.0), Some(400.0), Some(640.0)] {
                    let geometry = layout(viewport, expanded, preference);
                    if !geometry.context_dockable {
                        continue;
                    }
                    assert_eq!(geometry.left_width, if expanded { 300.0 } else { 48.0 });
                    assert_eq!(geometry.left_width, geometry.projects_width);
                    assert_eq!(
                        geometry.left_width + geometry.titlebar_center_width,
                        viewport - geometry.context_width
                    );
                    assert_eq!(geometry.titlebar_center_width, geometry.body_center_width);
                    let resized = geometry.resized_context_width(viewport, 415.0);
                    let next = layout(viewport, expanded, Some(resized));
                    assert_eq!(next.titlebar_right_width, next.context_width);
                    assert_eq!(next.body_center_width, next.titlebar_center_width);
                    assert_eq!(
                        next.left_width + next.body_center_width + next.context_width,
                        viewport
                    );
                }
            }
        }
    }

    #[test]
    fn saved_width_is_not_replaced_by_preview_default_or_viewport_changes() {
        assert_eq!(layout(1600.0, true, None).context_width, 284.0);
        for saved in [340.0, 400.0, 700.0] {
            let narrow = layout(800.0, true, Some(saved));
            assert_eq!(narrow.context_width, 0.0);
            let wide = layout(1920.0, true, Some(saved));
            assert_eq!(wide.context_width, saved as f32);
        }
        assert_eq!(
            ShellGeometry::preferred_context_width(Some(f64::NAN)),
            284.0
        );
    }

    #[test]
    fn hidden_and_overlaid_context_reserve_only_actual_caption_controls() {
        let mut context = ContextVisibility::default();
        context.toggle(true);
        let hidden = ShellGeometry::resolve(1280.0, false, true, false, context, Some(340.0));
        assert_eq!(hidden.context_width, 0.0);
        assert_eq!(hidden.titlebar_right_width, 3.0 * 46.0 + 32.0 + 1.0);
        assert_eq!(hidden.body_center_width, 980.0);
        context.toggle(false);
        let overlay = ShellGeometry::resolve(800.0, false, true, false, context, Some(340.0));
        assert!(overlay.context_overlay);
        assert_eq!(overlay.titlebar_right_width, hidden.titlebar_right_width);
        assert_eq!(overlay.body_center_width, 500.0);
        assert!(
            !context.open,
            "overlay navigation must not reset the wide preference"
        );
        context.show();
        assert!(context.visible(false));
        assert!(context.visible(true));
    }

    #[test]
    fn narrow_caption_keeps_add_overflow_drag_and_native_controls_reachable() {
        for is_mac in [false, true] {
            for viewport in [320.0, 480.0, 640.0, 760.0] {
                let geometry = ShellGeometry::resolve(
                    viewport,
                    is_mac,
                    true,
                    false,
                    ContextVisibility::default(),
                    None,
                );
                assert!(geometry.titlebar_center_width >= TOOL_WIDTH * 2.0 + CAPTION_DRAG_WIDTH);
                assert_eq!(
                    geometry.left_width
                        + geometry.titlebar_center_width
                        + geometry.titlebar_right_width,
                    viewport
                );
                assert!(geometry.left_width >= geometry.traffic_light_width + TOOL_WIDTH);
                assert_eq!(geometry.left_width, geometry.projects_width);
                assert!(!geometry.context_overlay);
            }
        }
    }

    #[test]
    fn project_overlay_leaves_caption_rail_and_saved_choice_unchanged() {
        let narrow = ShellGeometry::resolve(
            640.0,
            false,
            false,
            true,
            ContextVisibility::default(),
            None,
        );
        assert!(narrow.projects_overlay && narrow.projects_expanded);
        assert_eq!(narrow.projects_width, 300.0);
        assert_eq!(narrow.left_width, 48.0);
        let wide = layout(1280.0, false, None);
        assert!(!wide.projects_expanded);
        assert_eq!(wide.left_width, 48.0);
    }

    #[test]
    fn legacy_caption_preserves_native_controls_and_mac_inset() {
        for is_mac in [false, true] {
            for viewport in [480.0, 1280.0] {
                let geometry = ShellGeometry::legacy(viewport, is_mac);
                assert!(!geometry.orca);
                assert_eq!(
                    geometry.titlebar_right_width,
                    if is_mac { 0.0 } else { 138.0 }
                );
                assert!(geometry.left_width >= geometry.traffic_light_width);
                assert_eq!(
                    geometry.left_width
                        + geometry.titlebar_center_width
                        + geometry.titlebar_right_width,
                    viewport
                );
            }
        }
    }
}
