//! Modal 批:重命名 / 移除项目确认。
//!
//! 全部走 [`gpui_component::dialog::Dialog`] + [`gpui_component::input`],
//! 因此窗口的根视图必须是 `gpui_component::Root`(见 `main.rs`)—— Input 内部会
//! `Root::update` 登记当前焦点输入框,不是 Root 会直接 panic。
//!
//! # 与旧版的对照
//!
//! | 旧版 | 这里 |
//! |---|---|
//! | tab 右键菜单 → 重命名 | [`open_rename_pane`] |
//! | 移除项目的确认框(收编 `project_list` 的「点两次」临时方案) | [`open_confirm_remove_project`] |
//!
//! # 状态放在哪
//!
//! Dialog 的 builder 是 `Fn`,**每帧都会被重新调用**,不能把编辑中的表单状态藏在
//! 闭包捕获的普通变量里。表单状态一律放进 `Entity`:gpui 会把渲染期间读过的
//! entity 记进窗口的失效表,`cx.notify()` 即触发重画(见 `App::notify`)。
//!
//! 设置面板(原来那个单页「终端配置」对话框)已经拆去 [`crate::settings`] ——
//! 它是两级侧栏 + 10 个分页的独立视图,与这里两个「只给标题和文案」的小弹窗不同形。

use std::cell::Cell;

use gpui::{
    App, AppContext, ClickEvent, Entity, Focusable as _, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::{Input, InputState, SelectAll};

use crate::i18n::t;
use crate::prompt::{autofocus, is_open, kind, open_guarded, show_alert};
use crate::store::AppStore;
use crate::ui;

// ─── 重命名 ───────────────────────────────────────────────────

/// 重命名一个终端 tab。留空 = 恢复默认(shell 名)。
///
/// **不落盘**:`SavedPane` 里没有 customTitle 字段,装机版同样只在运行时保留 ——
/// 磁盘格式一字不改是这次迁移的红线。
pub fn open_rename_pane(
    store: Entity<AppStore>,
    project_id: String,
    pane_id: String,
    current: String,
    window: &mut Window,
    cx: &mut App,
) {
    // 守卫要在**建输入框之前**判:被 `open_guarded` 拦下时弹窗压根没开,
    // 底下那句 `autofocus` 就会把焦点送进虚空(判据见 `prompt::is_open`)
    if is_open(kind::RENAME_PANE) {
        return;
    }
    // 原版这条走的是同一个 `showPrompt`(`paneActions.ts:310`,标题当默认值),
    // 于是也吃到那句 `if (defaultValue) input.select()`
    let select_all = Cell::new(!current.is_empty());
    let input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(t("fileTree", "prompt.renameMessage"))
            .default_value(current)
    });
    // 打开即可直接改名,不必先点一下输入框。聚焦必须排在 `open_guarded`
    // **之后**(弹窗一开就抢焦点),判据见 `prompt::autofocus`
    let input_for_focus = input.clone();

    open_guarded(kind::RENAME_PANE, window, cx, move |dialog, window, cx| {
        // 有默认值就全选。手法与时机见 `prompt::show_prompt` 里那段注释
        if select_all.take() {
            let focus = input.read(cx).focus_handle(cx);
            window.on_next_frame(move |window, cx| {
                focus.dispatch_action(&SelectAll, window, cx);
            });
        }
        let store = store.clone();
        let project_id = project_id.clone();
        let pane_id = pane_id.clone();
        let input_for_ok = input.clone();
        dialog
            .title(t("paneGroup", "renameTerminal"))
            // 与 `showPrompt` 同宽:原版这条就是走 `.prompt-dialog`(360px)
            .w(px(360.0))
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text(t("prompt", "confirm"))
                    .cancel_text(t("prompt", "cancel")),
            )
            .child(div().px(px(20.0)).child(Input::new(&input)))
            .on_ok(move |_: &ClickEvent, _window, cx| {
                let title = input_for_ok.read(cx).value().to_string();
                store.update(cx, |store, cx| {
                    store.rename_pane(&project_id, &pane_id, &title, cx)
                });
                true
            })
    });

    autofocus(&input_for_focus, window, cx);
}

// ─── 移除项目确认 ─────────────────────────────────────────────

/// 移除项目前的确认。
///
/// 收编 `project_list` 里那个「点两次才真删」的临时方案:移除是不可逆的
/// (配置里的布局、展开目录一起没),必须让用户看清楚删的是哪一个。
pub fn open_confirm_remove_project(
    store: Entity<AppStore>,
    project_id: String,
    project_name: String,
    project_path: String,
    window: &mut Window,
    cx: &mut App,
) {
    open_guarded(
        kind::REMOVE_PROJECT,
        window,
        cx,
        move |dialog, _window, _cx| {
            let store = store.clone();
            let project_id = project_id.clone();
            dialog
                .title(t("projectList", "removeConfirm.title"))
                // 原版 `ProjectList.tsx` 的删除确认是 `w-[320px]`
                .w(px(320.0))
                .confirm()
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(t("projectList", "removeConfirm.confirm"))
                        .cancel_text(t("projectList", "removeConfirm.cancel")),
                )
                .child(
                    div()
                        .px(px(20.0))
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        // 正文与原版一样是「前缀 + 项目名 + 后缀」三段拼(后缀那半句
                        // 已经把"只从列表移除、不删文件"说清楚了,不必另起一行)
                        .child(
                            div()
                                .text_size(ui::font_px(13.0))
                                .text_color(ui::text_primary())
                                .child(format!(
                                    "{}{}{}",
                                    t("projectList", "removeConfirm.messagePrefix"),
                                    project_name,
                                    t("projectList", "removeConfirm.messageSuffix"),
                                )),
                        )
                        .child(
                            div()
                                .text_size(ui::font_px(11.0))
                                .text_color(ui::text_muted())
                                .child(project_path.clone()),
                        ),
                )
                .on_ok(move |_: &ClickEvent, window, cx| {
                    if crate::workbench_area::project_has_dirty_documents(&project_id, cx) {
                        window.defer(cx, |window, cx| {
                            show_alert(
                                t("fileViewer", "unsavedTitle"),
                                t("fileViewer", "projectRemovalBlocked"),
                                window,
                                cx,
                            );
                        });
                        return true;
                    }
                    store.update(cx, |store, cx| store.remove_project(&project_id, cx));
                    true
                })
        },
    );
}
