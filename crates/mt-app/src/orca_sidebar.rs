//! Orca-aligned project navigation: configured project -> Git worktree.
//!
//! The entity owns only presentation state and catalog snapshots. Project,
//! terminal, and file state remain in [`AppStore`], while Git facts remain in
//! [`mt_project::worktree`].

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, Context, Entity, EventEmitter, FontWeight, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use mt_ai::{AgentActivity, AgentConnectivity};
use mt_config::{ProjectConfig, ProjectTreeItem};
use mt_project::worktree::{WorktreeFact, WorktreePathState, WorktreeScan};
use mt_ui::icons::vector::{Geom, Ink, Shape, VectorIcon};
use mt_ui::icons::{AiVendor, BrandIcon, FileIcon};
use mt_ui::tooltip::Tooltip;

use crate::agent_activity::{agent_target_needs_user, global_agent_activity_enabled};
use crate::i18n::t;
use crate::menu;
use crate::store::{AgentTargetView, AppStore, orca_worktree_context_enabled};
use crate::tree::PaneStatus;
use crate::ui;

/// Fixed shell width used by `Workspace` and the Agents overlay anchor.
pub const WIDTH: f32 = 300.0;

const NAV_ICON_SIZE: f32 = 15.0;
const ROW_ICON_SIZE: f32 = 14.0;

/// Actions owned by `Workspace` rather than by the project sidebar itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrcaSidebarEvent {
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

#[derive(Clone)]
struct CatalogSnapshot {
    repo_path: String,
    scan: WorktreeScan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProjectRowModel {
    id: String,
    name: String,
    path: String,
    is_remote: bool,
    worktrees: Vec<WorktreeRowModel>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorktreeRowModel {
    path: PathBuf,
    label: String,
    configured_project_id: Option<String>,
    is_main: bool,
    is_remote: bool,
    is_sparse: bool,
    selectable: bool,
}

#[derive(Clone)]
struct ScanTarget {
    project_id: String,
    path: String,
}

struct ScanCompletion {
    target: ScanTarget,
    result: Result<WorktreeScan, String>,
}

/// Flatten saved groups without rendering group rows or honoring their visual
/// collapse state. Unknown tree IDs are ignored and unlisted projects retain
/// their configured order at the end.
fn ordered_top_level_project_ids(
    projects: &[ProjectConfig],
    tree: Option<&[ProjectTreeItem]>,
) -> Vec<String> {
    let known_ids: HashSet<&str> = projects.iter().map(|project| project.id.as_str()).collect();
    let top_level_ids: HashSet<&str> = projects
        .iter()
        .filter(|project| {
            project
                .parent_project_id
                .as_deref()
                .is_none_or(|parent| !known_ids.contains(parent))
        })
        .map(|project| project.id.as_str())
        .collect();

    fn walk(
        items: &[ProjectTreeItem],
        top_level_ids: &HashSet<&str>,
        seen: &mut HashSet<String>,
        out: &mut Vec<String>,
    ) {
        for item in items {
            match item {
                ProjectTreeItem::ProjectId(id) => {
                    if top_level_ids.contains(id.as_str()) && seen.insert(id.clone()) {
                        out.push(id.clone());
                    }
                }
                ProjectTreeItem::Group(group) => {
                    walk(&group.children, top_level_ids, seen, out);
                }
            }
        }
    }

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    walk(tree.unwrap_or(&[]), &top_level_ids, &mut seen, &mut out);
    for project in projects {
        if top_level_ids.contains(project.id.as_str()) && seen.insert(project.id.clone()) {
            out.push(project.id.clone());
        }
    }
    out
}

fn normalized_path(path: &Path) -> String {
    mt_project::worktree::normalize_path_for_comparison(&path.to_string_lossy())
}

fn configured_local_project_for_path<'a>(
    projects: &'a [ProjectConfig],
    path: &Path,
) -> Option<&'a ProjectConfig> {
    let target = normalized_path(path);
    projects.iter().find(|project| {
        project.ssh_connection_id.is_none()
            && mt_project::worktree::normalize_path_for_comparison(&project.path) == target
    })
}

fn configured_local_project_for_activation<'a>(
    projects: &'a [ProjectConfig],
    configured_project_id: Option<&str>,
    path: &Path,
) -> Option<&'a ProjectConfig> {
    let target = normalized_path(path);
    configured_project_id
        .and_then(|project_id| {
            projects.iter().find(|project| {
                project.id == project_id
                    && project.ssh_connection_id.is_none()
                    && mt_project::worktree::normalize_path_for_comparison(&project.path) == target
            })
        })
        .or_else(|| configured_local_project_for_path(projects, path))
}

fn configured_children<'a>(
    projects: &'a [ProjectConfig],
    parent_id: &str,
) -> impl Iterator<Item = &'a ProjectConfig> {
    projects.iter().filter(move |project| {
        project.ssh_connection_id.is_none()
            && project.parent_project_id.as_deref() == Some(parent_id)
    })
}

fn short_branch(branch_ref: Option<&str>) -> Option<String> {
    branch_ref.map(|branch| {
        branch
            .strip_prefix("refs/heads/")
            .unwrap_or(branch)
            .to_string()
    })
}

fn path_label(path: &Path, fallback: &str) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn configured_worktree_row(project: &ProjectConfig, is_main: bool) -> WorktreeRowModel {
    WorktreeRowModel {
        path: PathBuf::from(&project.path),
        label: project.name.clone(),
        configured_project_id: Some(project.id.clone()),
        is_main,
        is_remote: project.ssh_connection_id.is_some(),
        is_sparse: false,
        selectable: true,
    }
}

fn catalog_worktree_row<'a>(
    fact: &WorktreeFact,
    parent: &'a ProjectConfig,
    projects: &'a [ProjectConfig],
) -> WorktreeRowModel {
    let target_path = normalized_path(&fact.path);
    let configured = std::iter::once(parent)
        .chain(configured_children(projects, &parent.id))
        .find(|project| {
            mt_project::worktree::normalize_path_for_comparison(&project.path) == target_path
        })
        .or_else(|| configured_local_project_for_path(projects, &fact.path));
    let branch = short_branch(fact.branch_ref.as_deref());
    let fallback = if fact.is_main { "main" } else { "worktree" };
    let label = branch
        .clone()
        .or_else(|| configured.map(|project| project.name.clone()))
        .unwrap_or_else(|| path_label(&fact.path, fallback));
    let valid_unregistered =
        !fact.is_bare && fact.prunable.is_none() && fact.path_state != WorktreePathState::Missing;

    WorktreeRowModel {
        path: fact.path.clone(),
        label,
        configured_project_id: configured.map(|project| project.id.clone()),
        is_main: fact.is_main,
        is_remote: false,
        is_sparse: fact.is_sparse,
        selectable: configured.is_some() || valid_unregistered,
    }
}

/// Merge one local project's configured compatibility rows with its catalog
/// snapshot. Only an authoritative snapshot may omit a configured child;
/// degraded and absent snapshots preserve every configured fallback row.
fn merge_local_worktrees(
    parent: &ProjectConfig,
    projects: &[ProjectConfig],
    scan: Option<&WorktreeScan>,
) -> Vec<WorktreeRowModel> {
    let mut rows = Vec::new();
    let mut seen_paths = HashSet::new();

    if let Some(scan) = scan {
        for fact in scan
            .worktrees
            .iter()
            .filter(|fact| fact.is_main)
            .chain(scan.worktrees.iter().filter(|fact| !fact.is_main))
        {
            let key = normalized_path(&fact.path);
            if seen_paths.insert(key) {
                rows.push(catalog_worktree_row(fact, parent, projects));
            }
        }
    }

    let parent_key = mt_project::worktree::normalize_path_for_comparison(&parent.path);
    if seen_paths.insert(parent_key) {
        rows.push(configured_worktree_row(parent, true));
    }

    if scan.is_none_or(|scan| !scan.authoritative) {
        for child in configured_children(projects, &parent.id) {
            let key = mt_project::worktree::normalize_path_for_comparison(&child.path);
            if seen_paths.insert(key) {
                rows.push(configured_worktree_row(child, false));
            }
        }
    }

    // A degraded scan can omit the main fact. Keep the configured parent at
    // the front without disturbing catalog or configured child order.
    rows.sort_by_key(|row| !row.is_main);
    rows
}

fn build_project_rows(
    projects: &[ProjectConfig],
    tree: Option<&[ProjectTreeItem]>,
    snapshots: &HashMap<String, CatalogSnapshot>,
) -> Vec<ProjectRowModel> {
    let by_id: HashMap<&str, &ProjectConfig> = projects
        .iter()
        .map(|project| (project.id.as_str(), project))
        .collect();

    ordered_top_level_project_ids(projects, tree)
        .into_iter()
        .filter_map(|id| {
            let project = *by_id.get(id.as_str())?;
            let is_remote = project.ssh_connection_id.is_some();
            let worktrees = if is_remote {
                vec![configured_worktree_row(project, true)]
            } else {
                let scan = snapshots.get(&project.id).and_then(|snapshot| {
                    (mt_project::worktree::normalize_path_for_comparison(&snapshot.repo_path)
                        == mt_project::worktree::normalize_path_for_comparison(&project.path))
                    .then_some(&snapshot.scan)
                });
                merge_local_worktrees(project, projects, scan)
            };
            Some(ProjectRowModel {
                id: project.id.clone(),
                name: project.name.clone(),
                path: project.path.clone(),
                is_remote,
                worktrees,
            })
        })
        .collect()
}

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

fn scan_targets(projects: &[ProjectConfig], tree: Option<&[ProjectTreeItem]>) -> Vec<ScanTarget> {
    let by_id: HashMap<&str, &ProjectConfig> = projects
        .iter()
        .map(|project| (project.id.as_str(), project))
        .collect();
    ordered_top_level_project_ids(projects, tree)
        .into_iter()
        .filter_map(|id| {
            let project = *by_id.get(id.as_str())?;
            project.ssh_connection_id.is_none().then(|| ScanTarget {
                project_id: project.id.clone(),
                path: project.path.clone(),
            })
        })
        .collect()
}

fn scan_key(targets: &[ScanTarget]) -> String {
    let mut key = String::new();
    for target in targets {
        key.push_str(&target.project_id);
        key.push('\0');
        key.push_str(&mt_project::worktree::normalize_path_for_comparison(
            &target.path,
        ));
        key.push('\0');
        key.push_str(
            &mt_project::worktree::current_generation(Path::new(&target.path)).to_string(),
        );
        key.push('\0');
    }
    key
}

fn prepare_snapshots_for_refresh(
    snapshots: &mut HashMap<String, CatalogSnapshot>,
    current_paths: &HashMap<String, String>,
) {
    snapshots.retain(|project_id, snapshot| {
        current_paths.get(project_id).is_some_and(|path| {
            mt_project::worktree::normalize_path_for_comparison(path)
                == mt_project::worktree::normalize_path_for_comparison(&snapshot.repo_path)
        })
    });
    // A refresh attempt makes the prior inventory last-known presentation data.
    // If the new scan fails, it may still contribute rows but cannot omit configured
    // children as though it were current authoritative proof.
    for snapshot in snapshots.values_mut() {
        snapshot.scan.authoritative = false;
    }
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
    collapsed_projects: HashSet<String>,
    snapshots: HashMap<String, CatalogSnapshot>,
    catalog_request_generation: u64,
    catalog_key: String,
    was_focused: bool,
}

impl EventEmitter<OrcaSidebarEvent> for OrcaProjectSidebar {}

impl OrcaProjectSidebar {
    pub fn new(store: Entity<AppStore>, cx: &mut Context<Self>) -> Self {
        cx.observe(&store, |this: &mut Self, _, cx| {
            let focused = this.store.read(cx).window_focused();
            let regained_focus = focused && !this.was_focused;
            this.was_focused = focused;
            this.refresh_catalog(regained_focus, cx);
            cx.notify();
        })
        .detach();

        let was_focused = store.read(cx).window_focused();
        let mut this = Self {
            store,
            collapsed_projects: HashSet::new(),
            snapshots: HashMap::new(),
            catalog_request_generation: 0,
            catalog_key: String::new(),
            was_focused,
        };
        this.refresh_catalog(true, cx);
        this
    }

    fn refresh_catalog(&mut self, force: bool, cx: &mut Context<Self>) {
        let targets = {
            let store = self.store.read(cx);
            scan_targets(store.projects(), store.config().project_tree.as_deref())
        };
        let next_key = scan_key(&targets);
        if !force && next_key == self.catalog_key {
            return;
        }

        self.catalog_key = next_key;
        self.catalog_request_generation = self.catalog_request_generation.wrapping_add(1);
        let request_generation = self.catalog_request_generation;

        let current_paths: HashMap<String, String> = targets
            .iter()
            .map(|target| (target.project_id.clone(), target.path.clone()))
            .collect();
        prepare_snapshots_for_refresh(&mut self.snapshots, &current_paths);

        if targets.is_empty() {
            self.snapshots.clear();
            cx.notify();
            return;
        }

        cx.spawn(async move |this, cx| {
            let targets_for_task = targets.clone();
            let completions = cx
                .background_executor()
                .spawn(async move {
                    targets_for_task
                        .into_iter()
                        .map(|target| {
                            let result = mt_project::worktree::scan(Path::new(&target.path))
                                .map_err(|error| format!("{error:#}"));
                            ScanCompletion { target, result }
                        })
                        .collect::<Vec<_>>()
                })
                .await;

            let _ = this.update(cx, |this: &mut OrcaProjectSidebar, cx| {
                if this.catalog_request_generation != request_generation {
                    return;
                }

                let current: HashMap<String, String> = {
                    let store = this.store.read(cx);
                    scan_targets(store.projects(), store.config().project_tree.as_deref())
                        .into_iter()
                        .map(|target| (target.project_id, target.path))
                        .collect()
                };
                let mut catalog_changed_during_request = false;

                for completion in completions {
                    let Some(current_path) = current.get(&completion.target.project_id) else {
                        continue;
                    };
                    if mt_project::worktree::normalize_path_for_comparison(current_path)
                        != mt_project::worktree::normalize_path_for_comparison(
                            &completion.target.path,
                        )
                    {
                        continue;
                    }
                    let Ok(scan) = completion.result else {
                        // Preserve the prior snapshot and configured fallback rows.
                        continue;
                    };
                    if mt_project::worktree::current_generation(Path::new(current_path))
                        != scan.generation
                    {
                        catalog_changed_during_request = true;
                        continue;
                    }
                    this.snapshots.insert(
                        completion.target.project_id,
                        CatalogSnapshot {
                            repo_path: completion.target.path,
                            scan,
                        },
                    );
                }

                cx.notify();
                if catalog_changed_during_request {
                    this.refresh_catalog(true, cx);
                }
            });
        })
        .detach();
    }

    fn activate_worktree(
        &mut self,
        parent_id: &str,
        path: &Path,
        configured_project_id: Option<&str>,
        is_remote: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let parent_id = parent_id.to_string();
        let path = path.to_path_buf();
        let configured_project_id = configured_project_id.map(str::to_string);
        let activated = self.store.update(cx, |store, cx| {
            if is_remote {
                let target = configured_project_id
                    .as_deref()
                    .filter(|id| store.project(id).is_some())
                    .map(str::to_string)
                    .or_else(|| store.project(&parent_id).map(|_| parent_id.clone()));
                let target = target?;
                store.set_active_project(&target, cx);
                let worktree_id = store.active_worktree_id()?.clone();
                return (store.worktree_id_for_project(&target) == Some(&worktree_id))
                    .then_some((target, worktree_id));
            }

            // Resolve again inside the store update. A previous click or a
            // concurrent catalog completion may already have materialized it.
            let existing = configured_local_project_for_activation(
                store.projects(),
                configured_project_id.as_deref(),
                &path,
            )
            .map(|project| project.id.clone());
            let id = existing
                .unwrap_or_else(|| store.add_project_at(&path, Some(parent_id.as_str()), cx));
            store.set_active_project(&id, cx);
            let worktree_id = store.active_worktree_id()?.clone();
            (store.worktree_id_for_project(&id) == Some(&worktree_id)).then_some((id, worktree_id))
        });
        if let Some((project_id, worktree_id)) = activated {
            crate::workbench_area::reactivate_active_page(&project_id, &worktree_id, window, cx);
        }
    }

    fn render_top_actions(&self, cx: &mut Context<Self>) -> gpui::Div {
        let store_for_search = self.store.clone();
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
                .on_click(move |_event, window, cx| {
                    crate::search_modal::open(store_for_search.clone(), window, cx);
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
        let store_for_options = self.store.clone();
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
                                let remote_store = store_for_options.clone();
                                menu::show(
                                    event.position(),
                                    vec![
                                        menu::item(
                                            t("projectList", "menu.addRemoteProject"),
                                            move |window, cx| {
                                                crate::remote_project::open(
                                                    remote_store.clone(),
                                                    None,
                                                    window,
                                                    cx,
                                                );
                                            },
                                        ),
                                        menu::separator(),
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
                            crate::modal::open_add_project(store_for_add.clone(), window, cx);
                        }),
                    ),
            )
    }

    fn render_project_row(
        &self,
        project: &ProjectRowModel,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let project_id = project.id.clone();
        let toggle_project_id = project.id.clone();
        let project_path = project.path.clone();
        let project_name = project.name.clone();
        let is_remote = project.is_remote;
        let sidebar = cx.entity();

        let leading_icon: AnyElement = if is_remote {
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
            .id(SharedString::from(format!("orca-project-{}", project.id)))
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
                let path = project.path.clone();
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
            .child(div().flex_1().truncate().child(project.name.clone()))
            .when(!is_remote, |row| {
                row.child(
                    small_icon_button(
                        SharedString::from(format!("orca-add-worktree-{}", project.id)),
                        t("worktree", "createTitle").into(),
                        VectorIcon::new(PLUS_ICON, px(12.0))
                            .ink(ui::text_muted())
                            .into_any_element(),
                    )
                    .on_click(move |_event, window, cx| {
                        cx.stop_propagation();
                        let sidebar = sidebar.clone();
                        crate::git_worktree::open(
                            project_path.clone(),
                            true,
                            Some(project_id.clone()),
                            move |cx| {
                                sidebar.update(cx, |sidebar, cx| {
                                    sidebar.refresh_catalog(true, cx);
                                });
                            },
                            window,
                            cx,
                        );
                    }),
                )
            })
            .when(is_remote, |row| {
                row.child(
                    div()
                        .max_w(px(82.0))
                        .truncate()
                        .text_size(ui::font_px(9.5))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(ui::text_muted())
                        .child(project_name),
                )
            })
    }

    fn render_worktree_row(
        &self,
        parent_id: &str,
        worktree: &WorktreeRowModel,
        active_project_id: Option<&str>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let (status, needs_attention) = {
            let store = self.store.read(cx);
            worktree
                .configured_project_id
                .as_deref()
                .and_then(|id| store.project_state(id))
                .map(|state| (state.status, state.needs_attention))
                .unwrap_or((PaneStatus::Idle, false))
        };
        let is_active = worktree.configured_project_id.as_deref() == active_project_id;
        let path_text = worktree.path.to_string_lossy().to_string();
        let detail = if worktree.is_main {
            format!("{} | {}", t("worktree", "mainRepo"), path_text)
        } else if worktree.is_sparse {
            format!("sparse | {path_text}")
        } else {
            path_text
        };
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
            });

        let parent_id = parent_id.to_string();
        let path = worktree.path.clone();
        let configured_project_id = worktree.configured_project_id.clone();
        let is_remote = worktree.is_remote;
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
                mt_project::worktree::normalize_path_for_comparison(
                    &worktree.path.to_string_lossy()
                )
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
                let path = worktree.path.to_string_lossy().to_string();
                move |window, cx| Tooltip::new(path.clone()).build(window, cx)
            })
            .when(selectable, |row| {
                row.on_click(cx.listener(move |this, _event, window, cx| {
                    this.activate_worktree(
                        &parent_id,
                        &path,
                        configured_project_id.as_deref(),
                        is_remote,
                        window,
                        cx,
                    );
                }))
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
                build_project_rows(
                    store.projects(),
                    store.config().project_tree.as_deref(),
                    &self.snapshots,
                ),
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
            let expanded = !self.collapsed_projects.contains(&project.id);
            rows = rows.child(self.render_project_row(&project, expanded, cx));
            if expanded {
                for worktree in &project.worktrees {
                    rows = rows.child(self.render_worktree_row(
                        &project.id,
                        worktree,
                        active_project_id.as_deref(),
                        cx,
                    ));
                    if let Some(project_id) = worktree.configured_project_id.as_deref()
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

#[cfg(test)]
mod tests {
    use super::*;
    use mt_ai::{AgentEvidence, AgentProvider, AgentRoute};
    use mt_config::ProjectGroup;
    use mt_identity::{
        AgentEventId, AgentRunId, ExecutionHostId, HostInstallId, PaneKey, TabId,
        TerminalIncarnationId, TerminalSessionId, WorktreeId,
    };
    use mt_project::worktree::WorktreeScanSource;

    fn project(id: &str, path: &str, parent: Option<&str>) -> ProjectConfig {
        ProjectConfig {
            id: id.to_string(),
            name: id.to_string(),
            path: path.to_string(),
            description: None,
            saved_layout: None,
            expanded_dirs: Vec::new(),
            ssh_mcp_enabled: false,
            ssh_cli_token: None,
            ssh_connection_ids: None,
            env_vars: Vec::new(),
            wsl_sessions_distro: None,
            ssh_connection_id: None,
            parent_project_id: parent.map(str::to_string),
            kind_override: None,
        }
    }

    fn remote_project(id: &str, path: &str) -> ProjectConfig {
        let mut project = project(id, path, None);
        project.ssh_connection_id = Some("ssh-1".to_string());
        project
    }

    fn fact(path: &str, branch: &str, is_main: bool) -> WorktreeFact {
        WorktreeFact {
            path: PathBuf::from(path),
            head: Some("0123456789abcdef".to_string()),
            branch_ref: Some(format!("refs/heads/{branch}")),
            is_main,
            is_detached: false,
            is_bare: false,
            is_sparse: false,
            locked: None,
            prunable: None,
            path_state: WorktreePathState::Present,
        }
    }

    fn scan(authoritative: bool, worktrees: Vec<WorktreeFact>) -> WorktreeScan {
        WorktreeScan {
            generation: 1,
            source: if authoritative {
                WorktreeScanSource::PorcelainZ
            } else {
                WorktreeScanSource::LastKnown
            },
            authoritative,
            worktrees,
            warning: (!authoritative).then(|| "degraded".to_string()),
        }
    }

    fn snapshot(repo_path: &str, scan: WorktreeScan) -> CatalogSnapshot {
        CatalogSnapshot {
            repo_path: repo_path.to_string(),
            scan,
        }
    }

    fn agent_target(
        run_id: AgentRunId,
        project_id: &str,
        execution_host_id: &ExecutionHostId,
        worktree_id: &WorktreeId,
    ) -> AgentTargetView {
        AgentTargetView {
            run_id,
            last_event_id: AgentEventId::new(),
            project_id: project_id.to_string(),
            project_name: project_id.to_string(),
            root_project_name: project_id.to_string(),
            worktree_name: "shared".into(),
            host_label: "Local machine".into(),
            pane_id: format!("{project_id}-pane"),
            pane_label: format!("{project_id} terminal"),
            route: AgentRoute {
                execution_host_id: execution_host_id.clone(),
                worktree_id: worktree_id.clone(),
                tab_id: TabId::new(),
                pane_key: PaneKey::new(),
                terminal_session_id: TerminalSessionId::new(),
                terminal_incarnation_id: TerminalIncarnationId::new(),
            },
            provider: "codex".parse::<AgentProvider>().unwrap(),
            provider_session_id: Some(format!("{project_id}-session")),
            activity: AgentActivity::Working,
            connectivity: AgentConnectivity::Live,
            evidence: AgentEvidence::Hook,
            received_at_unix_ms: 1,
            attention: false,
            unread: false,
        }
    }

    #[test]
    fn top_level_order_flattens_groups_and_ignores_visual_collapse() {
        let projects = vec![
            project("a", "/a", None),
            project("c", "/c", None),
            project("b", "/b", None),
            project("a-child", "/a/child", Some("a")),
            project("orphan", "/orphan", Some("missing")),
        ];
        let tree = vec![
            ProjectTreeItem::Group(ProjectGroup {
                id: "group".to_string(),
                name: "Group".to_string(),
                collapsed: true,
                children: vec![ProjectTreeItem::ProjectId("b".to_string())],
            }),
            ProjectTreeItem::ProjectId("a".to_string()),
        ];

        assert_eq!(
            ordered_top_level_project_ids(&projects, Some(&tree)),
            vec!["b", "a", "c", "orphan"]
        );
    }

    #[test]
    fn configured_children_map_to_catalog_paths_after_normalization() {
        #[cfg(windows)]
        let (configured, catalog) = (r"C:\Repo\Feature\", r"c:/repo/feature");
        #[cfg(not(windows))]
        let (configured, catalog) = ("/repo/feature/", "/repo/feature");

        let projects = vec![
            project("parent", "/repo", None),
            project("child", configured, Some("parent")),
        ];
        assert_eq!(
            configured_local_project_for_path(&projects, Path::new(catalog))
                .map(|project| project.id.as_str()),
            Some("child")
        );
    }

    #[test]
    fn worktree_activation_prefers_the_rows_validated_same_path_alias() {
        let projects = vec![
            project("alias-a", "/repo", None),
            project("alias-b", "/repo", None),
        ];

        assert_eq!(
            configured_local_project_for_activation(
                &projects,
                Some("alias-b"),
                Path::new("/repo"),
            )
            .map(|project| project.id.as_str()),
            Some("alias-b")
        );
        assert_eq!(
            configured_local_project_for_activation(
                &projects,
                Some("alias-b"),
                Path::new("/other"),
            )
            .map(|project| project.id.as_str()),
            None,
            "a stale row owner must not route after its configured path changes"
        );
    }

    #[test]
    fn authoritative_catalog_omits_absent_configured_children() {
        let projects = vec![
            project("parent", "/repo", None),
            project("feature", "/repo-feature", Some("parent")),
            project("stale", "/repo-stale", Some("parent")),
        ];
        let mut snapshots = HashMap::new();
        snapshots.insert(
            "parent".to_string(),
            snapshot(
                "/repo",
                scan(
                    true,
                    vec![
                        fact("/repo", "main", true),
                        fact("/repo-feature", "feature", false),
                    ],
                ),
            ),
        );

        let rows = build_project_rows(&projects, None, &snapshots);
        let ids: Vec<Option<&str>> = rows[0]
            .worktrees
            .iter()
            .map(|row| row.configured_project_id.as_deref())
            .collect();
        assert_eq!(ids, vec![Some("parent"), Some("feature")]);
        assert!(!ids.contains(&Some("stale")));
    }

    #[test]
    fn authoritative_main_rows_prefer_their_current_same_path_parent() {
        let projects = vec![
            project("alias-a", "/repo", None),
            project("alias-b", "/repo", None),
        ];
        let snapshots = HashMap::from([
            (
                "alias-a".to_string(),
                snapshot("/repo", scan(true, vec![fact("/repo", "main", true)])),
            ),
            (
                "alias-b".to_string(),
                snapshot("/repo", scan(true, vec![fact("/repo", "main", true)])),
            ),
        ]);

        let rows = build_project_rows(&projects, None, &snapshots);

        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows.iter()
                .map(|row| row.worktrees[0].configured_project_id.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("alias-a"), Some("alias-b")]
        );
    }

    #[test]
    fn authoritative_linked_rows_prefer_their_current_same_path_parent() {
        let projects = vec![
            project("alias-a", "/repo/linked", None),
            project("alias-b", "/repo/linked", None),
        ];
        let catalog = scan(
            true,
            vec![
                fact("/repo/main", "main", true),
                fact("/repo/linked", "feature", false),
            ],
        );
        let snapshots = HashMap::from([
            (
                "alias-a".to_string(),
                snapshot("/repo/linked", catalog.clone()),
            ),
            ("alias-b".to_string(), snapshot("/repo/linked", catalog)),
        ]);

        let rows = build_project_rows(&projects, None, &snapshots);

        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows.iter()
                .map(|row| {
                    row.worktrees
                        .iter()
                        .find(|worktree| worktree.path == Path::new("/repo/linked"))
                        .and_then(|worktree| worktree.configured_project_id.as_deref())
                })
                .collect::<Vec<_>>(),
            vec![Some("alias-a"), Some("alias-b")]
        );
    }

    #[test]
    fn catalog_rows_prefer_children_of_the_current_project_over_global_aliases() {
        let projects = vec![
            project("other-alias", "/repo/linked", None),
            project("parent", "/repo/main", None),
            project("child", "/repo/linked", Some("parent")),
        ];
        let snapshots = HashMap::from([(
            "parent".to_string(),
            snapshot(
                "/repo/main",
                scan(
                    true,
                    vec![
                        fact("/repo/main", "main", true),
                        fact("/repo/linked", "feature", false),
                    ],
                ),
            ),
        )]);

        let rows = build_project_rows(&projects, None, &snapshots);
        let parent = rows.iter().find(|row| row.id == "parent").unwrap();
        let linked = parent
            .worktrees
            .iter()
            .find(|worktree| worktree.path == Path::new("/repo/linked"))
            .unwrap();
        assert_eq!(linked.configured_project_id.as_deref(), Some("child"));
    }

    #[test]
    fn degraded_catalog_preserves_configured_fallback_rows() {
        let projects = vec![
            project("parent", "/repo", None),
            project("feature", "/repo-feature", Some("parent")),
            project("other", "/repo-other", Some("parent")),
        ];
        let mut snapshots = HashMap::new();
        snapshots.insert(
            "parent".to_string(),
            snapshot(
                "/repo",
                scan(false, vec![fact("/repo-feature", "feature", false)]),
            ),
        );

        let rows = build_project_rows(&projects, None, &snapshots);
        let ids: Vec<Option<&str>> = rows[0]
            .worktrees
            .iter()
            .map(|row| row.configured_project_id.as_deref())
            .collect();
        assert_eq!(ids, vec![Some("parent"), Some("feature"), Some("other")]);
    }

    #[test]
    fn refresh_failure_cannot_leave_old_snapshot_authoritative() {
        let projects = vec![
            project("parent", "/repo", None),
            project("feature", "/repo-feature", Some("parent")),
        ];
        let mut snapshots = HashMap::from([(
            "parent".to_string(),
            snapshot("/repo", scan(true, vec![fact("/repo", "main", true)])),
        )]);
        let current_paths = HashMap::from([("parent".to_string(), "/repo".to_string())]);

        prepare_snapshots_for_refresh(&mut snapshots, &current_paths);

        assert!(!snapshots["parent"].scan.authoritative);
        let rows = build_project_rows(&projects, None, &snapshots);
        assert_eq!(
            rows[0]
                .worktrees
                .iter()
                .map(|row| row.configured_project_id.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("parent"), Some("feature")]
        );
    }

    #[test]
    fn remote_project_has_exactly_one_configured_worktree_row() {
        let projects = vec![remote_project("remote", "/srv/repo")];
        let rows = build_project_rows(&projects, None, &HashMap::new());

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].worktrees.len(), 1);
        assert!(rows[0].worktrees[0].is_remote);
        assert_eq!(
            rows[0].worktrees[0].configured_project_id.as_deref(),
            Some("remote")
        );
    }

    #[test]
    fn shared_worktree_agents_use_exact_project_and_unique_run_rows() {
        let execution_host_id = ExecutionHostId::derive("local", &HostInstallId::new());
        let worktree_id: WorktreeId = format!("worktree-v1:{}", "a".repeat(64)).parse().unwrap();
        let run_a = AgentRunId::new();
        let run_b = AgentRunId::new();
        let target_a = agent_target(run_a.clone(), "alias-a", &execution_host_id, &worktree_id);
        let target_b = agent_target(run_b.clone(), "alias-b", &execution_host_id, &worktree_id);

        let grouped = group_agent_targets_by_project(vec![
            target_a.clone(),
            target_a.clone(),
            target_b.clone(),
        ]);

        assert_eq!(grouped["alias-a"], vec![target_a.clone()]);
        assert_eq!(grouped["alias-b"], vec![target_b.clone()]);
        let rows = grouped.values().flatten().collect::<Vec<_>>();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows.iter()
                .map(|target| target.run_id.clone())
                .collect::<HashSet<_>>(),
            HashSet::from([run_a, run_b])
        );
        assert_eq!(
            rows.iter()
                .map(|target| format!("orca-agent-{}", target.run_id))
                .collect::<HashSet<_>>()
                .len(),
            rows.len()
        );
        assert_eq!(grouped["alias-a"][0].route, target_a.route);
        assert_eq!(grouped["alias-b"][0].route, target_b.route);
    }
}
