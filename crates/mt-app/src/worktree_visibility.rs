//! Sidebar preferences are a projection, never catalog or workbench mutations.

use std::collections::{HashMap, HashSet};

use mt_config::{
    HiddenWorktree, WorktreeVisibilityBackend, WorktreeVisibilityLocation, WorktreeVisibilitySource,
};
use mt_project::worktree::WorktreePathState;

use crate::execution_host::{
    ExecutionBackend, ProjectExecutionSnapshot, configured_execution_path,
    normalize_absolute_posix_path,
};
use crate::store::AppStore;
use crate::worktree_catalog::{WorktreeCatalogRow, root_config_key};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectSettingsTarget {
    pub root_project_id: String,
    pub root_config_key: String,
    pub source: Option<WorktreeVisibilitySource>,
}

impl ProjectSettingsTarget {
    pub fn capture(store: &AppStore, root_project_id: &str) -> Option<Self> {
        let root = store.project(root_project_id)?;
        if root.parent_project_id.is_some() {
            return None;
        }
        Some(Self {
            root_project_id: root.id.clone(),
            root_config_key: root_config_key(root),
            source: store.worktree_visibility_source(root_project_id),
        })
    }

    pub fn is_current(&self, store: &AppStore) -> bool {
        Self::capture(store, &self.root_project_id).as_ref() == Some(self)
    }
}

pub fn source_from_snapshot(
    snapshot: &ProjectExecutionSnapshot,
    configured_path: &str,
) -> Option<WorktreeVisibilitySource> {
    let root_path = configured_execution_path(&snapshot.backend, configured_path).ok()?;
    let backend = match &snapshot.backend {
        ExecutionBackend::Local => WorktreeVisibilityBackend::Local,
        ExecutionBackend::Wsl { distro } => WorktreeVisibilityBackend::Wsl {
            distro: distro.to_lowercase(),
        },
        ExecutionBackend::Ssh { connection, .. } => WorktreeVisibilityBackend::Ssh {
            connection_id: connection.id.clone(),
            host: connection.host.clone(),
            port: if connection.port == 0 { 22 } else { connection.port },
            user: connection.user.clone(),
        },
    };
    Some(WorktreeVisibilitySource {
        execution_host_id: snapshot.execution_host_id.clone(),
        root_path: normalize_path(&backend, &root_path)?,
        backend,
    })
}

fn normalize_path(backend: &WorktreeVisibilityBackend, path: &str) -> Option<String> {
    if path.is_empty() || path.contains('\0') {
        return None;
    }
    match backend {
        WorktreeVisibilityBackend::Local => {
            Some(mt_project::worktree::normalize_path_for_comparison(path))
        }
        WorktreeVisibilityBackend::Wsl { .. } | WorktreeVisibilityBackend::Ssh { .. } => {
            normalize_absolute_posix_path(path).ok()
        }
    }
}

pub fn preference_key(
    source: &WorktreeVisibilitySource,
    canonical_path: &str,
) -> Option<HiddenWorktree> {
    Some(HiddenWorktree {
        source: source.clone(),
        location: WorktreeVisibilityLocation::CanonicalWorktree {
            canonical_path: normalize_path(&source.backend, canonical_path)?,
        },
    })
}

pub fn configured_preference_key(
    source: &WorktreeVisibilitySource,
    project_id: &str,
    configured_path: &str,
) -> Option<HiddenWorktree> {
    if project_id.is_empty() || project_id.contains('\0') {
        return None;
    }
    Some(HiddenWorktree {
        source: source.clone(),
        location: WorktreeVisibilityLocation::ConfiguredProject {
            configured_project_id: project_id.to_string(),
            configured_path: normalize_path(&source.backend, configured_path)?,
        },
    })
}

pub fn is_invalid(row: &WorktreeCatalogRow) -> bool {
    row.is_prunable || row.path_state == WorktreePathState::Missing
}

pub fn sidebar_visible(row: &WorktreeCatalogRow, hidden: &[HiddenWorktree]) -> bool {
    !is_invalid(row) && visibility_keys(row).all(|key| !hidden.contains(key))
}

pub fn visibility_keys(row: &WorktreeCatalogRow) -> impl Iterator<Item = &HiddenWorktree> {
    row.visibility_key.iter().chain(row.configured_visibility_key.iter())
}

#[derive(Clone, Default)]
pub struct VisibilityDraft {
    original_hidden: HashSet<HiddenWorktree>,
    edits: HashMap<HiddenWorktree, bool>,
    configured_targets: HashSet<HiddenWorktree>,
}

impl VisibilityDraft {
    pub fn new(hidden: &[HiddenWorktree]) -> Self {
        Self {
            original_hidden: hidden.iter().cloned().collect(),
            edits: HashMap::new(),
            configured_targets: HashSet::new(),
        }
    }

    pub fn visible(&self, key: &HiddenWorktree) -> bool {
        self.edits
            .get(key)
            .copied()
            .unwrap_or_else(|| !self.original_hidden.contains(key))
    }

    pub fn set_visible(&mut self, key: HiddenWorktree, visible: bool) {
        if visible != self.original_hidden.contains(&key) {
            self.edits.remove(&key);
        } else {
            self.edits.insert(key, visible);
        }
    }

    pub fn edits(&self) -> &HashMap<HiddenWorktree, bool> {
        &self.edits
    }

    pub fn configured_targets(&self) -> &HashSet<HiddenWorktree> {
        &self.configured_targets
    }

    pub fn set_row_visible(&mut self, keys: &[HiddenWorktree], visible: bool) {
        let originally_visible = keys.iter().all(|key| !self.original_hidden.contains(key));
        if visible == originally_visible {
            for key in keys {
                self.edits.remove(key);
            }
        } else if visible {
            for key in keys {
                self.edits.insert(key.clone(), true);
            }
        } else if let Some(key) = keys.first() {
            self.set_visible(key.clone(), false);
        }

        // Canonical edits made through a configured row still belong to the
        // captured project/path, even though only the canonical key is written.
        let edited = keys.iter().any(|key| self.edits.contains_key(key));
        for key in keys {
            if matches!(key.location, WorktreeVisibilityLocation::ConfiguredProject { .. }) {
                if edited {
                    self.configured_targets.insert(key.clone());
                } else {
                    self.configured_targets.remove(key);
                }
            }
        }
    }
}

/// Merge only changed identities, preserving removed/offline rows and other edits.
pub fn merge_edits(
    current: &mut Vec<HiddenWorktree>,
    edits: &HashMap<HiddenWorktree, bool>,
) -> bool {
    let previous = current.clone();
    current.retain(|key| edits.get(key) != Some(&true));
    let mut additions = edits
        .iter()
        .filter(|(key, visible)| !**visible && !current.contains(key))
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    additions.sort_by(|a, b| a.location.cmp(&b.location));
    current.extend(additions);
    *current != previous
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::worktree_catalog::{CatalogBackend, WorktreeCatalogTarget};
    use mt_identity::{ExecutionHostId, HostInstallId, RepoId, WorktreeId};

    pub(crate) fn source() -> WorktreeVisibilitySource {
        let install: HostInstallId = "install-v1:123e4567-e89b-42d3-a456-426614174000".parse().unwrap();
        WorktreeVisibilitySource {
            execution_host_id: ExecutionHostId::derive("visibility", &install),
            root_path: "/repo".into(),
            backend: WorktreeVisibilityBackend::Local,
        }
    }

    pub(crate) fn row(path: &str) -> WorktreeCatalogRow {
        WorktreeCatalogRow {
            target: WorktreeCatalogTarget {
                root_project_id: "root".into(),
                row_key: path.into(),
                root_config_key: "root-config".into(),
                configured_project_id: None,
                host_visible_path: path.into(),
                execution_path: path.into(),
                suggested_name: "feature".into(),
                backend: CatalogBackend::Local,
                owner: None,
            },
            visibility_key: preference_key(&source(), path),
            configured_visibility_key: None,
            label: "feature".into(),
            branch: Some("feature".into()),
            head: None,
            is_main: false,
            is_detached: false,
            is_bare: false,
            is_sparse: false,
            is_locked: false,
            is_prunable: false,
            locked_reason: None,
            prunable_reason: None,
            path_state: WorktreePathState::Present,
            authoritative: true,
            last_known: false,
            selectable: true,
        }
    }

    #[test]
    fn default_show_is_uncapped_and_keeps_separate_example_inventories() {
        let v26 = (0..3).map(|i| row(&format!("/cyberbase-v26-{i}"))).collect::<Vec<_>>();
        let aicos = (0..12).map(|i| {
            let mut row = row(&format!("/aicos-{i}"));
            row.is_prunable = i >= 9;
            row
        }).collect::<Vec<_>>();
        assert_eq!(v26.iter().filter(|row| sidebar_visible(row, &[])).count(), 3);
        assert_eq!(aicos.iter().filter(|row| sidebar_visible(row, &[])).count(), 9);
        let hidden = vec![aicos[1].visibility_key.clone().unwrap()];
        assert!(sidebar_visible(&row("/repo/.claude/worktrees/new"), &hidden));
        assert!(sidebar_visible(&row("/repo/codex/new"), &hidden));
        assert_eq!(aicos.len(), 12);
    }

    #[test]
    fn invalidity_offline_recovery_and_manual_hiding_are_independent() {
        let mut row = row("/repo-feature");
        let key = row.visibility_key.clone().unwrap();
        for state in [WorktreePathState::Present, WorktreePathState::Unknown] {
            row.path_state = state;
            row.last_known = true;
            row.authoritative = false;
            assert!(sidebar_visible(&row, &[]));
            assert!(!sidebar_visible(&row, std::slice::from_ref(&key)));
        }
        row.path_state = WorktreePathState::Missing;
        assert!(!sidebar_visible(&row, &[]));
        row.path_state = WorktreePathState::Present;
        row.is_prunable = true;
        assert!(!sidebar_visible(&row, &[]));
        row.is_prunable = false;
        row.is_locked = true;
        row.is_detached = true;
        row.branch = Some("renamed".into());
        row.label = "new label".into();
        assert!(sidebar_visible(&row, &[]));
        assert!(!sidebar_visible(&row, &[key]));
    }

    #[test]
    fn cancel_no_op_merge_and_undisplayed_exclusions_are_preserved() {
        let hidden = preference_key(&source(), "/hidden").unwrap();
        let absent = preference_key(&source(), "/absent").unwrap();
        let new = preference_key(&source(), "/new").unwrap();
        let mut current = vec![hidden.clone(), absent.clone()];
        let mut cancelled = VisibilityDraft::new(&current);
        cancelled.set_visible(hidden.clone(), true);
        drop(cancelled);
        assert_eq!(current, vec![hidden.clone(), absent.clone()]);

        let mut draft = VisibilityDraft::new(&current);
        draft.set_visible(hidden.clone(), true);
        draft.set_visible(hidden.clone(), false);
        assert!(!merge_edits(&mut current, draft.edits()));
        draft.set_visible(hidden.clone(), true);
        draft.set_visible(new.clone(), false);
        let concurrent = preference_key(&source(), "/concurrent").unwrap();
        current.push(concurrent.clone());
        assert!(merge_edits(&mut current, draft.edits()));
        assert_eq!(current, vec![absent, concurrent, new]);
        assert!(!merge_edits(&mut current, draft.edits()));
        assert!(draft.visible(&preference_key(&source(), "/later-discovery").unwrap()));
    }

    #[test]
    fn source_keys_keep_host_root_namespace_and_posix_case() {
        let mut a = source();
        a.backend = WorktreeVisibilityBackend::Wsl { distro: "ubuntu".into() };
        assert_eq!(preference_key(&a, "/Repo/./feature/").unwrap(), preference_key(&a, "/Repo/feature").unwrap());
        assert_ne!(preference_key(&a, "/Repo"), preference_key(&a, "/repo"));
        assert_ne!(preference_key(&a, "/repo\\"), preference_key(&a, "/repo"));
        let key = preference_key(&a, "/repo").unwrap();
        let mut other = a.clone();
        other.root_path = "/another-root".into();
        assert_ne!(key, preference_key(&other, "/repo").unwrap());
        other = a.clone();
        other.execution_host_id = ExecutionHostId::derive("other", &HostInstallId::new());
        assert_ne!(key, preference_key(&other, "/repo").unwrap());
        other = a;
        other.backend = WorktreeVisibilityBackend::Wsl { distro: "debian".into() };
        assert_ne!(key, preference_key(&other, "/repo").unwrap());
        assert!(preference_key(&other, "relative").is_none());
        assert!(preference_key(&other, "/../escape").is_none());
    }

    #[test]
    fn configured_locations_are_distinct_and_source_path_and_project_qualified() {
        let mut source = source();
        source.backend = WorktreeVisibilityBackend::Wsl { distro: "ubuntu".into() };
        let key = configured_preference_key(&source, "root", "/Repo/./link/").unwrap();
        assert_eq!(key, configured_preference_key(&source, "root", "/Repo/link").unwrap());
        assert_ne!(key, preference_key(&source, "/Repo/link").unwrap());
        assert_ne!(key, configured_preference_key(&source, "other-project", "/Repo/link").unwrap());
        assert_ne!(key, configured_preference_key(&source, "root", "/repo/link").unwrap());
        let mut other = source.clone();
        other.root_path = "/another-root".into();
        assert_ne!(key, configured_preference_key(&other, "root", "/Repo/link").unwrap());
        other = source.clone();
        other.execution_host_id = ExecutionHostId::derive("other", &HostInstallId::new());
        assert_ne!(key, configured_preference_key(&other, "root", "/Repo/link").unwrap());
        other = source.clone();
        other.backend = WorktreeVisibilityBackend::Wsl { distro: "debian".into() };
        assert_ne!(key, configured_preference_key(&other, "root", "/Repo/link").unwrap());
        other.backend = WorktreeVisibilityBackend::Ssh {
            connection_id: "ssh-a".into(), host: "host".into(), port: 22, user: "user".into(),
        };
        let ssh_key = configured_preference_key(&other, "root", "/Repo/link").unwrap();
        assert_ne!(key, ssh_key);
        if let WorktreeVisibilityBackend::Ssh { connection_id, .. } = &mut other.backend {
            *connection_id = "ssh-b".into();
        }
        assert_ne!(ssh_key, configured_preference_key(&other, "root", "/Repo/link").unwrap());
        assert!(configured_preference_key(&source, "", "/Repo/link").is_none());
        assert!(configured_preference_key(&source, "root", "relative").is_none());
    }

    #[test]
    fn ssh_preferences_ignore_reconnect_epochs_credentials_and_display_names() {
        let host = source().execution_host_id;
        let mut snapshot = ProjectExecutionSnapshot {
            project_id: "root".into(),
            root_project_id: "root".into(),
            worktree_id: WorktreeId::derive(&RepoId::derive(&host, "/repo/.git"), "/repo", None),
            execution_host_id: host,
            canonical_path: "/repo".into(),
            root_source_path: "/repo".into(),
            backend: ExecutionBackend::Ssh {
                connection: mt_config::SshConnection {
                    id: "ssh".into(), name: "name".into(), host: "host".into(), port: 0,
                    user: "user".into(), password: None, identity_file: None, group: None,
                },
                connection_fingerprint: 1,
                connection_epoch: Some(1),
            },
            host_label: "SSH".into(),
        };
        let before = source_from_snapshot(&snapshot, "/repo-link").unwrap();
        if let ExecutionBackend::Ssh { connection, connection_fingerprint, connection_epoch } = &mut snapshot.backend {
            connection.name = "renamed".into();
            connection.password = Some("new password".into());
            connection.identity_file = Some("new key path".into());
            connection.port = 22;
            *connection_fingerprint = 99;
            *connection_epoch = Some(9);
        }
        assert_eq!(before, source_from_snapshot(&snapshot, "/repo-link").unwrap());
        assert_ne!(before, source_from_snapshot(&snapshot, "/different-root").unwrap());
        if let ExecutionBackend::Ssh { connection, .. } = &mut snapshot.backend {
            connection.id = "other".into();
        }
        assert_ne!(before, source_from_snapshot(&snapshot, "/repo-link").unwrap());
    }
}
