//! Git 面板容器。对应 `src/components/GitHistory.tsx`(548 行)。
//!
//! ```text
//! ① 仓库栏(repos 非空时才画)h34
//!    ├ 仓库下拉触发器(▾ + 名称 + ⎇)   右键 = 「在终端中打开」/「Worktree 管理」
//!    ├ 分支徽章 + 分支下拉(displayBranch 存在时)
//!    ├ 刷新 ↻ / pull ↓ / push ↑
//! ② SectionHeader「更改」  h30   右侧挂 视图切换 ⊞/≡
//! ③ GitChanges           .git-section-body
//! ④ 中缝拖拽手柄(两块都展开时)
//! ⑤ SectionHeader「提交历史」h30(带上边框)
//! ⑥ GitHistoryContent    .git-section-body
//! ```
//!
//! # 两块折叠区**常驻挂载**
//!
//! 折叠只把高度收到 0(`flex-grow: 0` + `flex-basis: 0`),**不摘子实体** ——
//! 已加载的 commits、提交草稿都不丢(`GitHistory.tsx:111-112` 原注释)。
//!
//! # 视图状态放哪
//!
//! `sectionUi`(两块的展开态与比例)是不落盘的临时视图状态。Orca 模式下
//! 它与仓库选择、分支一起按 `WorktreeId` 缓存，切回该 worktree 时恢复。
//!
//! # 可见性闸
//!
//! [`GitPanel::set_visible`] 收起时**不跑 `discover_git_repos`**(它要扫盘,
//! 大 monorepo 上是秒级);同时开关 [`git_watch`] 的输出旁路总闸。
//! 范式照 `SessionPanel::set_visible`。

use std::collections::HashMap;
use std::time::Duration;

use gpui::{
    AnyElement, App, AppContext as _, Bounds, ClickEvent, Context, Entity, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Task, Window, canvas,
    div, prelude::FluentBuilder as _, px,
};
use mt_identity::WorktreeId;
use mt_project::git::{BranchInfo, GitRepoInfo};

use crate::git_changes::{GitChanges, GitChangesEvent};
use crate::git_history::{GitHistoryContent, GitHistoryEvent};
use crate::git_watch;
use crate::i18n::t;
use crate::menu::{self, MenuItem};
use crate::store::{AppStore, orca_worktree_context_enabled};
use crate::ui;

/// 两块折叠区的会话级视图状态。**有意不落盘**。
#[derive(Clone, Copy)]
struct SectionUi {
    changes_open: bool,
    history_open: bool,
    ratio: f32,
}

impl Default for SectionUi {
    fn default() -> Self {
        Self {
            changes_open: true,
            history_open: true,
            ratio: 0.5,
        }
    }
}

#[derive(Clone)]
struct GitScopeState {
    repos: Vec<GitRepoInfo>,
    selected_repo: String,
    branches: Vec<BranchInfo>,
    view_branch: Option<String>,
    pull_state: Option<SyncState>,
    push_state: Option<SyncState>,
    section_ui: SectionUi,
    refresh_needed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GitScope {
    worktree_id: Option<WorktreeId>,
    generation: u64,
    cache_enabled: bool,
}

impl GitScope {
    pub(crate) fn new(
        worktree_id: Option<WorktreeId>,
        generation: u64,
        cache_enabled: bool,
    ) -> Self {
        Self {
            worktree_id,
            generation,
            cache_enabled,
        }
    }

    pub(crate) fn unbound() -> Self {
        Self::new(None, 0, false)
    }

    pub(crate) fn cache_key(&self) -> Option<&WorktreeId> {
        if self.cache_enabled {
            self.worktree_id.as_ref()
        } else {
            None
        }
    }

    pub(crate) fn same_cache_identity(&self, other: &Self) -> bool {
        self.cache_enabled
            && other.cache_enabled
            && self.worktree_id.is_some()
            && self.worktree_id == other.worktree_id
    }
}

pub(crate) fn swap_worktree_scope<T>(
    cache: &mut HashMap<WorktreeId, T>,
    current_worktree: Option<&WorktreeId>,
    next_worktree: Option<&WorktreeId>,
    current_state: T,
    empty: impl FnOnce() -> T,
) -> (T, bool) {
    if let Some(worktree_id) = current_worktree {
        cache.insert(worktree_id.clone(), current_state);
    }
    match next_worktree.and_then(|worktree_id| cache.remove(worktree_id)) {
        Some(state) => (state, true),
        None => (empty(), false),
    }
}

fn git_scope_request_matches(
    request_generation: u64,
    current_generation: u64,
    request_worktree: Option<&WorktreeId>,
    current_worktree: Option<&WorktreeId>,
) -> bool {
    request_generation == current_generation && request_worktree == current_worktree
}

fn git_panel_scope_changed(
    cache_enabled: bool,
    current_worktree: Option<&WorktreeId>,
    next_worktree: Option<&WorktreeId>,
    current_path: Option<&str>,
    next_path: Option<&str>,
) -> bool {
    current_path != next_path || (cache_enabled && current_worktree != next_worktree)
}

/// 区块比例的钳位(`GitHistory.tsx:17`)。
pub fn clamp_ratio(r: f32) -> f32 {
    r.clamp(0.15, 0.85)
}

/// pull / push 的一次性状态(`GitHistory.tsx:19`)。
#[derive(Clone, PartialEq)]
enum SyncState {
    Loading,
    Success,
    Error(String),
}

fn suspend_sync_state(state: Option<SyncState>) -> (Option<SyncState>, bool) {
    if matches!(state.as_ref(), Some(SyncState::Loading)) {
        (None, true)
    } else {
        (state, false)
    }
}

fn clear_sync_state(state: &mut Option<SyncState>) {
    if !matches!(state.as_ref(), Some(SyncState::Loading)) {
        *state = None;
    }
}

/// 中缝拖拽的一次会话。
#[derive(Clone, Copy)]
struct SectionDrag {
    start_y: Pixels,
    start_ratio: f32,
    /// 两块内容加起来的高度(靠 canvas 量出来)。
    total: f32,
}

pub struct GitPanel {
    store: Entity<AppStore>,
    changes: Entity<GitChanges>,
    history: Entity<GitHistoryContent>,
    repos: Vec<GitRepoInfo>,
    selected_repo: String,
    branches: Vec<BranchInfo>,
    branches_loading: bool,
    /// 正在查看(**未 checkout**)的分支;`None` = 跟随 HEAD。
    view_branch: Option<String>,
    pull_state: Option<SyncState>,
    push_state: Option<SyncState>,
    /// 抽屉里显示的是不是本面板。收着时不扫盘。
    visible: bool,
    /// 收着的时候项目切过 → 打开时补拉一次。
    stale: bool,
    /// 当前挂着的 canonical worktree 路径。
    project_path: Option<String>,
    current_worktree: Option<WorktreeId>,
    scope_cache: HashMap<WorktreeId, GitScopeState>,
    scope_generation: u64,
    section_ui: SectionUi,
    /// 迟到响应丢弃(换仓库后旧的分支响应不许覆盖)。
    branch_request: u64,
    repo_request: u64,
    /// 两块内容区加起来的高度,中缝拖拽换算比例要用。
    sections_height: f32,
    drag: Option<SectionDrag>,
    _tick: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl GitPanel {
    pub fn new(store: Entity<AppStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let changes = cx.new(|cx| GitChanges::new(store.clone(), window, cx));
        let history = cx.new(|_| GitHistoryContent::new(store.clone()));

        let mut subs = Vec::new();
        subs.push(
            cx.subscribe(&changes, |this: &mut Self, _entity, event, cx| {
                match event {
                    // 提交成功:历史整体重来 + 重载分支(分支头已前移)。即使
                    // A→B→A 让原 generation 失效，也按稳定 worktree 身份对账。
                    GitChangesEvent::Committed(scope, repo_path) => {
                        this.handle_commit_completion(scope, repo_path, cx);
                    }
                }
            }),
        );
        subs.push(cx.subscribe(
            &history,
            |this: &mut Self, _entity, event, cx| match event {
                GitHistoryEvent::RefreshRepos(scope) if this.scope_matches(scope) => {
                    this.refresh_repo_meta(cx)
                }
                GitHistoryEvent::RefreshRepos(_) => {}
            },
        ));
        subs.push(cx.observe(&store, |this: &mut Self, _, cx| {
            let (worktree_id, path) = {
                let store = this.store.read(cx);
                let project = store.active_project();
                (
                    store.active_worktree_id().cloned(),
                    project.map(|project| {
                        store
                            .canonical_worktree_path_for_project(&project.id)
                            .unwrap_or(&project.path)
                            .to_string()
                    }),
                )
            };
            if git_panel_scope_changed(
                orca_worktree_context_enabled(),
                this.current_worktree.as_ref(),
                worktree_id.as_ref(),
                this.project_path.as_deref(),
                path.as_deref(),
            ) {
                this.switch_scope(worktree_id, path, cx);
                if this.visible {
                    this.on_project_changed(cx);
                } else {
                    // 收着的时候不扫盘 —— 原版收起时组件根本没挂载
                    this.stale = true;
                }
            }
            cx.notify();
        }));

        Self {
            store,
            changes,
            history,
            repos: Vec::new(),
            selected_repo: String::new(),
            branches: Vec::new(),
            branches_loading: false,
            view_branch: None,
            pull_state: None,
            push_state: None,
            visible: false,
            stale: true,
            project_path: None,
            current_worktree: None,
            scope_cache: HashMap::new(),
            scope_generation: 0,
            section_ui: SectionUi::default(),
            branch_request: 0,
            repo_request: 0,
            sections_height: 0.0,
            drag: None,
            _tick: None,
            _subs: subs,
        }
    }

    /// 抽屉开合 / 换面板。收着时不扫盘、不嗅探 pty 输出。
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        git_watch::set_enabled(visible);
        if visible {
            let (worktree_id, path) = {
                let store = self.store.read(cx);
                let project = store.active_project();
                (
                    store.active_worktree_id().cloned(),
                    project.map(|project| {
                        store
                            .canonical_worktree_path_for_project(&project.id)
                            .unwrap_or(&project.path)
                            .to_string()
                    }),
                )
            };
            if git_panel_scope_changed(
                orca_worktree_context_enabled(),
                self.current_worktree.as_ref(),
                worktree_id.as_ref(),
                self.project_path.as_deref(),
                path.as_deref(),
            ) {
                self.switch_scope(worktree_id, path, cx);
            }
            if self.stale {
                self.on_project_changed(cx);
            }
            self.start_tick(cx);
        } else {
            self._tick = None;
        }
        cx.notify();
    }

    /// pty-output 嗅探的节拍。100ms 一拍:抽干旁路 → 命中就通知两个子面板
    /// (**各自** 500ms 去抖,原版是两个独立定时器)→ 到点的那个自己重取。
    fn start_tick(&mut self, cx: &mut Context<Self>) {
        self._tick = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(git_watch::POLL_MS))
                    .await;
                let alive = this
                    .update(cx, |this: &mut Self, cx| {
                        if git_watch::drain_hit() {
                            this.changes.update(cx, |c, _| c.note_pty_hit());
                            this.history.update(cx, |h, _| h.note_pty_hit());
                        }
                        this.changes.update(cx, |c, cx| c.tick(cx));
                        this.history.update(cx, |h, cx| h.tick(cx));
                    })
                    .is_ok();
                if !alive {
                    return;
                }
            }
        }));
    }

    fn save_scope(&mut self) {
        let Some(worktree_id) = self.current_worktree.clone() else {
            return;
        };
        let (pull_state, pull_refresh_needed) = suspend_sync_state(self.pull_state.take());
        let (push_state, push_refresh_needed) = suspend_sync_state(self.push_state.take());
        self.scope_cache.insert(
            worktree_id,
            GitScopeState {
                repos: std::mem::take(&mut self.repos),
                selected_repo: std::mem::take(&mut self.selected_repo),
                branches: std::mem::take(&mut self.branches),
                view_branch: self.view_branch.take(),
                pull_state,
                push_state,
                section_ui: self.section_ui,
                refresh_needed: pull_refresh_needed || push_refresh_needed,
            },
        );
    }

    fn restore_scope(&mut self, worktree_id: Option<&WorktreeId>) -> bool {
        let state = worktree_id.and_then(|worktree_id| self.scope_cache.remove(worktree_id));
        if let Some(state) = state {
            self.repos = state.repos;
            self.selected_repo = state.selected_repo;
            self.branches = state.branches;
            self.view_branch = state.view_branch;
            self.pull_state = state.pull_state;
            self.push_state = state.push_state;
            self.section_ui = state.section_ui;
            state.refresh_needed
        } else {
            self.repos.clear();
            self.selected_repo.clear();
            self.branches.clear();
            self.view_branch = None;
            self.pull_state = None;
            self.push_state = None;
            self.section_ui = SectionUi::default();
            false
        }
    }

    fn switch_scope(
        &mut self,
        worktree_id: Option<WorktreeId>,
        path: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let refresh_needed = if orca_worktree_context_enabled() {
            self.save_scope();
            self.restore_scope(worktree_id.as_ref())
        } else {
            self.scope_cache.clear();
            self.restore_scope(None);
            false
        };
        self.scope_generation = self.scope_generation.wrapping_add(1);
        self.repo_request = self.repo_request.wrapping_add(1);
        self.branch_request = self.branch_request.wrapping_add(1);
        self.branches_loading = false;
        self.current_worktree = worktree_id;
        self.project_path = path;
        self.drag = None;
        self.push_repo_down(cx);
        if refresh_needed {
            self.changes.update(cx, |changes, cx| changes.load(cx));
            self.history.update(cx, |history, cx| history.reload(cx));
        }
        if self.pull_state.is_some() {
            self.schedule_sync_clear(true, cx);
        }
        if self.push_state.is_some() {
            self.schedule_sync_clear(false, cx);
        }
    }

    fn is_remote_project(&self, cx: &App) -> bool {
        self.store
            .read(cx)
            .active_project()
            .is_some_and(|p| p.ssh_connection_id.is_some())
    }

    fn child_scope(&self) -> GitScope {
        GitScope::new(
            self.current_worktree.clone(),
            self.scope_generation,
            orca_worktree_context_enabled(),
        )
    }

    fn scope_matches(&self, scope: &GitScope) -> bool {
        self.child_scope() == *scope
    }

    fn active_scope_matches_repo(&self, scope: &GitScope, repo_path: &str) -> bool {
        let current = self.child_scope();
        (current == *scope || current.same_cache_identity(scope)) && self.selected_repo == repo_path
    }

    fn mark_scope_refresh_needed(&mut self, scope: &GitScope, repo_path: &str) -> bool {
        let Some(worktree_id) = scope.cache_key() else {
            return false;
        };
        let Some(state) = self.scope_cache.get_mut(worktree_id) else {
            return false;
        };
        if state.selected_repo != repo_path {
            return false;
        }
        state.refresh_needed = true;
        true
    }

    fn handle_commit_completion(
        &mut self,
        scope: &GitScope,
        repo_path: &str,
        cx: &mut Context<Self>,
    ) {
        if self.active_scope_matches_repo(scope, repo_path) {
            self.history.update(cx, |history, cx| history.reload(cx));
            self.load_branches(cx);
        } else {
            self.mark_scope_refresh_needed(scope, repo_path);
        }
    }

    fn reconcile_stale_sync(
        &mut self,
        scope: &GitScope,
        repo_path: &str,
        pull: bool,
        cx: &mut Context<Self>,
    ) {
        if self.active_scope_matches_repo(scope, repo_path) {
            self.changes.update(cx, |changes, cx| changes.load(cx));
            if pull {
                self.history.update(cx, |history, cx| history.reload(cx));
            }
            self.refresh_repo_meta(cx);
        } else {
            self.mark_scope_refresh_needed(scope, repo_path);
        }
    }

    /// 项目变了:先显示该 worktree 的缓存，再重新发现仓库。
    fn on_project_changed(&mut self, cx: &mut Context<Self>) {
        self.stale = false;
        self.push_repo_down(cx);
        self.load_repos(cx);
    }

    /// 发现仓库(**重**:首次扫盘深度 5)。远程项目直接 return。
    fn load_repos(&mut self, cx: &mut Context<Self>) {
        if self.is_remote_project(cx) {
            return;
        }
        let Some(path) = self.project_path.clone() else {
            return;
        };
        self.repo_request = self.repo_request.wrapping_add(1);
        let req = self.repo_request;
        let generation = self.scope_generation;
        let worktree_id = self.current_worktree.clone();
        let task_path = std::path::PathBuf::from(&path);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { mt_project::git::discover_git_repos(&task_path) })
                .await;
            let _ = this.update(cx, |this: &mut Self, cx| {
                if !git_scope_request_matches(
                    generation,
                    this.scope_generation,
                    worktree_id.as_ref(),
                    this.current_worktree.as_ref(),
                ) || this.repo_request != req
                {
                    return;
                }
                match result {
                    Ok(repos) => {
                        // 选中仓库保持原值(若仍在列表里),否则取第一个
                        let keep = repos
                            .iter()
                            .any(|r| r.path.to_string_lossy() == this.selected_repo);
                        if !keep {
                            this.selected_repo = repos
                                .first()
                                .map(|r| r.path.to_string_lossy().to_string())
                                .unwrap_or_default();
                        }
                        this.repos = repos;
                    }
                    Err(err) => {
                        eprintln!("[git] 发现仓库失败: {err:#}");
                        this.repos.clear();
                    }
                }
                this.push_repo_down(cx);
                this.load_branches(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn load_branches(&mut self, cx: &mut Context<Self>) {
        let repo = self.selected_repo.clone();
        if repo.is_empty() {
            self.branches.clear();
            return;
        }
        self.branches_loading = true;
        self.branch_request = self.branch_request.wrapping_add(1);
        let req = self.branch_request;
        let generation = self.scope_generation;
        let worktree_id = self.current_worktree.clone();
        let task_repo = std::path::PathBuf::from(&repo);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { mt_project::git::get_repo_branches(&task_repo) })
                .await;
            let _ = this.update(cx, |this: &mut Self, cx| {
                // 迟到响应丢弃:换仓库后的旧响应不许覆盖
                if !git_scope_request_matches(
                    generation,
                    this.scope_generation,
                    worktree_id.as_ref(),
                    this.current_worktree.as_ref(),
                ) || this.branch_request != req
                    || this.selected_repo != repo
                {
                    return;
                }
                this.branches_loading = false;
                match result {
                    Ok(list) => this.branches = list,
                    Err(err) => eprintln!("[git] 取分支失败: {err:#}"),
                }
                this.push_repo_down(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// 仓库栏的手动刷新与 pty 嗅探回调都走这里(原版 `refreshRepoMeta`)。
    fn refresh_repo_meta(&mut self, cx: &mut Context<Self>) {
        self.load_repos(cx);
        self.load_branches(cx);
    }

    /// 把仓库 / 分支透给两个子面板。
    fn push_repo_down(&mut self, cx: &mut Context<Self>) {
        let scope = self.child_scope();
        let repo = self.selected_repo.clone();
        let branches = self.branches.clone();
        let view_branch = self.view_branch.clone();
        self.changes
            .update(cx, |c, cx| c.set_repo(scope.clone(), &repo, cx));
        self.history.update(cx, |h, cx| {
            h.sync(scope, &repo, &branches, view_branch.as_deref(), cx)
        });
    }

    fn select_repo(&mut self, path: String, cx: &mut Context<Self>) {
        if self.selected_repo == path {
            return;
        }
        self.selected_repo = path;
        // 换仓库:分支清空、viewBranch 复位、pull/push 状态清掉
        self.branches.clear();
        self.view_branch = None;
        self.pull_state = None;
        self.push_state = None;
        self.push_repo_down(cx);
        self.load_branches(cx);
        cx.notify();
    }

    fn selected_repo_info(&self) -> Option<&GitRepoInfo> {
        self.repos
            .iter()
            .find(|r| r.path.to_string_lossy() == self.selected_repo)
    }

    /// 当前 HEAD 分支名。**detached HEAD 时是 `"(1a2b3c4)"` 带括号的短 hash**
    /// (`git.rs:484-489`)—— 显示照旧,但绝不能当分支名传给 `get_git_log`。
    fn current_branch(&self) -> Option<&str> {
        self.selected_repo_info()
            .and_then(|r| r.current_branch.as_deref())
    }

    // ── pull / push ────────────────────────────────────────

    fn schedule_sync_clear(&mut self, pull: bool, cx: &mut Context<Self>) {
        let generation = self.scope_generation;
        let worktree_id = self.current_worktree.clone();
        let repo_path = self.selected_repo.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this: &mut Self, cx| {
                if !git_scope_request_matches(
                    generation,
                    this.scope_generation,
                    worktree_id.as_ref(),
                    this.current_worktree.as_ref(),
                ) || this.selected_repo != repo_path
                {
                    return;
                }
                if pull {
                    clear_sync_state(&mut this.pull_state);
                } else {
                    clear_sync_state(&mut this.push_state);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn run_sync(&mut self, pull: bool, cx: &mut Context<Self>) {
        if self.selected_repo.is_empty()
            || self.pull_state == Some(SyncState::Loading)
            || self.push_state == Some(SyncState::Loading)
        {
            return;
        }
        if pull {
            self.pull_state = Some(SyncState::Loading);
            self.push_state = None;
        } else {
            self.push_state = Some(SyncState::Loading);
            self.pull_state = None;
        }
        cx.notify();
        let repo_path = self.selected_repo.clone();
        let repo = std::path::PathBuf::from(&repo_path);
        let generation = self.scope_generation;
        let worktree_id = self.current_worktree.clone();
        let scope = self.child_scope();
        cx.spawn(async move |this, cx| {
            // ⚠️ pull / push 是 30s 阻塞 CLI,必须后台执行器
            let result = cx
                .background_executor()
                .spawn(async move {
                    if pull {
                        mt_project::git::git_pull(&repo)
                    } else {
                        mt_project::git::git_push(&repo)
                    }
                })
                .await;
            let ok = result.is_ok();
            let _ = this.update(cx, |this: &mut Self, cx| {
                if !git_scope_request_matches(
                    generation,
                    this.scope_generation,
                    worktree_id.as_ref(),
                    this.current_worktree.as_ref(),
                ) || this.selected_repo != repo_path
                {
                    if ok {
                        this.reconcile_stale_sync(&scope, &repo_path, pull, cx);
                    }
                    return;
                }
                let state = match result {
                    Ok(_) => SyncState::Success,
                    Err(err) => SyncState::Error(format!("{err:#}")),
                };
                if pull {
                    this.pull_state = Some(state);
                } else {
                    this.push_state = Some(state);
                }
                if ok {
                    this.load_branches(cx);
                    // pull 成功还要刷历史;push 只重载分支
                    if pull {
                        this.history.update(cx, |h, cx| h.reload(cx));
                    }
                }
                cx.notify();
            });
            // 无论成败 1500ms 后清回 None(`GitHistory.tsx:276`)
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this: &mut Self, cx| {
                if !git_scope_request_matches(
                    generation,
                    this.scope_generation,
                    worktree_id.as_ref(),
                    this.current_worktree.as_ref(),
                ) || this.selected_repo != repo_path
                {
                    return;
                }
                if pull {
                    clear_sync_state(&mut this.pull_state);
                } else {
                    clear_sync_state(&mut this.push_state);
                }
                cx.notify();
            });
        })
        .detach();
    }
}

// ─── 渲染 ─────────────────────────────────────────────────────

fn centered_hint(text: &'static str) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(ui::bg_surface())
        .text_size(ui::font_px(15.0))
        .text_color(ui::text_muted())
        .child(text)
        .into_any_element()
}

/// 任意 flex-grow(gpui 的 `flex_grow()` 只会设成 1)。
fn grow<E: Styled>(mut el: E, value: f32) -> E {
    el.style().flex_grow = Some(value);
    el
}

impl Render for GitPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 空态按顺序短路(`GitHistory.tsx:331-345`)
        if self.store.read(cx).active_project().is_none() {
            return div()
                .size_full()
                .child(centered_hint(t("gitHistory", "selectProject")));
        }
        if self.is_remote_project(cx) {
            // git 命令跑在本地,对远程路径无意义(远程 Git 二期)
            return div()
                .size_full()
                .border_t_1()
                .border_color(ui::border_subtle())
                .child(centered_hint(t("gitHistory", "remoteNotSupported")));
        }

        let section = self.section_ui;
        let both_open = section.changes_open && section.history_open;
        let changes_grow = if section.changes_open {
            if section.history_open {
                section.ratio
            } else {
                1.0
            }
        } else {
            0.0
        };
        let history_grow = if section.history_open {
            if section.changes_open {
                1.0 - section.ratio
            } else {
                1.0
            }
        } else {
            0.0
        };

        let this = cx.entity();
        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(ui::bg_surface())
            .border_t_1()
            .border_color(ui::border_subtle());

        if !self.repos.is_empty() {
            root = root.child(self.render_repo_bar(cx));
        }

        root = root
            .child(section_header(
                "git-section-changes",
                t("panels", "changes"),
                section.changes_open,
                false,
                Some(self.render_view_mode_toggle(cx)),
                cx.listener(|this, _: &ClickEvent, _window, cx| {
                    this.section_ui.changes_open = !this.section_ui.changes_open;
                    cx.notify();
                }),
            ))
            .child(grow(
                div()
                    .flex_basis(px(0.0))
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .child(self.changes.clone()),
                changes_grow,
            ));

        if both_open {
            root = root.child(self.render_section_handle(cx));
        }

        root = root
            .child(section_header(
                "git-section-history",
                t("panels", "history"),
                section.history_open,
                true,
                None,
                cx.listener(|this, _: &ClickEvent, _window, cx| {
                    this.section_ui.history_open = !this.section_ui.history_open;
                    cx.notify();
                }),
            ))
            .child(grow(
                div()
                    .flex_basis(px(0.0))
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .child(self.history.clone()),
                history_grow,
            ))
            // 量一次两块内容区加起来的高度:中缝拖拽要按它换算比例
            .child(
                canvas(
                    move |bounds: Bounds<Pixels>, _window, cx| {
                        this.update(cx, |panel: &mut GitPanel, _cx| {
                            // 整块面板的高度减掉固定件(仓库栏 34 + 两个 header 30)
                            let fixed = if panel.repos.is_empty() { 60.0 } else { 94.0 };
                            panel.sections_height =
                                (f32::from(bounds.size.height) - fixed).max(1.0);
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );

        // 拖拽期间鼠标可能划出手柄,移动/松手挂在面板根上
        root.when(self.drag.is_some(), |el| {
            el.on_mouse_move(
                cx.listener(|this: &mut Self, event: &MouseMoveEvent, _window, cx| {
                    let Some(drag) = this.drag else { return };
                    if drag.total <= 0.0 {
                        return;
                    }
                    let dy = f32::from(event.position.y - drag.start_y);
                    let next = clamp_ratio(drag.start_ratio + dy / drag.total);
                    this.section_ui.ratio = next;
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this: &mut Self, _: &MouseUpEvent, _window, cx| {
                    this.drag = None;
                    cx.notify();
                }),
            )
        })
    }
}

/// SectionHeader(`GitHistory.tsx:69-100`)。`bordered` 只有下方「提交历史」用。
/// `trailing` 是右侧的动作位(「更改」的视图切换按钮住这);它自己的 on_click
/// 要 stop_propagation,否则会连带触发 header 的折叠。
fn section_header(
    id: &'static str,
    label: &'static str,
    open: bool,
    bordered: bool,
    trailing: Option<AnyElement>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(4.0))
        .px(px(8.0))
        .h(px(30.0))
        .flex_none()
        .cursor_pointer()
        .text_color(ui::text_primary())
        .hover(|el| el.bg(ui::border_subtle()))
        .when(bordered, |el| {
            el.border_t_1().border_color(ui::border_subtle())
        })
        .child(
            div()
                .w(px(12.0))
                .text_center()
                .text_size(ui::font_px(15.0))
                .text_color(ui::text_muted())
                // 原版靠 `transform: rotate(-90deg)` 转 ▾;gpui 没有元素旋转,
                // 换成两个字形(与文件树同一处理)
                .child(if open { "▾" } else { "▸" }),
        )
        .child(div().text_size(ui::font_px(13.0)).child(label))
        .when_some(trailing, |el, trailing| {
            el.child(div().flex_1()).child(trailing)
        })
        .on_click(on_click)
        .into_any_element()
}

impl GitPanel {
    /// 中缝拖拽手柄(`GitHistory.tsx:516-523`):零高包裹 + 内部 6px 绝对定位条。
    fn render_section_handle(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .relative()
            .h(px(0.0))
            .flex_none()
            .child(
                div()
                    .id("git-section-handle")
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(-3.0))
                    .h(px(6.0))
                    .cursor_row_resize()
                    .hover(|el| el.bg(ui::with_alpha(ui::accent(), 0.4)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this: &mut Self, event: &MouseDownEvent, _window, cx| {
                            cx.stop_propagation();
                            let ratio = this.section_ui.ratio;
                            this.drag = Some(SectionDrag {
                                start_y: event.position.y,
                                start_ratio: ratio,
                                total: this.sections_height,
                            });
                        }),
                    ),
            )
            .into_any_element()
    }

    /// 仓库栏(`GitHistory.tsx:353-499`)。
    fn render_repo_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let info = self.selected_repo_info();
        let repo_name = info.map(|r| r.name.clone()).unwrap_or_default();
        let is_worktree = info.is_some_and(|r| r.is_worktree);
        let repo_path_tip = self.selected_repo.clone();
        let display_branch = self
            .view_branch
            .clone()
            .or_else(|| self.current_branch().map(str::to_string));
        let current = self.current_branch().map(str::to_string);
        let viewing_other = self.view_branch.is_some() && self.view_branch != current;

        let mut bar = div()
            .flex()
            .items_center()
            .h(px(34.0))
            .flex_none()
            .pl(px(6.0))
            .pr(px(8.0))
            .border_b_1()
            .border_color(ui::border_subtle());

        // 仓库下拉触发器(左键开下拉 / 右键开菜单)
        bar = bar.child(
            div()
                .id("git-repo-trigger")
                .flex()
                .items_center()
                .gap(px(4.0))
                .px(px(4.0))
                .py(px(2.0))
                .rounded(px(4.0))
                .cursor_pointer()
                .min_w(px(0.0))
                .text_color(ui::color_folder())
                .hover(|el| el.bg(ui::border_subtle()))
                .child(div().text_color(ui::text_muted()).child("▾"))
                .child(
                    div()
                        .truncate()
                        .text_size(ui::font_px(13.0))
                        .child(SharedString::from(repo_name)),
                )
                .when(is_worktree, |el| {
                    el.child(
                        div()
                            .text_size(ui::font_px(13.0))
                            .text_color(ui::text_muted())
                            .child("⎇"),
                    )
                })
                .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                    let entries = this.repo_menu(cx);
                    menu::show(event.position(), entries, window, cx);
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        let entries = this.repo_context_menu(cx);
                        menu::show(event.position, entries, window, cx);
                    }),
                ),
        );
        let _ = repo_path_tip;

        // 分支徽章
        if let Some(branch) = display_branch {
            let (bg, fg) = if viewing_other {
                (
                    ui::with_alpha(mt_ui::rgb8(88, 166, 255), 0.15),
                    mt_ui::rgb8(88, 166, 255),
                )
            } else {
                (ui::border_subtle(), ui::text_muted())
            };
            bar = bar.child(
                div()
                    .id("git-branch-badge")
                    .ml(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .px(px(6.0))
                    .rounded(px(3.0))
                    .cursor_pointer()
                    .bg(bg)
                    .text_color(fg)
                    .text_size(ui::font_px(13.0))
                    .child(div().max_w(px(140.0)).truncate().child(branch))
                    .child(div().text_size(ui::font_px(11.0)).opacity(0.7).child("▾"))
                    .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                        // 分支列表为空时懒加载一次(`GitHistory.tsx:422`)
                        if this.branches.is_empty() {
                            this.load_branches(cx);
                        }
                        let entries = this.branch_menu(cx);
                        menu::show(event.position(), entries, window, cx);
                    })),
            );
        }

        bar = bar.child(div().flex_1());

        // 刷新
        bar = bar.child(
            div()
                .id("git-repo-refresh")
                .w(px(20.0))
                .h(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .flex_none()
                .rounded(px(3.0))
                .cursor_pointer()
                .text_size(ui::font_px(13.0))
                .text_color(ui::text_muted())
                .hover(|el| el.text_color(ui::text_primary()))
                .child("↻")
                .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                    this.refresh_repo_meta(cx);
                    // 「更改」列表也要跟着重取 —— push_repo_down 的 set_repo
                    // 对同仓库路径会短路,不显式 load 它就纹丝不动
                    this.changes.update(cx, |c, cx| c.load(cx));
                    this.history.update(cx, |h, cx| h.reload(cx));
                })),
        );

        bar = bar
            .child(self.render_sync_button(true, cx))
            .child(self.render_sync_button(false, cx));

        bar.into_any_element()
    }

    /// `GitActionButton`(`GitHistory.tsx:21-67`)。
    fn render_sync_button(&self, pull: bool, cx: &mut Context<Self>) -> AnyElement {
        let state = if pull {
            &self.pull_state
        } else {
            &self.push_state
        };
        let busy = self.pull_state == Some(SyncState::Loading)
            || self.push_state == Some(SyncState::Loading);
        let (glyph, color) = match state {
            Some(SyncState::Loading) => ("↻", ui::text_muted()),
            Some(SyncState::Success) => ("✓", ui::color_success()),
            Some(SyncState::Error(_)) => ("✕", ui::color_error()),
            None => (if pull { "↓" } else { "↑" }, ui::text_muted()),
        };
        // title:出错时是错误全文,否则是**硬编码**的 'Git Pull' / 'Git Push'
        // (原版这两个字符串没进 i18n,照抄)
        let tip: SharedString = match state {
            Some(SyncState::Error(err)) => err.clone().into(),
            _ => if pull { "Git Pull" } else { "Git Push" }.into(),
        };
        div()
            .id(if pull { "git-pull" } else { "git-push" })
            .w(px(20.0))
            .h(px(20.0))
            .flex()
            .items_center()
            .justify_center()
            .flex_none()
            .rounded(px(3.0))
            .text_size(ui::font_px(13.0))
            .text_color(color)
            .when(busy, |el| el.opacity(0.5))
            .when(!busy, |el| {
                el.cursor_pointer()
                    .hover(|el| el.text_color(ui::text_primary()))
            })
            .child(glyph)
            .tooltip(move |window, cx| mt_ui::tooltip::Tooltip::new(tip.clone()).build(window, cx))
            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| this.run_sync(pull, cx)))
            .into_any_element()
    }

    /// 「更改」标题栏右侧的视图切换(树/列表)。原先住在 GitChanges 的工具栏里,
    /// 工具栏撤掉后上移到这。
    fn render_view_mode_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
        let tree_mode = self.store.read(cx).git_changes_view_mode() == "tree";
        div()
            .id("git-changes-view-mode")
            .w(px(20.0))
            .h(px(20.0))
            .flex()
            .items_center()
            .justify_center()
            .flex_none()
            .rounded(px(3.0))
            .text_size(ui::font_px(12.0))
            .text_color(ui::text_muted())
            .hover(|el| el.text_color(ui::text_primary()))
            // list 时显示 ⊞(点它切树),tree 时显示 ≡(点它切列表)
            .child(if tree_mode { "≡" } else { "⊞" })
            .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                // 按钮在 header 里,不拦住冒泡会连带把整块折叠
                cx.stop_propagation();
                this.store.update(cx, |store, cx| {
                    let next = if store.git_changes_view_mode() == "tree" {
                        "list"
                    } else {
                        "tree"
                    };
                    store.set_git_changes_view_mode(next, cx);
                });
                // GitChanges 不 observe store,列表要重排得显式踢它一脚
                this.changes.update(cx, |_, cx| cx.notify());
                cx.notify();
            }))
            .into_any_element()
    }

    /// 仓库下拉。
    ///
    /// **走 [`menu`] 而不是自绘 absolute 面板**:点外关 / Esc / 贴边收拢 / 焦点
    /// 还原四件事那边已经做全了。代价是行内没法画「选中行 accent 底色」与右侧
    /// 分支胶囊的独立配色 —— 选中用 `✓ ` 前缀(菜单基件本来的勾选方案),
    /// 分支放右侧的弱化标签位。
    fn repo_menu(&self, cx: &mut Context<Self>) -> Vec<menu::MenuEntry> {
        let this = cx.entity();
        let scope = self.child_scope();
        self.repos
            .iter()
            .map(|repo| {
                let path = repo.path.to_string_lossy().to_string();
                let selected = path == self.selected_repo;
                let label = if selected {
                    format!("✓ {}", repo.name)
                } else {
                    format!("　{}", repo.name)
                };
                let mut item = MenuItem::new(label);
                if let Some(branch) = &repo.current_branch {
                    item = item.shortcut(if repo.is_worktree {
                        format!("⎇ {branch}")
                    } else {
                        branch.clone()
                    });
                }
                let this = this.clone();
                let scope = scope.clone();
                item.on_click(move |_window, cx| {
                    let path = path.clone();
                    this.update(cx, |this, cx| {
                        if this.scope_matches(&scope) {
                            this.select_repo(path, cx);
                        }
                    });
                })
                .into()
            })
            .collect()
    }

    /// 分支下拉。**只改历史显示,不做 checkout**。
    fn branch_menu(&self, cx: &mut Context<Self>) -> Vec<menu::MenuEntry> {
        if self.branches.is_empty() {
            return vec![
                MenuItem::new(if self.branches_loading {
                    t("gitHistoryContent", "loading")
                } else {
                    t("gitHistoryContent", "noCommits")
                })
                .disabled(true)
                .into(),
            ];
        }
        let this = cx.entity();
        let scope = self.child_scope();
        let current = self.current_branch().map(str::to_string);
        self.branches
            .iter()
            .map(|branch| {
                let name = branch.name.clone();
                let selected = Some(&name) == self.view_branch.as_ref();
                // 远程用空心圈、本地用实心点(原版是两种颜色的小圆点,
                // 菜单基件的行是纯文本,换成字形区分)
                let dot = if branch.is_remote { "○" } else { "●" };
                let label = format!("{}{dot} {name}", if selected { "✓ " } else { "　" });
                let mut item = MenuItem::new(label);
                if Some(&name) == current.as_ref() {
                    item = item.shortcut("HEAD");
                }
                let this = this.clone();
                let scope = scope.clone();
                item.on_click(move |_window, cx| {
                    let name = name.clone();
                    this.update(cx, |this, cx| {
                        if !this.scope_matches(&scope) {
                            return;
                        }
                        this.view_branch = Some(name);
                        this.push_repo_down(cx);
                        cx.notify();
                    });
                })
                .into()
            })
            .collect()
    }

    /// 仓库栏右键菜单(`GitHistory.tsx:300-329`)。
    fn repo_context_menu(&self, cx: &mut Context<Self>) -> Vec<menu::MenuEntry> {
        let Some(repo) = self.selected_repo_info().cloned() else {
            return Vec::new();
        };
        let store = self.store.clone();
        let project_id = self.store.read(cx).active_project_id.clone();
        let project_path = self.project_path.clone().unwrap_or_default();
        let repo_path = repo.path.to_string_lossy().to_string();
        // 项目根仓库不带 cwd 覆盖(默认就是项目根);尾部分隔符归一化后比较
        let same_as_root = trim_trailing_sep(&repo_path) == trim_trailing_sep(&project_path);
        let title = if repo.is_worktree {
            format!(
                "⎇ {}",
                repo.current_branch.clone().unwrap_or(repo.name.clone())
            )
        } else {
            repo.name.clone()
        };
        let this = cx.entity();
        let scope = self.child_scope();
        let terminal_entity = this.clone();
        let terminal_scope = scope.clone();
        let repo_for_worktree = repo_path.clone();

        vec![
            menu::item(
                t("gitHistoryContent", "openInTerminal"),
                move |window, cx| {
                    if !terminal_entity.read(cx).scope_matches(&terminal_scope) {
                        return;
                    }
                    let Some(project_id) = project_id.clone() else {
                        return;
                    };
                    let (cwd, title) = if same_as_root {
                        (None, None)
                    } else {
                        (Some(repo_path.clone()), Some(title.clone()))
                    };
                    let opened = store.update(cx, |store, cx| {
                        let pane =
                            store.new_terminal_with_cwd(&project_id, None, None, cwd, window, cx);
                        if let (Some(pane), Some(title)) = (pane.as_ref(), title) {
                            store.rename_pane(&project_id, pane, &title, cx);
                        }
                        pane.is_some()
                    });
                    if opened {
                        crate::workbench_area::activate_terminal_page(window, cx);
                    }
                },
            ),
            menu::separator(),
            menu::item(
                t("gitHistoryContent", "manageWorktrees"),
                move |window, cx| {
                    if !this.read(cx).scope_matches(&scope) {
                        return;
                    }
                    let this = this.clone();
                    let scope = scope.clone();
                    crate::git_worktree::open(
                        // 单仓库模式:discover_repos = false
                        repo_for_worktree.clone(),
                        false,
                        None,
                        move |cx| {
                            crate::worktree_catalog::force_refresh_global(cx);
                            this.update(cx, |this, cx| {
                                if this.scope_matches(&scope) {
                                    this.load_repos(cx);
                                }
                            });
                        },
                        window,
                        cx,
                    );
                },
            ),
        ]
    }
}

/// 去掉尾部的 `/` 与 `\`(原版 `replace(/[\\/]+$/,'')`)。
pub fn trim_trailing_sep(path: &str) -> &str {
    path.trim_end_matches(['/', '\\'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worktree_id(hex: char) -> WorktreeId {
        format!("worktree-v1:{}", hex.to_string().repeat(64))
            .parse()
            .unwrap()
    }

    #[test]
    fn 工作树缓存恢复仓库分支与分区状态() {
        let worktree_a = worktree_id('a');
        let worktree_b = worktree_id('b');
        let mut cache = HashMap::new();
        let state_a = GitScopeState {
            repos: Vec::new(),
            selected_repo: "/repo/a".into(),
            branches: Vec::new(),
            view_branch: Some("feature-a".into()),
            pull_state: Some(SyncState::Success),
            push_state: None,
            section_ui: SectionUi {
                changes_open: false,
                history_open: true,
                ratio: 0.35,
            },
            refresh_needed: false,
        };

        let (mut state_b, cached_b) = swap_worktree_scope(
            &mut cache,
            Some(&worktree_a),
            Some(&worktree_b),
            state_a,
            || GitScopeState {
                repos: Vec::new(),
                selected_repo: String::new(),
                branches: Vec::new(),
                view_branch: None,
                pull_state: None,
                push_state: None,
                section_ui: SectionUi::default(),
                refresh_needed: false,
            },
        );
        assert!(!cached_b);
        state_b.selected_repo = "/repo/b".into();
        state_b.view_branch = Some("feature-b".into());
        state_b.section_ui.ratio = 0.7;

        let (restored_a, cached_a) = swap_worktree_scope(
            &mut cache,
            Some(&worktree_b),
            Some(&worktree_a),
            state_b,
            || unreachable!("worktree A was cached"),
        );
        assert!(cached_a);
        assert_eq!(restored_a.selected_repo, "/repo/a");
        assert_eq!(restored_a.view_branch.as_deref(), Some("feature-a"));
        assert!(matches!(restored_a.pull_state, Some(SyncState::Success)));
        assert!(!restored_a.section_ui.changes_open);
        assert!(restored_a.section_ui.history_open);
        assert_eq!(restored_a.section_ui.ratio, 0.35);
    }

    #[test]
    fn 切走时在途同步转成待刷新而结果状态可恢复() {
        let (loading, refresh_needed) = suspend_sync_state(Some(SyncState::Loading));
        assert!(loading.is_none());
        assert!(refresh_needed);

        let (success, refresh_needed) = suspend_sync_state(Some(SyncState::Success));
        assert!(matches!(success, Some(SyncState::Success)));
        assert!(!refresh_needed);

        let mut newer_loading = Some(SyncState::Loading);
        clear_sync_state(&mut newer_loading);
        assert!(matches!(newer_loading, Some(SyncState::Loading)));

        let mut completed = Some(SyncState::Error("network".into()));
        clear_sync_state(&mut completed);
        assert!(completed.is_none());
    }

    #[test]
    fn 工作树请求同时校验身份与代次() {
        let worktree_a = worktree_id('a');
        let worktree_b = worktree_id('b');
        assert!(git_scope_request_matches(
            9,
            9,
            Some(&worktree_a),
            Some(&worktree_a)
        ));
        assert!(!git_scope_request_matches(
            8,
            9,
            Some(&worktree_a),
            Some(&worktree_a)
        ));
        assert!(!git_scope_request_matches(
            9,
            9,
            Some(&worktree_a),
            Some(&worktree_b)
        ));
    }

    #[test]
    fn stable_scope_identity_ignores_generation_but_requires_enabled_worktree() {
        let worktree_a = worktree_id('a');
        let worktree_b = worktree_id('b');
        let first = GitScope::new(Some(worktree_a.clone()), 1, true);
        let returned = GitScope::new(Some(worktree_a), 3, true);
        let other = GitScope::new(Some(worktree_b), 3, true);
        assert!(first.same_cache_identity(&returned));
        assert!(!first.same_cache_identity(&other));
        assert!(!first.same_cache_identity(&GitScope::new(None, 3, false)));
    }

    #[test]
    fn scope_gate_keeps_legacy_git_path_comparison() {
        let worktree_a = worktree_id('a');
        let worktree_b = worktree_id('b');
        assert!(git_panel_scope_changed(
            true,
            Some(&worktree_a),
            Some(&worktree_b),
            Some("/repo/shared"),
            Some("/repo/shared"),
        ));
        assert!(!git_panel_scope_changed(
            false,
            Some(&worktree_a),
            Some(&worktree_b),
            Some("/repo/shared"),
            Some("/repo/shared"),
        ));
        assert!(git_panel_scope_changed(
            false,
            Some(&worktree_a),
            Some(&worktree_a),
            Some("/repo/a"),
            Some("/repo/b"),
        ));
    }

    /// 比例钳在 0.15~0.85。
    #[test]
    fn 区块比例钳位() {
        assert_eq!(clamp_ratio(0.5), 0.5);
        assert_eq!(clamp_ratio(0.0), 0.15);
        assert_eq!(clamp_ratio(-3.0), 0.15);
        assert_eq!(clamp_ratio(1.0), 0.85);
        assert_eq!(clamp_ratio(0.15), 0.15);
        assert_eq!(clamp_ratio(0.85), 0.85);
    }

    /// 「在终端中打开」:项目根仓库**不带** cwd 覆盖,子仓库/worktree 才带。
    /// 判据是尾部分隔符归一化后的字符串比较。
    #[test]
    fn 项目根仓库不带_cwd_覆盖() {
        let project = r"D:\Git\mini-term";
        // 完全相同
        assert_eq!(
            trim_trailing_sep(r"D:\Git\mini-term"),
            trim_trailing_sep(project)
        );
        // 只差一个尾部反斜杠 —— 仍然算同一个,不该带覆盖
        assert_eq!(
            trim_trailing_sep(r"D:\Git\mini-term\"),
            trim_trailing_sep(project)
        );
        // 多个尾部分隔符也要吃掉
        assert_eq!(
            trim_trailing_sep(r"D:\Git\mini-term\\"),
            trim_trailing_sep(project)
        );
        assert_eq!(
            trim_trailing_sep("/home/u/proj//"),
            trim_trailing_sep("/home/u/proj")
        );
        // 子仓库:必须带覆盖
        assert_ne!(
            trim_trailing_sep(r"D:\Git\mini-term\sub"),
            trim_trailing_sep(project)
        );
    }

    /// detached HEAD 的 `current_branch` 是 `"(1a2b3c4)"` —— 它**不在**
    /// `get_repo_branches` 的结果里,所以 `viewBranch` 永远取不到它,
    /// 也就永远不会被当分支名传给 `get_git_log`(那会 `bail!`)。
    #[test]
    fn detached_head_不当分支名查询() {
        let branches = [
            BranchInfo {
                name: "main".into(),
                is_head: false,
                is_remote: false,
                commit_hash: "abc".into(),
            },
            BranchInfo {
                name: "origin/main".into(),
                is_head: false,
                is_remote: true,
                commit_hash: "abc".into(),
            },
        ];
        let detached = "(1a2b3c4)";
        assert!(
            !branches.iter().any(|b| b.name == detached),
            "括号短 hash 绝不会出现在分支列表里"
        );
        // viewBranch 只从分支列表里取 → 括号短 hash 进不去
        let picked: Option<&str> = branches
            .iter()
            .find(|b| b.name == detached)
            .map(|b| b.name.as_str());
        assert!(picked.is_none());
    }
}
