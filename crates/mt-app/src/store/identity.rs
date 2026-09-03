//! Compatibility project IDs projected onto stable worktree identities.
//!
//! This is the only mt-app boundary allowed to resolve or replace a project
//! binding. UI callers remain project-ID based while persistence and deferred
//! workbench routing use the stable worktree identity returned here.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use gpui::Context;
use mt_config::ProjectConfig;
use mt_identity::{
    HostInstallId, PaneKey, TabId, TerminalIncarnationId, TerminalSessionId, WorktreeId,
};
use mt_layout::ProjectWorktreeBinding;
use mt_project::worktree::{self, ResolvedWorktreeIdentity, WorktreeIdentitySource};
use mt_ssh::RemoteRuntimeSnapshot;

use crate::execution_host::{ExecutionBackend, ProjectExecutionSnapshot};
use crate::persist;

use super::AppStore;

pub(super) type TerminalRoute = mt_ai::AgentRoute;

pub(super) enum AuthoritativeBindingInstall {
    Installed,
    Unchanged,
    Deferred(String),
    Failed(String),
}

fn authoritative_rebind_blocker(
    worktree_changed: bool,
    has_live_pty: bool,
    has_documents: bool,
) -> Option<&'static str> {
    if !worktree_changed {
        None
    } else if has_live_pty {
        Some("existing terminal sessions still use the compatibility identity")
    } else if has_documents {
        Some("open documents still use the compatibility identity")
    } else {
        None
    }
}

pub(super) fn resolve_project_bindings(
    projects: &[ProjectConfig],
    install_id: &HostInstallId,
    existing: &HashMap<String, ProjectWorktreeBinding>,
) -> Vec<ProjectWorktreeBinding> {
    projects
        .iter()
        .filter_map(|project| {
            resolve_project_binding(project, install_id, existing.get(&project.id))
                .map_err(|error| {
                    eprintln!(
                        "[identity] 项目 {} ({}) 无法建立 worktree 身份: {error:#}",
                        project.id, project.path
                    );
                })
                .ok()
        })
        .collect()
}

fn resolve_project_binding(
    project: &ProjectConfig,
    install_id: &HostInstallId,
    existing: Option<&ProjectWorktreeBinding>,
) -> anyhow::Result<ProjectWorktreeBinding> {
    let resolution = if let Some(connection_id) = project.ssh_connection_id.as_deref() {
        worktree::resolve_provisional_ssh(install_id, connection_id, &project.path)
    } else if let Some(wsl) = mt_core::parse_wsl_unc(&project.path.replace('/', "\\")) {
        worktree::resolve_provisional_wsl(install_id, &wsl.distro, &project.path)
    } else {
        match worktree::resolve_local(install_id, Path::new(&project.path)) {
            Ok(resolved) => Ok(resolved),
            Err(error) if existing.is_some() => Err(error),
            Err(_) => worktree::resolve_provisional_local(install_id, &project.path),
        }
    };
    let resolved = match resolution {
        Ok(resolved) => resolved,
        Err(error) => {
            let Some(binding) = existing else {
                return Err(error);
            };
            eprintln!(
                "[identity] 项目 {} 当前无法解析身份({error:#}),复用持久化绑定",
                project.id
            );
            ResolvedWorktreeIdentity {
                execution_host_id: binding.execution_host_id.clone(),
                repo_id: binding.repo_id.clone(),
                worktree_id: binding.worktree_id.clone(),
                canonical_worktree_path: binding
                    .canonical_worktree_path
                    .clone()
                    .unwrap_or_else(|| project.path.clone()),
                canonical_git_common_dir: None,
                source: worktree::WorktreeIdentitySource::PersistedFallback,
            }
        }
    };

    Ok(binding_from_resolved(project.id.clone(), resolved))
}

pub(super) fn binding_from_resolved(
    project_id: String,
    resolved: ResolvedWorktreeIdentity,
) -> ProjectWorktreeBinding {
    let identity_source = serde_json::to_value(resolved.source)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_string());
    ProjectWorktreeBinding {
        project_id,
        execution_host_id: resolved.execution_host_id,
        repo_id: resolved.repo_id,
        worktree_id: resolved.worktree_id,
        identity_source,
        canonical_worktree_path: Some(resolved.canonical_worktree_path),
    }
}

fn root_project_id(projects: &[ProjectConfig], project_id: &str) -> String {
    let mut current = project_id;
    let mut visited = HashSet::new();
    while visited.insert(current) {
        let Some(project) = projects.iter().find(|project| project.id == current) else {
            break;
        };
        let Some(parent) = project.parent_project_id.as_deref() else {
            break;
        };
        if !projects.iter().any(|project| project.id == parent) {
            break;
        }
        current = parent;
    }
    current.to_string()
}

fn host_path(project: &ProjectConfig, canonical: &str) -> String {
    if project.ssh_connection_id.is_some() {
        return canonical.to_string();
    }
    mt_core::parse_wsl_unc(&canonical.replace('/', "\\"))
        .or_else(|| mt_core::parse_wsl_unc(&project.path.replace('/', "\\")))
        .map(|path| path.unix_path)
        .unwrap_or_else(|| canonical.to_string())
}

impl AppStore {
    /// Immutable command-routing facts for one configured project/worktree.
    /// The caller may move this snapshot to a background thread, but must
    /// re-resolve and compare it before applying a completion.
    pub fn project_execution_snapshot(
        &self,
        project_id: &str,
    ) -> Result<ProjectExecutionSnapshot, String> {
        let project = self
            .project(project_id)
            .ok_or_else(|| "project no longer exists".to_string())?;
        let binding = self
            .project_worktree_bindings
            .get(project_id)
            .ok_or_else(|| "project has no worktree identity".to_string())?;
        let root_project_id = root_project_id(&self.config.projects, project_id);
        let root_project = self
            .project(&root_project_id)
            .ok_or_else(|| "root project no longer exists".to_string())?;
        let root_canonical = self
            .project_worktree_bindings
            .get(&root_project_id)
            .and_then(|binding| binding.canonical_worktree_path.as_deref())
            .unwrap_or(&root_project.path);
        let root_source_path = host_path(root_project, root_canonical);
        let canonical = binding
            .canonical_worktree_path
            .as_deref()
            .unwrap_or(&project.path);

        let (canonical_path, backend, host_label) =
            if let Some(connection_id) = project.ssh_connection_id.as_deref() {
                let connection = self
                    .remote_connection_of(project_id)
                    .ok_or_else(|| format!("SSH connection {connection_id} is unavailable"))?;
                let connection_fingerprint = crate::remote_ssh::connection_fingerprint(&connection);
                let connection_epoch = crate::remote_ssh::current_connection_epoch(&connection.id);
                let summary = crate::ssh_conn::connection_summary(&connection);
                let host_label = if connection.name.trim().is_empty() {
                    summary
                } else {
                    format!("{} ({summary})", connection.name)
                };
                (
                    canonical.to_string(),
                    ExecutionBackend::Ssh {
                        connection,
                        connection_fingerprint,
                        connection_epoch,
                    },
                    host_label,
                )
            } else if let Some(wsl) = mt_core::parse_wsl_unc(&canonical.replace('/', "\\"))
                .or_else(|| mt_core::parse_wsl_unc(&project.path.replace('/', "\\")))
            {
                let host_label = format!("WSL ({})", wsl.distro);
                (
                    wsl.unix_path,
                    ExecutionBackend::Wsl { distro: wsl.distro },
                    host_label,
                )
            } else {
                (
                    canonical.to_string(),
                    ExecutionBackend::Local,
                    "Local machine".to_string(),
                )
            };
        if canonical_path.is_empty() {
            return Err("project worktree path is empty".into());
        }

        Ok(ProjectExecutionSnapshot {
            project_id: project.id.clone(),
            root_project_id,
            worktree_id: binding.worktree_id.clone(),
            execution_host_id: binding.execution_host_id.clone(),
            canonical_path,
            root_source_path,
            backend,
            host_label,
        })
    }

    pub fn active_worktree_id(&self) -> Option<&WorktreeId> {
        self.active_worktree_id.as_ref()
    }

    pub fn worktree_id_for_project(&self, project_id: &str) -> Option<&WorktreeId> {
        self.project_worktree_bindings
            .get(project_id)
            .map(|binding| &binding.worktree_id)
    }

    #[allow(dead_code)]
    pub fn project_id_for_worktree(&self, worktree_id: &WorktreeId) -> Option<&str> {
        if let Some(active_project_id) = self.active_project_id.as_deref()
            && self.worktree_id_for_project(active_project_id) == Some(worktree_id)
        {
            return Some(active_project_id);
        }
        self.config
            .projects
            .iter()
            .find(|project| self.worktree_id_for_project(&project.id) == Some(worktree_id))
            .map(|project| project.id.as_str())
    }

    #[allow(dead_code)]
    pub fn terminal_binding_matches(
        &self,
        worktree_id: &WorktreeId,
        tab_id: &TabId,
        pane_key: &PaneKey,
        terminal_session_id: &TerminalSessionId,
        terminal_incarnation_id: &TerminalIncarnationId,
    ) -> bool {
        self.project_worktree_bindings
            .iter()
            .any(|(project_id, binding)| {
                if &binding.worktree_id != worktree_id {
                    return false;
                }
                self.project_states
                    .get(project_id)
                    .and_then(|state| state.panels.iter().find(|panel| &panel.tab_id == tab_id))
                    .and_then(|panel| panel.layout.pane(pane_key.as_str()))
                    .is_some_and(|pane| {
                        &pane.terminal_session_id == terminal_session_id
                            && pane.accepts_terminal_incarnation(terminal_incarnation_id)
                    })
            })
    }

    pub(super) fn sync_active_worktree(&mut self) {
        self.active_worktree_id = self
            .active_project_id
            .as_deref()
            .and_then(|project_id| self.project_worktree_bindings.get(project_id))
            .map(|binding| binding.worktree_id.clone());
    }

    pub(super) fn register_project_identity(&mut self, project_id: &str) {
        let Some(project) = self.project(project_id).cloned() else {
            return;
        };
        let existing = self.project_worktree_bindings.get(project_id);
        let Ok(binding) = resolve_project_binding(&project, &self.host_install_id, existing) else {
            return;
        };

        let mut restored_layout = None;
        if let Some(store) = self.layout_store.as_ref() {
            let now_ms = super::layout::unix_time_ms();
            match store.reconcile_worktree_layouts(std::slice::from_ref(&binding), now_ms) {
                Ok(mut reconciled) => {
                    restored_layout = reconciled.layouts.remove(project_id);
                    self.project_worktree_bindings.extend(reconciled.bindings);
                }
                Err(error) => {
                    eprintln!("[identity] 项目 {project_id} 的 worktree 绑定落盘失败: {error:#}");
                    self.project_worktree_bindings
                        .insert(project_id.to_string(), binding.clone());
                }
            }
        } else {
            self.project_worktree_bindings
                .insert(project_id.to_string(), binding.clone());
        }

        if let Some(layout) = restored_layout {
            if let Some(project) = self
                .config
                .projects
                .iter_mut()
                .find(|project| project.id == project_id)
            {
                project.saved_layout = Some(layout.clone());
            }
            if self
                .project_states
                .get(project_id)
                .is_some_and(|state| state.panels.is_empty())
            {
                let (panels, active_panel_id) = persist::restore_layout(&layout, &self.config);
                if let Some(state) = self.project_states.get_mut(project_id) {
                    state.panels = panels;
                    state.active_panel_id = active_panel_id;
                    state.status = state.highest_status();
                }
            }
        }
        self.sync_active_worktree();
    }

    pub(super) fn install_authoritative_remote_binding(
        &mut self,
        project_id: &str,
        snapshot: &RemoteRuntimeSnapshot,
        cx: &mut Context<Self>,
    ) -> AuthoritativeBindingInstall {
        let Some(project) = self.project(project_id) else {
            return AuthoritativeBindingInstall::Failed("project no longer exists".into());
        };
        if project.ssh_connection_id.is_none() {
            return AuthoritativeBindingInstall::Failed(
                "project is no longer an SSH project".into(),
            );
        }

        let source = if snapshot.canonical_git_common_dir.is_some() {
            WorktreeIdentitySource::AuthoritativeRemoteGit
        } else {
            WorktreeIdentitySource::AuthoritativeRemoteDirectory
        };
        let binding = binding_from_resolved(
            project_id.to_string(),
            ResolvedWorktreeIdentity {
                execution_host_id: snapshot.identity.execution_host_id.clone(),
                repo_id: snapshot.repo_id.clone(),
                worktree_id: snapshot.worktree_id.clone(),
                canonical_worktree_path: snapshot.canonical_worktree_path.clone(),
                canonical_git_common_dir: snapshot.canonical_git_common_dir.clone(),
                source,
            },
        );

        let previous = self.project_worktree_bindings.get(project_id).cloned();
        if previous.as_ref() == Some(&binding) {
            return AuthoritativeBindingInstall::Unchanged;
        }
        let worktree_changed = previous
            .as_ref()
            .is_some_and(|previous| previous.worktree_id != binding.worktree_id);
        let has_live_pty = self
            .project_states
            .get(project_id)
            .is_some_and(|state| !state.pty_ids().is_empty());
        let has_documents = crate::workbench_area::project_has_documents(project_id, cx);
        if let Some(reason) =
            authoritative_rebind_blocker(worktree_changed, has_live_pty, has_documents)
        {
            return AuthoritativeBindingInstall::Deferred(reason.into());
        }

        let mut restored_layout = None;
        if let Some(store) = self.layout_store.as_ref() {
            let now_ms = super::layout::unix_time_ms();
            let mut reconciled =
                match store.reconcile_worktree_layouts(std::slice::from_ref(&binding), now_ms) {
                    Ok(reconciled) => reconciled,
                    Err(error) => {
                        return AuthoritativeBindingInstall::Failed(format!(
                            "authoritative worktree binding could not be persisted: {error:#}"
                        ));
                    }
                };
            restored_layout = reconciled.layouts.remove(project_id);
            self.project_worktree_bindings.extend(reconciled.bindings);
        } else {
            self.project_worktree_bindings
                .insert(project_id.to_string(), binding);
        }

        if let Some(layout) = restored_layout {
            if let Some(project) = self
                .config
                .projects
                .iter_mut()
                .find(|project| project.id == project_id)
            {
                project.saved_layout = Some(layout.clone());
            }
            if self
                .project_states
                .get(project_id)
                .is_some_and(|state| state.panels.is_empty())
            {
                let (panels, active_panel_id) = persist::restore_layout(&layout, &self.config);
                if let Some(state) = self.project_states.get_mut(project_id) {
                    state.panels = panels;
                    state.active_panel_id = active_panel_id;
                    state.status = state.highest_status();
                }
            }
        }
        self.sync_active_worktree();
        AuthoritativeBindingInstall::Installed
    }

    pub(super) fn remove_project_identity(&mut self, project_id: &str) {
        if let Some(store) = self.layout_store.as_ref()
            && let Err(error) = store.delete_project_binding(project_id)
        {
            eprintln!("[identity] 删除项目 {project_id} 的 worktree 绑定失败: {error:#}");
        }
        self.project_worktree_bindings.remove(project_id);
        self.sync_active_worktree();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt_identity::ExecutionHostId;

    fn project(id: &str, path: &str, ssh_connection_id: Option<&str>) -> ProjectConfig {
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
            ssh_connection_id: ssh_connection_id.map(str::to_string),
            parent_project_id: None,
            kind_override: None,
        }
    }

    fn route(incarnation: TerminalIncarnationId) -> TerminalRoute {
        let install = HostInstallId::new();
        let execution_host_id = ExecutionHostId::derive("local", &install);
        let repo_id = mt_identity::RepoId::derive(&execution_host_id, "/repo/.git");
        TerminalRoute {
            execution_host_id,
            worktree_id: WorktreeId::derive(&repo_id, "/repo", None),
            tab_id: TabId::new(),
            pane_key: PaneKey::new(),
            terminal_session_id: TerminalSessionId::new(),
            terminal_incarnation_id: incarnation,
        }
    }

    #[test]
    fn prior_incarnation_cannot_match_current_terminal_route() {
        let old = route(TerminalIncarnationId::new());
        let mut current = old.clone();
        current.terminal_incarnation_id = TerminalIncarnationId::new();

        assert_ne!(old, current);
        assert_eq!(old.terminal_session_id, current.terminal_session_id);
        assert_eq!(old.pane_key, current.pane_key);
    }

    #[test]
    fn authoritative_rebind_defers_for_live_terminal_or_document_scope() {
        assert_eq!(
            authoritative_rebind_blocker(true, true, false),
            Some("existing terminal sessions still use the compatibility identity")
        );
        assert_eq!(
            authoritative_rebind_blocker(true, false, true),
            Some("open documents still use the compatibility identity")
        );
        assert_eq!(authoritative_rebind_blocker(true, false, false), None);
        assert_eq!(authoritative_rebind_blocker(false, true, true), None);
    }

    #[test]
    fn persisted_binding_survives_temporarily_invalid_ssh_identity_input() {
        let install = HostInstallId::new();
        let existing = binding_from_resolved(
            "ssh-project".to_string(),
            worktree::resolve_provisional_ssh(&install, "connection-1", "/srv/repo").unwrap(),
        );
        let projects = [project(
            "ssh-project",
            "temporarily-not-an-absolute-path",
            Some("connection-1"),
        )];
        let existing_by_project = HashMap::from([(existing.project_id.clone(), existing.clone())]);

        let resolved = resolve_project_bindings(&projects, &install, &existing_by_project);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].execution_host_id, existing.execution_host_id);
        assert_eq!(resolved[0].repo_id, existing.repo_id);
        assert_eq!(resolved[0].worktree_id, existing.worktree_id);
        assert_eq!(resolved[0].identity_source, "persistedFallback");
        assert_eq!(
            resolved[0].canonical_worktree_path,
            existing.canonical_worktree_path
        );
    }

    #[test]
    fn linked_worktrees_resolve_to_one_root_project() {
        let root = project("root", "/repo", None);
        let mut child = project("child", "/repo-linked", None);
        child.parent_project_id = Some("root".into());
        let mut grandchild = project("grandchild", "/repo-linked-2", None);
        grandchild.parent_project_id = Some("child".into());
        let projects = vec![root, child, grandchild];

        assert_eq!(root_project_id(&projects, "root"), "root");
        assert_eq!(root_project_id(&projects, "child"), "root");
        assert_eq!(root_project_id(&projects, "grandchild"), "root");
    }

    #[test]
    fn host_path_converts_wsl_unc_without_changing_local_or_ssh_paths() {
        let wsl = project("wsl", r"\\wsl$\Ubuntu\home\u\repo", None);
        assert_eq!(host_path(&wsl, &wsl.path), "/home/u/repo");

        let local = project("local", "/repo", None);
        assert_eq!(host_path(&local, "/repo"), "/repo");

        let ssh = project("ssh", "/srv/repo", Some("connection"));
        assert_eq!(host_path(&ssh, "/srv/repo"), "/srv/repo");
    }
}
