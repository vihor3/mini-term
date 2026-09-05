//! Orca-aligned project navigation: configured project -> Git worktree.
//!
//! The entity owns only presentation state. Project, terminal, and file state
//! remain in [`AppStore`], while the shared [`WorktreeCatalog`] owns Git facts.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{
    AnyElement, AppContext, Bounds, Context, Entity, EventEmitter, FocusHandle, FontWeight,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Pixels, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, canvas, div,
    prelude::FluentBuilder as _, px,
};
use mt_ai::{AgentActivity, AgentConnectivity, AgentEvidence};
use mt_project::worktree::WorktreePathState;
use mt_ui::icon_tooltip::IconTooltips;
use mt_ui::icons::vector::{Geom, Ink, Shape, VectorIcon};
use mt_ui::icons::{AiVendor, BrandIcon, FileIcon, StatusDot, StatusKind};
use mt_ui::tooltip::Tooltip;

use crate::agent_activity::{agent_target_needs_user, global_agent_activity_enabled};
use crate::i18n::t;
use crate::menu;
use crate::store::{AgentTargetView, AppStore, orca_worktree_context_enabled};
use crate::tree::{PaneState, PaneStatus};
use crate::ui;
use crate::worktree_catalog::{
    CatalogBackend, ProjectWorktreeGroup, WorktreeCatalog, WorktreeCatalogRow,
};
use crate::worktree_visibility::{ProjectSettingsTarget, sidebar_visible};

/// Fixed shell width used by `Workspace` and the Agents overlay anchor.
pub const WIDTH: f32 = 300.0;

const NAV_ICON_SIZE: f32 = 15.0;
const ROW_ICON_SIZE: f32 = 14.0;

/// Actions owned by `Workspace` rather than by the project sidebar itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrcaSidebarEvent {
    OpenJumpPalette,
    ToggleAgents,
    OpenUsage,
    OpenSettings,
    OpenMobile,
}

/// Magnifier geometry shared with the existing file-tree search action.
const SEARCH_ICON: &[Shape] = &[
    Shape::line(
        Ink::Current,
        0.0875,
        Geom::Circle {
            c: (0.4375, 0.4375),
            r: 0.2625,
        },
    ),
    Shape::line(
        Ink::Current,
        0.0875,
        Geom::Polyline(&[(0.6375, 0.6375), (0.875, 0.875)]),
    ),
];

const PLUS_ICON: &[Shape] = &[
    Shape::line(
        Ink::Current,
        0.10,
        Geom::Polyline(&[(0.2, 0.5), (0.8, 0.5)]),
    ),
    Shape::line(
        Ink::Current,
        0.10,
        Geom::Polyline(&[(0.5, 0.2), (0.5, 0.8)]),
    ),
];

const MORE_ICON: &[Shape] = &[
    Shape::fill(
        Ink::Current,
        Geom::Circle {
            c: (0.24, 0.5),
            r: 0.08,
        },
    ),
    Shape::fill(
        Ink::Current,
        Geom::Circle {
            c: (0.5, 0.5),
            r: 0.08,
        },
    ),
    Shape::fill(
        Ink::Current,
        Geom::Circle {
            c: (0.76, 0.5),
            r: 0.08,
        },
    ),
];

const CHEVRON_RIGHT: &[Shape] = &[Shape::line(
    Ink::Current,
    0.11,
    Geom::Polyline(&[(0.36, 0.22), (0.64, 0.5), (0.36, 0.78)]),
)];

const CHEVRON_DOWN: &[Shape] = &[Shape::line(
    Ink::Current,
    0.11,
    Geom::Polyline(&[(0.22, 0.36), (0.5, 0.64), (0.78, 0.36)]),
)];

fn group_agent_targets_by_project(
    targets: Vec<AgentTargetView>,
) -> HashMap<String, Vec<AgentTargetView>> {
    let mut seen_run_ids = HashSet::new();
    let mut by_project = HashMap::<String, Vec<AgentTargetView>>::new();
    for target in targets {
        if !seen_run_ids.insert(target.run_id.clone()) {
            continue;
        }
        by_project
            .entry(target.project_id.clone())
            .or_default()
            .push(target);
    }
    by_project
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SidebarActivity {
    Idle,
    LastKnownWork,
    Done,
    Waiting,
    Working,
    Attention,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SidebarIndicator {
    activity: SidebarActivity,
    connectivity: Option<AgentConnectivity>,
}

impl SidebarIndicator {
    fn from_agent(
        activity: AgentActivity,
        connectivity: AgentConnectivity,
        attention: bool,
    ) -> Self {
        let activity = if activity == AgentActivity::Failed {
            SidebarActivity::Failed
        } else if attention || activity == AgentActivity::Blocked {
            SidebarActivity::Attention
        } else {
            match activity {
                AgentActivity::Starting | AgentActivity::Working => {
                    if connectivity == AgentConnectivity::Live {
                        SidebarActivity::Working
                    } else {
                        SidebarActivity::LastKnownWork
                    }
                }
                AgentActivity::Waiting => SidebarActivity::Waiting,
                AgentActivity::Done => SidebarActivity::Done,
                _ => SidebarActivity::Idle,
            }
        };
        Self {
            activity,
            connectivity: Some(connectivity),
        }
    }

    fn merge(&mut self, other: Self) {
        self.activity = self.activity.max(other.activity);
        let connectivity_priority = |state| match state {
            None => 0,
            Some(AgentConnectivity::Live) => 1,
            Some(AgentConnectivity::Stale) => 2,
            Some(AgentConnectivity::Disconnected) => 3,
        };
        if connectivity_priority(other.connectivity) > connectivity_priority(self.connectivity) {
            self.connectivity = other.connectivity;
        }
    }

    fn label(self) -> &'static str {
        match self.activity {
            SidebarActivity::Idle => "Idle",
            SidebarActivity::LastKnownWork => "Working (last known)",
            SidebarActivity::Done => "Done",
            SidebarActivity::Waiting => "Waiting",
            SidebarActivity::Working => "Working",
            SidebarActivity::Attention => "Needs you",
            SidebarActivity::Failed => "Failed",
        }
    }

    fn icon(self) -> AnyElement {
        match self.activity {
            SidebarActivity::Working => ui::status_dot(PaneStatus::AiWorking).into_any_element(),
            SidebarActivity::Done => ui::status_dot(PaneStatus::AiIdle).into_any_element(),
            SidebarActivity::Failed => ui::status_dot(PaneStatus::Error).into_any_element(),
            activity => StatusDot::new(StatusKind::Idle)
                .size(px(11.0))
                .color(match activity {
                    SidebarActivity::Attention => ui::color_warning(),
                    SidebarActivity::Waiting => ui::color_success(),
                    SidebarActivity::LastKnownWork => ui::accent(),
                    _ => ui::text_muted(),
                })
                .into_any_element(),
        }
    }
}

fn worktree_agent_indicator(agents: &[AgentTargetView], panes: &[&PaneState]) -> SidebarIndicator {
    // Rich evidence replaces only its own pane's legacy projection. Other
    // panes still contribute work, errors, and attention to the worktree.
    let fallback = panes
        .iter()
        .filter(|pane| {
            !agents.iter().any(|agent| {
                agent.pane_id == pane.id
                    && !agent.activity.is_ended()
                    && agent.evidence != AgentEvidence::RestoredHistory
            })
        })
        .map(|pane| match pane.status {
            PaneStatus::Error => SidebarActivity::Failed,
            _ if pane.attention => SidebarActivity::Attention,
            PaneStatus::Idle => SidebarActivity::Idle,
            PaneStatus::AiIdle => SidebarActivity::Done,
            PaneStatus::AiWorking => SidebarActivity::Working,
        })
        .max()
        .unwrap_or(SidebarActivity::Idle);
    let mut indicator = SidebarIndicator {
        activity: fallback,
        connectivity: None,
    };
    for agent in agents.iter().filter(|agent| {
        !agent.activity.is_ended() && agent.evidence != AgentEvidence::RestoredHistory
    }) {
        indicator.merge(SidebarIndicator::from_agent(
            agent.activity,
            agent.connectivity,
            agent.attention,
        ));
    }
    indicator
}

fn nav_row(
    id: &'static str,
    label: impl Into<SharedString>,
    icon: AnyElement,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(34.0))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(9.0))
        .px(px(9.0))
        .rounded(px(4.0))
        .cursor_pointer()
        .text_size(ui::font_px(12.5))
        .text_color(ui::text_secondary())
        .hover(|row| row.bg(ui::border_subtle()).text_color(ui::text_primary()))
        .child(
            div()
                .w(px(18.0))
                .flex()
                .items_center()
                .justify_center()
                .child(icon),
        )
        .child(div().flex_1().truncate().child(label.into()))
}

fn small_icon_button(
    id: impl Into<gpui::ElementId>,
    icon: AnyElement,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .w(px(24.0))
        .h(px(24.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.0))
        .cursor_pointer()
        .tab_index(0)
        .focus(|button| button.bg(ui::accent_subtle()).text_color(ui::accent()))
        .text_color(ui::text_muted())
        .hover(|button| {
            button
                .bg(ui::border_subtle())
                .text_color(ui::text_primary())
        })
        .child(icon)
}

fn project_menu_anchor(project_id: &str) -> SharedString {
    format!("orca-project-settings-{project_id}").into()
}

fn show_project_menu_trigger(hovered: bool, focused: bool, menu_open: bool) -> bool {
    hovered || focused || menu_open
}

struct ProjectRowControls {
    focus: FocusHandle,
    menu_focus: FocusHandle,
    menu_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    tooltips: Entity<IconTooltips>,
}

const FOOTER_ACTIONS: [(&str, &str, &[Shape], OrcaSidebarEvent); 3] = [
    (
        "orca-usage",
        "activityBar.stats",
        crate::activity_bar::STATS,
        OrcaSidebarEvent::OpenUsage,
    ),
    (
        "orca-settings",
        "activityBar.settings",
        crate::activity_bar::SETTINGS,
        OrcaSidebarEvent::OpenSettings,
    ),
    (
        "orca-mobile",
        "activityBar.mobile",
        crate::activity_bar::MOBILE,
        OrcaSidebarEvent::OpenMobile,
    ),
];

pub struct OrcaProjectSidebar {
    store: Entity<AppStore>,
    catalog: Entity<WorktreeCatalog>,
    collapsed_projects: HashSet<String>,
    hovered_project: Option<String>,
    project_controls: HashMap<String, ProjectRowControls>,
    header_tooltips: Entity<IconTooltips>,
    footer_tooltips: Entity<IconTooltips>,
}

impl EventEmitter<OrcaSidebarEvent> for OrcaProjectSidebar {}

impl OrcaProjectSidebar {
    pub fn new(
        store: Entity<AppStore>,
        catalog: Entity<WorktreeCatalog>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&store, |_this: &mut Self, _, cx| {
            cx.notify();
        })
        .detach();
        cx.observe(&catalog, |_this: &mut Self, _, cx| {
            cx.notify();
        })
        .detach();
        cx.observe(&menu::layer(cx), |_this: &mut Self, _, cx| {
            cx.notify();
        })
        .detach();
        Self {
            store,
            catalog,
            collapsed_projects: HashSet::new(),
            hovered_project: None,
            project_controls: HashMap::new(),
            header_tooltips: cx.new(|_| IconTooltips::default()),
            footer_tooltips: cx.new(|_| IconTooltips::default()),
        }
    }

    fn render_top_actions(&self, cx: &mut Context<Self>) -> gpui::Div {
        let activity_enabled = global_agent_activity_enabled();
        let needs_you_count = if activity_enabled {
            self.store
                .read(cx)
                .agent_target_views()
                .iter()
                .filter(|target| agent_target_needs_user(target))
                .count()
        } else {
            0
        };
        let badge = if needs_you_count > 99 {
            "99+".to_string()
        } else {
            needs_you_count.to_string()
        };
        let agents_lane = div()
            .w(px(28.0))
            .h(px(18.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .when(needs_you_count > 0, |lane| {
                lane.child(
                    div()
                        .min_w(px(18.0))
                        .h(px(16.0))
                        .px(px(4.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.0))
                        .bg(ui::accent_subtle())
                        .text_size(ui::font_px(9.0))
                        .text_color(ui::color_warning())
                        .child(badge),
                )
            });

        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .p(px(8.0))
            .border_b_1()
            .border_color(ui::border_subtle())
            .child(
                nav_row(
                    "orca-search",
                    t("search", "searchButton"),
                    VectorIcon::new(SEARCH_ICON, px(NAV_ICON_SIZE))
                        .ink(ui::text_muted())
                        .into_any_element(),
                )
                .on_click(cx.listener(|_this, _event, _window, cx| {
                    cx.emit(OrcaSidebarEvent::OpenJumpPalette);
                })),
            )
            .when(activity_enabled, |actions| {
                actions.child(
                    nav_row(
                        "orca-agents",
                        "Agents",
                        VectorIcon::new(crate::activity_bar::SESSIONS, px(NAV_ICON_SIZE))
                            .ink(ui::text_muted())
                            .into_any_element(),
                    )
                    .child(agents_lane)
                    .on_click(cx.listener(|_this, _event, _window, cx| {
                        cx.emit(OrcaSidebarEvent::ToggleAgents);
                    })),
                )
            })
    }

    fn render_projects_header(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let store_for_add = self.store.clone();
        let options = IconTooltips::button(
            &self.header_tooltips,
            "orca-project-options-description",
            "Project options",
            small_icon_button(
                "orca-project-options",
                VectorIcon::new(MORE_ICON, px(13.0))
                    .ink(ui::text_muted())
                    .into_any_element(),
            )
            .on_click(move |event: &gpui::ClickEvent, window, cx| {
                cx.stop_propagation();
                menu::show(
                    event.position(),
                    vec![menu::item(t("app", "activityBar.ssh"), |window, cx| {
                        crate::ssh_panel::open(window, cx);
                    })],
                    window,
                    cx,
                );
            }),
            window,
            cx,
        );
        let add = IconTooltips::button(
            &self.header_tooltips,
            "orca-add-project-description",
            t("projectList", "menu.addProject"),
            small_icon_button(
                "orca-add-project",
                VectorIcon::new(PLUS_ICON, px(13.0))
                    .ink(ui::text_muted())
                    .into_any_element(),
            )
            .on_click(move |_event, window, cx| {
                crate::project_onboarding::open(store_for_add.clone(), None, window, cx);
            }),
            window,
            cx,
        );
        let tools = IconTooltips::group(
            &self.header_tooltips,
            div()
                .id("orca-project-header-tools")
                .flex()
                .items_center()
                .gap(px(2.0))
                .child(options)
                .child(add),
            window,
            cx,
        );
        div()
            .h(px(36.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .px(px(10.0))
            .border_b_1()
            .border_color(ui::border_subtle())
            .child(
                div()
                    .truncate()
                    .text_size(ui::font_px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(ui::text_muted())
                    .child(t("panels", "projects")),
            )
            .child(tools)
    }

    fn open_project_menu(&self, project_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(controls) = self.project_controls.get(project_id) else {
            return;
        };
        let Some(bounds) = controls.menu_bounds.get() else {
            return;
        };
        let Some(target) = ProjectSettingsTarget::capture(self.store.read(cx), project_id) else {
            return;
        };
        IconTooltips::reset(&controls.tooltips, window, cx);
        let store = self.store.clone();
        let catalog = self.catalog.clone();
        menu::show_anchored(
            project_menu_anchor(project_id),
            bounds,
            vec![menu::item(t("worktree", "settings.title"), move |window, cx| {
                crate::project_settings::open(
                    store.clone(),
                    catalog.clone(),
                    target.clone(),
                    window,
                    cx,
                );
            })],
            window,
            cx,
        );
    }

    fn render_project_row(
        &self,
        project: &ProjectWorktreeGroup,
        expanded: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let project_id = project.root_project_id.clone();
        let toggle_project_id = project.root_project_id.clone();
        let project_path = project.root_project_path.clone();
        let host_label = project.host_label.clone();
        let is_ssh = matches!(project.backend, CatalogBackend::Ssh { .. });
        let can_create_worktree = matches!(project.backend, CatalogBackend::Local);
        let settings_project_id = project.root_project_id.clone();
        let hover_project_id = project.root_project_id.clone();
        let controls = &self.project_controls[&project.root_project_id];
        let menu_anchor = project_menu_anchor(&project.root_project_id);
        let menu_open = menu::layer(cx).read(cx).is_anchored_to(&menu_anchor);
        let show_menu = show_project_menu_trigger(
            window.is_window_hovered()
                && self.hovered_project.as_deref() == Some(project.root_project_id.as_str()),
            window.is_window_active() && controls.focus.contains_focused(window, cx),
            menu_open,
        );
        let mut tools = div()
            .id(SharedString::from(format!(
                "orca-project-tools-{}",
                project.root_project_id
            )))
            .flex_none()
            .flex()
            .items_center();
        if can_create_worktree {
            tools = tools.child(IconTooltips::button(
                &controls.tooltips,
                SharedString::from(format!(
                    "orca-add-worktree-description-{}",
                    project.root_project_id
                )),
                t("worktree", "createTitle"),
                small_icon_button(
                    SharedString::from(format!(
                        "orca-add-worktree-{}",
                        project.root_project_id
                    )),
                    VectorIcon::new(PLUS_ICON, px(12.0))
                        .ink(ui::text_muted())
                        .into_any_element(),
                )
                .on_click(move |_event, window, cx| {
                    cx.stop_propagation();
                    crate::git_worktree::open(
                        project_path.clone(),
                        true,
                        Some(project_id.clone()),
                        move |cx| crate::worktree_catalog::force_refresh_global(cx),
                        window,
                        cx,
                    );
                }),
                window,
                cx,
            ));
        }
        let mut menu_slot = div().w(px(24.0)).h(px(24.0)).flex_none();
        if show_menu {
            let bounds = controls.menu_bounds.clone();
            menu_slot = menu_slot.child(IconTooltips::button(
                &controls.tooltips,
                SharedString::from(format!(
                    "orca-project-menu-description-{}",
                    project.root_project_id
                )),
                t("worktree", "settings.title"),
                small_icon_button(
                    menu_anchor,
                    VectorIcon::new(MORE_ICON, px(13.0))
                        .ink(ui::text_muted())
                        .into_any_element(),
                )
                .track_focus(&controls.menu_focus)
                .when(menu_open, |button| button.bg(ui::border_subtle()))
                .child(
                    canvas(move |rect, _, _| bounds.set(Some(rect)), |_, _, _, _| {})
                        .absolute()
                        .size_full(),
                )
                .on_click(cx.listener(move |this, _event, window, cx| {
                    cx.stop_propagation();
                    this.open_project_menu(&settings_project_id, window, cx);
                })),
                window,
                cx,
            ));
        }
        tools = tools.child(menu_slot);
        let tools = IconTooltips::group(&controls.tooltips, tools, window, cx);

        let leading_icon: AnyElement = if is_ssh {
            VectorIcon::new(crate::activity_bar::SSH, px(ROW_ICON_SIZE))
                .ink(ui::color_info())
                .into_any_element()
        } else {
            FileIcon::folder(expanded)
                .size(px(ROW_ICON_SIZE))
                .color(ui::color_folder())
                .into_any_element()
        };

        div()
            .id(SharedString::from(format!(
                "orca-project-{}",
                project.root_project_id
            )))
            .h(px(36.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(7.0))
            .px(px(9.0))
            .cursor_pointer()
            .text_size(ui::font_px(12.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(ui::text_primary())
            .track_focus(&controls.focus)
            .tab_index(0)
            .focus(|row| row.bg(ui::accent_subtle()))
            .hover(|row| row.bg(ui::border_subtle()))
            .on_hover(cx.listener(move |this, hovered, _window, cx| {
                if *hovered {
                    this.hovered_project = Some(hover_project_id.clone());
                } else if this.hovered_project.as_deref() == Some(hover_project_id.as_str()) {
                    this.hovered_project = None;
                } else {
                    return;
                }
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _event, _window, cx| {
                if !this.collapsed_projects.remove(&toggle_project_id) {
                    this.collapsed_projects.insert(toggle_project_id.clone());
                }
                cx.notify();
            }))
            .child(
                VectorIcon::new(
                    if expanded {
                        CHEVRON_DOWN
                    } else {
                        CHEVRON_RIGHT
                    },
                    px(10.0),
                )
                .ink(ui::text_muted()),
            )
            .child(div().w(px(16.0)).flex_none().child(leading_icon))
            .child(
                div()
                    .id(SharedString::from(format!(
                        "orca-project-name-{}",
                        project.root_project_id
                    )))
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .tooltip({
                        let path = project.root_project_path.clone();
                        move |window, cx| Tooltip::new(path.clone()).build(window, cx)
                    })
                    .child(project.root_project_name.clone()),
            )
            .child(
                div()
                    .w(px(14.0))
                    .h(px(14.0))
                    .flex_none()
                    .when(project.refreshing, |lane| {
                        lane.child(
                            div()
                                .id(SharedString::from(format!(
                                    "orca-refresh-{}",
                                    project.root_project_id
                                )))
                                .tooltip(|window, cx| {
                                    Tooltip::new(t("worktree", "settings.refreshing"))
                                        .build(window, cx)
                                })
                                .child(
                                    VectorIcon::new(
                                        mt_ui::icons::usage_glyphs::ICON_REFRESH,
                                        px(12.0),
                                    )
                                    .ink(ui::text_muted()),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .max_w(px(62.0))
                    .truncate()
                    .text_size(ui::font_px(9.5))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(if project.warning.is_some() {
                        ui::color_warning()
                    } else {
                        ui::text_muted()
                    })
                    .child(host_label),
            )
            .child(tools)
    }

    fn render_worktree_row(
        &self,
        parent_id: &str,
        worktree: &WorktreeCatalogRow,
        active_project_id: Option<&str>,
        agents: &[AgentTargetView],
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let (indicator, needs_attention) = {
            let store = self.store.read(cx);
            worktree
                .target
                .configured_project_id
                .as_deref()
                .and_then(|id| store.project_state(id))
                .map(|state| {
                    (
                        worktree_agent_indicator(agents, &state.all_panes()),
                        state.needs_attention,
                    )
                })
                .unwrap_or_else(|| (worktree_agent_indicator(agents, &[]), false))
        };
        let is_active = worktree.target.configured_project_id.as_deref() == active_project_id;
        let path_text = worktree.target.execution_path.clone();
        let mut detail_parts = Vec::new();
        if worktree.is_main {
            detail_parts.push(t("worktree", "mainRepo").to_string());
        }
        if worktree.is_sparse {
            detail_parts.push("sparse".into());
        }
        if worktree.is_detached {
            detail_parts.push("detached".into());
        }
        if worktree.is_locked {
            detail_parts.push("locked".into());
        }
        if worktree.is_prunable {
            detail_parts.push("prunable".into());
        }
        if worktree.path_state == WorktreePathState::Missing {
            detail_parts.push("missing".into());
        }
        if worktree.last_known {
            detail_parts.push("last known".into());
        }
        detail_parts.push(path_text.clone());
        let detail = detail_parts.join(" | ");
        let status_lane = div()
            .id(SharedString::from(format!(
                "orca-worktree-status-{parent_id}-{path_text}"
            )))
            .w(px(18.0))
            .h(px(14.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(2.0))
            .tooltip(move |window, cx| {
                let connectivity = match indicator.connectivity {
                    None => "",
                    Some(AgentConnectivity::Live) => " | Live",
                    Some(AgentConnectivity::Stale) => " | Stale",
                    Some(AgentConnectivity::Disconnected) => " | Offline",
                };
                Tooltip::new(format!("{}{connectivity}", indicator.label())).build(window, cx)
            })
            .when(indicator.activity != SidebarActivity::Idle, |lane| {
                lane.child(indicator.icon())
            })
            .when(
                indicator.activity == SidebarActivity::Idle && needs_attention,
                |lane| {
                    lane.child(
                        div()
                            .w(px(6.0))
                            .h(px(6.0))
                            .rounded_full()
                            .bg(ui::color_success()),
                    )
                },
            )
            .when(
                indicator.activity == SidebarActivity::Idle
                    && !needs_attention
                    && worktree.last_known,
                |lane| {
                    lane.child(
                        div()
                            .w(px(6.0))
                            .h(px(6.0))
                            .rounded_full()
                            .bg(ui::color_warning()),
                    )
                },
            )
            .when_some(indicator.connectivity, |lane, connectivity| {
                lane.child(div().w(px(4.0)).h(px(4.0)).flex_none().rounded_full().bg(
                    match connectivity {
                        AgentConnectivity::Live => ui::color_success(),
                        AgentConnectivity::Stale => ui::color_warning(),
                        AgentConnectivity::Disconnected => ui::text_muted(),
                    },
                ))
            });

        let parent_id = parent_id.to_string();
        let target = worktree.target.clone();
        let catalog = self.catalog.clone();
        let store = self.store.clone();
        let is_remote = matches!(worktree.target.backend, CatalogBackend::Ssh { .. });
        let selectable = worktree.selectable;
        let icon = VectorIcon::new(
            if is_remote {
                crate::activity_bar::SSH
            } else {
                crate::activity_bar::GIT
            },
            px(ROW_ICON_SIZE),
        )
        .ink(if is_active {
            ui::accent()
        } else {
            ui::text_muted()
        });

        div()
            .id(SharedString::from(format!(
                "orca-worktree-{}-{}",
                parent_id,
                worktree.target.row_key.replace('\0', "-")
            )))
            .min_h(px(43.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .pl(px(22.0))
            .pr(px(9.0))
            .py(px(5.0))
            .when(selectable, |row| row.cursor_pointer())
            .when(!selectable, |row| row.opacity(0.45))
            .when(is_active, |row| {
                row.bg(ui::accent_subtle()).text_color(ui::accent())
            })
            .when(!is_active && selectable, |row| {
                row.text_color(ui::text_secondary())
                    .hover(|row| row.bg(ui::border_subtle()).text_color(ui::text_primary()))
            })
            .tooltip({
                let path = worktree.target.host_visible_path.clone();
                move |window, cx| Tooltip::new(path.clone()).build(window, cx)
            })
            .when(selectable, |row| {
                row.on_click(move |_event, window, cx| {
                    if let Err(message) = crate::worktree_catalog::activate_target(
                        &catalog, &store, &target, window, cx,
                    ) {
                        crate::toast::push_message(
                            crate::notify::ToastKind::WslInfo,
                            target.root_project_id.clone(),
                            target.suggested_name.clone(),
                            message,
                            cx,
                        );
                    }
                })
            })
            .child(status_lane)
            .child(div().w(px(16.0)).flex_none().child(icon))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .truncate()
                            .text_size(ui::font_px(12.0))
                            .font_weight(if needs_attention {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .child(worktree.label.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(ui::font_px(9.75))
                            .text_color(ui::text_muted())
                            .child(detail),
                    ),
            )
    }

    fn render_agent_row(
        &self,
        agent: &AgentTargetView,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let run_id = agent.run_id.clone();
        let store = self.store.clone();
        let provider = agent.provider.as_str();
        let vendor = match provider {
            "codex" => Some(AiVendor::OpenAi),
            other => {
                AiVendor::from_session_type(other).or_else(|| AiVendor::infer(Some(other), None))
            }
        };
        let activity = match agent.activity {
            AgentActivity::Starting => "Starting",
            AgentActivity::Working => "Working",
            AgentActivity::Blocked => "Needs you",
            AgentActivity::Waiting => "Waiting",
            AgentActivity::Done => "Done",
            AgentActivity::Failed => "Failed",
            AgentActivity::Interrupted => "Interrupted",
            AgentActivity::Exited => "Exited",
            AgentActivity::Unknown => "Unknown",
        };
        let activity_color = if agent.attention
            || matches!(
                agent.activity,
                AgentActivity::Blocked | AgentActivity::Failed
            ) {
            ui::color_warning()
        } else if matches!(
            agent.activity,
            AgentActivity::Starting | AgentActivity::Working
        ) {
            ui::accent()
        } else if matches!(agent.activity, AgentActivity::Done | AgentActivity::Waiting) {
            ui::color_success()
        } else {
            ui::text_muted()
        };
        let connectivity = match agent.connectivity {
            AgentConnectivity::Live => "Live",
            AgentConnectivity::Stale => "Stale",
            AgentConnectivity::Disconnected => "Offline",
        };
        let connectivity_color = match agent.connectivity {
            AgentConnectivity::Live => ui::color_success(),
            AgentConnectivity::Stale => ui::color_warning(),
            AgentConnectivity::Disconnected => ui::text_muted(),
        };
        let tooltip = format!(
            "{} · {} · {}",
            agent.provider, agent.pane_label, connectivity
        );

        div()
            .id(SharedString::from(format!("orca-agent-{}", agent.run_id)))
            .h(px(30.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(7.0))
            .pl(px(50.0))
            .pr(px(9.0))
            .cursor_pointer()
            .text_size(ui::font_px(10.5))
            .text_color(ui::text_secondary())
            .hover(|row| row.bg(ui::border_subtle()).text_color(ui::text_primary()))
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .on_click(cx.listener(move |_this, _event, window, cx| {
                AppStore::activate_agent_run(&store, &run_id, window, cx);
            }))
            .child(
                BrandIcon::new(vendor)
                    .size(px(13.0))
                    .color(ui::text_secondary()),
            )
            .child(
                div()
                    .w(px(6.0))
                    .h(px(6.0))
                    .flex_none()
                    .rounded_full()
                    .bg(activity_color),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(agent.pane_label.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .text_size(ui::font_px(9.0))
                    .text_color(ui::text_muted())
                    .child(
                        div()
                            .w(px(5.0))
                            .h(px(5.0))
                            .rounded_full()
                            .bg(connectivity_color),
                    )
                    .child(activity),
            )
    }

    fn render_footer(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let mut footer = div()
            .id("orca-footer")
            .h(px(48.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(8.0))
            .border_t_1()
            .border_color(ui::border_subtle());
        for (id, label, icon, action) in FOOTER_ACTIONS {
            footer = footer.child(IconTooltips::button(
                &self.footer_tooltips,
                SharedString::from(format!("{id}-description")),
                t("app", label),
                small_icon_button(
                    id,
                    VectorIcon::new(icon, px(18.0))
                        .ink(ui::text_muted())
                        .into_any_element(),
                )
                .w(px(32.0))
                .h(px(32.0))
                .on_click(cx.listener(move |_this, _event, _window, cx| {
                    cx.emit(action);
                })),
                window,
                cx,
            ));
        }
        IconTooltips::group(&self.footer_tooltips, footer, window, cx)
    }
}

impl Render for OrcaProjectSidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (projects, active_project_id, mut agents_by_project) = {
            let store = self.store.read(cx);
            let agents_by_project = if orca_worktree_context_enabled() {
                group_agent_targets_by_project(store.agent_target_views())
            } else {
                HashMap::new()
            };
            (
                self.catalog.read(cx).groups(cx),
                store.active_project_id.clone(),
                agents_by_project,
            )
        };

        let live_projects: HashSet<String> = projects
            .iter()
            .map(|project| project.root_project_id.clone())
            .collect();
        let menu = menu::layer(cx);
        let removed_open_anchor = self.project_controls.keys().any(|id| {
            !live_projects.contains(id)
                && menu.read(cx).is_anchored_to(&project_menu_anchor(id))
        });
        if removed_open_anchor {
            menu::close(window, cx);
        }
        self.project_controls
            .retain(|id, _| live_projects.contains(id));
        if self
            .hovered_project
            .as_ref()
            .is_some_and(|id| !live_projects.contains(id))
        {
            self.hovered_project = None;
        }
        for id in live_projects {
            self.project_controls
                .entry(id)
                .or_insert_with(|| ProjectRowControls {
                    // GPUI takes tab-stop state from explicit tracked handles.
                    focus: cx.focus_handle().tab_stop(true),
                    menu_focus: cx.focus_handle().tab_stop(true),
                    menu_bounds: Rc::new(Cell::new(None)),
                    tooltips: cx.new(|_| IconTooltips::default()),
                });
        }

        let mut rows = div()
            .id("orca-project-rows")
            .flex_1()
            .overflow_y_scroll()
            .py(px(3.0));
        for project in projects {
            let expanded = !self.collapsed_projects.contains(&project.root_project_id);
            rows = rows.child(self.render_project_row(&project, expanded, window, cx));
            if expanded {
                let hidden = self
                    .store
                    .read(cx)
                    .project(&project.root_project_id)
                    .map(|root| root.hidden_worktrees.clone())
                    .unwrap_or_default();
                for worktree in &project.rows {
                    if !sidebar_visible(worktree, &hidden) {
                        continue;
                    }
                    rows = rows.child(
                        self.render_worktree_row(
                            &project.root_project_id,
                            worktree,
                            active_project_id.as_deref(),
                            worktree
                                .target
                                .configured_project_id
                                .as_deref()
                                .and_then(|id| agents_by_project.get(id))
                                .map(Vec::as_slice)
                                .unwrap_or(&[]),
                            cx,
                        ),
                    );
                    if let Some(project_id) = worktree.target.configured_project_id.as_deref()
                        && let Some(agents) = agents_by_project.remove(project_id)
                    {
                        for agent in &agents {
                            rows = rows.child(self.render_agent_row(agent, cx));
                        }
                    }
                }
            }
        }

        div()
            .id("orca-project-sidebar")
            .tab_group()
            .w(px(WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .overflow_hidden()
            .border_r_1()
            .border_color(ui::border_default())
            .bg(ui::bg_surface())
            .on_key_down(|event: &KeyDownEvent, window, cx| {
                let modifiers = event.keystroke.modifiers;
                if event.keystroke.key == "tab"
                    && !modifiers.control
                    && !modifiers.platform
                    && !modifiers.alt
                {
                    cx.stop_propagation();
                    if modifiers.shift {
                        window.focus_prev();
                    } else {
                        window.focus_next();
                    }
                }
            })
            .child(self.render_top_actions(cx))
            .child(self.render_projects_header(window, cx))
            .child(rows)
            .child(self.render_footer(window, cx))
    }
}

#[cfg(test)]
mod navigation_tests {
    use super::*;

    #[test]
    fn project_menu_trigger_follows_hover_focus_or_its_open_menu() {
        assert!(!show_project_menu_trigger(false, false, false));
        assert!(show_project_menu_trigger(true, false, false));
        assert!(show_project_menu_trigger(false, true, false));
        assert!(show_project_menu_trigger(false, false, true));
        assert!(show_project_menu_trigger(true, true, true));
    }

    #[test]
    fn project_menu_anchor_is_project_specific() {
        assert_eq!(
            project_menu_anchor("project-a"),
            project_menu_anchor("project-a"),
        );
        assert_ne!(
            project_menu_anchor("project-a"),
            project_menu_anchor("project-b"),
        );
    }

    #[test]
    fn footer_icons_keep_distinct_commands_and_existing_descriptions() {
        let actions: Vec<_> = FOOTER_ACTIONS
            .iter()
            .map(|(id, label, icon, action)| {
                assert!(!icon.is_empty());
                for locale in mt_i18n::Locale::ALL {
                    assert!(
                        mt_i18n::lookup(locale, "app", label).is_some_and(|text| !text.is_empty())
                    );
                }
                (*id, *action)
            })
            .collect();
        assert_eq!(
            actions,
            vec![
                ("orca-usage", OrcaSidebarEvent::OpenUsage),
                ("orca-settings", OrcaSidebarEvent::OpenSettings),
                ("orca-mobile", OrcaSidebarEvent::OpenMobile),
            ],
        );
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;

    fn target(activity: AgentActivity, connectivity: AgentConnectivity) -> AgentTargetView {
        use mt_identity::{
            AgentEventId, AgentRunId, ExecutionHostId, HostInstallId, PaneKey, RepoId, TabId,
            TerminalIncarnationId, TerminalSessionId, WorktreeId,
        };

        let host = ExecutionHostId::derive("test", &HostInstallId::new());
        let repo = RepoId::derive(&host, "/repo/.git");
        let pane_key = PaneKey::new();
        AgentTargetView {
            run_id: AgentRunId::new(),
            last_event_id: AgentEventId::new(),
            project_id: "project".into(),
            project_name: "project".into(),
            root_project_name: "project".into(),
            worktree_name: "main".into(),
            host_label: "host".into(),
            pane_id: pane_key.to_string(),
            pane_label: "terminal".into(),
            route: mt_ai::AgentRoute {
                execution_host_id: host,
                worktree_id: WorktreeId::derive(&repo, "/repo", None),
                tab_id: TabId::new(),
                pane_key,
                terminal_session_id: TerminalSessionId::new(),
                terminal_incarnation_id: TerminalIncarnationId::new(),
            },
            provider: "codex".parse().unwrap(),
            provider_session_id: None,
            activity,
            connectivity,
            evidence: AgentEvidence::ProcessAttested,
            received_at_unix_ms: 1,
            attention: false,
            unread: false,
        }
    }

    fn pane_for_target(target: &AgentTargetView, status: PaneStatus) -> PaneState {
        let mut pane = PaneState::from_identity(
            "shell",
            target.route.pane_key.clone(),
            target.route.terminal_session_id.clone(),
            Some(target.route.terminal_incarnation_id.clone()),
        );
        pane.status = status;
        pane
    }

    #[test]
    fn rich_evidence_overrides_legacy_working_fallback() {
        for (activity, connectivity, expected) in [
            (
                AgentActivity::Working,
                AgentConnectivity::Disconnected,
                SidebarActivity::LastKnownWork,
            ),
            (
                AgentActivity::Working,
                AgentConnectivity::Stale,
                SidebarActivity::LastKnownWork,
            ),
            (
                AgentActivity::Waiting,
                AgentConnectivity::Live,
                SidebarActivity::Waiting,
            ),
            (
                AgentActivity::Blocked,
                AgentConnectivity::Live,
                SidebarActivity::Attention,
            ),
            (
                AgentActivity::Failed,
                AgentConnectivity::Live,
                SidebarActivity::Failed,
            ),
        ] {
            let agent = target(activity, connectivity);
            let pane = pane_for_target(&agent, PaneStatus::AiWorking);
            let indicator = worktree_agent_indicator(&[agent], &[&pane]);
            assert_eq!(indicator.activity, expected);
            assert_ne!(indicator.activity, SidebarActivity::Working);
            assert_eq!(indicator.connectivity, Some(connectivity));
        }
    }

    #[test]
    fn rich_evidence_preserves_other_panes_legacy_work_error_and_attention() {
        for (activity, connectivity) in [
            (AgentActivity::Working, AgentConnectivity::Disconnected),
            (AgentActivity::Waiting, AgentConnectivity::Live),
        ] {
            let agent = target(activity, connectivity);
            let rich_pane = pane_for_target(&agent, PaneStatus::AiWorking);
            let mut other_pane = PaneState::new("shell");
            for (status, attention, expected) in [
                (PaneStatus::AiWorking, false, SidebarActivity::Working),
                (PaneStatus::Error, false, SidebarActivity::Failed),
                (PaneStatus::AiWorking, true, SidebarActivity::Attention),
            ] {
                other_pane.status = status;
                other_pane.attention = attention;
                let indicator = worktree_agent_indicator(
                    std::slice::from_ref(&agent),
                    &[&rich_pane, &other_pane],
                );
                assert_eq!(indicator.activity, expected);
                assert_eq!(indicator.connectivity, Some(connectivity));
            }
        }
    }

    #[test]
    fn ended_or_restored_runs_do_not_hide_the_panes_current_legacy_status() {
        for (activity, evidence) in [
            (AgentActivity::Exited, AgentEvidence::ProcessAttested),
            (AgentActivity::Working, AgentEvidence::RestoredHistory),
        ] {
            let mut agent = target(activity, AgentConnectivity::Disconnected);
            agent.evidence = evidence;
            let pane = pane_for_target(&agent, PaneStatus::Error);
            let indicator = worktree_agent_indicator(&[agent], &[&pane]);
            assert_eq!(indicator.activity, SidebarActivity::Failed);
            assert_eq!(indicator.connectivity, None);
        }
    }

    #[test]
    fn same_pane_hook_does_not_suppress_independent_process_activity() {
        for (hook_activity, process_activity, expected) in [
            (
                AgentActivity::Done,
                AgentActivity::Working,
                SidebarActivity::Working,
            ),
            (
                AgentActivity::Done,
                AgentActivity::Waiting,
                SidebarActivity::Waiting,
            ),
            (
                AgentActivity::Blocked,
                AgentActivity::Working,
                SidebarActivity::Attention,
            ),
            (
                AgentActivity::Failed,
                AgentActivity::Working,
                SidebarActivity::Failed,
            ),
        ] {
            let mut hook = target(hook_activity, AgentConnectivity::Live);
            hook.evidence = AgentEvidence::Hook;
            let mut process = target(process_activity, AgentConnectivity::Live);
            process.provider = "claude".parse().unwrap();
            process.route = hook.route.clone();
            process.pane_id = hook.pane_id.clone();
            let pane = pane_for_target(&hook, PaneStatus::AiWorking);
            assert_eq!(
                worktree_agent_indicator(&[hook, process], &[&pane]).activity,
                expected
            );
        }
    }

    #[test]
    fn only_live_starting_and_working_use_the_working_indicator() {
        for activity in [AgentActivity::Starting, AgentActivity::Working] {
            assert_eq!(
                SidebarIndicator::from_agent(activity, AgentConnectivity::Live, false).activity,
                SidebarActivity::Working
            );
            for connectivity in [AgentConnectivity::Stale, AgentConnectivity::Disconnected] {
                let indicator = SidebarIndicator::from_agent(activity, connectivity, false);
                assert_eq!(indicator.activity, SidebarActivity::LastKnownWork);
                assert_eq!(indicator.connectivity, Some(connectivity));
            }
            assert_eq!(
                SidebarIndicator::from_agent(activity, AgentConnectivity::Live, true).activity,
                SidebarActivity::Attention
            );
        }
    }

    #[test]
    fn waiting_attention_completion_and_error_are_steady() {
        for (activity, expected) in [
            (AgentActivity::Waiting, SidebarActivity::Waiting),
            (AgentActivity::Blocked, SidebarActivity::Attention),
            (AgentActivity::Done, SidebarActivity::Done),
            (AgentActivity::Failed, SidebarActivity::Failed),
        ] {
            for connectivity in [
                AgentConnectivity::Live,
                AgentConnectivity::Stale,
                AgentConnectivity::Disconnected,
            ] {
                let indicator = SidebarIndicator::from_agent(activity, connectivity, false);
                assert_eq!(indicator.activity, expected);
            }
        }
    }

    #[test]
    fn no_evidence_is_idle_and_legacy_fallback_is_preserved() {
        for (fallback, expected) in [
            (PaneStatus::Idle, SidebarActivity::Idle),
            (PaneStatus::AiIdle, SidebarActivity::Done),
            (PaneStatus::AiWorking, SidebarActivity::Working),
            (PaneStatus::Error, SidebarActivity::Failed),
        ] {
            let mut pane = PaneState::new("shell");
            pane.status = fallback;
            let indicator = worktree_agent_indicator(&[], &[&pane]);
            assert_eq!(indicator.activity, expected);
            assert_eq!(indicator.connectivity, None);
        }
    }

    #[test]
    fn attention_and_error_override_work_without_rewriting_connectivity() {
        let mut indicator =
            SidebarIndicator::from_agent(AgentActivity::Working, AgentConnectivity::Live, false);
        indicator.merge(SidebarIndicator::from_agent(
            AgentActivity::Blocked,
            AgentConnectivity::Disconnected,
            false,
        ));
        assert_eq!(indicator.activity, SidebarActivity::Attention);
        assert_eq!(
            indicator.connectivity,
            Some(AgentConnectivity::Disconnected)
        );
        indicator.merge(SidebarIndicator::from_agent(
            AgentActivity::Failed,
            AgentConnectivity::Live,
            false,
        ));
        assert_eq!(indicator.activity, SidebarActivity::Failed);
        assert_eq!(
            indicator.connectivity,
            Some(AgentConnectivity::Disconnected)
        );
    }
}
