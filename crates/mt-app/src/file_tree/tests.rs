use super::menu::FileMenuAction::*;
use super::menu::file_menu_actions;
use super::*;

fn worktree(hex: char) -> WorktreeId {
    format!("worktree-v1:{}", hex.to_string().repeat(64))
        .parse()
        .unwrap()
}

fn context_row(path: &str, is_dir: bool) -> Row {
    Row {
        listing_directory: PathBuf::from(crate::remote_ssh::parent_posix(path).unwrap_or_else(|| "/work".into())),
        name: path.rsplit('/').next().unwrap().to_string(),
        path: PathBuf::from(path),
        is_dir,
        ignored: false,
        depth: 0,
        expanded: false,
        rel: path.strip_prefix("/work/").unwrap_or(path).to_string(),
        git: None,
        kind: None,
    }
}

fn context_target(entry: FileTreeContextEntry, remote: bool) -> FileTreeContextTarget {
    let directory = match &entry {
        FileTreeContextEntry::Row(row) => row.listing_directory.clone(),
        FileTreeContextEntry::Blank => PathBuf::from("/work"),
    };
    FileTreeContextTarget {
        context: FileOperationContext {
            project_id: "project-a".into(),
            root: PathBuf::from("/work"),
            backend: if remote {
                FileBackendIdentity::Remote {
                    connection_id: "ssh-a".into(),
                    connection_fingerprint: 7,
                }
            } else {
                FileBackendIdentity::Local
            },
            generation: 3,
        },
        worktree_id: Some(worktree('a')),
        connection_epoch: remote.then_some(11),
        listing: Some((directory, 1)),
        entry,
    }
}

fn menu_target(is_dir: bool, has_git_status: bool, remote: bool) -> FileTreeContextTarget {
    let mut row = context_row(
        if is_dir { "/work/src" } else { "/work/src/main.rs" },
        is_dir,
    );
    row.git = has_git_status.then(|| ("M".into(), is_dir));
    context_target(FileTreeContextEntry::Row(row), remote)
}

#[test]
fn 文件行单击预览双击重命名() {
    assert_eq!(row_click_action(false, 1), RowClickAction::OpenPreview);
    assert_eq!(row_click_action(false, 2), RowClickAction::Rename);
    assert_eq!(row_click_action(false, 3), RowClickAction::None);
}

#[test]
fn 目录单击展开双击重命名() {
    assert_eq!(row_click_action(true, 1), RowClickAction::ToggleDirectory);
    assert_eq!(row_click_action(true, 2), RowClickAction::Rename);
    assert_eq!(row_click_action(true, 3), RowClickAction::None);
}

#[test]
fn 同路径工作树恢复各自文件状态选择与滚动() {
    let worktree_a = worktree('a');
    let worktree_b = worktree('b');
    let root = PathBuf::from("/repo/shared");
    let mut cache = HashMap::new();

    let mut state_a = FileTreeScopeState::empty();
    state_a.entries.insert(
        root.clone(),
        vec![entry("a.rs", "/repo/shared/a.rs", false, false)],
    );
    state_a.git_status.insert("a.rs".into(), "M".into());
    state_a.selected_path = Some(PathBuf::from("/repo/shared/a.rs"));
    state_a.root_error = Some("last known warning".into());
    state_a.scroll.set_offset(gpui::point(px(0.0), px(-72.0)));

    let (mut state_b, cached_b) =
        swap_file_tree_scope(&mut cache, Some(&worktree_a), Some(&worktree_b), state_a);
    assert!(!cached_b);
    state_b.entries.insert(
        root.clone(),
        vec![entry("b.rs", "/repo/shared/b.rs", false, false)],
    );
    state_b.selected_path = Some(PathBuf::from("/repo/shared/b.rs"));
    state_b.git_status.insert("b.rs".into(), "A".into());
    state_b.root_error = Some("worktree B warning".into());
    state_b.scroll.set_offset(gpui::point(px(0.0), px(-144.0)));

    let (restored_a, cached_a) =
        swap_file_tree_scope(&mut cache, Some(&worktree_b), Some(&worktree_a), state_b);
    assert!(cached_a);
    assert_eq!(restored_a.entries[&root][0].name, "a.rs");
    assert_eq!(
        restored_a.git_status.get("a.rs").map(String::as_str),
        Some("M")
    );
    assert_eq!(
        restored_a.selected_path.as_deref(),
        Some(Path::new("/repo/shared/a.rs"))
    );
    assert_eq!(restored_a.root_error.as_deref(), Some("last known warning"));
    assert_eq!(restored_a.scroll.offset().y, px(-72.0));

    let (restored_b, cached_b) =
        swap_file_tree_scope(&mut cache, Some(&worktree_a), Some(&worktree_b), restored_a);
    assert!(cached_b);
    assert_eq!(restored_b.entries[&root][0].name, "b.rs");
    assert_eq!(restored_b.git_status.get("b.rs").map(String::as_str), Some("A"));
    assert_eq!(
        restored_b.selected_path.as_deref(),
        Some(Path::new("/repo/shared/b.rs"))
    );
    assert_eq!(restored_b.root_error.as_deref(), Some("worktree B warning"));
    assert_eq!(restored_b.scroll.offset().y, px(-144.0));
}

#[test]
fn 同一工作树切换项目别名保留文件展示状态() {
    let worktree = worktree('a');
    let root = PathBuf::from("/repo/shared");
    let selected = PathBuf::from("/repo/shared/lib.rs");
    let mut cache = HashMap::new();
    let mut state = FileTreeScopeState::empty();
    state.entries.insert(
        root.clone(),
        vec![entry("lib.rs", "/repo/shared/lib.rs", false, false)],
    );
    state.selected_path = Some(selected.clone());
    state.root_error = Some("last known warning".into());
    state.scroll.set_offset(gpui::point(px(0.0), px(-96.0)));

    let (preserved, had_presentation) =
        swap_file_tree_scope(&mut cache, Some(&worktree), Some(&worktree), state);

    assert!(had_presentation);
    assert!(cache.is_empty());
    assert_eq!(preserved.entries[&root][0].name, "lib.rs");
    assert_eq!(preserved.selected_path.as_deref(), Some(selected.as_path()));
    assert_eq!(preserved.root_error.as_deref(), Some("last known warning"));
    assert_eq!(preserved.scroll.offset().y, px(-96.0));
}

#[test]
fn 文件树作用域早退同时比较来源与工作树身份() {
    let worktree_a = worktree('a');
    let worktree_b = worktree('b');

    assert!(file_tree_scope_matches(
        Some(&worktree_a),
        Some("project-a|/repo/shared|local"),
        Some(&worktree_a),
        Some("project-a|/repo/shared|local"),
    ));
    assert!(!file_tree_scope_matches(
        Some(&worktree_a),
        Some("project-a|/repo/shared|local"),
        Some(&worktree_b),
        Some("project-a|/repo/shared|local"),
    ));
    assert!(!file_tree_scope_matches(
        Some(&worktree_a),
        Some("project-a|/repo/shared|local"),
        Some(&worktree_a),
        Some("project-b|/repo/shared|local"),
    ));
}

#[test]
fn 目录请求所有者拒绝同来源下的工作树切换() {
    let worktree_a = worktree('a');
    let worktree_b = worktree('b');
    let owner = DirectoryRequestOwner {
        source: context_target(FileTreeContextEntry::Blank, false).source(),
        source_signature: Some("project-a|/repo/shared|local".into()),
    };

    assert!(owner.matches(
        Some(&owner.source),
        Some("project-a|/repo/shared|local"),
        7,
        Some(7),
    ));
    assert_eq!(owner.source.worktree_id, Some(worktree_a));
    let mut rebound = owner.source.clone();
    rebound.worktree_id = Some(worktree_b);
    assert!(!owner.matches(
        Some(&rebound),
        Some("project-a|/repo/shared|local"),
        7,
        Some(7),
    ));
}

#[test]
fn watcher来源键拒绝工作树变化与_aba_旧世代() {
    let worktree_a = worktree('a');
    let worktree_b = worktree('b');
    let source = Some("project-a|/repo/shared|local");
    let first_a = watcher_source_key(source, Some(&worktree_a), 7).unwrap();
    let worktree_b_key = watcher_source_key(source, Some(&worktree_b), 8).unwrap();
    let second_a = watcher_source_key(source, Some(&worktree_a), 9).unwrap();

    assert_ne!(first_a, worktree_b_key);
    assert_ne!(first_a, second_a);
    assert!(watcher_event_matches(
        Some(second_a.as_str()),
        Some(Path::new("/repo/shared")),
        Some(second_a.as_str()),
        "/repo/shared",
    ));
    assert!(!watcher_event_matches(
        Some(second_a.as_str()),
        Some(Path::new("/repo/shared")),
        Some(first_a.as_str()),
        "/repo/shared",
    ));
}

fn listing_owner(remote: bool) -> DirectoryRequestOwner {
    DirectoryRequestOwner {
        source: context_target(FileTreeContextEntry::Blank, remote).source(),
        source_signature: Some("captured-source".into()),
    }
}

fn remote_listing_source() -> crate::remote_ssh::RemoteFileListingSource {
    crate::remote_ssh::RemoteFileListingSource {
        connection_id: "ssh-a".into(),
        connection_fingerprint: 7,
        connection_epoch: 11,
        project_root: "/work".into(),
        directory: "/work/src".into(),
    }
}

#[test]
fn listing_publication_requires_exact_request_source_and_generation() {
    let owner = listing_owner(true);
    let signature = owner.source_signature.as_deref();
    assert!(owner.matches(Some(&owner.source), signature, 41, Some(41)));
    assert!(!owner.matches(Some(&owner.source), signature, 41, Some(42)));
    assert!(!owner.matches(Some(&owner.source), signature, 41, None));
    assert!(!owner.matches(Some(&owner.source), Some("other"), 41, Some(41)));
    assert!(!owner.matches(None, signature, 41, Some(41)));
    for changed in [
        FileTreeSource { worktree_id: Some(worktree('b')), ..owner.source.clone() },
        FileTreeSource { connection_epoch: Some(12), ..owner.source.clone() },
        FileTreeSource {
            context: FileOperationContext { generation: 5, ..owner.source.context.clone() },
            ..owner.source.clone()
        },
        FileTreeSource {
            context: FileOperationContext { project_id: "project-b".into(), ..owner.source.context.clone() },
            ..owner.source.clone()
        },
        FileTreeSource {
            context: FileOperationContext { root: PathBuf::from("/other"), ..owner.source.context.clone() },
            ..owner.source.clone()
        },
    ] {
        assert!(!owner.matches(Some(&changed), signature, 41, Some(41)));
    }
}

#[test]
fn listing_provenance_uses_actual_producer_never_the_observed_replacement() {
    let owner = listing_owner(true);
    let producer = remote_listing_source();
    let directory = Path::new("/work/src");
    let accepted = owner.result_source(directory, Some(&producer), Some(11)).unwrap();
    assert_eq!(accepted, owner.source);
    for observed in [None, Some(12)] {
        assert!(owner.result_source(directory, Some(&producer), observed).is_none());
    }
    for changed in [
        crate::remote_ssh::RemoteFileListingSource { connection_id: "ssh-b".into(), ..producer.clone() },
        crate::remote_ssh::RemoteFileListingSource { connection_fingerprint: 8, ..producer.clone() },
        crate::remote_ssh::RemoteFileListingSource { connection_epoch: 12, ..producer.clone() },
        crate::remote_ssh::RemoteFileListingSource { project_root: "/other".into(), ..producer.clone() },
        crate::remote_ssh::RemoteFileListingSource { directory: "/work/other".into(), ..producer.clone() },
    ] {
        assert!(owner.result_source(directory, Some(&changed), Some(changed.connection_epoch)).is_none());
    }
    assert!(owner.result_source(directory, None, Some(11)).is_none());
    let local = listing_owner(false);
    assert_eq!(local.result_source(directory, None, None), Some(local.source.clone()));
    assert!(local.result_source(directory, Some(&producer), Some(11)).is_none());
    assert!(local.result_source(directory, None, Some(11)).is_none());
}

#[test]
fn only_bootstrap_read_can_adopt_its_producing_epoch() {
    let mut owner = listing_owner(true);
    owner.source.connection_epoch = None;
    let producer = remote_listing_source();
    let directory = Path::new("/work/src");
    let accepted = owner.result_source(directory, Some(&producer), Some(11)).unwrap();
    assert_eq!(accepted.connection_epoch, Some(11));
    assert_eq!(owner.source.connection_epoch, None);
    assert!(owner.result_source(directory, Some(&producer), Some(12)).is_none());
    let pinned = listing_owner(true);
    let replacement = crate::remote_ssh::RemoteFileListingSource { connection_epoch: 12, ..producer };
    assert!(pinned.result_source(directory, Some(&replacement), Some(12)).is_none());
}

#[test]
fn cached_rows_keep_original_provenance_across_aba_and_same_worktree_rebinding() {
    let original = listing_owner(true).source;
    let root = original.context.root.clone();
    let worktree_a = original.worktree_id.as_ref().unwrap();
    let worktree_b = worktree('b');
    let owner = DirectoryListingOwner { source: original.clone(), request_id: 41 };
    let mut state = FileTreeScopeState::empty();
    state.entries.insert(root.clone(), vec![entry("old.rs", "/work/old.rs", false, false)]);
    state.entry_sources.insert(root.clone(), owner.clone());
    let mut cache = HashMap::new();
    let (other, _) = swap_file_tree_scope(&mut cache, Some(worktree_a), Some(&worktree_b), state);
    let (restored, _) = swap_file_tree_scope(&mut cache, Some(&worktree_b), Some(worktree_a), other);
    let (preserved, _) = swap_file_tree_scope(&mut cache, Some(worktree_a), Some(worktree_a), restored);
    assert_eq!(preserved.entries[&root][0].name, "old.rs");
    assert_eq!(preserved.entry_sources[&root], owner);
    assert!(owner.matches(&original, 41));
    assert!(!owner.matches(&original, 42));
    let mut rebound = original.clone();
    rebound.context.generation += 2;
    assert!(!owner.matches(&rebound, 41));
    rebound = original.clone();
    rebound.connection_epoch = Some(12);
    assert!(!owner.matches(&rebound, 41));
    rebound = original;
    rebound.context.backend = FileBackendIdentity::Remote {
        connection_id: "ssh-a".into(),
        connection_fingerprint: 8,
    };
    assert!(!owner.matches(&rebound, 41));
}

#[test]
fn stale_expanded_cache_refreshes_its_parent_before_traversing_old_descendants() {
    let entries = listed(vec![
        ("/p", vec![entry("src", "/p/src", true, false)]),
        ("/p/src", vec![entry("old", "/p/src/old", true, false)]),
    ]);
    let mut missing = Vec::new();
    missing_expanded_dirs(
        &entries,
        Path::new("/p"),
        &expanded_set(&["/p/src", "/p/src/old"]),
        &|path| path == Path::new("/p"),
        &mut missing,
    );
    assert_eq!(missing, vec![PathBuf::from("/p/src")]);
}

#[test]
fn running_busy_owner_survives_scope_swaps_without_aba_presentation_authority() {
    let target = context_target(FileTreeContextEntry::Blank, true);
    let source = target.source();
    let mut operations = FileTreeOperations::default();
    let owner = operations.reserve(&target, FileTreeOperationPhase::Running).unwrap();
    let mut cache = HashMap::new();
    let other_worktree = worktree('b');
    let (other, _) = swap_file_tree_scope(
        &mut cache, source.worktree_id.as_ref(), Some(&other_worktree), FileTreeScopeState::empty(),
    );
    let mut rebound = source.clone();
    rebound.worktree_id = Some(other_worktree.clone());
    let mut rebound_target = target.clone();
    rebound_target.worktree_id = rebound.worktree_id.clone();
    assert!(operations.reserve(&rebound_target, FileTreeOperationPhase::Running).is_none());
    assert!(!owner.source.same_stable_source(&rebound));
    let _ = swap_file_tree_scope(&mut cache, Some(&other_worktree), source.worktree_id.as_ref(), other);
    rebound = source.clone();
    rebound.context.generation += 2;
    assert!(owner.source.same_stable_source(&rebound));
    assert!(!owner.source.matches(Some(&rebound.context), rebound.worktree_id.as_ref(), rebound.connection_epoch));
    rebound_target = target;
    rebound_target.context.generation = rebound.context.generation;
    assert!(operations.reserve(&rebound_target, FileTreeOperationPhase::Running).is_none());
    rebound.connection_epoch = Some(12);
    assert!(!owner.source.same_stable_source(&rebound));
    assert!(operations.finish(&owner, true));
    assert!(operations.active.is_none());
}

#[test]
fn download_choice_cancellation_cannot_release_running_or_successor_operation() {
    let target = menu_target(false, false, true);
    let mut operations = FileTreeOperations::default();
    let preflight = operations.reserve(&target, FileTreeOperationPhase::Preflight).unwrap();
    assert!(!operations.finish(&preflight, true));
    assert!(operations.transition(&preflight, FileTreeOperationPhase::Choice));
    assert!(!operations.transition(&preflight, FileTreeOperationPhase::Choice));
    assert!(operations.reserve(&target, FileTreeOperationPhase::Running).is_none());
    assert!(operations.transition(&preflight, FileTreeOperationPhase::Running));
    assert!(!operations.finish(&preflight, false));
    assert!(!operations.transition(&preflight, FileTreeOperationPhase::Running));
    assert!(operations.finish(&preflight, true));
    let successor = operations.reserve(&target, FileTreeOperationPhase::Preflight).unwrap();
    assert_ne!(successor.id, preflight.id);
    assert!(!operations.finish(&preflight, false));
    assert!(!operations.finish(&preflight, true));
    assert!(!operations.transition(&preflight, FileTreeOperationPhase::Running));
    assert_eq!(operations.active.as_ref().unwrap().0, successor);
    assert!(operations.finish(&successor, false));
    let no_conflicts = operations.reserve(&target, FileTreeOperationPhase::Preflight).unwrap();
    assert!(operations.transition(&no_conflicts, FileTreeOperationPhase::Running));
    assert!(!operations.finish(&successor, false));
    assert!(operations.finish(&no_conflicts, true));
}

#[test]
fn operation_ids_never_wrap_and_cannot_release_a_different_captured_source() {
    let target = context_target(FileTreeContextEntry::Blank, true);
    let mut operations = FileTreeOperations::default();
    let owner = operations.reserve(&target, FileTreeOperationPhase::Preflight).unwrap();
    let mut wrong_source = owner.clone();
    wrong_source.source.context.generation += 2;
    assert!(!operations.transition(&wrong_source, FileTreeOperationPhase::Choice));
    assert!(!operations.finish(&wrong_source, false));
    assert!(operations.finish(&owner, false));
    operations.next_id = u64::MAX;
    assert!(operations.reserve(&target, FileTreeOperationPhase::Preflight).is_none());
    assert!(operations.active.is_none());
}

#[test]
fn preflight_owner_requires_the_full_captured_target_at_choice_dispatch() {
    let target = menu_target(false, false, true);
    let mut operations = FileTreeOperations::default();
    let owner = operations.reserve(&target, FileTreeOperationPhase::Preflight).unwrap();
    assert!(owner.matches_target(&target));
    let mut changed = target.clone();
    changed.entry = FileTreeContextEntry::Blank;
    assert!(!owner.matches_target(&changed));
    changed = target.clone();
    changed.entry = FileTreeContextEntry::Row(context_row("/work/src/other.rs", false));
    assert!(!owner.matches_target(&changed));
    changed = target.clone();
    changed.listing.as_mut().unwrap().1 += 1;
    assert!(!owner.matches_target(&changed));
    changed = target.clone();
    changed.connection_epoch = Some(12);
    assert!(!owner.matches_target(&changed));
    changed = target.clone();
    changed.context.generation += 2;
    assert!(!owner.matches_target(&changed));
    assert!(operations.finish(&owner, false));
}

#[test]
fn remote_source_identity_compares_posix_text_without_host_path_normalization() {
    let mut source = listing_owner(true).source;
    source.context.root = PathBuf::from(r"/work/a\b");
    let mut different = source.clone();
    different.context.root = PathBuf::from("/work/a/b");
    assert_ne!(source, different);
    assert!(!source.same_stable_source(&different));
    assert!(!remote_download_context_matches(&source.context, "project-a", "/work/a/b", "ssh-a", 7));
}

#[test]
fn watcher事件同时校验注册来源与项目根() {
    let root = Path::new("/repo/shared");
    assert!(watcher_event_matches(
        Some("project-a|/repo/shared|local"),
        Some(root),
        Some("project-a|/repo/shared|local"),
        "/repo/shared",
    ));
    assert!(!watcher_event_matches(
        Some("project-b|/repo/shared|local"),
        Some(root),
        Some("project-a|/repo/shared|local"),
        "/repo/shared",
    ));
    assert!(!watcher_event_matches(
        Some("project-a|/repo/shared|local"),
        Some(root),
        Some("project-a|/repo/shared|local"),
        "/repo/other",
    ));
}

#[test]
fn 远程下载上下文要求项目根目录和连接身份完全一致() {
    let context = FileOperationContext {
        project_id: "project-a".into(),
        root: PathBuf::from("/workspace"),
        backend: FileBackendIdentity::Remote {
            connection_id: "ssh-a".into(),
            connection_fingerprint: 7,
        },
        generation: 3,
    };
    assert!(remote_download_context_matches(
        &context,
        "project-a",
        "/workspace",
        "ssh-a",
        7,
    ));
    assert!(!remote_download_context_matches(
        &context,
        "project-b",
        "/workspace",
        "ssh-a",
        7,
    ));
    assert!(!remote_download_context_matches(
        &context,
        "project-a",
        "/other",
        "ssh-a",
        7,
    ));
    assert!(!remote_download_context_matches(
        &context,
        "project-a",
        "/workspace",
        "ssh-b",
        7,
    ));
    assert!(!remote_download_context_matches(
        &context,
        "project-a",
        "/workspace",
        "ssh-a",
        8,
    ));

    for backend in [
        FileBackendIdentity::Local,
        FileBackendIdentity::BrokenRemote,
    ] {
        let context = FileOperationContext {
            backend,
            ..context.clone()
        };
        assert!(!remote_download_context_matches(
            &context,
            "project-a",
            "/workspace",
            "ssh-a",
            7,
        ));
    }
}

#[test]
fn file_menus_keep_applicable_actions_and_add_creation() {
    assert_eq!(
        file_menu_actions(&menu_target(false, true, false)),
        vec![
            Some(OpenWithDefault),
            Some(CopyEntry),
            None,
            Some(CopyRelativePath),
            Some(CopyAbsolutePath),
            Some(RevealInFolder),
            Some(OpenInTerminal),
            None,
            Some(Rename),
            Some(Delete),
            None,
            Some(NewFile),
            Some(NewFolder),
            None,
            Some(ViewDiff),
        ]
    );
    assert_eq!(
        file_menu_actions(&menu_target(false, false, false)),
        vec![
            Some(OpenWithDefault),
            Some(CopyEntry),
            None,
            Some(CopyRelativePath),
            Some(CopyAbsolutePath),
            Some(RevealInFolder),
            Some(OpenInTerminal),
            None,
            Some(Rename),
            Some(Delete),
            None,
            Some(NewFile),
            Some(NewFolder),
        ]
    );
}

/// 目录的菜单:没有「默认工具打开」,末尾多一段「新建文件 / 新建文件夹」。
#[test]
fn 目录菜单项序与原版一致() {
    assert_eq!(
        file_menu_actions(&menu_target(true, false, false)),
        vec![
            Some(CopyEntry),
            Some(Paste),
            None,
            Some(CopyRelativePath),
            Some(CopyAbsolutePath),
            Some(RevealInFolder),
            Some(OpenInTerminal),
            None,
            Some(Rename),
            Some(Delete),
            None,
            Some(NewFile),
            Some(NewFolder),
        ]
    );
}

/// 「查看变更」只给**有 git 状态的文件**:目录哪怕汇总出了字母也不给
/// (原版判定是 `entryGitStatus && !entry.isDir`,单文件 diff 对目录没意义);
/// 而「默认工具打开」只对文件出现。
#[test]
fn row_menu_applicability_keeps_file_open_and_directory_paste() {
    let file: Vec<_> = file_menu_actions(&menu_target(false, false, false))
        .into_iter()
        .flatten()
        .collect();
    let dir: Vec<_> = file_menu_actions(&menu_target(true, false, false))
        .into_iter()
        .flatten()
        .collect();
    assert!(file.contains(&OpenWithDefault));
    assert!(!dir.contains(&OpenWithDefault));
    assert!(dir.contains(&NewFile) && dir.contains(&NewFolder));
    assert!(file.contains(&NewFile) && file.contains(&NewFolder));
    assert!(dir.contains(&Paste));
    assert!(!file.contains(&Paste));
    // 有状态的目录同样不给 ViewDiff
    let dirty_dir: Vec<_> = file_menu_actions(&menu_target(true, true, false))
        .into_iter()
        .flatten()
        .collect();
    assert!(!dirty_dir.contains(&ViewDiff));
    assert_eq!(dirty_dir, dir);
}

#[test]
fn remote_menus_keep_download_without_upload_or_local_os_actions() {
    assert_eq!(
        file_menu_actions(&menu_target(false, true, true)),
        vec![
            Some(CopyEntry),
            Some(Download),
            None,
            Some(CopyRelativePath),
            Some(CopyAbsolutePath),
            Some(OpenInTerminal),
            None,
            Some(Rename),
            Some(Delete),
            None,
            Some(NewFile),
            Some(NewFolder),
        ]
    );
    assert_eq!(
        file_menu_actions(&menu_target(true, false, true)),
        vec![
            Some(CopyEntry),
            Some(Paste),
            Some(Download),
            None,
            Some(CopyRelativePath),
            Some(CopyAbsolutePath),
            Some(OpenInTerminal),
            None,
            Some(Rename),
            Some(Delete),
            None,
            Some(NewFile),
            Some(NewFolder),
        ]
    );
}

// ─── git 状态着色 ─────────────────────────────────────────

/// 六个字母的配色逐条对照 `FileTree.tsx:362-369`,认不出的退 muted。
#[test]
fn git状态配色照抄原版() {
    assert_eq!(git_color("M"), ui::color_warning());
    assert_eq!(git_color("A"), ui::color_success());
    // 未跟踪与新增同色(原版 `'?': text-success`)
    assert_eq!(git_color("?"), ui::color_success());
    assert_eq!(git_color("D"), ui::color_error());
    assert_eq!(git_color("C"), ui::color_error());
    assert_eq!(git_color("R"), ui::color_info());
    // 后端将来加了新字母也不会画成错的颜色
    assert_eq!(git_color("X"), ui::text_muted());
    assert_eq!(git_color(""), ui::text_muted());
}

/// 目录汇总取子树里优先级最高的那个字母,且**只认前缀是自己的**条目。
#[test]
fn 目录汇总取最高优先级() {
    let map: HashMap<String, String> = [
        ("src/a.rs", "M"),
        ("src/b.rs", "C"),
        ("src/deep/c.rs", "A"),
        // 同名前缀的兄弟目录不许被算进来
        ("srcx/d.rs", "D"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();

    assert_eq!(rollup_dir_label(&map, "src"), Some("C"));
    assert_eq!(rollup_dir_label(&map, "src/deep"), Some("A"));
    assert_eq!(rollup_dir_label(&map, "srcx"), Some("D"));
    // 没有子项的目录不出徽章
    assert_eq!(rollup_dir_label(&map, "docs"), None);
    // 文件自身那条不算「子树」(前缀要带 `/`)
    assert_eq!(rollup_dir_label(&map, "src/a.rs"), None);
}

// ─── 单链目录压缩 ─────────────────────────────────────────

fn entry(name: &str, path: &str, is_dir: bool, ignored: bool) -> FileEntry {
    FileEntry {
        name: name.to_string(),
        path: PathBuf::from(path),
        is_dir,
        ignored,
    }
}

/// 假目录表:`路径 → 子项`。
fn faker(table: Vec<(&'static str, Vec<FileEntry>)>) -> impl FnMut(&Path) -> Vec<FileEntry> {
    let table: HashMap<PathBuf, Vec<FileEntry>> = table
        .into_iter()
        .map(|(k, v)| (PathBuf::from(k), v))
        .collect();
    move |dir: &Path| table.get(dir).cloned().unwrap_or_default()
}

/// 一路单子目录 → 折成一行,名字用 `/` 拼,路径指向**链尾**。
#[test]
fn 单链目录折成一行() {
    let entries = vec![entry("src", "/p/src", true, false)];
    let list = faker(vec![
        ("/p/src", vec![entry("main", "/p/src/main", true, false)]),
        (
            "/p/src/main",
            vec![entry("java", "/p/src/main/java", true, false)],
        ),
        // 链尾有两个子项 → 停
        (
            "/p/src/main/java",
            vec![
                entry("A.java", "/p/src/main/java/A.java", false, false),
                entry("B.java", "/p/src/main/java/B.java", false, false),
            ],
        ),
    ]);
    let out = compact_dir_chains(entries, list);
    assert_eq!(out.len(), 1);
    let (entry, chain) = &out[0];
    assert_eq!(entry.name, "src/main/java");
    assert_eq!(entry.path, PathBuf::from("/p/src/main/java"));
    assert_eq!(
        chain,
        &vec![
            PathBuf::from("/p/src"),
            PathBuf::from("/p/src/main"),
            PathBuf::from("/p/src/main/java"),
        ]
    );
}

/// 不压缩的几种:文件 / 被忽略的目录 / 唯一子项是文件 / 唯一子项被忽略。
/// 这几种**都返回长度 1 的 chain**(调用方据此不登记链、不额外挂监听)。
#[test]
fn 不满足前提时原样返回() {
    let entries = vec![
        entry("readme.md", "/p/readme.md", false, false),
        entry("target", "/p/target", true, true),
        entry("only-file", "/p/only-file", true, false),
        entry("only-ignored", "/p/only-ignored", true, false),
    ];
    let list = faker(vec![
        (
            "/p/only-file",
            vec![entry("a.txt", "/p/only-file/a.txt", false, false)],
        ),
        (
            "/p/only-ignored",
            vec![entry(
                "node_modules",
                "/p/only-ignored/node_modules",
                true,
                true,
            )],
        ),
        // 被忽略的目录压根不该被列(命中就说明闸门漏了)
        ("/p/target", vec![entry("x", "/p/target/x", true, false)]),
    ]);
    let out = compact_dir_chains(entries, list);
    for (entry, chain) in &out {
        assert_eq!(chain.len(), 1, "{} 不该被压缩", entry.name);
        assert!(!entry.name.contains('/'), "{} 不该改名", entry.name);
    }
}

/// 链深上限 8:再深也不继续列(每层一次串行 IPC)。
#[test]
fn 链深封顶八层() {
    // /p/d0 → d1 → … 无限深
    let mut table: Vec<(&'static str, Vec<FileEntry>)> = Vec::new();
    const PATHS: [&str; 12] = [
        "/p/d0", "/p/d1", "/p/d2", "/p/d3", "/p/d4", "/p/d5", "/p/d6", "/p/d7", "/p/d8", "/p/d9",
        "/p/d10", "/p/d11",
    ];
    for (i, path) in PATHS.iter().enumerate().take(PATHS.len() - 1) {
        let next = PATHS[i + 1];
        let name = next.rsplit('/').next().unwrap();
        table.push((path, vec![entry(name, next, true, false)]));
    }
    let out = compact_dir_chains(vec![entry("d0", "/p/d0", true, false)], faker(table));
    let (entry, chain) = &out[0];
    assert_eq!(chain.len(), MAX_CHAIN);
    assert_eq!(entry.name, "d0/d1/d2/d3/d4/d5/d6/d7");
    assert_eq!(entry.path, PathBuf::from("/p/d7"));
}

// ─── 展开态与缓存的对账 ───────────────────────────────────

/// `entries` 缓存表:`目录 → 子项`。
fn listed(table: Vec<(&'static str, Vec<FileEntry>)>) -> HashMap<PathBuf, Vec<FileEntry>> {
    table
        .into_iter()
        .map(|(k, v)| (PathBuf::from(k), v))
        .collect()
}

fn expanded_set(paths: &'static [&'static str]) -> impl Fn(&Path) -> bool {
    let set: HashSet<PathBuf> = paths.iter().map(PathBuf::from).collect();
    move |p: &Path| set.contains(p)
}

/// 换项目回来的那一刻:只有根列过,展开着的一级目录全要补列。
#[test]
fn 展开却没列过的目录要补列() {
    let entries = listed(vec![(
        "/p",
        vec![
            entry("src", "/p/src", true, false),
            entry("docs", "/p/docs", true, false),
            entry("readme.md", "/p/readme.md", false, false),
        ],
    )]);
    let mut out = Vec::new();
    missing_expanded_dirs(
        &entries,
        Path::new("/p"),
        &expanded_set(&["/p/src"]),
        &|_| true,
        &mut out,
    );
    // 折叠的 docs 与文件 readme.md 都不掺和
    assert_eq!(out, vec![PathBuf::from("/p/src")]);
}

/// 已列过的目录不重复排队,但要**顺着它往下**继续对账。
#[test]
fn 已列过的目录只往下走() {
    let entries = listed(vec![
        ("/p", vec![entry("src", "/p/src", true, false)]),
        ("/p/src", vec![entry("core", "/p/src/core", true, false)]),
    ]);
    let mut out = Vec::new();
    missing_expanded_dirs(
        &entries,
        Path::new("/p"),
        &expanded_set(&["/p/src", "/p/src/core"]),
        &|_| true,
        &mut out,
    );
    assert_eq!(out, vec![PathBuf::from("/p/src/core")]);
}

/// 一轮只补**下一层**:祖先自己都还没列回来时,深层那条陈旧展开记录翻不到 ——
/// 远程一次列目录是一趟 SFTP 往返,不能按 `expandedDirs` 整份去列。
#[test]
fn 祖先没列出来时不越级补列() {
    let entries = listed(vec![("/p", vec![entry("src", "/p/src", true, false)])]);
    let mut out = Vec::new();
    missing_expanded_dirs(
        &entries,
        Path::new("/p"),
        // /p/src/core 也是展开的,但 /p/src 这一层还没内容,够不着
        &expanded_set(&["/p/src", "/p/src/core"]),
        &|_| true,
        &mut out,
    );
    assert_eq!(out, vec![PathBuf::from("/p/src")]);
}

/// 列失败时那条空记录(见 `load_dir_with` 的 Err 分支)让补列就此打住 ——
/// 否则 render → 补列 → 失败 → notify → render 会绕成死循环。
#[test]
fn 列过的空目录不再重排() {
    let entries = listed(vec![
        ("/p", vec![entry("src", "/p/src", true, false)]),
        ("/p/src", Vec::new()),
    ]);
    let mut out = Vec::new();
    missing_expanded_dirs(
        &entries,
        Path::new("/p"),
        &expanded_set(&["/p/src"]),
        &|_| true,
        &mut out,
    );
    assert!(out.is_empty());
}

/// 根目录自己都还没列出来(冷启动第一帧)时一条都不补:根那趟由
/// `sync_project` / `refresh_root` 显式排,补列不插手。
#[test]
fn 根没列出来时什么都不补() {
    let mut out = Vec::new();
    missing_expanded_dirs(
        &HashMap::new(),
        Path::new("/p"),
        &expanded_set(&["/p/src"]),
        &|_| true,
        &mut out,
    );
    assert!(out.is_empty());
}

/// 优先级表逐条(`PRIORITY = {C:6, D:5, M:4, A:3, R:2, '?':1}`)。
#[test]
fn 汇总优先级与原版一致() {
    let order = ["C", "D", "M", "A", "R", "?"];
    for pair in order.windows(2) {
        assert!(
            git_priority(pair[0]) > git_priority(pair[1]),
            "{} 应当排在 {} 前面",
            pair[0],
            pair[1]
        );
    }
    // 认不出的字母不参与汇总(优先级 0)
    assert_eq!(git_priority("X"), 0);
}

#[test]
fn blank_menus_expose_exactly_creation_on_the_displayed_root() {
    for remote in [false, true] {
        let target = context_target(FileTreeContextEntry::Blank, remote);
        assert_eq!(file_menu_actions(&target), vec![Some(NewFile), Some(NewFolder)]);
        assert_eq!(target.directory(), Some(PathBuf::from("/work")));
        assert!(target.row().is_none());
    }
    let mut broken = context_target(FileTreeContextEntry::Blank, true);
    broken.context.backend = FileBackendIdentity::BrokenRemote;
    assert!(file_menu_actions(&broken).is_empty());
}

#[test]
fn creation_and_drop_share_directory_file_parent_and_blank_targets() {
    for remote in [false, true] {
        let directory = menu_target(true, false, remote);
        let file = menu_target(false, false, remote);
        let blank = context_target(FileTreeContextEntry::Blank, remote);
        assert_eq!(directory.directory(), Some(PathBuf::from("/work/src")));
        assert_eq!(file.directory(), Some(PathBuf::from("/work/src")));
        assert_eq!(blank.directory(), Some(PathBuf::from("/work")));
        // Deletion/rename refresh the directory's parent, not its creation target.
        assert_eq!(
            directory.parent_directory(Path::new("/work/src")),
            blank.directory()
        );
    }
}

#[test]
fn posix_parent_keeps_backslashes_in_remote_entry_names() {
    let target = context_target(
        FileTreeContextEntry::Row(context_row(r"/work/src/name\with\slashes", false)),
        true,
    );
    assert_eq!(target.directory(), Some(PathBuf::from("/work/src")));
    let target = context_target(
        FileTreeContextEntry::Row(context_row(r"/work/src\part/file", false)),
        true,
    );
    assert_eq!(target.directory(), Some(PathBuf::from(r"/work/src\part")));
}

#[test]
fn row_relative_paths_preserve_posix_backslashes_before_the_git_diff_handoff() {
    for remote in [false, true] {
        let mut context = context_target(FileTreeContextEntry::Blank, remote).context;
        for root in ["/work", r"/work\root", "/home/User/wsl-project"] {
            context.root = PathBuf::from(root);
            let path = format!(r"{root}/src\part/name\with:slashes");
            assert_eq!(row_relative_path(&context, Path::new(&path)), Some(r"src\part/name\with:slashes".into()));
            assert_eq!(row_relative_path(&context, Path::new(root)), Some(".".into()));
            assert!(row_relative_path(&context, Path::new(&format!("{root}-other/file"))).is_none());
        }
        context.root = PathBuf::from("/");
        assert_eq!(row_relative_path(&context, Path::new(r"/name\with\slashes")), Some(r"name\with\slashes".into()));
        assert!(row_relative_path(&context, Path::new("/nul\0path")).is_none());
    }
}

#[cfg(windows)]
#[test]
fn row_relative_paths_convert_only_actual_native_windows_drive_and_unc_separators() {
    let mut context = context_target(FileTreeContextEntry::Blank, false).context;
    for (root, path) in [
        (r"C:\work", r"C:\work\src\file.rs"),
        (r"\\server\share", r"\\server\share\src\file.rs"),
        (r"\\wsl.localhost\Ubuntu\home\User", r"\\wsl.localhost\Ubuntu\home\User\src\file.rs"),
    ] {
        context.root = PathBuf::from(root);
        assert_eq!(row_relative_path(&context, Path::new(path)), Some("src/file.rs".into()));
    }
    context.root = PathBuf::from(r"C:\work");
    assert!(row_relative_path(&context, Path::new(r"D:\work\src\file.rs")).is_none());
    context.backend = FileBackendIdentity::Remote { connection_id: "ssh".into(), connection_fingerprint: 1 };
    assert!(row_relative_path(&context, Path::new(r"C:\work\src\file.rs")).is_none());
}

#[cfg(windows)]
#[test]
fn local_parent_keeps_drive_and_unc_path_semantics() {
    for (root, path, expected) in [
        (r"C:\work", r"C:\work\src\main.rs", r"C:\work\src"),
        (r"\\server\share", r"\\server\share\main.rs", r"\\server\share"),
        (
            r"\\wsl.localhost\Ubuntu\home\u",
            r"\\wsl.localhost\Ubuntu\home\u\main.rs",
            r"\\wsl.localhost\Ubuntu\home\u",
        ),
    ] {
        let mut target =
            context_target(FileTreeContextEntry::Row(context_row(path, false)), false);
        target.context.root = PathBuf::from(root);
        assert_eq!(target.directory(), Some(PathBuf::from(expected)));
    }
}

#[test]
fn captured_target_rejects_each_changed_source_field_and_reconnect() {
    let target = menu_target(false, false, true);
    let context = &target.context;
    let worktree_id = target.worktree_id.as_ref();
    let epoch = target.connection_epoch;
    assert!(target.matches_source(Some(context), worktree_id, epoch));
    assert!(!target.matches_source(None, worktree_id, epoch));
    assert!(!target.matches_source(Some(context), None, epoch));
    assert!(!target.matches_source(Some(context), Some(&worktree('b')), epoch));
    assert!(!target.matches_source(Some(context), worktree_id, None));
    assert!(!target.matches_source(Some(context), worktree_id, Some(12)));

    let mut changed = context.clone();
    changed.project_id = "project-b".into();
    assert!(!target.matches_source(Some(&changed), worktree_id, epoch));
    changed = context.clone();
    changed.root = PathBuf::from("/other");
    assert!(!target.matches_source(Some(&changed), worktree_id, epoch));
    changed = context.clone();
    changed.generation += 2;
    assert!(!target.matches_source(Some(&changed), worktree_id, epoch));
    for backend in [
        FileBackendIdentity::Local,
        FileBackendIdentity::BrokenRemote,
        FileBackendIdentity::Remote {
            connection_id: "ssh-b".into(),
            connection_fingerprint: 7,
        },
        FileBackendIdentity::Remote {
            connection_id: "ssh-a".into(),
            connection_fingerprint: 8,
        },
    ] {
        changed = context.clone();
        changed.backend = backend;
        assert!(!target.matches_source(Some(&changed), worktree_id, epoch));
    }
}

#[test]
fn clipboard_and_captured_target_stay_stale_after_aba_switch() {
    let target = menu_target(true, false, false);
    let context = &target.context;
    let clip = FileClipboardEntry {
        project_id: context.project_id.clone(),
        root: context.root.clone(),
        backend: context.backend.clone(),
        generation: context.generation,
        source: PathBuf::from("/work/src"),
        is_dir: true,
    };
    assert!(clip.can_paste_into(context));
    assert!(clip.would_copy_into_itself(&target.directory().unwrap()));
    assert!(!clip.would_copy_into_itself(&context.root));
    let mut restored = context.clone();
    restored.generation += 2;
    assert!(!clip.can_paste_into(&restored));
    assert!(!target.matches_source(Some(&restored), target.worktree_id.as_ref(), None));
}

#[test]
fn file_tree_clipboard_preserves_worktree_and_authenticated_epoch() {
    let owner = menu_target(false, false, true);
    let clipboard = FileTreeClipboard {
        entry: FileClipboardEntry {
            project_id: owner.context.project_id.clone(),
            root: owner.context.root.clone(),
            backend: owner.context.backend.clone(),
            generation: owner.context.generation,
            source: owner.row().unwrap().path.clone(),
            is_dir: false,
        },
        owner: owner.clone(),
    };
    let destination = menu_target(true, false, true);
    assert!(clipboard.can_paste_into(&destination));
    for epoch in [None, Some(12)] {
        let mut changed = destination.clone();
        changed.connection_epoch = epoch;
        assert!(!clipboard.can_paste_into(&changed));
    }
    let mut changed = destination.clone();
    changed.worktree_id = Some(worktree('b'));
    assert!(!clipboard.can_paste_into(&changed));
    let mut changed = destination;
    changed.context.generation += 2;
    assert!(!clipboard.can_paste_into(&changed));
}

fn upload_target() -> (FileTreeContextTarget, mt_config::SshConnection) {
    let connection = mt_config::SshConnection {
        id: "ssh-a".into(),
        name: "Fixture".into(),
        host: "fixture.invalid".into(),
        port: 22,
        user: "fixture".into(),
        password: None,
        identity_file: None,
        group: None,
    };
    let mut target = menu_target(false, false, true);
    target.context.backend = FileBackendIdentity::Remote {
        connection_id: connection.id.clone(),
        connection_fingerprint: crate::remote_ssh::connection_fingerprint(&connection),
    };
    (target, connection)
}

#[test]
fn upload_preparation_keeps_the_captured_epoch_connection_and_file_parent() {
    let (target, connection) = upload_target();
    assert_eq!(
        ops::prepare_upload_target(&target, &connection).unwrap(),
        (11, PathBuf::from("/work/src")),
    );
    let mut changed = connection.clone();
    changed.id = "ssh-b".into();
    assert!(ops::prepare_upload_target(&target, &changed).is_err());
    let mut changed = connection.clone();
    changed.host = "other.invalid".into();
    assert!(ops::prepare_upload_target(&target, &changed).is_err());
    let mut disconnected = target.clone();
    disconnected.connection_epoch = None;
    assert!(ops::prepare_upload_target(&disconnected, &connection).is_err());
    for backend in [FileBackendIdentity::Local, FileBackendIdentity::BrokenRemote] {
        let mut changed = target.clone();
        changed.context.backend = backend;
        assert!(ops::prepare_upload_target(&changed, &connection).is_err());
    }
    let mut directory = target.clone();
    directory.entry = FileTreeContextEntry::Row(context_row(r"/work/src\part", true));
    let (_, path) = ops::prepare_upload_target(&directory, &connection).unwrap();
    assert_eq!(remote_path_text(&path).unwrap(), r"/work/src\part");
    let mut blank = target;
    blank.entry = FileTreeContextEntry::Blank;
    assert_eq!(
        ops::prepare_upload_target(&blank, &connection).unwrap(),
        (11, PathBuf::from("/work")),
    );
}

#[cfg(any(unix, windows))]
#[test]
fn non_unicode_remote_paths_fail_before_upload_or_creation_io() {
    #[cfg(unix)]
    let invalid = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(b"/work/invalid-\xff".to_vec())
    };
    #[cfg(windows)]
    let invalid = {
        use std::os::windows::ffi::OsStringExt;
        let mut path: Vec<u16> = "/work/invalid-".encode_utf16().collect();
        path.push(0xd800);
        std::ffi::OsString::from_wide(&path)
    };
    let invalid = PathBuf::from(invalid);
    assert!(remote_path_text(&invalid).is_err());
    let (mut target, connection) = upload_target();
    target.context.root = invalid.clone();
    target.entry = FileTreeContextEntry::Blank;
    assert!(ops::prepare_upload_target(&target, &connection).is_err());
    assert!(ops::create_entry_at_target(&target, Some(&connection), "new.txt", false).is_err());
    let mut row = context_row("/work/file", false);
    row.path = invalid;
    target.entry = FileTreeContextEntry::Row(row);
    assert!(target.directory().is_none());
}

#[test]
fn files_list_has_one_bounded_scroll_owner_and_nonshrinking_rows() {
    let mut shell = file_tree_scroll_shell();
    let style = shell.style();
    assert_eq!(style.display, Some(gpui::Display::Flex));
    assert_eq!(style.flex_direction, Some(gpui::FlexDirection::Column));
    assert_eq!(style.flex_grow, Some(1.0));
    assert_eq!(style.min_size.height, Some(px(0.0).into()));
    assert_eq!(style.overflow.y, Some(gpui::Overflow::Hidden));

    let mut list = file_tree_scroll_list(&ScrollHandle::new());
    let style = list.style();
    assert_eq!(style.display, Some(gpui::Display::Flex));
    assert_eq!(style.flex_direction, Some(gpui::FlexDirection::Column));
    assert_eq!(style.flex_grow, Some(1.0));
    assert_eq!(style.min_size.height, Some(px(0.0).into()));
    assert_eq!(style.overflow.y, Some(gpui::Overflow::Scroll));
    assert_eq!(style.scrollbar_width, Some(SCROLLBAR_GUTTER.into()));

    let mut row = file_tree_row("row".into());
    assert_eq!(row.style().flex_shrink, Some(0.0));
    let mut overlay = file_tree_scrollbar_overlay();
    let style = overlay.style();
    assert_eq!(style.position, Some(gpui::Position::Absolute));
    assert_eq!(style.inset.top, Some(px(0.0).into()));
    assert_eq!(style.inset.bottom, Some(px(0.0).into()));
    assert_eq!(style.inset.right, Some(px(0.0).into()));
    assert_eq!(style.inset.left, None);
    assert_eq!(style.size.width, Some(SCROLLBAR_GUTTER.into()));
    assert_eq!(style.overflow.y, None);
}

struct CreationFixture(PathBuf);

impl CreationFixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("mini-term-file-tree-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn target(&self, entry: FileTreeContextEntry) -> FileTreeContextTarget {
        let mut target = context_target(entry, false);
        target.context.root = self.0.clone();
        target
    }
}

impl Drop for CreationFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn creation_dispatch_uses_the_captured_destination_and_preserves_existing_files() {
    let fixture = CreationFixture::new();
    let directory = fixture.0.join("src");
    std::fs::create_dir(&directory).unwrap();
    let file = directory.join("original.txt");
    std::fs::write(&file, b"original contents").unwrap();
    let file_target = fixture.target(FileTreeContextEntry::Row(context_row(
        &file.to_string_lossy(),
        false,
    )));
    let dir_target = fixture.target(FileTreeContextEntry::Row(context_row(
        &directory.to_string_lossy(),
        true,
    )));
    let blank_target = fixture.target(FileTreeContextEntry::Blank);

    ops::create_entry_at_target(&file_target, None, "peer.txt", false).unwrap();
    ops::create_entry_at_target(&file_target, None, "peer-dir", true).unwrap();
    ops::create_entry_at_target(&dir_target, None, "child.txt", false).unwrap();
    ops::create_entry_at_target(&blank_target, None, "root-dir", true).unwrap();
    assert!(directory.join("peer.txt").is_file());
    assert!(directory.join("peer-dir").is_dir());
    assert!(directory.join("child.txt").is_file());
    assert!(fixture.0.join("root-dir").is_dir());
    assert!(!fixture.0.join("peer.txt").exists());
    assert!(ops::create_entry_at_target(&file_target, None, "original.txt", false).is_err());
    assert!(ops::create_entry_at_target(&file_target, None, "original.txt", true).is_err());
    assert_eq!(std::fs::read(&file).unwrap(), b"original contents");
}

#[test]
fn creation_rejects_non_basename_input_before_filesystem_access() {
    let target = menu_target(false, false, false);
    let expected_errors = [mt_i18n::Locale::Zh, mt_i18n::Locale::En]
        .map(|locale| mt_i18n::t_in(locale, "fileTree", "operation.invalidName"));
    for name in [
        "", ".", "..", "../escape", "sub/file", r"sub\file", "C:file", "nul\0name",
    ] {
        for is_dir in [false, true] {
            let error = ops::create_entry_at_target(&target, None, name, is_dir).unwrap_err();
            assert!(expected_errors.contains(&error.as_str()), "{name:?}: {error}");
        }
    }
}

#[test]
fn unavailable_remote_creation_never_falls_back_to_local_io() {
    let fixture = CreationFixture::new();
    for backend in [
        FileBackendIdentity::BrokenRemote,
        FileBackendIdentity::Remote {
            connection_id: "missing".into(),
            connection_fingerprint: 1,
        },
    ] {
        let mut target = fixture.target(FileTreeContextEntry::Blank);
        target.context.backend = backend;
        assert!(ops::create_entry_at_target(&target, None, "never-created", false).is_err());
        assert!(!fixture.0.join("never-created").exists());
    }
}

#[cfg(unix)]
#[test]
fn creation_preserves_symlink_parent_containment_without_following_the_clicked_file() {
    use std::os::unix::fs::symlink;

    let fixture = CreationFixture::new();
    let outside = CreationFixture::new();
    let sentinel = outside.0.join("sentinel");
    std::fs::write(&sentinel, b"outside contents").unwrap();
    let directory_link = fixture.0.join("escape");
    symlink(&outside.0, &directory_link).unwrap();
    for (path, is_dir) in [
        (directory_link.clone(), true),
        (directory_link.join("sentinel"), false),
    ] {
        let target = fixture.target(FileTreeContextEntry::Row(context_row(
            &path.to_string_lossy(),
            is_dir,
        )));
        for create_directory in [false, true] {
            assert!(ops::create_entry_at_target(&target, None, "new-entry", create_directory).is_err());
            assert!(!outside.0.join("new-entry").exists());
        }
    }
    let file_link = fixture.0.join("file-link");
    symlink(&sentinel, &file_link).unwrap();
    let target = fixture.target(FileTreeContextEntry::Row(context_row(
        &file_link.to_string_lossy(),
        false,
    )));
    ops::create_entry_at_target(&target, None, "safe-peer", false).unwrap();
    assert!(fixture.0.join("safe-peer").is_file());
    assert!(!outside.0.join("safe-peer").exists());
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"outside contents");
}
