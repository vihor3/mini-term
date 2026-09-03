//! Project-scoped scheduling and stale-result fencing for SSH runtime identity.

use std::ffi::OsStr;

use gpui::Context;
use mt_ssh::RemoteRuntimeSnapshot;

use super::AppStore;
use super::identity::AuthoritativeBindingInstall;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteRuntimePhase {
    Connecting,
    Ready,
    CompatibilityFallback,
    RebindDeferred,
}

/// Read by the remote Agent and worktree diagnostics phases that follow this
/// transport foundation.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RemoteRuntimeProjectState {
    pub phase: RemoteRuntimePhase,
    pub snapshot: Option<RemoteRuntimeSnapshot>,
    pub error: Option<String>,
    generation: u64,
    connection_id: String,
    connection_fingerprint: u64,
    requested_path: String,
    hydrate_after: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteRuntimeRequest {
    project_id: String,
    generation: u64,
    connection_id: String,
    connection_fingerprint: u64,
    requested_path: String,
}

fn remote_runtime_enabled_value(value: Option<&OsStr>) -> bool {
    value != Some(OsStr::new("0"))
}

pub fn remote_runtime_enabled() -> bool {
    remote_runtime_enabled_value(std::env::var_os("MINI_TERM_REMOTE_RUNTIME").as_deref())
}

fn request_facts_match(
    request: &RemoteRuntimeRequest,
    state_generation: Option<u64>,
    current_path: Option<&str>,
    current_connection_id: Option<&str>,
    current_connection_fingerprint: Option<u64>,
) -> bool {
    state_generation == Some(request.generation)
        && current_path == Some(request.requested_path.as_str())
        && current_connection_id == Some(request.connection_id.as_str())
        && current_connection_fingerprint == Some(request.connection_fingerprint)
}

pub(super) fn allocate_generation(counter: &mut u64) -> Option<u64> {
    let next = counter.checked_add(1)?;
    *counter = next;
    Some(next)
}

fn connection_epoch_matches(result_epoch: u64, current_epoch: Option<u64>) -> bool {
    current_epoch == Some(result_epoch)
}

impl AppStore {
    #[allow(dead_code)]
    pub fn remote_runtime_state(&self, project_id: &str) -> Option<&RemoteRuntimeProjectState> {
        self.remote_runtime_projects.get(project_id)
    }

    /// Return true while remote identity must finish before a terminal is
    /// hydrated or spawned. Terminal callers simply stop and the completion
    /// path resumes restored-layout hydration when requested.
    pub(super) fn defer_remote_hydration(
        &mut self,
        project_id: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        self.request_remote_runtime(project_id, true, false, cx)
    }

    #[allow(dead_code)]
    pub fn retry_remote_runtime(&mut self, project_id: &str, cx: &mut Context<Self>) {
        self.remote_runtime_projects.remove(project_id);
        if !self.request_remote_runtime(project_id, true, true, cx) {
            self.hydrate_project(project_id, cx);
        }
        cx.notify();
    }

    pub(super) fn invalidate_remote_runtime_connection(&mut self, connection_id: &str) {
        self.invalidate_remote_agent_connection(connection_id);
        let affected = self
            .config
            .projects
            .iter()
            .filter(|project| project.ssh_connection_id.as_deref() == Some(connection_id))
            .map(|project| project.id.clone())
            .collect::<Vec<_>>();
        for project_id in affected {
            self.remote_runtime_projects.remove(&project_id);
        }
    }

    pub(super) fn remove_remote_runtime_project(&mut self, project_id: &str) {
        self.remove_remote_agent_project(project_id);
        self.remote_runtime_projects.remove(project_id);
    }

    pub(super) fn refresh_remote_runtime_for_agents(
        &mut self,
        project_id: &str,
        cx: &mut Context<Self>,
    ) {
        let _ = self.request_remote_runtime(project_id, false, true, cx);
    }

    fn request_remote_runtime(
        &mut self,
        project_id: &str,
        hydrate_after: bool,
        force: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if !remote_runtime_enabled() {
            self.remote_runtime_projects.remove(project_id);
            return false;
        }
        let Some(project) = self.project(project_id).cloned() else {
            self.remote_runtime_projects.remove(project_id);
            return false;
        };
        let Some(connection_id) = project.ssh_connection_id.clone() else {
            self.remote_runtime_projects.remove(project_id);
            return false;
        };
        let Some(connection) = self.remote_connection_of(project_id) else {
            self.remote_runtime_projects.insert(
                project_id.to_string(),
                RemoteRuntimeProjectState {
                    phase: RemoteRuntimePhase::CompatibilityFallback,
                    snapshot: None,
                    error: Some("SSH connection is missing".into()),
                    generation: self.next_remote_runtime_generation,
                    connection_id,
                    connection_fingerprint: 0,
                    requested_path: project.path,
                    hydrate_after: false,
                },
            );
            return false;
        };
        let connection_fingerprint = crate::remote_ssh::connection_fingerprint(&connection);

        if !force
            && let Some(state) = self.remote_runtime_projects.get_mut(project_id)
            && state.connection_id == connection.id
            && state.connection_fingerprint == connection_fingerprint
            && state.requested_path == project.path
        {
            match state.phase {
                RemoteRuntimePhase::Connecting => {
                    state.hydrate_after |= hydrate_after;
                    return true;
                }
                RemoteRuntimePhase::RebindDeferred => return true,
                RemoteRuntimePhase::Ready | RemoteRuntimePhase::CompatibilityFallback => {}
            }
            return false;
        }

        let Some(generation) = allocate_generation(&mut self.next_remote_runtime_generation) else {
            self.remote_runtime_projects.insert(
                project_id.to_string(),
                RemoteRuntimeProjectState {
                    phase: RemoteRuntimePhase::CompatibilityFallback,
                    snapshot: None,
                    error: Some("remote runtime request generation space exhausted".into()),
                    generation: u64::MAX,
                    connection_id: connection.id,
                    connection_fingerprint,
                    requested_path: project.path,
                    hydrate_after: false,
                },
            );
            return false;
        };
        let request = RemoteRuntimeRequest {
            project_id: project_id.to_string(),
            generation,
            connection_id: connection.id.clone(),
            connection_fingerprint,
            requested_path: project.path.clone(),
        };
        self.remote_runtime_projects.insert(
            project_id.to_string(),
            RemoteRuntimeProjectState {
                phase: RemoteRuntimePhase::Connecting,
                snapshot: None,
                error: None,
                generation,
                connection_id: connection.id.clone(),
                connection_fingerprint,
                requested_path: project.path.clone(),
                hydrate_after,
            },
        );

        let task_request = request.clone();
        let task_path = project.path;
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { crate::remote_ssh::runtime_snapshot(&connection, &task_path) })
                .await;
            let _ = this.update(cx, |store, cx| {
                store.finish_remote_runtime_request(task_request, outcome, cx)
            });
        })
        .detach();
        true
    }

    fn remote_runtime_request_is_current(&self, request: &RemoteRuntimeRequest) -> bool {
        let state_generation = self
            .remote_runtime_projects
            .get(&request.project_id)
            .filter(|state| state.phase == RemoteRuntimePhase::Connecting)
            .map(|state| state.generation);
        let current_project = self.project(&request.project_id);
        let current_connection = self.remote_connection_of(&request.project_id);
        request_facts_match(
            request,
            state_generation,
            current_project.map(|project| project.path.as_str()),
            current_connection
                .as_ref()
                .map(|connection| connection.id.as_str()),
            current_connection
                .as_ref()
                .map(crate::remote_ssh::connection_fingerprint),
        )
    }

    fn finish_remote_runtime_request(
        &mut self,
        request: RemoteRuntimeRequest,
        outcome: Result<RemoteRuntimeSnapshot, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.remote_runtime_request_is_current(&request) {
            return;
        }
        let outcome = match outcome {
            Ok(snapshot)
                if !connection_epoch_matches(
                    snapshot.identity.connection_epoch,
                    crate::remote_ssh::current_connection_epoch(&request.connection_id),
                ) =>
            {
                Err("remote runtime result was superseded by a newer SSH connection".into())
            }
            outcome => outcome,
        };
        let hydrate_after = self
            .remote_runtime_projects
            .get(&request.project_id)
            .is_some_and(|state| state.hydrate_after);

        let (phase, snapshot, error) = match outcome {
            Err(error) => (RemoteRuntimePhase::CompatibilityFallback, None, Some(error)),
            Ok(snapshot) => {
                match self.install_authoritative_remote_binding(&request.project_id, &snapshot, cx)
                {
                    AuthoritativeBindingInstall::Installed
                    | AuthoritativeBindingInstall::Unchanged => {
                        (RemoteRuntimePhase::Ready, Some(snapshot), None)
                    }
                    AuthoritativeBindingInstall::Deferred(reason) => (
                        RemoteRuntimePhase::RebindDeferred,
                        Some(snapshot),
                        Some(reason),
                    ),
                    AuthoritativeBindingInstall::Failed(error) => (
                        RemoteRuntimePhase::CompatibilityFallback,
                        Some(snapshot),
                        Some(error),
                    ),
                }
            }
        };
        let may_hydrate = phase != RemoteRuntimePhase::RebindDeferred;
        self.remote_runtime_projects.insert(
            request.project_id.clone(),
            RemoteRuntimeProjectState {
                phase,
                snapshot,
                error,
                generation: request.generation,
                connection_id: request.connection_id,
                connection_fingerprint: request.connection_fingerprint,
                requested_path: request.requested_path,
                hydrate_after: false,
            },
        );
        if hydrate_after && may_hydrate {
            self.hydrate_project(&request.project_id, cx);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> RemoteRuntimeRequest {
        RemoteRuntimeRequest {
            project_id: "project".into(),
            generation: 7,
            connection_id: "ssh".into(),
            connection_fingerprint: 11,
            requested_path: "/srv/repo".into(),
        }
    }

    #[test]
    fn rollback_gate_disables_only_explicit_zero() {
        assert!(!remote_runtime_enabled_value(Some(OsStr::new("0"))));
        assert!(remote_runtime_enabled_value(None));
        assert!(remote_runtime_enabled_value(Some(OsStr::new("1"))));
        assert!(remote_runtime_enabled_value(Some(OsStr::new("false"))));
    }

    #[test]
    fn request_fence_rejects_every_changed_owner_fact() {
        let request = request();
        assert!(request_facts_match(
            &request,
            Some(7),
            Some("/srv/repo"),
            Some("ssh"),
            Some(11),
        ));
        assert!(!request_facts_match(
            &request,
            Some(8),
            Some("/srv/repo"),
            Some("ssh"),
            Some(11),
        ));
        assert!(!request_facts_match(
            &request,
            Some(7),
            Some("/srv/other"),
            Some("ssh"),
            Some(11),
        ));
        assert!(!request_facts_match(
            &request,
            Some(7),
            Some("/srv/repo"),
            Some("other"),
            Some(11),
        ));
        assert!(!request_facts_match(
            &request,
            Some(7),
            Some("/srv/repo"),
            Some("ssh"),
            Some(12),
        ));
        assert!(!request_facts_match(
            &request,
            None,
            Some("/srv/repo"),
            Some("ssh"),
            Some(11),
        ));
    }

    #[test]
    fn request_generations_are_monotonic_and_fail_on_overflow() {
        let mut generation = 0;
        assert_eq!(allocate_generation(&mut generation), Some(1));
        assert_eq!(allocate_generation(&mut generation), Some(2));
        generation = u64::MAX;
        assert_eq!(allocate_generation(&mut generation), None);
        assert_eq!(generation, u64::MAX);
    }

    #[test]
    fn completion_epoch_must_equal_the_current_connection_epoch() {
        assert!(connection_epoch_matches(9, Some(9)));
        assert!(!connection_epoch_matches(8, Some(9)));
        assert!(!connection_epoch_matches(10, Some(9)));
        assert!(!connection_epoch_matches(9, None));
    }
}
