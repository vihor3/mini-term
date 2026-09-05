//! Global Quick Open for Agent runs, terminal panes, worktrees, settings, and
//! workspace commands.
//!
//! The palette is a projection-only surface: typing reads current in-memory
//! store/catalog snapshots and never starts Git, filesystem, SSH, or history
//! work. Every selectable row owns a complete stable target and revalidates it
//! through the store or catalog boundary before navigation.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{
    AnyElement, App, AppContext, Context, Entity, FontWeight, InteractiveElement, IntoElement,
    ParentElement, Render, ScrollHandle, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window, actions, div, prelude::FluentBuilder as _, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use mt_ai::{AgentActivity, AgentConnectivity, AgentRoute};
use mt_identity::WorktreeId;
use mt_project::worktree::WorktreePathState;
use mt_ui::icons::vector::{Geom, Ink, Shape, VectorIcon};
use mt_ui::icons::{AiVendor, BrandIcon};

use crate::i18n::t;
use crate::overlay::kind;
use crate::prompt::{autofocus, close_guarded, open_guarded_with_close};
use crate::settings::SettingsPage;
use crate::store::{
    AgentTargetView, AppStore, TerminalJumpTarget, TerminalJumpView,
};
use crate::tree::PaneStatus;
use crate::ui;
use crate::worktree_catalog::{CatalogBackend, WorktreeCatalog, WorktreeCatalogTarget};

actions!(
    mini_term,
    [
        JumpPrev,
        JumpNext,
        JumpToggleFilter,
    ]
);

#[derive(Clone, PartialEq, Default, Debug, gpui::Action)]
#[action(namespace = mini_term, no_json)]
pub struct JumpDirect(pub usize);

const MAX_QUERY_BYTES: usize = 2 * 1024;
const SEARCH_FIELD_CHAR_LIMIT: usize = 512;
const RECENCY_LIMIT: usize = 64;
const RECENT_TARGET_CAP: usize = 12;
const RECENT_WORKTREE_CAP: usize = 12;
const QUERY_RESULT_CAP: usize = 30;

const SEARCH_ICON: &[Shape] = &[
    Shape::line(
        Ink::Current,
        0.0875,
        Geom::Circle {
            c: (0.4375, 0.4375),
            r: 0.2625,
        },
    ),
    Shape::line(
        Ink::Current,
        0.0875,
        Geom::Polyline(&[(0.6375, 0.6375), (0.875, 0.875)]),
    ),
];

const FILTER_ICON: &[Shape] = &[
    Shape::line(
        Ink::Current,
        0.08,
        Geom::Polyline(&[(0.12, 0.22), (0.88, 0.22)]),
    ),
    Shape::line(
        Ink::Current,
        0.08,
        Geom::Polyline(&[(0.25, 0.50), (0.75, 0.50)]),
    ),
    Shape::line(
        Ink::Current,
        0.08,
        Geom::Polyline(&[(0.38, 0.78), (0.62, 0.78)]),
    ),
];

const COMMAND_ICON: &[Shape] = &[
    Shape::line(
        Ink::Current,
        0.08,
        Geom::Polyline(&[(0.18, 0.30), (0.42, 0.50), (0.18, 0.70)]),
    ),
    Shape::line(
        Ink::Current,
        0.08,
        Geom::Polyline(&[(0.50, 0.70), (0.82, 0.70)]),
    ),
];

const PLUS_ICON: &[Shape] = &[
    Shape::line(
        Ink::Current,
        0.10,
        Geom::Polyline(&[(0.2, 0.5), (0.8, 0.5)]),
    ),
    Shape::line(
        Ink::Current,
        0.10,
        Geom::Polyline(&[(0.5, 0.2), (0.5, 0.8)]),
    ),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum JumpFamily {
    Agent,
    Terminal,
    Worktree,
    Setting,
    Action,
}

impl JumpFamily {
    const ALL: [Self; 5] = [
        Self::Agent,
        Self::Terminal,
        Self::Worktree,
        Self::Setting,
        Self::Action,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Agent => t("projectSwitcher", "familyAgent"),
            Self::Terminal => t("projectSwitcher", "familyTerminal"),
            Self::Worktree => t("projectSwitcher", "familyWorktree"),
            Self::Setting => t("projectSwitcher", "familySetting"),
            Self::Action => t("projectSwitcher", "familyAction"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JumpAction {
    Settings,
    Usage,
    AddProject,
    NewTerminal,
    FileSearch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JumpCommand {
    Settings(Option<SettingsPage>),
    Usage,
    AddProject,
    NewTerminal,
    FileSearch,
}

#[derive(Clone)]
enum JumpItem {
    Agent(AgentTargetView),
    Terminal(TerminalJumpView),
    Worktree(WorktreeCatalogTarget),
    Setting(SettingsPage),
    Action(JumpAction),
}

impl JumpItem {
    fn family(&self) -> JumpFamily {
        match self {
            Self::Agent(_) => JumpFamily::Agent,
            Self::Terminal(_) => JumpFamily::Terminal,
            Self::Worktree(_) => JumpFamily::Worktree,
            Self::Setting(_) => JumpFamily::Setting,
            Self::Action(_) => JumpFamily::Action,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct WorktreeRecencyKey {
    project_id: String,
    worktree_id: WorktreeId,
}

#[derive(Clone)]
struct Timed<T> {
    key: T,
    unix_ms: i64,
}

/// Process-local navigation history. It observes stable active identities and
/// deliberately owns no persistence or palette query state.
pub struct JumpRecency {
    store: Entity<AppStore>,
    terminals: Vec<Timed<TerminalJumpTarget>>,
    worktrees: Vec<Timed<WorktreeRecencyKey>>,
    last_terminal: Option<TerminalJumpTarget>,
    last_worktree: Option<WorktreeRecencyKey>,
    _store_subscription: Subscription,
}

impl JumpRecency {
    pub fn new(store: Entity<AppStore>, cx: &mut Context<Self>) -> Self {
        let (terminal, worktree) = active_recency_targets(store.read(cx));
        let subscription = cx.observe(&store, |this, _, cx| {
            let (terminal, worktree) = active_recency_targets(this.store.read(cx));
            let terminal_changed = record_recency(
                terminal,
                &mut this.last_terminal,
                &mut this.terminals,
            );
            let worktree_changed = record_recency(
                worktree,
                &mut this.last_worktree,
                &mut this.worktrees,
            );
            if terminal_changed || worktree_changed {
                cx.notify();
            }
        });
        Self {
            store,
            terminals: Vec::new(),
            worktrees: Vec::new(),
            last_terminal: terminal,
            last_worktree: worktree,
            _store_subscription: subscription,
        }
    }

    fn terminal_time(&self, target: &TerminalJumpTarget) -> Option<i64> {
        self.terminals
            .iter()
            .find(|entry| &entry.key == target)
            .map(|entry| entry.unix_ms)
    }

    fn worktree_time(&self, key: &WorktreeRecencyKey) -> Option<i64> {
        self.worktrees
            .iter()
            .find(|entry| &entry.key == key)
            .map(|entry| entry.unix_ms)
    }
}

fn active_recency_targets(
    store: &AppStore,
) -> (Option<TerminalJumpTarget>, Option<WorktreeRecencyKey>) {
    let terminal = store
        .terminal_jump_views()
        .into_iter()
        .find(|view| view.active)
        .map(|view| view.target);
    let worktree = store
        .active_project_id
        .as_ref()
        .zip(store.active_worktree_id())
        .map(|(project_id, worktree_id)| WorktreeRecencyKey {
            project_id: project_id.clone(),
            worktree_id: worktree_id.clone(),
        });
    (terminal, worktree)
}

fn record_recency<T: Clone + PartialEq>(
    current: Option<T>,
    last: &mut Option<T>,
    entries: &mut Vec<Timed<T>>,
) -> bool {
    if current == *last {
        return false;
    }
    *last = current.clone();
    let Some(key) = current else {
        return false;
    };
    entries.retain(|entry| entry.key != key);
    entries.insert(
        0,
        Timed {
            key,
            unix_ms: unix_ms(),
        },
    );
    entries.truncate(RECENCY_LIMIT);
    true
}

fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

#[derive(Clone)]
struct FilterOption {
    key: String,
    label: String,
}

#[derive(Default)]
struct FilterOptions {
    families: Vec<JumpFamily>,
    hosts: Vec<FilterOption>,
    projects: Vec<FilterOption>,
}

impl FilterOptions {
    fn from_candidates(candidates: &[JumpCandidate]) -> Self {
        let present_families: HashSet<_> = candidates
            .iter()
            .map(|candidate| candidate.item.family())
            .collect();
        let families = JumpFamily::ALL
            .into_iter()
            .filter(|family| present_families.contains(family))
            .collect();
        let mut hosts = HashMap::<String, String>::new();
        let mut projects = HashMap::<String, String>::new();
        for candidate in candidates {
            if let Some(option) = candidate.host.as_ref() {
                hosts
                    .entry(option.key.clone())
                    .or_insert_with(|| option.label.clone());
            }
            if let Some(option) = candidate.project.as_ref() {
                projects
                    .entry(option.key.clone())
                    .or_insert_with(|| option.label.clone());
            }
        }
        let mut hosts: Vec<_> = hosts
            .into_iter()
            .map(|(key, label)| FilterOption { key, label })
            .collect();
        let mut projects: Vec<_> = projects
            .into_iter()
            .map(|(key, label)| FilterOption { key, label })
            .collect();
        hosts.sort_by(|left, right| compare_filter_options(left, right));
        projects.sort_by(|left, right| compare_filter_options(left, right));
        Self {
            families,
            hosts,
            projects,
        }
    }
}

fn compare_filter_options(left: &FilterOption, right: &FilterOption) -> Ordering {
    left.label
        .to_lowercase()
        .cmp(&right.label.to_lowercase())
        .then_with(|| left.key.cmp(&right.key))
}

#[derive(Default)]
struct FilterState {
    families: HashSet<JumpFamily>,
    hosts: HashSet<String>,
    projects: HashSet<String>,
}

impl FilterState {
    fn reconcile(&mut self, options: &FilterOptions) -> bool {
        let previous_counts = (self.families.len(), self.hosts.len(), self.projects.len());
        self.families
            .retain(|family| options.families.contains(family));
        self.hosts
            .retain(|host| options.hosts.iter().any(|option| &option.key == host));
        self.projects.retain(|project| {
            options
                .projects
                .iter()
                .any(|option| &option.key == project)
        });
        previous_counts != (self.families.len(), self.hosts.len(), self.projects.len())
    }

    fn matches(&self, candidate: &JumpCandidate) -> bool {
        (self.families.is_empty() || self.families.contains(&candidate.item.family()))
            && (self.hosts.is_empty()
                || candidate
                    .host
                    .as_ref()
                    .is_some_and(|host| self.hosts.contains(&host.key)))
            && (self.projects.is_empty()
                || candidate
                    .project
                    .as_ref()
                    .is_some_and(|project| self.projects.contains(&project.key)))
    }

    fn is_active(&self) -> bool {
        !self.families.is_empty() || !self.hosts.is_empty() || !self.projects.is_empty()
    }

    fn clear(&mut self) {
        self.families.clear();
        self.hosts.clear();
        self.projects.clear();
    }
}

fn toggle_selected<T: Clone + Eq + std::hash::Hash>(selected: &mut HashSet<T>, value: T) {
    if selected.is_empty() {
        selected.insert(value);
    } else if !selected.remove(&value) {
        selected.insert(value);
    }
}

#[derive(Clone)]
struct JumpCandidate {
    id: String,
    item: JumpItem,
    title: String,
    subtitle: String,
    search_fields: Vec<String>,
    intents: Vec<String>,
    badges: Vec<String>,
    project: Option<FilterOption>,
    host: Option<FilterOption>,
    source_order: usize,
    attention: bool,
    active: bool,
    timestamp: Option<i64>,
    pane_status: Option<PaneStatus>,
    warning: bool,
    selectable: bool,
}

impl JumpCandidate {
    #[allow(clippy::too_many_arguments)]
    fn new(
        id: String,
        item: JumpItem,
        title: String,
        subtitle: String,
        mut search_fields: Vec<String>,
        intents: Vec<String>,
        badges: Vec<String>,
        project: Option<FilterOption>,
        host: Option<FilterOption>,
        source_order: usize,
    ) -> Self {
        search_fields.push(title.clone());
        search_fields.push(subtitle.clone());
        search_fields = search_fields
            .into_iter()
            .map(|field| field.chars().take(SEARCH_FIELD_CHAR_LIMIT).collect())
            .collect();
        Self {
            id,
            item,
            title,
            subtitle,
            search_fields,
            intents,
            badges,
            project,
            host,
            source_order,
            attention: false,
            active: false,
            timestamp: None,
            pane_status: None,
            warning: false,
            selectable: true,
        }
    }
}

fn build_candidates(
    store: &Entity<AppStore>,
    catalog: &Entity<WorktreeCatalog>,
    recency: &JumpRecency,
    cx: &App,
) -> Vec<JumpCandidate> {
    let (
        agents,
        terminals,
        active_project_id,
        active_worktree_id,
        new_terminal_available,
        file_search_available,
    ) = {
        let store = store.read(cx);
        let new_terminal_available =
            store.active_project().is_some() && store.resolve_shell(None).is_some();
        let file_search_available = store.active_project().is_some_and(|project| {
            !store.is_remote_project(&project.id)
                && store.active_worktree_id().is_some_and(|worktree_id| {
                    store.worktree_id_for_project(&project.id) == Some(worktree_id)
                })
        });
        (
            store.agent_target_views(),
            store.terminal_jump_views(),
            store.active_project_id.clone(),
            store.active_worktree_id().cloned(),
            new_terminal_available,
            file_search_available,
        )
    };
    let active_terminal_targets: Vec<_> = terminals
        .iter()
        .filter(|terminal| terminal.active)
        .map(|terminal| terminal.target.clone())
        .collect();
    let agent_routes: Vec<_> = agents.iter().map(|agent| agent.route.clone()).collect();
    let mut candidates = Vec::new();
    let mut source_order = 0usize;

    for agent in agents {
        let provider = agent.provider.as_str().to_string();
        let title = nonempty_label(&agent.pane_label, &provider);
        let subtitle = format!(
            "{} | {} | {}",
            agent.project_name, agent.worktree_name, agent.host_label
        );
        let project = FilterOption {
            key: agent.project_id.clone(),
            label: agent.project_name.clone(),
        };
        let host = FilterOption {
            key: agent.route.execution_host_id.to_string(),
            label: agent.host_label.clone(),
        };
        let active = active_terminal_targets
            .iter()
            .any(|terminal| terminal_matches_agent(terminal, &agent.route));
        let mut candidate = JumpCandidate::new(
            format!("agent:{}", agent.run_id.as_str()),
            JumpItem::Agent(agent.clone()),
            title,
            subtitle,
            vec![
                agent.project_name.clone(),
                agent.root_project_name.clone(),
                agent.worktree_name.clone(),
                agent.host_label.clone(),
                provider.clone(),
                agent_activity_label(agent.activity).to_string(),
            ],
            vec![provider.clone(), format!("{provider} chat")],
            vec![agent.project_name.clone(), provider, agent.host_label.clone()],
            Some(project),
            Some(host),
            source_order,
        );
        candidate.attention = agent.attention
            || agent.unread
            || matches!(agent.activity, AgentActivity::Blocked | AgentActivity::Failed);
        candidate.active = active;
        candidate.timestamp = (agent.received_at_unix_ms > 0).then_some(agent.received_at_unix_ms);
        candidate.warning = agent.connectivity != AgentConnectivity::Live;
        candidates.push(candidate);
        source_order += 1;
    }

    for terminal in terminals {
        if agent_routes
            .iter()
            .any(|route| terminal_matches_agent(&terminal.target, route))
        {
            continue;
        }
        let title = nonempty_label(&terminal.pane_label, &terminal.panel_label);
        let subtitle = format!(
            "{} | {} | {}",
            terminal.project_name, terminal.worktree_name, terminal.host_label
        );
        let project = FilterOption {
            key: terminal.target.project_id.clone(),
            label: terminal.project_name.clone(),
        };
        let host = FilterOption {
            key: terminal.target.execution_host_id.to_string(),
            label: terminal.host_label.clone(),
        };
        let mut candidate = JumpCandidate::new(
            terminal_id(&terminal.target),
            JumpItem::Terminal(terminal.clone()),
            title,
            subtitle,
            vec![
                terminal.project_name.clone(),
                terminal.root_project_name.clone(),
                terminal.worktree_name.clone(),
                terminal.host_label.clone(),
                terminal.panel_label.clone(),
            ],
            vec!["terminal".into(), "pane".into()],
            vec![
                terminal.project_name.clone(),
                terminal.worktree_name.clone(),
                terminal.host_label.clone(),
            ],
            Some(project),
            Some(host),
            source_order,
        );
        candidate.active = terminal.active;
        candidate.timestamp = recency.terminal_time(&terminal.target);
        candidate.pane_status = Some(terminal.status);
        candidate.warning = !terminal.live && !terminal.dormant;
        candidates.push(candidate);
        source_order += 1;
    }

    let groups = catalog.read(cx).groups(cx);
    for group in groups {
        for row in group.rows {
            let project = FilterOption {
                key: group.root_project_id.clone(),
                label: group.root_project_name.clone(),
            };
            let host_key = group
                .execution_host_id
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| catalog_backend_key(&group.backend));
            let host = FilterOption {
                key: host_key,
                label: group.host_label.clone(),
            };
            let branch = row.branch.clone().unwrap_or_else(|| {
                if row.is_detached {
                    t("projectSwitcher", "detached").to_string()
                } else {
                    t("projectSwitcher", "configured").to_string()
                }
            });
            let mut state_terms = Vec::new();
            if row.is_main {
                state_terms.push(t("worktree", "mainRepo").to_string());
            }
            if row.is_sparse {
                state_terms.push("sparse".to_string());
            }
            if row.is_bare {
                state_terms.push("bare".to_string());
            }
            if row.is_detached {
                state_terms.push("detached".to_string());
            }
            if let Some(reason) = row.locked_reason.as_deref() {
                state_terms.push(format!("locked: {reason}"));
            } else if row.is_locked {
                state_terms.push("locked".to_string());
            }
            if let Some(reason) = row.prunable_reason.as_deref() {
                state_terms.push(format!("prunable: {reason}"));
            } else if row.is_prunable {
                state_terms.push("prunable".to_string());
            }
            if row.path_state == WorktreePathState::Missing {
                state_terms.push("missing".to_string());
            }
            if row.last_known {
                state_terms.push("last known".to_string());
            } else if !row.authoritative {
                state_terms.push(t("projectSwitcher", "configured").to_string());
            }
            if group.warning.is_some() && !row.last_known {
                state_terms.push("unavailable".to_string());
            }
            let mut subtitle_parts = vec![group.root_project_name.clone()];
            subtitle_parts.extend(state_terms.iter().cloned());
            subtitle_parts.push(row.target.execution_path.clone());
            let subtitle = subtitle_parts.join(" | ");
            let mut badges = vec![group.root_project_name.clone(), branch.clone()];
            badges.push(group.host_label.clone());
            let configured_worktree = row
                .target
                .configured_project_id
                .as_ref()
                .and_then(|project_id| {
                    store
                        .read(cx)
                        .worktree_id_for_project(project_id)
                        .cloned()
                        .map(|worktree_id| (project_id.clone(), worktree_id))
                });
            let mut search_fields = vec![
                group.root_project_name.clone(),
                group.root_project_path.clone(),
                row.target.execution_path.clone(),
                row.target.host_visible_path.clone(),
                branch,
                group.host_label.clone(),
            ];
            if let Some(head) = row.head.clone() {
                search_fields.push(head);
            }
            search_fields.extend(state_terms);
            let mut candidate = JumpCandidate::new(
                format!(
                    "worktree:{}:{}",
                    group.root_project_id, row.target.row_key
                ),
                JumpItem::Worktree(row.target.clone()),
                row.label.clone(),
                subtitle,
                search_fields,
                vec!["worktree".into(), "branch".into()],
                badges,
                Some(project),
                Some(host),
                source_order,
            );
            if let Some((project_id, worktree_id)) = configured_worktree {
                let recency_key = WorktreeRecencyKey {
                    project_id: project_id.clone(),
                    worktree_id: worktree_id.clone(),
                };
                candidate.timestamp = recency.worktree_time(&recency_key);
                candidate.active = active_worktree_id.as_ref() == Some(&worktree_id)
                    && active_project_id.as_deref() == Some(project_id.as_str());
                if let Some(state) = store.read(cx).project_state(&project_id) {
                    candidate.attention = state.needs_attention;
                    candidate.pane_status = Some(state.status);
                }
            }
            candidate.warning = group.warning.is_some()
                || row.last_known
                || row.path_state == WorktreePathState::Missing
                || row.is_locked
                || row.is_prunable;
            candidate.selectable = row.selectable;
            candidates.push(candidate);
            source_order += 1;
        }
    }

    for page in crate::settings::ALL_PAGES.iter().copied() {
        let page_label = settings_page_label(page).to_string();
        let title = format!("{}: {page_label}", t("projectSwitcher", "actionSettings"));
        candidates.push(JumpCandidate::new(
            format!("setting:{}", page.id()),
            JumpItem::Setting(page),
            title,
            t("projectSwitcher", "settingsSubtitle").to_string(),
            vec![page.id().to_string(), page_label],
            vec![format!("settings {}", page.id())],
            vec![t("projectSwitcher", "familySetting").to_string()],
            None,
            None,
            source_order,
        ));
        source_order += 1;
    }

    for action in [
        JumpAction::Settings,
        JumpAction::Usage,
        JumpAction::AddProject,
        JumpAction::NewTerminal,
        JumpAction::FileSearch,
    ] {
        if (action == JumpAction::NewTerminal && !new_terminal_available)
            || (action == JumpAction::FileSearch && !file_search_available)
        {
            continue;
        }
        let (key, title, intents) = action_text(action);
        candidates.push(JumpCandidate::new(
            format!("action:{key}"),
            JumpItem::Action(action),
            title.to_string(),
            t("projectSwitcher", "actionSubtitle").to_string(),
            intents.iter().map(|intent| (*intent).to_string()).collect(),
            intents.iter().map(|intent| (*intent).to_string()).collect(),
            vec![t("projectSwitcher", "familyAction").to_string()],
            None,
            None,
            source_order,
        ));
        source_order += 1;
    }

    candidates
}

fn nonempty_label(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn terminal_id(target: &TerminalJumpTarget) -> String {
    format!(
        "terminal:{}:{}:{}:{}:{}:{}:{}",
        target.project_id,
        target.execution_host_id,
        target.worktree_id,
        target.tab_id,
        target.pane_key,
        target.terminal_session_id,
        target
            .terminal_incarnation_id
            .as_ref()
            .map(|id| id.as_str())
            .unwrap_or("dormant")
    )
}

fn terminal_matches_agent(target: &TerminalJumpTarget, route: &AgentRoute) -> bool {
    target.execution_host_id == route.execution_host_id
        && target.worktree_id == route.worktree_id
        && target.tab_id == route.tab_id
        && target.pane_key == route.pane_key
        && target.terminal_session_id == route.terminal_session_id
        && target.terminal_incarnation_id.as_ref() == Some(&route.terminal_incarnation_id)
}

fn catalog_backend_key(backend: &CatalogBackend) -> String {
    match backend {
        CatalogBackend::Local => "local".into(),
        CatalogBackend::Wsl { distro } => format!("wsl:{}", distro.to_lowercase()),
        CatalogBackend::Ssh { connection_id } => format!("ssh:{connection_id}"),
    }
}

fn agent_activity_label(activity: AgentActivity) -> &'static str {
    match activity {
        AgentActivity::Starting => t("projectSwitcher", "statusStarting"),
        AgentActivity::Working => t("projectSwitcher", "statusWorking"),
        AgentActivity::Blocked => t("projectSwitcher", "statusNeedsYou"),
        AgentActivity::Waiting => t("projectSwitcher", "statusWaiting"),
        AgentActivity::Done => t("projectSwitcher", "statusDone"),
        AgentActivity::Failed => t("projectSwitcher", "statusFailed"),
        AgentActivity::Interrupted => t("projectSwitcher", "statusInterrupted"),
        AgentActivity::Exited => t("projectSwitcher", "statusExited"),
        AgentActivity::Unknown => t("projectSwitcher", "statusUnknown"),
    }
}

fn settings_page_label(page: SettingsPage) -> &'static str {
    match page {
        SettingsPage::Terminal => t("settings", "menu.shell"),
        SettingsPage::Clipboard => t("settings", "menu.clipboard"),
        SettingsPage::Appearance => t("settings", "menu.appearance"),
        SettingsPage::Font => t("settings", "menu.font"),
        SettingsPage::AiNotification => t("settings", "menu.aiNotification"),
        SettingsPage::AiHook => t("settings", "menu.aiHook"),
        SettingsPage::System => t("settings", "menu.general"),
        SettingsPage::Editor => t("settings", "menu.editor"),
        SettingsPage::Shortcuts => t("settings", "menu.shortcuts"),
        SettingsPage::About => t("settings", "menu.about"),
    }
}

fn action_text(action: JumpAction) -> (&'static str, &'static str, &'static [&'static str]) {
    match action {
        JumpAction::Settings => (
            "settings",
            t("projectSwitcher", "actionSettings"),
            &["settings", "preferences"],
        ),
        JumpAction::Usage => (
            "usage",
            t("projectSwitcher", "actionUsage"),
            &["usage", "tokens", "statistics"],
        ),
        JumpAction::AddProject => (
            "add-project",
            t("projectSwitcher", "actionAddProject"),
            &["add project", "open folder", "clone repository"],
        ),
        JumpAction::NewTerminal => (
            "new-terminal",
            t("projectSwitcher", "actionNewTerminal"),
            &["new terminal", "terminal", "shell"],
        ),
        JumpAction::FileSearch => (
            "file-search",
            t("projectSwitcher", "actionFileSearch"),
            &["file search", "find file", "search files"],
        ),
    }
}

struct NormalizedQuery {
    phrase: String,
    tokens: Vec<String>,
}

fn normalize_query(raw: &str) -> Result<NormalizedQuery, ()> {
    if raw.len() > MAX_QUERY_BYTES {
        return Err(());
    }
    let tokens: Vec<String> = raw
        .split_whitespace()
        .map(|token| token.to_lowercase())
        .filter(|token| !token.is_empty())
        .collect();
    Ok(NormalizedQuery {
        phrase: tokens.join(" "),
        tokens,
    })
}

fn search_rank(candidate: &JumpCandidate, query: &NormalizedQuery) -> Option<(u8, usize, usize)> {
    if query.tokens.is_empty() {
        return Some((0, 0, candidate.source_order));
    }
    let title = candidate.title.to_lowercase();
    let intents: Vec<_> = candidate
        .intents
        .iter()
        .map(|intent| intent.to_lowercase())
        .collect();
    if title == query.phrase || intents.iter().any(|intent| intent == &query.phrase) {
        return Some((0, 0, candidate.source_order));
    }
    if title.starts_with(&query.phrase)
        || intents
            .iter()
            .any(|intent| intent.starts_with(&query.phrase))
    {
        return Some((1, 0, candidate.source_order));
    }
    let fields: Vec<_> = candidate
        .search_fields
        .iter()
        .chain(candidate.intents.iter())
        .map(|field| field.to_lowercase())
        .collect();
    let mut penalty = 0usize;
    for token in &query.tokens {
        let best = fields
            .iter()
            .filter_map(|field| char_substring_position(field, token))
            .min()?;
        penalty = penalty.saturating_add(best);
    }
    Some((2, penalty, candidate.source_order))
}

fn char_substring_position(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .char_indices()
        .enumerate()
        .find_map(|(char_index, (byte_index, _))| {
            haystack[byte_index..]
                .starts_with(needle)
                .then_some(char_index)
        })
}

fn title_match_ranges(title: &str, query: &NormalizedQuery) -> Vec<(usize, usize)> {
    let chars: Vec<char> = title.chars().collect();
    let mut ranges = Vec::new();
    for token in &query.tokens {
        if let Some((start, end)) = original_char_range(&chars, token) {
            ranges.push((start, end));
        }
    }
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

fn original_char_range(chars: &[char], normalized_needle: &str) -> Option<(usize, usize)> {
    if normalized_needle.is_empty() {
        return None;
    }
    for start in 0..chars.len() {
        let mut normalized = String::new();
        for (end, ch) in chars.iter().enumerate().skip(start) {
            normalized.extend(ch.to_lowercase());
            if normalized == normalized_needle {
                return Some((start, end + 1));
            }
            if !normalized_needle.starts_with(&normalized) {
                break;
            }
        }
    }
    None
}

#[derive(Default)]
struct FrozenEmptyOrder {
    conversations: Vec<String>,
    worktrees: Vec<String>,
}

impl FrozenEmptyOrder {
    fn capture(candidates: &[JumpCandidate]) -> Self {
        let mut conversations: Vec<_> = candidates
            .iter()
            .filter(|candidate| {
                matches!(
                    candidate.item.family(),
                    JumpFamily::Agent | JumpFamily::Terminal
                )
            })
            .collect();
        let mut worktrees: Vec<_> = candidates
            .iter()
            .filter(|candidate| candidate.item.family() == JumpFamily::Worktree)
            .collect();
        conversations.sort_by(|left, right| compare_recent(left, right));
        worktrees.sort_by(|left, right| compare_recent(left, right));
        Self {
            conversations: conversations
                .into_iter()
                .map(|candidate| candidate.id.clone())
                .collect(),
            worktrees: worktrees
                .into_iter()
                .map(|candidate| candidate.id.clone())
                .collect(),
        }
    }
}

fn compare_recent(left: &JumpCandidate, right: &JumpCandidate) -> Ordering {
    right
        .attention
        .cmp(&left.attention)
        .then_with(|| right.active.cmp(&left.active))
        .then_with(|| right.timestamp.is_some().cmp(&left.timestamp.is_some()))
        .then_with(|| right.timestamp.cmp(&left.timestamp))
        .then_with(|| left.source_order.cmp(&right.source_order))
}

#[derive(Clone)]
enum PaletteRow {
    Header(String),
    Item(JumpCandidate),
    Overflow(String),
    Empty(String),
}

struct VisibleModel {
    rows: Vec<PaletteRow>,
    selectable_count: usize,
}

impl VisibleModel {
    fn candidate_at(&self, selectable_index: usize) -> Option<&JumpCandidate> {
        self.rows
            .iter()
            .filter_map(|row| match row {
                PaletteRow::Item(candidate) if candidate.selectable => Some(candidate),
                _ => None,
            })
            .nth(selectable_index)
    }

    fn selectable_row_top(&self, selectable_index: usize) -> Option<f32> {
        let mut top = 4.0;
        let mut current = 0usize;
        for row in &self.rows {
            match row {
                PaletteRow::Header(_) => top += 28.0,
                PaletteRow::Item(candidate) => {
                    if candidate.selectable {
                        if current == selectable_index {
                            return Some(top);
                        }
                        current += 1;
                    }
                    top += 56.0;
                }
                PaletteRow::Overflow(_) => top += 30.0,
                PaletteRow::Empty(_) => top += 80.0,
            }
        }
        None
    }
}

type CommandHandler = Rc<dyn Fn(JumpCommand, &mut Window, &mut App)>;

pub struct JumpPalette {
    store: Entity<AppStore>,
    catalog: Entity<WorktreeCatalog>,
    recency: Entity<JumpRecency>,
    query: Entity<InputState>,
    filters: FilterState,
    filters_open: bool,
    cursor: usize,
    scroll: ScrollHandle,
    frozen: FrozenEmptyOrder,
    on_command: CommandHandler,
    _subscriptions: Vec<Subscription>,
}

pub fn open(
    store: Entity<AppStore>,
    catalog: Entity<WorktreeCatalog>,
    recency: Entity<JumpRecency>,
    on_command: impl Fn(JumpCommand, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    if crate::overlay::contains(crate::overlay::key(kind::PROJECT_SWITCHER)) {
        return;
    }
    let previous_focus = window.focused(cx);
    let on_command: CommandHandler = Rc::new(on_command);
    let view = cx.new(|cx| {
        JumpPalette::new(store, catalog, recency, on_command, window, cx)
    });
    let input = view.read(cx).query.clone();
    let restore_focus = previous_focus.clone();
    open_guarded_with_close(
        kind::PROJECT_SWITCHER,
        window,
        cx,
        {
            let view = view.clone();
            move |dialog, window, _cx| {
                let viewport = window.viewport_size();
                let width = px(900.0).min(viewport.width * 0.96);
                let height = px(680.0).min(viewport.height * 0.80);
                dialog
                    .p_0()
                    .w(width)
                    .margin_top(viewport.height * 0.10)
                    .close_button(false)
                    .child(div().w_full().h(height).child(view.clone()))
            }
        },
        move |window, _cx| {
            if let Some(focus) = restore_focus.as_ref() {
                window.focus(focus);
            }
        },
    );
    autofocus(&input, window, cx);
}

impl JumpPalette {
    fn new(
        store: Entity<AppStore>,
        catalog: Entity<WorktreeCatalog>,
        recency: Entity<JumpRecency>,
        on_command: CommandHandler,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("projectSwitcher", "placeholder"))
        });
        let input_subscription = cx.subscribe_in(
            &query,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => this.reset_selection(cx),
                InputEvent::PressEnter { .. } => this.commit_selected(window, cx),
                _ => {}
            },
        );
        let store_subscription = cx.observe(&store, |_this, _, cx| cx.notify());
        let catalog_subscription = cx.observe(&catalog, |_this, _, cx| cx.notify());
        let frozen = FrozenEmptyOrder::capture(&build_candidates(
            &store,
            &catalog,
            recency.read(cx),
            cx,
        ));
        Self {
            store,
            catalog,
            recency,
            query,
            filters: FilterState::default(),
            filters_open: false,
            cursor: 0,
            scroll: ScrollHandle::new(),
            frozen,
            on_command,
            _subscriptions: vec![
                input_subscription,
                store_subscription,
                catalog_subscription,
            ],
        }
    }

    fn reset_selection(&mut self, cx: &mut Context<Self>) {
        self.cursor = 0;
        self.scroll = ScrollHandle::new();
        cx.notify();
    }

    fn toggle_filters(&mut self, cx: &mut Context<Self>) {
        self.filters_open = !self.filters_open;
        self.reset_selection(cx);
    }

    fn model(&mut self, cx: &App) -> VisibleModel {
        let candidates = build_candidates(
            &self.store,
            &self.catalog,
            self.recency.read(cx),
            cx,
        );
        let options = FilterOptions::from_candidates(&candidates);
        if self.filters.reconcile(&options) {
            self.cursor = 0;
            self.scroll = ScrollHandle::new();
        }
        let query_text = self.query.read(cx).value().to_string();
        let Ok(query) = normalize_query(&query_text) else {
            return VisibleModel {
                rows: vec![PaletteRow::Empty(
                    t("projectSwitcher", "queryTooLong").to_string(),
                )],
                selectable_count: 0,
            };
        };
        let filtered: Vec<_> = candidates
            .into_iter()
            .filter(|candidate| self.filters.matches(candidate))
            .collect();
        let mut rows = Vec::new();
        if query.tokens.is_empty() {
            let conversations = frozen_candidates(
                &filtered,
                &self.frozen.conversations,
                |candidate| {
                    matches!(
                        candidate.item.family(),
                        JumpFamily::Agent | JumpFamily::Terminal
                    )
                },
            );
            let worktrees = frozen_candidates(
                &filtered,
                &self.frozen.worktrees,
                |candidate| candidate.item.family() == JumpFamily::Worktree,
            );
            push_section(
                &mut rows,
                t("projectSwitcher", "sectionRecentTargets"),
                conversations,
                RECENT_TARGET_CAP,
            );
            push_section(
                &mut rows,
                t("projectSwitcher", "sectionRecentWorktrees"),
                worktrees,
                RECENT_WORKTREE_CAP,
            );
            if self.filters.families.contains(&JumpFamily::Setting)
                || self.filters.families.contains(&JumpFamily::Action)
            {
                let settings_actions = filtered
                    .iter()
                    .filter(|candidate| {
                        matches!(
                            candidate.item.family(),
                            JumpFamily::Setting | JumpFamily::Action
                        )
                    })
                    .cloned()
                    .collect();
                push_section(
                    &mut rows,
                    t("projectSwitcher", "sectionSettingsActions"),
                    settings_actions,
                    QUERY_RESULT_CAP,
                );
            }
        } else {
            let mut ranked: Vec<_> = filtered
                .into_iter()
                .filter_map(|candidate| {
                    search_rank(&candidate, &query).map(|rank| (rank, candidate))
                })
                .collect();
            ranked.sort_by(|(left, _), (right, _)| left.cmp(right));
            push_section(
                &mut rows,
                t("projectSwitcher", "sectionResults"),
                ranked
                    .into_iter()
                    .map(|(_, candidate)| candidate)
                    .collect(),
                QUERY_RESULT_CAP,
            );
        }
        if !rows.iter().any(|row| matches!(row, PaletteRow::Item(_))) {
            rows.clear();
            rows.push(PaletteRow::Empty(
                t("projectSwitcher", "noMatch").to_string(),
            ));
        }
        let selectable_count = rows
            .iter()
            .filter(|row| matches!(row, PaletteRow::Item(candidate) if candidate.selectable))
            .count();
        VisibleModel {
            rows,
            selectable_count,
        }
    }

    fn move_cursor(&mut self, delta: i32, window: &Window, cx: &mut Context<Self>) {
        let model = self.model(cx);
        self.cursor = next_cursor(self.cursor, model.selectable_count, delta);
        if let Some(row_top) = model.selectable_row_top(self.cursor) {
            let palette_height = (window.viewport_size().height.to_f64() * 0.80).min(680.0);
            let filter_height = if self.filters_open { 190.0 } else { 0.0 };
            let list_height = (palette_height - 54.0 - 38.0 - filter_height).max(56.0) as f32;
            let row_bottom = row_top + 56.0;
            let offset = self.scroll.offset();
            let visible_top = (-offset.y.to_f64()).max(0.0) as f32;
            let visible_bottom = visible_top + list_height;
            let next_top = if row_top < visible_top {
                row_top
            } else if row_bottom > visible_bottom {
                row_bottom - list_height
            } else {
                visible_top
            };
            self.scroll
                .set_offset(gpui::point(offset.x, px(-next_top.max(0.0))));
        }
        cx.notify();
    }

    fn commit_direct(&mut self, number: usize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.model(cx).selectable_count;
        let Some(index) = direct_selectable_index(number, count) else {
            return;
        };
        self.cursor = index;
        self.commit_selected(window, cx);
    }

    fn commit_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model(cx);
        let cursor = self.cursor.min(model.selectable_count.saturating_sub(1));
        let Some(candidate) = model.candidate_at(cursor).cloned() else {
            return;
        };
        self.commit_candidate(candidate, window, cx);
    }

    fn commit_candidate(
        &mut self,
        candidate: JumpCandidate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let command = match &candidate.item {
            JumpItem::Setting(page) => Some(JumpCommand::Settings(Some(*page))),
            JumpItem::Action(action) => Some(match action {
                JumpAction::Settings => JumpCommand::Settings(None),
                JumpAction::Usage => JumpCommand::Usage,
                JumpAction::AddProject => JumpCommand::AddProject,
                JumpAction::NewTerminal => JumpCommand::NewTerminal,
                JumpAction::FileSearch => JumpCommand::FileSearch,
            }),
            JumpItem::Agent(_) | JumpItem::Terminal(_) | JumpItem::Worktree(_) => None,
        };
        if let Some(command) = command {
            let on_command = self.on_command.clone();
            window.defer(cx, move |window, cx| {
                if close_guarded(kind::PROJECT_SWITCHER, window, cx) {
                    on_command(command, window, cx);
                }
            });
            return;
        }

        let success = match &candidate.item {
            JumpItem::Agent(agent) => {
                AppStore::activate_agent_run(&self.store, &agent.run_id, window, cx)
            }
            JumpItem::Terminal(terminal) => AppStore::activate_terminal_jump_target(
                &self.store,
                &terminal.target,
                window,
                cx,
            ),
            JumpItem::Worktree(target) => crate::worktree_catalog::activate_target(
                &self.catalog,
                &self.store,
                target,
                window,
                cx,
            )
            .is_ok(),
            JumpItem::Setting(_) | JumpItem::Action(_) => false,
        };
        if success {
            window.defer(cx, |window, cx| {
                close_guarded(kind::PROJECT_SWITCHER, window, cx);
            });
        } else {
            crate::toast::push_message(
                crate::notify::ToastKind::WslInfo,
                candidate
                    .project
                    .as_ref()
                    .map(|project| project.key.clone())
                    .unwrap_or_else(|| "jump-palette".into()),
                candidate.title,
                t("projectSwitcher", "staleTarget").to_string(),
                cx,
            );
        }
    }

    fn render_filters(&self, cx: &mut Context<Self>) -> AnyElement {
        let candidates = build_candidates(
            &self.store,
            &self.catalog,
            self.recency.read(cx),
            cx,
        );
        let options = FilterOptions::from_candidates(&candidates);
        let mut families = div().flex().flex_wrap().gap(px(5.0));
        for family in options.families {
            let selected = self.filters.families.is_empty()
                || self.filters.families.contains(&family);
            families = families.child(
                filter_chip(
                    SharedString::from(format!("jump-family-{family:?}")),
                    family.label(),
                    selected,
                )
                .on_click(cx.listener(move |this, _event, _window, cx| {
                    toggle_selected(&mut this.filters.families, family);
                    this.reset_selection(cx);
                })),
            );
        }

        let mut hosts = div().flex().flex_wrap().gap(px(5.0));
        for option in options.hosts {
            let selected = self.filters.hosts.is_empty()
                || self.filters.hosts.contains(&option.key);
            let key = option.key.clone();
            hosts = hosts.child(
                filter_chip(
                    SharedString::from(format!("jump-host-{key}")),
                    option.label,
                    selected,
                )
                .on_click(cx.listener(move |this, _event, _window, cx| {
                    toggle_selected(&mut this.filters.hosts, key.clone());
                    this.reset_selection(cx);
                })),
            );
        }

        let mut projects = div().flex().flex_wrap().gap(px(5.0));
        for option in options.projects {
            let selected = self.filters.projects.is_empty()
                || self.filters.projects.contains(&option.key);
            let key = option.key.clone();
            projects = projects.child(
                filter_chip(
                    SharedString::from(format!("jump-project-{key}")),
                    option.label,
                    selected,
                )
                .on_click(cx.listener(move |this, _event, _window, cx| {
                    toggle_selected(&mut this.filters.projects, key.clone());
                    this.reset_selection(cx);
                })),
            );
        }

        div()
            .w_full()
            .max_h(px(190.0))
            .overflow_y_scroll()
            .px(px(14.0))
            .py(px(10.0))
            .border_b_1()
            .border_color(ui::border_subtle())
            .bg(ui::bg_elevated())
            .child(filter_group(t("projectSwitcher", "filterType"), families))
            .child(filter_group(t("projectSwitcher", "filterHost"), hosts))
            .child(filter_group(t("projectSwitcher", "filterProject"), projects))
            .when(self.filters.is_active(), |panel| {
                panel.child(
                    div()
                        .id("jump-clear-filters")
                        .mt(px(8.0))
                        .cursor_pointer()
                        .text_size(ui::font_px(10.5))
                        .text_color(ui::accent())
                        .child(t("projectSwitcher", "clearFilters"))
                        .on_click(cx.listener(|this, _event, _window, cx| {
                            this.filters.clear();
                            this.reset_selection(cx);
                        })),
                )
            })
            .into_any_element()
    }

    fn render_candidate(
        &self,
        candidate: &JumpCandidate,
        selectable_index: Option<usize>,
        selected: bool,
        query: &NormalizedQuery,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ranges = title_match_ranges(&candidate.title, query);
        let mut title = div().flex().items_center().min_w_0().overflow_hidden();
        for (text, hit) in ui::highlight_runs(&candidate.title, &ranges) {
            title = title.child(
                div()
                    .flex_none()
                    .text_size(ui::font_px(12.0))
                    .when(hit, |text| {
                        text.text_color(ui::accent())
                            .font_weight(FontWeight::SEMIBOLD)
                    })
                    .when(!hit, |text| text.text_color(ui::text_primary()))
                    .child(SharedString::from(text)),
            );
        }
        let mut badges = div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .max_w(gpui::relative(0.48))
            .overflow_hidden()
            .flex_none();
        for badge in candidate.badges.iter().filter(|badge| !badge.is_empty()).take(3) {
            badges = badges.child(
                div()
                    .max_w(px(112.0))
                    .truncate()
                    .px(px(5.0))
                    .py(px(2.0))
                    .rounded(px(3.0))
                    .bg(ui::bg_elevated())
                    .text_size(ui::font_px(9.0))
                    .text_color(ui::text_muted())
                    .child(badge.clone()),
            );
        }
        if let Some(age) = candidate.timestamp.map(relative_age) {
            badges = badges.child(
                div()
                    .text_size(ui::font_px(9.0))
                    .text_color(ui::text_muted())
                    .child(age),
            );
        }
        if let Some(index) = selectable_index.filter(|index| *index < 9) {
            badges = badges.child(kbd(format!("Ctrl+{}", index + 1)));
        }

        let mut row = div()
            .id(SharedString::from(format!("jump-row-{}", candidate.source_order)))
            .h(px(56.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(9.0))
            .px(px(12.0))
            .mx(px(5.0))
            .rounded(px(4.0))
            .when(candidate.selectable, |row| row.cursor_pointer())
            .when(!candidate.selectable, |row| row.opacity(0.48))
            .when(selected, |row| row.bg(ui::accent_subtle()))
            .when(!selected && candidate.selectable, |row| {
                row.hover(|row| row.bg(ui::border_subtle()))
            })
            .child(render_status(candidate))
            .child(
                div()
                    .w(px(22.0))
                    .h(px(22.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(render_item_icon(&candidate.item)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(title)
                    .child(
                        div()
                            .truncate()
                            .text_size(ui::font_px(9.75))
                            .text_color(ui::text_muted())
                            .child(candidate.subtitle.clone()),
                    ),
            )
            .child(badges);
        if let Some(index) = selectable_index {
            row = row
                .on_hover(cx.listener(move |this, hovered: &bool, _window, cx| {
                    if *hovered && this.cursor != index {
                        this.cursor = index;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _event, window, cx| {
                    this.cursor = index;
                    this.commit_selected(window, cx);
                }));
        }
        row.into_any_element()
    }
}

impl Render for JumpPalette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model(cx);
        if model.selectable_count == 0 {
            self.cursor = 0;
        } else {
            self.cursor = self.cursor.min(model.selectable_count - 1);
        }
        let query_text = self.query.read(cx).value().to_string();
        let normalized = normalize_query(&query_text).unwrap_or(NormalizedQuery {
            phrase: String::new(),
            tokens: Vec::new(),
        });
        let mut selectable_index = 0usize;
        let mut list = div()
            .id("jump-palette-list")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .py(px(4.0));
        for row in &model.rows {
            match row {
                PaletteRow::Header(label) => {
                    list = list.child(
                        div()
                            .h(px(28.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .px(px(14.0))
                            .pt(px(5.0))
                            .text_size(ui::font_px(9.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(ui::text_muted())
                            .child(label.clone()),
                    );
                }
                PaletteRow::Item(candidate) => {
                    let index = candidate.selectable.then_some(selectable_index);
                    let selected = index == Some(self.cursor);
                    list = list.child(self.render_candidate(
                        candidate,
                        index,
                        selected,
                        &normalized,
                        cx,
                    ));
                    if candidate.selectable {
                        selectable_index += 1;
                    }
                }
                PaletteRow::Overflow(label) => {
                    list = list.child(
                        div()
                            .h(px(30.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .px(px(14.0))
                            .text_size(ui::font_px(9.5))
                            .text_color(ui::text_muted())
                            .child(label.clone()),
                    );
                }
                PaletteRow::Empty(label) => {
                    list = list.child(
                        div()
                            .py(px(40.0))
                            .text_center()
                            .text_size(ui::font_px(11.0))
                            .text_color(ui::text_muted())
                            .child(label.clone()),
                    );
                }
            }
        }

        let filter_active = self.filters.is_active();
        div()
            .key_context("JumpPalette")
            .on_action(cx.listener(|this, _: &JumpPrev, window, cx| {
                this.move_cursor(-1, window, cx)
            }))
            .on_action(cx.listener(|this, _: &JumpNext, window, cx| {
                this.move_cursor(1, window, cx)
            }))
            .on_action(cx.listener(|this, _: &JumpToggleFilter, _window, cx| {
                this.toggle_filters(cx)
            }))
            .on_action(cx.listener(|this, action: &JumpDirect, window, cx| {
                this.commit_direct(action.0, window, cx)
            }))
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(ui::bg_surface())
            .child(
                div()
                    .h(px(54.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(9.0))
                    .px(px(13.0))
                    .border_b_1()
                    .border_color(ui::border_subtle())
                    .child(
                        VectorIcon::new(SEARCH_ICON, px(17.0)).ink(ui::text_muted()),
                    )
                    .child(div().flex_1().min_w_0().child(Input::new(&self.query).cleanable(false)))
                    .child(
                        div()
                            .id("jump-filter-toggle")
                            .h(px(30.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(5.0))
                            .px(px(9.0))
                            .rounded(px(4.0))
                            .cursor_pointer()
                            .border_1()
                            .border_color(if filter_active {
                                ui::accent()
                            } else {
                                ui::border_default()
                            })
                            .text_size(ui::font_px(10.5))
                            .text_color(if filter_active {
                                ui::accent()
                            } else {
                                ui::text_secondary()
                            })
                            .hover(|button| button.bg(ui::border_subtle()))
                            .child(
                                VectorIcon::new(FILTER_ICON, px(13.0)).ink(if filter_active {
                                    ui::accent()
                                } else {
                                    ui::text_muted()
                                }),
                            )
                            .child(t("projectSwitcher", "filter"))
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.toggle_filters(cx)
                            })),
                    ),
            )
            .when(self.filters_open, |palette| {
                palette.child(self.render_filters(cx))
            })
            .child(list)
            .child(
                div()
                    .min_h(px(38.0))
                    .flex_none()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(14.0))
                    .px(px(13.0))
                    .py(px(7.0))
                    .border_t_1()
                    .border_color(ui::border_subtle())
                    .child(hint(&["Up", "Down"], t("projectSwitcher", "hintMove")))
                    .child(hint(&["Enter"], t("projectSwitcher", "hintOpen")))
                    .child(hint(&["Esc"], t("projectSwitcher", "hintClose")))
                    .child(hint(&["Tab"], t("projectSwitcher", "hintFilter"))),
            )
    }
}

fn frozen_candidates(
    candidates: &[JumpCandidate],
    frozen_ids: &[String],
    include: impl Fn(&JumpCandidate) -> bool,
) -> Vec<JumpCandidate> {
    let mut by_id: HashMap<_, _> = candidates
        .iter()
        .filter(|candidate| include(candidate))
        .map(|candidate| (candidate.id.clone(), candidate.clone()))
        .collect();
    let mut ordered = Vec::new();
    for id in frozen_ids {
        if let Some(candidate) = by_id.remove(id) {
            ordered.push(candidate);
        }
    }
    let mut appended: Vec<_> = by_id.into_values().collect();
    appended.sort_by_key(|candidate| candidate.source_order);
    ordered.extend(appended);
    ordered
}

fn push_section(
    rows: &mut Vec<PaletteRow>,
    label: &str,
    candidates: Vec<JumpCandidate>,
    cap: usize,
) {
    if candidates.is_empty() {
        return;
    }
    rows.push(PaletteRow::Header(label.to_string()));
    let hidden = candidates.len().saturating_sub(cap);
    rows.extend(candidates.into_iter().take(cap).map(PaletteRow::Item));
    if hidden > 0 {
        rows.push(PaletteRow::Overflow(format!(
            "+{hidden} {}",
            t("projectSwitcher", "moreResults")
        )));
    }
}

fn next_cursor(cursor: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    (cursor as i64 + delta as i64).rem_euclid(len as i64) as usize
}

fn direct_selectable_index(number: usize, count: usize) -> Option<usize> {
    if !(1..=9).contains(&number) {
        return None;
    }
    let index = number - 1;
    (index < count).then_some(index)
}

fn relative_age(timestamp: i64) -> String {
    let seconds = unix_ms().saturating_sub(timestamp).max(0) / 1000;
    if seconds < 60 {
        t("projectSwitcher", "ageNow").to_string()
    } else if seconds < 60 * 60 {
        format!("{}m", seconds / 60)
    } else if seconds < 24 * 60 * 60 {
        format!("{}h", seconds / (60 * 60))
    } else {
        format!("{}d", seconds / (24 * 60 * 60))
    }
}

fn render_status(candidate: &JumpCandidate) -> AnyElement {
    let lane = div()
        .w(px(12.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center();
    match &candidate.item {
        JumpItem::Agent(agent) => lane
            .child(
                div()
                    .w(px(7.0))
                    .h(px(7.0))
                    .rounded_full()
                    .bg(agent_status_color(agent)),
            )
            .into_any_element(),
        JumpItem::Terminal(_) | JumpItem::Worktree(_) => {
            if let Some(status) = candidate.pane_status
                && status != PaneStatus::Idle
            {
                return lane.child(ui::status_dot(status)).into_any_element();
            }
            lane.when(candidate.warning, |lane| {
                lane.child(
                    div()
                        .w(px(7.0))
                        .h(px(7.0))
                        .rounded_full()
                        .bg(ui::color_warning()),
                )
            })
            .when(candidate.active && !candidate.warning, |lane| {
                lane.child(
                    div()
                        .w(px(7.0))
                        .h(px(7.0))
                        .rounded_full()
                        .bg(ui::color_success()),
                )
            })
            .into_any_element()
        }
        JumpItem::Setting(_) | JumpItem::Action(_) => lane.into_any_element(),
    }
}

fn agent_status_color(agent: &AgentTargetView) -> gpui::Hsla {
    if agent.attention || matches!(agent.activity, AgentActivity::Blocked | AgentActivity::Failed) {
        ui::color_warning()
    } else if matches!(agent.activity, AgentActivity::Starting | AgentActivity::Working) {
        ui::accent()
    } else if agent.connectivity != AgentConnectivity::Live {
        ui::text_muted()
    } else {
        ui::color_success()
    }
}

fn render_item_icon(item: &JumpItem) -> AnyElement {
    match item {
        JumpItem::Agent(agent) => {
            let provider = agent.provider.as_str();
            let vendor = if provider == "codex" {
                Some(AiVendor::OpenAi)
            } else {
                AiVendor::from_session_type(provider)
                    .or_else(|| AiVendor::infer(Some(provider), None))
            };
            BrandIcon::new(vendor)
                .size(px(16.0))
                .color(ui::text_secondary())
                .into_any_element()
        }
        JumpItem::Terminal(_) => VectorIcon::new(crate::activity_bar::TERMINALS, px(16.0))
            .ink(ui::text_secondary())
            .into_any_element(),
        JumpItem::Worktree(_) => VectorIcon::new(crate::activity_bar::GIT, px(16.0))
            .ink(ui::text_secondary())
            .into_any_element(),
        JumpItem::Setting(_) => VectorIcon::new(crate::activity_bar::SETTINGS, px(16.0))
            .ink(ui::text_secondary())
            .into_any_element(),
        JumpItem::Action(action) => {
            let shapes = match action {
                JumpAction::Settings => crate::activity_bar::SETTINGS,
                JumpAction::NewTerminal => crate::activity_bar::TERMINALS,
                JumpAction::AddProject => PLUS_ICON,
                JumpAction::FileSearch => SEARCH_ICON,
                JumpAction::Usage => COMMAND_ICON,
            };
            VectorIcon::new(shapes, px(16.0))
                .ink(ui::text_secondary())
                .into_any_element()
        }
    }
}

fn filter_chip(
    id: SharedString,
    label: impl Into<SharedString>,
    selected: bool,
) -> gpui::Stateful<gpui::Div> {
    let label = label.into();
    div()
        .id(id)
        .h(px(25.0))
        .flex()
        .items_center()
        .px(px(7.0))
        .rounded(px(3.0))
        .cursor_pointer()
        .border_1()
        .border_color(if selected {
            ui::accent()
        } else {
            ui::border_default()
        })
        .bg(if selected {
            ui::accent_subtle()
        } else {
            ui::bg_surface()
        })
        .text_size(ui::font_px(10.0))
        .text_color(if selected {
            ui::accent()
        } else {
            ui::text_muted()
        })
        .hover(|chip| chip.bg(ui::border_subtle()))
        .child(label)
}

fn filter_group(label: &'static str, choices: gpui::Div) -> impl IntoElement {
    div()
        .mb(px(8.0))
        .child(
            div()
                .mb(px(5.0))
                .text_size(ui::font_px(9.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(ui::text_muted())
                .child(label),
        )
        .child(choices)
}

fn kbd(label: String) -> impl IntoElement {
    div()
        .px(px(5.0))
        .py(px(1.0))
        .rounded(px(3.0))
        .border_1()
        .border_color(ui::border_default())
        .bg(ui::bg_elevated())
        .text_size(ui::font_px(9.0))
        .text_color(ui::text_muted())
        .child(label)
}

fn hint(keys: &[&'static str], label: &'static str) -> impl IntoElement {
    let mut row = div().flex().items_center().gap(px(3.0));
    for key in keys {
        row = row.child(kbd((*key).to_string()));
    }
    row.child(
        div()
            .ml(px(2.0))
            .text_size(ui::font_px(9.5))
            .text_color(ui::text_muted())
            .child(label),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{KeyBinding, KeyContext, Keymap, Keystroke};

    fn candidate(title: &str, fields: &[&str], order: usize) -> JumpCandidate {
        JumpCandidate::new(
            format!("item-{order}"),
            JumpItem::Action(JumpAction::Settings),
            title.into(),
            String::new(),
            fields.iter().map(|field| (*field).into()).collect(),
            Vec::new(),
            Vec::new(),
            None,
            None,
            order,
        )
    }

    #[test]
    fn query_is_unicode_safe_and_bounded_before_ranking() {
        let query = normalize_query("  项目  CODEX ").unwrap();
        assert_eq!(query.tokens, vec!["项目", "codex"]);
        assert!(normalize_query(&"é".repeat(1024)).is_ok());
        assert!(normalize_query(&"é".repeat(1025)).is_err());
        assert_eq!(
            title_match_ranges("前端项目", &normalize_query("项目").unwrap()),
            vec![(2, 4)]
        );
    }

    #[test]
    fn rank_prefers_exact_then_prefix_then_token_coverage() {
        let exact = candidate("Open settings", &["preferences"], 2);
        let prefix = candidate("Open settings page", &[], 1);
        let coverage = candidate("Preferences", &["open settings"], 0);
        let query = normalize_query("open settings").unwrap();
        assert!(search_rank(&exact, &query) < search_rank(&prefix, &query));
        assert!(search_rank(&prefix, &query) < search_rank(&coverage, &query));
    }

    #[test]
    fn filters_reconcile_removed_options_and_empty_means_all() {
        let mut filters = FilterState::default();
        filters.hosts.insert("gone".into());
        filters.projects.insert("project".into());
        assert!(filters.reconcile(&FilterOptions {
            families: vec![JumpFamily::Action],
            hosts: vec![FilterOption {
                key: "host".into(),
                label: "Host".into(),
            }],
            projects: Vec::new(),
        }));
        assert!(filters.hosts.is_empty());
        assert!(filters.projects.is_empty());
        let row = candidate("Settings", &[], 0);
        assert!(filters.matches(&row));
    }

    #[test]
    fn recency_records_only_observed_target_changes() {
        let mut last = Some("current");
        let mut entries = Vec::new();
        assert!(!record_recency(Some("current"), &mut last, &mut entries));
        assert!(entries.is_empty());

        assert!(record_recency(Some("next"), &mut last, &mut entries));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "next");
    }

    #[test]
    fn terminal_agent_dedupe_uses_the_complete_runtime_route() {
        use mt_identity::{
            ExecutionHostId, HostInstallId, PaneKey, RepoId, TabId,
            TerminalIncarnationId, TerminalSessionId,
        };

        let host = ExecutionHostId::derive("jump-test", &HostInstallId::new());
        let route = AgentRoute {
            execution_host_id: host.clone(),
            worktree_id: WorktreeId::derive(
                &RepoId::derive(&host, "/repo/.git"),
                "/repo",
                None,
            ),
            tab_id: TabId::new(),
            pane_key: PaneKey::new(),
            terminal_session_id: TerminalSessionId::new(),
            terminal_incarnation_id: TerminalIncarnationId::new(),
        };
        let target = TerminalJumpTarget {
            project_id: "project".into(),
            execution_host_id: route.execution_host_id.clone(),
            worktree_id: route.worktree_id.clone(),
            tab_id: route.tab_id.clone(),
            pane_key: route.pane_key.clone(),
            terminal_session_id: route.terminal_session_id.clone(),
            terminal_incarnation_id: Some(route.terminal_incarnation_id.clone()),
        };
        assert!(terminal_matches_agent(&target, &route));

        let mut reused = target.clone();
        reused.terminal_session_id = TerminalSessionId::new();
        assert!(!terminal_matches_agent(&reused, &route));
        let mut reused = target;
        reused.terminal_incarnation_id = Some(TerminalIncarnationId::new());
        assert!(!terminal_matches_agent(&reused, &route));
    }

    #[test]
    fn direct_keys_address_only_the_first_nine_selectable_rows() {
        assert_eq!(direct_selectable_index(1, 12), Some(0));
        assert_eq!(direct_selectable_index(9, 12), Some(8));
        assert_eq!(direct_selectable_index(10, 12), None);
        assert_eq!(direct_selectable_index(3, 2), None);
    }

    #[test]
    fn selectable_row_offsets_include_headers_and_disabled_rows() {
        let mut disabled = candidate("Disabled", &[], 0);
        disabled.selectable = false;
        let model = VisibleModel {
            rows: vec![
                PaletteRow::Header("Section".into()),
                PaletteRow::Item(disabled),
                PaletteRow::Item(candidate("Selectable", &[], 1)),
            ],
            selectable_count: 1,
        };
        assert_eq!(model.selectable_row_top(0), Some(88.0));
    }

    #[test]
    fn empty_order_is_attention_then_active_then_mru_then_source() {
        let source = candidate("Source", &[], 0);
        let mut mru = candidate("MRU", &[], 1);
        mru.timestamp = Some(2);
        let mut active = candidate("Active", &[], 2);
        active.active = true;
        let mut attention = candidate("Attention", &[], 3);
        attention.attention = true;
        let mut rows = vec![source.clone(), mru.clone(), active.clone(), attention.clone()];
        rows.sort_by(compare_recent);
        assert_eq!(
            rows.into_iter().map(|row| row.title).collect::<Vec<_>>(),
            vec!["Attention", "Active", "MRU", "Source"]
        );
    }

    #[test]
    fn palette_bindings_override_input_and_workspace_bindings() {
        gpui::actions!(test_only, [InputMoveUp, WorkspaceDirect]);

        let keymap = Keymap::new(vec![
            KeyBinding::new("up", InputMoveUp, Some("Input")),
            KeyBinding::new("ctrl-1", WorkspaceDirect, Some("Workspace")),
            KeyBinding::new("up", JumpPrev, Some("JumpPalette > Input")),
            KeyBinding::new("ctrl-1", JumpDirect(1), Some("JumpPalette > Input")),
        ]);
        let palette_stack = context_stack(&["Workspace", "Dialog", "JumpPalette", "Input"]);

        let (bindings, _) =
            keymap.bindings_for_input(&[Keystroke::parse("up").unwrap()], &palette_stack);
        assert!(!bindings.is_empty());
        assert!(bindings[0].action().partial_eq(&JumpPrev));

        let (bindings, _) =
            keymap.bindings_for_input(&[Keystroke::parse("ctrl-1").unwrap()], &palette_stack);
        assert!(!bindings.is_empty());
        assert!(bindings[0].action().partial_eq(&JumpDirect(1)));

        let input_stack = context_stack(&["Workspace", "Dialog", "Input"]);
        let (bindings, _) =
            keymap.bindings_for_input(&[Keystroke::parse("up").unwrap()], &input_stack);
        assert!(!bindings.is_empty());
        assert!(bindings[0].action().partial_eq(&InputMoveUp));

        let workspace_stack = context_stack(&["Workspace"]);
        let (bindings, _) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-1").unwrap()],
            &workspace_stack,
        );
        assert!(!bindings.is_empty());
        assert!(bindings[0].action().partial_eq(&WorkspaceDirect));
    }

    fn context_stack(names: &[&str]) -> Vec<KeyContext> {
        names
            .iter()
            .map(|name| KeyContext::parse(name).expect("key context must parse"))
            .collect()
    }
}
