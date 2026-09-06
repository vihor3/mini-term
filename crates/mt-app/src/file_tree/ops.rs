//! 文件树的文件操作自由函数(从 `file_tree` 平移):preflight 三件套、
//! [`spawn_tree_op`]、应用内文件剪贴板的复制/粘贴、上传/下载全套、
//! 在终端打开、新建文件/文件夹。

use std::path::PathBuf;

use gpui::{App, Entity, SharedString, Window};

use crate::file_ops::{FileBackendIdentity, FileClipboardEntry};
use crate::i18n::{t, tr};
use crate::prompt::{show_alert, show_file_conflict_choice, show_prompt};
use crate::store::AppStore;

use super::{
    FileTree, FileTreeClipboard, FileTreeContextTarget, FileTreeOperationOwner,
    FileTreeOperationPhase, remote_path_text,
};

/// 跑一件阻塞文件操作。状态和结果都绑定开始时的项目/连接/generation；切换项目后
/// 旧结果不会刷新新树。同一 FileTree 同时只接受一件 mutation/transfer。
fn begin_tree_preflight(
    tree: &Entity<FileTree>,
    target: &FileTreeContextTarget,
    label: SharedString,
    window: &mut Window,
    cx: &mut App,
) -> Option<FileTreeOperationOwner> {
    reserve_tree_operation(
        tree,
        target,
        FileTreeOperationPhase::Preflight,
        label,
        window,
        cx,
    )
}

fn reserve_tree_operation(
    tree: &Entity<FileTree>,
    target: &FileTreeContextTarget,
    phase: FileTreeOperationPhase,
    label: SharedString,
    window: &mut Window,
    cx: &mut App,
) -> Option<FileTreeOperationOwner> {
    let start_state = tree.update(cx, |tree, cx| {
        if !target.is_current(tree, cx) {
            return None;
        }
        let Some(owner) = tree.operations.reserve(target, phase) else {
            return Some(None);
        };
        tree.operation_label = Some(label.to_string());
        tree.active_operation_suppressed_path = None;
        cx.notify();
        Some(Some(owner))
    });
    match start_state {
        Some(Some(owner)) => Some(owner),
        Some(None) => {
            show_alert(
                t("fileTree", "operation.busyTitle"),
                t("fileTree", "operation.busyMessage"),
                window,
                cx,
            );
            None
        }
        None => None,
    }
}

fn finish_tree_preflight(
    tree: &Entity<FileTree>,
    owner: &FileTreeOperationOwner,
    target: &FileTreeContextTarget,
    cx: &mut App,
) -> Option<bool> {
    tree.update(cx, |tree, cx| {
        if !owner.matches_target(target) || !tree.operations.finish(owner, false) {
            return None;
        }
        let context_matches = target.is_current(tree, cx);
        tree.operation_label = None;
        tree.active_operation_suppressed_path = None;
        cx.notify();
        Some(context_matches)
    })
}

fn retain_tree_preflight_for_choice(
    tree: &Entity<FileTree>,
    owner: &FileTreeOperationOwner,
    target: &FileTreeContextTarget,
    cx: &mut App,
) -> bool {
    tree.update(cx, |tree, cx| {
        if !owner.matches_target(target) {
            return false;
        }
        if !target.is_current(tree, cx) {
            if tree.operations.finish(owner, false) {
                tree.operation_label = None;
                cx.notify();
            }
            return false;
        }
        if !tree
            .operations
            .transition(owner, FileTreeOperationPhase::Choice)
        {
            return false;
        }
        tree.operation_label = Some(t("fileTree", "conflict.title").to_string());
        cx.notify();
        true
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_tree_op(
    tree: Entity<FileTree>,
    target: FileTreeContextTarget,
    reservation: Option<FileTreeOperationOwner>,
    refresh_dir: Option<PathBuf>,
    expand: bool,
    detach_before: Option<PathBuf>,
    label: SharedString,
    op: impl FnOnce() -> Result<Option<String>, String> + Send + 'static,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let suppressed_path = detach_before.clone();
    let owner = match reservation {
        Some(owner) => {
            let started = tree.update(cx, |tree, cx| {
                if !target.is_current(tree, cx) || !owner.matches_target(&target) {
                    if tree.operations.finish(&owner, false) {
                        tree.operation_label = None;
                        cx.notify();
                    }
                    return false;
                }
                if !tree
                    .operations
                    .transition(&owner, FileTreeOperationPhase::Running)
                {
                    return false;
                }
                tree.operation_label = Some(label.to_string());
                true
            });
            if !started {
                return false;
            }
            owner
        }
        None => {
            let Some(owner) = reserve_tree_operation(
                &tree,
                &target,
                FileTreeOperationPhase::Running,
                label,
                window,
                cx,
            ) else {
                return false;
            };
            owner
        }
    };
    tree.update(cx, |tree, cx| {
        tree.active_operation_suppressed_path = suppressed_path.clone();
        cx.notify();
    });
    if let Some(path) = detach_before {
        tree.update(cx, |tree, cx| {
            if owner.source.is_current(tree, cx) {
                tree.suppressed_subtrees.insert(path.clone());
                tree.detach_subtree(&path);
                cx.notify();
            }
        });
    }
    let failed_refresh_dir = refresh_dir.clone();
    let task = cx.background_executor().spawn(async move { op() });
    window
        .spawn(cx, async move |cx| {
            let result = task.await;
            let _ = cx.update(|window, cx| match result {
                Ok(summary) => {
                    let operation_owned = tree.update(cx, |tree, cx| {
                        if !tree.operations.finish(&owner, true) {
                            return false;
                        }
                        let same_source = owner.source.is_current(tree, cx);
                        tree.operation_label = None;
                        tree.active_operation_suppressed_path = None;
                        if let Some(path) = suppressed_path.as_ref() {
                            if same_source {
                                tree.detach_subtree(path);
                            }
                            if same_source
                                && !expand
                                && let Some(project_id) = tree.current_project.clone()
                            {
                                let key = path.to_string_lossy().to_string();
                                tree.store.update(cx, |store, cx| {
                                    store.set_dir_expanded(&project_id, &key, false, cx)
                                });
                            }
                            tree.suppressed_subtrees.remove(path);
                        }
                        if same_source && let Some(refresh_dir) = refresh_dir {
                            if expand {
                                tree.ensure_expanded(refresh_dir, cx);
                            } else {
                                tree.reload_dir(refresh_dir, cx);
                            }
                        }
                        if !same_source {
                            tree.reconcile_operation_source(&owner.source, cx);
                        }
                        cx.notify();
                        same_source
                    });
                    if operation_owned && let Some(summary) = summary {
                        show_alert(
                            t("fileTree", "operation.completeTitle"),
                            summary,
                            window,
                            cx,
                        );
                    }
                }
                Err(err) => {
                    eprintln!("[files] 操作失败: {err}");
                    let operation_owned = tree.update(cx, |tree, cx| {
                        if !tree.operations.finish(&owner, true) {
                            return false;
                        }
                        let same_source = owner.source.is_current(tree, cx);
                        tree.operation_label = None;
                        tree.active_operation_suppressed_path = None;
                        if let Some(path) = suppressed_path.as_ref() {
                            if same_source {
                                tree.detach_subtree(path);
                            }
                            tree.suppressed_subtrees.remove(path);
                        }
                        if same_source && let Some(failed_refresh_dir) = failed_refresh_dir {
                            tree.reload_dir(failed_refresh_dir, cx);
                        }
                        if !same_source {
                            tree.reconcile_operation_source(&owner.source, cx);
                        }
                        cx.notify();
                        same_source
                    });
                    if operation_owned {
                        show_alert(
                            t("fileTree", "operation.failedTitle"),
                            tr!("fileTree", "operation.failedMessage", error = err),
                            window,
                            cx,
                        );
                    }
                }
            });
        })
        .detach();
    true
}

fn operation_summary(summary: &crate::remote_ssh::FileOperationSummary) -> Option<String> {
    let mut text = tr!(
        "fileTree",
        "operation.summary",
        completed = summary.completed,
        skipped = summary.skipped,
        failed = summary.failed
    );
    if !summary.warnings.is_empty() {
        text.push_str("\n\n");
        text.push_str(&summary.warnings.join("\n"));
    }
    Some(text)
}

pub(super) fn copy_to_file_clipboard(
    tree: &Entity<FileTree>,
    target: &FileTreeContextTarget,
    cx: &mut App,
) {
    tree.update(cx, |tree, cx| {
        if !target.is_current(tree, cx) {
            return;
        }
        let Some(row) = target.row() else {
            return;
        };
        let context = &target.context;
        tree.file_clipboard = Some(FileTreeClipboard {
            entry: FileClipboardEntry {
                project_id: context.project_id.clone(),
                root: context.root.clone(),
                backend: context.backend.clone(),
                generation: context.generation,
                source: row.path.clone(),
                is_dir: row.is_dir,
            },
            owner: target.clone(),
        });
        cx.notify();
    });
}

pub(super) fn paste_file_clipboard(
    tree: Entity<FileTree>,
    target: FileTreeContextTarget,
    window: &mut Window,
    cx: &mut App,
) {
    let prepared = target
        .is_current(tree.read(cx), cx)
        .then(|| {
            let clipboard = tree.read(cx).file_clipboard.clone()?;
            (clipboard.owner.is_current(tree.read(cx), cx) && clipboard.can_paste_into(&target))
                .then_some((target.context.clone(), clipboard.entry, target.directory()?))
        })
        .flatten();
    let Some((context, clipboard, target_dir)) = prepared else {
        show_alert(
            t("fileTree", "clipboard.unavailableTitle"),
            t("fileTree", "clipboard.unavailableMessage"),
            window,
            cx,
        );
        return;
    };
    if clipboard.would_copy_into_itself(&target_dir) {
        show_alert(
            t("fileTree", "clipboard.recursiveTitle"),
            t("fileTree", "clipboard.recursiveMessage"),
            window,
            cx,
        );
        return;
    }
    match &context.backend {
        FileBackendIdentity::Local => {
            let Some(source_name) = clipboard.source.file_name() else {
                return;
            };
            let root = context.root.clone();
            let source = clipboard.source.clone();
            let destination = target_dir.join(source_name);
            spawn_tree_op(
                tree,
                target,
                None,
                Some(target_dir),
                true,
                None,
                t("fileTree", "operation.copying").into(),
                move || {
                    mt_project::fs::copy_entry(
                        &root,
                        &source,
                        &destination,
                        mt_project::fs::CopyConflictPolicy::KeepBoth,
                    )
                    .map(|_| None)
                    .map_err(|e| format!("{e:#}"))
                },
                window,
                cx,
            );
        }
        FileBackendIdentity::Remote { .. } => {
            let Some(conn) = tree.read(cx).remote_conn(cx) else {
                return;
            };
            let root = context.root.clone();
            let source = clipboard.source.clone();
            let operation_dir = target_dir.clone();
            let operation_target = target.clone();
            spawn_tree_op(
                tree,
                target,
                None,
                Some(target_dir),
                true,
                None,
                t("fileTree", "operation.copying").into(),
                move || {
                    crate::remote_ssh::copy_entry_keep_both_at_epoch(
                        &conn,
                        operation_target.remote_epoch(&conn)?,
                        remote_path_text(&root)?,
                        remote_path_text(&source)?,
                        remote_path_text(&operation_dir)?,
                    )
                    .map(|(_, summary)| operation_summary(&summary))
                },
                window,
                cx,
            );
        }
        FileBackendIdentity::BrokenRemote => {}
    }
}

pub(super) fn open_entry_in_terminal(
    tree: Entity<FileTree>,
    store: Entity<AppStore>,
    target: FileTreeContextTarget,
    window: &mut Window,
    cx: &mut App,
) {
    if !target.is_current(tree.read(cx), cx) {
        return;
    }
    let Some(cwd_path) = target.directory() else {
        return;
    };
    let context = target.context;
    let cwd = (cwd_path != context.root).then(|| cwd_path.to_string_lossy().into_owned());
    let opened = store.update(cx, |store, cx| {
        if store.active_project_id.as_deref() != Some(context.project_id.as_str()) {
            return false;
        }
        store
            .new_terminal_with_cwd(&context.project_id, None, None, cwd, window, cx)
            .is_some()
    });
    if opened {
        crate::workbench_area::activate_terminal_page(window, cx);
    }
}

#[allow(clippy::too_many_arguments)]
fn run_upload(
    tree: Entity<FileTree>,
    target: FileTreeContextTarget,
    owner: FileTreeOperationOwner,
    conn: mt_config::SshConnection,
    local_paths: Vec<PathBuf>,
    strategy: crate::remote_ssh::FileConflictStrategy,
    window: &mut Window,
    cx: &mut App,
) {
    if !target.is_current(tree.read(cx), cx) {
        finish_tree_preflight(&tree, &owner, &target, cx);
        return;
    }
    let (expected_epoch, target_dir) = match prepare_upload_target(&target, &conn) {
        Ok(destination) => destination,
        Err(error) => {
            if finish_tree_preflight(&tree, &owner, &target, cx) == Some(true) {
                show_alert(t("fileTree", "operation.failedTitle"), error, window, cx);
            }
            return;
        }
    };
    let root = target.context.root.clone();
    let operation_dir = target_dir.clone();
    let detach_before = target_dir.clone();
    spawn_tree_op(
        tree,
        target,
        Some(owner),
        Some(target_dir),
        true,
        Some(detach_before),
        t("fileTree", "operation.uploading").into(),
        move || {
            crate::remote_ssh::upload_paths_at_epoch(
                &conn,
                expected_epoch,
                remote_path_text(&root)?,
                remote_path_text(&operation_dir)?,
                &local_paths,
                strategy,
            )
            .map(|summary| operation_summary(&summary))
        },
        window,
        cx,
    );
}

pub(super) fn prepare_upload_target(
    target: &FileTreeContextTarget,
    conn: &mt_config::SshConnection,
) -> Result<(u64, PathBuf), String> {
    let epoch = target.remote_epoch(conn)?;
    let directory = target
        .directory()
        .ok_or_else(|| t("fileTree", "operation.invalidTarget").to_string())?;
    remote_path_text(&target.context.root)?;
    remote_path_text(&directory)?;
    Ok((epoch, directory))
}

pub(super) fn start_upload(
    tree: Entity<FileTree>,
    target: FileTreeContextTarget,
    local_paths: Vec<PathBuf>,
    window: &mut Window,
    cx: &mut App,
) {
    if local_paths.is_empty() {
        return;
    }
    if !target.is_current(tree.read(cx), cx) {
        return;
    }
    let context = target.context.clone();
    let FileBackendIdentity::Remote { .. } = &context.backend else {
        return;
    };
    let Some(conn) = tree.read(cx).remote_conn(cx) else {
        return;
    };
    let (expected_epoch, target_dir) = match prepare_upload_target(&target, &conn) {
        Ok(destination) => destination,
        Err(error) => {
            show_alert(t("fileTree", "operation.failedTitle"), error, window, cx);
            return;
        }
    };
    let Some(owner) = begin_tree_preflight(
        &tree,
        &target,
        t("fileTree", "operation.checkingConflicts").into(),
        window,
        cx,
    ) else {
        return;
    };
    let root = context.root.clone();
    let scan_target = target_dir;
    let scan_paths = local_paths.clone();
    let task = cx.background_executor().spawn(async move {
        crate::remote_ssh::upload_conflicts_at_epoch(
            &conn,
            expected_epoch,
            remote_path_text(&root)?,
            remote_path_text(&scan_target)?,
            &scan_paths,
        )
        .map(|conflicts| (conn, conflicts))
    });
    window
        .spawn(cx, async move |cx| {
            let result = task.await;
            let _ = cx.update(|window, cx| {
                if !target.is_current(tree.read(cx), cx) {
                    finish_tree_preflight(&tree, &owner, &target, cx);
                    return;
                }
                match result {
                    Ok((conn, conflicts)) if conflicts.is_empty() => {
                        run_upload(
                            tree.clone(),
                            target.clone(),
                            owner.clone(),
                            conn,
                            local_paths.clone(),
                            crate::remote_ssh::FileConflictStrategy::KeepBoth,
                            window,
                            cx,
                        );
                    }
                    Ok((conn, conflicts)) => {
                        if !retain_tree_preflight_for_choice(&tree, &owner, &target, cx) {
                            return;
                        }
                        let choice_tree = tree.clone();
                        let cancel_tree = tree.clone();
                        let cancel_owner = owner.clone();
                        let cancel_target = target.clone();
                        show_file_conflict_choice(
                            conflicts,
                            move |strategy, window, cx| {
                                run_upload(
                                    choice_tree.clone(),
                                    target.clone(),
                                    owner.clone(),
                                    conn.clone(),
                                    local_paths.clone(),
                                    strategy,
                                    window,
                                    cx,
                                );
                            },
                            move |_window, cx| {
                                finish_tree_preflight(
                                    &cancel_tree,
                                    &cancel_owner,
                                    &cancel_target,
                                    cx,
                                );
                            },
                            window,
                            cx,
                        );
                    }
                    Err(error) => {
                        if finish_tree_preflight(&tree, &owner, &target, cx) == Some(true) {
                            show_alert(
                                t("fileTree", "operation.failedTitle"),
                                tr!("fileTree", "operation.failedMessage", error = error),
                                window,
                                cx,
                            );
                        }
                    }
                }
            });
        })
        .detach();
}

#[allow(clippy::too_many_arguments)]
fn run_download(
    tree: Entity<FileTree>,
    target: FileTreeContextTarget,
    owner: FileTreeOperationOwner,
    conn: mt_config::SshConnection,
    remote_paths: Vec<PathBuf>,
    download_dir: PathBuf,
    strategy: crate::remote_ssh::FileConflictStrategy,
    window: &mut Window,
    cx: &mut App,
) {
    if !target.is_current(tree.read(cx), cx) {
        finish_tree_preflight(&tree, &owner, &target, cx);
        return;
    }
    let expected_epoch = match target.remote_epoch(&conn) {
        Ok(epoch) => epoch,
        Err(error) => {
            if finish_tree_preflight(&tree, &owner, &target, cx) == Some(true) {
                show_alert(t("fileTree", "operation.failedTitle"), error, window, cx);
            }
            return;
        }
    };
    let root = target.context.root.clone();
    spawn_tree_op(
        tree,
        target,
        Some(owner),
        None,
        false,
        None,
        t("fileTree", "operation.downloading").into(),
        move || {
            for path in &remote_paths {
                remote_path_text(path)?;
            }
            crate::remote_ssh::download_entries_at_epoch(
                &conn,
                expected_epoch,
                remote_path_text(&root)?,
                &remote_paths,
                &download_dir,
                strategy,
            )
            .map(|summary| {
                let mut message = operation_summary(&summary).unwrap_or_default();
                message.push_str("\n\n");
                message.push_str(&tr!(
                    "fileTree",
                    "operation.downloadLocation",
                    path = download_dir.display()
                ));
                Some(message)
            })
        },
        window,
        cx,
    );
}

pub(super) fn start_download(
    tree: Entity<FileTree>,
    target: FileTreeContextTarget,
    remote_paths: Vec<PathBuf>,
    window: &mut Window,
    cx: &mut App,
) {
    if !target.is_current(tree.read(cx), cx) {
        return;
    }
    let FileBackendIdentity::Remote { .. } = &target.context.backend else {
        return;
    };
    let Some(conn) = tree.read(cx).remote_conn(cx) else {
        return;
    };
    let store = tree.read(cx).store.clone();
    let download_dir = match store.read(cx).config().resolved_download_dir() {
        Ok(path) => path,
        Err(error) => {
            show_alert(
                t("fileTree", "download.directoryErrorTitle"),
                format!("{error:#}"),
                window,
                cx,
            );
            return;
        }
    };
    let Some(owner) = begin_tree_preflight(
        &tree,
        &target,
        t("fileTree", "operation.checkingConflicts").into(),
        window,
        cx,
    ) else {
        return;
    };
    let scan_dir = download_dir.clone();
    let scan_paths = remote_paths.clone();
    let task = cx.background_executor().spawn(async move {
        crate::remote_ssh::download_conflicts_for_files(&scan_dir, &scan_paths)
    });
    window
        .spawn(cx, async move |cx| {
            let result = task.await;
            let _ = cx.update(|window, cx| match result {
                Ok(conflicts) if conflicts.is_empty() => {
                    run_download(
                        tree,
                        target,
                        owner,
                        conn,
                        remote_paths,
                        download_dir,
                        crate::remote_ssh::FileConflictStrategy::KeepBoth,
                        window,
                        cx,
                    );
                }
                Ok(conflicts) => {
                    if !retain_tree_preflight_for_choice(&tree, &owner, &target, cx) {
                        return;
                    }
                    let choice_tree = tree.clone();
                    let cancel_tree = tree.clone();
                    let cancel_owner = owner.clone();
                    let cancel_target = target.clone();
                    show_file_conflict_choice(
                        conflicts,
                        move |strategy, window, cx| {
                            run_download(
                                choice_tree.clone(),
                                target.clone(),
                                owner.clone(),
                                conn.clone(),
                                remote_paths.clone(),
                                download_dir.clone(),
                                strategy,
                                window,
                                cx,
                            );
                        },
                        move |_window, cx| {
                            finish_tree_preflight(&cancel_tree, &cancel_owner, &cancel_target, cx);
                        },
                        window,
                        cx,
                    );
                }
                Err(error) => {
                    if finish_tree_preflight(&tree, &owner, &target, cx) == Some(true) {
                        show_alert(
                            t("fileTree", "operation.failedTitle"),
                            tr!("fileTree", "operation.failedMessage", error = error),
                            window,
                            cx,
                        );
                    }
                }
            });
        })
        .detach();
}

/// 「新建文件 / 新建文件夹」:问名字 → 建 → 展开父目录并重列。
pub(super) fn new_entry_prompt(
    tree: Entity<FileTree>,
    target: FileTreeContextTarget,
    connection: Option<mt_config::SshConnection>,
    is_dir: bool,
    window: &mut Window,
    cx: &mut App,
) {
    if !target.is_current(tree.read(cx), cx) {
        return;
    }
    let Some(dir) = target.directory() else {
        return;
    };
    let (title, message) = if is_dir {
        (
            t("fileTree", "prompt.newFolderTitle"),
            t("fileTree", "prompt.newFolderMessage"),
        )
    } else {
        (
            t("fileTree", "prompt.newFileTitle"),
            t("fileTree", "prompt.newFileMessage"),
        )
    };
    show_prompt(
        title,
        message,
        "",
        move |value, window, cx| {
            if !target.is_current(tree.read(cx), cx) {
                return;
            }
            let name = value.trim().to_string();
            if name.is_empty() {
                return;
            }
            let operation_target = target.clone();
            let connection = connection.clone();
            spawn_tree_op(
                tree.clone(),
                target.clone(),
                None,
                Some(dir.clone()),
                true,
                None,
                t("fileTree", "operation.creating").into(),
                move || {
                    create_entry_at_target(&operation_target, connection.as_ref(), &name, is_dir)
                },
                window,
                cx,
            );
        },
        window,
        cx,
    );
}

pub(super) fn create_entry_at_target(
    target: &FileTreeContextTarget,
    connection: Option<&mt_config::SshConnection>,
    name: &str,
    is_dir: bool,
) -> Result<Option<String>, String> {
    if name.is_empty() || matches!(name, "." | "..") || name.contains(['/', '\\', ':', '\0']) {
        return Err(t("fileTree", "operation.invalidName").to_string());
    }
    let dir = target
        .directory()
        .ok_or_else(|| t("fileTree", "operation.invalidTarget").to_string())?;
    let context = &target.context;
    match (&context.backend, connection) {
        (FileBackendIdentity::Local, None) => {
            let path = dir.join(name);
            if is_dir {
                mt_project::fs::create_directory(&context.root, &path)
            } else {
                mt_project::fs::create_file(&context.root, &path)
            }
            .map(|_| None)
            .map_err(|error| format!("{error:#}"))
        }
        (
            FileBackendIdentity::Remote {
                connection_id,
                connection_fingerprint,
            },
            Some(connection),
        ) if connection.id == *connection_id
            && crate::remote_ssh::connection_fingerprint(connection) == *connection_fingerprint =>
        {
            crate::remote_ssh::create_entry_at_epoch(
                connection,
                target.remote_epoch(connection)?,
                remote_path_text(&context.root)?,
                remote_path_text(&dir)?,
                name,
                is_dir,
            )
            .map(|_| None)
        }
        _ => Err(t("fileTree", "operation.sourceUnavailable").to_string()),
    }
}
