use gpui::{App, Entity};

use crate::execution_host::{
    ExecutionBackendSignature, ExecutionSourceSignature, ProjectExecutionSnapshot,
};
use crate::git_backend::{GitBackend, GitLifetime, GitRepository, GitWriteOutcome};
use crate::store::AppStore;

pub(crate) fn active_snapshot(
    store: &Entity<AppStore>,
    cx: &App,
) -> Result<ProjectExecutionSnapshot, String> {
    let store = store.read(cx);
    let id = store
        .active_project_id
        .as_deref()
        .ok_or("No project selected")?;
    store.project_execution_snapshot(id)
}

pub(crate) fn read_source_matches(
    captured: &ProjectExecutionSnapshot,
    observed: &ProjectExecutionSnapshot,
) -> bool {
    captured.project_id == observed.project_id
        && read_signature_matches(&captured.source_signature(), &observed.source_signature())
}

fn read_signature_matches(
    captured: &ExecutionSourceSignature,
    observed: &ExecutionSourceSignature,
) -> bool {
    match &captured.backend {
        ExecutionBackendSignature::Ssh {
            connection_epoch: None,
            ..
        } => captured.with_connection_epoch(None) == observed.with_connection_epoch(None),
        _ => captured == observed,
    }
}

pub(crate) fn snapshot_current(
    snapshot: &ProjectExecutionSnapshot,
    store: &Entity<AppStore>,
    cx: &App,
) -> bool {
    store
        .read(cx)
        .project_execution_snapshot(&snapshot.project_id)
        .is_ok_and(|current| current.source_signature() == snapshot.source_signature())
}

pub(crate) fn repository_current(
    repository: &GitRepository,
    store: &Entity<AppStore>,
    cx: &App,
) -> bool {
    let snapshot = repository.backend().snapshot();
    snapshot_current(snapshot, store, cx) && repository.matches_snapshot(snapshot)
}

pub(crate) fn active_repository(
    repository: &GitRepository,
    store: &Entity<AppStore>,
    cx: &App,
) -> bool {
    store.read(cx).active_project_id.as_deref()
        == Some(repository.backend().snapshot().project_id.as_str())
        && repository_current(repository, store, cx)
}

/// Blocking: a legacy path is only a selector inside its captured host's
/// discovery, never authority to probe the client filesystem.
pub(crate) fn resolve_repository(
    snapshot: ProjectExecutionSnapshot,
    path: &str,
    lifetime: GitLifetime,
) -> Result<GitRepository, String> {
    let backend =
        GitBackend::connect(snapshot.clone(), lifetime).map_err(|error| error.to_string())?;
    if !read_source_matches(&snapshot, backend.snapshot()) {
        return Err("Git source epoch changed; reopen this view".into());
    }
    backend
        .discover()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|repository| repository.authority().worktree_root == path)
        .ok_or_else(|| "The selected repository is no longer part of this project source".into())
}

pub(crate) fn outcome_error(outcome: &GitWriteOutcome) -> Option<String> {
    let mut errors = Vec::new();
    if outcome.lease_retained {
        errors.push("Git outcome uncertain. Writes remain locked until the original operation is confirmed stopped and reviewed.".to_string());
    }
    if let Some(error) = &outcome.error {
        errors.push(error.to_string());
    }
    if let Some(error) = &outcome.reconciliation_error {
        errors.push(error.to_string());
    }
    if errors.is_empty() && !outcome.succeeded() {
        errors.push("Git operation did not complete".into());
    }
    (!errors.is_empty()).then(|| errors.join("\n"))
}
