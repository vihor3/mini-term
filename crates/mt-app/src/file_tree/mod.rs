//! 中栏:文件树。对应 `src/components/FileTree.tsx` 的主干。
//!
//! - Listing dispatch captures the backend and source before leaving the UI.
//!   Remote reads use [`crate::remote_ssh::list_directory_at_epoch`]; local reads
//!   use [`mt_project::fs::list_directory`](mt_project::fs::list_directory).
//!   (`.gitignore` 过滤与排序都在那边,这里不重复实现),SSH 远程项目走 SFTP
//!   readdir。两条路返回同一个 `FileEntry`,所以整棵树共用同一段加载代码,
//!   不会出现「树顶刷新走了本地、展开子目录走了远程」这类半截状态。
//!   两条都是**阻塞**函数,一律丢 background executor,不能在主线程上跑;
//!
//! # SSH 远程项目的四条差异(逐条对照 `FileTree.tsx:432-508`)
//!
//! 1. **不注册 notify watcher**(远端文件系统本机监听不到);
//! 2. **不拉 git 状态**(远程 Git 是二期);
//! 3. **不做单链目录压缩** —— 逐级 SFTP 往返太贵,原版原话「保持原样」;
//! 4. **不探子工程技术栈**(`ensure_dir_kinds` 是本机 `stat`)。
//!
//! 断链(连接被删)不去读本机同名路径:直接给
//! `fileTree.remote.broken` 那句明确错误(项目仍可见、可删)。
//! - 目录变化走 [`mt_project::watch::FsWatcher`](mt_project::watch::FsWatcher):
//!   sink 里往 channel 丢,主线程上的前台任务醒来后失效缓存并重列 ——
//!   与 AI 状态、终端重绘是同一套跨线程唤醒模式;
//! - 单击文件开[文件预览器](crate::file_viewer)(AA 批之前是双击调外部编辑器 ——
//!   预览器缺位时的临时替身,原版文件行上只有预览这一条路)。
//!
//! 文件拖进终端(把路径当文本写进 PTY)走 gpui 原生 drag:这边只在行上挂
//! [`on_drag`](gpui::StatefulInteractiveElement::on_drag) 交出
//! [`crate::dnd::DragFilePath`],落点与写入在 `terminal_area.rs`。
//!
//! # git 状态着色(Y 批)
//!
//! 数据是 [`mt_project::git::get_git_status`](mt_project::git::get_git_status)
//! (阻塞,丢后台),键为**以 `/` 分隔的相对路径**,与 `FileTree.tsx:496-507` 同构。
//! 刷新时机照抄原版四条:切项目 / `fs-change`(500ms 去抖)/ 终端里跑过 git 命令
//! (同一个去抖)/ 头部刷新按钮。第三条走 [`crate::git_watch`] 的**输出旁路**,
//! 本模块是它的第二个订阅者([`git_watch::Subscriber::FileTree`])——
//! `isAiPty` 那道闸在旁路里,AI pane 刷屏带不起这边的刷新。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    AnyElement, App, AppContext, Context, DragMoveEvent, Entity, ExternalPaths, Global, Hsla,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Render,
    ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Task, Window, div,
    prelude::FluentBuilder, px,
};
use gpui_component::scroll::Scrollbar;
use mt_identity::WorktreeId;
use mt_project::fs::FileEntry;
use mt_project::watch::{FsChange, FsWatcher};
use mt_ui::icon_tooltip::IconTooltips;
use mt_ui::icons::FileIcon;
use mt_ui::icons::usage_glyphs::ICON_REFRESH;
use mt_ui::icons::vector::VectorIcon;

use crate::file_ops::{FileBackendIdentity, FileClipboardEntry, FileOperationContext};
use crate::git_watch;
use crate::i18n::{t, tr};
use crate::store::{AppStore, orca_worktree_context_enabled};
use crate::ui;

mod menu;
mod ops;
#[cfg(test)]
mod tests;

use menu::{file_menu, open_rename_prompt};
use ops::{start_download, start_upload};

#[derive(Clone)]
enum FileTreeContextEntry {
    Row(Row),
    Blank,
}

#[derive(Clone, Debug, Eq)]
struct FileTreeSource {
    context: FileOperationContext,
    worktree_id: Option<WorktreeId>,
    connection_epoch: Option<u64>,
}

impl PartialEq for FileTreeSource {
    fn eq(&self, other: &Self) -> bool {
        self.same_stable_source(other) && self.context.generation == other.context.generation
    }
}

impl FileTreeSource {
    fn same_stable_source(&self, other: &Self) -> bool {
        same_file_source(&self.context, &other.context)
            && self.worktree_id == other.worktree_id
            && self.connection_epoch == other.connection_epoch
    }

    fn matches(
        &self,
        context: Option<&FileOperationContext>,
        worktree_id: Option<&WorktreeId>,
        connection_epoch: Option<u64>,
    ) -> bool {
        context.is_some_and(|context| {
            same_file_source(&self.context, context) && self.context.generation == context.generation
        }) && self.worktree_id.as_ref() == worktree_id
            && self.connection_epoch == connection_epoch
            && !matches!(&self.context.backend, FileBackendIdentity::BrokenRemote)
    }

    fn is_current(&self, tree: &FileTree, cx: &App) -> bool {
        self.matches(
            tree.operation_context(cx).as_ref(),
            tree.current_worktree.as_ref(),
            tree.source_epoch,
        ) && tree
                .store
                .read(cx)
                .worktree_id_for_project(&self.context.project_id)
                == self.worktree_id.as_ref()
            && file_context_connection_epoch(&self.context) == self.connection_epoch
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DirectoryListingOwner {
    source: FileTreeSource,
    request_id: u64,
}

impl DirectoryListingOwner {
    fn matches(&self, source: &FileTreeSource, request_id: u64) -> bool {
        self.request_id == request_id && self.source == *source
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileTreeOperationOwner {
    id: u64,
    source: FileTreeSource,
    listing: Option<(std::ffi::OsString, u64)>,
    entry: Option<(std::ffi::OsString, bool)>,
}

impl FileTreeOperationOwner {
    fn matches_target(&self, target: &FileTreeContextTarget) -> bool {
        self.source == target.source()
            && self.listing
                == target
                    .listing
                    .as_ref()
                    .map(|(path, id)| (path.as_os_str().to_owned(), *id))
            && self.entry
                == target
                    .row()
                    .map(|row| (row.path.as_os_str().to_owned(), row.is_dir))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileTreeOperationPhase {
    Preflight,
    Choice,
    Running,
}

#[derive(Default)]
struct FileTreeOperations {
    next_id: u64,
    active: Option<(FileTreeOperationOwner, FileTreeOperationPhase)>,
}

impl FileTreeOperations {
    fn reserve(
        &mut self,
        target: &FileTreeContextTarget,
        phase: FileTreeOperationPhase,
    ) -> Option<FileTreeOperationOwner> {
        if self.active.is_some() {
            return None;
        }
        self.next_id = self.next_id.checked_add(1)?;
        let owner = FileTreeOperationOwner {
            id: self.next_id,
            source: target.source(),
            listing: target
                .listing
                .as_ref()
                .map(|(path, id)| (path.as_os_str().to_owned(), *id)),
            entry: target
                .row()
                .map(|row| (row.path.as_os_str().to_owned(), row.is_dir)),
        };
        self.active = Some((owner.clone(), phase));
        Some(owner)
    }

    fn transition(&mut self, owner: &FileTreeOperationOwner, next: FileTreeOperationPhase) -> bool {
        let Some((active, phase)) = self.active.as_mut() else {
            return false;
        };
        let allowed = matches!(
            (*phase, next),
            (FileTreeOperationPhase::Preflight, FileTreeOperationPhase::Choice)
                | (FileTreeOperationPhase::Preflight, FileTreeOperationPhase::Running)
                | (FileTreeOperationPhase::Choice, FileTreeOperationPhase::Running)
        );
        if active != owner || !allowed {
            return false;
        }
        *phase = next;
        true
    }

    fn finish(&mut self, owner: &FileTreeOperationOwner, running: bool) -> bool {
        if !self.active.as_ref().is_some_and(|(active, phase)| {
            active == owner && (*phase == FileTreeOperationPhase::Running) == running
        }) {
            return false;
        }
        self.active = None;
        true
    }
}

#[derive(Clone)]
struct FileTreeContextTarget {
    context: FileOperationContext,
    worktree_id: Option<WorktreeId>,
    connection_epoch: Option<u64>,
    listing: Option<(PathBuf, u64)>,
    entry: FileTreeContextEntry,
}

impl FileTreeContextTarget {
    fn remote_epoch(&self, conn: &mt_config::SshConnection) -> Result<u64, String> {
        if matches!(&self.context.backend, FileBackendIdentity::Remote { connection_id, connection_fingerprint }
            if connection_id == &conn.id && *connection_fingerprint == crate::remote_ssh::connection_fingerprint(conn))
        {
            return self
                .connection_epoch
                .ok_or_else(|| t("fileTree", "operation.sourceUnavailable").to_string());
        }
        Err(t("fileTree", "operation.sourceUnavailable").to_string())
    }

    fn source(&self) -> FileTreeSource {
        FileTreeSource {
            context: self.context.clone(),
            worktree_id: self.worktree_id.clone(),
            connection_epoch: self.connection_epoch,
        }
    }

    fn row(&self) -> Option<&Row> {
        match &self.entry {
            FileTreeContextEntry::Row(row) => Some(row),
            FileTreeContextEntry::Blank => None,
        }
    }

    // This selects a destination, not filesystem authority. The existing I/O
    // helpers still canonicalize parents and enforce root/symlink containment.
    fn directory(&self) -> Option<PathBuf> {
        match &self.entry {
            FileTreeContextEntry::Blank => Some(self.context.root.clone()),
            FileTreeContextEntry::Row(row) if row.is_dir => Some(row.path.clone()),
            FileTreeContextEntry::Row(row) => self.parent_directory(&row.path),
        }
    }

    fn parent_directory(&self, path: &Path) -> Option<PathBuf> {
        match &self.context.backend {
            FileBackendIdentity::Local => path.parent().map(Path::to_path_buf),
            FileBackendIdentity::Remote { .. } => {
                crate::remote_ssh::parent_posix(path.to_str()?).map(PathBuf::from)
            }
            FileBackendIdentity::BrokenRemote => None,
        }
    }

    fn matches_source(
        &self,
        context: Option<&FileOperationContext>,
        worktree_id: Option<&WorktreeId>,
        connection_epoch: Option<u64>,
    ) -> bool {
        self.source().matches(context, worktree_id, connection_epoch)
    }

    fn is_current(&self, tree: &FileTree, cx: &App) -> bool {
        self.source().is_current(tree, cx)
            && self.listing.as_ref().is_some_and(|(directory, request_id)| {
                tree.entry_sources.get(directory).is_some_and(|owner| {
                    owner.matches(&self.source(), *request_id)
                })
            })
    }
}

#[derive(Clone)]
struct FileTreeClipboard {
    entry: FileClipboardEntry,
    owner: FileTreeContextTarget,
}

impl FileTreeClipboard {
    fn can_paste_into(&self, target: &FileTreeContextTarget) -> bool {
        self.entry.can_paste_into(&target.context)
            && self.owner.matches_source(
                Some(&target.context),
                target.worktree_id.as_ref(),
                target.connection_epoch,
            )
    }
}

fn remote_path_text(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| t("fileTree", "operation.invalidTarget").to_string())
}

fn file_context_connection_epoch(context: &FileOperationContext) -> Option<u64> {
    match &context.backend {
        FileBackendIdentity::Remote { connection_id, .. } => {
            crate::remote_ssh::current_connection_epoch(connection_id)
        }
        FileBackendIdentity::Local | FileBackendIdentity::BrokenRemote => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExternalDropTarget {
    hit_id: String,
    target_dir: PathBuf,
}

struct GlobalFileTree(Entity<FileTree>);
impl Global for GlobalFileTree {}

/// 安装文件管理器的统一操作入口。文件页中的远程二进制/超限提示通过它复用
/// 现有下载冲突检查、下载目录设置和 busy ownership，不另建一套下载流程。
pub fn install(tree: Entity<FileTree>, cx: &mut App) {
    cx.set_global(GlobalFileTree(tree));
}

fn show_download_context_changed(project_id: &str, cx: &mut App) {
    let project_name = AppStore::global(cx)
        .read(cx)
        .project(project_id)
        .map(|project| project.name.clone())
        .unwrap_or_else(|| project_id.to_string());
    crate::toast::push_message(
        crate::notify::ToastKind::PasteError,
        project_id.to_string(),
        project_name,
        t("fileTree", "download.contextChanged").to_string(),
        cx,
    );
}

fn remote_download_context_matches(
    context: &FileOperationContext,
    project_id: &str,
    project_root: &str,
    connection_id: &str,
    connection_fingerprint: u64,
) -> bool {
    context.project_id == project_id
        && context.root.to_str() == Some(project_root)
        && matches!(
            &context.backend,
            FileBackendIdentity::Remote {
                connection_id: current_id,
                connection_fingerprint: current_fingerprint,
            } if current_id == connection_id
                && *current_fingerprint == connection_fingerprint
        )
}

/// 从当前可见文件页下载一个远程文件。项目或连接上下文已经变化时拒绝并提示，
/// 避免旧页签借当前文件树的连接把同名路径下载自另一台主机。
pub fn download_remote_file(
    project_id: &str,
    project_root: &str,
    connection_id: &str,
    connection_fingerprint: u64,
    path: PathBuf,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(tree) = cx
        .try_global::<GlobalFileTree>()
        .map(|global| global.0.clone())
    else {
        show_download_context_changed(project_id, cx);
        return;
    };
    let target = {
        let tree = tree.read(cx);
        tree.context_target(FileTreeContextEntry::Blank, cx)
    };
    let Some(target) = target else {
        show_download_context_changed(project_id, cx);
        return;
    };
    if !remote_download_context_matches(
        &target.context,
        project_id,
        project_root,
        connection_id,
        connection_fingerprint,
    ) {
        show_download_context_changed(project_id, cx);
        return;
    }
    start_download(tree, target, vec![path], window, cx);
}

struct FileTreeScopeState {
    entries: HashMap<PathBuf, Vec<FileEntry>>,
    entry_sources: HashMap<PathBuf, DirectoryListingOwner>,
    git_status: HashMap<String, String>,
    chain_owner: HashMap<PathBuf, PathBuf>,
    root_loading: bool,
    root_error: Option<String>,
    row_focus: HashMap<PathBuf, gpui::FocusHandle>,
    selected_path: Option<PathBuf>,
    scroll: ScrollHandle,
}

impl FileTreeScopeState {
    fn empty() -> Self {
        Self {
            entries: HashMap::new(),
            entry_sources: HashMap::new(),
            git_status: HashMap::new(),
            chain_owner: HashMap::new(),
            root_loading: false,
            root_error: None,
            row_focus: HashMap::new(),
            selected_path: None,
            scroll: ScrollHandle::new(),
        }
    }
}

fn swap_file_tree_scope(
    cache: &mut HashMap<WorktreeId, FileTreeScopeState>,
    current_worktree: Option<&WorktreeId>,
    next_worktree: Option<&WorktreeId>,
    current_state: FileTreeScopeState,
) -> (FileTreeScopeState, bool) {
    if current_worktree.is_some() && current_worktree == next_worktree {
        return (current_state, true);
    }
    if let Some(worktree_id) = current_worktree {
        cache.insert(worktree_id.clone(), current_state);
    }
    match next_worktree.and_then(|worktree_id| cache.remove(worktree_id)) {
        Some(state) => (state, true),
        None => (FileTreeScopeState::empty(), false),
    }
}

fn file_tree_scope_matches(
    current_worktree: Option<&WorktreeId>,
    current_signature: Option<&str>,
    next_worktree: Option<&WorktreeId>,
    next_signature: Option<&str>,
) -> bool {
    current_worktree == next_worktree && current_signature == next_signature
}

fn watcher_source_key(
    source_signature: Option<&str>,
    worktree_id: Option<&WorktreeId>,
    source_generation: u64,
) -> Option<String> {
    source_signature.map(|signature| {
        format!(
            "{signature}\0{}\0{source_generation}",
            worktree_id.map(WorktreeId::as_str).unwrap_or_default()
        )
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DirectoryRequestOwner {
    source: FileTreeSource,
    source_signature: Option<String>,
}

impl DirectoryRequestOwner {
    fn matches(
        &self,
        source: Option<&FileTreeSource>,
        source_signature: Option<&str>,
        request_id: u64,
        current_request_id: Option<u64>,
    ) -> bool {
        Some(&self.source) == source
            && self.source_signature.as_deref() == source_signature
            && Some(request_id) == current_request_id
    }

    fn result_source(
        &self,
        directory: &Path,
        remote: Option<&crate::remote_ssh::RemoteFileListingSource>,
        current_epoch: Option<u64>,
    ) -> Option<FileTreeSource> {
        let mut source = self.source.clone();
        match (&source.context.backend, remote) {
            (FileBackendIdentity::Local, None) if current_epoch.is_none() => {}
            (FileBackendIdentity::Remote { connection_id, connection_fingerprint }, Some(remote))
                if connection_id == &remote.connection_id
                    && *connection_fingerprint == remote.connection_fingerprint
                    && source.context.root.to_str() == Some(remote.project_root.as_str())
                    && directory.to_str() == Some(remote.directory.as_str())
                    && current_epoch == Some(remote.connection_epoch)
                    && source.connection_epoch.is_none_or(|epoch| epoch == remote.connection_epoch) =>
            {
                source.connection_epoch = Some(remote.connection_epoch);
            }
            _ => return None,
        }
        Some(source)
    }
}

pub struct FileTree {
    store: Entity<AppStore>,
    icon_tooltips: Entity<IconTooltips>,
    /// 已列出的目录 → 子项。
    entries: HashMap<PathBuf, Vec<FileEntry>>,
    entry_sources: HashMap<PathBuf, DirectoryListingOwner>,
    /// 正在列的目录(防重复排队)。
    loading: HashSet<PathBuf>,
    /// 每个目录当前有效的请求号；删除/切换时移除即可丢弃迟到结果。
    dir_request_ids: HashMap<PathBuf, u64>,
    next_dir_request_id: u64,
    /// 文件操作或 watcher 命中正在加载的目录时，当前请求完成后必须再列一次。
    /// bool 表示那次补列是否需要强制刷新远程根 `.gitignore`。
    pending_reload: HashMap<PathBuf, bool>,
    failed_dirs: HashSet<PathBuf>,
    watcher: Arc<FsWatcher>,
    watched: HashSet<PathBuf>,
    /// 当前挂着的项目;换项目时整表作废。
    current_project: Option<String>,
    /// Orca context ownership is stable-worktree scoped, not project-path scoped.
    current_worktree: Option<WorktreeId>,
    scope_cache: HashMap<WorktreeId, FileTreeScopeState>,
    selected_path: Option<PathBuf>,
    scroll: ScrollHandle,
    /// 项目 + 根路径 + 后端/连接配置的身份签名。连接配置原地修改时也会变化。
    /// watcher 另把 worktree 与 generation 编入注册 source key，避免 A→B→A
    /// 后接收第一轮 A 遗留在 channel 里的事件。
    source_signature: Option<String>,
    /// Source/worktree/epoch changes advance this fence, including ABA returns.
    source_generation: u64,
    source_exhausted: bool,
    source_epoch: Option<u64>,
    /// 应用内文件复制剪贴板；显式绑定项目和后端，不与系统文本剪贴板混用。
    file_clipboard: Option<FileTreeClipboard>,
    /// 当前 FileTree 同时只允许一个 mutation/transfer。
    operations: FileTreeOperations,
    operation_label: Option<String>,
    /// The running operation's subtree. Only its owner in `operations`, never
    /// this path alone, can release busy state across source switches.
    active_operation_suppressed_path: Option<PathBuf>,
    /// 正在被删除、重命名或批量改写的子树。操作结束前禁止 watcher / 展开态补列
    /// 重新挂载它，避免大目录删除时 watcher 事件洪泛和半成品缓存回写。
    suppressed_subtrees: HashSet<PathBuf>,
    /// 外部文件拖放当前命中的远程目标目录。
    external_drop_target: Option<ExternalDropTarget>,
    /// git 状态:相对项目根的 `/` 分隔路径 → 状态字母(M/A/D/R/?/C)。
    git_status: HashMap<String, String>,
    /// 排着的 git 状态刷新(去抖到点时刻);`None` = 没排。
    git_refresh_at: Option<Instant>,
    /// 压缩链的每一段 → 「产出这条链的那次列目录」。中段变化要重列它、重新压缩。
    chain_owner: HashMap<PathBuf, PathBuf>,
    /// 根目录还在列、且一份内容都还没有 —— 三态占位里的 loading 那一档。
    root_loading: bool,
    /// 上一次列根目录的错误原文。
    root_error: Option<String>,
    /// 当前项目是「断链的 SSH 远程项目」——连接被删,什么都列不出来。
    /// 与 `root_error` 分开存是因为它**不是一次加载失败**,而是一个静态状态:
    /// 不发请求、不重试,直接画那句提示。
    remote_broken: bool,
    /// 每行一个焦点句柄(原版每行 `tabIndex={0}`)。行拿到焦点后 Enter/Space
    /// 与 ←→ 才有落点,见 [`Self::on_row_key`]。
    row_focus: HashMap<PathBuf, gpui::FocusHandle>,
    _fs_task: Task<()>,
    _git_task: Task<()>,
}

impl FileTree {
    fn drop_external_paths(
        &mut self,
        target: Option<&FileTreeContextTarget>,
        paths: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = target.filter(|target| target.is_current(self, cx)).cloned() else {
            return;
        };
        self.external_drop_target = None;
        cx.notify();
        let tree = cx.entity();
        let paths = paths.paths().to_vec();
        // Upload reads the entity. Defer until this update lease and the native
        // OLE Drop callback have returned; a double lease there would abort.
        window.defer(cx, move |window, cx| {
            start_upload(tree, target, paths, window, cx);
        });
    }

    fn note_external_drop_target(
        &mut self,
        hit_id: &str,
        target: &FileTreeContextTarget,
        event: &DragMoveEvent<ExternalPaths>,
        cx: &mut Context<Self>,
    ) {
        if !target.is_current(self, cx) {
            return;
        }
        let Some(target) = target.directory() else {
            return;
        };
        if event.bounds.contains(&event.event.position) {
            if self
                .external_drop_target
                .as_ref()
                .is_none_or(|active| active.hit_id != hit_id || active.target_dir != target)
            {
                self.external_drop_target = Some(ExternalDropTarget {
                    hit_id: hit_id.to_string(),
                    target_dir: target,
                });
                cx.notify();
            }
        } else if self
            .external_drop_target
            .as_ref()
            .is_some_and(|active| active.hit_id == hit_id)
        {
            self.external_drop_target = None;
            cx.notify();
        }
    }

    pub fn new(store: Entity<AppStore>, cx: &mut Context<Self>) -> Self {
        cx.observe(&store, |this: &mut Self, _, cx| {
            this.sync_project(cx);
            cx.notify();
        })
        .detach();

        // 丢过去的是**变动文件的完整路径**:重列只要它的父目录,但技术栈缓存的
        // 失效判据要看文件名本身(`Cargo.toml` / `package.json` 之类)
        let (tx, mut rx) = mpsc::unbounded::<FsChange>();
        let watcher = Arc::new(FsWatcher::new(move |change| {
            // notify 自己的线程:把变更与注册时的 source owner 一起排回主线程。
            let _ = tx.unbounded_send(change);
        }));

        let fs_task = cx.spawn(async move |this, cx| {
            while let Some(change) = rx.next().await {
                let path = change.path;
                let dir = match path.parent() {
                    Some(parent) => parent.to_path_buf(),
                    None => path.clone(),
                };
                if this
                    .update(cx, |tree: &mut FileTree, cx| {
                        let current_source_key = watcher_source_key(
                            tree.source_signature.as_deref(),
                            tree.current_worktree.as_ref(),
                            tree.source_generation,
                        );
                        if !watcher_event_matches(
                            current_source_key.as_deref(),
                            tree.project_root(cx).as_deref(),
                            change.source_key.as_deref(),
                            &change.project_path,
                        ) || tree.path_is_suppressed(&path)
                        {
                            return;
                        }
                        tree.invalidate(&dir, cx);
                        // 原版第二条:`fs-change` 且属于当前项目 → 500ms 去抖刷 git 状态。
                        // watcher 是按项目根注册的,能走到这儿的必然属于当前项目。
                        tree.schedule_git_refresh();
                        tree.invalidate_dir_kind(&path, &dir, cx);
                    })
                    .is_err()
                {
                    return;
                }
            }
        });

        // 100ms 节拍:收 git 输出旁路的命中 + 到点跑去抖的那次刷新。
        // 与 Git 面板同一条旁路、同一个节拍常数,只是各自一个游标(见 git_watch)。
        let git_task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(git_watch::POLL_MS))
                    .await;
                if this
                    .update(cx, |tree: &mut FileTree, cx| tree.tick_git(cx))
                    .is_err()
                {
                    return;
                }
            }
        });

        let mut this = Self {
            store,
            icon_tooltips: cx.new(|_| IconTooltips::default()),
            entries: HashMap::new(),
            entry_sources: HashMap::new(),
            loading: HashSet::new(),
            dir_request_ids: HashMap::new(),
            next_dir_request_id: 0,
            pending_reload: HashMap::new(),
            failed_dirs: HashSet::new(),
            watcher,
            watched: HashSet::new(),
            current_project: None,
            current_worktree: None,
            scope_cache: HashMap::new(),
            selected_path: None,
            scroll: ScrollHandle::new(),
            source_signature: None,
            source_generation: 0,
            source_exhausted: false,
            source_epoch: None,
            file_clipboard: None,
            operations: FileTreeOperations::default(),
            operation_label: None,
            active_operation_suppressed_path: None,
            suppressed_subtrees: HashSet::new(),
            external_drop_target: None,
            git_status: HashMap::new(),
            git_refresh_at: None,
            chain_owner: HashMap::new(),
            root_loading: false,
            root_error: None,
            remote_broken: false,
            row_focus: HashMap::new(),
            _fs_task: fs_task,
            _git_task: git_task,
        };
        this.sync_project(cx);
        this
    }

    /// 排一次去抖刷新(原版 `debouncedRefresh`,500ms)。重复排只推后到点时刻。
    fn schedule_git_refresh(&mut self) {
        self.git_refresh_at = Some(Instant::now() + Duration::from_millis(git_watch::DEBOUNCE_MS));
    }

    /// 节拍:旁路命中就排一次去抖,到点就真去拉。
    fn tick_git(&mut self, cx: &mut Context<Self>) {
        // SSH epochs change outside AppStore notifications. Poll only identity;
        // source invalidation schedules a fresh read, never a mutation replay.
        self.sync_project(cx);
        if git_watch::drain_hit_for(git_watch::Subscriber::FileTree) {
            self.schedule_git_refresh();
        }
        if self.git_refresh_at.is_some_and(|at| Instant::now() >= at) {
            self.git_refresh_at = None;
            self.load_git_status(cx);
        }
    }

    /// 拉一次 git 状态。`get_git_status` 要跑 libgit2 的全量 status,**必须丢后台**。
    ///
    /// 失败一律清空(原版 `.catch(() => setGitStatusMap(new Map()))`):
    /// 不是 git 仓库 / 仓库坏了的时候,留着上一个项目的状态字母比没有更糟。
    fn load_git_status(&mut self, cx: &mut Context<Self>) {
        let Some(context) = self.operation_context(cx) else {
            self.git_status.clear();
            return;
        };
        if !matches!(&context.backend, FileBackendIdentity::Local) {
            self.git_status.clear();
            return;
        }
        let root = context.root.clone();
        cx.spawn(async move |this, cx| {
            let probe = root.clone();
            let result = cx
                .background_executor()
                .spawn(async move { mt_project::git::get_git_status(&probe) })
                .await;
            let _ = this.update(cx, |tree: &mut FileTree, cx| {
                // 回来时项目、连接或 source generation 都可能已经变化；完整上下文
                // 不相等就丢弃，避免同路径项目之间串入迟到的 git 状态。
                if tree.operation_context(cx).as_ref() != Some(&context) {
                    return;
                }
                tree.git_status = result
                    .map(|files| {
                        files
                            .into_iter()
                            .map(|f| (f.path, f.status_label))
                            .collect()
                    })
                    .unwrap_or_default();
                cx.notify();
            });
        })
        .detach();
    }

    fn take_scope_state(&mut self) -> FileTreeScopeState {
        FileTreeScopeState {
            entries: std::mem::take(&mut self.entries),
            entry_sources: std::mem::take(&mut self.entry_sources),
            git_status: std::mem::take(&mut self.git_status),
            chain_owner: std::mem::take(&mut self.chain_owner),
            root_loading: self.root_loading,
            root_error: self.root_error.take(),
            row_focus: std::mem::take(&mut self.row_focus),
            selected_path: self.selected_path.take(),
            scroll: std::mem::replace(&mut self.scroll, ScrollHandle::new()),
        }
    }

    fn install_scope_state(&mut self, state: FileTreeScopeState) {
        self.entries = state.entries;
        self.entry_sources = state.entry_sources;
        self.git_status = state.git_status;
        self.chain_owner = state.chain_owner;
        self.root_loading = state.root_loading;
        self.root_error = state.root_error;
        self.row_focus = state.row_focus;
        self.selected_path = state.selected_path;
        self.scroll = state.scroll;
    }

    fn clear_scope(&mut self) {
        self.entries.clear();
        self.entry_sources.clear();
        self.git_status.clear();
        self.chain_owner.clear();
        self.root_loading = false;
        self.root_error = None;
        self.row_focus.clear();
        self.selected_path = None;
        self.scroll = ScrollHandle::new();
    }

    /// 活动项目变了:清空活跃请求/监听，按 worktree 恢复展示缓存，再重列根目录。
    fn sync_project(&mut self, cx: &mut Context<Self>) {
        let (project_id, worktree_id, root, remote, broken, signature, epoch) = {
            let store = self.store.read(cx);
            match store.active_project() {
                Some(p) => {
                    let is_remote = store.is_remote_project(&p.id);
                    let conn = store.remote_connection_of(&p.id);
                    let path = store
                        .canonical_worktree_path_for_project(&p.id)
                        .unwrap_or(&p.path)
                        .to_string();
                    let signature = match &conn {
                        Some(conn) => format!(
                            "{}|{}|ssh:{:016x}",
                            p.id,
                            path,
                            crate::remote_ssh::connection_fingerprint(conn)
                        ),
                        None if is_remote => format!("{}|{}|ssh:broken", p.id, path),
                        None => format!("{}|{}|local", p.id, path),
                    };
                    (
                        Some(p.id.clone()),
                        store.worktree_id_for_project(&p.id).cloned(),
                        Some(PathBuf::from(path)),
                        conn.is_some(),
                        // 断链 = 是远程项目但连接查不到
                        is_remote && conn.is_none(),
                        Some(signature),
                        conn.as_ref().and_then(|conn| crate::remote_ssh::current_connection_epoch(&conn.id)),
                    )
                }
                None => (None, None, None, false, false, None, None),
            }
        };
        if file_tree_scope_matches(
            self.current_worktree.as_ref(),
            self.source_signature.as_deref(),
            worktree_id.as_ref(),
            signature.as_deref(),
        ) && (self.source_epoch == epoch
            || (self.source_epoch.is_none()
                && root.as_ref().is_some_and(|root| self.loading.contains(root))))
        {
            return;
        }
        for dir in std::mem::take(&mut self.watched) {
            self.watcher.unwatch(&dir);
        }
        if orca_worktree_context_enabled() {
            let current_worktree = self.current_worktree.clone();
            let current_state = self.take_scope_state();
            let (state, _) = swap_file_tree_scope(
                &mut self.scope_cache,
                current_worktree.as_ref(),
                worktree_id.as_ref(),
                current_state,
            );
            self.install_scope_state(state);
        } else {
            self.clear_scope();
            self.scope_cache.clear();
        }
        self.loading.clear();
        self.dir_request_ids.clear();
        self.pending_reload.clear();
        self.failed_dirs.clear();
        self.git_refresh_at = None;
        self.current_project = project_id;
        self.current_worktree = worktree_id;
        self.source_signature = signature;
        match self.source_generation.checked_add(1) {
            Some(generation) => self.source_generation = generation,
            None => self.source_exhausted = true,
        }
        self.source_epoch = epoch;
        self.suppressed_subtrees.clear();
        if let Some((active, _)) = self.operations.active.as_ref()
            && let (Some(current), Some(path)) = (
                self.current_source(cx),
                self.active_operation_suppressed_path.as_ref(),
            )
            && active.source.same_stable_source(&current)
        {
            self.suppressed_subtrees.insert(path.clone());
        }
        self.external_drop_target = None;
        self.remote_broken = broken;
        // 没有项目 / 远程项目都把旁路那一份关掉:远程不拉 git 状态,
        // reader 线程上那道总闸能少开一个人是一个
        git_watch::set_enabled_for(
            git_watch::Subscriber::FileTree,
            root.is_some() && !remote && !broken,
        );
        if broken {
            // 断链:不发任何请求,直接给那句明确提示(项目仍可见、可删)
            self.root_loading = false;
            self.root_error = Some(t("fileTree", "remote.broken").to_string());
            cx.notify();
            return;
        }
        if self.source_exhausted {
            self.root_loading = false;
            self.root_error = Some(t("fileTree", "operation.sourceUnavailable").to_string());
            cx.notify();
            return;
        }
        if let Some(root) = root {
            self.root_loading = true;
            self.load_dir(root.clone(), root, cx);
            // 原版第一条:切项目时与 `list_directory` 并发拉一次。
            // 远程项目跳过(远程 Git 二期)
            if !remote {
                self.load_git_status(cx);
            }
        } else {
            self.root_loading = false;
        }
        cx.notify();
    }

    /// 当前项目的远程连接(`None` = 本地项目 **或** 断链)。
    ///
    /// 返回克隆:它要被丢进 background executor(`remote_ssh` 的入口全是阻塞函数)。
    fn remote_conn(&self, cx: &App) -> Option<mt_config::SshConnection> {
        let store = self.store.read(cx);
        let id = store.active_project_id.as_deref()?;
        store.remote_connection_of(id)
    }

    fn project_root(&self, cx: &App) -> Option<PathBuf> {
        let store = self.store.read(cx);
        let project = store.active_project()?;
        Some(PathBuf::from(
            store
                .canonical_worktree_path_for_project(&project.id)
                .unwrap_or(&project.path),
        ))
    }

    fn operation_context(&self, cx: &App) -> Option<FileOperationContext> {
        if self.source_exhausted {
            return None;
        }
        let store = self.store.read(cx);
        let project = store.active_project()?;
        let backend = if store.is_remote_project(&project.id) {
            match store.remote_connection_of(&project.id) {
                Some(connection) => FileBackendIdentity::Remote {
                    connection_id: connection.id.clone(),
                    connection_fingerprint: crate::remote_ssh::connection_fingerprint(&connection),
                },
                None => FileBackendIdentity::BrokenRemote,
            }
        } else {
            FileBackendIdentity::Local
        };
        Some(FileOperationContext {
            project_id: project.id.clone(),
            root: PathBuf::from(
                store
                    .canonical_worktree_path_for_project(&project.id)
                    .unwrap_or(&project.path),
            ),
            backend,
            generation: self.source_generation,
        })
    }

    fn context_target(&self, entry: FileTreeContextEntry, cx: &App) -> Option<FileTreeContextTarget> {
        let directory = match &entry {
            FileTreeContextEntry::Blank => self.project_root(cx)?,
            FileTreeContextEntry::Row(row) => row.listing_directory.clone(),
        };
        let owner = self.entry_sources.get(&directory)?;
        if !owner.source.is_current(self, cx) {
            return None;
        }
        Some(FileTreeContextTarget {
            connection_epoch: owner.source.connection_epoch,
            context: owner.source.context.clone(),
            worktree_id: owner.source.worktree_id.clone(),
            listing: Some((directory, owner.request_id)),
            entry,
        })
    }

    fn current_source(&self, cx: &App) -> Option<FileTreeSource> {
        Some(FileTreeSource {
            context: self.operation_context(cx)?,
            worktree_id: self.current_worktree.clone(),
            connection_epoch: self.source_epoch,
        })
    }

    fn directory_is_current(&self, directory: &Path, cx: &App) -> bool {
        self.entry_sources
            .get(directory)
            .is_some_and(|owner| owner.source.is_current(self, cx))
    }

    fn reconcile_operation_source(&mut self, source: &FileTreeSource, cx: &mut Context<Self>) {
        if self.current_source(cx).is_some_and(|current| {
            source.same_stable_source(&current) && current.is_current(self, cx)
        }) {
            // Reconciliation is a new owned read. It never replays expansion,
            // selection, alerts, or mutation results from an old generation.
            self.entry_sources
                .retain(|_, owner| !owner.source.same_stable_source(source));
            self.loading.clear();
            self.dir_request_ids.clear();
            self.pending_reload.clear();
            self.refresh_root(cx);
        } else if let Some(state) = source
            .worktree_id
            .as_ref()
            .and_then(|id| self.scope_cache.get_mut(id))
        {
            state.entry_sources
                .retain(|_, owner| !owner.source.same_stable_source(source));
        }
    }

    fn path_is_suppressed(&self, path: &Path) -> bool {
        self.suppressed_subtrees
            .iter()
            .any(|suppressed| path.starts_with(suppressed))
    }

    /// 列一个目录(后台线程)+ 挂监听。
    ///
    /// `refresh_ignore` **只对远程有效**:强制后端重读远程根 `.gitignore`
    /// (头部刷新按钮那一路,原版 `loadRootEntries(true)`)。
    fn load_dir(&mut self, root: PathBuf, dir: PathBuf, cx: &mut Context<Self>) {
        self.load_dir_with(root, dir, false, false, cx);
    }

    fn load_dir_with(
        &mut self,
        root: PathBuf,
        dir: PathBuf,
        refresh_ignore: bool,
        queue_if_loading: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(request_source) = self.current_source(cx) else {
            return;
        };
        if matches!(&request_source.context.backend, FileBackendIdentity::BrokenRemote) {
            return;
        }
        if self.path_is_suppressed(&dir) {
            return;
        }
        if self.loading.contains(&dir) {
            if queue_if_loading {
                self.pending_reload
                    .entry(dir)
                    .and_modify(|pending| *pending |= refresh_ignore)
                    .or_insert(refresh_ignore);
            }
            return;
        }
        let Some(request_id) = self.next_dir_request_id.checked_add(1) else {
            self.entry_sources.clear();
            self.root_loading = false;
            self.root_error = Some(t("fileTree", "operation.sourceUnavailable").to_string());
            cx.notify();
            return;
        };
        self.next_dir_request_id = request_id;
        self.loading.insert(dir.clone());
        self.failed_dirs.remove(&dir);
        self.dir_request_ids.insert(dir.clone(), request_id);

        // 远程项目**不注册 watcher**:远端文件系统本机监听不到
        let remote = self.remote_conn(cx);
        let current_watcher_source_key = watcher_source_key(
            self.source_signature.as_deref(),
            self.current_worktree.as_ref(),
            self.source_generation,
        )
        .unwrap_or_default();
        if matches!(&request_source.context.backend, FileBackendIdentity::Local)
            && remote.is_none()
            && self.watched.insert(dir.clone())
            && let Err(err) = self.watcher.watch_scoped(
                &dir,
                root.to_string_lossy().as_ref(),
                current_watcher_source_key,
            )
        {
            eprintln!("[files] 监听 {} 失败: {err:#}", dir.display());
        }

        let task_dir = dir.clone();
        let task_root = root.clone();
        let request_owner = DirectoryRequestOwner {
            source: request_source,
            source_signature: self.source_signature.clone(),
        };
        // 根目录那一趟额外承担三态占位(loading / 加载失败 / 刷新失败)
        let is_root = dir == root;
        cx.spawn(async move |this, cx| {
            // 两条路都是阻塞 IO(本地要逐级读 .gitignore,远程是 SFTP 往返),
            // 必须离开主线程;单链压缩最多再串行列 7 层,**整段都在后台**跑完再回来。
            // 远程**不压缩单链**:逐级 SFTP 往返太贵(原版原话)
            let backend_source = request_owner.source.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let entries = match (&backend_source.context.backend, remote) {
                        (
                            FileBackendIdentity::Remote { connection_id, connection_fingerprint },
                            Some(conn),
                        ) if connection_id == &conn.id
                            && *connection_fingerprint == crate::remote_ssh::connection_fingerprint(&conn) =>
                        {
                            let listing = crate::remote_ssh::list_directory_at_epoch(
                                &conn,
                                backend_source.connection_epoch,
                                remote_path_text(&task_dir)?,
                                remote_path_text(&task_root)?,
                                refresh_ignore,
                            )?;
                            return Ok((
                                listing.entries.into_iter().map(|entry| (entry, Vec::new())).collect(),
                                Some(listing.source),
                            ));
                        }
                        (FileBackendIdentity::Local, None) => {
                            mt_project::fs::list_directory(&task_root, &task_dir)
                                .map_err(|error| format!("{error:#}"))?
                        }
                        _ => return Err(t("fileTree", "operation.sourceUnavailable").to_string()),
                    };
                    let chains = compact_dir_chains(entries, |d| {
                        mt_project::fs::list_directory(&task_root, d).unwrap_or_default()
                    });
                    Ok((chains, None))
                })
                .await;
            let _ = this.update(cx, |tree: &mut FileTree, cx| {
                if !request_owner.matches(
                    tree.current_source(cx).as_ref(),
                    tree.source_signature.as_deref(),
                    request_id,
                    tree.dir_request_ids.get(&dir).copied(),
                ) || tree.store.read(cx).worktree_id_for_project(&request_owner.source.context.project_id)
                    != request_owner.source.worktree_id.as_ref()
                {
                    return;
                }
                tree.loading.remove(&dir);
                tree.dir_request_ids.remove(&dir);
                if let Some(pending_refresh_ignore) = tree.pending_reload.remove(&dir) {
                    tree.load_dir_with(
                        root.clone(),
                        dir.clone(),
                        pending_refresh_ignore,
                        false,
                        cx,
                    );
                    return;
                }
                if is_root {
                    tree.root_loading = false;
                }
                match result {
                    Ok((rows, remote_source)) => {
                        let Some(source) = request_owner.result_source(
                            &dir,
                            remote_source.as_ref(),
                            file_context_connection_epoch(&request_owner.source.context),
                        ) else {
                            tree.entry_sources.remove(&dir);
                            tree.sync_project(cx);
                            if is_root && !tree.root_loading {
                                tree.root_error = Some(t("fileTree", "operation.sourceUnavailable").to_string());
                            }
                            cx.notify();
                            return;
                        };
                        tree.source_epoch = source.connection_epoch;
                        if is_root {
                            tree.root_error = None;
                        }
                        // 这一趟列出来的压缩链先整份作废,再按新结果登记 ——
                        // 链缩短/消失时旧的中段登记不能留着
                        tree.chain_owner.retain(|_, owner| owner != &dir);
                        let mut entries = Vec::with_capacity(rows.len());
                        for (entry, chain) in rows {
                            if chain.len() > 1 {
                                for segment in &chain {
                                    tree.chain_owner.insert(segment.clone(), dir.clone());
                                    // 链上**每一段**都要监听:后端 watcher 是
                                    // NonRecursive,中段新增文件否则无人上报,
                                    // 压缩前提破了也不知道
                                    let current_watcher_source_key = watcher_source_key(
                                        tree.source_signature.as_deref(),
                                        tree.current_worktree.as_ref(),
                                        tree.source_generation,
                                    )
                                    .unwrap_or_default();
                                    if tree.watched.insert(segment.clone())
                                        && let Err(err) = tree.watcher.watch_scoped(
                                            segment,
                                            root.to_string_lossy().as_ref(),
                                            current_watcher_source_key,
                                        )
                                    {
                                        eprintln!(
                                            "[files] 监听 {} 失败: {err:#}",
                                            segment.display()
                                        );
                                    }
                                }
                            }
                            entries.push(entry);
                        }
                        // 根目录一级子目录的子工程探测(原版 `FileTree.tsx:488-491`
                        // 那个 effect):不必展开就能在树里看到技术栈图标。
                        // 忽略项不探 —— 图标那一路也只在 `!entry.ignored` 时才换。
                        // **远程项目跳过**:`ensure_dir_kinds` 是本机 `stat`,
                        // 拿远端 POSIX 路径去探等于探一个不存在的本机目录
                        if is_root && !tree.is_remote(cx) {
                            let probe: Vec<String> = entries
                                .iter()
                                .filter(|e| e.is_dir && !e.ignored)
                                .map(|e| e.path.to_string_lossy().to_string())
                                .collect();
                            tree.store
                                .update(cx, |store, cx| store.ensure_dir_kinds(probe, cx));
                        }
                        tree.entry_sources.insert(
                            dir.clone(),
                            DirectoryListingOwner { source, request_id },
                        );
                        tree.entries.insert(dir, entries);
                    }
                    Err(err) => {
                        if file_context_connection_epoch(&request_owner.source.context) != request_owner.source.connection_epoch {
                            tree.sync_project(cx);
                            return;
                        }
                        tree.failed_dirs.insert(dir.clone());
                        eprintln!("[files] 列目录失败: {err:#}");
                        if is_root {
                            tree.root_error = Some(format!("{err:#}"));
                        } else {
                            // 子目录列失败也要在 `entries` 里留下这一条(空的)——
                            // 展开态补列(`missing_expanded_dirs`)的判据就是
                            // 「`entries` 里有没有这一项」,不落这条的话
                            // render → 补列 → 失败 → notify → render 会绕成死循环。
                            // `or_default` 不动已有内容:刷新失败时旧内容照旧留着,
                            // 与根目录那条「有旧内容就静默保留」同一口径
                            tree.entries.entry(dir).or_default();
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 头部刷新按钮 / 加载失败后的「重试」:重列根目录 + 重拉 git 状态
    /// (原版 `loadRootEntries() + loadGitStatus()` 那一对)。
    fn refresh_root(&mut self, cx: &mut Context<Self>) {
        // 断链:没什么可刷的(连接都没了),提示留着
        if self.remote_broken {
            return;
        }
        let Some(root) = self.project_root(cx) else {
            return;
        };
        self.failed_dirs.clear();
        // 没有内容可显示时才亮 loading —— 有旧内容就静默重列(原版同一条口径)
        if !self.entries.contains_key(&root) {
            self.root_loading = true;
        }
        let remote = self.is_remote(cx);
        // 手动刷新时强制重读远程根 `.gitignore`(原版 `loadRootEntries(true)`);
        // 本地那一路后端不认这个参数,传什么都一样
        self.load_dir_with(root.clone(), root, remote, true, cx);
        if !remote {
            self.load_git_status(cx);
        }
        cx.notify();
    }

    /// 当前项目是 SSH 远程项目吗(**断链也算** —— 那仍是个远程项目)。
    fn is_remote(&self, cx: &App) -> bool {
        let store = self.store.read(cx);
        store
            .active_project_id
            .as_deref()
            .is_some_and(|id| store.is_remote_project(id))
    }

    /// 目录内容变了:已列过的重列一次。
    ///
    /// 压缩链的**任何一段**变了也算 —— 重列产出这条链的那次列目录,让它按
    /// 新内容重新压缩(原版 `midChainHit` 那段的等价物,这里连链尾一起管:
    /// 链尾多出一个子目录同样能把链接长,重列一次比漏一次划算)。
    fn invalidate(&mut self, dir: &Path, cx: &mut Context<Self>) {
        let target = if self.entries.contains_key(dir) {
            dir.to_path_buf()
        } else if let Some(owner) = self.chain_owner.get(dir) {
            owner.clone()
        } else {
            return;
        };
        let Some(root) = self.project_root(cx) else {
            return;
        };
        self.load_dir_with(root, target, false, true, cx);
    }

    /// 技术栈缓存的失效(`useProjectKinds.ts:88-103` 的 `fs-change` 监听)。
    ///
    /// 判据逐条照抄:变动的**文件名**在标记文件表里,且它的**父目录正好是某个
    /// 本地项目的根**。原版注释点明了为什么只认项目根 —— 只有活跃项目的根目录
    /// 被 watch,那正是唯一能在应用内改到这些文件的场景。
    fn invalidate_dir_kind(&mut self, path: &Path, dir: &Path, cx: &mut Context<Self>) {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return;
        };
        if !crate::project_kind::is_marker_file(name) {
            return;
        }
        let parent = crate::project_kind::norm_path(&dir.to_string_lossy());
        let target = self
            .store
            .read(cx)
            .projects()
            .iter()
            .find(|p| {
                p.ssh_connection_id.is_none() && crate::project_kind::norm_path(&p.path) == parent
            })
            .map(|p| p.path.clone());
        if let Some(target) = target {
            self.store
                .update(cx, |store, cx| store.remove_dir_kind(&target, cx));
        }
    }

    // ─── 键盘导航(`FileTree.tsx:197-209`) ─────────────────────

    /// 行的焦点句柄(按需建、跨帧稳定)。
    fn row_focus(&mut self, path: &Path, cx: &mut Context<Self>) -> gpui::FocusHandle {
        self.row_focus
            .entry(path.to_path_buf())
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }

    /// 行按键。逐条照抄原版:目录 Enter/Space/→ 展开、← 折叠;文件 Enter/Space 开预览。
    ///
    /// ⚠️ **→ 只在折叠时生效、← 只在展开时生效**(原版那两个 `&& !expanded` /
    /// `&& expanded`),否则方向键会变成 toggle,在展开的目录上按 → 反而折叠。
    fn on_row_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        path: &Path,
        is_dir: bool,
        expanded: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.keystroke.key.as_str() {
            "enter" | "space" => {
                cx.stop_propagation();
                if is_dir {
                    self.toggle_dir(path.to_path_buf(), cx);
                } else {
                    self.open_file(path.to_path_buf(), window, cx);
                }
            }
            "right" if is_dir && !expanded => {
                cx.stop_propagation();
                self.toggle_dir(path.to_path_buf(), cx);
            }
            "left" if is_dir && expanded => {
                cx.stop_propagation();
                self.toggle_dir(path.to_path_buf(), cx);
            }
            _ => {}
        }
    }

    fn toggle_dir(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(project_id) = self.current_project.clone() else {
            return;
        };
        let key = path.to_string_lossy().to_string();
        let expanded = self.store.read(cx).is_dir_expanded(&project_id, &key);
        self.store.update(cx, |store, cx| {
            store.set_dir_expanded(&project_id, &key, !expanded, cx)
        });
        if !expanded {
            if let Some(root) = self.project_root(cx) {
                self.load_dir(root, path.clone(), cx);
            }
        } else if self.chain_owner.contains_key(&path) {
            // 压缩链上的目录**折叠了也要继续监听**:链的成立与否只看内容,
            // 不看展开状态(原版 `watchActive = expanded || chainPaths !== undefined`)
        } else {
            self.watched.remove(&path);
            self.watcher.unwatch(&path);
        }
        cx.notify();
    }

    /// 单击文件行 = 开文件预览器(`FileTree.tsx:151-155` 的 `handleToggle`:
    /// `!entry.isDir → onViewFile(entry.path)`)。
    ///
    /// **原版没有「双击调外部编辑器」这条路** —— 文件行上只有预览一条,
    /// 外部编辑器在原版只出现在项目级(头部按钮)与右键「使用默认工具打开」。
    /// AA 批之前 GPUI 侧的双击调编辑器是预览器缺位时的临时替身,现在撤掉:
    /// 留着的话双击会先开预览器再拉起编辑器(gpui 的双击是两个 click 事件,
    /// click_count 依次为 1、2),两个窗口一起冒出来。
    fn open_file(&self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        crate::workbench_area::open_active_file(self.store.clone(), path, None, window, cx);
    }

    /// 展开某个目录并(重)列它。新建文件/文件夹之后要用:原版是
    /// `if (!expanded) handleToggle(); else loadChildren();`。
    fn ensure_expanded(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let Some(project_id) = self.current_project.clone() else {
            return;
        };
        // 项目根不是树里的一行,没有「展开」这回事 —— 真去置位的话
        // `expandedDirs` 里会多出一条根路径落进 config.json(装机版没有这一条)
        if self.project_root(cx).as_deref() != Some(dir.as_path()) {
            let key = dir.to_string_lossy().to_string();
            if !self.store.read(cx).is_dir_expanded(&project_id, &key) {
                self.store.update(cx, |store, cx| {
                    store.set_dir_expanded(&project_id, &key, true, cx)
                });
            }
        }
        self.reload_dir(dir, cx);
    }

    /// 重列一个目录(文件操作跑完之后)。
    ///
    /// 监听那一路(`FsWatcher`)也会来一次,这里多列一遍是为了**立刻**看到结果 ——
    /// notify 在 Windows 上有几十到几百毫秒的抖动窗口。`load_dir` 自带
    /// 「同一目录不重复排队」的闸门,两条路撞上也只列一次。
    fn reload_dir(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.failed_dirs.remove(&dir);
        if let Some(root) = self.project_root(cx) {
            self.load_dir_with(root, dir, false, true, cx);
        }
        cx.notify();
    }

    /// 删除前解除目标子树 watcher 并清掉缓存，避免大目录逐文件事件洪泛；远程树虽
    /// 没 watcher，也共用缓存清理。失败后父目录重列会按展开状态逐层恢复。
    fn detach_subtree(&mut self, target: &Path) {
        let watched: Vec<PathBuf> = self
            .watched
            .iter()
            .filter(|path| path.starts_with(target))
            .cloned()
            .collect();
        for path in watched {
            self.watched.remove(&path);
            self.watcher.unwatch(&path);
        }
        self.entries.retain(|path, _| !path.starts_with(target));
        self.entry_sources.retain(|path, _| !path.starts_with(target));
        self.loading.retain(|path| !path.starts_with(target));
        self.dir_request_ids
            .retain(|path, _| !path.starts_with(target));
        self.pending_reload
            .retain(|path, _| !path.starts_with(target));
        self.chain_owner
            .retain(|path, owner| !path.starts_with(target) && !owner.starts_with(target));
        self.row_focus.retain(|path, _| !path.starts_with(target));
    }

    /// 把树按展开状态拍平成可渲染的行。
    fn rows(
        &self,
        project_id: &str,
        dir: &Path,
        depth: usize,
        cx: &App,
        out: &mut Vec<Row>,
    ) {
        let Some(entries) = self.entries.get(dir) else {
            return;
        };
        let store = self.store.read(cx);
        for entry in entries {
            let key = entry.path.to_string_lossy().to_string();
            let expanded = entry.is_dir && store.is_dir_expanded(project_id, &key);
            let rel = self.entry_sources.get(dir)
                .and_then(|owner| row_relative_path(&owner.source.context, &entry.path))
                .unwrap_or_default();
            let git = match self.git_status.get(&rel) {
                Some(label) => Some((label.clone(), false)),
                // 目录自身没有状态时才汇总子树(原版就是这个 if/else 的顺序)
                None if entry.is_dir => {
                    rollup_dir_label(&self.git_status, &rel).map(|l| (l.to_string(), true))
                }
                None => None,
            };
            out.push(Row {
                listing_directory: dir.to_path_buf(),
                name: entry.name.clone(),
                path: entry.path.clone(),
                is_dir: entry.is_dir,
                ignored: entry.ignored,
                depth,
                expanded,
                rel,
                git,
                // 一级子目录被识别为子工程时领位换技术栈徽标(`FileTree.tsx:346-351`)。
                // 条件一字不差:目录、depth == 0、非远程、未被 gitignore
                kind: (entry.is_dir && depth == 0 && !entry.ignored)
                    .then(|| store.dir_kind(&key))
                    .flatten()
                    .flatten(),
            });
            if expanded {
                self.rows(project_id, &entry.path, depth + 1, cx, out);
            }
        }
    }
}

fn row_relative_path(context: &FileOperationContext, path: &Path) -> Option<String> {
    if matches!(&context.backend, FileBackendIdentity::BrokenRemote) {
        return None;
    }
    let root = context.root.to_str()?;
    let path_text = path.to_str()?;
    if root.contains('\0') || path_text.contains('\0') {
        return None;
    }
    #[cfg(windows)]
    if matches!(&context.backend, FileBackendIdentity::Local)
        && context.root.is_absolute()
        && matches!(context.root.components().next(), Some(std::path::Component::Prefix(_)))
    {
        let relative = path.strip_prefix(&context.root).ok()?.to_str()?;
        return Some(if relative.is_empty() { ".".into() } else { relative.replace('\\', "/") });
    }
    // POSIX backslashes are filename data even on a Windows SSH client.
    // The legacy fs_ops relative helper normalizes them before stripping root.
    if !root.starts_with('/') || !path_text.starts_with('/') {
        return None;
    }
    let relative = crate::remote_ssh::posix_relative(root, path_text)?;
    Some(if relative.is_empty() { ".".into() } else { relative })
}

fn watcher_event_matches(
    current_source_key: Option<&str>,
    current_root: Option<&Path>,
    event_source_key: Option<&str>,
    event_project_path: &str,
) -> bool {
    current_source_key == event_source_key && current_root == Some(Path::new(event_project_path))
}

fn same_file_source(left: &FileOperationContext, right: &FileOperationContext) -> bool {
    left.project_id == right.project_id
        && left.backend == right.backend
        && match &left.backend {
            FileBackendIdentity::Local => left.root == right.root,
            FileBackendIdentity::Remote { .. } | FileBackendIdentity::BrokenRemote => {
                left.root.as_os_str() == right.root.as_os_str()
            }
        }
}

/// 「展开着、却一份内容都没列过」的目录 —— 要补列的那些。
///
/// 展开状态存在 [`AppStore`] 里并**落盘**(`ProjectConfig::expanded_dirs`),
/// 而 [`FileTree::entries`] 是纯内存缓存,[`FileTree::sync_project`] 换项目时整表清掉、
/// 只重列根目录(面板重建、冷启动同理)。两者一留一清,回到该项目时目录行还是
/// 展开态(`▾`),但 [`FileTree::rows`] 在 `entries` 里查不到内容 → 那一层一行不画,
/// 就成了「展开着,里头空的」。补列把两边接回去。
///
/// 只顺着**祖先全已列出**的那条链往下走:陈旧的深层展开记录(祖先早折叠了的)
/// 翻不到,不会白列一趟 —— 远程项目一次列目录是一趟 SFTP 往返,这笔不是小钱。
/// 一轮只补一层,列回来 notify 触发下一帧再补下一层,逐层收敛。
fn missing_expanded_dirs(
    entries: &HashMap<PathBuf, Vec<FileEntry>>,
    dir: &Path,
    is_expanded: &dyn Fn(&Path) -> bool,
    is_current: &dyn Fn(&Path) -> bool,
    out: &mut Vec<PathBuf>,
) {
    let Some(rows) = entries.get(dir) else {
        return;
    };
    for entry in rows {
        if !entry.is_dir || !is_expanded(&entry.path) {
            continue;
        }
        if entries.contains_key(&entry.path) && is_current(&entry.path) {
            missing_expanded_dirs(entries, &entry.path, is_expanded, is_current, out);
        } else {
            out.push(entry.path.clone());
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowClickAction {
    ToggleDirectory,
    OpenPreview,
    Rename,
    None,
}

fn row_click_action(is_dir: bool, click_count: usize) -> RowClickAction {
    if is_dir {
        return match click_count {
            1 => RowClickAction::ToggleDirectory,
            2 => RowClickAction::Rename,
            _ => RowClickAction::None,
        };
    }
    match click_count {
        1 => RowClickAction::OpenPreview,
        2 => RowClickAction::Rename,
        _ => RowClickAction::None,
    }
}

#[derive(Clone)]
struct Row {
    listing_directory: PathBuf,
    name: String,
    path: PathBuf,
    is_dir: bool,
    ignored: bool,
    depth: usize,
    expanded: bool,
    /// 相对项目根、`/` 分隔的路径(git 状态表的键,「查看变更」也拿它当参数)。
    rel: String,
    /// git 状态字母 + **是不是汇总来的**(汇总的那枚画淡一档)。
    git: Option<(String, bool)>,
    /// 一级子目录的技术栈徽标(`None` = 用普通文件夹图标)。
    kind: Option<mt_ui::icons::ProjectKind>,
}

// ─── 单链目录压缩(`FileTree.tsx:50-86` 的 `compactDirChains`) ──

/// 链深上限。每深一层多一次**串行** `list_directory`(后端还要跑 gitignore 匹配),
/// 8 层足够覆盖 Java 式深包名(`src/main/java/com/foo/bar`)。
const MAX_CHAIN: usize = 8;

/// IDE 的 "compact middle packages":目录**一路只有唯一子目录、没有文件**时,
/// 折成一行 `main/java/com/…`。
///
/// `list` 是「列一个目录」的闭包(真跑时是阻塞的 `list_directory`,单测里喂假表),
/// 返回值与入参一一对应:`(展示用的条目, 链上每一段的路径)`。
/// **没压缩的条目 `chain` 长度为 1**,调用方据此判断要不要登记链。
///
/// 三条规则照抄原版:非目录 / 被 gitignore 的条目不参与;继续的条件是
/// 「唯一子项且它是未被忽略的目录」;拼名字用 `/` 而**不是**平台分隔符。
fn compact_dir_chains(
    entries: Vec<FileEntry>,
    mut list: impl FnMut(&Path) -> Vec<FileEntry>,
) -> Vec<(FileEntry, Vec<PathBuf>)> {
    entries
        .into_iter()
        .map(|mut entry| {
            if !entry.is_dir || entry.ignored {
                let chain = vec![entry.path.clone()];
                return (entry, chain);
            }
            let mut chain = vec![entry.path.clone()];
            let mut name = entry.name.clone();
            while chain.len() < MAX_CHAIN {
                let kids = list(chain.last().expect("链至少有一段"));
                let [only] = kids.as_slice() else {
                    break;
                };
                if !only.is_dir || only.ignored {
                    break;
                }
                name.push('/');
                name.push_str(&only.name);
                chain.push(only.path.clone());
            }
            if chain.len() > 1 {
                entry.name = name;
                // 展示的是链尾那个**真实**目录:展开它列的就是链尾的子项
                entry.path = chain.last().cloned().expect("链至少有一段");
            }
            (entry, chain)
        })
        .collect()
}

// ─── git 状态着色(`FileTree.tsx:359-400`) ───────────────────

/// 状态字母 → 颜色。认不出的字母退 `--text-muted`(原版的 `?? text-muted`)。
fn git_color(label: &str) -> Hsla {
    match label {
        "M" => ui::color_warning(),
        "A" | "?" => ui::color_success(),
        "D" | "C" => ui::color_error(),
        "R" => ui::color_info(),
        _ => ui::text_muted(),
    }
}

/// 目录汇总的优先级(原版 `PRIORITY`,数越大越优先)。0 = 不参与汇总。
fn git_priority(label: &str) -> u8 {
    match label {
        "C" => 6,
        "D" => 5,
        "M" => 4,
        "A" => 3,
        "R" => 2,
        "?" => 1,
        _ => 0,
    }
}

/// 目录行的汇总字母:扫状态表里所有以 `rel/` 开头的条目,取优先级最高的那个。
///
/// `rel` 传空串(理论上的项目根)时前缀是 `"/"`,与原版一样谁也匹配不上 ——
/// 根不是树里的一行,不会真的走到这一支。
fn rollup_dir_label<'a>(status: &'a HashMap<String, String>, rel: &str) -> Option<&'a str> {
    let prefix = if rel.ends_with('/') {
        rel.to_string()
    } else {
        format!("{rel}/")
    };
    let mut best: Option<(&str, u8)> = None;
    for (path, label) in status {
        if !path.starts_with(&prefix) {
            continue;
        }
        let p = git_priority(label);
        if p > best.map(|(_, bp)| bp).unwrap_or(0) {
            best = Some((label.as_str(), p));
        }
    }
    best.map(|(label, _)| label)
}

/// 头部 26×26 图标钮共用的外观(`FileTree.tsx:734`)。
fn header_button(id: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .relative()
        .w(px(26.0))
        .h(px(26.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.0))
        .text_color(ui::text_muted())
        .cursor_pointer()
        .hover(|el| el.text_color(ui::text_primary()).bg(ui::border_subtle()))
}

// gpui-component 0.5.1 uses a 16px vertical bar (4px insets + 8px thumb).
const SCROLLBAR_GUTTER: gpui::Pixels = px(16.0);

fn file_tree_scroll_shell() -> gpui::Div {
    div()
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .min_w(px(0.0))
        .overflow_hidden()
}

fn file_tree_scroll_list(scroll: &ScrollHandle) -> gpui::Stateful<gpui::Div> {
    div()
        .id("file-tree-list")
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .min_w(px(0.0))
        .track_scroll(scroll)
        .overflow_y_scroll()
        .scrollbar_width(SCROLLBAR_GUTTER)
}

fn file_tree_scrollbar_overlay() -> gpui::Div {
    div()
        .absolute()
        .top(px(0.0))
        .right(px(0.0))
        .bottom(px(0.0))
        .w(SCROLLBAR_GUTTER)
}

fn file_tree_row(id: SharedString) -> gpui::Stateful<gpui::Div> {
    div().id(id).flex().flex_none().min_w_0()
}

impl Render for FileTree {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !cx.has_active_drag() {
            self.external_drop_target = None;
        }
        let project_name = self.store.read(cx).active_project().map(|p| p.name.clone());
        let is_remote = self.is_remote(cx);
        let mut header = div()
            .id("file-tree-header")
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .flex_none()
            .min_w_0()
            .px(px(10.0))
            .py(px(6.0))
            .border_b_1()
            .border_color(ui::border_subtle())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(ui::font_px(11.0))
                    .text_color(ui::text_muted())
                    // 有项目时带项目名(`panels.filesOf`),没有就退回纯「文件」
                    .child(match &project_name {
                        Some(name) => tr!("panels", "filesOf", project = name.clone()),
                        None => t("panels", "files").to_string(),
                    }),
            );

        if project_name.is_some() {
            let refresh = header_button("file-tree-refresh")
                .on_click(cx.listener(|this, _event, _window, cx| this.refresh_root(cx)))
                .child(VectorIcon::new(ICON_REFRESH, px(13.0)).ink(ui::text_muted()));
            let refresh = IconTooltips::button(
                &self.icon_tooltips,
                "file-tree-refresh",
                if is_remote {
                    t("fileTree", "remote.refreshTitle")
                } else {
                    t("fileTree", "header.refresh")
                },
                refresh,
                window,
                cx,
            );
            header = header.child(IconTooltips::group(
                &self.icon_tooltips,
                div()
                    .id("file-tree-tools")
                    .relative()
                    .flex()
                    .flex_none()
                    .child(refresh),
                window,
                cx,
            ));
        }

        let Some(project_id) = self.current_project.clone() else {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .bg(ui::bg_surface())
                .child(header)
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(ui::font_px(11.4))
                        .text_color(ui::text_muted())
                        .child(t("fileTree", "empty.selectProject")),
                );
        };
        let Some(root) = self.project_root(cx) else {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .bg(ui::bg_surface())
                .child(header);
        };

        // 展开态与 `entries` 缓存的对账:展开着却没列过的目录在这儿补列回来
        // (换项目/面板重建/冷启动都会把缓存清掉,展开状态却是落盘的)。
        // `load_dir` 自带「同目录不重复排队」的闸门 —— 排队的那几帧重复走到这儿
        // 只多查一次 HashSet
        {
            let mut missing = Vec::new();
            {
                let store = self.store.read(cx);
                let is_expanded = |path: &Path| {
                    store.is_dir_expanded(&project_id, path.to_string_lossy().as_ref())
                };
                if self.directory_is_current(&root, cx) {
                    let is_current = |path: &Path| self.directory_is_current(path, cx) || self.failed_dirs.contains(path);
                    missing_expanded_dirs(&self.entries, &root, &is_expanded, &is_current, &mut missing);
                }
            }
            missing.retain(|dir| !self.path_is_suppressed(dir));
            for dir in missing {
                self.load_dir(root.clone(), dir, cx);
            }
        }

        let mut rows = Vec::new();
        self.rows(&project_id, &root, 0, cx, &mut rows);
        // 行焦点句柄按**当前可见行**补齐并回收(折叠掉的行不必留着句柄)。
        // 句柄要跨帧稳定 —— 每帧新建的话 Tab 过去的焦点每帧都会丢
        {
            let visible: HashSet<&PathBuf> = rows.iter().map(|r| &r.path).collect();
            self.row_focus.retain(|path, _| visible.contains(path));
            let missing: Vec<PathBuf> = rows
                .iter()
                .filter(|r| !self.row_focus.contains_key(&r.path))
                .map(|r| r.path.clone())
                .collect();
            for path in missing {
                self.row_focus(&path, cx);
            }
        }

        // 断链的远程项目:静态提示,**没有重试按钮**(连接都没了,重试也是白试)
        if self.remote_broken {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .bg(ui::bg_surface())
                .child(header)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .py(px(32.0))
                        .px(px(12.0))
                        .text_size(ui::font_px(11.4))
                        .text_color(ui::color_error())
                        .child(t("fileTree", "remote.broken")),
                );
        }

        // 三态占位:**都以「一行都没有」为前置** —— 有缓存内容时不整块盖掉
        if rows.is_empty() && (self.root_loading || self.root_error.is_some()) {
            let body = if self.root_loading {
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .py(px(32.0))
                    .text_size(ui::font_px(11.4))
                    .text_color(ui::text_muted())
                    .child(t("fileTree", "empty.loading"))
            } else {
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(8.0))
                    .py(px(32.0))
                    .px(px(12.0))
                    .text_size(ui::font_px(11.4))
                    .child(
                        div()
                            .truncate()
                            .text_color(ui::text_muted())
                            .child(t("fileTree", "empty.loadFailed")),
                    )
                    .child(
                        div()
                            .id("file-tree-retry")
                            .px(px(8.0))
                            .py(px(4.0))
                            .rounded(px(3.0))
                            .cursor_pointer()
                            .text_color(ui::accent())
                            .hover(|el| el.bg(ui::border_subtle()))
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.refresh_root(cx);
                            }))
                            .child(t("fileTree", "empty.retry")),
                    )
            };
            return div()
                .size_full()
                .flex()
                .flex_col()
                .bg(ui::bg_surface())
                .child(header)
                .child(body);
        }

        let has_stale_rows = rows
            .iter()
            .any(|row| !self.directory_is_current(&row.listing_directory, cx));
        let operation_label = self.operation_label.clone().filter(|_| {
            self.operations
                .active
                .as_ref()
                .is_some_and(|(owner, _)| owner.source.is_current(self, cx))
        });
        let mut list = file_tree_scroll_list(&self.scroll);
        for row in rows {
            list = list.child(self.render_row(row, cx));
        }
        let background_target = self.context_target(FileTreeContextEntry::Blank, cx);
        let background_menu_target = background_target.clone();
        let background_drop_id = "background".to_string();
        let background_highlight = self
            .external_drop_target
            .as_ref()
            .is_some_and(|active| active.hit_id == background_drop_id);
        list = list.child(
            div()
                .id("file-tree-background")
                .flex_1()
                .min_h(px(24.0))
                .when(background_highlight, |el| {
                    el.bg(ui::with_alpha(ui::accent(), 0.12))
                })
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        let Some(target) = background_menu_target.clone() else {
                            return;
                        };
                        if !target.is_current(this, cx) {
                            return;
                        }
                        let store = this.store.clone();
                        let entries = file_menu(
                            &cx.entity(),
                            &store,
                            target,
                            this.remote_conn(cx),
                            false,
                        );
                        crate::menu::show(event.position, entries, window, cx);
                    }),
                )
                .when(is_remote, |el| {
                    let move_target = background_target.clone();
                    let drop_target = background_target.clone();
                    let move_id = background_drop_id.clone();
                    el.on_drag_move(cx.listener(
                        move |this, event: &DragMoveEvent<ExternalPaths>, _window, cx| {
                            if let Some(target) = &move_target {
                                this.note_external_drop_target(&move_id, target, event, cx);
                            }
                        },
                    ))
                    .on_drop(cx.listener(
                        move |this, paths: &ExternalPaths, window, cx| {
                            this.drop_external_paths(drop_target.as_ref(), paths, window, cx);
                        },
                    ))
                }),
        );
        let scroll_shell = file_tree_scroll_shell().child(list).child(
            file_tree_scrollbar_overlay()
                .child(Scrollbar::vertical(&self.scroll).id("file-tree-scrollbar")),
        );

        div()
            .size_full()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(ui::bg_surface())
            .child(header)
            .when(has_stale_rows, |el| {
                el.child(
                    div()
                        .flex_none()
                        .px(px(8.0))
                        .py(px(4.0))
                        .truncate()
                        .text_size(ui::font_px(9.75))
                        .text_color(ui::text_muted())
                        .child(t("fileTree", if self.loading.is_empty() {
                            "empty.refreshFailed"
                        } else {
                            "empty.loading"
                        })),
                )
            })
            // 有旧内容时的刷新失败:一条细提示挂在列表**上方**,内容照旧留着
            .when_some(self.root_error.clone().filter(|_| !has_stale_rows), |el, _err| {
                el.child(
                    div()
                        .flex_none()
                        .px(px(8.0))
                        .py(px(4.0))
                        .truncate()
                        .text_size(ui::font_px(9.75))
                        .text_color(ui::text_muted())
                        .child(t("fileTree", "empty.refreshFailed")),
                )
            })
            .when_some(operation_label, |el, label| {
                el.child(
                    div()
                        .flex_none()
                        .px(px(8.0))
                        .py(px(4.0))
                        .truncate()
                        .text_size(ui::font_px(10.5))
                        .text_color(ui::accent())
                        .child(label),
                )
            })
            .child(scroll_shell)
    }
}

impl FileTree {
    fn render_row(&self, row: Row, cx: &mut Context<Self>) -> AnyElement {
        let path = row.path.clone();
        let is_dir = row.is_dir;
        let drag_path = row.path.clone();
        let drag_name = row.name.clone();
        let drag_is_dir = row.is_dir;
        let indent = px(6.0 + row.depth as f32 * 12.0);
        let color = if row.ignored {
            ui::text_muted()
        } else if row.is_dir {
            ui::color_folder()
        } else {
            ui::color_file()
        };
        // 图标按文件名/是否目录/是否展开取类别(`FileIcon` 内含 53 类映射,
        // 「特殊文件名压扩展名」的语义也在那边:Cargo.lock 是锁文件不是 toml)。
        //
        // `.gitignore` 掉的条目统一压成 muted,与文字同色;其余用类别自带的
        // 语言色。**git 状态着的是行尾那枚状态字母,不是文件名本身的颜色**
        // (`FileTree.tsx:565` 的注释专门点了这条)。
        let git_badge = row.git.clone();
        // 一级子工程目录优先显示技术栈徽标(原版那段 IIFE 的第一条分支)
        let icon: AnyElement = match row.kind {
            Some(kind) => mt_ui::icons::TechIcon::new(kind)
                .size(px(14.0))
                .into_any_element(),
            None => {
                let icon = FileIcon::new(&row.name, row.is_dir, row.expanded).size(px(14.0));
                if row.ignored {
                    icon.color(ui::text_muted()).into_any_element()
                } else {
                    icon.into_any_element()
                }
            }
        };
        let focus = self.row_focus.get(&row.path).cloned();
        let key_path = row.path.clone();
        let key_expanded = row.expanded;
        let remote = self.is_remote(cx);
        let row_target = self.context_target(FileTreeContextEntry::Row(row.clone()), cx);
        let actionable = row_target.is_some();
        let key_target = row_target.clone();
        let click_target = row_target.clone();
        let selection_target = row_target.clone();
        let click_connection = self.remote_conn(cx);
        let tree_for_click = cx.entity();
        let upload_target = row_target.clone();
        let row_drop_id = format!("row:{}", row.path.display());
        let drop_highlight = self
            .external_drop_target
            .as_ref()
            .is_some_and(|active| active.hit_id == row_drop_id);

        file_tree_row(SharedString::from(format!("fs-{}", row.path.display())))
            // 行级焦点 + tab 停靠点(原版每行 `tabIndex={0}` + `role=treeitem`)
            .when_some(focus, |el, focus| el.track_focus(&focus).tab_index(0))
            .on_key_down(
                cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                    if !key_target
                        .as_ref()
                        .is_some_and(|target| target.is_current(this, cx))
                    {
                        return;
                    }
                    this.on_row_key(event, &key_path, is_dir, key_expanded, window, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener({
                    let path = row.path.clone();
                    move |this, _event: &MouseDownEvent, window, cx| {
                        if !selection_target
                            .as_ref()
                            .is_some_and(|target| target.is_current(this, cx))
                        {
                            return;
                        }
                        // 浏览器点 `tabIndex=0` 的行就会聚焦,←→ 折叠展开靠这一条才够得着
                        if let Some(focus) = this.row_focus.get(&path) {
                            window.focus(focus);
                        }
                        if this.selected_path.as_ref() != Some(&path) {
                            this.selected_path = Some(path.clone());
                            cx.notify();
                        }
                    }
                }),
            )
            .items_center()
            .gap(px(4.0))
            .pl(indent)
            .pr(px(6.0))
            .py(px(2.0))
            .when(actionable, |el| el.cursor_pointer())
            .text_size(ui::font_px(12.0))
            .text_color(color)
            .when(!actionable, |el| el.text_color(ui::text_muted()))
            .hover(|el| el.bg(ui::bg_overlay()))
            .when(self.selected_path.as_ref() == Some(&row.path), |el| {
                el.bg(ui::accent_subtle())
            })
            .when(drop_highlight, |el| {
                el.bg(ui::with_alpha(ui::accent(), 0.18))
            })
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    let Some(target) = click_target
                        .as_ref()
                        .filter(|target| target.is_current(this, cx))
                    else {
                        return;
                    };
                    match row_click_action(is_dir, event.click_count()) {
                        RowClickAction::ToggleDirectory => this.toggle_dir(path.clone(), cx),
                        RowClickAction::OpenPreview => this.open_file(path.clone(), window, cx),
                        RowClickAction::Rename => {
                            let tree = tree_for_click.clone();
                            let target = target.clone();
                            let connection = click_connection.clone();
                            window.defer(cx, move |window, cx| {
                                open_rename_prompt(tree, target, connection, window, cx);
                            });
                        }
                        RowClickAction::None => {}
                    }
                }),
            )
            // 拖进终端 = 把路径当文本写进 PTY(不是上传文件)。目录同样可拖,
            // 与原版一致(`FileTree.tsx:326-328` 的 `initFileDrag(entry.path)`
            // 不区分文件/目录)。落点在 `terminal_area.rs` 的 pane 主体。
            //
            // 原版为此自研了一整套 pointer 跟踪 + `body.file-dragging` 的
            // `pointer-events:none` 穿透规则(要让鼠标穿过 xterm 的子 DOM 打到
            // drop-zone 上);gpui 侧终端是自绘 Element、drop 目标就是它的容器,
            // 那条穿透规则一行都不必移植。
            .when(actionable, |el| el.on_drag(
                crate::dnd::DragFilePath(drag_path),
                move |_item, _offset, _window, cx| {
                    crate::dnd::preview(
                        drag_name.clone(),
                        crate::dnd::PreviewIcon::File {
                            name: drag_name.clone(),
                            is_dir: drag_is_dir,
                        },
                        cx,
                    )
                },
            ))
            .when(remote, |el| {
                let move_target = upload_target.clone();
                let drop_target = upload_target.clone();
                let move_id = row_drop_id.clone();
                el.on_drag_move(cx.listener(
                    move |this, event: &DragMoveEvent<ExternalPaths>, _window, cx| {
                        if let Some(target) = &move_target {
                            this.note_external_drop_target(&move_id, target, event, cx);
                        }
                    },
                ))
                .on_drop(cx.listener(
                    move |this, paths: &ExternalPaths, window, cx| {
                        this.drop_external_paths(drop_target.as_ref(), paths, window, cx);
                    },
                ))
            })
            // 行的右键菜单。**必须 stop_propagation** —— 否则会连带触发列表容器
            // 那个「空白处右键 = 新建」的菜单(原版靠 `e.stopPropagation()` 同理)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let Some(target) = row_target.clone() else {
                        return;
                    };
                    if !target.is_current(this, cx) {
                        return;
                    }
                    let store = this.store.clone();
                    let connection = this.remote_conn(cx);
                    let can_paste = this.file_clipboard.as_ref().is_some_and(|clipboard| {
                        clipboard.owner.is_current(this, cx)
                            && clipboard.can_paste_into(&target)
                            && target
                                .directory()
                                .is_some_and(|dir| !clipboard.entry.would_copy_into_itself(&dir))
                    });
                    let entries = file_menu(&cx.entity(), &store, target, connection, can_paste);
                    crate::menu::show(event.position, entries, window, cx);
                }),
            )
            .child(
                div()
                    .w(px(10.0))
                    .text_color(ui::text_muted())
                    .when(row.is_dir, |el| {
                        el.child(if row.expanded { "▾" } else { "▸" })
                    }),
            )
            .child(icon)
            .child(div().flex_1().min_w_0().truncate().child(row.name))
            // git 状态字母。目录那枚是**汇总**来的,画淡一档以示区别(原版 opacity-70)
            .when_some(git_badge, |el, (label, rolled_up)| {
                el.child(
                    div()
                        .flex_none()
                        .ml(px(6.0))
                        .text_size(ui::font_px(9.75))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(if rolled_up {
                            ui::with_alpha(git_color(&label), 0.7)
                        } else {
                            git_color(&label)
                        })
                        .child(label),
                )
            })
            .into_any_element()
    }
}
