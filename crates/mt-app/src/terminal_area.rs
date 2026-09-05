//! One selected terminal body, with the original entities owned by AppStore.
//! Legacy panels and split trees remain storage/routing facts, never render layers.

use gpui::{
    AnyElement, App, AppContext, Bounds, ClickEvent, Context, Entity, FocusHandle,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, ParentElement,
    Pixels, Render, SharedString, StatefulInteractiveElement, Styled, Window, anchored, canvas,
    deferred, div, point, prelude::FluentBuilder, px,
};
use mt_identity::WorktreeId;
use mt_ui::icon_tooltip::IconTooltips;
use mt_ui::icons::{Geom, Ink, Shape, VectorIcon};
use mt_ui::tooltip::Tooltip;

use crate::branch_family;
use crate::i18n::{t, tr};
use crate::markers;
use crate::menu::{self, MenuEntry, MenuItem, hotkey_label};
use crate::modal;
use crate::overlay;
use crate::pane_actions;
use crate::session_branch::{BranchMenuSegment, branch_menu_segment};
use crate::store::{AppStore, TerminalJumpTarget};
use crate::ui;

const fn cu(v: f32) -> f32 {
    v / 16.0
}
/// 原版这组图标统一 `stroke-width="1.3"`。
const CTRL_STROKE: f32 = 1.3 / 16.0;

/// 终端内查找。原版 `ICON_SEARCH`:`<circle cx="7" cy="7" r="4.2"/>` +
/// `<path d="M10.2 10.2L14 14"/>`。
const ICON_SEARCH: &[Shape] = &[
    Shape::line(
        Ink::Current,
        CTRL_STROKE,
        Geom::Circle {
            c: (cu(7.0), cu(7.0)),
            r: cu(4.2),
        },
    ),
    Shape::line(
        Ink::Current,
        CTRL_STROKE,
        Geom::Polyline(&[(cu(10.2), cu(10.2)), (cu(14.0), cu(14.0))]),
    ),
];

const MARKER_PANEL_WIDTH: f32 = 300.0;
/// 列表最大高度(`MarkerList.tsx:30` 的 `max-h-80` = 20rem)。
const MARKER_PANEL_MAX_HEIGHT: f32 = 320.0;
/// 正文截断字数(`MarkerList.tsx:16` 的 `truncate(s, 40)`)。
const MARKER_LINE_MAX: usize = 40;
/// `#seq` 列宽(`MarkerList.tsx:41` 的 `w-8`)。
const MARKER_SEQ_W: f32 = 32.0;
/// 时间列宽(`MarkerList.tsx:42` 的 `w-10`)。**别再往下调**:`HH:mm` 是五个
/// 字符,收窄到装不下就会折行 —— 两列都用 `min_w` 而不是 `w`,宽度算漏时
/// 宁可把列撑开也别把字折了。
const MARKER_TIME_W: f32 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TabMenuAction {
    Rename,
    ForkSession,
    ViewSessionBranches,
    ForkNeedsIdentity,
    CloseTab,
}

fn tab_menu_actions(can_fork: bool, identity_missing: bool) -> Vec<Option<TabMenuAction>> {
    use TabMenuAction::*;
    let mut actions = vec![Some(Rename)];
    if can_fork {
        actions.extend([None, Some(ForkSession), Some(ViewSessionBranches)]);
    } else if identity_missing {
        actions.extend([None, Some(ForkNeedsIdentity)]);
    }
    actions.extend([None, Some(CloseTab)]);
    actions
}

pub(crate) fn tab_menu(
    store: &Entity<AppStore>,
    target: &TerminalJumpTarget,
    label: &str,
    cx: &App,
) -> Vec<MenuEntry> {
    if store
        .read(cx)
        .resolve_terminal_jump_target(target)
        .is_none()
    {
        return Vec::new();
    }
    let pane_state = store
        .read(cx)
        .project_state(&target.project_id)
        .and_then(|state| state.pane(target.pane_key.as_str()));
    let segment = pane_state
        .map(|pane| branch_menu_segment(pane.ai_session.as_ref(), pane.detected_agent.as_deref()))
        .unwrap_or(BranchMenuSegment::None);
    let project_path = store
        .read(cx)
        .project(&target.project_id)
        .map(|project| project.path.clone())
        .unwrap_or_default();
    let session_id = match &segment {
        BranchMenuSegment::Fork { session_id, .. } => session_id.clone(),
        _ => String::new(),
    };
    tab_menu_actions(
        matches!(segment, BranchMenuSegment::Fork { .. }),
        segment == BranchMenuSegment::NeedsIdentity,
    )
    .into_iter()
    .map(|action| {
        let store = store.clone();
        let target = target.clone();
        match action {
            None => menu::separator(),
            Some(TabMenuAction::Rename) => {
                let label = label.to_string();
                MenuItem::new(t("paneGroup", "rename"))
                    .shortcut(hotkey_label(false, false, false, "F2"))
                    .on_click(move |window, cx| {
                        if store
                            .read(cx)
                            .resolve_terminal_jump_target(&target)
                            .is_none()
                        {
                            return;
                        }
                        modal::open_rename_pane(
                            store.clone(),
                            target.project_id.clone(),
                            target.pane_key.to_string(),
                            label.clone(),
                            window,
                            cx,
                        );
                    })
                    .into()
            }
            Some(TabMenuAction::ForkSession) => {
                menu::item(t("paneGroup", "forkSession"), move |window, cx| {
                    if store
                        .read(cx)
                        .resolve_terminal_jump_target(&target)
                        .is_some()
                    {
                        pane_actions::fork_pane_session(
                            store.clone(),
                            target.project_id.clone(),
                            target.pane_key.to_string(),
                            window,
                            cx,
                        );
                    }
                })
            }
            Some(TabMenuAction::ViewSessionBranches) => branch_family::view_branches_menu_item(
                &store,
                project_path.clone(),
                session_id.clone(),
            ),
            Some(TabMenuAction::ForkNeedsIdentity) => branch_family::needs_identity_menu_item(),
            Some(TabMenuAction::CloseTab) => MenuItem::new(t("paneGroup", "closeTab"))
                .danger()
                .shortcut(hotkey_label(true, true, false, "W"))
                .on_click(move |window, cx| {
                    pane_actions::close_terminal_target(store.clone(), target.clone(), window, cx);
                })
                .into(),
        }
    })
    .collect()
}

fn new_terminal_scope_matches(
    store: &AppStore,
    project_id: &str,
    worktree_id: &WorktreeId,
    anchor: Option<&TerminalJumpTarget>,
) -> bool {
    store.active_project_id.as_deref() == Some(project_id)
        && store.active_worktree_id() == Some(worktree_id)
        && store.worktree_id_for_project(project_id) == Some(worktree_id)
        && anchor.is_none_or(|target| store.resolve_terminal_jump_target(target).is_some())
}

/// Both titlebar and empty-state creation use the same shell/launcher menu.
pub(crate) fn open_new_terminal_menu(
    store: Entity<AppStore>,
    position: gpui::Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let (project_id, worktree_id, anchor, shells, launchers) = {
        let state = store.read(cx);
        let Some(project_id) = state.active_project_id.clone() else {
            return;
        };
        let Some(worktree_id) = state.active_worktree_id().cloned() else {
            return;
        };
        let anchor = state
            .active_pane_id(&project_id)
            .and_then(|pane| state.terminal_jump_target_for_pane(&project_id, &pane));
        let (shells, launchers) = pane_actions::new_terminal_menu_data(state, &project_id);
        (project_id, worktree_id, anchor, shells, launchers)
    };
    if !pane_actions::should_show_new_terminal_menu(shells.len(), launchers.len()) {
        store.update(cx, |store, cx| {
            store.new_terminal(
                &project_id,
                None,
                anchor.map(|target| target.pane_key.to_string()),
                window,
                cx,
            );
        });
        crate::workbench_area::activate_terminal_page(window, cx);
        return;
    }
    let entries = pane_actions::new_terminal_menu_entries(
        shells,
        launchers,
        {
            let store = store.clone();
            let (project_id, worktree_id, anchor) =
                (project_id.clone(), worktree_id.clone(), anchor.clone());
            move |shell, window, cx| {
                if !new_terminal_scope_matches(
                    store.read(cx),
                    &project_id,
                    &worktree_id,
                    anchor.as_ref(),
                ) {
                    return;
                }
                store.update(cx, |store, cx| {
                    store.new_terminal(
                        &project_id,
                        Some(shell),
                        anchor.as_ref().map(|target| target.pane_key.to_string()),
                        window,
                        cx,
                    );
                });
                crate::workbench_area::activate_terminal_page(window, cx);
            }
        },
        move |launcher, window, cx| {
            if !new_terminal_scope_matches(
                store.read(cx),
                &project_id,
                &worktree_id,
                anchor.as_ref(),
            ) {
                return;
            }
            store.update(cx, |store, cx| {
                store.new_terminal_from_launcher(
                    &project_id,
                    &launcher,
                    anchor.as_ref().map(|target| target.pane_key.to_string()),
                    window,
                    cx,
                );
            });
            crate::workbench_area::activate_terminal_page(window, cx);
        },
    );
    menu::show(position, entries, window, cx);
}

fn selected_target_matches(store: &AppStore, target: &TerminalJumpTarget) -> bool {
    store.active_project_id.as_deref() == Some(target.project_id.as_str())
        && store.active_worktree_id() == Some(&target.worktree_id)
        && store.active_pane_id(&target.project_id).as_deref() == Some(target.pane_key.as_str())
        && store.resolve_terminal_jump_target(target).is_some()
}

pub struct TerminalArea {
    store: Entity<AppStore>,
    icon_tooltips: Entity<IconTooltips>,
    marker_open: Option<(TerminalJumpTarget, u32)>,
    marker_focus: FocusHandle,
    marker_prev_focus: Option<FocusHandle>,
    marker_anchor: Option<Bounds<Pixels>>,
    file_drop_target: Option<TerminalJumpTarget>,
}

impl TerminalArea {
    pub fn new(store: Entity<AppStore>, cx: &mut Context<Self>) -> Self {
        cx.observe(&store, |area, _, cx| {
            if area
                .marker_open
                .as_ref()
                .is_some_and(|(target, _)| !selected_target_matches(area.store.read(cx), target))
            {
                area.marker_open = None;
                area.marker_prev_focus = None;
                overlay::pop(overlay::key(overlay::kind::MARKER_LIST));
            }
            cx.notify();
        })
        .detach();
        Self {
            store,
            icon_tooltips: cx.new(|_| IconTooltips::default()),
            marker_open: None,
            marker_focus: cx.focus_handle(),
            marker_prev_focus: None,
            marker_anchor: None,
            file_drop_target: None,
        }
    }

    /// Hiding the workbench body releases only transient UI, never its PTY.
    pub fn suspend(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.marker_open.take().is_some() {
            overlay::pop(overlay::key(overlay::kind::MARKER_LIST));
        }
        self.marker_prev_focus = None;
        self.file_drop_target = None;
        IconTooltips::reset(&self.icon_tooltips, window, cx);
        cx.notify();
    }

    fn toggle_marker_popover(
        &mut self,
        target: &TerminalJumpTarget,
        pty_id: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        IconTooltips::reset(&self.icon_tooltips, window, cx);
        if self.marker_open.is_some() {
            self.close_marker_popover(window, cx);
            return;
        }
        if !selected_target_matches(self.store.read(cx), target)
            || !overlay::push(overlay::key(overlay::kind::MARKER_LIST))
        {
            return;
        }
        self.store
            .update(cx, |store, cx| store.refresh_markers_for_pty(pty_id, cx));
        self.marker_open = Some((target.clone(), pty_id));
        self.marker_prev_focus = window.focused(cx);
        window.focus(&self.marker_focus);
        cx.notify();
    }

    fn close_marker_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((target, _)) = self.marker_open.take() else {
            return;
        };
        overlay::pop(overlay::key(overlay::kind::MARKER_LIST));
        let prev = self.marker_prev_focus.take();
        if selected_target_matches(self.store.read(cx), &target)
            && let Some(prev) = prev
        {
            window.focus(&prev);
        }
        cx.notify();
    }

    fn render_marker_popover(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (target, pty_id) = self.marker_open.clone()?;
        if !selected_target_matches(self.store.read(cx), &target) {
            self.close_marker_popover(window, cx);
            return None;
        }
        let bounds = self.marker_anchor?;
        let panel_width = ui::font_px(MARKER_PANEL_WIDTH)
            .min((window.viewport_size().width - px(8.0)).max(px(1.0)));
        let anchor = point(bounds.right() - panel_width, bounds.bottom() + px(4.0));
        // 只画验明正身的那些 —— 与 `⚑ N` 的计数同一个口,否则会出现
        // 「按钮写着 5 条、点开只有 3 条」
        let markers = self.store.read(cx).visible_markers_for_pty(pty_id);
        // 列表本体单独一层:`overflow_y_scroll` 要 Stateful(必须带 id),
        // 而外层要 `track_focus`(Esc 的落点),两件事分层最省心
        let mut list = div()
            .id(SharedString::from(format!("marker-list-{pty_id}")))
            .w_full()
            .max_h(ui::font_px(MARKER_PANEL_MAX_HEIGHT))
            .overflow_y_scroll();

        if markers.is_empty() {
            // 到不了(按钮在 count == 0 时就不画了),照抄空态兜底 `MarkerList.tsx:22-28`
            list = list.child(
                div()
                    .px(px(12.0))
                    .py(px(8.0))
                    .text_size(ui::font_px(12.0))
                    .text_color(ui::text_muted())
                    .child(t("markerList", "empty")),
            );
        } else {
            list = list.py(px(4.0));
            for (idx, marker) in markers.into_iter().enumerate() {
                let marker_id = marker.id.clone();
                let store = self.store.clone();
                let target = target.clone();
                // 还没定位的条目:那条消息还在 AI 的队列里没上屏,没有行可跳。
                // **照样进列表**(用户看得到自己追加过什么),只是画成灰的,
                // 见 `markers::MarkerAnchor::Pending`
                let pending = marker.anchor.is_pending();
                list = list.child(
                    div()
                        .id(SharedString::from(format!("marker-{}", marker.id)))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .px(px(12.0))
                        .py(px(6.0))
                        .cursor_pointer()
                        .text_size(ui::font_px(12.0))
                        .text_color(if pending {
                            ui::text_muted()
                        } else {
                            ui::text_primary()
                        })
                        // `--bg-hover` 在 ui::Palette 里没有对应项,统一用 bg_overlay
                        // (与文件树行 hover 同一档)
                        .hover(|el| el.bg(ui::bg_overlay()))
                        // 悬停看全文(含粘贴多行时的换行);挂着的再补一句为什么跳不了
                        .tooltip({
                            let full = SharedString::from(if pending {
                                format!("{}\n\n{}", marker.line, t("markerList", "pendingAnchor"))
                            } else {
                                marker.line.clone()
                            });
                            move |window, cx| Tooltip::new(full.clone()).build(window, cx)
                        })
                        .on_click(cx.listener(move |this, _event, window, cx| {
                            cx.stop_propagation();
                            if !selected_target_matches(this.store.read(cx), &target) {
                                this.close_marker_popover(window, cx);
                                return;
                            }
                            let id = marker_id.clone();
                            // 挂着的条目点一下也有意义:`jump_to_marker` 会先补一次锚,
                            // AI 刚把它处理掉的话这一下就跳过去了
                            let jumped = store
                                .update(cx, |store, cx| store.jump_to_marker(pty_id, &id, cx));
                            // 跳转**并关闭浮层**(`MarkerList.tsx:36-39`);跳不动就不关 ——
                            // 关掉的话「点了没反应」会让人以为是坏的
                            if jumped {
                                this.close_marker_popover(window, cx);
                            }
                        }))
                        // 两列都是 min_w + nowrap:宽度跟着界面字号缩放,万一还是
                        // 算窄了(字族不同、`#100` 这种三位数),让它把列撑开,
                        // 而不是把 `15:29` 折成两行
                        .child(
                            div()
                                .flex_none()
                                .min_w(ui::font_px(MARKER_SEQ_W))
                                .whitespace_nowrap()
                                .text_color(ui::text_muted())
                                // 按**可见列表**里的位置编号,不用 `marker.seq`
                                // (那是全量列表里的位置,过滤掉候选条目之后会跳号:
                                // #1、#3、#4)。原版 seq 的语义本就是「它在列表里
                                // 排第几」,这里的「列表」就是用户看到的这一份
                                .child(format!("#{}", idx + 1)),
                        )
                        .child(
                            div()
                                .flex_none()
                                .min_w(ui::font_px(MARKER_TIME_W))
                                .whitespace_nowrap()
                                .text_color(ui::text_muted())
                                .child(markers::format_time(marker.ts)),
                        )
                        // 原版是 CSS `truncate`(= hidden + ellipsis + nowrap):
                        // 40 字是**字数**上限,窄面板里照样宽过一行,不 nowrap 会折
                        .child(
                            div()
                                .flex_1()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(markers::truncate_line(&marker.line, MARKER_LINE_MAX)),
                        )
                        // 进行中圆点。⚠️ 最后一条**永远**亮着 —— 原版没有任何地方
                        // 在 AI 完成时把它翻掉,照抄(见 markers::AiMarker::in_progress)
                        .when(marker.in_progress, |el| {
                            el.child(
                                div()
                                    .id(SharedString::from(format!("marker-dot-{}", marker.id)))
                                    .flex_none()
                                    .w(px(6.0))
                                    .h(px(6.0))
                                    .rounded_full()
                                    .bg(ui::color_ai_working())
                                    // 原版是 aria-label,gpui 没有 aria,落成 tooltip
                                    .tooltip(move |window, cx| {
                                        Tooltip::new(t("markerList", "inProgress")).build(window, cx)
                                    }),
                            )
                        }),
                );
            }
        }

        let panel = div()
            .track_focus(&self.marker_focus)
            .key_context("MarkerList")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.close_marker_popover(window, cx);
                }
            }))
            .w(panel_width)
            .rounded(px(6.0))
            .border_1()
            .border_color(ui::border_subtle())
            .bg(ui::bg_elevated())
            .shadow_lg()
            // 面板内的按下不算「点外」—— 遮罩的 on_mouse_down 靠 hitbox 判定
            .occlude()
            .child(list);

        let size = window.viewport_size();
        Some(
            deferred(
                anchored().position(point(px(0.0), px(0.0))).child(
                    div()
                        .w(size.width)
                        .h(size.height)
                        // 点浮层外任意处关闭(原版挂 document 的 mousedown)。
                        // occlude 让这一层吃掉这次按下 —— 否则关浮层那一下会顺手
                        // 点到底下的终端/tab
                        .occlude()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                                this.close_marker_popover(window, cx);
                            }),
                        )
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                                this.close_marker_popover(window, cx);
                            }),
                        )
                        .child(
                            anchored()
                                .position(anchor)
                                .snap_to_window_with_margin(px(4.0))
                                .child(panel),
                        ),
                ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    fn render_tools(
        &mut self,
        target: &TerminalJumpTarget,
        pty_id: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let marker_count = markers::visible(self.store.read(cx).markers_for_pty(pty_id)).count();
        let target_search = target.clone();
        let search = IconTooltips::button(
            &self.icon_tooltips,
            "terminal-search-description",
            t("terminalSearch", "title"),
            div()
                .id("terminal-search")
                .w(px(26.0))
                .h(px(26.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.0))
                .cursor_pointer()
                .hover(|el| el.bg(ui::border_subtle()))
                .child(VectorIcon::new(ICON_SEARCH, px(14.0)).ink(ui::text_muted()))
                .on_click(cx.listener(move |this, _event, window, cx| {
                    if !selected_target_matches(this.store.read(cx), &target_search) {
                        return;
                    }
                    let pane = this.store.read(cx).terminal(pty_id).cloned();
                    if let Some(pane) = pane {
                        pane.update(cx, |pane, cx| pane.open_search(window, cx));
                    }
                })),
            window,
            cx,
        );
        let mut tools = div()
            .id("terminal-tools")
            .h(px(30.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(4.0))
            .px(px(6.0))
            .child(search);
        if marker_count > 0 {
            let target = target.clone();
            let this = cx.entity();
            let markers = div()
                .id("terminal-markers")
                .relative()
                .h(px(24.0))
                .w(px(76.0))
                .flex_none()
                .px(px(6.0))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(4.0))
                .rounded(px(3.0))
                .cursor_pointer()
                .text_size(ui::font_px(12.0))
                .text_color(ui::text_muted())
                .hover(|el| el.bg(ui::border_subtle()))
                .child("⚑")
                .child(
                    div()
                        .min_w(px(0.0))
                        .truncate()
                        .child(marker_count.to_string()),
                )
                .child(
                    canvas(
                        move |bounds, _window, cx| {
                            this.update(cx, |area, _| area.marker_anchor = Some(bounds));
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
                .on_click(cx.listener(move |this, _event, window, cx| {
                    this.toggle_marker_popover(&target, pty_id, window, cx);
                }));
            tools = tools.child(IconTooltips::button(
                &self.icon_tooltips,
                "terminal-markers-description",
                mt_i18n::t_args(
                    "paneGroup",
                    "markerTooltip",
                    &[(
                        "mod",
                        if cfg!(target_os = "macos") {
                            "Cmd"
                        } else {
                            "Ctrl"
                        },
                    )],
                ),
                markers,
                window,
                cx,
            ));
        }
        IconTooltips::group(&self.icon_tooltips, tools, window, cx).into_any_element()
    }

    fn note_file_drag_over(
        &mut self,
        target: &TerminalJumpTarget,
        bounds: Bounds<Pixels>,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let next =
            if bounds.contains(&position) && selected_target_matches(self.store.read(cx), target) {
                Some(target.clone())
            } else {
                None
            };
        if self.file_drop_target != next {
            self.file_drop_target = next;
            cx.notify();
        }
    }

    fn insert_path_into_pane(
        &mut self,
        target: &TerminalJumpTarget,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.file_drop_target = None;
        if !text.is_empty() && selected_target_matches(self.store.read(cx), target) {
            self.store.update(cx, |store, cx| {
                store.write_to_pane(&target.project_id, target.pane_key.as_str(), text, cx);
                store.focus_pane(&target.project_id, target.pane_key.as_str(), window, cx);
            });
        }
        cx.notify();
    }

    fn render_reconnect(&self, target: &TerminalJumpTarget, cx: &mut Context<Self>) -> AnyElement {
        let target = target.clone();
        div()
            .id("terminal-reconnect")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.0))
            .bg(gpui::rgba(0x0000008c))
            .child(
                div()
                    .text_size(ui::font_px(12.0))
                    .text_color(ui::text_secondary())
                    .child(t("paneGroup", "remoteDisconnected")),
            )
            .child(
                ui::ghost_button("terminal-reconnect-button", t("paneGroup", "reconnect"))
                    .on_click(cx.listener(move |this, _event, window, cx| {
                        if !selected_target_matches(this.store.read(cx), &target) {
                            return;
                        }
                        this.store.update(cx, |store, cx| {
                            store.reset_pane_for_reconnect(
                                &target.project_id,
                                target.pane_key.as_str(),
                                cx,
                            );
                            store.focus_pane(
                                &target.project_id,
                                target.pane_key.as_str(),
                                window,
                                cx,
                            );
                        });
                    })),
            )
            .into_any_element()
    }
}

impl Render for TerminalArea {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = {
            let store = self.store.read(cx);
            store.active_project_id.as_ref().and_then(|project_id| {
                let pane = store
                    .project_state(project_id)?
                    .selected_terminal()?
                    .clone();
                let target = store.terminal_jump_target_for_pane(project_id, &pane.id)?;
                let terminal = store
                    .resolve_terminal_jump_target(&target)
                    .filter(|(_, live)| *live)
                    .and_then(|_| pane.pty_id.and_then(|pty| store.terminal(pty)))
                    .cloned();
                let reconnect = store.is_remote_project(project_id)
                    && pane.pty_id.is_some_and(|pty| store.is_pty_exited(pty));
                Some((target, pane, terminal, reconnect))
            })
        };
        if !cx.has_active_drag() {
            self.file_drop_target = None;
        }
        if self
            .marker_open
            .as_ref()
            .is_some_and(|(target, _)| !selected_target_matches(self.store.read(cx), target))
        {
            self.close_marker_popover(window, cx);
        }
        let mut root = div()
            .size_full()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(ui::bg_terminal());
        let Some((target, pane, terminal, reconnect)) = selected else {
            if self.store.read(cx).active_project().is_none() {
                return if self.store.read(cx).config().projects.is_empty() {
                    crate::first_run::guide(self.store.clone())
                } else {
                    root.items_center()
                        .justify_center()
                        .text_color(ui::text_muted())
                        .child(t("app", "emptyState"))
                };
            }
            let project_name = self
                .store
                .read(cx)
                .active_project()
                .map(|project| project.name.clone());
            return root.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(12.0))
                    .text_color(ui::text_muted())
                    .children(
                        project_name.map(|name| {
                            div().child(tr!("terminalArea", "emptyTitle", project = name))
                        }),
                    )
                    .when(self.store.read(cx).active_project_id.is_some(), |el| {
                        el.child(
                            ui::ghost_button(
                                "empty-new-terminal",
                                t("terminalArea", "newTerminal"),
                            )
                            .on_click(cx.listener(
                                |this, event: &ClickEvent, window, cx| {
                                    open_new_terminal_menu(
                                        this.store.clone(),
                                        click_position(event, window),
                                        window,
                                        cx,
                                    );
                                },
                            )),
                        )
                    }),
            );
        };
        if let Some(pty_id) = pane.pty_id {
            root = root.child(self.render_tools(&target, pty_id, window, cx));
        }
        let file_drop_over = self.file_drop_target.as_ref() == Some(&target);
        let focus_target = target.clone();
        let mut body = div()
            .id("selected-terminal-body")
            .flex_1()
            .min_h(px(0.0))
            .relative()
            .overflow_hidden()
            .on_click(cx.listener(move |this, _event, window, cx| {
                if selected_target_matches(this.store.read(cx), &focus_target) {
                    this.store.update(cx, |store, cx| {
                        store.focus_pane(
                            &focus_target.project_id,
                            focus_target.pane_key.as_str(),
                            window,
                            cx,
                        );
                    });
                }
            }))
            .on_drag_move(cx.listener({
                let target = target.clone();
                move |this, event: &gpui::DragMoveEvent<crate::dnd::DragFilePath>, _window, cx| {
                    this.note_file_drag_over(&target, event.bounds, event.event.position, cx);
                }
            }))
            .on_drag_move(cx.listener({
                let target = target.clone();
                move |this, event: &gpui::DragMoveEvent<gpui::ExternalPaths>, _window, cx| {
                    this.note_file_drag_over(&target, event.bounds, event.event.position, cx);
                }
            }))
            .on_drop(cx.listener({
                let target = target.clone();
                move |this, item: &crate::dnd::DragFilePath, window, cx| {
                    this.insert_path_into_pane(
                        &target,
                        &crate::dnd::quote_path(&item.0),
                        window,
                        cx,
                    );
                }
            }))
            .on_drop(cx.listener({
                let target = target.clone();
                move |this, item: &gpui::ExternalPaths, window, cx| {
                    this.insert_path_into_pane(
                        &target,
                        &crate::dnd::quote_paths(item.paths()),
                        window,
                        cx,
                    );
                }
            }));
        // A selection change never mounts an outgoing live terminal.
        body = if let Some(terminal) = terminal {
            body.child(terminal)
        } else {
            body.child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(ui::text_muted())
                    .child(t(
                        "paneGroup",
                        if pane.status == crate::tree::PaneStatus::Error {
                            "startFailed"
                        } else {
                            "starting"
                        },
                    )),
            )
        };
        root.child(
            body.when(file_drop_over, |el| el.child(drop_hint()))
                .when(reconnect, |el| el.child(self.render_reconnect(&target, cx))),
        )
        .children(self.render_marker_popover(window, cx))
    }
}

fn drop_hint() -> AnyElement {
    div()
        .absolute()
        .inset(px(4.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .border_2()
        .border_dashed()
        .border_color(ui::accent())
        .bg(ui::accent_subtle())
        .child(
            div()
                .px(px(12.0))
                .py(px(6.0))
                .rounded(px(6.0))
                .bg(ui::bg_overlay())
                .text_size(ui::font_px(9.75))
                .text_color(ui::accent())
                .child(t("terminal", "dropToInsertPath")),
        )
        .into_any_element()
}

pub(crate) fn click_position(event: &ClickEvent, window: &Window) -> gpui::Point<gpui::Pixels> {
    match event {
        ClickEvent::Mouse(e) => e.up.position,
        ClickEvent::Keyboard(_) => window.mouse_position(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fork_and_close_copy_describe_individual_terminals() {
        for (locale, terminal_word, split_word) in [
            (mt_i18n::Locale::En, "terminal", "split"),
            (mt_i18n::Locale::Zh, "终端", "分屏"),
        ] {
            for (namespace, key) in [
                ("paneGroup", "forkSession"),
                ("settings", "shortcuts.closePane"),
            ] {
                let label = mt_i18n::lookup(locale, namespace, key).expect("navigation label");
                assert!(label.contains(terminal_word), "{namespace}.{key}: {label}");
                assert!(!label.contains(split_word), "{namespace}.{key}: {label}");
            }
        }
    }

    #[test]
    fn terminal_menu_closes_only_one_terminal_and_never_offers_splits() {
        use TabMenuAction::*;
        assert_eq!(
            tab_menu_actions(false, false),
            vec![Some(Rename), None, Some(CloseTab)]
        );
        assert_eq!(
            tab_menu_actions(true, false),
            vec![
                Some(Rename),
                None,
                Some(ForkSession),
                Some(ViewSessionBranches),
                None,
                Some(CloseTab),
            ]
        );
        assert_eq!(
            tab_menu_actions(false, true),
            vec![
                Some(Rename),
                None,
                Some(ForkNeedsIdentity),
                None,
                Some(CloseTab),
            ]
        );
        assert!(!tab_menu_actions(true, true).contains(&Some(ForkNeedsIdentity)));
    }
}
