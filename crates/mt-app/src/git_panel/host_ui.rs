use gpui::{App, ClickEvent, Entity, InteractiveElement, IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div, px};

use crate::execution_host::{ExecutionBackendSignature, ExecutionSourceSignature, ProjectExecutionSnapshot};
use crate::git_backend::{GitBackend, GitLifetime, GitRepository, GitWriteOutcome};
use crate::store::AppStore;
use crate::{prompt, ui};

pub(crate) fn active_snapshot(store: &Entity<AppStore>, cx: &App) -> Result<ProjectExecutionSnapshot, String> {
    let store = store.read(cx);
    let id = store.active_project_id.as_deref().ok_or("No project selected")?;
    store.project_execution_snapshot(id)
}

pub(crate) fn read_source_matches(captured: &ProjectExecutionSnapshot, observed: &ProjectExecutionSnapshot) -> bool {
    captured.project_id == observed.project_id
        && read_signature_matches(&captured.source_signature(), &observed.source_signature())
}

fn read_signature_matches(captured: &ExecutionSourceSignature, observed: &ExecutionSourceSignature) -> bool {
    match &captured.backend {
        ExecutionBackendSignature::Ssh { connection_epoch: None, .. } => {
            captured.with_connection_epoch(None) == observed.with_connection_epoch(None)
        }
        _ => captured == observed,
    }
}

pub(crate) fn snapshot_current(snapshot: &ProjectExecutionSnapshot, store: &Entity<AppStore>, cx: &App) -> bool {
    store.read(cx).project_execution_snapshot(&snapshot.project_id).is_ok_and(|current| {
        current.source_signature() == snapshot.source_signature()
    })
}

pub(crate) fn repository_current(repository: &GitRepository, store: &Entity<AppStore>, cx: &App) -> bool {
    let snapshot = repository.backend().snapshot();
    snapshot_current(snapshot, store, cx) && repository.matches_snapshot(snapshot)
}

pub(crate) fn active_repository(repository: &GitRepository, store: &Entity<AppStore>, cx: &App) -> bool {
    store.read(cx).active_project_id.as_deref() == Some(repository.backend().snapshot().project_id.as_str())
        && repository_current(repository, store, cx)
}

/// Blocking: a legacy path is only a selector inside its captured host's
/// discovery, never authority to probe the client filesystem.
pub(crate) fn resolve_repository(snapshot: ProjectExecutionSnapshot, path: &str, lifetime: GitLifetime) -> Result<GitRepository, String> {
    let backend = GitBackend::connect(snapshot.clone(), lifetime).map_err(|error| error.to_string())?;
    if !read_source_matches(&snapshot, backend.snapshot()) {
        return Err("Git source epoch changed; reopen this view".into());
    }
    backend.discover().map_err(|error| error.to_string())?.into_iter()
        .find(|repository| repository.authority().worktree_root == path)
        .ok_or_else(|| "The selected repository is no longer part of this project source".into())
}

pub(crate) fn outcome_error(outcome: &GitWriteOutcome) -> Option<String> {
    let mut errors = Vec::new();
    if outcome.lease_retained {
        errors.push("Git outcome uncertain. Writes remain locked until the original operation is confirmed stopped and reviewed.".to_string());
    }
    if let Some(error) = &outcome.error { errors.push(error.to_string()); }
    if let Some(error) = &outcome.reconciliation_error { errors.push(error.to_string()); }
    if errors.is_empty() && !outcome.succeeded() {
        errors.push("Git operation did not complete".into());
    }
    (!errors.is_empty()).then(|| errors.join("\n"))
}

/// Programmatic close does not call Dialog::on_close. Both close paths must
/// invalidate the same instance before removing it from the overlay stack.
pub(crate) fn dialog_title(kind: &'static str, title: impl Into<SharedString>, lifetime: GitLifetime) -> impl IntoElement {
    div().w_full().flex().items_center().justify_between().gap(px(12.0))
        .child(div().flex_1().truncate().child(title.into()))
        .child(div().id(SharedString::from(format!("{kind}-owned-close")))
            .w(px(20.0)).h(px(20.0)).flex_none().flex().items_center().justify_center()
            .cursor_pointer().text_size(ui::font_px(12.0)).child("\u{2715}")
            .tooltip(|window, cx| mt_ui::tooltip::Tooltip::new("Close").build(window, cx))
            .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                if lifetime.is_valid() && prompt::close_guarded(kind, window, cx) {
                    lifetime.invalidate();
                }
            }))
}
