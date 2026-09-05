//! Orca-aligned project navigation: configured project -> Git worktree.
//!
//! The entity owns only presentation state. Project, terminal, and file state
//! remain in [`AppStore`], while the shared [`WorktreeCatalog`] owns Git facts.

use std::collections::{HashMap, HashSet};

use gpui::{
    AnyElement, Context, Entity, EventEmitter, FontWeight, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use mt_ai::{AgentActivity, AgentConnectivity};
use mt_project::worktree::WorktreePathState;
use mt_ui::icons::vector::{Geom, Ink, Shape, VectorIcon};
use mt_ui::icons::{AiVendor, BrandIcon, FileIcon};
use mt_ui::tooltip::Tooltip;

use crate::agent_activity::{agent_target_needs_user, global_agent_activity_enabled};
use crate::i18n::t;
use crate::menu;
use crate::store::{AgentTargetView, AppStore, orca_worktree_context_enabled};
use crate::tree::PaneStatus;
use crate::ui;
use crate::worktree_catalog::{
    CatalogBackend, ProjectWorktreeGroup, WorktreeCatalog, WorktreeCatalogRow,
};

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
    tooltip: SharedString,
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
        .text_color(ui::text_muted())
        .hover(|button| {
            button
                .bg(ui::border_subtle())
                .text_color(ui::text_primary())
        })
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .child(icon)
}

pub struct OrcaProjectSidebar {
    store: Entity<AppStore>,
    catalog: Entity<WorktreeCatalog>,
    collapsed_projects: HashSet<String>,
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
        Self {
            store,
            catalog,
            collapsed_projects: HashSet::new(),
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
                }),
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

    fn render_projects_header(&self, _cx: &mut Context<Self>) -> gpui::Div {
        let store_for_add = self.store.clone();
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
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .child(
                        small_icon_button(
                            "orca-project-options",
                            "Project options".into(),
                            VectorIcon::new(MORE_ICON, px(13.0))
                                .ink(ui::text_muted())
                                .into_any_element(),
                        )
                        .on_click(
                            move |event: &gpui::ClickEvent, window, cx| {
                                cx.stop_propagation();
                                menu::show(
                                    event.position(),
                                    vec![
                                        menu::item(t("app", "activityBar.ssh"), |window, cx| {
                                            crate::ssh_panel::open(window, cx);
                                        }),
                                        menu::item(t("app", "activityBar.mobile"), |window, cx| {
                                            crate::mobile_panel::open(window, cx);
                                        }),
                                    ],
                                    window,
                                    cx,
                                );
                            },
                        ),
                    )
                    .child(
                        small_icon_button(
                            "orca-add-project",
                            t("projectList", "menu.addProject").into(),
                            VectorIcon::new(PLUS_ICON, px(13.0))
                                .ink(ui::text_muted())
                                .into_any_element(),
                        )
                        .on_click(move |_event, window, cx| {
                            crate::project_onboarding::open(
                                store_for_add.clone(),
                                None,
                                window,
                                cx,
                            );
                        }),
                    ),
            )
    }

    fn render_project_row(
        &self,
        project: &ProjectWorktreeGroup,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let project_id = project.root_project_id.clone();
        let toggle_project_id = project.root_project_id.clone();
        let project_path = project.root_project_path.clone();
        let host_label = project.host_label.clone();
        let is_ssh = matches!(project.backend, CatalogBackend::Ssh { .. });
        let can_create_worktree = matches!(project.backend, CatalogBackend::Local);

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
            .hover(|row| row.bg(ui::border_subtle()))
            .tooltip({
                let path = project.root_project_path.clone();
                move |window, cx| Tooltip::new(path.clone()).build(window, cx)
            })
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
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(project.root_project_name.clone()),
            )
            .when(project.refreshing, |row| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(ui::font_px(9.0))
                        .text_color(ui::text_muted())
                        .child("Refreshing"),
                )
            })
            .when(can_create_worktree, |row| {
                row.child(
                    small_icon_button(
                        SharedString::from(format!(
                            "orca-add-worktree-{}",
                            project.root_project_id
                        )),
                        t("worktree", "createTitle").into(),
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
                            move |cx| {
                                crate::worktree_catalog::force_refresh_global(cx);
                            },
                            window,
                            cx,
                        );
                    }),
                )
            })
            .child(
                div()
                    .max_w(px(92.0))
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
    }

    fn render_worktree_row(
        &self,
        parent_id: &str,
        worktree: &WorktreeCatalogRow,
        active_project_id: Option<&str>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let (status, needs_attention) = {
            let store = self.store.read(cx);
            worktree
                .target
                .configured_project_id
                .as_deref()
                .and_then(|id| store.project_state(id))
                .map(|state| (state.status, state.needs_attention))
                .unwrap_or((PaneStatus::Idle, false))
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
            .w(px(18.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .when(status != PaneStatus::Idle, |lane| {
                lane.child(ui::status_dot(status))
            })
            .when(status == PaneStatus::Idle && needs_attention, |lane| {
                lane.child(
                    div()
                        .w(px(6.0))
                        .h(px(6.0))
                        .rounded_full()
                        .bg(ui::color_success()),
                )
            })
            .when(status == PaneStatus::Idle && !needs_attention && worktree.last_known, |lane| {
                lane.child(
                    div()
                        .w(px(6.0))
                        .h(px(6.0))
                        .rounded_full()
                        .bg(ui::color_warning()),
                )
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

    fn render_footer(&self, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .p(px(8.0))
            .border_t_1()
            .border_color(ui::border_subtle())
            .child(
                nav_row(
                    "orca-usage",
                    "Usage",
                    VectorIcon::new(crate::activity_bar::STATS, px(NAV_ICON_SIZE))
                        .ink(ui::text_muted())
                        .into_any_element(),
                )
                .on_click(cx.listener(|_this, _event, _window, cx| {
                    cx.emit(OrcaSidebarEvent::OpenUsage);
                })),
            )
            .child(
                nav_row(
                    "orca-settings",
                    t("app", "activityBar.settings"),
                    VectorIcon::new(crate::activity_bar::SETTINGS, px(NAV_ICON_SIZE))
                        .ink(ui::text_muted())
                        .into_any_element(),
                )
                .on_click(cx.listener(|_this, _event, _window, cx| {
                    cx.emit(OrcaSidebarEvent::OpenSettings);
                })),
            )
    }
}

impl Render for OrcaProjectSidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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

        let mut rows = div()
            .id("orca-project-rows")
            .flex_1()
            .overflow_y_scroll()
            .py(px(3.0));
        for project in projects {
            let expanded = !self
                .collapsed_projects
                .contains(&project.root_project_id);
            rows = rows.child(self.render_project_row(&project, expanded, cx));
            if expanded {
                for worktree in &project.rows {
                    rows = rows.child(self.render_worktree_row(
                        &project.root_project_id,
                        worktree,
                        active_project_id.as_deref(),
                        cx,
                    ));
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
            .w(px(WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .overflow_hidden()
            .border_r_1()
            .border_color(ui::border_default())
            .bg(ui::bg_surface())
            .child(self.render_top_actions(cx))
            .child(self.render_projects_header(cx))
            .child(rows)
            .child(self.render_footer(cx))
    }
}
