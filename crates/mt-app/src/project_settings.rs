//! Per-root sidebar visibility form over the shared, unfiltered catalog.

use gpui::{
    App, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Window,
    div, prelude::FluentBuilder as _, px,
};
use mt_config::{HiddenWorktree, WorktreeVisibilityLocation};

use crate::i18n::t;
use crate::prompt::{close_guarded, kind, open_guarded_with_close};
use crate::store::AppStore;
use crate::ui;
use crate::worktree_catalog::{ProjectWorktreeGroup, WorktreeCatalog};
use crate::worktree_visibility::{ProjectSettingsTarget, VisibilityDraft, is_invalid, visibility_keys};

const ROW_HEIGHT: f32 = 54.0;

fn scroll_top_for_cursor(cursor: usize, visible_top: f32, list_height: f32) -> f32 {
    let top = cursor as f32 * ROW_HEIGHT;
    if top < visible_top {
        top
    } else if top + ROW_HEIGHT > visible_top + list_height {
        (top + ROW_HEIGHT - list_height).max(0.0)
    } else {
        visible_top
    }
}

#[derive(Clone)]
struct SettingsRow {
    keys: Vec<HiddenWorktree>,
    saved_only: bool,
    label: String,
    path: String,
    state: &'static str,
    editable: bool,
    invalid: bool,
}

impl SettingsRow {
    fn checked(&self, draft: &VisibilityDraft) -> bool {
        !self.invalid && self.keys.iter().all(|key| draft.visible(key))
    }

    fn toggle(&self, draft: &mut VisibilityDraft) {
        if self.editable {
            if self.saved_only {
                if let Some(key) = self.keys.first() {
                    draft.set_visible(key.clone(), !self.checked(draft));
                }
            } else {
                draft.set_row_visible(&self.keys, !self.checked(draft));
            }
        }
    }
}

fn settings_rows(
    group: &ProjectWorktreeGroup,
    target: &ProjectSettingsTarget,
    hidden: &[HiddenWorktree],
) -> Vec<SettingsRow> {
    let mut rows = group.rows.iter().map(|row| {
        let keys = visibility_keys(row).filter(|key| {
            Some(&key.source) == target.source.as_ref()
        }).cloned().collect::<Vec<_>>();
        let invalid = is_invalid(row);
        let state = if row.is_prunable {
            "settings.prunable"
        } else if invalid {
            "settings.missing"
        } else if keys.is_empty() || row.visibility_key.is_none() {
            "settings.unresolved"
        } else if row.last_known {
            "settings.lastKnown"
        } else if row.is_bare {
            "settings.bare"
        } else if row.is_locked {
            "settings.locked"
        } else if row.is_detached {
            "settings.detached"
        } else {
            "settings.available"
        };
        SettingsRow {
            saved_only: false,
            editable: !keys.is_empty() && !invalid,
            invalid,
            keys,
            label: row.label.clone(),
            path: row.target.execution_path.clone(),
            state,
        }
    }).collect::<Vec<_>>();
    // Saved exclusions remain recoverable even when their rows are offline or
    // absent from Git. No new identity is inferred from the unavailable row.
    for key in hidden {
        if Some(&key.source) == target.source.as_ref()
            && !rows.iter().any(|row| row.keys.contains(key))
        {
            rows.push(SettingsRow {
                keys: vec![key.clone()],
                saved_only: true,
                label: t("worktree", "settings.savedWorktree").into(),
                path: key.location.path().to_string(),
                state: "settings.notInInventory",
                editable: true,
                invalid: false,
            });
        }
    }
    rows
}

struct ProjectSettings {
    store: Entity<AppStore>,
    catalog: Entity<WorktreeCatalog>,
    target: ProjectSettingsTarget,
    initial_group: ProjectWorktreeGroup,
    initial_hidden: Vec<HiddenWorktree>,
    draft: VisibilityDraft,
    error: Option<&'static str>,
    focus: FocusHandle,
    cancel_focus: FocusHandle,
    save_focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    scroll: ScrollHandle,
    cursor: usize,
    list_height: f32,
}

pub fn open(
    store: Entity<AppStore>,
    catalog: Entity<WorktreeCatalog>,
    target: ProjectSettingsTarget,
    window: &mut Window,
    cx: &mut App,
) {
    if crate::prompt::is_open(kind::PROJECT_SETTINGS) {
        return;
    }
    if !target.is_current(store.read(cx)) {
        crate::prompt::show_alert(
            t("worktree", "settings.title"),
            t("worktree", "settings.staleSource"),
            window,
            cx,
        );
        return;
    }
    let Some(group) = catalog.read(cx).groups(cx).into_iter()
        .find(|group| group.root_project_id == target.root_project_id)
    else {
        return;
    };
    let hidden = store.read(cx).project(&target.root_project_id)
        .map(|project| project.hidden_worktrees.clone()).unwrap_or_default();
    let previous_focus = window.focused(cx);
    let restore_focus = previous_focus.clone();
    let panel = cx.new(|cx| {
        cx.observe(&catalog, |_, _, cx| cx.notify()).detach();
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        ProjectSettings {
            store,
            catalog,
            target,
            initial_group: group,
            draft: VisibilityDraft::new(&hidden),
            initial_hidden: hidden,
            error: None,
            focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            save_focus: cx.focus_handle(),
            previous_focus,
            scroll: ScrollHandle::new(),
            cursor: 0,
            list_height: 300.0,
        }
    });
    let focus = panel.read(cx).focus.clone();
    open_guarded_with_close(
        kind::PROJECT_SETTINGS,
        window,
        cx,
        move |dialog, window, _cx| {
            let viewport = window.viewport_size();
            dialog.p_0()
                .w(px(680.0).min(viewport.width * 0.96))
                .margin_top(viewport.height * 0.10)
                .overlay_closable(false)
                .on_ok(|_, _, _| false)
                .child(div().w_full().h(px(600.0).min(viewport.height * 0.80)).child(panel.clone()))
        },
        move |window, _cx| {
            if let Some(focus) = restore_focus.as_ref() {
                window.focus(focus);
            }
        },
    );
    window.defer(cx, move |window, _cx| window.focus(&focus));
}

impl ProjectSettings {
    fn rows(&self, cx: &App) -> Vec<SettingsRow> {
        let group = self.catalog.read(cx).groups(cx).into_iter()
            .find(|group| group.root_project_id == self.target.root_project_id);
        let mut rows = settings_rows(group.as_ref().unwrap_or(&self.initial_group), &self.target, &self.initial_hidden);
        for row in &mut rows {
            if row.saved_only {
                continue;
            }
            row.editable &= row.keys.iter().all(|key| match &key.location {
                WorktreeVisibilityLocation::ConfiguredProject { configured_project_id, .. } => {
                    self.store.read(cx).configured_project_visibility_key(&self.target.root_project_id, configured_project_id)
                        .as_ref() == Some(key)
                }
                WorktreeVisibilityLocation::CanonicalWorktree { .. } => true,
            });
        }
        rows
    }

    fn close(&self, window: &mut Window, cx: &mut App) {
        if close_guarded(kind::PROJECT_SETTINGS, window, cx)
            && let Some(focus) = self.previous_focus.as_ref()
        {
            window.focus(focus);
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.store.update(cx, |store, cx| {
            store.set_project_worktree_visibility(&self.target, &self.draft, cx)
        });
        match result {
            Ok(()) => self.close(window, cx),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    fn toggle(&mut self, row: &SettingsRow, cx: &mut Context<Self>) {
        if row.editable && self.target.is_current(self.store.read(cx)) {
            row.toggle(&mut self.draft);
            cx.notify();
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        if rows.is_empty() {
            return;
        }
        match event.keystroke.key.as_str() {
            "up" => self.cursor = self.cursor.saturating_sub(1),
            "down" => self.cursor = (self.cursor + 1).min(rows.len() - 1),
            "home" => self.cursor = 0,
            "end" => self.cursor = rows.len() - 1,
            "space" | "enter" => {
                if let Some(row) = rows.get(self.cursor) {
                    self.toggle(row, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        let offset = self.scroll.offset();
        let visible_top = -f32::from(offset.y);
        let next_top = scroll_top_for_cursor(self.cursor, visible_top, self.list_height);
        self.scroll.set_offset(gpui::point(offset.x, px(-next_top.max(0.0))));
        window.focus(&self.focus);
        cx.notify();
    }
}

impl Render for ProjectSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows(cx);
        let stale = !self.target.is_current(self.store.read(cx));
        let error = self.error.or(stale.then_some("settings.staleSource"));
        self.cursor = self.cursor.min(rows.len().saturating_sub(1));
        self.list_height = (f32::from(px(600.0).min(window.viewport_size().height * 0.80)) - 170.0).max(0.0);
        let mut list = div().id("project-settings-list")
            .flex_1().min_h(px(0.0)).overflow_y_scroll().track_scroll(&self.scroll)
            .track_focus(&self.focus).tab_index(0)
            .on_key_down(cx.listener(Self::key_down));
        for (index, row) in rows.into_iter().enumerate() {
            let checked = row.checked(&self.draft);
            let editable = row.editable && !stale;
            let selected = self.cursor == index && self.focus.is_focused(window);
            let detail = format!("{} | {}", self.initial_group.host_label, row.path);
            list = list.child(
                div().id(SharedString::from(format!("project-settings-row-{index}")))
                    .h(px(ROW_HEIGHT)).flex_none().flex().items_center().gap(px(12.0)).px(px(20.0))
                    .when(selected, |el| el.bg(ui::accent_subtle()))
                    .when(editable, |el| el.cursor_pointer().hover(|el| el.bg(ui::border_subtle())))
                    .when(!editable, |el| el.opacity(0.55))
                    .tooltip(move |window, cx| mt_ui::tooltip::Tooltip::new(detail.clone()).build(window, cx))
                    .child(ui::checkbox(SharedString::from(format!("project-settings-check-{index}")), checked))
                    .child(div().flex_1().min_w(px(0.0)).flex().flex_col().gap(px(3.0))
                        .child(div().truncate().text_size(ui::font_px(12.0)).child(row.label.clone()))
                        .child(div().truncate().text_size(ui::font_px(10.0)).text_color(ui::text_muted()).child(row.path.clone())))
                    .child(div().w(px(100.0)).flex_none().truncate().text_size(ui::font_px(10.0))
                        .text_color(ui::text_muted()).child(t("worktree", row.state)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.cursor = index;
                        window.focus(&this.focus);
                        this.toggle(&row, cx);
                        cx.notify();
                    })),
            );
        }
        div().h_full().w_full().flex().flex_col().overflow_hidden()
            .text_color(ui::text_primary())
            .child(div().h(px(96.0)).flex_none().px(px(20.0)).py(px(12.0)).flex().flex_col().gap(px(4.0))
                .child(div().text_size(ui::font_px(16.0)).child(t("worktree", "settings.title")))
                .child(div().truncate().text_size(ui::font_px(12.0)).child(self.initial_group.root_project_name.clone()))
                .child(div().truncate().text_size(ui::font_px(10.0)).text_color(ui::text_muted())
                    .child(format!("{} | {}", self.initial_group.host_label, self.initial_group.root_project_path))))
            .child(list)
            .child(div().h(px(26.0)).flex_none().px(px(20.0)).truncate()
                .text_size(ui::font_px(10.0)).text_color(ui::color_error())
                .when_some(error, |el, error| el.child(t("worktree", error))))
            .child(div().h(px(48.0)).flex_none().flex().items_center().justify_end().gap(px(8.0))
                .px(px(20.0)).border_t_1().border_color(ui::border_subtle())
                .child(ui::ghost_button("project-settings-cancel", t("worktree", "cancel"))
                    .track_focus(&self.cancel_focus).tab_index(0)
                    .on_click(cx.listener(|this, _, window, cx| this.close(window, cx)))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            cx.stop_propagation();
                            this.close(window, cx);
                        }
                    })))
                .child(ui::primary_button("project-settings-save", t("worktree", "settings.save"))
                    .track_focus(&self.save_focus).tab_index(0)
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx)))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            cx.stop_propagation();
                            this.save(window, cx);
                        }
                    }))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree_catalog::CatalogBackend;
    use crate::worktree_visibility::{
        configured_preference_key, merge_edits, preference_key, sidebar_visible,
        tests::{row, source},
    };
    use mt_project::worktree::WorktreePathState;

    fn group() -> ProjectWorktreeGroup {
        ProjectWorktreeGroup {
            root_project_id: "root".into(),
            root_project_name: "Project".into(),
            root_project_path: "/repo".into(),
            execution_host_id: Some(source().execution_host_id),
            host_label: "Local machine".into(),
            backend: CatalogBackend::Local,
            warning: None,
            refreshing: false,
            rows: vec![row("/repo"), row("/feature")],
        }
    }

    fn target() -> ProjectSettingsTarget {
        ProjectSettingsTarget {
            root_project_id: "root".into(),
            root_config_key: "root-config".into(),
            source: Some(source()),
        }
    }

    #[test]
    fn keyboard_cursor_keeps_long_lists_in_view_without_resizing_rows() {
        assert_eq!(scroll_top_for_cursor(0, 0.0, 270.0), 0.0);
        assert_eq!(scroll_top_for_cursor(4, 0.0, 270.0), 0.0);
        assert_eq!(scroll_top_for_cursor(5, 0.0, 270.0), 54.0);
        assert_eq!(scroll_top_for_cursor(99, 0.0, 270.0), 5130.0);
        assert_eq!(scroll_top_for_cursor(0, 5130.0, 270.0), 0.0);
    }

    #[test]
    fn invalid_rows_are_unchecked_disabled_and_cannot_override_visibility() {
        let mut group = group();
        group.rows[0].is_prunable = true;
        group.rows[0].configured_visibility_key = configured_preference_key(&source(), "root", "/repo");
        group.rows[1].path_state = WorktreePathState::Missing;
        let mut draft = VisibilityDraft::new(&[]);
        for row in settings_rows(&group, &target(), &[]) {
            assert!(row.invalid);
            assert!(!row.editable);
            assert!(!row.checked(&draft));
            row.toggle(&mut draft);
        }
        assert!(draft.edits().is_empty());
        assert!(group.rows.iter().all(|row| !sidebar_visible(row, &[])));
        // Recovery reveals the prior manual choice, not the disabled checkbox.
        let hidden = vec![group.rows[1].visibility_key.clone().unwrap()];
        group.rows[0].is_prunable = false;
        group.rows[1].path_state = WorktreePathState::Present;
        let rows = settings_rows(&group, &target(), &hidden);
        let draft = VisibilityDraft::new(&hidden);
        assert!(rows[0].editable && rows[0].checked(&draft));
        assert!(rows[1].editable && !rows[1].checked(&draft));
    }

    #[test]
    fn all_hidden_rows_and_absent_saved_choices_remain_recoverable() {
        let group = group();
        let original = group.clone();
        let mut hidden = group.rows.iter()
            .map(|row| row.visibility_key.clone().unwrap()).collect::<Vec<_>>();
        hidden.push(preference_key(&source(), "/absent").unwrap());
        let rows = settings_rows(&group, &target(), &hidden);
        let mut draft = VisibilityDraft::new(&hidden);
        assert_eq!(rows.len(), 3);
        assert!(group.rows.iter().all(|row| !sidebar_visible(row, &hidden)));
        for row in rows {
            assert!(row.editable);
            assert!(!row.checked(&draft));
            row.toggle(&mut draft);
            assert!(row.checked(&draft));
        }
        assert_eq!(group, original);
    }

    #[test]
    fn fresh_rows_default_checked_and_unresolved_rows_cannot_persist_ui_keys() {
        let mut group = group();
        let hidden = vec![group.rows[0].visibility_key.clone().unwrap()];
        let draft = VisibilityDraft::new(&hidden);
        group.rows.push(row("/later"));
        group.rows[1].visibility_key = None;
        let rows = settings_rows(&group, &target(), &hidden);
        assert!(!rows[0].checked(&draft));
        assert!(!rows[1].editable);
        assert!(rows[1].keys.is_empty());
        assert!(rows[2].editable && rows[2].checked(&draft));
        let mut other_source = target();
        other_source.source.as_mut().unwrap().root_path = "/other-root".into();
        assert!(settings_rows(&group, &other_source, &hidden).iter().all(|row| !row.editable));
    }

    #[test]
    fn configured_alias_checkbox_is_editable_without_canonical_authority() {
        let mut group = group();
        group.rows.truncate(1);
        group.rows[0].visibility_key = None;
        group.rows[0].configured_visibility_key = configured_preference_key(&source(), "root", "/repo");
        group.rows[0].authoritative = false;
        group.rows[0].path_state = WorktreePathState::Unknown;
        let row = settings_rows(&group, &target(), &[]).remove(0);
        let mut draft = VisibilityDraft::new(&[]);
        assert!(row.editable && row.checked(&draft));
        assert_eq!(row.state, "settings.unresolved");
        row.toggle(&mut draft);
        assert!(!row.checked(&draft));
        let mut hidden = Vec::new();
        assert!(merge_edits(&mut hidden, draft.edits()));
        assert!(!sidebar_visible(&group.rows[0], &hidden));
        assert!(!group.rows[0].authoritative);
    }

    #[test]
    fn saved_only_checkbox_removes_a_preference_without_capturing_a_live_target() {
        let group = group();
        let saved = configured_preference_key(&source(), "removed", "/old-path").unwrap();
        let hidden = vec![saved.clone()];
        let row = settings_rows(&group, &target(), &hidden).pop().unwrap();
        let mut draft = VisibilityDraft::new(&hidden);
        assert!(row.saved_only && row.editable);
        assert!(!row.checked(&draft));
        row.toggle(&mut draft);
        assert_eq!(draft.edits().get(&saved), Some(&true));
        assert!(draft.configured_targets().is_empty());
        row.toggle(&mut draft);
        assert!(draft.edits().is_empty());
    }

    #[test]
    fn resolved_checkbox_unhides_both_exact_keys_and_preserves_absent_exclusions() {
        let mut group = group();
        group.rows.truncate(1);
        let canonical = group.rows[0].visibility_key.clone().unwrap();
        let configured = configured_preference_key(&source(), "root", "/repo-link").unwrap();
        group.rows[0].configured_visibility_key = Some(configured.clone());
        let absent = configured_preference_key(&source(), "removed", "/removed").unwrap();
        for original in [vec![configured.clone()], vec![canonical, configured]] {
            let mut hidden = original;
            hidden.push(absent.clone());
            let row = settings_rows(&group, &target(), &hidden).remove(0);
            let mut draft = VisibilityDraft::new(&hidden);
            assert!(!row.checked(&draft));
            assert!(!sidebar_visible(&group.rows[0], &hidden));
            row.toggle(&mut draft);
            assert!(row.checked(&draft));
            assert_eq!(draft.edits().len(), 2);
            // Save wins for this edited row even when its second exclusion was
            // added after the dialog opened; unrelated saved choices survive.
            if !hidden.contains(&row.keys[0]) {
                hidden.push(row.keys[0].clone());
            }
            assert!(merge_edits(&mut hidden, draft.edits()));
            assert_eq!(hidden, vec![absent.clone()]);
            assert!(sidebar_visible(&group.rows[0], &hidden));

            // Reverting the checkbox restores the original individual keys,
            // rather than introducing another exclusion for the same row.
            row.toggle(&mut draft);
            assert!(draft.edits().is_empty());
            assert!(draft.configured_targets().is_empty());
        }
    }
}
