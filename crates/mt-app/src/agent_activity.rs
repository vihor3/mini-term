//! Pure projection for the global exact-run Agent activity feed.
//!
//! Live truth stays in `AppStore::agent_target_views()`. This module only groups,
//! orders, and bounds those immutable rows for the global overlay.

use std::cmp::Ordering;
use std::ffi::OsStr;

use mt_ai::{AgentActivity, AgentConnectivity};

use crate::store::AgentTargetView;

pub(crate) const AGENT_ACTIVITY_RECENT_LIMIT: usize = 40;

#[derive(Debug, Default)]
pub(crate) struct AgentActivityFeed {
    pub needs_you: Vec<AgentTargetView>,
    pub working: Vec<AgentTargetView>,
    pub recent: Vec<AgentTargetView>,
}

impl AgentActivityFeed {
    pub fn is_empty(&self) -> bool {
        self.needs_you.is_empty() && self.working.is_empty() && self.recent.is_empty()
    }
}

/// Only the exact value `0` rolls back the global entry and overlay. Inline
/// worktree rows, Sessions, runtime reconciliation, and exact activation remain
/// available because they do not consult this presentation gate.
pub(crate) fn global_agent_activity_enabled() -> bool {
    global_agent_activity_enabled_for(
        std::env::var_os("MINI_TERM_GLOBAL_AGENT_ACTIVITY").as_deref(),
    )
}

fn global_agent_activity_enabled_for(value: Option<&OsStr>) -> bool {
    value.is_none_or(|value| value != "0")
}

pub(crate) fn agent_target_needs_user(target: &AgentTargetView) -> bool {
    target.attention
        || matches!(
            target.activity,
            AgentActivity::Blocked | AgentActivity::Failed
        )
        || (target.unread
            && matches!(
                target.activity,
                AgentActivity::Done | AgentActivity::Waiting
            ))
}

fn target_section_rank(target: &AgentTargetView) -> u8 {
    if agent_target_needs_user(target) {
        0
    } else if target.connectivity == AgentConnectivity::Live
        && matches!(
            target.activity,
            AgentActivity::Starting | AgentActivity::Working
        )
    {
        1
    } else {
        2
    }
}

fn compare_targets(left: &AgentTargetView, right: &AgentTargetView) -> Ordering {
    right
        .received_at_unix_ms
        .cmp(&left.received_at_unix_ms)
        .then_with(|| left.root_project_name.cmp(&right.root_project_name))
        .then_with(|| left.project_id.cmp(&right.project_id))
        .then_with(|| {
            left.route
                .execution_host_id
                .cmp(&right.route.execution_host_id)
        })
        .then_with(|| left.route.worktree_id.cmp(&right.route.worktree_id))
        .then_with(|| left.pane_label.cmp(&right.pane_label))
        .then_with(|| left.pane_id.cmp(&right.pane_id))
        .then_with(|| left.provider.cmp(&right.provider))
        .then_with(|| left.run_id.cmp(&right.run_id))
}

pub(crate) fn build_agent_activity_feed(
    mut targets: Vec<AgentTargetView>,
    recent_limit: usize,
) -> AgentActivityFeed {
    targets.sort_by(|left, right| {
        target_section_rank(left)
            .cmp(&target_section_rank(right))
            .then_with(|| compare_targets(left, right))
    });

    let mut feed = AgentActivityFeed::default();
    for target in targets {
        match target_section_rank(&target) {
            0 => feed.needs_you.push(target),
            1 => feed.working.push(target),
            _ if feed.recent.len() < recent_limit => feed.recent.push(target),
            _ => {}
        }
    }
    feed
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt_ai::{AgentEvidence, AgentProvider, AgentRoute};
    use mt_identity::{
        AgentEventId, AgentRunId, ExecutionHostId, HostInstallId, PaneKey, RepoId, TabId,
        TerminalIncarnationId, TerminalSessionId, WorktreeId,
    };

    fn run_id(value: u32) -> AgentRunId {
        format!("agent-run-v1:00000000-0000-4000-8000-{value:012}")
            .parse()
            .unwrap()
    }

    fn route(host_name: &str, worktree_path: &str) -> AgentRoute {
        let host = ExecutionHostId::derive(host_name, &HostInstallId::new());
        let repo = RepoId::derive(&host, "/repo/.git");
        AgentRoute {
            execution_host_id: host,
            worktree_id: WorktreeId::derive(&repo, worktree_path, None),
            tab_id: TabId::new(),
            pane_key: PaneKey::new(),
            terminal_session_id: TerminalSessionId::new(),
            terminal_incarnation_id: TerminalIncarnationId::new(),
        }
    }

    fn target(
        id: u32,
        project: &str,
        activity: AgentActivity,
        connectivity: AgentConnectivity,
        unread: bool,
        received_at_unix_ms: i64,
    ) -> AgentTargetView {
        AgentTargetView {
            run_id: run_id(id),
            last_event_id: AgentEventId::new(),
            project_id: project.to_string(),
            project_name: project.to_string(),
            root_project_name: project.to_string(),
            worktree_name: format!("{project}-worktree"),
            host_label: format!("{project}-host"),
            pane_id: format!("pane-{id}"),
            pane_label: format!("Pane {id}"),
            route: route(project, &format!("/repo/{project}")),
            provider: "codex".parse::<AgentProvider>().unwrap(),
            provider_session_id: Some(format!("session-{id}")),
            activity,
            connectivity,
            evidence: AgentEvidence::Hook,
            received_at_unix_ms,
            attention: false,
            unread,
        }
    }

    #[test]
    fn groups_needs_you_working_and_recent_without_collapsing_connectivity() {
        let mut waiting = target(
            1,
            "alpha",
            AgentActivity::Waiting,
            AgentConnectivity::Disconnected,
            true,
            10,
        );
        waiting.attention = false;
        let working = target(
            2,
            "beta",
            AgentActivity::Working,
            AgentConnectivity::Live,
            false,
            20,
        );
        let disconnected_working = target(
            3,
            "gamma",
            AgentActivity::Working,
            AgentConnectivity::Disconnected,
            false,
            30,
        );
        let failed = target(
            4,
            "delta",
            AgentActivity::Failed,
            AgentConnectivity::Disconnected,
            false,
            40,
        );

        let feed = build_agent_activity_feed(
            vec![disconnected_working, working, waiting, failed],
            AGENT_ACTIVITY_RECENT_LIMIT,
        );

        assert_eq!(
            feed.needs_you
                .iter()
                .map(|target| target.run_id.clone())
                .collect::<Vec<_>>(),
            vec![run_id(4), run_id(1)]
        );
        assert_eq!(feed.working[0].run_id, run_id(2));
        assert_eq!(feed.recent[0].run_id, run_id(3));
        assert_eq!(feed.recent[0].activity, AgentActivity::Working);
        assert_eq!(feed.recent[0].connectivity, AgentConnectivity::Disconnected);
    }

    #[test]
    fn ordering_is_input_independent_across_hosts_and_equal_timestamps() {
        let alpha = target(
            1,
            "alpha",
            AgentActivity::Working,
            AgentConnectivity::Live,
            false,
            50,
        );
        let beta = target(
            2,
            "beta",
            AgentActivity::Working,
            AgentConnectivity::Live,
            false,
            50,
        );
        let first = build_agent_activity_feed(
            vec![beta.clone(), alpha.clone()],
            AGENT_ACTIVITY_RECENT_LIMIT,
        );
        let second = build_agent_activity_feed(vec![alpha, beta], AGENT_ACTIVITY_RECENT_LIMIT);
        assert_eq!(
            first
                .working
                .iter()
                .map(|target| target.run_id.clone())
                .collect::<Vec<_>>(),
            second
                .working
                .iter()
                .map(|target| target.run_id.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn recent_is_newest_first_and_bounded_without_limiting_active_rows() {
        let targets = vec![
            target(
                1,
                "alpha",
                AgentActivity::Done,
                AgentConnectivity::Live,
                false,
                10,
            ),
            target(
                2,
                "beta",
                AgentActivity::Exited,
                AgentConnectivity::Disconnected,
                false,
                30,
            ),
            target(
                3,
                "gamma",
                AgentActivity::Unknown,
                AgentConnectivity::Stale,
                false,
                20,
            ),
            target(
                4,
                "delta",
                AgentActivity::Working,
                AgentConnectivity::Live,
                false,
                40,
            ),
        ];
        let feed = build_agent_activity_feed(targets, 2);
        assert_eq!(feed.working.len(), 1);
        assert_eq!(
            feed.recent
                .iter()
                .map(|target| target.run_id.clone())
                .collect::<Vec<_>>(),
            vec![run_id(2), run_id(3)]
        );
    }

    #[test]
    fn duplicate_provider_runs_and_same_path_labels_remain_distinct() {
        let mut local = target(
            1,
            "alpha",
            AgentActivity::Working,
            AgentConnectivity::Live,
            false,
            10,
        );
        let mut remote = target(
            2,
            "alpha",
            AgentActivity::Working,
            AgentConnectivity::Live,
            false,
            10,
        );
        local.worktree_name = "shared".into();
        remote.worktree_name = "shared".into();
        remote.route = route("remote", "/repo/alpha");
        let feed = build_agent_activity_feed(vec![local, remote], AGENT_ACTIVITY_RECENT_LIMIT);
        assert_eq!(feed.working.len(), 2);
        assert_ne!(feed.working[0].run_id, feed.working[1].run_id);
        assert_ne!(
            feed.working[0].route.execution_host_id,
            feed.working[1].route.execution_host_id
        );
    }

    #[test]
    fn rollback_only_accepts_exact_zero() {
        assert!(global_agent_activity_enabled_for(None));
        assert!(!global_agent_activity_enabled_for(Some(OsStr::new("0"))));
        assert!(global_agent_activity_enabled_for(Some(OsStr::new("false"))));
        assert!(global_agent_activity_enabled_for(Some(OsStr::new("1"))));
    }
}
