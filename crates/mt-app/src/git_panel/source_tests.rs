use super::*;

use crate::execution_host::{ExecutionBackend, ExecutionBackendSignature};
use mt_identity::{ExecutionHostId, HostInstallId, RepoId};
use mt_project::git::cli::{GitRef, HeadState, ObjectId, RepositoryAuthority};

pub(crate) fn snapshot(backend: ExecutionBackend) -> ProjectExecutionSnapshot {
    let host = ExecutionHostId::derive("git-ui-fixture", &HostInstallId::new());
    let repo = RepoId::derive(&host, "/repo/.git");
    ProjectExecutionSnapshot {
        project_id: "project".into(),
        root_project_id: "root".into(),
        worktree_id: WorktreeId::derive(&repo, "/repo", None),
        execution_host_id: host,
        canonical_path: "/repo".into(),
        root_source_path: "/repo".into(),
        backend,
        host_label: "fixture".into(),
    }
}

pub(crate) fn ssh_snapshot(epoch: Option<u64>) -> ProjectExecutionSnapshot {
    snapshot(ExecutionBackend::Ssh {
        connection: mt_config::SshConnection {
            id: "fixture".into(),
            name: "fixture".into(),
            host: "unused.invalid".into(),
            port: 22,
            user: "fixture".into(),
            password: None,
            identity_file: None,
            group: None,
        },
        connection_fingerprint: 7,
        connection_epoch: epoch,
    })
}

pub(crate) fn scope(snapshot: &ProjectExecutionSnapshot, generation: u64) -> GitScope {
    GitScope::new(Some(snapshot.worktree_id.clone()), generation, true)
        .with_source(snapshot.project_id.clone(), snapshot.source_signature())
}

#[test]
fn readiness_accepts_only_the_first_unobserved_epoch() {
    let captured = ssh_snapshot(None);
    let mut observed = captured.clone();
    if let ExecutionBackend::Ssh { connection_epoch, .. } = &mut observed.backend {
        *connection_epoch = Some(11);
    }
    assert!(host_ui::read_source_matches(&captured, &observed));
    assert!(!host_ui::read_source_matches(&observed, &captured));
    let mut replacement = observed.clone();
    if let ExecutionBackend::Ssh { connection_epoch, .. } = &mut replacement.backend {
        *connection_epoch = Some(12);
    }
    assert!(!host_ui::read_source_matches(&observed, &replacement));
}

#[test]
fn readiness_keeps_project_host_path_and_fingerprint_authority() {
    let original = ssh_snapshot(None);
    let mut changed = original.clone();
    changed.project_id = "other-project".into();
    assert!(!host_ui::read_source_matches(&original, &changed));
    changed = original.clone();
    changed.root_project_id = "other-root".into();
    assert!(!host_ui::read_source_matches(&original, &changed));
    changed = original.clone();
    changed.canonical_path = "/other".into();
    assert!(!host_ui::read_source_matches(&original, &changed));
    changed = original.clone();
    changed.root_source_path = "/other-root".into();
    assert!(!host_ui::read_source_matches(&original, &changed));
    changed = original.clone();
    if let ExecutionBackend::Ssh { connection_fingerprint, .. } = &mut changed.backend {
        *connection_fingerprint += 1;
    }
    assert!(!host_ui::read_source_matches(&original, &changed));
    changed = original.clone();
    changed.backend = ExecutionBackend::Local;
    assert!(!host_ui::read_source_matches(&original, &changed));
}

#[test]
fn same_path_worktree_cache_rejects_other_sources_and_aliases() {
    let source = ssh_snapshot(Some(11));
    let original = scope(&source, 1);
    assert!(original.same_cache_identity(&scope(&source, 3)));
    let mut changed = source.clone();
    changed.backend = ExecutionBackend::Local;
    assert!(!original.same_cache_identity(&scope(&changed, 3)));
    changed = source.clone();
    changed.project_id = "another-alias".into();
    assert!(!original.same_cache_identity(&scope(&changed, 3)));
    changed = source.clone();
    if let ExecutionBackend::Ssh { connection_epoch, .. } = &mut changed.backend {
        *connection_epoch = Some(12);
    }
    assert!(!original.same_cache_identity(&scope(&changed, 3)));
}

#[test]
fn sync_owner_rejects_old_operation_repo_authority_and_a_b_a_generation() {
    let source = snapshot(ExecutionBackend::Local);
    let owner = GitSyncOwner {
        scope: scope(&source, 1),
        repo_path: "/repo".into(),
        authority: Some(RepositoryAuthority {
            worktree_root: "/repo".into(),
            git_dir: "/repo/.git".into(),
            common_dir: "/repo/.git".into(),
        }),
        request: 7,
    };
    let mut newer = owner.clone();
    newer.request = 8;
    assert!(!sync_owner_matches(&owner, &newer));
    newer = owner.clone();
    newer.scope.generation = 3;
    assert!(!sync_owner_matches(&owner, &newer));
    newer = owner.clone();
    newer.authority.as_mut().unwrap().common_dir = "/replacement/.git".into();
    assert!(!sync_owner_matches(&owner, &newer));
    newer = owner.clone();
    newer.scope.source.as_mut().unwrap().backend = ExecutionBackendSignature::Wsl {
        distro: "fixture".into(),
    };
    assert!(!sync_owner_matches(&owner, &newer));
}

#[test]
fn detached_completion_refreshes_without_resetting_a_returned_history_view() {
    let source = snapshot(ExecutionBackend::Local);
    let captured = scope(&source, 1);
    assert!(reconciliation_resets_history(&captured, &captured, "/repo", "/repo"));
    let returned = scope(&source, 3);
    assert!(captured.same_cache_identity(&returned));
    assert!(!reconciliation_resets_history(&captured, &returned, "/repo", "/repo"));
    assert!(!reconciliation_resets_history(&captured, &captured, "/repo", "/repo/nested"));
}

#[test]
fn rediscovery_at_the_same_path_revokes_replaced_repository_authority() {
    let original = RepositoryAuthority {
        worktree_root: "/repo".into(),
        git_dir: "/repo/.git".into(),
        common_dir: "/repo/.git".into(),
    };
    assert!(!repository_selection_disposed("/repo", true, Some(&original), Some(&original)));
    let mut replacement = original.clone();
    replacement.common_dir = "/another/.git".into();
    assert!(repository_selection_disposed("/repo", true, Some(&original), Some(&replacement)));
    replacement = original.clone();
    replacement.git_dir = "/repo/.git/replacement".into();
    assert!(repository_selection_disposed("/repo", true, Some(&original), Some(&replacement)));
    assert!(repository_selection_disposed("/repo", false, Some(&original), None));
    assert!(!repository_selection_disposed("", false, None, None));
}

#[test]
fn repository_terminal_cwd_never_reinterprets_posix_names_as_wsl_separators() {
    let wsl = ExecutionBackend::Wsl { distro: "Ubuntu".into() };
    assert_eq!(repository_terminal_cwd(&wsl, "/repo with spaces/nested").unwrap(), r"\\wsl.localhost\Ubuntu\repo with spaces\nested");
    assert!(repository_terminal_cwd(&wsl, r"/repo\literal/nested").is_err());
    assert!(repository_terminal_cwd(&wsl, "/repo:literal/nested").is_err());
    let ssh = ssh_snapshot(Some(7));
    assert_eq!(repository_terminal_cwd(&ssh.backend, "/repo\\literal:exact").unwrap(), "/repo\\literal:exact");
    assert_eq!(repository_terminal_cwd(&ExecutionBackend::Local, r"C:\repo\native").unwrap(), r"C:\repo\native");
}

#[test]
fn status_preserves_detached_and_unborn_head_labels() {
    let oid = ObjectId::parse(&"a".repeat(40)).unwrap();
    assert_eq!(head_label(&HeadState { oid: Some(oid), branch: None }).as_deref(), Some("(aaaaaaa)"));
    assert_eq!(head_label(&HeadState { oid: None, branch: Some(GitRef::local("main").unwrap()) }).as_deref(), Some("main"));
    assert_eq!(head_label(&HeadState { oid: None, branch: None }), None);
}

#[test]
fn explicit_draft_recovery_allows_only_epoch_changes_in_the_same_source() {
    let source = ssh_snapshot(Some(7));
    let original = scope(&source, 1);
    let mut changed = source.clone();
    if let ExecutionBackend::Ssh { connection_epoch, .. } = &mut changed.backend {
        *connection_epoch = Some(8);
    }
    let reconnected = scope(&changed, 3);
    assert!(!original.same_cache_identity(&reconnected));
    assert!(original.same_draft_context(&reconnected));
    if let ExecutionBackend::Ssh { connection_fingerprint, .. } = &mut changed.backend {
        *connection_fingerprint += 1;
    }
    assert!(!original.same_draft_context(&scope(&changed, 3)));
}

#[test]
fn explicit_draft_recovery_never_crosses_projects_or_worktrees() {
    let source = ssh_snapshot(Some(7));
    let original = scope(&source, 1);
    let mut changed = source.clone();
    changed.project_id = "other-project".into();
    assert!(!original.same_draft_context(&scope(&changed, 3)));
    changed = source.clone();
    let repo = RepoId::derive(&source.execution_host_id, "/another/.git");
    changed.worktree_id = WorktreeId::derive(&repo, "/repo", None);
    assert!(!original.same_draft_context(&scope(&changed, 3)));
}
