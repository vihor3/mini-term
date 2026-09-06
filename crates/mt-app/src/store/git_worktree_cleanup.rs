//! Retained deletion authority for Git worktree cleanup, separate from Git effects.

use futures::channel::oneshot;
use gpui::{App, Context, Task};
use mt_layout::ProjectWorktreeBinding;
use serde_json::Value;

use super::panes::TerminalCloseRequest;
use super::{AppStore, ProjectLocationKey, ProjectState, TerminalJumpTarget};
use crate::execution_host::{ExecutionSourceSignature, ProjectExecutionSnapshot};
use crate::git_backend::GitLifetime;

const STALE: &str = "Worktree projects, documents or terminals changed. Configuration was kept; review the target again.";

#[derive(Clone, Debug, PartialEq, Eq)]
struct TerminalRecord {
    target: TerminalJumpTarget,
    pty_id: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AliasAuthority {
    id: String,
    config: Value,
    binding: ProjectWorktreeBinding,
    source: ExecutionSourceSignature,
    layout: Option<Value>,
    saved_layout: Option<Value>,
    records: Vec<TerminalRecord>,
}

/// Opaque and cloneable for pre-confirmation capture. Only this module can
/// advance its expected inventory after an exactly observed owned close.
#[derive(Clone)]
pub(crate) struct GitWorktreeRemovalGuard {
    location: ProjectLocationKey,
    aliases: Vec<AliasAuthority>,
    requests: Vec<(TerminalJumpTarget, TerminalCloseRequest)>,
}

impl GitWorktreeRemovalGuard {
    pub(crate) fn matches_location(&self, location: &ProjectLocationKey) -> bool {
        &self.location == location
    }

    pub(crate) fn terminals_empty(&self) -> bool {
        self.aliases.iter().all(|alias| alias.records.is_empty())
    }

    fn same_authority(&self, current: &Self) -> bool {
        self.location == current.location && self.aliases == current.aliases
    }
}

fn binding_matches_location(
    project: &mt_config::ProjectConfig,
    trusted_canonical_path: Option<&str>,
    location: &ProjectLocationKey,
) -> bool {
    let Some(canonical) = trusted_canonical_path else {
        return false;
    };
    // Registration may match a configured alias. Destruction must additionally
    // match its retained binding, without falling back to that configured path.
    let mut bound = project.clone();
    bound.path = if project.ssh_connection_id.is_none()
        && let Some(wsl) = mt_core::parse_wsl_unc(&project.path.replace('/', "\\"))
    {
        let path = if let Some(bound_wsl) = mt_core::parse_wsl_unc(&canonical.replace('/', "\\")) {
            if !wsl.distro.eq_ignore_ascii_case(&bound_wsl.distro) {
                return false;
            }
            bound_wsl.unix_path
        } else {
            canonical.to_string()
        };
        let Ok(path) = crate::execution_host::normalize_absolute_posix_path(&path) else {
            return false;
        };
        let Ok(path) = (crate::project_onboarding::DirectoryLocation {
            source: crate::project_onboarding::DirectorySource::Wsl { distro: wsl.distro },
            path,
        })
        .host_path() else {
            return false;
        };
        path
    } else {
        if project.ssh_connection_id.is_none()
            && mt_core::parse_wsl_unc(&canonical.replace('/', "\\")).is_some()
        {
            return false;
        }
        canonical.to_string()
    };
    super::projects::project_matches_location(&bound, None, location)
}

fn close_source_is_current(
    lifetime: &GitLifetime,
    expected: &ProjectExecutionSnapshot,
    current: Option<&ProjectExecutionSnapshot>,
) -> bool {
    lifetime.is_valid()
        && current.is_some_and(|current| {
            expected.project_id == current.project_id
                && expected.source_signature() == current.source_signature()
        })
}

fn dormant_alias_of(record: &TerminalRecord, target: &TerminalJumpTarget) -> bool {
    record.pty_id.is_none()
        && record.target.execution_host_id == target.execution_host_id
        && record.target.worktree_id == target.worktree_id
        && record.target.tab_id == target.tab_id
        && record.target.pane_key == target.pane_key
        && record.target.terminal_session_id == target.terminal_session_id
        && record.target.terminal_incarnation_id == target.terminal_incarnation_id
}

fn next_close<'a>(
    records: impl Iterator<Item = &'a TerminalRecord>,
    selected: Option<&TerminalJumpTarget>,
) -> Option<&'a TerminalRecord> {
    let mut sessions = std::collections::BTreeMap::new();
    for record in records {
        let rank = |record: &TerminalRecord| {
            (
                record.pty_id.is_none(),
                selected.is_none_or(|selected| selected.project_id != record.target.project_id),
            )
        };
        let entry = sessions
            .entry(record.target.terminal_session_id.as_str())
            .or_insert(record);
        if rank(record) < rank(entry) {
            *entry = record;
        }
    }
    sessions
        .into_values()
        .min_by_key(|record| selected == Some(&record.target))
}

fn retained_state(state: &ProjectState) -> ProjectState {
    ProjectState {
        panels: state.panels.clone(),
        active_panel_id: state.active_panel_id.clone(),
        selected_terminal_pane_key: state.selected_terminal_pane_key.clone(),
        terminal_order: state.terminal_order.clone(),
        status: state.status,
        needs_attention: state.needs_attention,
        maximized_pane_id: state.maximized_pane_id.clone(),
    }
}

fn expected_alias_removal(
    alias: &AliasAuthority,
    state: &ProjectState,
    target: &TerminalJumpTarget,
) -> Result<AliasAuthority, String> {
    if alias.id != target.project_id || !alias.records.iter().any(|record| &record.target == target)
    {
        return Err(STALE.into());
    }
    let mut expected = alias.clone();
    let mut state = retained_state(state);
    state.remove_pane(target.pane_key.as_str());
    state.normalize_terminal_navigation(None);
    let mut saved = state.saved_layout();
    expected.layout = Some(serde_json::to_value(&saved).map_err(|_| STALE)?);
    saved.worktree_id = Some(target.worktree_id.clone());
    expected.saved_layout = Some(serde_json::to_value(saved).map_err(|_| STALE)?);
    expected.records.retain(|record| &record.target != target);
    Ok(expected)
}

impl AppStore {
    pub(crate) fn prepare_git_worktree_removal(
        &self,
        location: &ProjectLocationKey,
        cx: &App,
    ) -> Result<GitWorktreeRemovalGuard, String> {
        let ids = self.project_ids_for_location(location);
        let mut aliases = Vec::new();
        let mut requests = Vec::new();
        for id in &ids {
            if crate::workbench_area::project_has_dirty_documents(id, cx) {
                return Err(crate::i18n::t("fileViewer", "projectRemovalBlocked").to_string());
            }
            let project = self.project(id).ok_or(STALE)?;
            if !binding_matches_location(
                project,
                self.onboarding_canonical_path_for_project(project),
                location,
            ) {
                return Err(STALE.into());
            }
            let binding = self.project_worktree_bindings.get(id).ok_or(STALE)?.clone();
            if binding.project_id != *id {
                return Err(STALE.into());
            }
            // A location alias cannot authorize deletion of another location
            // merely because a stale binding happens to reuse its WorktreeId.
            if self.project_worktree_bindings.values().any(|other| {
                other.worktree_id == binding.worktree_id && !ids.contains(&other.project_id)
            }) {
                return Err(STALE.into());
            }
            if self.config.projects.iter().any(|child| {
                child.parent_project_id.as_deref() == Some(id.as_str()) && !ids.contains(&child.id)
            }) {
                return Err("Other configured worktrees still use this project as their parent. Configuration was kept.".into());
            }
            let mut records = Vec::new();
            let state = self.project_states.get(id);
            if let Some(state) = state {
                for pane in state.all_panes() {
                    let target = self
                        .terminal_jump_target_for_pane(id, &pane.id)
                        .ok_or(STALE)?;
                    let request = self.terminal_close_request(&target).ok_or(STALE)?;
                    records.push(TerminalRecord {
                        target: target.clone(),
                        pty_id: pane.pty_id,
                    });
                    requests.push((target, request));
                }
            } else if project.saved_layout.is_some() {
                // Never hydrate a missing runtime bucket to discover its records.
                return Err(
                    "Saved worktree layout is unavailable; project configuration was kept.".into(),
                );
            }
            records.sort_by(|a, b| a.target.pane_key.as_str().cmp(b.target.pane_key.as_str()));
            if self.terminal_routes.iter().any(|(pty_id, route)| {
                route.worktree_id == binding.worktree_id
                    && !ids.iter().any(|id| {
                        self.project_states.get(id).is_some_and(|state| {
                            state
                                .all_panes()
                                .iter()
                                .any(|pane| pane.pty_id == Some(*pty_id))
                        })
                    })
            }) {
                return Err(STALE.into());
            }
            aliases.push(AliasAuthority {
                id: id.clone(),
                config: serde_json::to_value(project).map_err(|_| STALE)?,
                source: self.project_execution_snapshot(id)?.source_signature(),
                binding,
                layout: state
                    .map(|state| serde_json::to_value(state.saved_layout()))
                    .transpose()
                    .map_err(|_| STALE)?,
                // ProjectConfig deliberately skips saved_layout during serde.
                saved_layout: project
                    .saved_layout
                    .as_ref()
                    .map(serde_json::to_value)
                    .transpose()
                    .map_err(|_| STALE)?,
                records,
            });
        }
        Ok(GitWorktreeRemovalGuard {
            location: location.clone(),
            aliases,
            requests,
        })
    }

    pub(crate) fn git_worktree_removal_is_current(
        &self,
        guard: &GitWorktreeRemovalGuard,
        cx: &App,
    ) -> bool {
        self.prepare_git_worktree_removal(&guard.location, cx)
            .is_ok_and(|current| guard.same_authority(&current))
    }

    fn expected_git_terminal_removal(
        &self,
        guard: &GitWorktreeRemovalGuard,
        target: &TerminalJumpTarget,
    ) -> Result<GitWorktreeRemovalGuard, String> {
        let mut expected = guard.clone();
        let alias = expected
            .aliases
            .iter_mut()
            .find(|alias| alias.id == target.project_id)
            .ok_or(STALE)?;
        *alias = expected_alias_removal(
            alias,
            self.project_states.get(&target.project_id).ok_or(STALE)?,
            target,
        )?;
        Ok(expected)
    }

    /// The detached worker finishes dispatched closes even after cancellation,
    /// but cancellation forbids another dispatch. A close bool is only a focus hint.
    pub(crate) fn close_git_worktree_terminals(
        &mut self,
        mut guard: GitWorktreeRemovalGuard,
        source: ProjectExecutionSnapshot,
        lifetime: GitLifetime,
        cx: &mut Context<Self>,
    ) -> Task<Result<GitWorktreeRemovalGuard, String>> {
        if !self.git_worktree_removal_is_current(&guard, cx) {
            return Task::ready(Err(STALE.into()));
        }
        let (send, receive) = oneshot::channel();
        cx.spawn(async move |this, cx| {
            let result = async {
                while !guard.terminals_empty() {
                    let (target, expected, close) = this
                        .update(cx, |store, cx| {
                            if !close_source_is_current(
                                &lifetime,
                                &source,
                                store
                                    .project_execution_snapshot(&source.project_id)
                                    .ok()
                                    .as_ref(),
                            ) || !store.git_worktree_removal_is_current(&guard, cx)
                            {
                                return Err(STALE.to_string());
                            }
                            // Closing selected last avoids ordinary close's neighbor
                            // hydration. Prefer the actual attachment's alias.
                            let selected = store.active_project_id.as_deref().and_then(|id| {
                                store
                                    .active_pane_id(id)
                                    .and_then(|pane| store.terminal_jump_target_for_pane(id, &pane))
                            });
                            let record = next_close(
                                guard.aliases.iter().flat_map(|alias| &alias.records),
                                selected.as_ref(),
                            )
                            .ok_or(STALE)?;
                            let target = record.target.clone();
                            let request = guard
                                .requests
                                .iter()
                                .find(|(owner, _)| owner == &target)
                                .ok_or(STALE)?
                                .1
                                .clone();
                            let expected = store.expected_git_terminal_removal(&guard, &target)?;
                            let close = store.close_terminal_target(request, cx);
                            Ok((target, expected, close))
                        })
                        .map_err(|_| STALE.to_string())??;
                    let _focus_handoff = close.await;
                    guard = this
                        .update(cx, |store, cx| {
                            let mut current =
                                store.prepare_git_worktree_removal(&guard.location, cx)?;
                            if !expected.same_authority(&current) {
                                return Err(STALE.into());
                            }
                            // The host close already succeeded for this logical
                            // session. Identical dormant aliases may follow that
                            // exact removal, never issue another Kill for history.
                            let duplicates = current
                                .aliases
                                .iter()
                                .flat_map(|alias| &alias.records)
                                .filter(|record| {
                                    record.target.terminal_session_id == target.terminal_session_id
                                })
                                .cloned()
                                .collect::<Vec<_>>();
                            for record in duplicates {
                                if !dormant_alias_of(&record, &target) {
                                    return Err(STALE.into());
                                }
                                let expected = store
                                    .expected_git_terminal_removal(&current, &record.target)?;
                                store
                                    .project_states
                                    .get_mut(&record.target.project_id)
                                    .ok_or(STALE)?
                                    .remove_pane(record.target.pane_key.as_str());
                                store.after_layout_change(&record.target.project_id, cx);
                                current =
                                    store.prepare_git_worktree_removal(&guard.location, cx)?;
                                if !expected.same_authority(&current) {
                                    return Err(STALE.into());
                                }
                            }
                            // Keep the confirmed source as the shared-layout save
                            // owner; a mirrored alias must not obstruct its next close.
                            store.save_project_layout_soon(&target.project_id, cx);
                            store.prepare_git_worktree_removal(&guard.location, cx)
                        })
                        .map_err(|_| STALE.to_string())??;
                }
                Ok(guard)
            }
            .await;
            let _ = send.send(result);
        })
        .detach();
        cx.spawn(async move |_, _| receive.await.unwrap_or_else(|_| Err(STALE.into())))
    }

    /// The caller separately proves a normal Git removal/prune postcondition
    /// and its live dialog/source. No Git uncertainty is interpreted here.
    pub(crate) fn finish_git_worktree_removal(
        &mut self,
        guard: GitWorktreeRemovalGuard,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !guard.terminals_empty() || !self.git_worktree_removal_is_current(&guard, cx) {
            return Err(STALE.into());
        }
        // No await: validate the whole group once, then remove only its empty
        // captured aliases. Removing one alias changes the others' root context.
        for alias in &guard.aliases {
            self.remove_project(&alias.id, cx);
            if self.project(&alias.id).is_some() {
                return Err(STALE.into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_host::ExecutionBackendSignature;
    use crate::tree::{PaneState, ProjectPanel, SplitNode};
    use mt_identity::{
        ExecutionHostId, HostInstallId, PaneKey, RepoId, TabId, TerminalIncarnationId,
        TerminalSessionId, WorktreeId,
    };

    fn record() -> TerminalRecord {
        let host = ExecutionHostId::derive("guard-test", &HostInstallId::new());
        let repo = RepoId::derive(&host, "/repo/.git");
        TerminalRecord {
            target: TerminalJumpTarget {
                project_id: "source".into(),
                execution_host_id: host,
                worktree_id: WorktreeId::derive(&repo, "/repo/wt", None),
                tab_id: TabId::new(),
                pane_key: PaneKey::new(),
                terminal_session_id: TerminalSessionId::new(),
                terminal_incarnation_id: Some(TerminalIncarnationId::new()),
            },
            pty_id: None,
        }
    }

    fn guard(record: TerminalRecord) -> GitWorktreeRemovalGuard {
        let target = &record.target;
        let binding = ProjectWorktreeBinding {
            project_id: target.project_id.clone(),
            execution_host_id: target.execution_host_id.clone(),
            repo_id: RepoId::derive(&target.execution_host_id, "/repo/.git"),
            worktree_id: target.worktree_id.clone(),
            identity_source: "authoritative-test".into(),
            canonical_worktree_path: Some("/repo/wt".into()),
            identity_context: None,
        };
        let source = ExecutionSourceSignature {
            execution_host_id: target.execution_host_id.clone(),
            root_project_id: "root".into(),
            root_source_path: "/repo".into(),
            worktree_id: target.worktree_id.clone(),
            canonical_path: "/repo/wt".into(),
            backend: ExecutionBackendSignature::Ssh {
                connection_id: "ssh".into(),
                connection_fingerprint: 1,
                connection_epoch: Some(7),
            },
        };
        GitWorktreeRemovalGuard {
            location: ProjectLocationKey::Ssh {
                connection_id: "ssh".into(),
                normalized_posix_path: "/repo/wt".into(),
            },
            aliases: vec![AliasAuthority {
                id: target.project_id.clone(),
                config: serde_json::json!({"path": "/repo/wt"}),
                binding,
                source,
                layout: Some(serde_json::json!({"panes": ["original"]})),
                saved_layout: None,
                records: vec![record],
            }],
            requests: Vec::new(),
        }
    }

    #[test]
    fn guard_rejects_location_alias_configuration_binding_layout_and_epoch_changes() {
        let original = guard(record());
        assert!(original.same_authority(&original.clone()));
        assert!(!original.terminals_empty());
        let mut changed = vec![original.clone(); 10];
        changed[0].location = ProjectLocationKey::Local {
            normalized_canonical_path: "/repo/wt".into(),
        };
        changed[1].aliases.push(original.aliases[0].clone());
        changed[2].aliases.clear();
        changed[3].aliases[0].config = serde_json::json!({"path": "/replacement"});
        changed[4].aliases[0].binding.identity_context = Some("replacement".into());
        changed[5].aliases[0].layout = None;
        changed[6].aliases[0].source = original.aliases[0].source.with_connection_epoch(Some(8));
        changed[7].aliases[0].records[0].pty_id = Some(10);
        changed[8].aliases[0].records[0]
            .target
            .terminal_incarnation_id = Some(TerminalIncarnationId::new());
        changed[9].aliases[0].saved_layout = Some(serde_json::json!({"panes": ["replacement"]}));
        for current in changed {
            assert!(!original.same_authority(&current));
        }
    }

    #[test]
    fn cleanup_requires_the_bound_location_not_a_repointed_configured_alias() {
        let mut project: mt_config::ProjectConfig = serde_json::from_value(serde_json::json!({
            "id": "project", "name": "project", "path": "/repo/target"
        }))
        .unwrap();
        let local = ProjectLocationKey::Local {
            normalized_canonical_path: crate::execution_host::normalize_host_visible_project_path(
                "/repo/target",
            )
            .unwrap(),
        };
        assert!(super::super::projects::project_matches_location(
            &project,
            Some("/repo/other"),
            &local
        ));
        assert!(!binding_matches_location(
            &project,
            Some("/repo/other"),
            &local
        ));
        assert!(!binding_matches_location(&project, None, &local));
        assert!(binding_matches_location(
            &project,
            Some("/repo/target"),
            &local
        ));
        project.path = "/configured/alias".into();
        assert!(binding_matches_location(
            &project,
            Some("/repo/target"),
            &local
        ));
        project.ssh_connection_id = Some("ssh-a".into());
        let remote = ProjectLocationKey::Ssh {
            connection_id: "ssh-a".into(),
            normalized_posix_path: "/repo/target".into(),
        };
        assert!(binding_matches_location(
            &project,
            Some("/repo/target"),
            &remote
        ));
        assert!(!binding_matches_location(
            &project,
            Some("/repo/other"),
            &remote
        ));
        assert!(!binding_matches_location(
            &project,
            Some("/repo/target"),
            &local
        ));
        project.ssh_connection_id = Some("ssh-b".into());
        assert!(!binding_matches_location(
            &project,
            Some("/repo/target"),
            &remote
        ));
    }

    #[test]
    fn cleanup_bound_wsl_path_keeps_distribution_case_and_native_host_separate() {
        let project: mt_config::ProjectConfig = serde_json::from_value(serde_json::json!({
            "id": "wsl", "name": "wsl", "path": r"\\wsl$\Ubuntu\configured\alias"
        }))
        .unwrap();
        let location = ProjectLocationKey::Local {
            normalized_canonical_path: "wsl:ubuntu:/srv/Repo".into(),
        };
        assert!(binding_matches_location(
            &project,
            Some("/srv/Repo"),
            &location
        ));
        assert!(binding_matches_location(
            &project,
            Some(r"\\wsl.localhost\Ubuntu\srv\Repo"),
            &location
        ));
        assert!(!binding_matches_location(
            &project,
            Some(r"\\wsl.localhost\Debian\srv\Repo"),
            &location
        ));
        assert!(!binding_matches_location(
            &project,
            Some("/srv/repo"),
            &location
        ));
        let native = ProjectLocationKey::Local {
            normalized_canonical_path: crate::execution_host::normalize_host_visible_project_path(
                "/srv/Repo",
            )
            .unwrap(),
        };
        assert!(!binding_matches_location(
            &project,
            Some("/srv/Repo"),
            &native
        ));
        let different_directory = ProjectLocationKey::Local {
            normalized_canonical_path: "wsl:ubuntu:/srv/Repo/other".into(),
        };
        assert!(!binding_matches_location(
            &project,
            Some(r"/srv/Repo\other"),
            &different_directory
        ));
    }

    #[test]
    fn cancelled_or_changed_removal_owner_cannot_dispatch_the_next_close() {
        let target = record().target;
        let source = ProjectExecutionSnapshot {
            project_id: target.project_id.clone(),
            root_project_id: "root".into(),
            root_source_path: "/repo".into(),
            worktree_id: target.worktree_id,
            execution_host_id: target.execution_host_id,
            canonical_path: "/repo".into(),
            host_label: "Local".into(),
            backend: crate::execution_host::ExecutionBackend::Local,
        };
        let lifetime = GitLifetime::new();
        assert!(close_source_is_current(&lifetime, &source, Some(&source)));
        let mut changed = source.clone();
        changed.root_source_path = "/replacement".into();
        assert!(!close_source_is_current(&lifetime, &source, Some(&changed)));
        changed = source.clone();
        changed.project_id = "replacement-alias".into();
        assert!(!close_source_is_current(&lifetime, &source, Some(&changed)));
        assert!(!close_source_is_current(&lifetime, &source, None));
        let dispatched_owner = lifetime.clone();
        lifetime.invalidate();
        assert!(!close_source_is_current(
            &dispatched_owner,
            &source,
            Some(&source)
        ));
        assert!(close_source_is_current(
            &GitLifetime::new(),
            &source,
            Some(&source)
        ));
    }

    #[test]
    fn predicted_close_updates_runtime_and_skipped_config_layout_without_mutating_inventory() {
        let first = PaneState::new("first");
        let second = PaneState::new("second");
        let second_id = second.id.clone();
        let mut record = record();
        record.target.pane_key = first.pane_key.clone();
        record.target.terminal_session_id = first.terminal_session_id.clone();
        record.target.terminal_incarnation_id = first.terminal_incarnation_id.clone();
        let target = record.target.clone();
        let mut second_record = record.clone();
        second_record.target.pane_key = second.pane_key.clone();
        second_record.target.terminal_session_id = second.terminal_session_id.clone();
        second_record.target.terminal_incarnation_id = second.terminal_incarnation_id.clone();
        let mut original = guard(record);
        original.aliases[0].records.push(second_record.clone());
        let mut state = ProjectState::new();
        let mut layout = SplitNode::leaf(first);
        layout.append_pane(Some(target.pane_key.as_str()), second);
        state
            .panels
            .push(ProjectPanel::with_tab_id(target.tab_id.clone(), layout));
        state.select_terminal(&second_id);
        let mut captured = state.saved_layout();
        original.aliases[0].layout = Some(serde_json::to_value(&captured).unwrap());
        captured.worktree_id = Some(target.worktree_id.clone());
        original.aliases[0].saved_layout = Some(serde_json::to_value(captured).unwrap());
        let expected = expected_alias_removal(&original.aliases[0], &state, &target).unwrap();
        assert_eq!(expected.config, original.aliases[0].config);
        assert_eq!(expected.records, vec![second_record]);
        assert!(state.pane(target.pane_key.as_str()).is_some());
        let mut after = retained_state(&state);
        after.remove_pane(target.pane_key.as_str());
        assert_eq!(after.selected_terminal().unwrap().id, second_id);
        assert!(after.all_panes().iter().all(|pane| pane.pty_id.is_none()));
        let mut saved = after.saved_layout();
        assert_eq!(expected.layout, Some(serde_json::to_value(&saved).unwrap()));
        saved.worktree_id = Some(target.worktree_id.clone());
        assert_eq!(
            expected.saved_layout,
            Some(serde_json::to_value(saved).unwrap())
        );
        let mut changed = target.clone();
        changed.terminal_session_id = TerminalSessionId::new();
        assert!(expected_alias_removal(&original.aliases[0], &state, &changed).is_err());
    }

    #[test]
    fn only_the_expected_removal_can_advance_a_retained_inventory() {
        let original = guard(record());
        let mut expected = original.clone();
        expected.aliases[0].records.clear();
        expected.aliases[0].layout = Some(serde_json::json!({"panes": []}));
        assert!(expected.terminals_empty());
        assert!(expected.same_authority(&expected.clone()));
        assert!(
            !expected.same_authority(&original),
            "a failed close is not absence"
        );
        let mut replacement = expected.clone();
        replacement.aliases[0].records.push(record());
        assert!(!expected.same_authority(&replacement));
        let mut alias_change = expected.clone();
        alias_change.aliases[0].config =
            serde_json::json!({"path": "/repo/wt", "name": "new owner"});
        assert!(!expected.same_authority(&alias_change));
    }

    #[test]
    fn dormant_alias_projection_rejects_every_replacement_route_field() {
        let original = record();
        let mut alias = original.clone();
        alias.target.project_id = "another-alias".into();
        assert!(dormant_alias_of(&alias, &original.target));
        let mut variants = vec![alias; 7];
        variants[0].pty_id = Some(1);
        variants[1].target.execution_host_id =
            ExecutionHostId::derive("other", &HostInstallId::new());
        variants[2].target.worktree_id = record().target.worktree_id;
        variants[3].target.tab_id = TabId::new();
        variants[4].target.pane_key = PaneKey::new();
        variants[5].target.terminal_session_id = TerminalSessionId::new();
        variants[6].target.terminal_incarnation_id = Some(TerminalIncarnationId::new());
        for variant in variants {
            assert!(!dormant_alias_of(&variant, &original.target));
        }
    }

    #[test]
    fn close_order_deduplicates_aliases_prefers_the_attachment_and_leaves_selected_last() {
        let mut selected = record();
        selected.pty_id = Some(1);
        let mut alias = selected.clone();
        alias.pty_id = None;
        alias.target.project_id = "alias".into();
        let background = record();
        let records = [alias.clone(), selected.clone(), background.clone()];
        assert_eq!(
            next_close(records.iter(), Some(&selected.target)),
            Some(&background)
        );
        let records = [alias, selected.clone()];
        assert_eq!(
            next_close(records.iter(), Some(&selected.target)),
            Some(&selected)
        );
        assert_eq!(
            next_close(records.iter().rev(), Some(&selected.target)),
            Some(&selected)
        );
    }

    #[test]
    fn predicted_background_removal_preserves_selection_and_never_hydrates_records() {
        let first = PaneState::new("first");
        let first_id = first.id.clone();
        let second = PaneState::new("second");
        let second_id = second.id.clone();
        let mut state = ProjectState::new();
        let mut layout = SplitNode::leaf(first);
        layout.append_pane(Some(&first_id), second);
        state
            .panels
            .push(ProjectPanel::with_tab_id(TabId::new(), layout));
        state.select_terminal(&second_id);
        let mut expected = retained_state(&state);
        expected.remove_pane(&first_id);
        assert_eq!(expected.selected_terminal().unwrap().id, second_id);
        assert!(
            expected
                .all_panes()
                .iter()
                .all(|pane| pane.pty_id.is_none())
        );
        assert!(
            state.pane(&first_id).is_some(),
            "prediction must not mutate live inventory"
        );
        expected.remove_pane(&second_id);
        assert!(expected.all_panes().is_empty());
        assert!(expected.selected_terminal().is_none());
    }
}
