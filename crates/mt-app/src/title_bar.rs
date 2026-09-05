//! Flat terminal tabs and native window controls.
//! Only the logo and trailing blank area are window-drag hit regions.
//! Windows control buttons deliberately have no click handlers: native hit
//! testing owns their actions and the guarded window-close request.

use std::collections::HashMap;

use gpui::{
    AnyElement, App, AppContext, Bounds, ClickEvent, Context, Div, Entity, FocusHandle,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, Pixels, Render,
    ScrollHandle, SharedString, Stateful, StatefulInteractiveElement, Styled, Window,
    WindowControlArea, canvas, div, point, prelude::FluentBuilder, px,
};
use mt_identity::{PaneKey, WorktreeId};
use mt_ui::icon_tooltip::IconTooltips;
use mt_ui::icons::{AiVendor, BrandIcon, Geom, Ink, Shape, VectorIcon};
use mt_ui::rgb8;
use mt_ui::tooltip::Tooltip;

use crate::dnd::{self, DragTerminalTab};
use crate::i18n::t;
use crate::menu;
use crate::pane_actions;
use crate::prompt::Confirm;
use crate::store::{AppStore, TerminalJumpTarget, TerminalJumpView};
use crate::terminal_area::{click_position, open_new_terminal_menu, tab_menu};
use crate::ui;

pub const HEIGHT: f32 = 44.0;

/// macOS 交通灯占位:三颗灯 + 左右留白,内容从这条线之后开始。
const MAC_TRAFFIC_LIGHT_WIDTH: f32 = 78.0;

/// 单颗窗口控制键的宽度(原版 `w-[46px]`)。
const BUTTON_WIDTH: f32 = 46.0;

/// 关闭键 hover 的**Windows 系统红**。原版就是写死的 `#c42b1c` 字面量而不是
/// CSS 变量 —— 它不属于配色主题,换主题包也不该变。
const CLOSE_HOVER_BG: (u8, u8, u8) = (0xc4, 0x2b, 0x1c);

// ─── 图形 ────────────────────────────────────────────────────
//
// 窗口控制三键全部是 **10×10 viewBox / stroke=currentColor / strokeWidth=1** 的细线
// (原版注释:「Windows 的画法是 10×10 内的细线,不是 Material 那种粗描边」)。
// 单位方框换算 = 除以 10。

const fn u10(v: f32) -> f32 {
    v / 10.0
}
/// 三个窗口控制图标统一 `stroke-width="1"`。
const STROKE10: f32 = 1.0 / 10.0;

/// 最小化。原版 `<path d="M0 5.5h10"/>` —— 一条横线。
pub const ICON_MINIMIZE: &[Shape] = &[Shape::line(
    Ink::Current,
    STROKE10,
    Geom::Polyline(&[(u10(0.0), u10(5.5)), (u10(10.0), u10(5.5))]),
)];

/// 最大化。原版 `<rect x="0.5" y="0.5" width="9" height="9"/>`。
pub const ICON_MAXIMIZE: &[Shape] = &[Shape::line(
    Ink::Current,
    STROKE10,
    Geom::Rect {
        x: u10(0.5),
        y: u10(0.5),
        w: u10(9.0),
        h: u10(9.0),
        round: 0.0,
    },
)];

/// 还原(已最大化态)。原版 `<rect x="0.5" y="2.5" width="7" height="7"/>` +
/// `<path d="M2.5 2.5V0.5h7v7h-2"/>` —— 后面那层窗口**只画露出来的两条边**
/// (原注释:画成完整方框会糊成一团)。
pub const ICON_RESTORE: &[Shape] = &[
    Shape::line(
        Ink::Current,
        STROKE10,
        Geom::Rect {
            x: u10(0.5),
            y: u10(2.5),
            w: u10(7.0),
            h: u10(7.0),
            round: 0.0,
        },
    ),
    Shape::line(
        Ink::Current,
        STROKE10,
        Geom::Polyline(&[
            (u10(2.5), u10(2.5)),
            (u10(2.5), u10(0.5)),
            (u10(9.5), u10(0.5)),
            (u10(9.5), u10(7.5)),
            (u10(7.5), u10(7.5)),
        ]),
    ),
];

/// 关闭。原版 `<path d="M0.5 0.5l9 9M9.5 0.5l-9 9"/>` —— 两条对角线。
pub const ICON_CLOSE: &[Shape] = &[
    Shape::line(
        Ink::Current,
        STROKE10,
        Geom::Polyline(&[(u10(0.5), u10(0.5)), (u10(9.5), u10(9.5))]),
    ),
    Shape::line(
        Ink::Current,
        STROKE10,
        Geom::Polyline(&[(u10(9.5), u10(0.5)), (u10(0.5), u10(9.5))]),
    ),
];

/// 品牌 logo。原版 viewBox 16 / strokeWidth 1.3:
/// `<rect x="1.5" y="2.5" width="13" height="11" rx="1.5"/>` +
/// `<path d="M4.5 6.5L6.5 8l-2 1.5M8.5 10h3"/>`(终端窗口 + 提示符 + 光标横线)。
pub const ICON_LOGO: &[Shape] = &[
    Shape::line(
        Ink::Current,
        1.3 / 16.0,
        Geom::Rect {
            x: 1.5 / 16.0,
            y: 2.5 / 16.0,
            w: 13.0 / 16.0,
            h: 11.0 / 16.0,
            round: 1.5 / 16.0,
        },
    ),
    Shape::line(
        Ink::Current,
        1.3 / 16.0,
        Geom::Polyline(&[(4.5 / 16.0, 6.5 / 16.0), (6.5 / 16.0, 8.0 / 16.0), (4.5 / 16.0, 9.5 / 16.0)]),
    ),
    Shape::line(
        Ink::Current,
        1.3 / 16.0,
        Geom::Polyline(&[(8.5 / 16.0, 10.0 / 16.0), (11.5 / 16.0, 10.0 / 16.0)]),
    ),
];

/// 项目切换胶囊的下拉箭头。原版 viewBox 10 / strokeWidth 1.2 /
/// `<path d="M1.5 3.25L5 6.75l3.5-3.5"/>`,比窗口控制图标小一号、同样的细线风格。
pub const ICON_CHEVRON_DOWN: &[Shape] = &[Shape::line(
    Ink::Current,
    1.2 / 10.0,
    Geom::Polyline(&[
        (u10(1.5), u10(3.25)),
        (u10(5.0), u10(6.75)),
        (u10(8.5), u10(3.25)),
    ]),
)];

// ─── 纯函数(可测) ──────────────────────────────────────────

/// 最大化键的两态:已最大化画「还原」,否则画「最大化」。tooltip 跟着换。
///
/// 原版靠 `appWindow.isMaximized()` + `onResized` 订阅维护一个 state;GPUI 每帧
/// 重绘,`window.is_maximized()` 在 `render` 里直接读即可 —— 那套订阅整个删掉。
pub fn max_button_face(maximized: bool) -> (&'static [Shape], &'static str) {
    if maximized {
        (ICON_RESTORE, "titleBar.restore")
    } else {
        (ICON_MAXIMIZE, "titleBar.maximize")
    }
}


const TAB_WIDTH: f32 = 176.0;

pub fn blink_phase(delta: f32) -> f32 {
    let triangle = 1.0 - (delta * 2.0 - 1.0).abs();
    // smoothstep,等价于 CSS 的 ease-in-out
    triangle * triangle * (3.0 - 2.0 * triangle)
}


thread_local! {
    /// 已经确认过要关了 —— 之后任何一次询问都直接放行。见上面的防重入说明。
    static FORCE_CLOSE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

const CLOSE_RISK_PREVIEW_LIMIT: usize = 5;

fn close_risk_preview(items: &[String]) -> String {
    let mut lines = items
        .iter()
        .take(CLOSE_RISK_PREVIEW_LIMIT)
        .cloned()
        .collect::<Vec<_>>();
    let remaining = items.len().saturating_sub(lines.len());
    if remaining > 0 {
        lines.push(crate::i18n::tr!(
            "app",
            "closeConfirm.remaining",
            count = remaining
        ));
    }
    lines.join("\n")
}

/// 现在可以关窗吗。`false` = 这次别关(确认框已经弹出来了 / 或正开着)。
///
/// 这是 `on_window_should_close` 的**同步**回调体:gpui 要求当场返回 bool,
/// 而确认框是异步的(用户点了才有结果)。套路即上图 —— 先返回 false 把这次关闭
/// 吞掉,确认回调里再走 `remove_window()`(它绕过本函数)。
pub fn allow_close(window: &mut Window, cx: &mut App) -> bool {
    if FORCE_CLOSE.with(|f| f.get()) {
        return true;
    }

    let live = crate::pane_actions::collect_live_ai_panes(AppStore::global(cx).read(cx));
    let dirty_documents = crate::workbench_area::dirty_document_names(cx);
    if live.is_empty() && dirty_documents.is_empty() {
        finish_close(cx);
        return true;
    }

    // 正文里那两个换行符由 `prompt::body` 拆成多个 child —— gpui 的文本不认转义符
    let (title, message) = match (dirty_documents.is_empty(), live.is_empty()) {
        (true, false) => (
            t("app", "closeConfirm.titleAi"),
            crate::i18n::tr!(
                "app",
                "closeConfirm.messageWithSessions",
                count = live.len(),
                names = close_risk_preview(&live)
            ),
        ),
        (false, true) => (
            t("app", "closeConfirm.titleUnsaved"),
            crate::i18n::tr!(
                "app",
                "closeConfirm.messageWithDocuments",
                count = dirty_documents.len(),
                names = close_risk_preview(&dirty_documents)
            ),
        ),
        (false, false) => (
            t("app", "closeConfirm.titleUnsaved"),
            crate::i18n::tr!(
                "app",
                "closeConfirm.messageWithDocumentsAndSessions",
                document_count = dirty_documents.len(),
                document_names = close_risk_preview(&dirty_documents),
                session_count = live.len(),
                session_names = close_risk_preview(&live)
            ),
        ),
        (true, true) => unreachable!("close risks were checked above"),
    };
    Confirm::new(title, message).open(
        move |window, cx| {
            FORCE_CLOSE.with(|f| f.set(true));
            finish_close(cx);
            window.remove_window();
        },
        window,
        cx,
    );
    false
}

/// 真关之前把配置刷下去。
///
/// 原版在这里逐项目 `flushLayoutToConfig` / `flushExpandedDirsToConfig` 再
/// `persistConfig()`;GPUI 侧那两步是**即时**写进 `self.config` 的(布局与展开目录
/// 一变就落进配置结构,只有写盘走 500ms 去抖),所以只差最后这一次强刷。
///
/// `cx.on_app_quit` 里也有一次 `save_config_now`(S 批记档)—— 那条管「进程退出」,
/// 这条管「窗口关闭」,两条都要有:写盘本身是幂等的。
fn finish_close(cx: &mut App) {
    AppStore::global(cx).update(cx, |store, _| store.save_config_now());
}

/// 关闭窗口(Linux 降级路径的 ✕;Windows 上关闭键走系统 `WM_CLOSE`,
/// 进的是 `main.rs` 注册的 `on_window_should_close`)。
///
/// 两条路都过 [`allow_close`],确认口径于是只有一份。
pub fn request_close_window(window: &mut Window, cx: &mut App) {
    if allow_close(window, cx) {
        window.remove_window();
    }
}

/// 最小化窗口(Linux 降级路径;Windows 由 `WindowControlArea::Min` 接管)。
pub fn minimize(window: &mut Window, _cx: &mut App) {
    window.minimize_window();
}

/// 最大化 / 还原(Linux 降级路径)。
///
/// ⚠️ gpui 的 `zoom_window()` 在 **Windows 上只会最大化**
/// (`platform/windows/window.rs:782` 直接 `ShowWindowAsync(SW_MAXIMIZE)`,没有还原
/// 分支)。这条路径在 Windows 上本来就到不了(`WindowControlArea::Max` 会把
/// `WM_NCLBUTTONUP` 变成系统的 `is_maximized() ? SW_NORMAL : SW_MAXIMIZE`),
/// 所以照抄 gpui-component 的 Linux 分支即可;真在 Windows 上退化到这里
/// (窗口不可移动 / 全屏时命中测试早退)只会少一个「还原」方向。
pub fn toggle_maximize(window: &mut Window, _cx: &mut App) {
    window.zoom_window();
}


#[derive(Clone, Copy, PartialEq, Eq)]
enum Control {
    Min,
    Max,
    Close,
}

fn revealed_offset(current: f32, viewport: f32, index: usize, tab_width: f32) -> f32 {
    let left = index as f32 * tab_width;
    let right = left + tab_width;
    if left < current || viewport < tab_width {
        left
    } else if right > current + viewport {
        (right - viewport).max(0.0)
    } else {
        current
    }
}

pub struct TitleBar {
    store: Entity<AppStore>,
    workbench: Entity<crate::workbench_area::WorkbenchArea>,
    navigation_tooltips: Entity<IconTooltips>,
    window_tooltips: Entity<IconTooltips>,
    tab_focus: HashMap<PaneKey, FocusHandle>,
    add_focus: FocusHandle,
    overflow_focus: FocusHandle,
    scroll: ScrollHandle,
    last_scope: Option<(String, WorktreeId)>,
    last_selected: Option<PaneKey>,
    last_terminal_page_active: bool,
    last_order: Vec<PaneKey>,
    tabs_width: Pixels,
    tabs_bounds: Option<Bounds<Pixels>>,
    reveal_selected: bool,
    tab_drop: Option<(TerminalJumpTarget, bool)>,
}

impl TitleBar {
    pub fn new(
        store: Entity<AppStore>,
        workbench: Entity<crate::workbench_area::WorkbenchArea>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        cx.observe(&workbench, |_, _, cx| cx.notify()).detach();
        Self {
            store,
            workbench,
            navigation_tooltips: cx.new(|_| IconTooltips::default()),
            window_tooltips: cx.new(|_| IconTooltips::default()),
            tab_focus: HashMap::new(),
            add_focus: cx.focus_handle().tab_stop(true),
            overflow_focus: cx.focus_handle().tab_stop(true),
            scroll: ScrollHandle::new(),
            last_scope: None,
            last_selected: None,
            last_terminal_page_active: false,
            last_order: Vec::new(),
            tabs_width: px(0.0),
            tabs_bounds: None,
            reveal_selected: true,
            tab_drop: None,
        }
    }

    fn control_button(
        &self,
        which: Control,
        shapes: &'static [Shape],
        tip: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = match which {
            Control::Min => "titlebar-min",
            Control::Max => "titlebar-max",
            Control::Close => "titlebar-close",
        };
        let button = div()
            .id(id)
            .w(px(BUTTON_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .hover(move |el| {
                el.bg(if which == Control::Close {
                    rgb8(CLOSE_HOVER_BG.0, CLOSE_HOVER_BG.1, CLOSE_HOVER_BG.2)
                } else {
                    ui::border_default()
                })
            })
            .child(VectorIcon::new(shapes, px(10.0)).ink(ui::text_primary()));
        IconTooltips::button(
            &self.window_tooltips,
            SharedString::from(format!("{id}-description")),
            t("app", tip),
            button,
            window,
            cx,
        )
    }

    fn open_tab_overflow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        IconTooltips::reset(&self.navigation_tooltips, window, cx);
        let titlebar = cx.entity();
        let entries = self
            .store
            .read(cx)
            .active_project_id
            .as_ref()
            .map(|project| self.store.read(cx).terminal_tab_views(project))
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, view)| {
                let store = self.store.clone();
                let titlebar = titlebar.clone();
                menu::item(
                    format!("{}. {}", index + 1, view.pane_label),
                    move |window, cx| {
                        if AppStore::activate_terminal_jump_target(&store, &view.target, window, cx)
                        {
                            titlebar.update(cx, |bar, cx| {
                                bar.reveal_selected = true;
                                cx.notify();
                            });
                        }
                    },
                )
            })
            .collect();
        let position = self
            .tabs_bounds
            .map(|bounds| point(bounds.right() + px(32.0), bounds.bottom()))
            .unwrap_or_else(|| window.mouse_position());
        menu::show(position, entries, window, cx);
    }

    fn render_tab(
        &mut self,
        view: &TerminalJumpView,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let target = view.target.clone();
        let key = target.pane_key.clone();
        let focus = self
            .tab_focus
            .entry(key.clone())
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let vendor = {
            let store = self.store.read(cx);
            store
                .project_state(&target.project_id)
                .and_then(|state| state.pane(target.pane_key.as_str()))
                .filter(|pane| pane.shows_ai_session(store.config().ai_auto_resume.unwrap_or(true)))
                .and_then(|pane| pane.ai_agent())
                .and_then(|agent| {
                    AiVendor::from_session_type(agent)
                        .or_else(|| AiVendor::infer(Some(agent), None))
                })
        };
        let unread = self.store.read(cx).is_pane_unread_done(key.as_str());
        let active = view.active && self.workbench.read(cx).is_terminal_active(cx);
        let label = view.pane_label.clone();
        let target_click = target.clone();
        let target_key = target.clone();
        let target_menu = target.clone();
        let target_close = target.clone();
        let label_menu = label.clone();
        let drop_side = self
            .tab_drop
            .as_ref()
            .filter(|(candidate, _)| candidate == &target)
            .map(|(_, after)| *after);
        let close = IconTooltips::button(
            &self.navigation_tooltips,
            SharedString::from(format!("terminal-close-description-{key}")),
            t("paneGroup", "closeTab"),
            div()
                .id(SharedString::from(format!("terminal-close-{key}")))
                .w(px(24.0))
                .h(px(24.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.0))
                .cursor_pointer()
                .hover(|el| el.bg(ui::border_subtle()))
                .child(VectorIcon::new(ICON_CLOSE, px(10.0)).ink(ui::text_muted()))
                .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                    cx.stop_propagation()
                })
                .on_click(cx.listener(move |this, _event, window, cx| {
                    cx.stop_propagation();
                    pane_actions::close_terminal_target(
                        this.store.clone(),
                        target_close.clone(),
                        window,
                        cx,
                    );
                })),
            window,
            cx,
        );
        div()
            .id(SharedString::from(format!("terminal-tab-{key}")))
            .relative()
            .w(px(TAB_WIDTH))
            .h_full()
            .flex_none()
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .track_focus(&focus)
            .tab_index(0)
            .cursor_pointer()
            .border_t_2()
            .border_color(if active {
                ui::accent()
            } else {
                ui::with_alpha(ui::accent(), 0.0)
            })
            .bg(if active {
                ui::bg_terminal()
            } else {
                ui::bg_surface()
            })
            .text_color(if active {
                ui::text_primary()
            } else {
                ui::text_muted()
            })
            .text_size(ui::font_px(13.0))
            .hover(|el| el.bg(ui::bg_overlay()))
            .on_mouse_down(MouseButton::Left, |_event, window, _cx| {
                // Reordering preserves focus; a completed click activates its exact target.
                window.prevent_default();
            })
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.stop_propagation();
                    AppStore::activate_terminal_jump_target(&this.store, &target_key, window, cx);
                }
            }))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                cx.stop_propagation();
                if !AppStore::activate_terminal_jump_target(&this.store, &target_click, window, cx)
                {
                    return;
                }
                if event.click_count() >= 2 {
                    crate::modal::open_rename_pane(
                        this.store.clone(),
                        target_click.project_id.clone(),
                        target_click.pane_key.to_string(),
                        label.clone(),
                        window,
                        cx,
                    );
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    IconTooltips::reset(&this.navigation_tooltips, window, cx);
                    let entries = tab_menu(&this.store, &target_menu, &label_menu, cx);
                    if !entries.is_empty() {
                        menu::show(event.position, entries, window, cx);
                    }
                }),
            )
            .on_drag(
                DragTerminalTab {
                    target: target.clone(),
                },
                {
                    let label = view.pane_label.clone();
                    move |_item, _offset, _window, cx| {
                        dnd::preview(label.clone(), dnd::PreviewIcon::Terminal, cx)
                    }
                },
            )
            .on_drag_move(cx.listener({
                let target = target.clone();
                move |this, event: &gpui::DragMoveEvent<DragTerminalTab>, _window, cx| {
                    let source = &event.drag(cx).target;
                    let store = this.store.read(cx);
                    let after = dnd::terminal_tab_drop_after(event.bounds, event.event.position)
                        .filter(|_| {
                            this.tabs_bounds
                                .is_some_and(|bounds| bounds.contains(&event.event.position))
                                && source.project_id == target.project_id
                                && source.worktree_id == target.worktree_id
                                && source.pane_key != target.pane_key
                                && store.active_project_id.as_deref()
                                    == Some(target.project_id.as_str())
                                && store.resolve_terminal_jump_target(source).is_some()
                                && store.resolve_terminal_jump_target(&target).is_some()
                        });
                    let next = after.map(|after| (target.clone(), after));
                    if (next.is_some()
                        || this
                            .tab_drop
                            .as_ref()
                            .is_some_and(|(candidate, _)| candidate == &target))
                        && this.tab_drop != next
                    {
                        this.tab_drop = next;
                        cx.notify();
                    }
                }
            }))
            .on_drop(cx.listener({
                let target = target.clone();
                move |this, item: &DragTerminalTab, _window, cx| {
                    let Some((destination, after)) = this.tab_drop.take() else {
                        return;
                    };
                    if destination == target {
                        this.store.update(cx, |store, cx| {
                            store.reorder_terminal_tabs(&item.target, &destination, after, cx);
                        });
                    }
                    cx.notify();
                }
            }))
            .child(
                div()
                    .w(px(24.0))
                    .flex_none()
                    .truncate()
                    .text_size(ui::font_px(10.0))
                    .child((index + 1).to_string()),
            )
            .when_some(vendor, |el, vendor| {
                el.child(
                    BrandIcon::new(Some(vendor))
                        .size(px(14.0))
                        .color(ui::text_muted()),
                )
            })
            .child(
                div()
                    .id(SharedString::from(format!("terminal-label-{key}")))
                    .min_w(px(0.0))
                    .flex_1()
                    .truncate()
                    .child(view.pane_label.clone())
                    .tooltip({
                        let title = view.pane_label.clone();
                        move |window, cx| Tooltip::new(title.clone()).build(window, cx)
                    }),
            )
            .child(
                div()
                    .w(px(5.0))
                    .h(px(5.0))
                    .flex_none()
                    .rounded_full()
                    .when(unread, |el| el.bg(ui::color_success()))
                    .when(view.status == crate::tree::PaneStatus::Error, |el| {
                        el.bg(ui::color_error())
                    }),
            )
            .child(close)
            .when_some(drop_side, |el, after| {
                el.child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .w(px(3.0))
                        .bg(ui::accent())
                        .when(after, |el| el.right_0())
                        .when(!after, |el| el.left_0()),
                )
            })
            .into_any_element()
    }
}

impl Render for TitleBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (scope, views) = {
            let store = self.store.read(cx);
            let scope = store
                .active_project_id
                .clone()
                .zip(store.active_worktree_id().cloned());
            let views = scope
                .as_ref()
                .map(|(project, _)| store.terminal_tab_views(project))
                .unwrap_or_default();
            (scope, views)
        };
        let order: Vec<_> = views
            .iter()
            .map(|view| view.target.pane_key.clone())
            .collect();
        let selected = views
            .iter()
            .find(|view| view.active)
            .map(|view| view.target.pane_key.clone());
        let terminal_page_active = self.workbench.read(cx).is_terminal_active(cx);
        if self.last_terminal_page_active != terminal_page_active {
            self.last_terminal_page_active = terminal_page_active;
            self.reveal_selected = true;
        }
        if self.last_scope != scope {
            self.last_scope = scope.clone();
            self.scroll = ScrollHandle::new();
            self.tab_drop = None;
            IconTooltips::reset(&self.navigation_tooltips, window, cx);
            self.reveal_selected = true;
        }
        if self.last_selected != selected || self.last_order != order {
            self.last_selected = selected;
            self.last_order = order;
            self.reveal_selected = true;
        }
        self.tab_focus
            .retain(|key, _| self.last_order.contains(key));
        if !cx.has_active_drag() {
            self.tab_drop = None;
        }
        let selected_index = views.iter().position(|view| view.active);
        let this = cx.entity();
        let mut tabs = div()
            .id("titlebar-terminal-tabs")
            .relative()
            .w_full()
            .min_w(px(0.0))
            .h_full()
            .flex()
            .items_center()
            .overflow_x_scroll()
            .track_scroll(&self.scroll)
            .on_drag_move(cx.listener(
                |this, event: &gpui::DragMoveEvent<DragTerminalTab>, _window, cx| {
                    if !event.bounds.contains(&event.event.position) {
                        return;
                    }
                    let offset = this.scroll.offset();
                    let max_scroll = (this.last_order.len() as f32 * TAB_WIDTH
                        - f32::from(event.bounds.size.width))
                    .max(0.0);
                    let step = if event.event.position.x < event.bounds.origin.x + px(20.0) {
                        -16.0
                    } else if event.event.position.x > event.bounds.right() - px(20.0) {
                        16.0
                    } else {
                        return;
                    };
                    let next = (-f32::from(offset.x) + step).clamp(0.0, max_scroll);
                    if (f32::from(offset.x) + next).abs() > 0.5 {
                        this.scroll.set_offset(point(px(-next), offset.y));
                        cx.notify();
                    }
                },
            ));
        for (index, view) in views.iter().enumerate() {
            tabs = tabs.child(self.render_tab(view, index, window, cx));
        }
        let tabs = div()
            .id("titlebar-tabs-viewport")
            .relative()
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .overflow_hidden()
            .child(tabs)
            .child(
                canvas(
                    move |bounds: Bounds<Pixels>, _window, cx| {
                        this.update(cx, |bar, cx| {
                            bar.tabs_bounds = Some(bounds);
                            if bar.tabs_width != bounds.size.width || bar.reveal_selected {
                                bar.tabs_width = bounds.size.width;
                                bar.reveal_selected = false;
                                if let Some(index) = selected_index {
                                    let offset = bar.scroll.offset();
                                    let current = (-f32::from(offset.x)).max(0.0);
                                    let next = revealed_offset(
                                        current,
                                        f32::from(bounds.size.width),
                                        index,
                                        TAB_WIDTH,
                                    );
                                    if (next - current).abs() > 0.5 {
                                        bar.scroll.set_offset(point(px(-next), offset.y));
                                        cx.notify();
                                    }
                                }
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        let mut navigation = div()
            .id("titlebar-terminal-navigation")
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .flex()
            .items_center()
            .child(tabs);
        if scope.is_some() {
            let add = div()
                .id("titlebar-new-terminal")
                .w(px(32.0))
                .h(px(32.0))
                .track_focus(&self.add_focus)
                .tab_index(0)
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .rounded(px(3.0))
                .text_color(ui::text_muted())
                .text_size(px(20.0))
                .hover(|el| el.bg(ui::border_subtle()))
                .child("+")
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        IconTooltips::reset(&this.navigation_tooltips, window, cx);
                        let position = this
                            .tabs_bounds
                            .map(|bounds| point(bounds.right(), bounds.bottom()))
                            .unwrap_or_else(|| window.mouse_position());
                        open_new_terminal_menu(this.store.clone(), position, window, cx);
                    }
                }))
                .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                    IconTooltips::reset(&this.navigation_tooltips, window, cx);
                    open_new_terminal_menu(
                        this.store.clone(),
                        click_position(event, window),
                        window,
                        cx,
                    );
                }));
            navigation = navigation.child(IconTooltips::button(
                &self.navigation_tooltips,
                "titlebar-new-description",
                t("terminalArea", "newTerminal"),
                add,
                window,
                cx,
            ));
        }
        if !views.is_empty() {
            let overflow = div()
                .id("titlebar-terminal-overflow")
                .w(px(32.0))
                .h(px(32.0))
                .track_focus(&self.overflow_focus)
                .tab_index(0)
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .rounded(px(3.0))
                .hover(|el| el.bg(ui::border_subtle()))
                .child(VectorIcon::new(ICON_CHEVRON_DOWN, px(12.0)).ink(ui::text_muted()))
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        this.open_tab_overflow(window, cx);
                    }
                }))
                .on_click(
                    cx.listener(|this, _event, window, cx| this.open_tab_overflow(window, cx)),
                );
            navigation = navigation.child(IconTooltips::button(
                &self.navigation_tooltips,
                "titlebar-overflow-description",
                t("paneGroup", "tablistLabel"),
                overflow,
                window,
                cx,
            ));
        }
        let navigation = IconTooltips::group(&self.navigation_tooltips, navigation, window, cx);
        let (max_shapes, max_tip) = max_button_face(window.is_maximized());
        let click_fallback = cfg!(target_os = "linux");
        let is_mac = cfg!(target_os = "macos");
        let controls = div()
            .id("titlebar-window-controls")
            .flex()
            .h_full()
            .flex_none()
            .child(
                self.control_button(Control::Min, ICON_MINIMIZE, "titleBar.minimize", window, cx)
                    .window_control_area(WindowControlArea::Min)
                    .when(click_fallback, |el| {
                        el.on_click(|_, window, cx| minimize(window, cx))
                    }),
            )
            .child(
                self.control_button(Control::Max, max_shapes, max_tip, window, cx)
                    .window_control_area(WindowControlArea::Max)
                    .when(click_fallback, |el| {
                        el.on_click(|_, window, cx| toggle_maximize(window, cx))
                    }),
            )
            .child(
                self.control_button(Control::Close, ICON_CLOSE, "titleBar.close", window, cx)
                    .window_control_area(WindowControlArea::Close)
                    .when(click_fallback, |el| {
                        el.on_click(|_, window, cx| request_close_window(window, cx))
                    }),
            );
        let controls = IconTooltips::group(&self.window_tooltips, controls, window, cx);
        div()
            .w_full()
            .h(px(HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .bg(ui::bg_surface())
            .border_b_1()
            .border_color(ui::border_subtle())
            .when(is_mac, |el| {
                el.child(div().w(px(MAC_TRAFFIC_LIGHT_WIDTH)).h_full().flex_none())
            })
            .child(
                div()
                    .h_full()
                    .px(px(12.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .window_control_area(WindowControlArea::Drag)
                    .child(VectorIcon::new(ICON_LOGO, px(14.0)).ink(ui::text_muted()))
                    .when(window.viewport_size().width > px(760.0), |el| {
                        el.child(
                            div()
                                .text_size(ui::font_px(12.0))
                                .text_color(ui::text_secondary())
                                .child("Mini-Term"),
                        )
                    }),
            )
            .child(navigation)
            .child(
                div()
                    .w(px(28.0))
                    .h_full()
                    .flex_none()
                    .window_control_area(WindowControlArea::Drag),
            )
            .when(!is_mac, |el| el.child(controls))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_tabs_are_revealed_without_scrolling_visible_tabs() {
        assert_eq!(revealed_offset(0.0, 352.0, 0, 176.0), 0.0);
        assert_eq!(revealed_offset(0.0, 352.0, 3, 176.0), 352.0);
        assert_eq!(revealed_offset(352.0, 352.0, 0, 176.0), 0.0);
        assert_eq!(revealed_offset(100.0, 352.0, 1, 176.0), 100.0);
        assert_eq!(revealed_offset(0.0, 90.0, 2, 176.0), 352.0);
    }

    /// 形状表的点必须全落在单位方框内 —— 越界会画到相邻按钮上。
    /// (mt-ui 与 M 批边条各有一份同款体检,这里是标题栏这五张表的。)
    #[test]
    fn 标题栏图标的点全在单位方框内() {
        let mut points = 0usize;
        for shapes in [
            ICON_MINIMIZE,
            ICON_MAXIMIZE,
            ICON_RESTORE,
            ICON_CLOSE,
            ICON_LOGO,
            ICON_CHEVRON_DOWN,
        ] {
            for shape in shapes {
                let (pts, _) = shape.geom.points();
                for (x, y) in pts {
                    assert!(
                        (-0.001..=1.001).contains(&x) && (-0.001..=1.001).contains(&y),
                        "越界点 ({x}, {y})"
                    );
                    points += 1;
                }
            }
        }
        assert!(points > 30, "形状表看起来没被遍历到(只有 {points} 个点)");
    }

    /// 形状表拍平成点列。`&'static [Shape]` 是 const,用 `ptr::eq` 比较不可靠
    /// (每个使用点都可能各自内联出一份),按几何数据比才是真的。
    fn dump(shapes: &[Shape]) -> Vec<Vec<(f32, f32)>> {
        shapes.iter().map(|s| s.geom.points().0).collect()
    }

    /// 最大化键两态:图形与 tooltip 必须一起换,只换一个就会出现
    /// 「画着还原、写着最大化」。
    #[test]
    fn 最大化键两态一起换() {
        let (shapes, tip) = max_button_face(false);
        assert_eq!(dump(shapes), dump(ICON_MAXIMIZE));
        assert_eq!(tip, "titleBar.maximize");

        let (shapes, tip) = max_button_face(true);
        assert_eq!(dump(shapes), dump(ICON_RESTORE));
        assert_eq!(tip, "titleBar.restore");

        // 还原态是「两笔」:后层只画露出来的两条边(4 个顶点的开放折线),
        // 画成完整方框会糊成一团;最大化态是单独一个方框。
        assert_eq!(ICON_RESTORE.len(), 2);
        assert_eq!(ICON_MAXIMIZE.len(), 1);
        assert_eq!(dump(ICON_RESTORE)[1].len(), 5);
        assert_ne!(dump(ICON_MAXIMIZE), dump(ICON_RESTORE));
    }

    #[test]
    fn 关窗风险只预览前五项并汇总剩余数量() {
        let items = [
            "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel",
        ]
        .map(str::to_string)
        .to_vec();
        let preview = close_risk_preview(&items);
        for name in &items[..CLOSE_RISK_PREVIEW_LIMIT] {
            assert!(preview.contains(name), "{preview}");
        }
        assert!(!preview.contains("foxtrot"), "{preview}");
        assert_eq!(preview.lines().count(), CLOSE_RISK_PREVIEW_LIMIT + 1);
        assert!(preview.contains('3'), "剩余数量未显示:{preview}");
    }

    /// 三键的 tooltip key 都在字典里(拼错的后果是空 tooltip,真机上很难发现)。
    #[test]
    fn 窗口控制键文案_key_齐全() {
        let mut keys = vec!["titleBar.minimize", "titleBar.close"];
        keys.push(max_button_face(false).1);
        keys.push(max_button_face(true).1);
        for key in keys {
            for locale in mt_i18n::Locale::ALL {
                assert!(
                    mt_i18n::lookup(locale, "app", key).is_some(),
                    "字典缺条目 app.{key}({locale})"
                );
            }
        }
    }

    /// 闪烁相位:两端是 0(全亮全尺寸)、正中是 1(最暗最小),且全程落在 0..1。
    #[test]
    fn 闪烁相位在中点折返() {
        assert!(blink_phase(0.0).abs() < 1e-6);
        assert!((blink_phase(1.0)).abs() < 1e-6);
        assert!((blink_phase(0.5) - 1.0).abs() < 1e-6);
        // 单调:前半段升,后半段降
        assert!(blink_phase(0.25) < blink_phase(0.4));
        assert!(blink_phase(0.6) > blink_phase(0.75));
        for i in 0..=100 {
            let v = blink_phase(i as f32 / 100.0);
            assert!((0.0..=1.0).contains(&v), "相位越界 {v}");
        }
    }
}
