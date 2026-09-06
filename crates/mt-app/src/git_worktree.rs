//! Worktree 管理弹窗。对应 `src/components/GitWorktreeModal.tsx`(833 行)。
//!
//! # 两个入口(语义不同)
//!
//! | 入口 | `discover_repos` | `on_changed` |
//! |---|---|---|
//! | Git 面板仓库栏右键「Worktree 管理」 | `false`(单仓库,`repo_path` 就是仓库根) | 刷新仓库列表 |
//! | 项目列表右键「Worktrees」 | `true`(项目根未必是仓库,向下发现) | 空函数(后端已失效缓存) |
//!
//! Both entries capture an execution source before yielding. Backend calls run
//! in detached background workers; dialog lifetime never releases a dispatched
//! mutation. Registration and cleanup require normal verified postconditions.

use std::collections::HashMap;
#[cfg(test)]
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use mt_project::git::{BranchInfo, WorktreeInfo};

use crate::i18n::{t, tr};
use crate::menu::{self, MenuItem};
use crate::prompt::{autofocus, kind, open_guarded_with_close, show_alert};
use crate::git_panel::host_ui;
use crate::git_backend::{GitBackend, GitLifetime, GitRead, GitReadValue, GitRepository, GitWorktrees, GitWrite, GitWriteOutcome, GitPostcondition, PreparedGitWrite};
use crate::execution_host::{self, ExecutionBackend, ProjectExecutionSnapshot};
use mt_project::git::cli::{GitRef, ObjectId, RepositoryAuthority};
use crate::store::{ProjectLocationKey, ProjectPlacement};
use crate::store::AppStore;
use crate::ui;

mod actions;
use actions::{browse_destination, create, open_remove_confirm, open_worktree, prune, review_uncertain};

fn host_path_key(backend: &ExecutionBackend, path: &str) -> String {
    match backend {
        ExecutionBackend::Local => normalize_path(path),
        ExecutionBackend::Wsl { .. } | ExecutionBackend::Ssh { .. } => path.to_string(),
    }
}

fn discovery_selector_matches(backend: &ExecutionBackend, selector: &str, configured_path: &str) -> bool {
    let Ok(selector) = execution_host::configured_execution_path(backend, selector) else { return false; };
    let Ok(configured) = execution_host::configured_execution_path(backend, configured_path) else { return false; };
    host_path_key(backend, &selector) == host_path_key(backend, &configured)
}

fn host_parent<'a>(backend: &ExecutionBackend, path: &'a str) -> &'a str {
    match backend {
        ExecutionBackend::Local => parent_dir(path),
        ExecutionBackend::Wsl { .. } | ExecutionBackend::Ssh { .. } => {
            let path = path.trim_end_matches('/');
            match path.rfind('/') { Some(0) => "/", Some(index) => &path[..index], None => path }
        }
    }
}

fn host_name<'a>(backend: &ExecutionBackend, path: &'a str) -> &'a str {
    match backend {
        ExecutionBackend::Local => base_name(path),
        ExecutionBackend::Wsl { .. } | ExecutionBackend::Ssh { .. } => path.trim_end_matches('/').rsplit('/').next().unwrap_or(path),
    }
}

fn host_join(backend: &ExecutionBackend, parent: &str, name: &str) -> String {
    match backend {
        ExecutionBackend::Local => join_path(parent, name, if cfg!(windows) && parent.contains('\\') { '\\' } else { '/' }),
        ExecutionBackend::Wsl { .. } | ExecutionBackend::Ssh { .. } => format!("{}/{name}", parent.trim_end_matches('/')),
    }
}

fn project_location(backend: &ExecutionBackend, path: &str) -> Result<(ProjectLocationKey, String), String> {
    match backend {
        ExecutionBackend::Ssh { connection, .. } => {
            let path = execution_host::normalize_absolute_posix_path(path)?;
            Ok((ProjectLocationKey::Ssh { connection_id: connection.id.clone(), normalized_posix_path: path.clone() }, path))
        }
        ExecutionBackend::Local | ExecutionBackend::Wsl { .. } => {
            let path = match backend {
                ExecutionBackend::Wsl { distro } => crate::project_onboarding::DirectoryLocation {
                    source: crate::project_onboarding::DirectorySource::Wsl { distro: distro.clone() },
                    path: execution_host::normalize_absolute_posix_path(path)?,
                }.host_path().map_err(|error| error.detail)?,
                _ => path.to_string(),
            };
            Ok((ProjectLocationKey::Local { normalized_canonical_path: execution_host::normalize_host_visible_project_path(&path)? }, path))
        }
    }
}

// ─── 纯逻辑小件 ───────────────────────────────────────────────

/// Legacy native path comparison for external project-list callers.
/// Host-aware registration uses ProjectLocationKey instead.
pub fn normalize_path(p: &str) -> String {
    mt_project::worktree::normalize_path_for_comparison(p)
}

/// 分支名 → 目录名片段(`GitWorktreeModal.tsx:45-47`)。
pub fn sanitize_branch_for_dir(branch: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for c in branch.chars() {
        let bad = matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
            || c.is_whitespace();
        if bad {
            if !last_dash {
                out.push('-');
                last_dash = true;
            }
        } else {
            out.push(c);
            last_dash = false;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "worktree".to_string()
    } else {
        trimmed.to_string()
    }
}

/// 拼路径(分隔符跟随输入)。
pub fn join_path(base: &str, child: &str, sep: char) -> String {
    let base = if cfg!(windows) {
        base.trim_end_matches(['/', '\\'])
    } else {
        base.trim_end_matches('/')
    };
    format!("{base}{sep}{child}")
}

/// 父目录。没有父级时返回原串。
pub fn parent_dir(path: &str) -> &str {
    let trimmed = if cfg!(windows) {
        path.trim_end_matches(['/', '\\'])
    } else {
        path.trim_end_matches('/')
    };
    let separator = if cfg!(windows) {
        trimmed.rfind(['/', '\\'])
    } else {
        trimmed.rfind('/')
    };
    match separator {
        Some(0) => &trimmed[..1],
        Some(idx) => &trimmed[..idx],
        None => trimmed,
    }
}

/// 末段名。
pub fn base_name(path: &str) -> &str {
    let trimmed = if cfg!(windows) {
        path.trim_end_matches(['/', '\\'])
    } else {
        path.trim_end_matches('/')
    };
    let separator = if cfg!(windows) {
        trimmed.rfind(['/', '\\'])
    } else {
        trimmed.rfind('/')
    };
    match separator {
        Some(idx) => &trimmed[idx + 1..],
        None => trimmed,
    }
}

/// 「检出现有分支」的可选项(`GitWorktreeModal.tsx:243-258`)。
///
/// 逐组算「本地分支 − 该组已被任一 worktree 占用的分支」,再取**全组交集**,
/// 顺序按第一个组的列表。
pub fn available_branches(groups: &[(Vec<BranchInfo>, Vec<Option<String>>)]) -> Vec<String> {
    let per_group: Vec<Vec<String>> = groups
        .iter()
        .map(|(branches, occupied)| {
            branches
                .iter()
                .filter(|b| !b.is_remote)
                .map(|b| b.name.clone())
                .filter(|name| !occupied.iter().any(|o| o.as_deref() == Some(name)))
                .collect()
        })
        .collect();
    intersect_ordered(&per_group)
}

/// 「新分支起点」的可选项(`:261-268`):逐组的**全部**分支名(含远程)取交集。
pub fn base_branch_options(groups: &[Vec<BranchInfo>]) -> Vec<String> {
    let per_group: Vec<Vec<String>> = groups
        .iter()
        .map(|branches| branches.iter().map(|b| b.name.clone()).collect())
        .collect();
    intersect_ordered(&per_group)
}

/// 取交集,顺序按第一组。
fn intersect_ordered(groups: &[Vec<String>]) -> Vec<String> {
    let Some(first) = groups.first() else {
        return Vec::new();
    };
    first
        .iter()
        .filter(|name| groups[1..].iter().all(|g| g.contains(name)))
        .cloned()
        .collect()
}

// ─── 分组 ─────────────────────────────────────────────────────

/// 归并后的一组(`GitWorktreeModal.tsx:31-42`)。
#[derive(Clone)]
struct RepoGroup {
    generation: u64,
    /// Main worktree path under this modal's captured host semantics.
    key: String,
    /// 主仓库目录名(worktree 目录建议名的前缀)。
    name: String,
    /// Display/group identity, not permission to resolve a new execution host.
    main_path: String,
    worktrees: Vec<WorktreeInfo>,
    readable: bool,
    authoritative: bool,
    error: Option<String>,
}

struct RepoLoad {
    path: String,
    result: Result<GitWorktrees, String>,
}

fn previous_group_for_path<'a>(previous: &'a [RepoGroup], path: &str, backend: &ExecutionBackend) -> Option<&'a RepoGroup> {
    previous.iter().find(|group| {
        host_path_key(backend, &group.main_path) == host_path_key(backend, path)
            || group
                .worktrees
                .iter()
                .any(|worktree| host_path_key(backend, &worktree.path) == host_path_key(backend, path))
    })
}

/// 把逐仓库的 catalog 结果归并成组。非权威结果或失败会保留上一帧的组。
fn merge_groups_on_host(items: Vec<RepoLoad>, previous: &[RepoGroup], backend: &ExecutionBackend) -> Vec<RepoGroup> {
    let mut out: Vec<RepoGroup> = Vec::new();
    for RepoLoad { path, result } in items {
        match result {
            Ok(GitWorktrees { scan, entries }) => {
                let mut worktrees = entries;
                let mut readable = scan.source != mt_project::worktree::WorktreeScanSource::LastKnown;
                if !scan.authoritative
                    && scan.source != mt_project::worktree::WorktreeScanSource::Libgit2Fallback
                    && let Some(old) = previous_group_for_path(previous, &path, backend)
                {
                    worktrees = old.worktrees.clone();
                    readable = false;
                }
                let main_path = worktrees
                    .iter()
                    .find(|w| w.is_main)
                    .map(|w| w.path.clone())
                    .unwrap_or_else(|| path.clone());
                let key = host_path_key(backend, &main_path);
                let group = RepoGroup {
                    generation: 0,
                    key: key.clone(),
                    name: host_name(backend, &main_path).to_string(),
                    main_path,
                    worktrees,
                    readable,
                    authoritative: scan.authoritative,
                    error: scan.warning.or_else(|| (!scan.authoritative).then(|| "Worktree inventory is read-only; authoritative Git inventory is unavailable.".into())),
                };
                if let Some(index) = out.iter().position(|existing| existing.key == key) {
                    if group.authoritative && !out[index].authoritative {
                        out[index] = group;
                    }
                } else {
                    out.push(group);
                }
            }
            Err(err) => {
                if let Some(old) = previous_group_for_path(previous, &path, backend) {
                    if !out.iter().any(|group| group.key == old.key) {
                        let mut retained = old.clone();
                        retained.authoritative = false;
                        retained.readable = false;
                        retained.error = Some(err);
                        out.push(retained);
                    }
                    continue;
                }
                let key = host_path_key(backend, &path);
                if out.iter().any(|g| g.key == key) {
                    continue;
                }
                out.push(RepoGroup {
                    generation: 0,
                    key,
                    name: host_name(backend, &path).to_string(),
                    main_path: path,
                    worktrees: Vec::new(),
                    readable: false,
                    authoritative: false,
                    error: Some(err),
                });
            }
        }
    }
    out
}

#[cfg(test)]
fn merge_groups(items: Vec<RepoLoad>, previous: &[RepoGroup]) -> Vec<RepoGroup> {
    merge_groups_on_host(items, previous, &ExecutionBackend::Local)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Existing,
    New,
}

// ─── 弹窗状态 ─────────────────────────────────────────────────

struct WorktreeModal {
    store: Entity<AppStore>,
    repo_path: String,
    discover_repos: bool,
    snapshot: ProjectExecutionSnapshot,
    expected_repository: Option<RepositoryAuthority>,
    backend: Option<GitBackend>,
    repositories: HashMap<String, GitRepository>,
    lifetime: GitLifetime,
    view_lifetime: GitLifetime,
    child_lifetimes: Vec<GitLifetime>,
    branch_requests: HashMap<String, u64>,
    branch_errors: HashMap<String, String>,
    write_request: u64,
    picker_request: u64,
    _source_watch: Option<gpui::Subscription>,
    _input_watch: Vec<gpui::Subscription>,
    /// `None` = 还在加载。
    groups: Option<Vec<RepoGroup>>,
    load_error: Option<String>,
    load_generation: u64,
    loading: bool,
    selected_keys: Vec<String>,
    branches_by_repo: HashMap<String, Vec<BranchInfo>>,
    mode: Mode,
    sel_branch: String,
    base_branch: String,
    new_branch: Entity<InputState>,
    wt_path: Entity<InputState>,
    /// 用户手改过路径之后就不再跟随建议。
    path_edited: bool,
    suggested_path: String,
    add_as_project: bool,
    creating: bool,
    create_error: Option<String>,
    /// 逐仓库的创建错误(部分失败时留在弹窗里)。
    create_results: Vec<(String, String)>,
    pruning_key: Option<String>,
    removing: bool,
    on_changed: Rc<dyn Fn(&mut App)>,
}

impl WorktreeModal {
    fn is_live(&self, cx: &App) -> bool {
        self.lifetime.is_valid() && self.store.read(cx).project_execution_snapshot(&self.snapshot.project_id).is_ok_and(|current| {
            if let Some(backend) = &self.backend { backend.matches_snapshot(&current) }
            else { host_ui::read_source_matches(&self.snapshot, &current) }
        })
    }

    fn repository(&self, group: &RepoGroup, cx: &App) -> Option<GitRepository> {
        if !group.authoritative { return None; }
        self.read_repository(group, cx)
    }

    fn read_repository(&self, group: &RepoGroup, cx: &App) -> Option<GitRepository> {
        if !self.is_live(cx) || self.loading || !group.readable || group.generation != self.load_generation { return None; }
        self.repositories.get(&group.key).filter(|repo| host_ui::repository_current(repo, &self.store, cx)).cloned()
    }

    fn idle(&self, cx: &App) -> bool {
        self.is_live(cx) && !self.loading && !self.creating && !self.removing && self.pruning_key.is_none()
    }

    fn next_write(&mut self) -> Option<u64> {
        self.write_request = self.write_request.checked_add(1).or_else(|| { self.invalidate(); None })?;
        self.bump_picker()?;
        Some(self.write_request)
    }

    fn bump_picker(&mut self) -> Option<u64> {
        self.picker_request = self.picker_request.checked_add(1).or_else(|| { self.invalidate(); None })?;
        Some(self.picker_request)
    }

    fn invalidate(&self) {
        self.lifetime.invalidate();
        for child in &self.child_lifetimes { child.invalidate(); }
    }

    fn dispose(&self) {
        self.view_lifetime.invalidate();
        self.invalidate();
    }
}

impl Drop for WorktreeModal {
    fn drop(&mut self) { self.dispose(); }
}

impl Render for WorktreeModal {
    /// 状态盒子。真正的画面由 Dialog 的 builder 每帧重建(见 `modal.rs` 的说明)。
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// 打开 worktree 管理弹窗。
pub fn open(
    repo_path: String,
    discover_repos: bool,
    project_id: Option<String>,
    on_changed: impl Fn(&mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    if repo_path.is_empty() || crate::prompt::is_open(kind::GIT_WORKTREE) {
        return;
    }
    let store = AppStore::global(cx);
    let project_id = project_id.or_else(|| store.read(cx).active_project_id.clone());
    let snapshot = match project_id.as_deref().ok_or("No project selected".to_string())
        .and_then(|id| store.read(cx).project_execution_snapshot(id)) {
        Ok(snapshot) => snapshot,
        Err(error) => { show_alert("Git", error, window, cx); return; }
    };
    if discover_repos && !store.read(cx).project(&snapshot.project_id).is_some_and(|project| {
        discovery_selector_matches(&snapshot.backend, &repo_path, &project.path)
    }) {
        show_alert("Git", "Project folder changed; reopen Worktree Management", window, cx);
        return;
    }
    open_host(snapshot, repo_path, discover_repos, None, Rc::new(on_changed), window, cx);
}

pub(crate) fn open_repository(repository: GitRepository, on_changed: impl Fn(&mut App) + 'static,
    window: &mut Window, cx: &mut App) {
    let store = AppStore::global(cx);
    if !host_ui::repository_current(&repository, &store, cx) { return; }
    open_host(repository.backend().snapshot().clone(), repository.authority().worktree_root.clone(),
        false, Some(repository.authority().clone()), Rc::new(on_changed), window, cx);
}

fn open_host(snapshot: ProjectExecutionSnapshot, repo_path: String, discover_repos: bool,
    expected_repository: Option<RepositoryAuthority>, on_changed: Rc<dyn Fn(&mut App)>, window: &mut Window, cx: &mut App) {
    if crate::prompt::is_open(kind::GIT_WORKTREE) { return; }
    let store = AppStore::global(cx);
    let state = cx.new(|cx| {
        let source_watch = cx.observe(&store, |state: &mut WorktreeModal, _, cx| {
            if !state.is_live(cx) {
                state.invalidate();
                state.load_error = Some("Git source changed; reopen Worktree Management".into());
                state.creating = false;
                cx.notify();
            }
        });
        WorktreeModal {
        store: store.clone(),
        repo_path: repo_path.clone(),
        discover_repos,
        snapshot,
        expected_repository,
        backend: None,
        repositories: HashMap::new(),
        lifetime: GitLifetime::new(),
        view_lifetime: GitLifetime::new(),
        child_lifetimes: Vec::new(),
        branch_requests: HashMap::new(),
        branch_errors: HashMap::new(),
        write_request: 0,
        picker_request: 0,
        _source_watch: Some(source_watch),
        _input_watch: Vec::new(),
        groups: None,
        load_error: None,
        load_generation: 0,
        loading: false,
        selected_keys: Vec::new(),
        branches_by_repo: HashMap::new(),
        mode: Mode::Existing,
        sel_branch: String::new(),
        base_branch: String::new(),
        new_branch: cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("worktree", "newBranchPlaceholder"))
        }),
        wt_path: cx
            .new(|cx| InputState::new(window, cx).placeholder(t("worktree", "pathPlaceholder"))),
        path_edited: false,
        suggested_path: String::new(),
        // 默认勾选(`GitWorktreeModal.tsx:198`)
        add_as_project: true,
        creating: false,
        create_error: None,
        create_results: Vec::new(),
        pruning_key: None,
        removing: false,
        on_changed,
    }});

    state.update(cx, |s, cx| {
        s._input_watch.push(cx.subscribe(&s.wt_path, |s: &mut WorktreeModal, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) && input.read(cx).value().as_ref() != s.suggested_path.as_str() {
                s.path_edited = true;
                s.bump_picker();
                cx.notify();
            }
        }));
        s._input_watch.push(cx.subscribe(&s.new_branch, |s: &mut WorktreeModal, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) { s.bump_picker(); cx.notify(); }
        }));
    });
    load(&state, cx);

    let root_name = host_name(&state.read(cx).snapshot.backend, &repo_path).to_string();
    let close_state = state.clone();
    open_guarded_with_close(kind::GIT_WORKTREE, window, cx, move |dialog, window, cx| {
        let body = render_body(&state, window, cx);
        dialog
            // 无底部按钮,右上角 ✕ 是唯一看得见的出口(见 `prompt::dialog_title`)
            .title(actions::manager_title(
                &state,
                tr!("worktree", "title", name = root_name.clone()),
            ))
            .w(px(600.0))
            .child(div().px(px(16.0)).child(body))
    }, move |_, cx| close_state.read(cx).dispose());
}

/// Reload last-known presentation, but revoke row authority until completion.
fn load(state: &Entity<WorktreeModal>, cx: &mut App) {
    let Some((snapshot, lifetime, discover, selector, expected, generation)) = state.update(cx, |s, cx| {
        if !s.is_live(cx) || s.creating || s.removing || s.pruning_key.is_some() { return None; }
        let Some(generation) = s.load_generation.checked_add(1) else {
            s.lifetime.invalidate();
            return None;
        };
        s.load_generation = generation;
        s.loading = true;
        s.branch_requests.clear();
        s.branches_by_repo.clear();
        s.branch_errors.clear();
        s.load_error = None;
        cx.notify();
        Some((s.snapshot.clone(), s.lifetime.clone(), s.discover_repos, s.repo_path.clone(), s.expected_repository.clone(), generation))
    }) else { return; };
    let state = state.clone();
    cx.spawn(async move |cx| {
        let result = cx.background_executor().spawn(async move {
            let backend = GitBackend::connect(snapshot.clone(), lifetime).map_err(|error| error.to_string())?;
            if !host_ui::read_source_matches(&snapshot, backend.snapshot()) {
                return Err("Git source changed during discovery".to_string());
            }
            let mut repositories = backend.discover().map_err(|error| error.to_string())?;
            if !discover {
                let selector = match &snapshot.backend {
                    ExecutionBackend::Wsl { distro } => {
                        if let Some(path) = mt_core::parse_wsl_unc(&selector.replace('/', "\\")) {
                            if !path.distro.eq_ignore_ascii_case(distro) { return Err("Worktree distribution changed".into()); }
                            path.unix_path
                        } else { selector }
                    }
                    _ => selector,
                };
                let source_selected = host_path_key(&snapshot.backend, &selector) == host_path_key(&snapshot.backend, &snapshot.canonical_path);
                let sole_source = source_selected && repositories.len() == 1 && expected.is_none();
                repositories.retain(|repository| {
                    expected.as_ref().map_or_else(
                        || sole_source || host_path_key(&snapshot.backend, &repository.authority().worktree_root) == host_path_key(&snapshot.backend, &selector),
                        |expected| repository.authority() == expected,
                    )
                });
                if repositories.len() != 1 { return Err("Selected repository is no longer part of the captured project source".into()); }
            }
            let mut items = Vec::new();
            for repository in repositories {
                let request = repository.request(GitRead::Worktrees).map_err(|error| error.to_string())?;
                let id = request.id();
                let result = request.execute().map_err(|error| error.to_string());
                items.push((repository, id, result));
            }
            Ok((backend, items))
        }).await;
        let _ = state.update(cx, |s, cx| {
            if s.load_generation != generation || !s.is_live(cx) { return; }
            s.loading = false;
            match result {
                Err(error) => {
                    s.load_error = Some(error);
                    s.repositories.clear();
                    s.groups.get_or_insert_with(Vec::new);
                    if let Some(groups) = &mut s.groups {
                        for group in groups { group.authoritative = false; group.readable = false; }
                    }
                }
                Ok((backend, loaded)) => {
                    if !host_ui::snapshot_current(backend.snapshot(), &s.store, cx) {
                        s.invalidate();
                        s.groups.get_or_insert_with(Vec::new);
                        s.load_error = Some("Git source changed during inventory loading; reopen Worktree Management".into());
                        cx.notify();
                        return;
                    }
                    let mut items = Vec::new();
                    let mut repositories = Vec::new();
                    for (repository, request_id, result) in loaded {
                        let path = repository.authority().worktree_root.clone();
                        let result = result.and_then(|result| {
                            if !result.is_current(&repository, request_id) { return Err("Worktree read became stale".into()); }
                            match result.value {
                                GitReadValue::Worktrees(inventory) => Ok(inventory),
                                _ => Err("Unexpected worktree response".into()),
                            }
                        });
                        repositories.push(repository);
                        items.push(RepoLoad { path, result });
                    }
                    let mut groups = merge_groups_on_host(items, s.groups.as_deref().unwrap_or_default(), &backend.snapshot().backend);
                    for group in &mut groups { group.generation = generation; }
                    s.repositories.clear();
                    for group in &groups {
                        if let Some(repository) = repositories.iter().find(|repository| {
                            let path = &repository.authority().worktree_root;
                            host_path_key(&s.snapshot.backend, path) == group.key ||
                                group.worktrees.iter().any(|wt| host_path_key(&s.snapshot.backend, &wt.path) == host_path_key(&s.snapshot.backend, path))
                        }) {
                            s.repositories.insert(group.key.clone(), repository.clone());
                        }
                    }
                    s.snapshot = backend.snapshot().clone();
                    s.backend = Some(backend);
                    s.selected_keys.retain(|key| groups.iter().any(|group| &group.key == key));
                    if s.selected_keys.is_empty() && groups.len() == 1 && groups[0].readable {
                        s.selected_keys.push(groups[0].key.clone());
                    }
                    s.groups = Some(groups);
                }
            }
            cx.notify();
        });
    }).detach();
}

/// Each request carries its repository and refresh generation. Failures remain
/// visible and retry only on Refresh, never as an empty successful branch list.
fn ensure_branches(state: &Entity<WorktreeModal>, cx: &mut App) {
    let pending = {
        let s = state.read(cx);
        if !s.is_live(cx) || s.loading { return; }
        s.groups.as_deref().unwrap_or_default().iter()
            .filter(|group| s.selected_keys.contains(&group.key))
            .filter(|group| !s.branches_by_repo.contains_key(&group.key) && !s.branch_requests.contains_key(&group.key) && !s.branch_errors.contains_key(&group.key))
            .filter_map(|group| s.read_repository(group, cx).map(|repository| (group.key.clone(), repository)))
            .collect::<Vec<_>>()
    };
    for (key, repository) in pending {
        let request = match repository.request(GitRead::Branches) {
            Ok(request) => request,
            Err(error) => {
                state.update(cx, |s, cx| { s.branch_errors.insert(key, error.to_string()); cx.notify(); });
                continue;
            }
        };
        let request_id = request.id();
        let generation = state.update(cx, |s, _| {
            s.branch_requests.insert(key.clone(), request_id);
            s.load_generation
        });
        let state = state.clone();
        cx.spawn(async move |cx| {
            let result = cx.background_executor().spawn(async move { request.execute() }).await;
            let _ = state.update(cx, |s, cx| {
                if !s.is_live(cx) || s.load_generation != generation || s.branch_requests.get(&key) != Some(&request_id) { return; }
                s.branch_requests.remove(&key);
                match result {
                    Ok(result) if s.repositories.get(&key).is_some_and(|current| result.is_current(current, request_id)) => {
                        if let GitReadValue::Branches(branches) = result.value { s.branches_by_repo.insert(key, branches); }
                    }
                    Ok(_) => { s.branch_errors.insert(key, "Branch read became stale".into()); }
                    Err(error) => { s.branch_errors.insert(key, error.to_string()); }
                }
                cx.notify();
            });
        }).detach();
    }
}

// ─── 渲染 ─────────────────────────────────────────────────────

fn badge(text: impl Into<SharedString>) -> AnyElement {
    div()
        .flex_none()
        .px(px(6.0))
        .rounded(px(3.0))
        .bg(ui::border_subtle())
        .text_size(ui::font_px(11.0))
        .text_color(ui::text_muted())
        .child(text.into())
        .into_any_element()
}

fn colored_badge(text: impl Into<SharedString>, fg: gpui::Hsla, bg: gpui::Hsla) -> AnyElement {
    div()
        .flex_none()
        .px(px(6.0))
        .rounded(px(3.0))
        .bg(bg)
        .text_size(ui::font_px(11.0))
        .text_color(fg)
        .child(text.into())
        .into_any_element()
}

fn hint_line(text: impl Into<SharedString>, color: gpui::Hsla) -> AnyElement {
    div()
        .py(px(8.0))
        .w_full()
        .text_center()
        .text_size(ui::font_px(13.0))
        .text_color(color)
        .child(text.into())
        .into_any_element()
}

fn render_body(state: &Entity<WorktreeModal>, window: &mut Window, cx: &mut App) -> AnyElement {
    ensure_branches(state, cx);
    let s = state.read(cx);
    let mut root = div().flex().flex_col().gap(px(8.0));
    if !s.is_live(cx) {
        root = root.child(hint_line("Git source changed; reopen Worktree Management", ui::color_error()));
    }
    let idle = s.idle(cx);
    let refresh_state = state.clone();
    root = root.child(div().flex().justify_end().child(
        ui::ghost_button("worktree-refresh", if s.loading { "Loading..." } else { "Refresh" })
            .when(idle, |el| el.on_click(move |_: &ClickEvent, _, cx| load(&refresh_state, cx))),
    ));

    let Some(groups) = s.groups.clone() else {
        return root
            .child(hint_line(t("worktree", "loading"), ui::text_muted()))
            .into_any_element();
    };
    if let Some(err) = &s.load_error {
        root = root.child(hint_line(err.clone(), ui::color_error()));
    }
    if groups.is_empty() && s.load_error.is_none() {
        return root
            .child(hint_line(t("worktree", "noRepoFound"), ui::text_muted()))
            .into_any_element();
    }

    let multi_repo = groups.len() > 1;
    if multi_repo {
        let all = s.selected_keys.len() == groups.len();
        let state_for_toggle = state.clone();
        let keys: Vec<String> = groups.iter().map(|g| g.key.clone()).collect();
        root = root.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(ui::font_px(12.0))
                        .text_color(ui::text_muted())
                        .child(tr!(
                            "worktree",
                            "reposFound",
                            count = groups.len().to_string()
                        )),
                )
                .child(
                    ui::ghost_button(
                        "worktree-select-all",
                        if all {
                            t("worktree", "clearAll")
                        } else {
                            t("worktree", "selectAll")
                        },
                    )
                    .on_click(move |_: &ClickEvent, _window, cx| {
                        state_for_toggle.update(cx, |s, cx| {
                            if !s.idle(cx) { return; }
                            s.selected_keys = if all { Vec::new() } else { keys.clone() };
                            reset_form_on_selection(s);
                            cx.notify();
                        });
                    }),
                ),
        );
    }

    for group in &groups {
        root = root.child(render_group(state, group, multi_repo, cx));
    }

    // 默认路径建议(`GitWorktreeModal.tsx:289-303` 那个 effect 的对应物)。
    // 放在这里是因为它要 `&mut Window` 去写输入框,而这里正是每帧重跑的地方 ——
    // 值一致就直接 return,不会自激。
    sync_path_suggestion(state, &groups, window, cx);

    root = root.child(render_create_section(state, &groups, multi_repo, cx));
    root.into_any_element()
}

/// 默认路径建议:
///
/// - 未勾选 → 清空;
/// - 多选 → `repo_path`(此时输入框语义是「父目录」);
/// - 单选 → `parentDir(mainPath) + <仓库名>-<分支>`(**仓库同级**)。
///
/// `path_edited` 为真(用户手改过 / 用过「浏览…」)之后**不再跟随**。
/// 分支为空时 `sanitize_branch_for_dir` 回落 `"worktree"` —— 原版就是这样,
/// 选分支之前先给一个 `<仓库名>-worktree` 的建议。
fn sync_path_suggestion(
    state: &Entity<WorktreeModal>,
    groups: &[RepoGroup],
    window: &mut Window,
    cx: &mut App,
) {
    let want = {
        let s = state.read(cx);
        if s.path_edited || !s.idle(cx) {
            return;
        }
        let selected: Vec<&RepoGroup> = groups
            .iter()
            .filter(|g| s.selected_keys.contains(&g.key))
            .collect();
        let branch = match s.mode {
            Mode::Existing => s.sel_branch.clone(),
            Mode::New => s.new_branch.read(cx).value().trim().to_string(),
        };
        match selected.len() {
            0 => String::new(),
            1 => {
                let g = selected[0];
                let parent = host_parent(&s.snapshot.backend, &g.main_path);
                if parent.is_empty() {
                    return;
                }
                host_join(
                    &s.snapshot.backend,
                    parent,
                    &format!("{}-{}", g.name, sanitize_branch_for_dir(&branch)),
                )
            }
            _ => s.snapshot.canonical_path.clone(),
        }
    };
    let input = state.read(cx).wt_path.clone();
    if input.read(cx).value() == want.as_str() {
        return;
    }
    state.update(cx, |s, _| s.suggested_path = want.clone());
    input.update(cx, |st, cx| st.set_value(want, window, cx));
}

/// 勾选变化时要清掉的表单状态(`toggleRepo` / `toggleAll`)。
fn reset_form_on_selection(s: &mut WorktreeModal) {
    s.bump_picker();
    s.path_edited = false;
    s.sel_branch.clear();
    s.base_branch.clear();
    s.create_error = None;
    s.create_results.clear();
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FormChoiceOwner {
    generation: u64,
    picker_request: u64,
}

impl FormChoiceOwner {
    fn capture(state: &WorktreeModal) -> Self {
        Self { generation: state.load_generation, picker_request: state.picker_request }
    }

    fn is_current(self, current: Self) -> bool {
        self == current
    }
}

fn render_group(
    state: &Entity<WorktreeModal>,
    group: &RepoGroup,
    multi_repo: bool,
    cx: &mut App,
) -> AnyElement {
    // 先把要用的字段拷出来:下面每一行都要拿 `&mut App` 去渲染,不能一直借着 `s`
    let (selected, pruning) = {
        let s = state.read(cx);
        (
            s.selected_keys.contains(&group.key),
            s.pruning_key.as_deref() == Some(group.key.as_str()),
        )
    };
    let mut block = div()
        .flex()
        .flex_col()
        .when(multi_repo, |el| {
            el.rounded(px(4.0))
                .border_1()
                .border_color(ui::border_subtle())
        });

    if multi_repo {
        let (state_for_click, key) = (state.clone(), group.key.clone());
        block = block.child(
            div()
                .id(SharedString::from(format!("worktree-group-{}", group.key)))
                .flex()
                .items_center()
                .gap(px(6.0))
                .px(px(8.0))
                .py(px(4.0))
                .cursor_pointer()
                .when(selected, |el| el.bg(ui::accent_subtle()))
                .child(
                    div()
                        .w(px(12.0))
                        .text_size(ui::font_px(12.0))
                        .text_color(if selected {
                            ui::accent()
                        } else {
                            ui::text_muted()
                        })
                        .child(if selected { "☑" } else { "☐" }),
                )
                .child(
                    div()
                        .text_size(ui::font_px(13.0))
                        .text_color(if selected {
                            ui::accent()
                        } else {
                            ui::text_primary()
                        })
                        .child(group.name.clone()),
                )
                .child(
                    div()
                        .ml_auto()
                        .truncate()
                        .text_size(ui::font_px(11.0))
                        .text_color(ui::text_muted())
                        .child(group.main_path.clone()),
                )
                .on_click(move |_: &ClickEvent, _window, cx| {
                    state_for_click.update(cx, |s, cx| {
                        if !s.idle(cx) { return; }
                        if let Some(pos) = s.selected_keys.iter().position(|k| *k == key) {
                            s.selected_keys.remove(pos);
                        } else {
                            s.selected_keys.push(key.clone());
                        }
                        reset_form_on_selection(s);
                        cx.notify();
                    });
                }),
        );
    }

    if let Some(err) = &group.error {
        block = block
            .child(
                div()
                    .px(px(8.0))
                    .py(px(6.0))
                    .text_size(ui::font_px(11.0))
                    .text_color(ui::color_error())
                    .child(err.clone()),
            );
    }

    let mut rows = div().flex().flex_col().gap(px(2.0));
    for (idx, wt) in group.worktrees.iter().enumerate() {
        rows = rows.child(render_worktree_row(state, group, idx, wt, cx));
    }
    block = block.child(rows);

    if group.worktrees.iter().any(|w| !w.is_valid) {
        let enabled = state.read(cx).idle(cx) && state.read(cx).repository(group, cx).is_some_and(|repo| repo.busy().is_none());
        let (state_for_prune, group_for_prune) = (state.clone(), group.clone());
        block = block.child(
            div().flex().justify_end().py(px(4.0)).child(
                ui::ghost_button(
                    SharedString::from(format!("worktree-prune-{}", group.key)),
                    if pruning {
                        t("worktree", "pruning")
                    } else {
                        t("worktree", "prune")
                    },
                )
                .when(enabled && !pruning, |el| {
                    el.on_click(move |_: &ClickEvent, _window, cx| {
                        prune(&state_for_prune, &group_for_prune, cx);
                    })
                }),
            ),
        );
    }

    if let Some(busy) = state.read(cx).repositories.get(&group.key).and_then(GitRepository::busy) {
        block = block.child(hint_line(format!("Git operation {}: {:?}", busy.operation_id, busy.phase), ui::color_warning()));
        if busy.phase == crate::git_backend::GitWritePhase::Uncertain {
            let (state, group) = (state.clone(), group.clone());
            block = block.child(ui::ghost_button(SharedString::from(format!("wt-review-{}", group.key)), "Review uncertain operation")
                .on_click(move |_: &ClickEvent, window, cx| review_uncertain(&state, &group, busy.operation_id, window, cx)));
        }
    }

    block.into_any_element()
}

fn render_worktree_row(
    state: &Entity<WorktreeModal>,
    group: &RepoGroup,
    idx: usize,
    wt: &WorktreeInfo,
    cx: &mut App,
) -> AnyElement {
    let s = state.read(cx);
    let is_project = project_location(&s.snapshot.backend, &wt.path).ok()
        .is_some_and(|(location, _)| !s.store.read(cx).project_ids_for_location(&location).is_empty());
    let readable = s.idle(cx) && s.read_repository(group, cx).is_some_and(|repository| repository.busy().is_none());
    let writable = readable && s.repository(group, cx).is_some_and(|repository| repository.busy().is_none());

    let mut badges = div().flex().items_center().gap(px(4.0));
    if wt.is_main {
        badges = badges.child(badge(t("worktree", "mainRepo")));
    }
    if let Some(branch) = &wt.branch {
        badges = badges.child(badge(format!("⎇ {branch}")));
    }
    if !wt.is_valid {
        badges = badges.child(colored_badge(
            t("worktree", "invalid"),
            ui::color_error(),
            ui::with_alpha(ui::color_error(), 0.15),
        ));
    }
    if wt.is_locked {
        badges = badges.child(badge(t("worktree", "locked")));
    }
    if is_project {
        badges = badges.child(colored_badge(
            t("worktree", "isProject"),
            ui::accent(),
            ui::accent_subtle(),
        ));
    }

    let mut actions = div().flex().items_center().gap(px(4.0)).flex_none();
    if wt.is_valid {
        for (terminal, label, id) in [
            (true, t("worktree", "openTerminal"), "wt-open"),
            (false, if is_project { t("worktree", "switchToProject") } else { t("worktree", "addAsProject") }, "wt-project"),
        ] {
            let (state, group, wt) = (state.clone(), group.clone(), wt.clone());
            actions = actions.child(
                ui::ghost_button(SharedString::from(format!("{id}-{}-{idx}", group.key)), label)
                    .when(!readable, |el| el.opacity(0.5))
                    .when(readable, |el| el.on_click(move |_: &ClickEvent, window, cx| {
                        open_worktree(&state, &group, &wt, terminal, window, cx);
                    })),
            );
        }
        if !wt.is_main {
            let (state, group, wt) = (state.clone(), group.clone(), wt.clone());
            actions = actions.child(
                ui::danger_button(SharedString::from(format!("wt-remove-{}-{idx}", group.key)), t("worktree", "remove"))
                    .when(!writable, |el| el.opacity(0.5))
                    .when(writable, |el| el.on_click(move |_: &ClickEvent, window, cx| {
                        open_remove_confirm(&state, &group, &wt, window, cx);
                    })),
            );
        }
    }

    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(8.0))
        .py(px(6.0))
        .rounded(px(4.0))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            div()
                                .truncate()
                                .text_size(ui::font_px(13.0))
                                .text_color(ui::text_primary())
                                .child(wt.name.clone()),
                        )
                        .child(badges),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(ui::font_px(11.0))
                        .text_color(ui::text_muted())
                        .child(wt.path.clone()),
                ),
        )
        .child(actions)
        .into_any_element()
}

/// 新建 worktree 那一段。
fn render_create_section(
    state: &Entity<WorktreeModal>,
    groups: &[RepoGroup],
    multi_repo: bool,
    cx: &mut App,
) -> AnyElement {
    let s = state.read(cx);
    let selected: Vec<&RepoGroup> = groups
        .iter()
        .filter(|g| s.selected_keys.contains(&g.key))
        .collect();
    let multi_target = selected.len() > 1;

    let mut section = div()
        .mt(px(8.0))
        .pt(px(8.0))
        .border_t_1()
        .border_color(ui::border_subtle())
        .flex()
        .flex_col()
        .gap(px(6.0));

    // 标题行 + 模式段控件
    let mut head = div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .text_size(ui::font_px(13.0))
                .text_color(ui::text_primary())
                .child(t("worktree", "createTitle")),
        );
    if multi_repo {
        head = head.child(
            div()
                .text_size(ui::font_px(11.0))
                .text_color(ui::text_muted())
                .child(tr!(
                    "worktree",
                    "selectedCount",
                    count = selected.len().to_string()
                )),
        );
    }
    let mut seg = div()
        .ml_auto()
        .flex()
        .rounded(px(4.0))
        .overflow_hidden()
        .border_1()
        .border_color(ui::border_default());
    for (mode, label) in [
        (Mode::Existing, t("worktree", "modeExisting")),
        (Mode::New, t("worktree", "modeNew")),
    ] {
        let active = s.mode == mode;
        let state_for_mode = state.clone();
        seg = seg.child(
            div()
                .id(SharedString::from(format!(
                    "worktree-mode-{}",
                    if matches!(mode, Mode::New) { "new" } else { "existing" }
                )))
                .px(px(10.0))
                .py(px(2.0))
                .text_size(ui::font_px(12.0))
                .cursor_pointer()
                .when(active, |el| {
                    el.bg(ui::accent_subtle()).text_color(ui::accent())
                })
                .when(!active, |el| el.text_color(ui::text_muted()))
                .child(label)
                .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let input = state_for_mode.update(cx, |s, cx| {
                        if !s.idle(cx) { return None; }
                        s.bump_picker();
                        s.mode = mode;
                        cx.notify();
                        matches!(mode, Mode::New).then(|| s.new_branch.clone())
                    });
                    // 切到「新建」就该能直接敲分支名,不必再点一下输入框
                    // (「已有」那侧是下拉选择,没有可聚焦的输入框)
                    if let Some(input) = input {
                        autofocus(&input, window, cx);
                    }
                }),
        );
    }
    section = section.child(head.child(seg));

    // 模式区
    if selected.is_empty() {
        section = section.child(hint_line(t("worktree", "selectRepoHint"), ui::text_muted()));
        return section.into_any_element();
    }

    let branches_ready = selected
        .iter()
        .all(|g| s.branches_by_repo.contains_key(&g.key));
    for group in &selected {
        if let Some(error) = s.branch_errors.get(&group.key) {
            section = section.child(hint_line(error.clone(), ui::color_error()));
        }
    }
    match s.mode {
        Mode::Existing => {
            if !branches_ready {
                section = section.child(hint_line(t("worktree", "loading"), ui::text_muted()));
            } else {
                let per_group: Vec<(Vec<BranchInfo>, Vec<Option<String>>)> = selected
                    .iter()
                    .map(|g| {
                        (
                            s.branches_by_repo.get(&g.key).cloned().unwrap_or_default(),
                            g.worktrees.iter().map(|w| w.branch.clone()).collect(),
                        )
                    })
                    .collect();
                let options = available_branches(&per_group);
                if options.is_empty() {
                    section = section.child(hint_line(
                        if multi_target {
                            t("worktree", "noCommonBranch")
                        } else {
                            t("worktree", "noBranchAvailable")
                        },
                        ui::text_muted(),
                    ));
                } else {
                    section = section.child(dropdown(
                        state,
                        "worktree-branch",
                        if s.sel_branch.is_empty() {
                            t("worktree", "selectBranch").to_string()
                        } else {
                            s.sel_branch.clone()
                        },
                        options,
                        |s, value| s.sel_branch = value,
                        cx,
                    ));
                }
            }
        }
        Mode::New => {
            let per_group: Vec<Vec<BranchInfo>> = selected
                .iter()
                .map(|g| s.branches_by_repo.get(&g.key).cloned().unwrap_or_default())
                .collect();
            let mut options = vec![t("worktree", "baseHead").to_string()];
            options.extend(base_branch_options(&per_group));
            section = section.child(
                div()
                    .flex()
                    .gap(px(6.0))
                    .child(div().flex_1().child(Input::new(&s.new_branch)))
                    .child(dropdown(
                        state,
                        "worktree-base",
                        if s.base_branch.is_empty() {
                            t("worktree", "baseHead").to_string()
                        } else {
                            s.base_branch.clone()
                        },
                        options,
                        |s, value| {
                            s.base_branch = if value == t("worktree", "baseHead") {
                                String::new()
                            } else {
                                value
                            }
                        },
                        cx,
                    )),
            );
        }
    }

    // 路径行
    let state_for_browse = state.clone();
    section = section.child(
        div()
            .flex()
            .gap(px(6.0))
            .child(div().flex_1().child(Input::new(&s.wt_path)))
            .child(
                ui::ghost_button("worktree-browse", t("worktree", "browse")).on_click(
                    move |_: &ClickEvent, window, cx| {
                        browse_destination(&state_for_browse, window, cx);
                    },
                ),
            ),
    );

    // 「创建后添加为项目并切换过去」
    let add_as_project = s.add_as_project;
    let state_for_check = state.clone();
    section = section.child(
        div()
            .id("worktree-add-project")
            .flex()
            .items_center()
            .gap(px(6.0))
            .cursor_pointer()
            .text_size(ui::font_px(12.0))
            .text_color(ui::text_secondary())
            .child(
                div()
                    .w(px(12.0))
                    .text_color(if add_as_project {
                        ui::accent()
                    } else {
                        ui::text_muted()
                    })
                    .child(if add_as_project { "☑" } else { "☐" }),
            )
            .child(t("worktree", "addAsProjectAfterCreate"))
            .on_click(move |_: &ClickEvent, _window, cx| {
                state_for_check.update(cx, |s, cx| {
                    if !s.idle(cx) { return; }
                    s.bump_picker();
                    s.add_as_project = !s.add_as_project;
                    cx.notify();
                });
            }),
    );

    if let Some(err) = &s.create_error {
        section = section.child(
            div()
                .text_size(ui::font_px(11.0))
                .text_color(ui::color_error())
                .child(err.clone()),
        );
    }
    for (target, err) in &s.create_results {
        section = section.child(
            div()
                .text_size(ui::font_px(11.0))
                .text_color(ui::color_error())
                .child(format!("{target}: {err}")),
        );
    }

    let creating = s.creating;
    let can_create = s.idle(cx) && branches_ready && selected.iter().all(|group| s.repository(group, cx).is_some_and(|repo| repo.busy().is_none()));
    let state_for_create = state.clone();
    let groups_for_create = groups.to_vec();
    section = section.child(
        div().flex().justify_end().child(
            ui::primary_button(
                "worktree-create",
                if creating {
                    t("worktree", "creating").to_string()
                } else if multi_target {
                    tr!(
                        "worktree",
                        "createMulti",
                        count = selected.len().to_string()
                    )
                } else {
                    t("worktree", "create").to_string()
                },
            )
            .when(!can_create, |el| el.opacity(0.5))
            .when(can_create, |el| {
                el.on_click(move |_: &ClickEvent, window, cx| {
                    create(&state_for_create, &groups_for_create, window, cx);
                })
            }),
        ),
    );

    section.into_any_element()
}

/// 一个走 [`menu`] 的下拉选择器(替原版的 `<select>`)。
fn dropdown(
    state: &Entity<WorktreeModal>,
    id: &'static str,
    current: String,
    options: Vec<String>,
    apply: fn(&mut WorktreeModal, String),
    cx: &App,
) -> AnyElement {
    let owner = FormChoiceOwner::capture(state.read(cx));
    let state = state.clone();
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .gap(px(6.0))
        .px(px(8.0))
        .py(px(4.0))
        .w(px(180.0))
        .flex_none()
        .rounded(px(4.0))
        .border_1()
        .border_color(ui::border_default())
        .cursor_pointer()
        .text_size(ui::font_px(12.0))
        .text_color(ui::text_primary())
        .child(div().truncate().child(current))
        .child(div().text_color(ui::text_muted()).child("▾"))
        .on_click(move |event: &ClickEvent, window, cx| {
            let current = state.read(cx);
            if !current.idle(cx) || !owner.is_current(FormChoiceOwner::capture(current)) { return; }
            let entries: Vec<menu::MenuEntry> = options
                .iter()
                .map(|option| {
                    let (state, option) = (state.clone(), option.clone());
                    MenuItem::new(option.clone())
                        .on_click(move |_window, cx| {
                            state.update(cx, |s, cx| {
                                if !s.idle(cx) || !owner.is_current(FormChoiceOwner::capture(s)) { return; }
                                s.bump_picker();
                                apply(s, option.clone());
                                cx.notify();
                            });
                        })
                        .into()
                })
                .collect();
            menu::show(event.position(), entries, window, cx);
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssh_backend() -> ExecutionBackend {
        ExecutionBackend::Ssh {
            connection: mt_config::SshConnection {
                id: "host-a".into(), name: "test".into(), host: "example.invalid".into(), port: 22,
                user: "test".into(), password: None, identity_file: None, group: None,
            },
            connection_fingerprint: 1, connection_epoch: Some(3),
        }
    }

    #[test]
    fn remote_path_helpers_preserve_case_and_literal_backslashes_on_every_client() {
        for backend in [ssh_backend(), ExecutionBackend::Wsl { distro: "Ubuntu".into() }] {
            assert_ne!(host_path_key(&backend, "/Repo"), host_path_key(&backend, "/repo"));
            assert_eq!(host_parent(&backend, r"/srv/repo\name"), "/srv");
            assert_eq!(host_name(&backend, r"/srv/repo\name"), r"repo\name");
            assert_eq!(host_join(&backend, r"/srv/parent\", "worktree"), r"/srv/parent\/worktree");
            assert_eq!(host_join(&backend, "/", "worktree"), "/worktree");
        }
    }

    #[test]
    fn discovery_entry_rejects_repointed_project_and_foreign_wsl_selector() {
        let ssh = ssh_backend();
        assert!(discovery_selector_matches(&ssh, "/srv/Repo", "/srv/Repo/"));
        assert!(!discovery_selector_matches(&ssh, "/srv/Repo", "/srv/other"));
        assert!(!discovery_selector_matches(&ssh, "/srv/Repo", "/srv/repo"));
        assert!(!discovery_selector_matches(&ssh, r"/srv/repo\child", "/srv/repo/child"));
        let wsl = ExecutionBackend::Wsl { distro: "Ubuntu".into() };
        assert!(discovery_selector_matches(&wsl, r"\\wsl$\Ubuntu\srv\repo", r"\\wsl.localhost\Ubuntu\srv\repo"));
        assert!(!discovery_selector_matches(&wsl, r"\\wsl$\Debian\srv\repo", r"\\wsl$\Ubuntu\srv\repo"));
        assert!(!discovery_selector_matches(&wsl, "/srv/repo", r"\\wsl$\Ubuntu\srv\repo"));
    }

    #[test]
    fn registration_locations_preserve_ssh_connection_and_wsl_distribution() {
        let ssh = ssh_backend();
        let (remote, remote_path) = project_location(&ssh, r"/srv/Repo\name").unwrap();
        assert_eq!(remote_path, r"/srv/Repo\name");
        assert_eq!(remote, ProjectLocationKey::Ssh { connection_id: "host-a".into(), normalized_posix_path: r"/srv/Repo\name".into() });
        let local = project_location(&ExecutionBackend::Local, "/srv/Repo").unwrap().0;
        assert_ne!(local, project_location(&ssh, "/srv/Repo").unwrap().0);
        let wsl = ExecutionBackend::Wsl { distro: "Ubuntu".into() };
        let (key, path) = project_location(&wsl, "/srv/Repo").unwrap();
        assert_eq!(path, r"\\wsl.localhost\Ubuntu\srv\Repo");
        assert_eq!(key, ProjectLocationKey::Local { normalized_canonical_path: "wsl:ubuntu:/srv/Repo".into() });
        assert!(project_location(&ssh, "/srv/../escape").is_err());
    }

    #[test]
    fn worktree_registration_rejects_wsl_names_that_would_retarget_native_paths() {
        let wsl = ExecutionBackend::Wsl { distro: "Ubuntu".into() };
        for path in [r"/srv/repo\child", "/srv/name:stream", "/srv/trailing.", "/srv/../escape"] {
            assert!(project_location(&wsl, path).is_err(), "{path}");
        }
        assert_eq!(project_location(&wsl, "/srv/repo/child").unwrap().1, r"\\wsl.localhost\Ubuntu\srv\repo\child");
        assert_eq!(project_location(&ssh_backend(), r"/srv/repo\child").unwrap().1, r"/srv/repo\child");
    }

    #[test]
    fn branch_menu_choice_cannot_adopt_a_refreshed_or_edited_form() {
        let owner = FormChoiceOwner { generation: 7, picker_request: 2 };
        assert!(owner.is_current(owner));
        assert!(!owner.is_current(FormChoiceOwner { generation: 8, ..owner }));
        assert!(!owner.is_current(FormChoiceOwner { picker_request: 3, ..owner }));
        assert!(!owner.is_current(FormChoiceOwner { generation: 9, picker_request: 4 }));
    }

    #[test]
    fn fresh_libgit2_fallback_is_readable_but_never_mutation_authority() {
        let previous = merge_groups(vec![RepoLoad { path: "/repo".into(), result: Ok(scan(vec![fact("/repo", true, None)], true)) }], &[]);
        let mut fallback = scan(vec![fact("/repo", true, Some("main"))], false);
        fallback.scan.source = mt_project::worktree::WorktreeScanSource::Libgit2Fallback;
        let groups = merge_groups(vec![RepoLoad { path: "/repo".into(), result: Ok(fallback) }], &previous);
        assert!(groups[0].readable);
        assert!(!groups[0].authoritative);
        assert_eq!(groups[0].worktrees[0].branch.as_deref(), Some("main"));
        assert!(groups[0].error.is_some());
    }

    #[test]
    fn failed_inventory_retains_rows_with_explicit_error_and_no_row_authority() {
        let previous = merge_groups(vec![RepoLoad { path: "/repo".into(), result: Ok(scan(vec![fact("/repo", true, None)], true)) }], &[]);
        let groups = merge_groups(vec![RepoLoad { path: "/repo".into(), result: Err("offline".into()) }], &previous);
        assert_eq!(groups[0].worktrees.len(), 1);
        assert_eq!(groups[0].error.as_deref(), Some("offline"));
        assert!(!groups[0].readable);
        assert!(!groups[0].authoritative);
    }

    fn fact(path: &str, is_main: bool, branch: Option<&str>) -> mt_project::worktree::WorktreeFact {
        mt_project::worktree::WorktreeFact {
            path: PathBuf::from(path),
            head: Some("abc".into()),
            branch_ref: branch.map(|branch| format!("refs/heads/{branch}")),
            is_main,
            is_detached: branch.is_none(),
            is_bare: false,
            is_sparse: false,
            locked: None,
            prunable: None,
            path_state: mt_project::worktree::WorktreePathState::Present,
        }
    }

    fn scan(
        worktrees: Vec<mt_project::worktree::WorktreeFact>,
        authoritative: bool,
    ) -> GitWorktrees {
        let scan = mt_project::worktree::WorktreeScan {
            generation: 0,
            source: if authoritative {
                mt_project::worktree::WorktreeScanSource::PorcelainZ
            } else {
                mt_project::worktree::WorktreeScanSource::LastKnown
            },
            authoritative,
            worktrees,
            warning: None,
        };
        GitWorktrees { entries: mt_project::git::project_worktree_scan(&scan), scan }
    }

    fn branch(name: &str, remote: bool) -> BranchInfo {
        BranchInfo {
            name: name.to_string(),
            is_head: false,
            is_remote: remote,
            commit_hash: "x".into(),
        }
    }

    /// 路径分隔符与尾斜杠统一；只有 Windows 本地路径按平台规则折叠大小写。
    #[test]
    fn 路径归一化() {
        if cfg!(windows) {
            assert_eq!(normalize_path(r"D:\Git\Repo\"), "d:/git/repo");
            assert_eq!(
                normalize_path(r"D:\Git\Repo"),
                normalize_path("D:/Git/repo")
            );
        } else {
            assert_eq!(normalize_path("/home/U/Proj/"), "/home/U/Proj");
            assert_ne!(
                normalize_path("/home/U/Proj"),
                normalize_path("/home/u/proj")
            );
            assert_ne!(normalize_path(r"/tmp/a\b"), normalize_path("/tmp/a/b"));
        }
        assert_eq!(normalize_path(""), "");
    }

    /// 分支名 → 目录名片段。
    #[test]
    fn 分支名转目录名() {
        assert_eq!(sanitize_branch_for_dir("feature/login"), "feature-login");
        assert_eq!(sanitize_branch_for_dir("fix: a b"), "fix-a-b");
        assert_eq!(sanitize_branch_for_dir("---"), "worktree");
        assert_eq!(sanitize_branch_for_dir(""), "worktree");
        assert_eq!(sanitize_branch_for_dir("main"), "main");
        // 首尾的分隔符要削掉,中间的连续片段压成一个 `-`
        assert_eq!(sanitize_branch_for_dir("/a//b/"), "a-b");
    }

    /// 路径拼接 / 父目录 / 末段名。
    #[test]
    fn 路径小件() {
        assert_eq!(join_path("/home/u", "p", '/'), "/home/u/p");
        assert_eq!(parent_dir("/home/u/p/"), "/home/u");
        assert_eq!(parent_dir("repo"), "repo", "没有父级时返回原串");
        assert_eq!(base_name("/home/u/p/"), "p");
        assert_eq!(base_name("repo"), "repo");
        if cfg!(windows) {
            assert_eq!(join_path(r"D:\Git", "repo", '\\'), r"D:\Git\repo");
            assert_eq!(join_path(r"D:\Git\", "repo", '\\'), r"D:\Git\repo");
            assert_eq!(parent_dir(r"D:\Git\repo"), r"D:\Git");
            assert_eq!(base_name(r"D:\Git\repo"), "repo");
        } else {
            assert_eq!(join_path(r"/tmp/base\", "repo", '/'), r"/tmp/base\/repo");
            assert_eq!(parent_dir(r"/tmp/foo\bar"), "/tmp");
            assert_eq!(base_name(r"/tmp/foo\bar"), r"foo\bar");
            assert_eq!(base_name(r"/tmp/repo\"), r"repo\");
        }
    }

    /// 归并:主仓库与它内部的 worktree 扫出来结果完全相同,必须合成一组。
    #[test]
    fn 按主工作区归并() {
        let list = vec![
            fact("/a/repo", true, Some("main")),
            fact("/a/repo-wt1", false, Some("feat")),
        ];
        let groups = merge_groups(
            vec![
                RepoLoad {
                    path: "/a/repo".into(),
                    result: Ok(scan(list.clone(), true)),
                },
                // 同一份结果(从 worktree 目录扫到的)—— 不该重复展示
                RepoLoad {
                    path: "/a/repo-wt1".into(),
                    result: Ok(scan(list, true)),
                },
            ],
            &[],
        );
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].main_path, "/a/repo");
        assert_eq!(groups[0].name, "repo");
        assert_eq!(groups[0].worktrees.len(), 2);
    }

    /// 某个仓库 `list_worktrees` 失败时只让那一组显示错误,别的组照常。
    #[test]
    fn 单组失败不拖垮别的组() {
        let groups = merge_groups(
            vec![
                RepoLoad {
                    path: "/a/ok".into(),
                    result: Ok(scan(vec![fact("/a/ok", true, None)], true)),
                },
                RepoLoad {
                    path: "/a/bad".into(),
                    result: Err("仓库损坏".into()),
                },
            ],
            &[],
        );
        assert_eq!(groups.len(), 2);
        assert!(groups[0].error.is_none());
        assert_eq!(groups[1].error.as_deref(), Some("仓库损坏"));
        assert!(groups[1].worktrees.is_empty());
    }

    #[test]
    fn 非权威结果保留上一帧而不采用部分列表() {
        let previous = merge_groups(
            vec![RepoLoad {
                path: "/a/repo".into(),
                result: Ok(scan(
                    vec![
                        fact("/a/repo", true, Some("main")),
                        fact("/a/repo-wt", false, Some("feature")),
                    ],
                    true,
                )),
            }],
            &[],
        );
        let groups = merge_groups(
            vec![RepoLoad {
                path: "/a/repo".into(),
                result: Ok(scan(vec![fact("/a/repo", true, Some("main"))], false)),
            }],
            &previous,
        );
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].worktrees.len(), 2);
        assert_eq!(groups[0].worktrees[0].branch.as_deref(), Some("main"));
        assert_eq!(groups[0].worktrees[1].branch.as_deref(), Some("feature"));
        assert!(!groups[0].authoritative);
    }

    /// 「检出现有分支」:本地分支减去已被占用的,再取全组交集。
    #[test]
    fn 可用分支取交集且排除已占用() {
        let g1 = (
            vec![
                branch("main", false),
                branch("feat", false),
                branch("origin/main", true),
            ],
            // main 被主工作区占着
            vec![Some("main".to_string())],
        );
        assert_eq!(available_branches(std::slice::from_ref(&g1)), vec!["feat"]);

        let g2 = (
            vec![
                branch("main", false),
                branch("feat", false),
                branch("x", false),
            ],
            vec![Some("main".to_string())],
        );
        // 两组交集仍是 feat(x 只有第二组有)
        assert_eq!(available_branches(&[g1.clone(), g2]), vec!["feat"]);

        // 没有共同可用分支
        let g3 = (vec![branch("main", false)], vec![Some("main".to_string())]);
        assert!(available_branches(&[g1, g3]).is_empty());
    }

    /// 「新分支起点」含远程分支,同样取交集,顺序按第一组。
    #[test]
    fn 起点分支含远程且取交集() {
        let a = vec![
            branch("main", false),
            branch("origin/main", true),
            branch("only-a", false),
        ];
        let b = vec![branch("origin/main", true), branch("main", false)];
        assert_eq!(
            base_branch_options(&[a.clone(), b]),
            vec!["main", "origin/main"],
            "顺序按第一组"
        );
        assert_eq!(base_branch_options(&[a]).len(), 3);
        assert!(base_branch_options(&[]).is_empty());
    }
}
