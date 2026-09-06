//! Git 面板的「更改」区。对应 `src/components/GitChanges.tsx`(442 行)。
//!
//! # 三个分组的口径(`GitChanges.tsx:109-111`)
//!
//! ```text
//! staged    = 有 stagedStatus
//! unstaged  = 有 unstagedStatus 且不是 untracked
//! untracked = unstagedStatus === 'untracked'
//! ```
//!
//! ⚠️ **同一个文件可以同时出现在 staged 与 unstaged 两组**(部分暂存),这是正确
//! 行为 —— 所以行的 `ElementId` 必须带区名前缀(原版 key 是 `${area}-${path}`,
//! 规格 §11 第 29 条),否则 gpui 会撞 id。
//!
//! # 失败一律静默
//!
//! 原版每个 `invoke` 的 catch 都只 `console.error`(不弹 toast、不显红),
//! 这里对应 `eprintln!`。**唯一的例外**是「丢弃」前的确认框(§4.5)。
//!
//! # 阻塞调用
//!
//! `get_changes_status` / `git_stage_all` / `git_unstage_all` / `git_discard_file`
//! 是 git2 的同步 IO,`git_commit` 是带 60s 超时的 git CLI —— 全部丢
//! `cx.background_executor()`(范式照 `file_tree.rs:138-156`)。

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, AppContext as _, ClickEvent, Context, Entity, EventEmitter, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, ParentElement, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use mt_identity::WorktreeId;
use mt_project::git::cli::RepositoryAuthority;
use mt_project::git::{ChangeFileStatus, GitStatus};

use crate::git_backend::{GitRead, GitReadValue, GitRepository, GitWrite, PreparedGitWrite};
use crate::git_panel::host_ui;
use crate::git_panel::{GitScope, swap_worktree_scope};
use crate::i18n::{t, tr};
use crate::menu;
use crate::prompt::Confirm;
use crate::store::AppStore;
use crate::ui;
use crate::{git_diff, git_watch};

gpui::actions!(
    mini_term,
    [
        /// 提交(Ctrl+Enter / Cmd+Enter,`GitChanges.tsx:411-415`)
        GitCommitMessage
    ]
);

/// 「更改」区往上冒的事件。原版是 `onCommitSuccess` 这个 prop。
pub(crate) enum GitChangesEvent {
    /// Known or possible effects require source-owned reconciliation, including errors.
    Reconciled(GitScope, GitRepository),
    StatusLoaded(GitScope, GitRepository, mt_project::git::cli::HeadState),
}

/// 三个分组。`area` 既决定取哪一个 status,也进 `ElementId` 前缀。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Staged,
    Unstaged,
    Untracked,
}

impl Area {
    fn key(self) -> &'static str {
        match self {
            Area::Staged => "staged",
            Area::Unstaged => "unstaged",
            Area::Untracked => "untracked",
        }
    }
}

/// 该文件在该区里生效的状态(staged 区取 `stagedStatus`,其余取 `unstagedStatus`)。
pub fn status_in_area(file: &ChangeFileStatus, area: Area) -> Option<&GitStatus> {
    match area {
        Area::Staged => file.staged_status.as_ref(),
        _ => file.unstaged_status.as_ref(),
    }
}

/// 状态 → 单字符(`statusLabelFor`,`GitChanges.tsx:60-70`)。认不出的是**空格**。
pub fn status_label_for(status: Option<&GitStatus>) -> &'static str {
    match status {
        Some(GitStatus::Modified) => "M",
        Some(GitStatus::Added) => "A",
        Some(GitStatus::Deleted) => "D",
        Some(GitStatus::Renamed) => "R",
        Some(GitStatus::Untracked) => "?",
        Some(GitStatus::Conflicted) => "C",
        None => " ",
    }
}

/// 状态 → 颜色(`statusColor`,`GitChanges.tsx:72-82`)。
///
/// ⚠️ `conflicted` 落到 default 的 muted —— 原版如此,照抄。
pub fn status_color(status: Option<&GitStatus>) -> gpui::Hsla {
    match status {
        Some(GitStatus::Modified) => ui::color_warning(),
        Some(GitStatus::Added) => ui::color_success(),
        Some(GitStatus::Deleted) => ui::color_error(),
        Some(GitStatus::Renamed) => ui::color_info(),
        Some(GitStatus::Untracked) => ui::color_success(),
        _ => ui::text_muted(),
    }
}

// ─── 树形视图 ─────────────────────────────────────────────────

/// `buildFileTree` 建出来的节点(`GitChanges.tsx:36-56`)。
///
/// `file` 是**下标**而不是拷贝 —— 建树只是换个排列方式,内容仍在原数组里。
#[derive(Debug, PartialEq)]
pub struct FileTreeNode {
    pub name: String,
    pub full_path: String,
    pub file: Option<usize>,
    pub children: Vec<FileTreeNode>,
}

/// 按 `/` 切 path 建目录树。**不做单链目录压缩**(与 FileTree 的
/// `compactDirChains` 不同,这里没有)。
pub fn build_file_tree(paths: &[(usize, &str)]) -> Vec<FileTreeNode> {
    let mut root: Vec<FileTreeNode> = Vec::new();
    for (index, path) in paths {
        let parts: Vec<&str> = path.split('/').collect();
        let mut current = &mut root;
        let mut path_so_far = String::new();
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                path_so_far.push('/');
            }
            path_so_far.push_str(part);
            if i == parts.len() - 1 {
                current.push(FileTreeNode {
                    name: (*part).to_string(),
                    full_path: path_so_far.clone(),
                    file: Some(*index),
                    children: Vec::new(),
                });
            } else {
                let pos = current
                    .iter()
                    .position(|n| n.name == *part && n.file.is_none());
                let pos = match pos {
                    Some(pos) => pos,
                    None => {
                        current.push(FileTreeNode {
                            name: (*part).to_string(),
                            full_path: path_so_far.clone(),
                            file: None,
                            children: Vec::new(),
                        });
                        current.len() - 1
                    }
                };
                current = &mut current[pos].children;
            }
        }
    }
    root
}

/// 拍平后的一行。
enum TreeRow {
    Dir {
        name: String,
        full_path: String,
        depth: usize,
        collapsed: bool,
    },
    File {
        index: usize,
        depth: usize,
    },
}

fn flatten_tree(
    nodes: &[FileTreeNode],
    area: Area,
    collapsed: &HashSet<String>,
    depth: usize,
    out: &mut Vec<TreeRow>,
) {
    for node in nodes {
        match node.file {
            Some(index) => out.push(TreeRow::File { index, depth }),
            None => {
                // 折叠集合 key 带区名 —— 同一路径在三个区各自独立折叠
                // (`GitChanges.tsx:285`、`:292`)
                let key = format!("{}:{}", area.key(), node.full_path);
                let is_collapsed = collapsed.contains(&key);
                out.push(TreeRow::Dir {
                    name: node.name.clone(),
                    full_path: node.full_path.clone(),
                    depth,
                    collapsed: is_collapsed,
                });
                if !is_collapsed {
                    flatten_tree(&node.children, area, collapsed, depth + 1, out);
                }
            }
        }
    }
}

// ─── 组件 ─────────────────────────────────────────────────────

struct GitChangesScopeState {
    repo_path: String,
    repo_authority: Option<RepositoryAuthority>,
    changes: Vec<ChangeFileStatus>,
    collapsed_dirs: HashSet<String>,
    commit_draft: String,
    scroll: ScrollHandle,
    refresh_needed: bool,
}

impl GitChangesScopeState {
    fn empty() -> Self {
        Self {
            repo_path: String::new(),
            repo_authority: None,
            changes: Vec::new(),
            collapsed_dirs: HashSet::new(),
            commit_draft: String::new(),
            scroll: ScrollHandle::new(),
            refresh_needed: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GitChangesOwner {
    scope: GitScope,
    repo_generation: u64,
    repo_path: String,
    authority: Option<RepositoryAuthority>,
}

fn git_changes_owner_matches(request: &GitChangesOwner, current: &GitChangesOwner) -> bool {
    request == current
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GitChangesActionOwner {
    owner: GitChangesOwner,
    status_request: u64,
}

fn changes_action_matches(
    captured: &GitChangesActionOwner,
    current: &GitChangesActionOwner,
    status_current: bool,
) -> bool {
    status_current && captured == current
}

fn commit_draft_should_clear(
    succeeded: bool,
    revision: u64,
    current_revision: u64,
    message: &str,
    draft: &str,
) -> bool {
    succeeded && revision == current_revision && draft.trim() == message
}

#[derive(Clone, PartialEq, Eq)]
struct RetainedDraft {
    scope: GitScope,
    repo_path: String,
    authority: Option<RepositoryAuthority>,
    message: String,
}

fn mark_cached_git_changes_refresh(
    cache: &mut HashMap<WorktreeId, GitChangesScopeState>,
    scope: &GitScope,
    repo_path: &str,
) -> bool {
    let Some(worktree_id) = scope.cache_key() else {
        return false;
    };
    let Some(state) = cache.get_mut(worktree_id) else {
        return false;
    };
    if state.repo_path != repo_path {
        return false;
    }
    state.refresh_needed = true;
    true
}

pub struct GitChanges {
    store: Entity<AppStore>,
    /// 空串 = 无仓库。此时 `load` 直接 return(既不 loading 也不报错,
    /// 停在「暂无变更」——`GitChanges.tsx:115`)。
    repo_path: String,
    repository: Option<GitRepository>,
    repo_authority: Option<RepositoryAuthority>,
    cache_sources: HashMap<WorktreeId, GitScope>,
    retained_drafts: Vec<RetainedDraft>,
    load_error: Option<String>,
    operation_error: Option<String>,
    status_current: bool,
    untracked_directories: Vec<String>,
    write_request: u64,
    draft_revision: u64,
    pending_clear_revision: Option<u64>,
    _draft_subscription: gpui::Subscription,
    scope: GitScope,
    scope_cache: HashMap<WorktreeId, GitChangesScopeState>,
    repo_generation: u64,
    changes: Vec<ChangeFileStatus>,
    loading: bool,
    refresh_again: bool,
    commit_input: Entity<InputState>,
    pending_commit_draft: Option<String>,
    committing: bool,
    mutations_in_flight: usize,
    /// 组件态,**不落盘**(抽屉一关就没,规格 §11 第 25 条)。
    collapsed_dirs: HashSet<String>,
    scroll: ScrollHandle,
    /// 迟到响应丢弃。
    request: u64,
    /// pty-output 嗅探的 500ms 去抖终点。
    debounce_until: Option<Instant>,
}

impl EventEmitter<GitChangesEvent> for GitChanges {}

impl GitChanges {
    pub fn new(store: Entity<AppStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let commit_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(3)
                .placeholder(t("panels", "commitPlaceholder"))
        });
        let draft_subscription = cx.subscribe(&commit_input, |this: &mut Self, _, event, _| {
            if matches!(event, InputEvent::Change) {
                this.draft_revision = this.draft_revision.wrapping_add(1);
            }
        });
        Self {
            store,
            repo_path: String::new(),
            repository: None,
            repo_authority: None,
            cache_sources: HashMap::new(),
            retained_drafts: Vec::new(),
            load_error: None,
            operation_error: None,
            status_current: false,
            untracked_directories: Vec::new(),
            write_request: 0,
            draft_revision: 0,
            pending_clear_revision: None,
            _draft_subscription: draft_subscription,
            scope: GitScope::unbound(),
            scope_cache: HashMap::new(),
            repo_generation: 0,
            changes: Vec::new(),
            loading: false,
            refresh_again: false,
            commit_input,
            pending_commit_draft: None,
            committing: false,
            mutations_in_flight: 0,
            collapsed_dirs: HashSet::new(),
            scroll: ScrollHandle::new(),
            request: 0,
            debounce_until: None,
        }
    }

    fn current_repository(&self, cx: &gpui::App) -> Option<GitRepository> {
        self.repository
            .as_ref()
            .filter(|repo| host_ui::active_repository(repo, &self.store, cx))
            .cloned()
    }

    fn can_write(&self, cx: &gpui::App) -> bool {
        self.status_current
            && !self.loading
            && self.mutations_in_flight == 0
            && self
                .current_repository(cx)
                .is_some_and(|repo| repo.busy().is_none())
    }

    fn recoverable_draft(&self) -> Option<RetainedDraft> {
        self.retained_drafts
            .iter()
            .rev()
            .find(|draft| {
                draft.scope.same_draft_context(&self.scope)
                    && draft.repo_path == self.repo_path
                    && draft.authority.is_some()
                    && draft.authority == self.repo_authority
            })
            .cloned()
    }

    fn retain_draft(&mut self, scope: GitScope, state: &GitChangesScopeState) {
        if !state.commit_draft.is_empty() {
            self.retained_drafts.push(RetainedDraft {
                scope,
                repo_path: state.repo_path.clone(),
                authority: state.repo_authority.clone(),
                message: state.commit_draft.clone(),
            });
        }
    }

    fn current_owner(&self) -> GitChangesOwner {
        GitChangesOwner {
            scope: self.scope.clone(),
            repo_generation: self.repo_generation,
            repo_path: self.repo_path.clone(),
            authority: self.repo_authority.clone(),
        }
    }

    fn owner_matches(&self, owner: &GitChangesOwner) -> bool {
        git_changes_owner_matches(owner, &self.current_owner())
    }

    fn action_owner(&self) -> GitChangesActionOwner {
        GitChangesActionOwner {
            owner: self.current_owner(),
            status_request: self.request,
        }
    }

    fn action_matches(&self, owner: &GitChangesActionOwner, cx: &gpui::App) -> bool {
        changes_action_matches(owner, &self.action_owner(), self.status_current)
            && owner.owner.scope.matches_active(&self.store, cx)
    }

    fn reconcile_stale_mutation(&mut self, owner: &GitChangesOwner, cx: &mut Context<Self>) {
        if self.scope.same_cache_identity(&owner.scope)
            && self.repo_path == owner.repo_path
            && self.repo_authority == owner.authority
        {
            self.load(cx);
            return;
        }
        if owner
            .scope
            .cache_key()
            .and_then(|key| self.cache_sources.get(key))
            .is_some_and(|scope| scope.same_source(&owner.scope))
            && owner
                .scope
                .cache_key()
                .and_then(|key| self.scope_cache.get(key))
                .is_some_and(|state| state.repo_authority == owner.authority)
        {
            mark_cached_git_changes_refresh(&mut self.scope_cache, &owner.scope, &owner.repo_path);
        }
    }

    fn take_scope_state(&mut self, cx: &mut Context<Self>) -> GitChangesScopeState {
        let mut commit_draft = self
            .pending_commit_draft
            .take()
            .unwrap_or_else(|| self.commit_input.read(cx).value().to_string());
        if self.pending_clear_revision.take() == Some(self.draft_revision) {
            commit_draft.clear();
        }
        GitChangesScopeState {
            repo_path: std::mem::take(&mut self.repo_path),
            repo_authority: self.repo_authority.take(),
            changes: std::mem::take(&mut self.changes),
            collapsed_dirs: std::mem::take(&mut self.collapsed_dirs),
            commit_draft,
            scroll: std::mem::replace(&mut self.scroll, ScrollHandle::new()),
            refresh_needed: self.loading || self.committing || self.mutations_in_flight > 0,
        }
    }

    fn install_scope_state(&mut self, state: GitChangesScopeState) -> bool {
        self.repo_path = state.repo_path;
        self.repo_authority = state.repo_authority;
        self.changes = state.changes;
        self.collapsed_dirs = state.collapsed_dirs;
        self.pending_commit_draft = Some(state.commit_draft);
        self.scroll = state.scroll;
        self.pending_clear_revision = None;
        state.refresh_needed
    }

    /// 换 worktree / 仓库(容器调)。空串 = 无仓库。
    pub(crate) fn set_repository(
        &mut self,
        scope: GitScope,
        repo_path: &str,
        repository: Option<GitRepository>,
        cx: &mut Context<Self>,
    ) {
        let scope_changed = self.scope != scope;
        let became_ready = self.repository.is_none() && repository.is_some();
        self.repository = repository;
        let mut restored = true;
        let mut refresh_needed = false;

        if scope_changed {
            self.request = self.request.wrapping_add(1);
            let current_key = self.scope.cache_key().cloned();
            let next_key = scope.cache_key().cloned();
            if current_key.is_some() || next_key.is_some() {
                if let Some(key) = &current_key {
                    self.cache_sources.insert(key.clone(), self.scope.clone());
                }
                let current_state = self.take_scope_state(cx);
                let (mut state, was_cached) = swap_worktree_scope(
                    &mut self.scope_cache,
                    current_key.as_ref(),
                    next_key.as_ref(),
                    current_state,
                    GitChangesScopeState::empty,
                );
                if !next_key
                    .as_ref()
                    .and_then(|key| self.cache_sources.get(key))
                    .is_some_and(|saved| saved.same_source(&scope))
                {
                    if let Some(saved) = next_key
                        .as_ref()
                        .and_then(|key| self.cache_sources.get(key))
                        .cloned()
                    {
                        self.retain_draft(saved, &state);
                    }
                    state = GitChangesScopeState::empty();
                }
                restored = was_cached;
                refresh_needed = self.install_scope_state(state);
            } else {
                self.scope_cache.clear();
                self.cache_sources.clear();
                if !self.scope.same_source(&scope) {
                    let state = self.take_scope_state(cx);
                    self.retain_draft(self.scope.clone(), &state);
                    self.install_scope_state(GitChangesScopeState::empty());
                }
            }
            self.scope = scope;
            self.repo_generation = self.repo_generation.wrapping_add(1);
            self.loading = false;
            self.refresh_again = false;
            self.committing = false;
            self.mutations_in_flight = 0;
            self.debounce_until = None;
            self.status_current = false;
            self.untracked_directories.clear();
            self.load_error = None;
            self.operation_error = None;
            self.write_request = self.write_request.wrapping_add(1);
        }

        let authority = self
            .repository
            .as_ref()
            .map(|repo| repo.authority().clone());
        let authority_changed = authority
            .as_ref()
            .zip(self.repo_authority.as_ref())
            .is_some_and(|(new, old)| new != old);
        let repo_changed = self.repo_path != repo_path || authority_changed;
        if authority.is_some() {
            self.repo_authority = authority;
        }
        if repo_changed {
            self.repo_path = repo_path.to_string();
            self.repo_generation = self.repo_generation.wrapping_add(1);
            self.request = self.request.wrapping_add(1);
            self.loading = false;
            self.refresh_again = false;
            self.committing = false;
            self.mutations_in_flight = 0;
            self.debounce_until = None;
            self.changes.clear();
            self.collapsed_dirs.clear();
            self.scroll = ScrollHandle::new();
            self.status_current = false;
            self.untracked_directories.clear();
            self.write_request = self.write_request.wrapping_add(1);
        }

        if self.repo_path.is_empty() {
            self.loading = false;
        } else if became_ready || repo_changed || (scope_changed && (!restored || refresh_needed)) {
            self.load(cx);
        }

        if scope_changed || repo_changed {
            cx.notify();
        }
    }

    /// 容器的 pty-output 嗅探命中了 —— 起(或推后)自己那个 500ms 去抖窗口。
    ///
    /// 原版是两个面板**各有一个**定时器(规格 §11 第 27 条),所以这里的窗口
    /// 与提交历史区各算各的。
    pub fn note_pty_hit(&mut self) {
        self.debounce_until = Some(Instant::now() + Duration::from_millis(git_watch::DEBOUNCE_MS));
    }

    /// 容器的节拍:去抖窗口到点了就重取。
    pub fn tick(&mut self, cx: &mut Context<Self>) {
        if self.debounce_until.is_some_and(|at| Instant::now() >= at) {
            self.debounce_until = None;
            self.load(cx);
        }
    }

    pub fn load(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            self.refresh_again = true;
            return;
        }
        let Some(repository) = self.current_repository(cx) else {
            return;
        };
        let request = match repository.request(GitRead::Status) {
            Ok(request) => request,
            Err(error) => {
                self.load_error = Some(error.to_string());
                self.status_current = false;
                return;
            }
        };
        self.loading = true;
        self.status_current = false;
        self.request = request.id();
        let req = self.request;
        let owner = self.current_owner();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { request.execute() })
                .await;
            let _ = this.update(cx, |this: &mut Self, cx| {
                if this.request != req
                    || !this.owner_matches(&owner)
                    || !owner.scope.matches_active(&this.store, cx)
                {
                    return;
                }
                this.loading = false;
                if std::mem::take(&mut this.refresh_again) {
                    this.load(cx);
                    return;
                }
                match result {
                    Ok(result) => {
                        let Some(current) = this.current_repository(cx) else {
                            return;
                        };
                        if !result.is_current(&current, req) {
                            return;
                        }
                        if let GitReadValue::Status(status) = result.value {
                            cx.emit(GitChangesEvent::StatusLoaded(
                                owner.scope.clone(),
                                current,
                                status.head,
                            ));
                            this.changes = status.changes;
                            this.untracked_directories = status.untracked_directories;
                            this.status_current = true;
                            this.load_error = None;
                        }
                    }
                    Err(err) => {
                        this.load_error = Some(err.to_string());
                        this.status_current = false;
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn group_indices(&self, area: Area) -> Vec<usize> {
        self.changes
            .iter()
            .enumerate()
            .filter(|(_, c)| match area {
                Area::Staged => c.staged_status.is_some(),
                Area::Unstaged => matches!(
                    c.unstaged_status,
                    Some(ref s) if *s != GitStatus::Untracked
                ),
                Area::Untracked => c.unstaged_status == Some(GitStatus::Untracked),
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Workers retain their write lease through completion, even after the
    /// panel changes source. UI ownership never substitutes for that lease.
    fn run_op(&mut self, op: GitWrite, cx: &mut Context<Self>) {
        if !self.can_write(cx) {
            return;
        }
        self.run_write(op, None, cx);
    }

    fn run_write(
        &mut self,
        op: GitWrite,
        prepared: Option<PreparedGitWrite>,
        cx: &mut Context<Self>,
    ) {
        let Some(repository) = self.current_repository(cx) else {
            return;
        };
        let owner = self.current_owner();
        self.write_request = self.write_request.wrapping_add(1);
        let write_request = self.write_request;
        let draft_revision = self.draft_revision;
        let message = match &op {
            GitWrite::Commit { message } => Some(message.clone()),
            _ => None,
        };
        self.committing = message.is_some();
        self.mutations_in_flight = 1;
        self.operation_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match prepared {
                        Some(prepared) => Ok(prepared.execute()),
                        None => repository
                            .prepare_write(op)
                            .map(|prepared| prepared.execute()),
                    }
                })
                .await;
            let _ = this.update(cx, |this: &mut Self, cx| {
                if !this.owner_matches(&owner)
                    || this.write_request != write_request
                    || !owner.scope.matches_active(&this.store, cx)
                {
                    if let Ok(outcome) = &result {
                        this.reconcile_stale_mutation(&owner, cx);
                        cx.emit(GitChangesEvent::Reconciled(
                            owner.scope.clone(),
                            outcome.repository().clone(),
                        ));
                    }
                    return;
                }
                this.mutations_in_flight = 0;
                this.committing = false;
                match result {
                    Ok(outcome) => {
                        let Some(current) = this.current_repository(cx) else {
                            return;
                        };
                        if !outcome.is_current(&current, outcome.operation_id) {
                            return;
                        }
                        this.operation_error = host_ui::outcome_error(&outcome);
                        if message.as_deref().is_some_and(|message| {
                            commit_draft_should_clear(
                                outcome.succeeded(),
                                draft_revision,
                                this.draft_revision,
                                message,
                                &this.commit_input.read(cx).value(),
                            )
                        }) {
                            this.pending_clear_revision = Some(draft_revision);
                        }
                        cx.emit(GitChangesEvent::Reconciled(
                            owner.scope.clone(),
                            outcome.repository().clone(),
                        ));
                    }
                    Err(error) => this.operation_error = Some(error.to_string()),
                }
                this.load(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn stage(&mut self, path: String, cx: &mut Context<Self>) {
        self.run_op(GitWrite::Stage { paths: vec![path] }, cx);
    }

    fn unstage(&mut self, path: String, cx: &mut Context<Self>) {
        self.run_op(GitWrite::Unstage { paths: vec![path] }, cx);
    }

    fn stage_all(&mut self, cx: &mut Context<Self>) {
        self.run_op(GitWrite::StageAll, cx);
    }

    fn unstage_all(&mut self, cx: &mut Context<Self>) {
        self.run_op(GitWrite::UnstageAll, cx);
    }

    fn discard(&mut self, paths: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        if paths.is_empty() || !self.can_write(cx) {
            return;
        }
        let Some(repository) = self.current_repository(cx) else {
            return;
        };
        let count = paths.len();
        let owner = self.current_owner();
        self.write_request = self.write_request.wrapping_add(1);
        let operation = self.write_request;
        self.mutations_in_flight = 1;
        self.operation_error = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let prepared = cx
                .background_executor()
                .spawn(async move { repository.prepare_write(GitWrite::Discard { paths }) })
                .await;
            let _ = this.update_in(cx, |view, window, cx| {
                if !view.owner_matches(&owner)
                    || view.write_request != operation
                    || !owner.scope.matches_active(&view.store, cx)
                {
                    return;
                }
                view.mutations_in_flight = 0;
                let prepared = match prepared {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        view.operation_error = Some(error.to_string());
                        cx.notify();
                        return;
                    }
                };
                if view.current_repository(cx).is_none() {
                    return;
                }
                let selected = match prepared.operation() {
                    GitWrite::Discard { paths } => paths.clone(),
                    _ => return,
                };
                let slot = std::rc::Rc::new(std::cell::RefCell::new(Some(prepared)));
                let entity = cx.entity();
                Confirm::new(
                    t("gitChanges", "discardTitle"),
                    tr!("gitChanges", "discardConfirm", count = count.to_string()),
                )
                .detail(selected)
                .ok_text(t("gitChanges", "discardOk"))
                .cancel_text(t("gitChanges", "discardCancel"))
                .open(
                    move |_, cx| {
                        entity.update(cx, |view, cx| {
                            if !view.owner_matches(&owner)
                                || view.write_request != operation
                                || !view.can_write(cx)
                            {
                                return;
                            }
                            let Some(prepared) = slot.borrow_mut().take() else {
                                return;
                            };
                            view.run_write(prepared.operation().clone(), Some(prepared), cx);
                        });
                    },
                    window,
                    cx,
                );
                cx.notify();
            });
        })
        .detach();
    }

    fn commit(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let message = self
            .pending_commit_draft
            .as_deref()
            .map(str::trim)
            .map(str::to_string)
            .unwrap_or_else(|| self.commit_input.read(cx).value().trim().to_string());
        // 前置:消息非空 + 有暂存内容(`GitChanges.tsx:186`)
        if message.is_empty()
            || self.group_indices(Area::Staged).is_empty()
            || self.committing
            || self.repo_path.is_empty()
            || !self.can_write(cx)
        {
            return;
        }
        self.run_op(GitWrite::Commit { message }, cx);
    }

    fn view_diff(
        &self,
        owner: &GitChangesActionOwner,
        path: String,
        staged: bool,
        status_label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.action_matches(owner, cx) {
            return;
        }
        let Some(repository) = self.current_repository(cx) else {
            return;
        };
        let old_path = self
            .changes
            .iter()
            .find(|change| change.path == path)
            .and_then(|change| change.old_path.clone());
        git_diff::open_repository_file_diff(
            self.store.clone(),
            repository,
            path,
            old_path,
            staged,
            status_label,
            window,
            cx,
        );
    }

    fn apply_pending_commit_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_clear_revision.take() == Some(self.draft_revision) {
            self.commit_input
                .update(cx, |state, cx| state.set_value("", window, cx));
        }
        let Some(draft) = self.pending_commit_draft.take() else {
            return;
        };
        if self.commit_input.read(cx).value().as_ref() != draft.as_str() {
            self.commit_input
                .update(cx, |state, cx| state.set_value(draft, window, cx));
        }
    }
}

// ─── 渲染 ─────────────────────────────────────────────────────

/// 文案样式统一:`text-center text-[var(--text-muted)] text-sm py-6`。
fn placeholder_text(text: impl Into<SharedString>) -> AnyElement {
    div()
        .py(px(24.0))
        .w_full()
        .text_center()
        .text_size(ui::font_px(13.0))
        .text_color(ui::text_muted())
        .child(text.into())
        .into_any_element()
}

impl Render for GitChanges {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.apply_pending_commit_draft(window, cx);
        let tree_mode = self.store.read(cx).git_changes_view_mode() == "tree";
        let staged = self.group_indices(Area::Staged);
        let staged_count = staged.len();
        let unstaged = self.group_indices(Area::Unstaged);
        let untracked = self.group_indices(Area::Untracked);
        let empty = self.changes.is_empty();
        let action_owner = self.action_owner();
        let keyboard_owner = action_owner.clone();

        // ① 文件列表(原先上方还有一条 刷新/视图切换 工具栏:刷新并入仓库栏的
        // ↻,视图切换上移到「更改」标题栏右侧,整条撤掉)
        let mut list = div()
            .id("git-changes-list")
            .flex_1()
            .min_h(px(0.0))
            .track_scroll(&self.scroll)
            .overflow_y_scroll()
            .px(px(4.0))
            .pt(px(4.0));

        for error in [&self.load_error, &self.operation_error]
            .into_iter()
            .flatten()
        {
            list = list.child(
                div()
                    .p(px(8.0))
                    .text_size(ui::font_px(11.0))
                    .text_color(ui::color_error())
                    .child(error.clone()),
            );
        }
        if !self.untracked_directories.is_empty() {
            list = list.child(
                div()
                    .p(px(8.0))
                    .text_size(ui::font_px(11.0))
                    .child(format!(
                        "Untracked directories: {}",
                        self.untracked_directories.len()
                    ))
                    .child(
                        ui::ghost_button("git-stage-directories", t("gitChanges", "stageAll"))
                            .on_click({
                                let owner = action_owner.clone();
                                cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    if this.action_matches(&owner, cx) {
                                        this.stage_all(cx);
                                    }
                                })
                            }),
                    ),
            );
        }
        if !self.status_current && !empty {
            list = list.child(
                div()
                    .p(px(8.0))
                    .text_size(ui::font_px(11.0))
                    .text_color(ui::text_muted())
                    .child(if self.loading {
                        "Refreshing status"
                    } else {
                        "Last known status"
                    }),
            );
        }

        if self.loading && empty {
            list = list.child(placeholder_text(t("gitChanges", "loading")));
        } else if empty
            && self.load_error.is_none()
            && self.repository.is_some()
            && self.untracked_directories.is_empty()
        {
            list = list.child(placeholder_text(t("gitChanges", "empty")));
        } else {
            for (area, title, action_label) in [
                (
                    Area::Staged,
                    t("panels", "stagedChanges"),
                    t("gitChanges", "unstageAll"),
                ),
                (
                    Area::Unstaged,
                    t("panels", "unstagedChanges"),
                    t("gitChanges", "stageAll"),
                ),
                (
                    Area::Untracked,
                    t("panels", "untrackedFiles"),
                    t("gitChanges", "stageAll"),
                ),
            ] {
                let indices = match area {
                    Area::Staged => &staged,
                    Area::Unstaged => &unstaged,
                    Area::Untracked => &untracked,
                };
                if indices.is_empty() {
                    continue;
                }
                list = list.child(self.render_group(
                    area,
                    title,
                    action_label,
                    indices,
                    tree_mode,
                    cx,
                ));
            }
        }

        // ② 提交区
        let can_commit = staged_count > 0 && !self.committing && self.can_write(cx);
        let commit_label = if self.committing {
            t("gitChanges", "committing").to_string()
        } else {
            tr!("panels", "commit", count = staged_count.to_string())
        };
        let recovery = self
            .recoverable_draft()
            .filter(|_| self.commit_input.read(cx).value().is_empty());
        let recovery_owner = self.current_owner();
        let commit_area = div()
            .flex_none()
            .border_t_1()
            .border_color(ui::border_subtle())
            .p(px(8.0))
            .when_some(recovery, |el, draft| {
                el.child(
                    ui::ghost_button("git-restore-draft", "Restore draft").on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            if !this.owner_matches(&recovery_owner)
                                || !recovery_owner.scope.matches_active(&this.store, cx)
                                || !this.commit_input.read(cx).value().is_empty()
                                || this.committing
                                || this.recoverable_draft().as_ref() != Some(&draft)
                            {
                                return;
                            }
                            this.retained_drafts.retain(|saved| saved != &draft);
                            this.pending_commit_draft = Some(draft.message.clone());
                            cx.notify();
                        },
                    )),
                )
            })
            .child(Input::new(&self.commit_input))
            .child(
                div()
                    .id("git-commit-button")
                    .mt(px(6.0))
                    .py(px(6.0))
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .text_size(ui::font_px(13.0))
                    .when(can_commit, |el| {
                        el.bg(ui::accent())
                            .text_color(gpui::white())
                            .cursor_pointer()
                            .hover(|el| el.opacity(0.9))
                    })
                    .when(!can_commit, |el| {
                        el.bg(ui::bg_elevated()).text_color(ui::text_muted())
                    })
                    .child(commit_label)
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        if this.action_matches(&action_owner, cx) {
                            this.commit(window, cx);
                        }
                    })),
            );

        div()
            .size_full()
            .flex()
            .flex_col()
            // Ctrl+Enter / Cmd+Enter 直接提交(键位绑在 main.rs,谓词
            // `"GitChanges > Input"` —— 与项目切换器的方向键同一套路)
            .key_context("GitChanges")
            .on_action(cx.listener(move |this, _: &GitCommitMessage, window, cx| {
                if this.action_matches(&keyboard_owner, cx) {
                    this.commit(window, cx);
                }
            }))
            .child(list)
            .child(commit_area)
    }
}

impl GitChanges {
    fn render_group(
        &self,
        area: Area,
        title: &'static str,
        action_label: &'static str,
        indices: &[usize],
        tree_mode: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let action_owner = self.action_owner();
        let can_write = self.can_write(cx);
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .px(px(8.0))
            .py(px(4.0))
            .child(
                div()
                    .text_size(ui::font_px(11.0))
                    .text_color(ui::text_muted())
                    // 原版 `「{title} ({count})」`,大写与字距靠 CSS,
                    // gpui 没有 text-transform,照文案原样显示
                    .child(format!("{title} ({})", indices.len())),
            )
            .child(
                div()
                    .id(SharedString::from(format!(
                        "git-group-action-{}",
                        area.key()
                    )))
                    .text_size(ui::font_px(11.0))
                    .text_color(ui::text_muted())
                    .when(can_write, |el| el.cursor_pointer())
                    .when(!can_write, |el| el.opacity(0.5))
                    .hover(|el| el.text_color(ui::text_primary()))
                    .child(action_label)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        if !this.action_matches(&action_owner, cx) {
                            return;
                        }
                        // ⚠️ 未跟踪组的按钮虽写着「全部暂存」,走的也是 git_stage_all
                        // (`add_all("*")` 会连未跟踪一起暂存)—— 原版行为
                        match area {
                            Area::Staged => this.unstage_all(cx),
                            _ => this.stage_all(cx),
                        }
                    })),
            );

        let mut group = div().child(header);
        if tree_mode {
            let paths: Vec<(usize, &str)> = indices
                .iter()
                .map(|&i| (i, self.changes[i].path.as_str()))
                .collect();
            let nodes = build_file_tree(&paths);
            let mut rows = Vec::new();
            flatten_tree(&nodes, area, &self.collapsed_dirs, 0, &mut rows);
            for row in rows {
                group = group.child(match row {
                    TreeRow::Dir {
                        name,
                        full_path,
                        depth,
                        collapsed,
                    } => self.render_dir_row(area, name, full_path, depth, collapsed, cx),
                    TreeRow::File { index, depth } => self.render_file_row(index, area, depth, cx),
                });
            }
        } else {
            for &index in indices {
                group = group.child(self.render_file_row(index, area, 0, cx));
            }
        }
        group.into_any_element()
    }

    fn render_dir_row(
        &self,
        area: Area,
        name: String,
        full_path: String,
        depth: usize,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = format!("{}:{}", area.key(), full_path);
        let owner = self.action_owner();
        div()
            .id(SharedString::from(format!("git-dir-{key}")))
            .flex()
            .items_center()
            .gap(px(4.0))
            .py(px(2.0))
            .pr(px(8.0))
            .pl(px(depth as f32 * 16.0 + 8.0))
            .rounded(px(4.0))
            .cursor_pointer()
            .text_size(ui::font_px(13.0))
            .text_color(ui::text_muted())
            .hover(|el| el.bg(ui::border_subtle()))
            .child(
                div()
                    .w(px(12.0))
                    .text_center()
                    .child(if collapsed { "▸" } else { "▾" }),
            )
            .child(div().truncate().child(name))
            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                if !this.action_matches(&owner, cx) {
                    return;
                }
                if !this.collapsed_dirs.remove(&key) {
                    this.collapsed_dirs.insert(key.clone());
                }
                cx.notify();
            }))
            .into_any_element()
    }

    fn render_file_row(
        &self,
        index: usize,
        area: Area,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(file) = self.changes.get(index) else {
            return div().into_any_element();
        };
        let status = status_in_area(file, area);
        let label = status_label_for(status);
        let color = status_color(status);
        let path = file.path.clone();
        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
        let display = if depth > 0 { name } else { path.clone() };
        let is_staged = area == Area::Staged;
        let status_label = label.to_string();
        let owner = self.action_owner();
        // 行 id 必须带区名前缀:同一文件可同时在 staged 与 unstaged 两组
        let row_id = SharedString::from(format!("git-file-{}-{}", area.key(), path));
        let menu_path = path.clone();
        let menu_owner = owner.clone();
        let menu_status_label = status_label.clone();
        let can_write = self.can_write(cx);

        div()
            .id(row_id)
            .group("git-file-row")
            .flex()
            .items_center()
            .justify_between()
            .py(px(4.0))
            .pr(px(8.0))
            .pl(px(depth as f32 * 16.0 + 8.0))
            .rounded(px(4.0))
            .cursor_pointer()
            .text_size(ui::font_px(13.0))
            .hover(|el| el.bg(ui::border_subtle()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .flex_1()
                    .min_w(px(0.0))
                    .child(
                        div()
                            .flex_none()
                            .w(px(16.0))
                            .text_center()
                            .text_size(ui::font_px(11.0))
                            .text_color(color)
                            .child(label),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_color(ui::text_primary())
                            .child(display),
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!(
                        "git-file-act-{}-{}",
                        area.key(),
                        path
                    )))
                    .w(px(20.0))
                    .h(px(20.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_none()
                    .text_size(ui::font_px(13.0))
                    .text_color(ui::text_muted())
                    // 原版 `opacity-0 group-hover:opacity-100`
                    .opacity(0.0)
                    .group_hover("git-file-row", move |el| {
                        el.opacity(if can_write { 1.0 } else { 0.4 })
                    })
                    .hover(|el| el.text_color(ui::text_primary()))
                    .child(if is_staged { "−" } else { "+" })
                    .on_click({
                        let path = path.clone();
                        let owner = owner.clone();
                        cx.listener(move |this, event: &ClickEvent, _window, cx| {
                            // 行内按钮:别把整行的「看 diff」也触发了
                            let _ = event;
                            cx.stop_propagation();
                            if !this.action_matches(&owner, cx) {
                                return;
                            }
                            if is_staged {
                                this.unstage(path.clone(), cx);
                            } else {
                                this.stage(path.clone(), cx);
                            }
                        })
                    }),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.view_diff(
                    &owner,
                    path.clone(),
                    is_staged,
                    status_label.clone(),
                    window,
                    cx,
                );
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let entries = this.file_menu(
                        area,
                        menu_path.clone(),
                        menu_status_label.clone(),
                        menu_owner.clone(),
                        cx,
                    );
                    menu::show(event.position, entries, window, cx);
                }),
            )
            .into_any_element()
    }

    /// 文件行右键菜单(`GitChanges.tsx:242-256`)。
    fn file_menu(
        &self,
        area: Area,
        path: String,
        status_label: String,
        owner: GitChangesActionOwner,
        cx: &mut Context<Self>,
    ) -> Vec<menu::MenuEntry> {
        if !self.action_matches(&owner, cx) {
            return Vec::new();
        }
        let this = cx.entity();
        let can_write = self.can_write(cx);
        let mut entries = vec![
            {
                let this = this.clone();
                let owner = owner.clone();
                let path = path.clone();
                let status_label = status_label.clone();
                menu::item(t("gitChanges", "contextViewDiff"), move |window, cx| {
                    this.update(cx, |this, cx| {
                        this.view_diff(
                            &owner,
                            path.clone(),
                            area == Area::Staged,
                            status_label.clone(),
                            window,
                            cx,
                        )
                    });
                })
            },
            menu::separator(),
        ];
        entries.push(if area == Area::Staged {
            let (this, path) = (this.clone(), path.clone());
            let owner = owner.clone();
            menu::MenuItem::new(t("panels", "unstage"))
                .disabled(!can_write)
                .on_click(move |_window, cx| {
                    this.update(cx, |this, cx| {
                        if this.action_matches(&owner, cx) {
                            this.unstage(path.clone(), cx);
                        }
                    });
                })
                .into()
        } else {
            let (this, path) = (this.clone(), path.clone());
            let owner = owner.clone();
            menu::MenuItem::new(t("panels", "stage"))
                .disabled(!can_write)
                .on_click(move |_window, cx| {
                    this.update(cx, |this, cx| {
                        if this.action_matches(&owner, cx) {
                            this.stage(path.clone(), cx);
                        }
                    });
                })
                .into()
        });
        if area != Area::Staged {
            entries.push(menu::separator());
            entries.push(
                menu::MenuItem::new(t("gitChanges", "contextDiscard"))
                    .disabled(!can_write)
                    .on_click(move |window, cx| {
                        let path = path.clone();
                        this.update(cx, |this, cx| {
                            if this.action_matches(&owner, cx) {
                                this.discard(vec![path], window, cx);
                            }
                        });
                    })
                    .into(),
            );
        }
        entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_refresh_revokes_old_row_menus_without_reassigning_write_completion() {
        let source = crate::git_panel::source_tests::ssh_snapshot(Some(7));
        let captured = GitChangesActionOwner {
            owner: GitChangesOwner {
                scope: crate::git_panel::source_tests::scope(&source, 3),
                repo_generation: 5,
                repo_path: "/repo".into(),
                authority: Some(RepositoryAuthority {
                    worktree_root: "/repo".into(),
                    git_dir: "/repo/.git".into(),
                    common_dir: "/repo/.git".into(),
                }),
            },
            status_request: 11,
        };
        assert!(changes_action_matches(&captured, &captured, true));
        assert!(!changes_action_matches(&captured, &captured, false));
        let refreshed = GitChangesActionOwner {
            status_request: 12,
            ..captured.clone()
        };
        assert!(!changes_action_matches(&captured, &refreshed, true));
        assert!(git_changes_owner_matches(&captured.owner, &refreshed.owner));
        let mut returned = refreshed.clone();
        returned.owner.scope = crate::git_panel::source_tests::scope(&source, 5);
        assert!(!changes_action_matches(&refreshed, &returned, true));
        let mut rebound = refreshed.clone();
        rebound.owner.authority.as_mut().unwrap().common_dir = "/replacement/.git".into();
        assert!(!changes_action_matches(&refreshed, &rebound, true));
        assert!(!git_changes_owner_matches(&refreshed.owner, &rebound.owner));
    }

    #[test]
    fn commit_completion_preserves_newer_edits_even_when_text_matches() {
        assert!(commit_draft_should_clear(
            true,
            7,
            7,
            "message",
            " message\n"
        ));
        assert!(!commit_draft_should_clear(true, 7, 8, "message", "message"));
        assert!(!commit_draft_should_clear(
            true,
            7,
            7,
            "message",
            "new draft"
        ));
    }

    #[test]
    fn failed_or_uncertain_commit_keeps_its_draft() {
        assert!(!commit_draft_should_clear(
            false, 7, 7, "message", "message"
        ));
    }

    #[test]
    fn changes_owner_rejects_a_same_path_connection_replacement() {
        let source = crate::git_panel::source_tests::ssh_snapshot(Some(7));
        let current = GitChangesOwner {
            scope: crate::git_panel::source_tests::scope(&source, 3),
            repo_generation: 5,
            repo_path: "/repo".into(),
            authority: None,
        };
        let mut changed = source.clone();
        if let crate::execution_host::ExecutionBackend::Ssh {
            connection_epoch, ..
        } = &mut changed.backend
        {
            *connection_epoch = Some(8);
        }
        let stale = GitChangesOwner {
            scope: crate::git_panel::source_tests::scope(&changed, 3),
            ..current.clone()
        };
        assert!(!git_changes_owner_matches(&stale, &current));
    }

    fn worktree_id(hex: char) -> WorktreeId {
        format!("worktree-v1:{}", hex.to_string().repeat(64))
            .parse()
            .unwrap()
    }

    fn change(
        path: &str,
        staged: Option<GitStatus>,
        unstaged: Option<GitStatus>,
    ) -> ChangeFileStatus {
        ChangeFileStatus {
            path: path.to_string(),
            old_path: None,
            staged_status: staged,
            unstaged_status: unstaged,
            status_label: String::new(),
        }
    }

    fn group(changes: &[ChangeFileStatus], area: Area) -> Vec<&str> {
        changes
            .iter()
            .filter(|c| match area {
                Area::Staged => c.staged_status.is_some(),
                Area::Unstaged => {
                    matches!(c.unstaged_status, Some(ref s) if *s != GitStatus::Untracked)
                }
                Area::Untracked => c.unstaged_status == Some(GitStatus::Untracked),
            })
            .map(|c| c.path.as_str())
            .collect()
    }

    #[test]
    fn 同路径工作树恢复各自的变更草稿与滚动() {
        let worktree_a = worktree_id('a');
        let worktree_b = worktree_id('b');
        let mut cache = HashMap::new();

        let mut state_a = GitChangesScopeState::empty();
        state_a.repo_path = "/repo/shared".into();
        state_a.changes = vec![change("a.rs", Some(GitStatus::Added), None)];
        state_a.collapsed_dirs.insert("staged:src".into());
        state_a.commit_draft = "commit from A".into();
        state_a.scroll.set_offset(gpui::point(px(0.0), px(-48.0)));

        let (mut state_b, cached_b) = swap_worktree_scope(
            &mut cache,
            Some(&worktree_a),
            Some(&worktree_b),
            state_a,
            GitChangesScopeState::empty,
        );
        assert!(!cached_b);
        state_b.repo_path = "/repo/shared".into();
        state_b.changes = vec![change("b.rs", None, Some(GitStatus::Modified))];
        state_b.commit_draft = "commit from B".into();
        state_b.scroll.set_offset(gpui::point(px(0.0), px(-96.0)));

        let (restored_a, cached_a) = swap_worktree_scope(
            &mut cache,
            Some(&worktree_b),
            Some(&worktree_a),
            state_b,
            GitChangesScopeState::empty,
        );
        assert!(cached_a);
        assert_eq!(restored_a.repo_path, "/repo/shared");
        assert_eq!(restored_a.changes[0].path, "a.rs");
        assert!(restored_a.collapsed_dirs.contains("staged:src"));
        assert_eq!(restored_a.commit_draft, "commit from A");
        assert_eq!(restored_a.scroll.offset().y, px(-48.0));
    }

    #[test]
    fn 变更请求拒绝旧工作树旧代次与旧仓库() {
        let worktree_a = worktree_id('a');
        let worktree_b = worktree_id('b');
        let current = GitChangesOwner {
            scope: GitScope::new(Some(worktree_a.clone()), 8, true),
            repo_generation: 13,
            repo_path: "/repo/shared".into(),
            authority: None,
        };
        assert!(git_changes_owner_matches(&current, &current));

        let mut stale = current.clone();
        stale.scope = GitScope::new(Some(worktree_b), 8, true);
        assert!(!git_changes_owner_matches(&stale, &current));

        let mut stale = current.clone();
        stale.scope = GitScope::new(Some(worktree_a), 7, true);
        assert!(!git_changes_owner_matches(&stale, &current));

        let mut stale = current.clone();
        stale.repo_generation = 12;
        assert!(!git_changes_owner_matches(&stale, &current));

        let mut stale = current.clone();
        stale.repo_path = "/repo/other".into();
        assert!(!git_changes_owner_matches(&stale, &current));
    }

    #[test]
    fn stale_mutation_marks_only_the_matching_cached_repo() {
        let worktree = worktree_id('a');
        let scope = GitScope::new(Some(worktree.clone()), 4, true);
        let mut cache = HashMap::new();
        let mut state = GitChangesScopeState::empty();
        state.repo_path = "/repo/shared".into();
        cache.insert(worktree, state);

        assert!(!mark_cached_git_changes_refresh(
            &mut cache,
            &scope,
            "/repo/other",
        ));
        assert!(!cache.values().next().unwrap().refresh_needed);
        assert!(mark_cached_git_changes_refresh(
            &mut cache,
            &scope,
            "/repo/shared",
        ));
        assert!(cache.values().next().unwrap().refresh_needed);
        assert!(!mark_cached_git_changes_refresh(
            &mut cache,
            &GitScope::new(None, 9, false),
            "/repo/shared",
        ));
    }

    /// 三分组口径:未跟踪不落进「未暂存」,而**同一个文件可以同时进 staged 与
    /// unstaged**(部分暂存)。
    #[test]
    fn 三分组口径含部分暂存() {
        let changes = [
            change("a.rs", Some(GitStatus::Added), None),
            // 部分暂存:两组都要出现
            change("b.rs", Some(GitStatus::Modified), Some(GitStatus::Modified)),
            change("c.rs", None, Some(GitStatus::Modified)),
            change("d.rs", None, Some(GitStatus::Untracked)),
        ];
        assert_eq!(group(&changes, Area::Staged), vec!["a.rs", "b.rs"]);
        assert_eq!(group(&changes, Area::Unstaged), vec!["b.rs", "c.rs"]);
        assert_eq!(group(&changes, Area::Untracked), vec!["d.rs"]);
    }

    /// 取哪一个 status 由所在区决定 —— 部分暂存的文件在两组里显示的字母不同。
    #[test]
    fn 状态按区取值() {
        let file = change("b.rs", Some(GitStatus::Added), Some(GitStatus::Deleted));
        assert_eq!(status_label_for(status_in_area(&file, Area::Staged)), "A");
        assert_eq!(status_label_for(status_in_area(&file, Area::Unstaged)), "D");
        assert_eq!(
            status_label_for(status_in_area(&file, Area::Untracked)),
            "D",
            "untracked 区同样取 unstagedStatus"
        );
    }

    /// 六种状态 → 单字符。认不出(None)的是**空格**,不是空串。
    #[test]
    fn 状态字母表() {
        use GitStatus::*;
        let table = [
            (Modified, "M"),
            (Added, "A"),
            (Deleted, "D"),
            (Renamed, "R"),
            (Untracked, "?"),
            (Conflicted, "C"),
        ];
        for (status, label) in table {
            assert_eq!(status_label_for(Some(&status)), label);
        }
        assert_eq!(status_label_for(None), " ");
    }

    /// 状态色:`conflicted` **落到 muted**(原版 default 分支),不是错误色。
    #[test]
    fn 状态色映射() {
        use GitStatus::*;
        ui::set_palette(ui::Palette::dark());
        assert_eq!(status_color(Some(&Modified)), ui::color_warning());
        assert_eq!(status_color(Some(&Added)), ui::color_success());
        assert_eq!(status_color(Some(&Deleted)), ui::color_error());
        assert_eq!(status_color(Some(&Renamed)), ui::color_info());
        assert_eq!(status_color(Some(&Untracked)), ui::color_success());
        assert_eq!(
            status_color(Some(&Conflicted)),
            ui::text_muted(),
            "conflicted 走 default 分支 —— 原版如此"
        );
        assert_eq!(status_color(None), ui::text_muted());
    }

    /// 建树:嵌套路径共用目录节点,**不做单链压缩**。
    #[test]
    fn 建树共用目录节点() {
        let paths = [(0, "src/a.rs"), (1, "src/b.rs"), (2, "README.md")];
        let tree = build_file_tree(&paths);
        assert_eq!(tree.len(), 2, "src 目录 + README.md");
        assert_eq!(tree[0].name, "src");
        assert_eq!(tree[0].full_path, "src");
        assert!(tree[0].file.is_none());
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[0].full_path, "src/a.rs");
        assert_eq!(tree[0].children[0].file, Some(0));
        assert_eq!(tree[1].name, "README.md");
        assert_eq!(tree[1].file, Some(2));

        // 深嵌套不压缩:a/b/c/d.rs 是三层目录 + 一个文件
        let deep = build_file_tree(&[(0, "a/b/c/d.rs")]);
        assert_eq!(deep[0].name, "a");
        assert_eq!(deep[0].children[0].name, "b");
        assert_eq!(deep[0].children[0].children[0].name, "c");
        assert_eq!(deep[0].children[0].children[0].children[0].name, "d.rs");
    }

    /// 同名的目录与文件互不合并(`find` 带 `&& !n.file` 那道判定)。
    #[test]
    fn 同名目录与文件不合并() {
        let tree = build_file_tree(&[(0, "build"), (1, "build/out.js")]);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].file, Some(0), "先来的是文件");
        assert!(tree[1].file.is_none(), "后来的目录另起一个节点");
        assert_eq!(tree[1].children[0].full_path, "build/out.js");
    }

    /// 折叠 key 带区名:同一路径在三个区各自独立折叠。
    #[test]
    fn 折叠_key_按区隔离() {
        let tree = build_file_tree(&[(0, "src/a.rs")]);
        let mut collapsed = HashSet::new();
        collapsed.insert("staged:src".to_string());

        let mut rows = Vec::new();
        flatten_tree(&tree, Area::Staged, &collapsed, 0, &mut rows);
        assert_eq!(rows.len(), 1, "staged 区的 src 折起来了,只剩目录行");

        let mut rows = Vec::new();
        flatten_tree(&tree, Area::Unstaged, &collapsed, 0, &mut rows);
        assert_eq!(rows.len(), 2, "unstaged 区不受影响");
        assert!(matches!(rows[1], TreeRow::File { depth: 1, .. }));
    }
}
