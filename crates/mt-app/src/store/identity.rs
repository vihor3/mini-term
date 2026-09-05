//! Compatibility project IDs projected onto stable worktree identities.
//!
//! This is the only mt-app boundary allowed to resolve or replace a project
//! binding. UI callers remain project-ID based while persistence and deferred
//! workbench routing use the stable worktree identity returned here.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use gpui::Context;
use mt_config::{AppConfig, ProjectConfig, SavedProjectLayout, SshConnection};
use mt_identity::{
    HostInstallId, PaneKey, TabId, TerminalIncarnationId, TerminalSessionId, WorktreeId,
};
use mt_layout::ProjectWorktreeBinding;
use mt_project::worktree::{self, ResolvedWorktreeIdentity, WorktreeIdentitySource};
use mt_ssh::RemoteRuntimeSnapshot;

use crate::execution_host::{ExecutionBackend, ProjectExecutionSnapshot};
use crate::persist;

use super::{AppStore, ProjectState};

pub(super) type TerminalRoute = mt_ai::AgentRoute;

pub(super) enum AuthoritativeBindingInstall {
    Installed,
    Unchanged,
    Deferred(String),
    Failed(String),
}

pub(super) struct PreparedProjectIdentity {
    binding: ProjectWorktreeBinding,
    restored_layout: Option<SavedProjectLayout>,
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

fn is_authoritative_remote_binding(binding: &ProjectWorktreeBinding) -> bool {
    binding.identity_source == WorktreeIdentitySource::AuthoritativeRemoteGit.as_str()
        || binding.identity_source == WorktreeIdentitySource::AuthoritativeRemoteDirectory.as_str()
}

fn normalized_ssh_port(port: u16) -> u16 {
    if port == 0 { 22 } else { port }
}

fn ssh_binding_identity_context(
    connection: &SshConnection,
    normalized_configured_path: &str,
) -> String {
    serde_json::to_string(&(
        "ssh-authority-v2",
        connection.id.as_str(),
        connection.host.as_str(),
        normalized_ssh_port(connection.port),
        connection.user.as_str(),
        normalized_configured_path,
    ))
    .expect("SSH binding context contains only JSON-safe scalar values")
}

fn preserved_authoritative_ssh_binding(
    project_id: String,
    binding: &ProjectWorktreeBinding,
    identity_context: String,
) -> ProjectWorktreeBinding {
    ProjectWorktreeBinding {
        project_id,
        execution_host_id: binding.execution_host_id.clone(),
        repo_id: binding.repo_id.clone(),
        worktree_id: binding.worktree_id.clone(),
        identity_source: binding.identity_source.clone(),
        canonical_worktree_path: binding.canonical_worktree_path.clone(),
        identity_context: Some(identity_context),
    }
}

fn has_other_worktree_alias(
    bindings: &HashMap<String, ProjectWorktreeBinding>,
    project_id: &str,
) -> bool {
    let Some(worktree_id) = bindings.get(project_id).map(|binding| &binding.worktree_id) else {
        return false;
    };
    bindings
        .iter()
        .any(|(other_id, binding)| other_id != project_id && &binding.worktree_id == worktree_id)
}

fn apply_reconciled_project_layout(
    config: &mut AppConfig,
    project_states: &mut HashMap<String, ProjectState>,
    project_id: &str,
    layout: SavedProjectLayout,
    replace_nonempty_state: bool,
) {
    if let Some(project) = config
        .projects
        .iter_mut()
        .find(|project| project.id == project_id)
    {
        project.saved_layout = Some(layout.clone());
    }
    let should_restore = project_states
        .get(project_id)
        .is_some_and(|state| replace_nonempty_state || state.panels.is_empty());
    if !should_restore {
        return;
    }

    let (panels, active_panel_id) = persist::restore_layout(&layout, config);
    if let Some(state) = project_states.get_mut(project_id) {
        state.panels = panels;
        state.active_panel_id = active_panel_id;
        state.restore_terminal_navigation(&layout);
        state.maximized_pane_id = None;
        state.status = state.highest_status();
    }
}

pub(super) fn resolve_project_bindings(
    projects: &[ProjectConfig],
    ssh_connections: &[SshConnection],
    install_id: &HostInstallId,
    existing: &HashMap<String, ProjectWorktreeBinding>,
) -> Vec<ProjectWorktreeBinding> {
    projects
        .iter()
        .filter_map(|project| {
            resolve_project_binding(
                project,
                ssh_connections,
                install_id,
                existing.get(&project.id),
            )
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
    ssh_connections: &[SshConnection],
    install_id: &HostInstallId,
    existing: Option<&ProjectWorktreeBinding>,
) -> anyhow::Result<ProjectWorktreeBinding> {
    let persisted_authoritative_ssh_binding = project
        .ssh_connection_id
        .as_ref()
        .and_then(|_| existing.filter(|binding| is_authoritative_remote_binding(binding)));
    let connection = crate::ssh_conn::remote_connection(project, ssh_connections);

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
        Ok(resolved) => {
            if let Some(binding) = persisted_authoritative_ssh_binding
                && let Some(connection) = connection
            {
                let identity_context =
                    ssh_binding_identity_context(connection, &resolved.canonical_worktree_path);
                if binding.identity_context.as_deref() == Some(identity_context.as_str()) {
                    // Provenance names the same configured endpoint and path. Keep
                    // the authenticated canonical path because the configured path
                    // may be a symlink or another remote alias for that worktree.
                    return Ok(preserved_authoritative_ssh_binding(
                        project.id.clone(),
                        binding,
                        identity_context,
                    ));
                }
            }
            resolved
        }
        Err(error) if persisted_authoritative_ssh_binding.is_some() => {
            // Without a normalized configured path there is no proof that the
            // persisted remote identity still belongs to this project.
            return Err(error);
        }
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
        identity_context: None,
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
    pub(super) fn onboarding_canonical_path_for_project(
        &self,
        project: &ProjectConfig,
    ) -> Option<&str> {
        let binding = self.project_worktree_bindings.get(&project.id)?;
        let canonical_path = binding.canonical_worktree_path.as_deref()?;
        let identity_source = binding.identity_source.as_str();

        let Some(connection_id) = project.ssh_connection_id.as_deref() else {
            if [
                WorktreeIdentitySource::AuthoritativeLocalGit.as_str(),
                WorktreeIdentitySource::LocalDirectory.as_str(),
            ]
            .contains(&identity_source)
            {
                return Some(canonical_path);
            }

            let configured_path = if identity_source
                == WorktreeIdentitySource::ProvisionalLocal.as_str()
            {
                worktree::resolve_provisional_local(&self.host_install_id, &project.path)
                    .ok()?
                    .canonical_worktree_path
            } else if identity_source == WorktreeIdentitySource::ProvisionalWsl.as_str() {
                let host_visible_path = project.path.replace('/', "\\");
                let wsl = mt_core::parse_wsl_unc(&host_visible_path)?;
                worktree::resolve_provisional_wsl(&self.host_install_id, &wsl.distro, &project.path)
                    .ok()?
                    .canonical_worktree_path
            } else {
                return None;
            };
            return (canonical_path == configured_path.as_str()).then_some(canonical_path);
        };
        let connection = crate::ssh_conn::remote_connection(project, &self.config.ssh_connections)?;
        let configured_path =
            worktree::resolve_provisional_ssh(&self.host_install_id, connection_id, &project.path)
                .ok()?
                .canonical_worktree_path;

        if is_authoritative_remote_binding(binding) {
            let expected_context = ssh_binding_identity_context(connection, &configured_path);
            (binding.identity_context.as_deref() == Some(expected_context.as_str()))
                .then_some(canonical_path)
        } else if identity_source == WorktreeIdentitySource::ProvisionalSsh.as_str() {
            (canonical_path == configured_path.as_str()).then_some(canonical_path)
        } else {
            None
        }
    }

    /// Canonical path that is safe to use for catalog dedupe. Authoritative
    /// aliases are returned only when their resolver-owned provenance still
    /// matches the current project and execution-host configuration.
    pub(crate) fn trusted_canonical_worktree_path_for_project(
        &self,
        project_id: &str,
    ) -> Option<&str> {
        self.project(project_id)
            .and_then(|project| self.onboarding_canonical_path_for_project(project))
    }

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

    pub(super) fn prepare_project_identity(
        &self,
        project: &ProjectConfig,
    ) -> Result<PreparedProjectIdentity, String> {
        let existing = self.project_worktree_bindings.get(&project.id);
        let binding = resolve_project_binding(
            project,
            &self.config.ssh_connections,
            &self.host_install_id,
            existing,
        )
        .map_err(|error| format!("could not resolve project identity: {error:#}"))?;
        let mut restored_layout = None;
        let binding = if let Some(store) = self.layout_store.as_ref() {
            let now_ms = super::layout::unix_time_ms();
            let mut reconciled = store
                .reconcile_worktree_layouts(std::slice::from_ref(&binding), now_ms)
                .map_err(|error| format!("could not persist project identity: {error:#}"))?;
            restored_layout = reconciled.layouts.remove(&project.id);
            reconciled.bindings.remove(&project.id).unwrap_or(binding)
        } else {
            binding
        };
        Ok(PreparedProjectIdentity {
            binding,
            restored_layout,
        })
    }

    pub(super) fn install_prepared_project_identity(
        &mut self,
        project_id: &str,
        prepared: PreparedProjectIdentity,
    ) -> WorktreeId {
        let worktree_id = prepared.binding.worktree_id.clone();
        self.project_worktree_bindings
            .insert(project_id.to_string(), prepared.binding);
        if let Some(layout) = prepared.restored_layout {
            apply_reconciled_project_layout(
                &mut self.config,
                &mut self.project_states,
                project_id,
                layout,
                false,
            );
        }
        self.sync_active_worktree();
        worktree_id
    }

    pub(super) fn register_project_identity(&mut self, project_id: &str) {
        let Some(project) = self.project(project_id).cloned() else {
            return;
        };
        match self.prepare_project_identity(&project) {
            Ok(prepared) => {
                self.install_prepared_project_identity(project_id, prepared);
            }
            Err(error) => {
                eprintln!("[identity] project {project_id} identity registration failed: {error}");
                let existing = self.project_worktree_bindings.get(project_id);
                if let Ok(binding) = resolve_project_binding(
                    &project,
                    &self.config.ssh_connections,
                    &self.host_install_id,
                    existing,
                ) {
                    self.project_worktree_bindings
                        .insert(project_id.to_string(), binding);
                    self.sync_active_worktree();
                }
            }
        }
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
        let Some(connection_id) = project.ssh_connection_id.as_deref() else {
            return AuthoritativeBindingInstall::Failed(
                "project is no longer an SSH project".into(),
            );
        };
        let Some(connection) =
            crate::ssh_conn::remote_connection(project, &self.config.ssh_connections)
        else {
            return AuthoritativeBindingInstall::Failed(format!(
                "SSH connection {connection_id} is unavailable"
            ));
        };
        let normalized_configured_path = match worktree::resolve_provisional_ssh(
            &self.host_install_id,
            connection_id,
            &project.path,
        ) {
            Ok(resolved) => resolved.canonical_worktree_path,
            Err(error) => {
                return AuthoritativeBindingInstall::Failed(format!(
                    "configured SSH worktree path is invalid: {error:#}"
                ));
            }
        };
        let identity_context =
            ssh_binding_identity_context(connection, &normalized_configured_path);

        let source = if snapshot.canonical_git_common_dir.is_some() {
            WorktreeIdentitySource::AuthoritativeRemoteGit
        } else {
            WorktreeIdentitySource::AuthoritativeRemoteDirectory
        };
        let mut binding = binding_from_resolved(
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
        binding.identity_context = Some(identity_context);

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
            apply_reconciled_project_layout(
                &mut self.config,
                &mut self.project_states,
                project_id,
                layout,
                worktree_changed,
            );
        }
        self.sync_active_worktree();
        AuthoritativeBindingInstall::Installed
    }

    pub(super) fn project_has_other_worktree_alias(&self, project_id: &str) -> bool {
        has_other_worktree_alias(&self.project_worktree_bindings, project_id)
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
    use mt_config::{SavedPane, SavedSplitNode, SavedTab};
    use mt_identity::{ExecutionHostId, RepoId};

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
            hidden_worktrees: Vec::new(),
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

    fn saved_layout(shell_name: &str, cwd: &str, worktree_id: WorktreeId) -> SavedProjectLayout {
        SavedProjectLayout {
            selected_terminal_pane_key: None,
            terminal_order: None,
            worktree_id: Some(worktree_id),
            tabs: vec![SavedTab {
                tab_id: Some(TabId::new()),
                custom_title: None,
                split_layout: SavedSplitNode::Leaf {
                    active_pane_key: None,
                    pane: None,
                    panes: vec![SavedPane {
                        pane_key: Some(PaneKey::new()),
                        terminal_session_id: Some(TerminalSessionId::new()),
                        terminal_incarnation_id: None,
                        shell_name: shell_name.to_string(),
                        cwd: Some(cwd.to_string()),
                        ai_session: None,
                    }],
                },
            }],
            active_tab_index: 0,
            active_tab_id: None,
        }
    }

    fn ssh_connection(id: &str, host: &str, port: u16, user: &str) -> SshConnection {
        SshConnection {
            id: id.to_string(),
            name: format!("display-{id}"),
            host: host.to_string(),
            port,
            user: user.to_string(),
            password: None,
            identity_file: None,
            group: None,
        }
    }

    fn authoritative_remote_binding(
        project_id: &str,
        path: &str,
        connection: &SshConnection,
    ) -> ProjectWorktreeBinding {
        let remote_install = HostInstallId::new();
        let execution_host_id = ExecutionHostId::derive("SHA256:verified-host", &remote_install);
        let repo_id = RepoId::derive(&execution_host_id, &format!("{path}/.git"));
        let mut binding = binding_from_resolved(
            project_id.to_string(),
            ResolvedWorktreeIdentity {
                execution_host_id,
                repo_id: repo_id.clone(),
                worktree_id: WorktreeId::derive(&repo_id, path, None),
                canonical_worktree_path: path.to_string(),
                canonical_git_common_dir: Some(format!("{path}/.git")),
                source: WorktreeIdentitySource::AuthoritativeRemoteGit,
            },
        );
        binding.identity_context = Some(ssh_binding_identity_context(connection, path));
        binding
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
        let connection = ssh_connection("connection-1", "host.example", 22, "deploy");
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

        let resolved = resolve_project_bindings(
            &projects,
            std::slice::from_ref(&connection),
            &install,
            &existing_by_project,
        );

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
    fn unchanged_ssh_context_preserves_authoritative_remote_identity() {
        let local_install = HostInstallId::new();
        let persisted_connection = ssh_connection("connection-1", "host.example", 0, "deploy");
        let authoritative =
            authoritative_remote_binding("ssh-project", "/srv/repo", &persisted_connection);
        let mut current_connection = persisted_connection.clone();
        current_connection.port = 22;
        current_connection.name = "renamed display".into();
        current_connection.password = Some("new password".into());
        current_connection.identity_file = Some("/new/private/key".into());
        let configured = project("ssh-project", "/srv/repo", Some("connection-1"));
        let provisional = binding_from_resolved(
            configured.id.clone(),
            worktree::resolve_provisional_ssh(&local_install, "connection-1", "/srv/repo").unwrap(),
        );
        assert_ne!(provisional.worktree_id, authoritative.worktree_id);

        let resolved = resolve_project_bindings(
            &[configured],
            std::slice::from_ref(&current_connection),
            &local_install,
            &HashMap::from([("ssh-project".to_string(), authoritative.clone())]),
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].execution_host_id,
            authoritative.execution_host_id
        );
        assert_eq!(resolved[0].repo_id, authoritative.repo_id);
        assert_eq!(resolved[0].worktree_id, authoritative.worktree_id);
        assert_eq!(
            resolved[0].canonical_worktree_path,
            authoritative.canonical_worktree_path
        );
        assert_eq!(
            resolved[0].identity_source,
            WorktreeIdentitySource::AuthoritativeRemoteGit.as_str()
        );
        assert_eq!(
            resolved[0].identity_context.as_deref(),
            Some(ssh_binding_identity_context(&current_connection, "/srv/repo").as_str())
        );
    }

    #[test]
    fn configured_ssh_alias_preserves_authenticated_canonical_path() {
        let local_install = HostInstallId::new();
        let connection = ssh_connection("connection-1", "host.example", 22, "deploy");
        let mut authoritative =
            authoritative_remote_binding("ssh-project", "/srv/repo-real", &connection);
        authoritative.identity_context =
            Some(ssh_binding_identity_context(&connection, "/srv/repo-link"));
        let configured = project("ssh-project", "/srv/repo-link", Some("connection-1"));

        let resolved = resolve_project_bindings(
            &[configured],
            &[connection],
            &local_install,
            &HashMap::from([("ssh-project".to_string(), authoritative.clone())]),
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].execution_host_id,
            authoritative.execution_host_id
        );
        assert_eq!(resolved[0].repo_id, authoritative.repo_id);
        assert_eq!(resolved[0].worktree_id, authoritative.worktree_id);
        assert_eq!(
            resolved[0].canonical_worktree_path.as_deref(),
            Some("/srv/repo-real")
        );
        assert_eq!(resolved[0].identity_context, authoritative.identity_context);
    }

    #[test]
    fn ssh_authority_context_contains_only_public_endpoint_identity() {
        let mut connection = ssh_connection("connection-1", "host.example", 0, "deploy");
        connection.name = "display-name-secret".into();
        connection.password = Some("password-secret".into());
        connection.identity_file = Some("private-key-secret".into());
        connection.group = Some("group-secret".into());

        let context = ssh_binding_identity_context(&connection, "/srv/repo");

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&context).unwrap(),
            serde_json::json!([
                "ssh-authority-v2",
                "connection-1",
                "host.example",
                22,
                "deploy",
                "/srv/repo"
            ])
        );
        for secret in [
            "display-name-secret",
            "password-secret",
            "private-key-secret",
            "group-secret",
            "token-secret",
        ] {
            assert!(!context.contains(secret));
        }
    }

    #[test]
    fn changed_ssh_host_user_or_port_rejects_authoritative_binding() {
        let local_install = HostInstallId::new();
        let persisted_connection = ssh_connection("connection-1", "host.example", 22, "deploy");
        let authoritative =
            authoritative_remote_binding("ssh-project", "/srv/repo", &persisted_connection);
        let configured = project("ssh-project", "/srv/repo", Some("connection-1"));
        let expected = binding_from_resolved(
            configured.id.clone(),
            worktree::resolve_provisional_ssh(&local_install, "connection-1", "/srv/repo").unwrap(),
        );

        let mut changed_host = persisted_connection.clone();
        changed_host.host = "other.example".into();
        let mut changed_user = persisted_connection.clone();
        changed_user.user = "other-user".into();
        let mut changed_port = persisted_connection;
        changed_port.port = 2222;

        for connection in [changed_host, changed_user, changed_port] {
            let resolved = resolve_project_bindings(
                std::slice::from_ref(&configured),
                std::slice::from_ref(&connection),
                &local_install,
                &HashMap::from([("ssh-project".to_string(), authoritative.clone())]),
            );
            assert_eq!(resolved, vec![expected.clone()]);
        }
    }

    #[test]
    fn legacy_null_ssh_context_rejects_authoritative_binding() {
        let local_install = HostInstallId::new();
        let connection = ssh_connection("connection-1", "host.example", 22, "deploy");
        let mut authoritative =
            authoritative_remote_binding("ssh-project", "/srv/repo", &connection);
        authoritative.identity_context = None;
        let configured = project("ssh-project", "/srv/repo", Some("connection-1"));
        let expected = binding_from_resolved(
            configured.id.clone(),
            worktree::resolve_provisional_ssh(&local_install, "connection-1", "/srv/repo").unwrap(),
        );

        let resolved = resolve_project_bindings(
            &[configured],
            &[connection],
            &local_install,
            &HashMap::from([("ssh-project".to_string(), authoritative)]),
        );

        assert_eq!(resolved, vec![expected]);
    }

    #[test]
    fn missing_ssh_connection_rejects_authoritative_binding() {
        let local_install = HostInstallId::new();
        let connection = ssh_connection("connection-1", "host.example", 22, "deploy");
        let authoritative = authoritative_remote_binding("ssh-project", "/srv/repo", &connection);
        let configured = project("ssh-project", "/srv/repo", Some("connection-1"));
        let expected = binding_from_resolved(
            configured.id.clone(),
            worktree::resolve_provisional_ssh(&local_install, "connection-1", "/srv/repo").unwrap(),
        );

        let resolved = resolve_project_bindings(
            &[configured],
            &[],
            &local_install,
            &HashMap::from([("ssh-project".to_string(), authoritative)]),
        );

        assert_eq!(resolved, vec![expected]);
    }

    #[test]
    fn untyped_persisted_fallback_cannot_become_ssh_host_authority() {
        let install = HostInstallId::new();
        let connection = ssh_connection("connection-1", "host.example", 22, "deploy");
        let configured = project("ssh-project", "/srv/repo", Some("connection-1"));
        let expected = binding_from_resolved(
            configured.id.clone(),
            worktree::resolve_provisional_ssh(&install, "connection-1", "/srv/repo").unwrap(),
        );
        let local_execution_host_id = ExecutionHostId::derive("local", &install);
        let local_repo_id = RepoId::derive(&local_execution_host_id, "/srv/repo");
        let stale_fallback = ProjectWorktreeBinding {
            project_id: configured.id.clone(),
            execution_host_id: local_execution_host_id,
            repo_id: local_repo_id.clone(),
            worktree_id: WorktreeId::derive(&local_repo_id, "/srv/repo", None),
            identity_source: WorktreeIdentitySource::PersistedFallback
                .as_str()
                .to_string(),
            canonical_worktree_path: Some("/srv/repo".into()),
            identity_context: None,
        };

        let resolved = resolve_project_bindings(
            &[configured],
            &[connection],
            &install,
            &HashMap::from([("ssh-project".to_string(), stale_fallback.clone())]),
        );

        assert_eq!(resolved, vec![expected]);
        assert_ne!(
            resolved[0].execution_host_id,
            stale_fallback.execution_host_id
        );
        assert_eq!(
            resolved[0].identity_source,
            WorktreeIdentitySource::ProvisionalSsh.as_str()
        );
    }

    #[test]
    fn changed_authoritative_remote_path_rejects_fallback_and_resolves_provisionally() {
        let local_install = HostInstallId::new();
        let connection = ssh_connection("connection-1", "host.example", 22, "deploy");
        let authoritative = authoritative_remote_binding("ssh-project", "/srv/repo", &connection);
        let configured = project("ssh-project", "/srv/renamed-repo", Some("connection-1"));
        let expected = binding_from_resolved(
            configured.id.clone(),
            worktree::resolve_provisional_ssh(&local_install, "connection-1", "/srv/renamed-repo")
                .unwrap(),
        );

        let resolved = resolve_project_bindings(
            &[configured],
            &[connection],
            &local_install,
            &HashMap::from([("ssh-project".to_string(), authoritative.clone())]),
        );

        assert_eq!(resolved, vec![expected]);
        assert_ne!(resolved[0].worktree_id, authoritative.worktree_id);
        assert_eq!(
            resolved[0].identity_source,
            WorktreeIdentitySource::ProvisionalSsh.as_str()
        );
    }

    #[test]
    fn invalid_remote_path_never_reuses_old_authoritative_binding() {
        let local_install = HostInstallId::new();
        let connection = ssh_connection("connection-1", "host.example", 22, "deploy");
        let authoritative = authoritative_remote_binding("ssh-project", "/srv/repo", &connection);
        let configured = project(
            "ssh-project",
            "temporarily-not-an-absolute-path",
            Some("connection-1"),
        );

        let resolved = resolve_project_bindings(
            &[configured],
            &[connection],
            &local_install,
            &HashMap::from([("ssh-project".to_string(), authoritative)]),
        );

        assert!(resolved.is_empty());
    }

    #[test]
    fn authoritative_rebind_replaces_nonempty_cold_provisional_state() {
        let mut config = AppConfig {
            projects: vec![project("ssh-project", "/srv/repo", Some("connection-1"))],
            ..AppConfig::default()
        };
        let shell_name = config.default_shell.clone();
        let old_route = route(TerminalIncarnationId::new());
        let new_route = route(TerminalIncarnationId::new());
        let old_layout = saved_layout(&shell_name, "/srv/repo/provisional", old_route.worktree_id);
        let mut destination_layout = saved_layout(
            &shell_name,
            "/srv/repo/authoritative",
            new_route.worktree_id.clone(),
        );
        destination_layout.tabs.extend(
            saved_layout(
                &shell_name,
                "/srv/repo/authoritative/selected",
                new_route.worktree_id.clone(),
            )
            .tabs,
        );
        let key_for_tab = |tab: &SavedTab| match &tab.split_layout {
            SavedSplitNode::Leaf { panes, .. } => panes[0].pane_key.clone().unwrap(),
            SavedSplitNode::Split { .. } => panic!("fixture must be a leaf"),
        };
        let selected = key_for_tab(&destination_layout.tabs[1]);
        let order = vec![selected.clone(), key_for_tab(&destination_layout.tabs[0])];
        destination_layout.selected_terminal_pane_key = Some(selected.clone());
        destination_layout.terminal_order = Some(order.clone());
        let selected_owner = destination_layout.tabs[1].tab_id.clone();
        let (panels, active_panel_id) = persist::restore_layout(&old_layout, &config);
        let mut state = ProjectState::new();
        state.panels = panels;
        state.active_panel_id = active_panel_id;
        state.restore_terminal_navigation(&old_layout);
        let old_pane_id = state.all_panes()[0].id.clone();
        state.maximized_pane_id = Some(old_pane_id);
        assert!(!state.panels.is_empty());
        assert!(
            state.pty_ids().is_empty(),
            "restored state must still be cold"
        );
        let mut states = HashMap::from([("ssh-project".to_string(), state)]);

        apply_reconciled_project_layout(
            &mut config,
            &mut states,
            "ssh-project",
            destination_layout,
            true,
        );

        let state = states.get("ssh-project").unwrap();
        assert_eq!(
            state.all_panes()[0].cwd.as_deref(),
            Some("/srv/repo/authoritative")
        );
        assert!(state.maximized_pane_id.is_none());
        assert_eq!(state.selected_terminal_pane_key.as_ref(), Some(&selected));
        assert_eq!(state.terminal_order, order);
        assert_eq!(
            state.active_panel().map(|panel| &panel.tab_id),
            selected_owner.as_ref()
        );
        let snapshot = state.saved_layout();
        assert_eq!(snapshot.selected_terminal_pane_key.as_ref(), Some(&selected));
        assert_eq!(snapshot.terminal_order.as_ref(), Some(&order));
        assert_eq!(snapshot.active_tab_index, 1);
        assert_eq!(
            config.projects[0]
                .saved_layout
                .as_ref()
                .and_then(|layout| layout.worktree_id.as_ref()),
            Some(&new_route.worktree_id)
        );
    }

    #[test]
    fn shared_alias_detection_uses_stable_worktree_id() {
        let first = binding_from_resolved(
            "p1".into(),
            worktree::resolve_provisional_ssh(&HostInstallId::new(), "ssh", "/repo").unwrap(),
        );
        let mut second = first.clone();
        second.project_id = "p2".into();
        let unrelated = binding_from_resolved(
            "p3".into(),
            worktree::resolve_provisional_ssh(&HostInstallId::new(), "ssh", "/other").unwrap(),
        );
        let bindings = HashMap::from([
            (first.project_id.clone(), first),
            (second.project_id.clone(), second),
            (unrelated.project_id.clone(), unrelated),
        ]);

        assert!(has_other_worktree_alias(&bindings, "p1"));
        assert!(has_other_worktree_alias(&bindings, "p2"));
        assert!(!has_other_worktree_alias(&bindings, "p3"));
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
