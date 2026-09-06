//! FileTree row and blank-area menus, bound to the captured source and target.

use gpui::{App, ClipboardItem, Entity, Window};

use crate::file_ops::FileBackendIdentity;
use crate::fs_ops;
use crate::i18n::{t, tr};
use crate::menu::{self, MenuEntry, MenuItem};
use crate::prompt::{Confirm, show_prompt};
use crate::store::AppStore;

use super::ops::{
    copy_to_file_clipboard, new_entry_prompt, open_entry_in_terminal, paste_file_clipboard,
    spawn_tree_op, start_download,
};
use super::{FileTree, FileTreeContextTarget, remote_path_text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FileMenuAction {
    OpenWithDefault,
    CopyEntry,
    Paste,
    Download,
    CopyRelativePath,
    CopyAbsolutePath,
    RevealInFolder,
    OpenInTerminal,
    Rename,
    Delete,
    NewFile,
    NewFolder,
    ViewDiff,
}

impl FileMenuAction {
    fn label_key(self) -> &'static str {
        match self {
            Self::OpenWithDefault => "menu.openWithDefault",
            Self::CopyEntry => "menu.copy",
            Self::Paste => "menu.paste",
            Self::Download => "menu.download",
            Self::CopyRelativePath => "menu.copyRelativePath",
            Self::CopyAbsolutePath => "menu.copyAbsolutePath",
            Self::RevealInFolder => "menu.revealInFolder",
            Self::OpenInTerminal => "menu.openInTerminal",
            Self::Rename => "menu.rename",
            Self::Delete => "menu.delete",
            Self::NewFile => "menu.newFile",
            Self::NewFolder => "menu.newFolder",
            Self::ViewDiff => "menu.viewDiff",
        }
    }
}

pub(super) fn file_menu_actions(target: &FileTreeContextTarget) -> Vec<Option<FileMenuAction>> {
    use FileMenuAction::*;
    if matches!(&target.context.backend, FileBackendIdentity::BrokenRemote) {
        return Vec::new();
    }
    let Some(row) = target.row() else {
        return vec![Some(NewFile), Some(NewFolder)];
    };
    let remote = matches!(&target.context.backend, FileBackendIdentity::Remote { .. });
    let mut actions = Vec::new();
    if !remote && !row.is_dir {
        actions.push(Some(OpenWithDefault));
    }
    actions.push(Some(CopyEntry));
    if row.is_dir {
        actions.push(Some(Paste));
    }
    if remote {
        actions.push(Some(Download));
    }
    actions.extend([None, Some(CopyRelativePath), Some(CopyAbsolutePath)]);
    if !remote {
        actions.push(Some(RevealInFolder));
    }
    actions.extend([
        Some(OpenInTerminal),
        None,
        Some(Rename),
        Some(Delete),
        None,
        Some(NewFile),
        Some(NewFolder),
    ]);
    if !remote && !row.is_dir && row.git.is_some() {
        actions.extend([None, Some(ViewDiff)]);
    }
    actions
}

pub(super) fn open_rename_prompt(
    tree: Entity<FileTree>,
    target: FileTreeContextTarget,
    connection: Option<mt_config::SshConnection>,
    window: &mut Window,
    cx: &mut App,
) {
    if !target.is_current(tree.read(cx), cx) {
        return;
    }
    let Some(row) = target.row() else {
        return;
    };
    let Some(parent) = target.parent_directory(&row.path) else {
        return;
    };
    let path = row.path.clone();
    let detach_before = row.is_dir.then(|| path.clone());
    let old_name = row.name.clone();
    show_prompt(
        t("fileTree", "prompt.renameTitle"),
        t("fileTree", "prompt.renameMessage"),
        old_name.clone(),
        move |value, window, cx| {
            if !target.is_current(tree.read(cx), cx) {
                return;
            }
            let new_name = value.trim().to_string();
            if new_name.is_empty() || new_name == old_name {
                return;
            }
            let root = target.context.root.clone();
            let path = path.clone();
            let connection = connection.clone();
            let operation_target = target.clone();
            spawn_tree_op(
                tree.clone(),
                target.clone(),
                None,
                Some(parent.clone()),
                false,
                detach_before.clone(),
                t("fileTree", "operation.renaming").into(),
                move || match (&operation_target.context.backend, connection) {
                    (FileBackendIdentity::Remote { .. }, Some(conn)) => crate::remote_ssh::rename_entry_at_epoch(
                        &conn,
                        operation_target.remote_epoch(&conn)?,
                        remote_path_text(&root)?,
                        remote_path_text(&path)?,
                        &new_name,
                    )
                    .map(|_| None),
                    (FileBackendIdentity::Local, None) => mt_project::fs::rename_entry(&root, &path, &new_name)
                        .map(|_| None)
                        .map_err(|e| format!("{e:#}")),
                    _ => Err(t("fileTree", "operation.sourceUnavailable").to_string()),
                },
                window,
                cx,
            );
        },
        window,
        cx,
    );
}

pub(super) fn file_menu(
    tree: &Entity<FileTree>,
    store: &Entity<AppStore>,
    target: FileTreeContextTarget,
    connection: Option<mt_config::SshConnection>,
    can_paste: bool,
) -> Vec<MenuEntry> {
    file_menu_actions(&target)
        .into_iter()
        .map(|action| {
            let Some(action) = action else {
                return menu::separator();
            };
            let tree = tree.clone();
            let store = store.clone();
            let target = target.clone();
            let connection = connection.clone();
            let mut item = MenuItem::new(t("fileTree", action.label_key()))
                .disabled(action == FileMenuAction::Paste && !can_paste);
            if action == FileMenuAction::Delete {
                item = item.danger();
            }
            item.on_click(move |window, cx| {
                if !target.is_current(tree.read(cx), cx) {
                    return;
                }
                if matches!(action, FileMenuAction::NewFile | FileMenuAction::NewFolder) {
                    new_entry_prompt(
                        tree.clone(),
                        target.clone(),
                        connection.clone(),
                        action == FileMenuAction::NewFolder,
                        window,
                        cx,
                    );
                    return;
                }
                let Some(row) = target.row() else {
                    return;
                };
                let path = row.path.clone();
                let context = target.context.clone();
                let root = context.root.clone();
                match action {
                    FileMenuAction::OpenWithDefault => {
                        cx.background_executor()
                            .spawn(async move {
                                if let Err(error) =
                                    mt_project::editor::open_path_with_default_app(&path)
                                {
                                    eprintln!("[files] Open with default app failed: {error:#}");
                                }
                            })
                            .detach();
                    }
                    FileMenuAction::CopyEntry => {
                        copy_to_file_clipboard(&tree, &target, cx);
                    }
                    FileMenuAction::Paste => {
                        paste_file_clipboard(tree.clone(), target.clone(), window, cx);
                    }
                    FileMenuAction::Download => {
                        start_download(tree.clone(), target.clone(), vec![path], window, cx);
                    }
                    FileMenuAction::CopyRelativePath => {
                        let relative = match &context.backend {
                            FileBackendIdentity::Remote { .. } => crate::remote_ssh::posix_relative(
                                &root.to_string_lossy(),
                                &path.to_string_lossy(),
                            )
                            .unwrap_or_default(),
                            _ => {
                                fs_ops::relative_path(&path.to_string_lossy(), &root.to_string_lossy())
                            }
                        };
                        cx.write_to_clipboard(ClipboardItem::new_string(relative));
                    }
                    FileMenuAction::CopyAbsolutePath => {
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            path.to_string_lossy().into_owned(),
                        ));
                    }
                    FileMenuAction::RevealInFolder => {
                        cx.background_executor()
                            .spawn(async move {
                                if let Err(error) = fs_ops::reveal_in_file_manager(&path) {
                                    eprintln!("[files] Reveal in file manager failed: {error}");
                                }
                            })
                            .detach();
                    }
                    FileMenuAction::OpenInTerminal => {
                        open_entry_in_terminal(
                            tree.clone(),
                            store.clone(),
                            target.clone(),
                            window,
                            cx,
                        );
                    }
                    FileMenuAction::Rename => {
                        open_rename_prompt(
                            tree.clone(),
                            target.clone(),
                            connection.clone(),
                            window,
                            cx,
                        );
                    }
                    FileMenuAction::Delete => {
                        let Some(parent) = target.parent_directory(&path) else {
                            return;
                        };
                        let (title, message) = if row.is_dir {
                            (
                                t("fileTree", "dialog.deleteFolderTitle"),
                                tr!("fileTree", "dialog.deleteConfirmFolder", name = row.name),
                            )
                        } else {
                            (
                                t("fileTree", "dialog.deleteFileTitle"),
                                tr!("fileTree", "dialog.deleteConfirmFile", name = row.name),
                            )
                        };
                        let tree = tree.clone();
                        let target = target.clone();
                        let connection = connection.clone();
                        Confirm::new(title, message)
                            .ok_text(t("fileTree", "dialog.deleteOk"))
                            .cancel_text(t("fileTree", "dialog.deleteCancel"))
                            .open(
                                move |window, cx| {
                                    if !target.is_current(tree.read(cx), cx) {
                                        return;
                                    }
                                    let root = root.clone();
                                    let operation_path = path.clone();
                                    let connection = connection.clone();
                                    let operation_target = target.clone();
                                    spawn_tree_op(
                                        tree.clone(),
                                        target.clone(),
                                        None,
                                        Some(parent.clone()),
                                        false,
                                        Some(path.clone()),
                                        t("fileTree", "operation.deleting").into(),
                                        move || match (&operation_target.context.backend, connection) {
                                            (FileBackendIdentity::Remote { .. }, Some(conn)) => crate::remote_ssh::delete_entry_at_epoch(
                                                &conn,
                                                operation_target.remote_epoch(&conn)?,
                                                remote_path_text(&root)?,
                                                remote_path_text(&operation_path)?,
                                            )
                                            .map(|_| None),
                                            (FileBackendIdentity::Local, None) => {
                                                mt_project::fs::delete_entry(&root, &operation_path)
                                                    .map(|_| None)
                                                    .map_err(|e| format!("{e:#}"))
                                            }
                                            _ => Err(t("fileTree", "operation.sourceUnavailable").to_string()),
                                        },
                                        window,
                                        cx,
                                    );
                                },
                                window,
                                cx,
                            );
                    }
                    FileMenuAction::ViewDiff => {
                        crate::git_diff::open_file_diff(
                            store.clone(),
                            root.to_string_lossy().into_owned(),
                            row.rel.clone(),
                            false,
                            row.git
                                .as_ref()
                                .map(|(label, _)| label.clone())
                                .unwrap_or_default(),
                            window,
                            cx,
                        );
                    }
                    FileMenuAction::NewFile | FileMenuAction::NewFolder => {}
                }
            })
            .into()
        })
        .collect()
}
