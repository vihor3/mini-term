//! 主内容工作区：常驻终端页 + worktree 级文件页签。
//!
//! 文件页签是纯运行时状态，不进入 `SplitNode`、PTY 映射或布局数据库。切到文件页
//! 只是不渲染 [`TerminalArea`]，终端实体及其全部后台会话仍由 `AppStore` 保活。

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use gpui::{
    App, AppContext, Context, Entity, Global, InteractiveElement, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder,
    px,
};
use gpui_component::WindowExt as _;
use mt_identity::WorktreeId;
use mt_ui::icons::FileIcon;
use mt_ui::tooltip::Tooltip;

use crate::file_viewer::{DocumentSource, FileViewer};
use crate::github_tasks::{
    GitHubTaskService, GitHubWorkItemTabKey, GitHubWorkItemViewer, OpenGitHubWorkItem,
};
use crate::i18n::t;
use crate::prompt::{Confirm, show_alert};
use crate::store::AppStore;
use crate::terminal_area::TerminalArea;
use crate::ui;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum DocumentBackendKey {
    Local,
    Remote {
        connection_id: String,
        connection_fingerprint: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DocumentSourceKey {
    project_id: String,
    backend: DocumentBackendKey,
    normalized_path: String,
}

impl DocumentSourceKey {
    fn from_source(source: &DocumentSource) -> Self {
        let (backend, normalized_path) = match source {
            DocumentSource::Local { path, .. } => (
                DocumentBackendKey::Local,
                normalize_local_document_path(path),
            ),
            DocumentSource::Remote {
                connection, path, ..
            } => (
                DocumentBackendKey::Remote {
                    connection_id: connection.id.clone(),
                    connection_fingerprint: crate::remote_ssh::connection_fingerprint(connection),
                },
                normalize_remote_document_path(path),
            ),
        };
        Self {
            project_id: source.project_id().to_string(),
            backend,
            normalized_path,
        }
    }
}

/// 一个打开文件的稳定身份。worktree、后端连接身份和路径共同参与去重。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DocumentKey {
    worktree_id: WorktreeId,
    backend: DocumentBackendKey,
    normalized_path: String,
}

impl DocumentKey {
    fn from_source(worktree_id: WorktreeId, source: &DocumentSource) -> Self {
        let source_key = DocumentSourceKey::from_source(source);
        Self {
            worktree_id,
            backend: source_key.backend,
            normalized_path: source_key.normalized_path,
        }
    }

    fn matches_source_key(&self, source_key: &DocumentSourceKey) -> bool {
        self.backend == source_key.backend && self.normalized_path == source_key.normalized_path
    }
}

fn normalize_local_document_path(path: &Path) -> String {
    let normalized = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        normalized.replace('\\', "/").to_lowercase()
    } else {
        normalized
    }
}

fn normalize_remote_document_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum WorkbenchPage {
    Terminal,
    Document(DocumentKey),
    WorkItem(GitHubWorkItemTabKey),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DocumentTabState {
    Preview,
    Permanent,
}

impl DocumentTabState {
    fn promote(&mut self) {
        *self = Self::Permanent;
    }
}

struct DocumentTab {
    key: DocumentKey,
    // Compatibility callbacks still carry DocumentSource rather than a
    // WorktreeId. Keep the source project beside the stable key so those
    // callbacks can recover their originating worktree without consulting the
    // project's possibly changed current binding.
    source_project_id: String,
    title: String,
    document: Entity<FileViewer>,
    state: DocumentTabState,
}

struct WorkItemTab {
    key: GitHubWorkItemTabKey,
    source_project_id: String,
    title: String,
    viewer: Entity<GitHubWorkItemViewer>,
    state: DocumentTabState,
}

type RenderDocumentTab = (DocumentKey, String, Entity<FileViewer>, DocumentTabState);
type RenderWorkItemTab = (
    GitHubWorkItemTabKey,
    String,
    Entity<GitHubWorkItemViewer>,
    DocumentTabState,
);
type ActiveWorkbenchSnapshot = (
    String,
    WorktreeId,
    WorkbenchPage,
    Vec<RenderDocumentTab>,
    Vec<RenderWorkItemTab>,
);

struct WorktreeDocuments {
    tabs: Vec<DocumentTab>,
    work_items: Vec<WorkItemTab>,
    active: WorkbenchPage,
}

impl Default for WorktreeDocuments {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            work_items: Vec::new(),
            active: WorkbenchPage::Terminal,
        }
    }
}

impl WorktreeDocuments {
    fn index_of(&self, key: &DocumentKey) -> Option<usize> {
        self.tabs.iter().position(|tab| &tab.key == key)
    }

    fn insert_preview(&mut self, tab: DocumentTab, cx: &App) {
        insert_preview_tab(
            &mut self.tabs,
            tab,
            |tab| tab.state,
            |tab| tab.document.read(cx).is_dirty(),
            |tab| tab.state.promote(),
        );
    }

    fn promote(&mut self, key: &DocumentKey) -> bool {
        let Some(index) = self.index_of(key) else {
            return false;
        };
        if self.tabs[index].state == DocumentTabState::Permanent {
            return false;
        }
        self.tabs[index].state.promote();
        true
    }

    fn close(&mut self, key: &DocumentKey) -> Option<Entity<FileViewer>> {
        let index = self.index_of(key)?;
        let removed = self.tabs.remove(index).document;
        if self.active == WorkbenchPage::Document(key.clone()) {
            let remaining = self
                .tabs
                .iter()
                .map(|tab| tab.key.clone())
                .collect::<Vec<_>>();
            self.active = next_document_after_close(&remaining, index)
                .map(WorkbenchPage::Document)
                .unwrap_or(WorkbenchPage::Terminal);
        }
        Some(removed)
    }

    fn work_item_index_of(&self, key: &GitHubWorkItemTabKey) -> Option<usize> {
        self.work_items.iter().position(|tab| &tab.key == key)
    }

    fn insert_work_item_preview(&mut self, tab: WorkItemTab) {
        insert_preview_tab(
            &mut self.work_items,
            tab,
            |tab| tab.state,
            |_| false,
            |tab| tab.state.promote(),
        );
    }

    fn promote_work_item(&mut self, key: &GitHubWorkItemTabKey) -> bool {
        let Some(index) = self.work_item_index_of(key) else {
            return false;
        };
        if self.work_items[index].state == DocumentTabState::Permanent {
            return false;
        }
        self.work_items[index].state.promote();
        true
    }

    fn close_work_item(
        &mut self,
        key: &GitHubWorkItemTabKey,
    ) -> Option<Entity<GitHubWorkItemViewer>> {
        let index = self.work_item_index_of(key)?;
        let removed = self.work_items.remove(index).viewer;
        if self.active == WorkbenchPage::WorkItem(key.clone()) {
            let remaining = self
                .work_items
                .iter()
                .map(|tab| tab.key.clone())
                .collect::<Vec<_>>();
            self.active = next_tab_after_close(&remaining, index)
                .map(WorkbenchPage::WorkItem)
                .unwrap_or(WorkbenchPage::Terminal);
        }
        Some(removed)
    }
}

fn insert_preview_tab<T>(
    tabs: &mut Vec<T>,
    new_tab: T,
    tab_state: impl Fn(&T) -> DocumentTabState,
    tab_is_dirty: impl Fn(&T) -> bool,
    promote: impl Fn(&mut T),
) -> usize {
    if let Some(index) = tabs
        .iter()
        .position(|tab| tab_state(tab) == DocumentTabState::Preview)
    {
        if tab_is_dirty(&tabs[index]) {
            promote(&mut tabs[index]);
        } else {
            tabs[index] = new_tab;
            return index;
        }
    }

    tabs.push(new_tab);
    tabs.len() - 1
}

fn next_tab_after_close<T: Clone>(remaining: &[T], removed_index: usize) -> Option<T> {
    remaining
        .get(removed_index)
        .or_else(|| {
            removed_index
                .checked_sub(1)
                .and_then(|index| remaining.get(index))
        })
        .cloned()
}

fn next_document_after_close(
    remaining: &[DocumentKey],
    removed_index: usize,
) -> Option<DocumentKey> {
    next_tab_after_close(remaining, removed_index)
}

fn project_binding_matches(
    bound_worktree_id: Option<&WorktreeId>,
    expected_worktree_id: &WorktreeId,
) -> bool {
    bound_worktree_id == Some(expected_worktree_id)
}

fn source_project_binding_matches(
    project_bindings: &HashMap<String, WorktreeId>,
    source_project_id: &str,
    expected_worktree_id: &WorktreeId,
) -> bool {
    project_binding_matches(
        project_bindings.get(source_project_id),
        expected_worktree_id,
    )
}

fn active_worktree_matches(
    active_worktree_id: Option<&WorktreeId>,
    active_project_worktree_id: Option<&WorktreeId>,
    expected_worktree_id: &WorktreeId,
) -> bool {
    active_worktree_id == Some(expected_worktree_id)
        && active_project_worktree_id == Some(expected_worktree_id)
}

fn active_scope_matches(
    active_project_id: Option<&str>,
    active_worktree_id: Option<&WorktreeId>,
    expected_project_worktree_id: Option<&WorktreeId>,
    expected_project_id: &str,
    expected_worktree_id: &WorktreeId,
) -> bool {
    active_project_id == Some(expected_project_id)
        && active_worktree_matches(
            active_worktree_id,
            expected_project_worktree_id,
            expected_worktree_id,
        )
}

struct GlobalWorkbench(Entity<WorkbenchArea>);
impl Global for GlobalWorkbench {}

/// 安装统一文件打开入口。文件树和全局搜索都只调用这里。
pub fn install(area: Entity<WorkbenchArea>, cx: &mut App) {
    cx.set_global(GlobalWorkbench(area));
}

fn global(cx: &App) -> Option<Entity<WorkbenchArea>> {
    cx.try_global::<GlobalWorkbench>()
        .map(|global| global.0.clone())
}

fn worktree_documents_are_dirty(worktree: &WorktreeDocuments, cx: &App) -> bool {
    worktree
        .tabs
        .iter()
        .any(|tab| tab.document.read(cx).is_dirty())
}

/// 项目移除、worktree 清理等生命周期操作的统一防丢失闸。
pub fn project_has_dirty_documents(project_id: &str, cx: &App) -> bool {
    global(cx).is_some_and(|area| {
        let area = area.read(cx);
        area.worktrees.values().any(|documents| {
            documents
                .tabs
                .iter()
                .any(|tab| tab.source_project_id == project_id && tab.document.read(cx).is_dirty())
        })
    })
}

/// Rebinding a compatibility project while any of its documents are open
/// would either retag an in-flight callback or make the old tab unreachable.
/// Remote runtime identity therefore defers until the next clean activation.
pub fn project_has_documents(project_id: &str, cx: &App) -> bool {
    global(cx).is_some_and(|area| {
        area.read(cx).worktrees.values().any(|documents| {
            documents
                .tabs
                .iter()
                .any(|tab| tab.source_project_id == project_id)
        })
    })
}

/// 关窗确认使用的未保存文档列表。项目名与页签名一起展示，避免不同项目中的
/// 同名文件让用户无法判断哪些草稿会被丢弃。
pub fn dirty_document_names(cx: &App) -> Vec<String> {
    let Some(area) = global(cx) else {
        return Vec::new();
    };
    let area = area.read(cx);
    let store = area.store.read(cx);
    let mut names = Vec::new();
    for documents in area.worktrees.values() {
        for tab in &documents.tabs {
            if tab.document.read(cx).is_dirty() {
                let project_name = store
                    .project(&tab.source_project_id)
                    .map(|project| project.name.as_str())
                    .unwrap_or(&tab.source_project_id);
                names.push(format!("{project_name}: {}", tab.title));
            }
        }
    }
    names.sort();
    names
}

/// 按当前项目快照打开文件。远程项目会同时快照连接身份，断链时明确报错。
pub fn open_active_file(
    store: Entity<AppStore>,
    path: PathBuf,
    highlight_line: Option<u32>,
    window: &mut Window,
    cx: &mut App,
) {
    let snapshot = {
        let store = store.read(cx);
        let Some(project) = store.active_project() else {
            return;
        };
        let Some(worktree_id) = store.active_worktree_id().cloned() else {
            return;
        };
        if !project_binding_matches(store.worktree_id_for_project(&project.id), &worktree_id) {
            return;
        }
        (
            project.clone(),
            worktree_id,
            store.is_remote_project(&project.id),
            store.remote_connection_of(&project.id),
        )
    };
    let (project, worktree_id, remote, connection) = snapshot;
    let source = if remote {
        let Some(connection) = connection else {
            show_alert(
                t("terminalArea", "remoteConnectFailedTitle"),
                t("fileTree", "remote.broken"),
                window,
                cx,
            );
            return;
        };
        DocumentSource::Remote {
            project_id: project.id.clone(),
            connection,
            project_root: project.path.clone(),
            path,
        }
    } else {
        DocumentSource::Local {
            project_id: project.id.clone(),
            project_root: PathBuf::from(&project.path),
            path,
        }
    };

    let Some(area) = global(cx) else {
        return;
    };
    area.update(cx, |area, cx| {
        area.open_document(worktree_id, source, highlight_line, window, cx)
    });
}

/// Open one exact GitHub work item in the active worktree's unified tab strip.
pub fn open_github_work_item(
    service: Entity<GitHubTaskService>,
    request: OpenGitHubWorkItem,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(area) = global(cx) else {
        return;
    };
    area.update(cx, |area, cx| {
        area.open_work_item(service, request, window, cx)
    });
}

/// 文件页内部的 Ctrl/Cmd+W 入口。延迟执行前先快照来源身份，避免用户在
/// `window.defer` 落地前切换页签后误关新的活动页。
pub fn close_document_source(
    expected_worktree_id: WorktreeId,
    source: DocumentSource,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(area) = global(cx) else {
        return;
    };
    let project_id = source.project_id().to_string();
    let Some(key) = area
        .read(cx)
        .document_key_for_source(&expected_worktree_id, &source)
    else {
        return;
    };
    area.update(cx, |area, cx| {
        area.request_close_document(project_id, key, window, cx)
    });
}

/// Unified entry for existing navigation surfaces that explicitly jump to a
/// terminal pane (toast, session list, title bar, tray). Activating a hidden
/// pane without switching the workbench page would leave focus in an invisible
/// PTY while the document page remains on screen.
pub fn activate_terminal_page(window: &mut Window, cx: &mut App) -> bool {
    let Some(area) = global(cx) else {
        return false;
    };
    area.update(cx, |area, cx| {
        area.activate_terminal(window, cx);
        area.is_terminal_active(cx)
    })
}

/// 文档异步读盘完成时用来判断是否可以接管焦点。后台页签或其它项目的迟到结果
/// 只能更新自身内容，不能把键盘焦点从当前终端/文档页抢走。
pub fn is_document_active(
    expected_worktree_id: &WorktreeId,
    source: &DocumentSource,
    cx: &App,
) -> bool {
    let Some(area) = global(cx) else {
        return false;
    };
    let area = area.read(cx);
    let Some(key) = area.document_key_for_source(expected_worktree_id, source) else {
        return false;
    };
    area.document_page_is_active(&key, cx)
}

/// Restore focus after a modal overlay closes. Callers capture both the
/// compatibility project and stable worktree before yielding; the workbench
/// rejects the handoff if either binding changed.
pub fn reactivate_active_document(
    expected_project_id: &str,
    expected_worktree_id: &WorktreeId,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(area) = global(cx) else {
        return;
    };
    area.update(cx, |area, cx| {
        area.reactivate_active_document(expected_project_id, expected_worktree_id, window, cx);
    });
}

/// Restore the active workbench page after switching project/worktree scope.
/// Each stable worktree keeps its own terminal/document route, so the
/// handoff must focus that route instead of leaving focus in the hidden source.
pub fn reactivate_active_page(
    expected_project_id: &str,
    expected_worktree_id: &WorktreeId,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(area) = global(cx) else {
        return;
    };
    area.update(cx, |area, cx| {
        area.reactivate_active_page(expected_project_id, expected_worktree_id, window, cx);
    });
}

/// 文件页签宿主。
pub struct WorkbenchArea {
    store: Entity<AppStore>,
    terminal_area: Entity<TerminalArea>,
    worktrees: HashMap<WorktreeId, WorktreeDocuments>,
    last_rendered_project: Option<String>,
    last_rendered_worktree: Option<WorktreeId>,
    last_rendered_page: Option<WorkbenchPage>,
}

impl WorkbenchArea {
    pub fn new(
        store: Entity<AppStore>,
        terminal_area: Entity<TerminalArea>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&store, |this, store, cx| {
            let project_bindings = {
                let store = store.read(cx);
                store
                    .projects()
                    .iter()
                    .filter_map(|project| {
                        store
                            .worktree_id_for_project(&project.id)
                            .cloned()
                            .map(|worktree_id| (project.id.clone(), worktree_id))
                    })
                    .collect::<HashMap<_, _>>()
            };
            for documents in this.worktrees.values_mut() {
                let stale_clean_keys = documents
                    .tabs
                    .iter()
                    .filter(|tab| {
                        !source_project_binding_matches(
                            &project_bindings,
                            &tab.source_project_id,
                            &tab.key.worktree_id,
                        ) && !tab.document.read(cx).is_dirty()
                    })
                    .map(|tab| tab.key.clone())
                    .collect::<Vec<_>>();
                for key in stale_clean_keys {
                    documents.close(&key);
                }
                let stale_work_items = documents
                    .work_items
                    .iter()
                    .filter(|tab| {
                        !source_project_binding_matches(
                            &project_bindings,
                            &tab.source_project_id,
                            &tab.key.worktree_id,
                        )
                    })
                    .map(|tab| tab.key.clone())
                    .collect::<Vec<_>>();
                for key in stale_work_items {
                    documents.close_work_item(&key);
                }
            }
            let worktree_ids = project_bindings
                .values()
                .cloned()
                .collect::<std::collections::HashSet<_>>();
            // 正常移除入口会在 AppStore 层拒绝丢弃脏页签；这里再留一道兜底，
            // 防止配置被其它路径直接改写时观察者静默销毁内存草稿。
            this.worktrees.retain(|worktree_id, documents| {
                worktree_ids.contains(worktree_id) || worktree_documents_are_dirty(documents, cx)
            });
            for documents in this.worktrees.values() {
                for tab in &documents.tabs {
                    tab.document
                        .update(cx, |document, cx| document.validate_remote_source(cx));
                }
            }
            cx.notify();
        })
        .detach();
        Self {
            store,
            terminal_area,
            worktrees: HashMap::new(),
            last_rendered_project: None,
            last_rendered_worktree: None,
            last_rendered_page: None,
        }
    }

    pub fn is_terminal_active(&self, cx: &App) -> bool {
        let scope = {
            let store = self.store.read(cx);
            let Some(project_id) = store.active_project_id.clone() else {
                return true;
            };
            let Some(worktree_id) = store.active_worktree_id().cloned() else {
                return true;
            };
            (project_id, worktree_id)
        };
        self.is_terminal_page_for_scope(&scope.0, &scope.1, cx)
    }

    fn project_binding_is_current(
        &self,
        project_id: &str,
        expected_worktree_id: &WorktreeId,
        cx: &App,
    ) -> bool {
        let store = self.store.read(cx);
        project_binding_matches(
            store.worktree_id_for_project(project_id),
            expected_worktree_id,
        )
    }

    fn active_worktree_is_current(&self, expected_worktree_id: &WorktreeId, cx: &App) -> bool {
        let store = self.store.read(cx);
        let Some(active_project_id) = store.active_project_id.as_deref() else {
            return false;
        };
        active_worktree_matches(
            store.active_worktree_id(),
            store.worktree_id_for_project(active_project_id),
            expected_worktree_id,
        )
    }

    fn active_scope_is_current(
        &self,
        expected_project_id: &str,
        expected_worktree_id: &WorktreeId,
        cx: &App,
    ) -> bool {
        let store = self.store.read(cx);
        active_scope_matches(
            store.active_project_id.as_deref(),
            store.active_worktree_id(),
            store.worktree_id_for_project(expected_project_id),
            expected_project_id,
            expected_worktree_id,
        )
    }

    fn document_key_for_source(
        &self,
        expected_worktree_id: &WorktreeId,
        source: &DocumentSource,
    ) -> Option<DocumentKey> {
        let source_key = DocumentSourceKey::from_source(source);
        self.worktrees
            .get(expected_worktree_id)?
            .tabs
            .iter()
            .filter(|tab| {
                tab.source_project_id == source_key.project_id
                    && tab.key.matches_source_key(&source_key)
            })
            .map(|tab| tab.key.clone())
            .next()
    }

    fn document_binding_is_current(&self, key: &DocumentKey, cx: &App) -> bool {
        let Some(tab) = self
            .worktrees
            .get(&key.worktree_id)
            .and_then(|documents| documents.index_of(key).map(|index| &documents.tabs[index]))
        else {
            return false;
        };
        self.project_binding_is_current(&tab.source_project_id, &key.worktree_id, cx)
    }

    fn document_page_is_active(&self, key: &DocumentKey, cx: &App) -> bool {
        self.document_binding_is_current(key, cx)
            && self.active_worktree_is_current(&key.worktree_id, cx)
            && self
                .worktrees
                .get(&key.worktree_id)
                .is_some_and(|documents| documents.active == WorkbenchPage::Document(key.clone()))
    }

    fn work_item_binding_is_current(&self, key: &GitHubWorkItemTabKey, cx: &App) -> bool {
        let Some(tab) = self.worktrees.get(&key.worktree_id).and_then(|documents| {
            documents
                .work_item_index_of(key)
                .map(|index| &documents.work_items[index])
        }) else {
            return false;
        };
        self.project_binding_is_current(&tab.source_project_id, &key.worktree_id, cx)
    }

    fn is_terminal_page_for_scope(
        &self,
        project_id: &str,
        worktree_id: &WorktreeId,
        cx: &App,
    ) -> bool {
        if !self.active_scope_is_current(project_id, worktree_id, cx) {
            return false;
        }
        self.worktrees
            .get(worktree_id)
            .is_none_or(|documents| documents.active == WorkbenchPage::Terminal)
    }

    fn open_document(
        &mut self,
        worktree_id: WorktreeId,
        source: DocumentSource,
        highlight_line: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let project_id = source.project_id().to_string();
        if !self.project_binding_is_current(&project_id, &worktree_id, cx)
            || !self.active_worktree_is_current(&worktree_id, cx)
        {
            return;
        }

        let key = DocumentKey::from_source(worktree_id.clone(), &source);
        let documents = self.worktrees.entry(worktree_id).or_default();
        if let Some(index) = documents.index_of(&key) {
            let document = documents.tabs[index].document.clone();
            documents.active = WorkbenchPage::Document(key.clone());
            let area = cx.entity();
            window.defer(cx, move |window, cx| {
                let should_activate = area.read(cx).document_page_is_active(&key, cx);
                document.update(cx, |document, cx| {
                    document.reveal_line(highlight_line, window, cx);
                    if should_activate {
                        document.on_activated(window, cx);
                    }
                });
            });
            cx.notify();
            return;
        }

        let title = source.file_name();
        let document = cx.new(|cx| {
            FileViewer::new_document(key.worktree_id.clone(), source, highlight_line, window, cx)
        });
        let observed_key = key.clone();
        cx.observe(&document, move |this, document, cx| {
            if document.read(cx).is_dirty() {
                this.promote_document(&observed_key);
            }
            cx.notify();
        })
        .detach();
        documents.insert_preview(
            DocumentTab {
                key: key.clone(),
                source_project_id: project_id,
                title,
                document: document.clone(),
                state: DocumentTabState::Preview,
            },
            cx,
        );
        documents.active = WorkbenchPage::Document(key.clone());
        let area = cx.entity();
        window.defer(cx, move |window, cx| {
            if area.read(cx).document_page_is_active(&key, cx) {
                document.update(cx, |document, cx| document.on_activated(window, cx));
            }
        });
        cx.notify();
    }

    fn open_work_item(
        &mut self,
        service: Entity<GitHubTaskService>,
        request: OpenGitHubWorkItem,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let project_id = request.project_id.clone();
        let worktree_id = request.worktree_id.clone();
        if !self.active_scope_is_current(&project_id, &worktree_id, cx) {
            return;
        }
        let key = request.tab_key();
        let title = format!(
            "{} #{}",
            request.summary.kind.short_label(),
            request.summary.number
        );
        let existing = self
            .worktrees
            .get(&worktree_id)
            .and_then(|documents| documents.work_item_index_of(&key));
        if let Some(index) = existing {
            let matches = self.worktrees[&worktree_id].work_items[index]
                .viewer
                .read(cx)
                .matches_request(&request);
            if matches {
                service.update(cx, |service, cx| service.ensure_detail(request.clone(), cx));
                let documents = self
                    .worktrees
                    .get_mut(&worktree_id)
                    .expect("worktree exists");
                documents.active = WorkbenchPage::WorkItem(key);
                cx.notify();
                return;
            }
            let documents = self
                .worktrees
                .get_mut(&worktree_id)
                .expect("worktree exists");
            let state = documents.work_items[index].state;
            let viewer = cx.new(|cx| GitHubWorkItemViewer::new(service, request, cx));
            documents.work_items[index] = WorkItemTab {
                key: key.clone(),
                source_project_id: project_id,
                title,
                viewer,
                state,
            };
            documents.active = WorkbenchPage::WorkItem(key);
            cx.notify();
            return;
        }

        let viewer = cx.new(|cx| GitHubWorkItemViewer::new(service, request, cx));
        let documents = self.worktrees.entry(worktree_id).or_default();
        documents.insert_work_item_preview(WorkItemTab {
            key: key.clone(),
            source_project_id: project_id,
            title,
            viewer,
            state: DocumentTabState::Preview,
        });
        documents.active = WorkbenchPage::WorkItem(key);
        cx.notify();
    }

    pub fn activate_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let scope = {
            let store = self.store.read(cx);
            let Some(project_id) = store.active_project_id.clone() else {
                return;
            };
            let Some(worktree_id) = store.active_worktree_id().cloned() else {
                return;
            };
            (project_id, worktree_id)
        };
        self.activate_terminal_for_scope(&scope.0, &scope.1, window, cx);
    }

    fn activate_terminal_for_scope(
        &mut self,
        project_id: &str,
        worktree_id: &WorktreeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_scope_is_current(project_id, worktree_id, cx) {
            return;
        }
        self.worktrees
            .entry(worktree_id.clone())
            .or_default()
            .active = WorkbenchPage::Terminal;
        let pane_id = self.store.read(cx).active_pane_id(project_id);
        if let Some(pane_id) = pane_id {
            self.store.update(cx, |store, cx| {
                store.focus_pane(project_id, &pane_id, window, cx)
            });
        }
        cx.notify();
    }

    fn promote_document(&mut self, key: &DocumentKey) -> bool {
        let Some(documents) = self.worktrees.get_mut(&key.worktree_id) else {
            return false;
        };
        documents.promote(key)
    }

    fn promote_work_item(&mut self, key: &GitHubWorkItemTabKey) -> bool {
        let Some(documents) = self.worktrees.get_mut(&key.worktree_id) else {
            return false;
        };
        documents.promote_work_item(key)
    }

    fn activate_work_item(
        &mut self,
        project_id: &str,
        key: &GitHubWorkItemTabKey,
        cx: &mut Context<Self>,
    ) {
        if !self.active_scope_is_current(project_id, &key.worktree_id, cx)
            || !self.work_item_binding_is_current(key, cx)
        {
            return;
        }
        let Some(documents) = self.worktrees.get_mut(&key.worktree_id) else {
            return;
        };
        if documents.work_item_index_of(key).is_none() {
            return;
        }
        documents.active = WorkbenchPage::WorkItem(key.clone());
        cx.notify();
    }

    fn activate_document(
        &mut self,
        project_id: &str,
        key: &DocumentKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_scope_is_current(project_id, &key.worktree_id, cx)
            || !self.document_binding_is_current(key, cx)
        {
            return;
        }
        let Some(documents) = self.worktrees.get_mut(&key.worktree_id) else {
            return;
        };
        let Some(index) = documents.index_of(key) else {
            return;
        };
        let document = documents.tabs[index].document.clone();
        documents.active = WorkbenchPage::Document(key.clone());
        let area = cx.entity();
        let key = key.clone();
        window.defer(cx, move |window, cx| {
            if area.read(cx).document_page_is_active(&key, cx) {
                document.update(cx, |document, cx| {
                    document.on_activated(window, cx);
                });
            }
        });
        cx.notify();
    }

    fn reactivate_active_document(
        &mut self,
        expected_project_id: &str,
        expected_worktree_id: &WorktreeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_scope_is_current(expected_project_id, expected_worktree_id, cx) {
            return;
        }
        let Some(WorkbenchPage::Document(key)) = self
            .worktrees
            .get(expected_worktree_id)
            .map(|documents| documents.active.clone())
        else {
            return;
        };
        self.activate_document(expected_project_id, &key, window, cx);
    }

    fn reactivate_active_page(
        &mut self,
        expected_project_id: &str,
        expected_worktree_id: &WorktreeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_scope_is_current(expected_project_id, expected_worktree_id, cx) {
            return;
        }
        let page = self
            .worktrees
            .get(expected_worktree_id)
            .map(|documents| documents.active.clone())
            .unwrap_or(WorkbenchPage::Terminal);
        match page {
            WorkbenchPage::Terminal => self.activate_terminal_for_scope(
                expected_project_id,
                expected_worktree_id,
                window,
                cx,
            ),
            WorkbenchPage::Document(key) => {
                self.activate_document(expected_project_id, &key, window, cx)
            }
            WorkbenchPage::WorkItem(key) => self.activate_work_item(expected_project_id, &key, cx),
        }
    }

    pub fn search_active_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let scope = {
            let store = self.store.read(cx);
            let Some(project_id) = store.active_project_id.clone() else {
                return;
            };
            let Some(worktree_id) = store.active_worktree_id().cloned() else {
                return;
            };
            (project_id, worktree_id)
        };
        if !self.active_scope_is_current(&scope.0, &scope.1, cx) {
            return;
        }
        let Some(WorkbenchPage::Document(key)) = self
            .worktrees
            .get(&scope.1)
            .map(|documents| documents.active.clone())
        else {
            return;
        };
        if !self.document_page_is_active(&key, cx) {
            return;
        }
        let Some(document) = self.worktrees.get(&scope.1).and_then(|documents| {
            documents
                .index_of(&key)
                .map(|index| documents.tabs[index].document.clone())
        }) else {
            return;
        };
        document.update(cx, |document, cx| document.open_search(window, cx));
    }

    fn request_close_document(
        &mut self,
        project_id: String,
        key: DocumentKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.project_binding_is_current(&project_id, &key.worktree_id, cx) {
            return;
        }
        let dirty = self
            .worktrees
            .get(&key.worktree_id)
            .and_then(|documents| documents.index_of(&key).map(|index| &documents.tabs[index]))
            .is_some_and(|tab| tab.document.read(cx).is_dirty());
        if !dirty {
            self.close_document(&project_id, &key, window, cx);
            return;
        }

        let this = cx.entity();
        Confirm::new(
            t("fileViewer", "unsavedTitle"),
            t("fileViewer", "unsavedMessage"),
        )
        .open(
            move |window, cx| {
                let this = this.clone();
                let project_id = project_id.clone();
                let key = key.clone();
                window.defer(cx, move |window, cx| {
                    this.update(cx, |area, cx| {
                        area.close_document(&project_id, &key, window, cx)
                    });
                });
            },
            window,
            cx,
        );
    }

    fn close_document(
        &mut self,
        project_id: &str,
        key: &DocumentKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.project_binding_is_current(project_id, &key.worktree_id, cx) {
            return;
        }
        let (was_active, next_page) = {
            let Some(documents) = self.worktrees.get_mut(&key.worktree_id) else {
                return;
            };
            let was_active = documents.active == WorkbenchPage::Document(key.clone());
            if documents.close(key).is_none() {
                return;
            }
            (was_active, documents.active.clone())
        };
        let active_project_id = {
            let store = self.store.read(cx);
            store
                .active_project_id
                .as_deref()
                .and_then(|active_project_id| {
                    active_worktree_matches(
                        store.active_worktree_id(),
                        store.worktree_id_for_project(active_project_id),
                        &key.worktree_id,
                    )
                    .then(|| active_project_id.to_string())
                })
        };
        if !was_active {
            cx.notify();
            return;
        }
        let Some(active_project_id) = active_project_id else {
            cx.notify();
            return;
        };
        match next_page {
            WorkbenchPage::Terminal => {
                self.activate_terminal_for_scope(&active_project_id, &key.worktree_id, window, cx)
            }
            WorkbenchPage::Document(next) => {
                self.activate_document(&active_project_id, &next, window, cx)
            }
            WorkbenchPage::WorkItem(next) => self.activate_work_item(&active_project_id, &next, cx),
        }
        cx.notify();
    }

    fn close_work_item(
        &mut self,
        project_id: &str,
        key: &GitHubWorkItemTabKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.project_binding_is_current(project_id, &key.worktree_id, cx) {
            return;
        }
        let (was_active, next_page) = {
            let Some(documents) = self.worktrees.get_mut(&key.worktree_id) else {
                return;
            };
            let was_active = documents.active == WorkbenchPage::WorkItem(key.clone());
            if documents.close_work_item(key).is_none() {
                return;
            }
            (was_active, documents.active.clone())
        };
        if !was_active {
            cx.notify();
            return;
        }
        let active_project_id = {
            let store = self.store.read(cx);
            store
                .active_project_id
                .as_deref()
                .and_then(|active_project_id| {
                    active_worktree_matches(
                        store.active_worktree_id(),
                        store.worktree_id_for_project(active_project_id),
                        &key.worktree_id,
                    )
                    .then(|| active_project_id.to_string())
                })
        };
        let Some(active_project_id) = active_project_id else {
            cx.notify();
            return;
        };
        match next_page {
            WorkbenchPage::Terminal => {
                self.activate_terminal_for_scope(&active_project_id, &key.worktree_id, window, cx)
            }
            WorkbenchPage::Document(next) => {
                self.activate_document(&active_project_id, &next, window, cx)
            }
            WorkbenchPage::WorkItem(next) => self.activate_work_item(&active_project_id, &next, cx),
        }
        cx.notify();
    }

    fn active_snapshot(&self, cx: &App) -> Option<ActiveWorkbenchSnapshot> {
        let (project_id, worktree_id) = {
            let store = self.store.read(cx);
            let project_id = store.active_project_id.clone()?;
            let worktree_id = store.active_worktree_id().cloned()?;
            if !active_scope_matches(
                store.active_project_id.as_deref(),
                store.active_worktree_id(),
                store.worktree_id_for_project(&project_id),
                &project_id,
                &worktree_id,
            ) {
                return None;
            }
            (project_id, worktree_id)
        };
        let documents = self.worktrees.get(&worktree_id);
        let active = documents
            .map(|documents| documents.active.clone())
            .unwrap_or(WorkbenchPage::Terminal);
        let tabs = documents
            .map(|documents| {
                documents
                    .tabs
                    .iter()
                    .map(|tab| {
                        (
                            tab.key.clone(),
                            tab.title.clone(),
                            tab.document.clone(),
                            tab.state,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let work_items = documents
            .map(|documents| {
                documents
                    .work_items
                    .iter()
                    .map(|tab| {
                        (
                            tab.key.clone(),
                            tab.title.clone(),
                            tab.viewer.clone(),
                            tab.state,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some((project_id, worktree_id, active, tabs, work_items))
    }
}

impl Render for WorkbenchArea {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((project_id, worktree_id, active, tabs, work_items)) = self.active_snapshot(cx)
        else {
            self.last_rendered_project = None;
            self.last_rendered_worktree = None;
            self.last_rendered_page = None;
            return div().size_full().child(self.terminal_area.clone());
        };

        let page_changed = self.last_rendered_project.as_deref() != Some(project_id.as_str())
            || self.last_rendered_worktree.as_ref() != Some(&worktree_id)
            || self.last_rendered_page.as_ref() != Some(&active);
        if page_changed {
            self.last_rendered_project = Some(project_id.clone());
            self.last_rendered_worktree = Some(worktree_id.clone());
            self.last_rendered_page = Some(active.clone());
            if active != WorkbenchPage::Terminal {
                self.terminal_area
                    .update(cx, |area, cx| area.suspend(window, cx));
            }
            match &active {
                WorkbenchPage::Terminal => {
                    let area = cx.entity();
                    let store = self.store.clone();
                    let project_id = project_id.clone();
                    let worktree_id = worktree_id.clone();
                    window.defer(cx, move |window, cx| {
                        if !area
                            .read(cx)
                            .is_terminal_page_for_scope(&project_id, &worktree_id, cx)
                            || window.has_active_dialog(cx)
                            || !crate::overlay::allows(crate::overlay::Yield::ToOverlay)
                        {
                            return;
                        }
                        let pane_id = store.read(cx).active_pane_id(&project_id);
                        if let Some(pane_id) = pane_id {
                            store.update(cx, |store, cx| {
                                store.focus_pane(&project_id, &pane_id, window, cx)
                            });
                        }
                    });
                }
                WorkbenchPage::Document(key) => {
                    if let Some((_, _, document, _)) =
                        tabs.iter().find(|(candidate, _, _, _)| candidate == key)
                    {
                        let area = cx.entity();
                        let key = key.clone();
                        let document = document.clone();
                        window.defer(cx, move |window, cx| {
                            if area.read(cx).document_page_is_active(&key, cx) {
                                document.update(cx, |document, cx| {
                                    document.on_activated(window, cx);
                                });
                            }
                        });
                    }
                }
                WorkbenchPage::WorkItem(_) => {}
            }
        }

        // 尚未打开文件或工作项时保持原终端区的尺寸与结构。
        if tabs.is_empty() && work_items.is_empty() {
            return div().size_full().child(self.terminal_area.clone());
        }

        let mut tab_bar = div()
            .id("workbench-tabs")
            .h(px(34.0))
            .flex_none()
            .flex()
            .items_center()
            .overflow_x_scroll()
            .bg(ui::bg_elevated())
            .border_b_1()
            .border_color(ui::border_subtle());

        for (key, title, document, state) in &tabs {
            let selected = active == WorkbenchPage::Document(key.clone());
            let dirty = document.read(cx).is_dirty();
            let preview = *state == DocumentTabState::Preview;
            let tab_key = key.clone();
            let close_key = key.clone();
            let click_project = project_id.clone();
            let close_project = project_id.clone();
            tab_bar = tab_bar.child(
                div()
                    .id(SharedString::from(format!(
                        "workbench-tab-{:016x}",
                        stable_hash(key)
                    )))
                    .h_full()
                    .min_w(px(120.0))
                    .max_w(px(220.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .cursor_pointer()
                    .text_size(ui::font_px(12.0))
                    .when(selected, |el| {
                        // 与文档页容器同色:背景图皮肤下随内容区一起半透明,
                        // 不透明的 bg_base 会在半透明页签条上凸成一块实色
                        el.bg(ui::bg_document())
                            .text_color(ui::text_primary())
                            .border_t_2()
                            .border_color(ui::accent())
                    })
                    .when(!selected, |el| {
                        el.text_color(ui::text_muted())
                            .border_t_2()
                            .border_color(gpui::Hsla {
                                a: 0.0,
                                ..ui::accent()
                            })
                    })
                    .child(FileIcon::new(title, false, false).size(px(14.0)))
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_1()
                            .truncate()
                            .when(preview, |el| el.opacity(0.76))
                            .child(title.clone()),
                    )
                    .when(dirty, |el| {
                        el.child(
                            div()
                                .id(SharedString::from(format!(
                                    "workbench-tab-dirty-{:016x}",
                                    stable_hash(key)
                                )))
                                .w(px(6.0))
                                .h(px(6.0))
                                .flex_none()
                                .rounded_full()
                                .bg(ui::accent())
                                .tooltip(|window, cx| {
                                    Tooltip::new(t("fileViewer", "unsaved")).build(window, cx)
                                }),
                        )
                    })
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "workbench-tab-close-{:016x}",
                                stable_hash(key)
                            )))
                            .w(px(18.0))
                            .h(px(18.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(3.0))
                            .text_color(ui::text_muted())
                            .hover(|el| el.bg(ui::border_subtle()).text_color(ui::text_primary()))
                            .child("×")
                            .on_click(cx.listener(move |this, _event, window, cx| {
                                cx.stop_propagation();
                                this.request_close_document(
                                    close_project.clone(),
                                    close_key.clone(),
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .on_click(
                        cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                            if event.click_count() >= 2 {
                                this.promote_document(&tab_key);
                            }
                            this.activate_document(&click_project, &tab_key, window, cx)
                        }),
                    ),
            );
        }

        for (key, title, _viewer, state) in &work_items {
            let selected = active == WorkbenchPage::WorkItem(key.clone());
            let preview = *state == DocumentTabState::Preview;
            let tab_key = key.clone();
            let close_key = key.clone();
            let click_project = project_id.clone();
            let close_project = project_id.clone();
            tab_bar = tab_bar.child(
                div()
                    .id(SharedString::from(format!(
                        "workbench-github-tab-{:016x}",
                        stable_hash(key)
                    )))
                    .h_full()
                    .min_w(px(120.0))
                    .max_w(px(220.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .cursor_pointer()
                    .text_size(ui::font_px(12.0))
                    .when(selected, |el| {
                        el.bg(ui::bg_document())
                            .text_color(ui::text_primary())
                            .border_t_2()
                            .border_color(ui::accent())
                    })
                    .when(!selected, |el| {
                        el.text_color(ui::text_muted())
                            .border_t_2()
                            .border_color(gpui::Hsla {
                                a: 0.0,
                                ..ui::accent()
                            })
                    })
                    .child(
                        div()
                            .flex_none()
                            .text_size(ui::font_px(10.0))
                            .text_color(ui::color_success())
                            .child("#"),
                    )
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_1()
                            .truncate()
                            .when(preview, |el| el.opacity(0.76))
                            .child(title.clone()),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "workbench-github-tab-close-{:016x}",
                                stable_hash(key)
                            )))
                            .w(px(18.0))
                            .h(px(18.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(3.0))
                            .text_color(ui::text_muted())
                            .hover(|el| el.bg(ui::border_subtle()).text_color(ui::text_primary()))
                            .child("×")
                            .on_click(cx.listener(move |this, _event, window, cx| {
                                cx.stop_propagation();
                                this.close_work_item(&close_project, &close_key, window, cx);
                            })),
                    )
                    .on_click(
                        cx.listener(move |this, event: &gpui::ClickEvent, _window, cx| {
                            if event.click_count() >= 2 {
                                this.promote_work_item(&tab_key);
                            }
                            this.activate_work_item(&click_project, &tab_key, cx);
                        }),
                    ),
            );
        }

        let body = match &active {
            WorkbenchPage::Terminal => self.terminal_area.clone().into_any_element(),
            WorkbenchPage::Document(key) => tabs
                .iter()
                .find(|(candidate, _, _, _)| candidate == key)
                .map(|(_, _, document, _)| document.clone().into_any_element())
                .unwrap_or_else(|| self.terminal_area.clone().into_any_element()),
            WorkbenchPage::WorkItem(key) => work_items
                .iter()
                .find(|(candidate, _, _, _)| candidate == key)
                .map(|(_, _, viewer, _)| viewer.clone().into_any_element())
                .unwrap_or_else(|| self.terminal_area.clone().into_any_element()),
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(tab_bar)
            .child(div().flex_1().min_h(px(0.0)).overflow_hidden().child(body))
    }
}

fn stable_hash<T: Hash>(key: &T) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct TestTab {
        name: &'static str,
        state: DocumentTabState,
        dirty: bool,
    }

    impl TestTab {
        fn permanent(name: &'static str) -> Self {
            Self {
                name,
                state: DocumentTabState::Permanent,
                dirty: false,
            }
        }

        fn preview(name: &'static str, dirty: bool) -> Self {
            Self {
                name,
                state: DocumentTabState::Preview,
                dirty,
            }
        }
    }

    fn insert_test_preview(tabs: &mut Vec<TestTab>, tab: TestTab) -> usize {
        insert_preview_tab(
            tabs,
            tab,
            |tab| tab.state,
            |tab| tab.dirty,
            |tab| tab.state.promote(),
        )
    }

    fn worktree_id(seed: char) -> WorktreeId {
        format!("worktree-v1:{}", seed.to_string().repeat(64))
            .parse()
            .expect("valid test worktree id")
    }

    fn key_in(worktree_id: WorktreeId, path: &str) -> DocumentKey {
        DocumentKey {
            worktree_id,
            backend: DocumentBackendKey::Local,
            normalized_path: path.into(),
        }
    }

    fn key(path: &str) -> DocumentKey {
        key_in(worktree_id('a'), path)
    }

    #[test]
    fn document_key_separates_worktree_identity() {
        assert_ne!(
            key_in(worktree_id('a'), "/work/a.rs"),
            key_in(worktree_id('b'), "/work/a.rs")
        );
    }

    #[test]
    fn compatibility_project_aliases_share_one_worktree_document_key() {
        let source = |project_id: &str| DocumentSource::Local {
            project_id: project_id.to_string(),
            project_root: PathBuf::from("/work"),
            path: PathBuf::from("/work/a.rs"),
        };
        let worktree_id = worktree_id('a');

        assert_eq!(
            DocumentKey::from_source(worktree_id.clone(), &source("project-a")),
            DocumentKey::from_source(worktree_id, &source("project-b")),
        );
    }

    #[test]
    fn remaining_alias_does_not_validate_removed_document_source_project() {
        let worktree_id = worktree_id('a');
        let project_bindings = HashMap::from([("project-b".to_string(), worktree_id.clone())]);

        assert!(!source_project_binding_matches(
            &project_bindings,
            "project-a",
            &worktree_id,
        ));
        assert!(source_project_binding_matches(
            &project_bindings,
            "project-b",
            &worktree_id,
        ));
    }

    #[test]
    fn document_key_separates_remote_connection_identity() {
        let a = DocumentKey {
            worktree_id: worktree_id('a'),
            backend: DocumentBackendKey::Remote {
                connection_id: "ssh".into(),
                connection_fingerprint: 1,
            },
            normalized_path: "/work/a.rs".into(),
        };
        let mut b = a.clone();
        b.backend = DocumentBackendKey::Remote {
            connection_id: "ssh".into(),
            connection_fingerprint: 2,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn stale_callback_rejects_same_project_rebound_to_another_worktree() {
        let original = worktree_id('a');
        let rebound = worktree_id('b');

        assert!(active_scope_matches(
            Some("p"),
            Some(&original),
            Some(&original),
            "p",
            &original,
        ));
        assert!(!active_scope_matches(
            Some("p"),
            Some(&rebound),
            Some(&rebound),
            "p",
            &original,
        ));
        assert!(!active_scope_matches(
            Some("other-project"),
            Some(&original),
            Some(&original),
            "p",
            &original,
        ));
        assert!(!project_binding_matches(Some(&rebound), &original));
    }

    #[test]
    fn active_worktree_requires_active_project_binding_to_match() {
        let expected = worktree_id('a');
        let other = worktree_id('b');

        assert!(active_worktree_matches(
            Some(&expected),
            Some(&expected),
            &expected,
        ));
        assert!(!active_worktree_matches(
            Some(&expected),
            Some(&other),
            &expected,
        ));
    }

    #[cfg(windows)]
    #[test]
    fn normalized_paths_deduplicate_separator_variants_on_windows() {
        let normalized = normalize_local_document_path(Path::new("C:\\Work\\src\\main.rs"));
        assert!(!normalized.contains('\\'));
    }

    #[cfg(not(windows))]
    #[test]
    fn local_posix_paths_preserve_backslash_file_names() {
        assert_ne!(
            normalize_local_document_path(Path::new("/work/a\\b.rs")),
            normalize_local_document_path(Path::new("/work/a/b.rs"))
        );
    }

    #[test]
    fn remote_paths_remain_case_sensitive_on_windows_hosts() {
        assert_ne!(
            normalize_remote_document_path(Path::new("/work/A.rs")),
            normalize_remote_document_path(Path::new("/work/a.rs"))
        );
        assert_ne!(
            normalize_remote_document_path(Path::new("/work/a\\b.rs")),
            normalize_remote_document_path(Path::new("/work/a/b.rs"))
        );
    }

    #[test]
    fn opening_new_document_replaces_clean_preview_at_same_index() {
        let mut tabs = vec![
            TestTab::permanent("a"),
            TestTab::preview("b", false),
            TestTab::permanent("c"),
        ];

        let inserted_at = insert_test_preview(&mut tabs, TestTab::preview("next", false));

        assert_eq!(inserted_at, 1);
        assert_eq!(
            tabs.iter().map(|tab| tab.name).collect::<Vec<_>>(),
            vec!["a", "next", "c"]
        );
        assert_eq!(tabs[1].state, DocumentTabState::Preview);
    }

    #[test]
    fn dirty_preview_is_promoted_before_new_preview_is_appended() {
        let mut tabs = vec![TestTab::permanent("a"), TestTab::preview("b", true)];

        let inserted_at = insert_test_preview(&mut tabs, TestTab::preview("next", false));

        assert_eq!(inserted_at, 2);
        assert_eq!(tabs[1].state, DocumentTabState::Permanent);
        assert_eq!(tabs[2].state, DocumentTabState::Preview);
    }

    #[test]
    fn preview_replacement_is_isolated_by_worktree_bucket() {
        let first = worktree_id('a');
        let second = worktree_id('b');
        let mut worktrees = HashMap::from([
            (first.clone(), vec![TestTab::preview("first", false)]),
            (second.clone(), vec![TestTab::preview("second", false)]),
        ]);

        insert_test_preview(
            worktrees.get_mut(&first).unwrap(),
            TestTab::preview("next", false),
        );

        assert_eq!(worktrees[&first][0].name, "next");
        assert_eq!(worktrees[&second][0].name, "second");
    }

    #[test]
    fn closing_active_document_prefers_right_neighbor_then_left() {
        assert_eq!(
            next_document_after_close(&[key("a"), key("c")], 1),
            Some(key("c"))
        );
        assert_eq!(
            next_document_after_close(&[key("a"), key("b")], 2),
            Some(key("b"))
        );
        assert_eq!(next_document_after_close(&[], 0), None);
    }
}
