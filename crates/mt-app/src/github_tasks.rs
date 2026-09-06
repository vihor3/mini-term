//! Read-only GitHub tasks with execution-host, independently selected accounts.
//! Host work and request ownership live in private pipeline/service modules.

use std::collections::HashMap;
use std::ffi::OsStr;

use gpui::{
    AnyElement, App, ClipboardItem, Context, Entity, InteractiveElement, IntoElement,
    ParentElement, Render, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Window,
    div, prelude::FluentBuilder, px,
};
use gpui_component::ActiveTheme as _;
use gpui_component::scroll::Scrollbar;
use gpui_component::text::{TextView, TextViewStyle};
use mt_github::{
    GitHubRepoIdentity, GitHubWorkItemSummary, WorkItemKind, WorkItemState, WorkItemStateFilter,
    auth_login_command,
};
use mt_identity::WorktreeId;
use mt_ui::icons::usage_glyphs::ICON_REFRESH;
use mt_ui::icons::vector::VectorIcon;
use mt_ui::tooltip::Tooltip;

use crate::execution_host::{ExecutionSourceSignature, ProjectExecutionSnapshot};
use crate::store::AppStore;
use crate::ui;

mod model;
mod pipeline;
mod service;
#[cfg(test)]
mod tests;

use model::GitHubListView;
pub use model::{GitHubWorkItemTabKey, OpenGitHubWorkItem};
pub use service::GitHubTaskService;

/// Only the exact value `0` restores the old unavailable placeholder.
pub fn github_project_tasks_enabled() -> bool {
    github_project_tasks_enabled_for(std::env::var_os("MINI_TERM_GITHUB_PROJECT_TASKS").as_deref())
}

fn github_project_tasks_enabled_for(value: Option<&OsStr>) -> bool {
    value.is_none_or(|value| value != "0")
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SelectedWorkItem {
    repository: GitHubRepoIdentity,
    kind: WorkItemKind,
    number: u64,
}

struct PanelScopeState {
    mode: WorkItemKind,
    filter: WorkItemStateFilter,
    selected: Option<SelectedWorkItem>,
    scroll: ScrollHandle,
}

/// Right-side Tasks surface. Network data lives in [`GitHubTaskService`].
pub struct GitHubTasksPanel {
    store: Entity<AppStore>,
    service: Entity<GitHubTaskService>,
    current_project: Option<String>,
    current_worktree: Option<WorktreeId>,
    current_source: Option<ExecutionSourceSignature>,
    mode: WorkItemKind,
    filter: WorkItemStateFilter,
    selected: Option<SelectedWorkItem>,
    scroll: ScrollHandle,
    scope_cache: HashMap<WorktreeId, PanelScopeState>,
    visible: bool,
}

impl GitHubTasksPanel {
    pub fn new(
        store: Entity<AppStore>,
        service: Entity<GitHubTaskService>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&store, |this, _, cx| {
            this.service
                .update(cx, |service, cx| service.reconcile_sources(cx));
            this.sync_scope(cx);
            if this.visible {
                this.ensure_current(false, cx);
            }
            cx.notify();
        })
        .detach();
        cx.observe(&service, |this, _, cx| {
            this.sync_scope(cx);
            if this.visible {
                this.ensure_current(false, cx);
            }
            cx.notify();
        })
        .detach();
        Self {
            store,
            service,
            current_project: None,
            current_worktree: None,
            current_source: None,
            mode: WorkItemKind::Issue,
            filter: WorkItemStateFilter::Open,
            selected: None,
            scroll: ScrollHandle::new(),
            scope_cache: HashMap::new(),
            visible: false,
        }
    }

    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if visible && !self.sync_scope(cx) {
            self.access_current(cx);
        }
        cx.notify();
    }

    fn active_snapshot(&self, cx: &App) -> Option<ProjectExecutionSnapshot> {
        let store = self.store.read(cx);
        let project_id = store.active_project_id.as_deref()?;
        store.project_execution_snapshot(project_id).ok()
    }

    fn save_scope(&mut self) {
        let Some(worktree_id) = self.current_worktree.clone() else {
            return;
        };
        self.scope_cache.insert(
            worktree_id,
            PanelScopeState {
                mode: self.mode,
                filter: self.filter,
                selected: self.selected.take(),
                scroll: std::mem::replace(&mut self.scroll, ScrollHandle::new()),
            },
        );
    }

    fn restore_scope(&mut self, worktree_id: Option<&WorktreeId>) {
        if let Some(state) = worktree_id.and_then(|id| self.scope_cache.remove(id)) {
            self.mode = state.mode;
            self.filter = state.filter;
            self.selected = state.selected;
            self.scroll = state.scroll;
        } else {
            self.mode = WorkItemKind::Issue;
            self.filter = WorkItemStateFilter::Open;
            self.selected = None;
            self.scroll = ScrollHandle::new();
        }
    }

    fn sync_scope(&mut self, cx: &mut Context<Self>) -> bool {
        let snapshot = self.active_snapshot(cx);
        let next_project = snapshot
            .as_ref()
            .map(|snapshot| snapshot.project_id.clone());
        let next_worktree = snapshot
            .as_ref()
            .map(|snapshot| snapshot.worktree_id.clone());
        let next_source = snapshot
            .as_ref()
            .map(ProjectExecutionSnapshot::source_signature);
        if self.current_project == next_project
            && self.current_worktree == next_worktree
            && self.current_source == next_source
        {
            return false;
        }
        self.save_scope();
        self.restore_scope(next_worktree.as_ref());
        if let Some(source) = self.current_source.as_ref() {
            self.service
                .update(cx, |service, _| service.suspend_source(source));
        }
        self.current_project = next_project;
        self.current_worktree = next_worktree;
        self.current_source = next_source;
        if self.visible {
            self.access_current(cx);
        }
        true
    }

    fn access_current(&mut self, cx: &mut Context<Self>) {
        let Some(snapshot) = self.active_snapshot(cx) else {
            return;
        };
        let mode = self.mode;
        self.service.update(cx, |service, cx| {
            service.access_list(snapshot, mode, false, cx)
        });
    }

    fn ensure_current(&mut self, force: bool, cx: &mut Context<Self>) {
        if !github_project_tasks_enabled() {
            return;
        }
        let Some(snapshot) = self.active_snapshot(cx) else {
            return;
        };
        let mode = self.mode;
        self.service.update(cx, |service, cx| {
            service.ensure_list(snapshot, mode, force, cx)
        });
    }

    fn set_mode(&mut self, mode: WorkItemKind, cx: &mut Context<Self>) {
        self.mode = mode;
        self.access_current(cx);
        cx.notify();
    }

    fn set_filter(&mut self, filter: WorkItemStateFilter, cx: &mut Context<Self>) {
        if self.filter == filter {
            return;
        }
        self.filter = filter;
        cx.notify();
    }

    fn open_row(
        &mut self,
        view: &GitHubListView,
        row: GitHubWorkItemSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(snapshot) = self.active_snapshot(cx) else {
            return;
        };
        if !self
            .service
            .read(cx)
            .row_is_current(&snapshot, view, row.kind, cx)
        {
            return;
        }
        if self.current_project.as_deref() != Some(snapshot.project_id.as_str())
            || self.current_worktree.as_ref() != Some(&snapshot.worktree_id)
        {
            return;
        }
        let (Some(project_id), Some(worktree_id), Some(repository), Some(account)) = (
            self.current_project.clone(),
            self.current_worktree.clone(),
            view.repository.clone(),
            view.account.clone(),
        ) else {
            return;
        };
        self.selected = Some(SelectedWorkItem {
            repository: repository.clone(),
            kind: row.kind,
            number: row.number,
        });
        crate::workbench_area::open_github_work_item(
            self.service.clone(),
            OpenGitHubWorkItem {
                project_id,
                worktree_id,
                source: view.source.clone(),
                repository,
                account,
                auth_generation: view.auth_generation,
                summary: row,
            },
            window,
            cx,
        );
        cx.notify();
    }

    fn render_segment(
        &self,
        id: SharedString,
        label: &'static str,
        active: bool,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .h(px(25.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(3.0))
            .text_size(ui::font_px(10.0))
            .cursor_pointer()
            .when(active, |el| {
                el.bg(ui::accent_muted()).text_color(ui::text_primary())
            })
            .when(!active, |el| {
                el.text_color(ui::text_muted())
                    .hover(|el| el.bg(ui::border_subtle()).text_color(ui::text_primary()))
            })
            .child(label)
    }

    fn open_account_menu(&self, view: GitHubListView, window: &mut Window, cx: &mut Context<Self>) {
        use crate::menu::{MenuEntry, MenuItem, MenuOptions};
        let Some(snapshot) = self.active_snapshot(cx) else {
            return;
        };
        if snapshot.source_signature() != view.source {
            return;
        }
        let Some(accounts) = view.accounts.as_ref() else {
            return;
        };
        let mut entries = Vec::new();
        for account in accounts.accounts() {
            let identity = account.identity().clone();
            let selected = view.account.as_deref() == Some(identity.login());
            let status = match (
                selected,
                account.problem().is_some() || (selected && view.error.is_some()),
            ) {
                (true, true) => "Selected / unavailable",
                (true, false) => "Selected",
                (false, true) => "Unavailable",
                (false, false) => "",
            };
            let owner = view.clone();
            let entity = cx.entity().downgrade();
            let item = MenuItem::new(format!("@{}", account.login()))
                .shortcut(status)
                .on_click(move |_window, cx| {
                    let _ = entity.update(cx, |this, cx| {
                        let Some(snapshot) = this.active_snapshot(cx) else {
                            return;
                        };
                        if snapshot.source_signature() != owner.source {
                            return;
                        }
                        let kind = this.mode;
                        this.service.update(cx, |service, cx| {
                            service.select_account(snapshot, &owner, identity.clone(), kind, cx);
                        });
                    });
                });
            entries.push(MenuEntry::Item(item));
        }
        if entries.is_empty() {
            entries.push(MenuEntry::Item(
                MenuItem::new("No configured accounts").disabled(true),
            ));
        }
        crate::menu::show_with(
            window.mouse_position(),
            entries,
            MenuOptions::max_height(px(300.0)),
            window,
            cx,
        );
    }

    fn render_toolbar(&self, view: Option<&GitHubListView>, cx: &mut Context<Self>) -> AnyElement {
        let loading = view.is_some_and(|view| view.loading);
        let mut modes = div()
            .flex()
            .items_center()
            .p(px(2.0))
            .rounded(px(4.0))
            .bg(ui::bg_elevated());
        for kind in WorkItemKind::ALL {
            modes = modes.child(
                self.render_segment(
                    SharedString::from(format!("github-mode-{:?}", kind)),
                    kind.short_label(),
                    self.mode == kind,
                )
                .on_click(cx.listener(move |this, _, _window, cx| this.set_mode(kind, cx))),
            );
        }
        let mut filters = div()
            .flex()
            .items_center()
            .p(px(2.0))
            .rounded(px(4.0))
            .bg(ui::bg_elevated());
        for filter in WorkItemStateFilter::ALL {
            filters = filters.child(
                self.render_segment(
                    SharedString::from(format!("github-filter-{:?}", filter)),
                    filter.label(),
                    self.filter == filter,
                )
                .on_click(cx.listener(move |this, _, _window, cx| this.set_filter(filter, cx))),
            );
        }
        let refresh_owner = self.current_source.clone();
        let controls = div()
            .min_h(px(38.0))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.0))
            .child(modes)
            .child(filters)
            .child(div().flex_1())
            .child(
                div()
                    .id("github-tasks-refresh")
                    .w(px(26.0))
                    .h(px(26.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .text_color(ui::text_muted())
                    .when(loading, |el| el.opacity(0.65))
                    .cursor_pointer()
                    .hover(|el| el.bg(ui::border_subtle()))
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        if this.current_source == refresh_owner {
                            this.ensure_current(true, cx);
                        }
                    }))
                    .tooltip(|window, cx| {
                        Tooltip::new("Refresh GitHub tasks")
                            .instant()
                            .build(window, cx)
                    })
                    .child(VectorIcon::new(ICON_REFRESH, px(14.0)).ink(ui::text_muted())),
            );
        let mut toolbar = div()
            .flex_none()
            .w_full()
            .px(px(8.0))
            .pb(px(6.0))
            .flex()
            .flex_col()
            .border_b_1()
            .border_color(ui::border_subtle())
            .child(controls);
        if let Some(view) = view {
            let label = view
                .account
                .as_ref()
                .map(|login| format!("@{login}"))
                .unwrap_or_else(|| {
                    if view.loading {
                        "Loading accounts..."
                    } else {
                        "Select account"
                    }
                    .into()
                });
            let repository = view
                .repository
                .as_ref()
                .map(GitHubRepoIdentity::cli_spec)
                .unwrap_or_default();
            let context = bounded_context(&view.host_label);
            let tooltip = context.clone();
            let repo_tooltip = repository.clone();
            let account_tooltip = format!("Tasks account: {label}");
            let menu_view = view.clone();
            toolbar = toolbar.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .id("github-account-context")
                            .w_full()
                            .truncate()
                            .text_size(ui::font_px(9.0))
                            .text_color(ui::text_muted())
                            .tooltip(move |window, cx| {
                                Tooltip::new(tooltip.clone()).instant().build(window, cx)
                            })
                            .child(context),
                    )
                    .child(
                        div()
                            .id("github-account-repository")
                            .w_full()
                            .truncate()
                            .text_size(ui::font_px(9.0))
                            .text_color(ui::text_secondary())
                            .tooltip(move |window, cx| {
                                Tooltip::new(repo_tooltip.clone())
                                    .instant()
                                    .build(window, cx)
                            })
                            .child(repository),
                    )
                    .child(
                        div()
                            .id("github-tasks-account")
                            .w_full()
                            .h(px(27.0))
                            .px(px(7.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .rounded(px(4.0))
                            .border_1()
                            .border_color(ui::border_default())
                            .bg(ui::bg_elevated())
                            .text_size(ui::font_px(10.0))
                            .text_color(ui::text_primary())
                            .tooltip(move |window, cx| {
                                Tooltip::new(account_tooltip.clone())
                                    .instant()
                                    .build(window, cx)
                            })
                            .child(div().min_w(px(0.0)).flex_1().truncate().child(label))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(ui::text_muted())
                                    .child("\u{25be}"),
                            )
                            .when(view.accounts.is_some(), |el| {
                                el.cursor_pointer()
                                    .hover(|el| el.border_color(ui::accent()))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_account_menu(menu_view.clone(), window, cx);
                                    }))
                            })
                            .when(view.accounts.is_none(), |el| el.opacity(0.65)),
                    ),
            );
        }
        toolbar.into_any_element()
    }

    fn render_status(
        &self,
        title: impl Into<String>,
        body: impl Into<String>,
        retry: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let retry_owner = self.current_source.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .px(px(18.0))
            .text_center()
            .child(
                div()
                    .text_size(ui::font_px(12.0))
                    .text_color(ui::text_primary())
                    .child(title.into()),
            )
            .child(
                div()
                    .text_size(ui::font_px(10.0))
                    .text_color(ui::text_muted())
                    .child(body.into()),
            )
            .when(retry, |el| {
                el.child(
                    ui::ghost_button("github-tasks-retry", "Retry").on_click(cx.listener(
                        move |this, _, _window, cx| {
                            if this.current_source == retry_owner {
                                this.ensure_current(true, cx);
                            }
                        },
                    )),
                )
            })
            .into_any_element()
    }

    fn render_auth_required(&self, view: &GitHubListView, cx: &mut Context<Self>) -> AnyElement {
        let host = view
            .repository
            .as_ref()
            .map(GitHubRepoIdentity::host)
            .unwrap_or("github.com");
        let command = auth_login_command(host);
        let command_tooltip = command.clone();
        let retry_owner = view.source.clone();
        let title = view
            .error
            .as_ref()
            .map_or("GitHub CLI sign-in required", |error| error.title());
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(9.0))
            .px(px(18.0))
            .text_center()
            .child(
                div()
                    .text_size(ui::font_px(12.0))
                    .text_color(ui::text_primary())
                    .child(title),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.0))
                    .text_size(ui::font_px(10.0))
                    .text_color(ui::text_muted())
                    .child(
                        view.error
                            .as_ref()
                            .map(|error| error.summary())
                            .unwrap_or_default(),
                    )
                    .child(format!("Host: {}", bounded_context(&view.host_label))),
            )
            .child(
                div()
                    .id("github-auth-command")
                    .w_full()
                    .truncate()
                    .px(px(9.0))
                    .py(px(7.0))
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(ui::border_default())
                    .bg(ui::bg_elevated())
                    .text_size(ui::font_px(10.0))
                    .text_color(ui::text_secondary())
                    .tooltip(move |window, cx| {
                        Tooltip::new(command_tooltip.clone())
                            .instant()
                            .build(window, cx)
                    })
                    .child(command.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(ui::ghost_button("github-auth-copy", "Copy").on_click(
                        move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(command.clone()));
                        },
                    ))
                    .child(
                        ui::ghost_button("github-auth-retry", "Retry").on_click(cx.listener(
                            move |this, _, _window, cx| {
                                if this.current_source.as_ref() == Some(&retry_owner) {
                                    this.ensure_current(true, cx);
                                }
                            },
                        )),
                    ),
            )
            .into_any_element()
    }

    fn render_error(&self, view: &GitHubListView, cx: &mut Context<Self>) -> AnyElement {
        let Some(error) = view.error.as_ref() else {
            return self.render_status("GitHub Tasks", "No data is available", true, cx);
        };
        if error.offers_login() {
            return self.render_auth_required(view, cx);
        }
        self.render_status(error.title(), error.summary(), true, cx)
    }

    fn render_row(
        &self,
        view: GitHubListView,
        row: GitHubWorkItemSummary,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.selected.as_ref().is_some_and(|selected| {
            view.repository.as_ref() == Some(&selected.repository)
                && selected.kind == row.kind
                && selected.number == row.number
        });
        let state_color = match row.state {
            WorkItemState::Open => ui::color_success(),
            WorkItemState::Merged => ui::color_info(),
            WorkItemState::Closed => ui::text_muted(),
        };
        let author = row.author.clone().unwrap_or_else(|| "unknown".into());
        let time = chrono::DateTime::parse_from_rfc3339(&row.updated_at)
            .ok()
            .map(|timestamp| {
                crate::git_history::format_relative_time(
                    timestamp.timestamp(),
                    chrono::Utc::now().timestamp(),
                )
            })
            .unwrap_or_default();
        let row_for_click = row.clone();
        let interactive = view.interactive;
        div()
            .id(SharedString::from(format!(
                "github-row-{:?}-{}",
                row.kind, row.number
            )))
            .w_full()
            .px(px(9.0))
            .py(px(8.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .border_b_1()
            .border_color(ui::border_subtle())
            .when(selected, |el| el.bg(ui::accent_subtle()))
            .when(!interactive, |el| el.opacity(0.72))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(6.0))
                    .child(
                        div()
                            .flex_none()
                            .text_size(ui::font_px(10.0))
                            .text_color(state_color)
                            .child(format!("#{}", row.number)),
                    )
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_1()
                            .text_size(ui::font_px(11.0))
                            .text_color(ui::text_primary())
                            .child(row.title.clone()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .text_size(ui::font_px(9.0))
                    .text_color(ui::text_muted())
                    .child(row.state.label())
                    .child(format!("@{author}"))
                    .when(!time.is_empty(), |el| el.child(time))
                    .when(row.is_draft, |el| el.child("Draft"))
                    .when(!row.labels.is_empty(), |el| {
                        el.child(
                            row.labels
                                .iter()
                                .take(2)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", "),
                        )
                    }),
            )
            .when(interactive, |el| {
                el.cursor_pointer()
                    .when(!selected, |el| el.hover(|el| el.bg(ui::border_subtle())))
                    .on_click(cx.listener(move |this, _event, window, cx| {
                        this.open_row(&view, row_for_click.clone(), window, cx)
                    }))
            })
            .into_any_element()
    }
}

impl Render for GitHubTasksPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.active_snapshot(cx);
        let view = snapshot
            .as_ref()
            .map(|snapshot| self.service.read(cx).list_view(snapshot, self.mode, cx));
        let toolbar = self.render_toolbar(view.as_ref(), cx);
        let body = match view {
            None => self.render_status("No active project", "Select a project worktree", false, cx),
            Some(view) if view.error.is_some() && view.rows.is_empty() => {
                self.render_error(&view, cx)
            }
            Some(view) if view.loading && view.rows.is_empty() => {
                self.render_status("Loading GitHub Tasks", view.host_label.clone(), false, cx)
            }
            Some(view) => {
                let filtered = view
                    .rows
                    .iter()
                    .filter(|row| self.filter.matches(row))
                    .cloned()
                    .collect::<Vec<_>>();
                if filtered.is_empty() {
                    self.render_status(
                        format!("No {}", self.mode.label()),
                        format!("No {} items match this filter", self.filter.label()),
                        false,
                        cx,
                    )
                } else {
                    let mut list = div().id("github-tasks-list").w_full().flex().flex_col();
                    if let Some(error) = view.error.as_ref() {
                        let stale = view.updated_at_unix_ms.map(|timestamp| {
                            format!("Last updated {}", format_refresh_time(timestamp))
                        });
                        list = list.child(
                            div()
                                .px(px(9.0))
                                .py(px(6.0))
                                .bg(ui::with_alpha(ui::color_warning(), 0.10))
                                .text_size(ui::font_px(9.0))
                                .text_color(ui::color_warning())
                                .child(error.summary())
                                .when_some(stale, |el, stale| el.child(stale)),
                        );
                    }
                    for row in filtered {
                        list = list.child(self.render_row(view.clone(), row, cx));
                    }
                    div()
                        .size_full()
                        .relative()
                        .overflow_hidden()
                        .child(list.track_scroll(&self.scroll).overflow_y_scroll())
                        .child(Scrollbar::vertical(&self.scroll).id("github-tasks-scrollbar"))
                        .into_any_element()
                }
            }
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(ui::bg_surface())
            .child(toolbar)
            .child(div().flex_1().min_h(px(0.0)).overflow_hidden().child(body))
    }
}

fn bounded_context(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_control())
        .take(512)
        .collect()
}

fn format_refresh_time(timestamp_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp_ms)
        .map(|timestamp| {
            timestamp
                .with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "earlier".into())
}

fn detail_text_style(cx: &mut App) -> TextViewStyle {
    let mut code_block = gpui::StyleRefinement::default();
    {
        let text = code_block.text.get_or_insert_default();
        text.font_size = Some(ui::font_px(11.0).into());
        text.line_height = Some(gpui::relative(1.5));
    }
    TextViewStyle {
        highlight_theme: cx.theme().highlight_theme.clone(),
        is_dark: cx.theme().mode.is_dark(),
        heading_base_font_size: ui::font_px(13.0),
        paragraph_gap: gpui::rems(0.65),
        code_block,
        ..Default::default()
    }
}

/// Read-only, internal work-item detail surface. It exposes no URL action.
pub struct GitHubWorkItemViewer {
    service: Entity<GitHubTaskService>,
    request: OpenGitHubWorkItem,
    access: Option<model::RequestOwner>,
    scroll: ScrollHandle,
}

impl GitHubWorkItemViewer {
    pub fn new(
        service: Entity<GitHubTaskService>,
        request: OpenGitHubWorkItem,
        cx: &mut Context<Self>,
    ) -> Self {
        let access = service.update(cx, |service, cx| service.access_detail(request.clone(), cx));
        cx.observe(&service, |_this, _, cx| {
            cx.notify();
        })
        .detach();
        let store = service.read(cx).store.clone();
        cx.observe(&store, |this, _, cx| {
            this.service
                .update(cx, |service, cx| service.reconcile_sources(cx));
            cx.notify();
        })
        .detach();
        Self {
            service,
            request,
            access,
            scroll: ScrollHandle::new(),
        }
    }

    pub(crate) fn on_activated(&mut self, cx: &mut Context<Self>) {
        self.access = self.service.update(cx, |service, cx| {
            service.access_detail(self.request.clone(), cx)
        });
        cx.notify();
    }

    pub fn matches_request(&self, request: &OpenGitHubWorkItem) -> bool {
        self.request.project_id == request.project_id
            && self.request.worktree_id == request.worktree_id
            && self.request.source == request.source
            && self.request.repository == request.repository
            && self.request.account == request.account
            && self.request.auth_generation == request.auth_generation
            && self.request.summary.kind == request.summary.kind
            && self.request.summary.number == request.summary.number
    }
}

impl Render for GitHubWorkItemViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self
            .service
            .read(cx)
            .detail_view(&self.request, self.access.as_ref(), cx);
        let summary = view
            .detail
            .as_ref()
            .map(|detail| &detail.summary)
            .unwrap_or(&self.request.summary);
        let body = if let Some(detail) = view.detail.as_ref() {
            let sanitized = crate::file_viewer::sanitize_github_markdown(&detail.body);
            if sanitized.trim().is_empty() {
                div()
                    .py(px(24.0))
                    .text_size(ui::font_px(11.0))
                    .text_color(ui::text_muted())
                    .child("No description provided")
                    .into_any_element()
            } else {
                TextView::markdown(
                    SharedString::from(format!(
                        "github-detail-{:?}-{}",
                        summary.kind, summary.number
                    )),
                    sanitized,
                    window,
                    cx,
                )
                .style(detail_text_style(cx))
                .selectable(true)
                .into_any_element()
            }
        } else if let Some(error) = view.error.as_ref() {
            div()
                .py(px(24.0))
                .text_size(ui::font_px(11.0))
                .text_color(ui::color_error())
                .child(error.summary())
                .into_any_element()
        } else {
            div()
                .py(px(24.0))
                .text_size(ui::font_px(11.0))
                .text_color(ui::text_muted())
                .child(if view.loading {
                    "Loading work item..."
                } else {
                    "Work item is unavailable"
                })
                .into_any_element()
        };
        let author = summary.author.as_deref().unwrap_or("unknown");
        let content =
            div()
                .id("github-detail-content")
                .w_full()
                .max_w(px(880.0))
                .mx_auto()
                .px(px(28.0))
                .py(px(24.0))
                .flex()
                .flex_col()
                .gap(px(12.0))
                .when(
                    view.detail.is_some() && (view.loading || view.error.is_some()),
                    |el| {
                        el.child(
                            div()
                                .text_size(ui::font_px(10.0))
                                .text_color(ui::color_warning())
                                .child(
                                    view.error
                                        .as_ref()
                                        .map(|error| {
                                            format!("Last known data. {}", error.summary())
                                        })
                                        .unwrap_or_else(|| "Refreshing work item...".into()),
                                ),
                        )
                    },
                )
                .child(
                    div()
                        .text_size(ui::font_px(20.0))
                        .text_color(ui::text_primary())
                        .child(summary.title.clone()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .text_size(ui::font_px(10.0))
                        .text_color(ui::text_muted())
                        .child(format!(
                            "{} #{}",
                            summary.kind.short_label(),
                            summary.number
                        ))
                        .child(summary.state.label())
                        .child(format!("@{author}"))
                        .when(summary.is_draft, |el| el.child("Draft")),
                )
                .when(!summary.labels.is_empty(), |el| {
                    el.child(div().flex().flex_wrap().gap(px(5.0)).children(
                        summary.labels.iter().map(|label| {
                            div()
                                .px(px(6.0))
                                .py(px(2.0))
                                .rounded(px(3.0))
                                .bg(ui::border_subtle())
                                .text_size(ui::font_px(9.0))
                                .text_color(ui::text_secondary())
                                .child(label.clone())
                        }),
                    ))
                })
                .child(
                    div()
                        .pt(px(8.0))
                        .border_t_1()
                        .border_color(ui::border_subtle())
                        .text_size(ui::font_px(12.0))
                        .text_color(ui::text_secondary())
                        .child(body),
                );
        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(ui::bg_document())
            .child(content.track_scroll(&self.scroll).overflow_y_scroll())
            .child(Scrollbar::vertical(&self.scroll).id("github-detail-scrollbar"))
    }
}
