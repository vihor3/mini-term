use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use mt_ai::sessions::normalize_unix_path;
use mt_config::SshConnection;
use mt_project::fs::{ALWAYS_IGNORE, FileEntry, TextGitignore, natural_cmp};
use mt_ssh::{SftpNodeKind, SftpTransferError};

use super::project_ops::{RemoteProjectContext, ensure_operation_session};
use super::transfer::FileSessionPin;

use super::{
    DEFAULT_REMOTE_PASTE_DIR, GITIGNORE_MAX_BYTES, expand_tilde, join_posix, lock,
    open_sftp_with_session, posix_relative, split_posix_leaf, state, valid_remote_name,
    valid_sftp_child_name, validate_remote_dir_under_root, validate_remote_leaf_under_root,
};

#[derive(Debug, Clone)]
pub struct RemoteDirectoryEntry {
    pub name: String,
    pub path: String,
    pub is_symlink: bool,
}

#[derive(Debug, Clone)]
pub struct RemoteDirectoryListing {
    pub canonical_path: String,
    pub directories: Vec<RemoteDirectoryEntry>,
    pub connection_epoch: u64,
    pub connection_fingerprint: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteDirectoryBrowseErrorKind {
    InvalidPath,
    Unavailable,
}

#[derive(Clone, Debug)]
pub struct RemoteDirectoryBrowseError {
    pub kind: RemoteDirectoryBrowseErrorKind,
    pub message: String,
}

impl RemoteDirectoryBrowseError {
    fn new(kind: RemoteDirectoryBrowseErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into().chars().take(1024).collect(),
        }
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self::new(RemoteDirectoryBrowseErrorKind::Unavailable, message)
    }
}

const BROWSER_LISTING_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const BROWSER_ENTRY_LIMIT: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteConnectionProbe {
    pub canonical_home: String,
    pub connection_epoch: u64,
}

/// 把配置里的「远程粘贴落盘目录」解析成远端绝对路径。
///
/// 三种写法(对齐 `AppConfig::remote_paste_dir` 的文档):
/// - 相对路径 `.mini-term/pasted` → 相对**项目根**展开(默认形态,图片落项目内)
/// - `~` / `~/xxx` → 远程 home 展开
/// - 绝对路径 `/tmp/mini-term` → 原样
///
/// **保证返回的路径不含 `..` 段**。这条路径最终会拼进 SFTP **写**操作 ——
/// 逃出项目根 / home 的写入不是这个功能该有的能力,宁可报错。
/// 判定放在归一之后,`project_path`(调用方传入)带 `..` 的情形一并挡掉,
/// 而不只是校验用户填的 `dest_dir`。
pub(super) fn resolve_paste_dir(
    project_path: &str,
    home: &str,
    dest_dir: &str,
) -> Result<String, String> {
    // 用户可能顺手填了反斜杠,统一成 POSIX 分隔符再判定。
    let raw = dest_dir.trim().replace('\\', "/");
    let raw = if raw.trim().is_empty() {
        DEFAULT_REMOTE_PASTE_DIR.to_string()
    } else {
        raw
    };

    let abs = if raw.starts_with('/') {
        raw.clone()
    } else if raw == "~" || raw.starts_with("~/") {
        expand_tilde(&raw, home)
    } else {
        // 相对项目根。项目根必须是绝对路径(添加远程项目时已 canonicalize)。
        if !project_path.starts_with('/') {
            return Err(format!("远程项目路径不是绝对路径: {project_path}"));
        }
        join_posix(project_path, raw.trim_start_matches('/'))
    };

    // 归一:丢掉空段与 `.` 段。`./x` 和 `x` 必须解析成同一条路径,否则
    // `/proj/.` 这种写法会绕过下游「目录是否严格位于项目内」的判定。
    // 注意 `.` / `..` 都是**整段**比较,`.mini-term` 这类点开头的目录名不受影响。
    let normalized: Vec<&str> = abs
        .split('/')
        .filter(|seg| !seg.is_empty() && *seg != ".")
        .collect();
    if normalized.is_empty() {
        return Err("远程粘贴目录解析为空".into());
    }
    // 归一后再查 `..`:此时 dest_dir 与 project_path 两部分都已合入 abs,
    // 一处判定覆盖两个来源。
    if normalized.contains(&"..") {
        return Err("远程粘贴目录不能包含 `..`".into());
    }
    Ok(format!("/{}", normalized.join("/")))
}

/// 从本地临时文件路径提取文件名。两种分隔符都切 —— 传进来的是 Windows 路径,
/// 不能让 `\` 残留在远端路径里。
pub(super) fn paste_file_name(local_path: &str) -> Result<String, String> {
    let name = local_path.rsplit(['/', '\\']).next().unwrap_or("").trim();
    if name.is_empty() || name == "." || name == ".." {
        return Err(format!("无法从本地路径提取文件名: {local_path}"));
    }
    Ok(name.to_string())
}

// ---------------------------------------------------------------------------
// 入口 1:远程文件树
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFileListingSource {
    pub connection_id: String,
    pub connection_fingerprint: u64,
    pub connection_epoch: u64,
    pub project_root: String,
    pub directory: String,
}

#[derive(Debug, Clone)]
pub struct RemoteFileListing {
    pub entries: Vec<FileEntry>,
    pub source: RemoteFileListingSource,
}

/// None is allowed only for initial read-only discovery; the returned epoch
/// must still be current before the caller accepts any rows.
pub fn list_directory_at_epoch(
    conn: &SshConnection,
    expected_epoch: Option<u64>,
    path: &str,
    project_root: &str,
    refresh_ignore: bool,
) -> Result<RemoteFileListing, String> {
    list_directory_with_epoch(
        conn,
        path,
        project_root,
        refresh_ignore,
        expected_epoch,
        true,
    )
}

fn list_directory_with_epoch(
    conn: &SshConnection,
    path: &str,
    project_root: &str,
    refresh_ignore: bool,
    expected_epoch: Option<u64>,
    scoped: bool,
) -> Result<RemoteFileListing, String> {
    let st = state();
    let ignore_key = format!("{}|{}", conn.id, normalize_unix_path(project_root));
    if !scoped && refresh_ignore {
        lock(&st.gitignore_cache).remove(&ignore_key);
    }
    // 锁即取即放;miss 时在 SFTP 打开后无锁读取,再短暂加锁写回。
    let cached_ignore = (!scoped)
        .then(|| lock(&st.gitignore_cache).get(&ignore_key).cloned())
        .flatten();

    st.block_on(async move {
        let (session, sftp) = open_sftp_with_session(st, conn).await?;
        let epoch = session.connection_epoch().get();
        let fingerprint = super::connection_fingerprint(conn);
        let pin = FileSessionPin::new(
            st,
            conn,
            scoped.then_some(expected_epoch.unwrap_or(epoch)),
            &session,
        );
        let result = async {
            pin.check().await?;
            if scoped {
                validate_remote_dir_under_root(&sftp, project_root, path).await?;
                pin.check().await?;
            }
            let (ignore_key, cached_ignore) = if scoped {
                let key = format!("{}|files|{fingerprint}|{epoch}|{}", conn.id, project_root);
                let mut cache = lock(&st.gitignore_cache);
                if refresh_ignore {
                    cache.remove(&key);
                }
                let cached = cache.get(&key).cloned();
                (key, cached)
            } else {
                (ignore_key, cached_ignore)
            };
            let gitignore = match cached_ignore {
                Some(g) => g,
                None => {
                    let gi_path = join_posix(project_root, ".gitignore");
                    // .gitignore 不存在 / 读失败 → 空规则,静默降级。
                    let content = match sftp.read_head(&gi_path, GITIGNORE_MAX_BYTES).await {
                        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                        Err(_) => String::new(),
                    };
                    let g = Arc::new(TextGitignore::from_text(&content));
                    pin.check().await?;
                    lock(&st.gitignore_cache).insert(ignore_key.clone(), g.clone());
                    g
                }
            };

            pin.check().await?;
            let entries = sftp
                .read_dir(path)
                .await
                .map_err(|e| format!("读取远程目录失败: {}", e.message()))?;

            let mut out: Vec<FileEntry> = entries
                .into_iter()
                .filter_map(|e| {
                    // FileTree 目前用宿主 `PathBuf` 承载远程路径；反斜杠在 Windows
                    // 会被解释成分隔符，因此无法无损、安全地操作这类远程名称。
                    if !valid_remote_name(&e.name) {
                        return None;
                    }
                    // ALWAYS_IGNORE 目录完全隐藏(与本地树一致)
                    if e.is_dir && ALWAYS_IGNORE.contains(&e.name.as_str()) {
                        return None;
                    }
                    let full = join_posix(path, &e.name);
                    let ignored = posix_relative(project_root, &full)
                        .map(|rel| gitignore.is_ignored(&rel, e.is_dir))
                        .unwrap_or(false);
                    Some(FileEntry {
                        name: e.name,
                        // 远程路径是 POSIX 字符串,`PathBuf` 在这里只是容器 ——
                        // 拼接一律走 `join_posix`,绝不用 `Path::join`(会插 `\`)。
                        path: PathBuf::from(full),
                        is_dir: e.is_dir,
                        ignored,
                    })
                })
                .collect();
            out.sort_by(|a, b| {
                b.is_dir
                    .cmp(&a.is_dir)
                    .then_with(|| a.ignored.cmp(&b.ignored))
                    .then_with(|| natural_cmp(&a.name, &b.name))
            });
            Ok(RemoteFileListing {
                entries: out,
                source: RemoteFileListingSource {
                    connection_id: conn.id.clone(),
                    connection_fingerprint: fingerprint,
                    connection_epoch: epoch,
                    project_root: project_root.to_string(),
                    directory: path.to_string(),
                },
            })
        }
        .await;
        sftp.close().await;
        pin.finish(result).await
    })
}

/// Read one directory on the exact authenticated onboarding session. Home
/// expansion belongs to the caller's validated host probe, not a pooled cache.
/// Blocking: invoke only on a background executor. No project ignore filters.
pub fn browse_directory(
    context: &RemoteProjectContext,
    requested_path: &str,
) -> Result<RemoteDirectoryListing, RemoteDirectoryBrowseError> {
    context
        .validate()
        .map_err(RemoteDirectoryBrowseError::unavailable)?;
    validate_browser_path(requested_path)?;
    let st = state();
    st.block_on(async move {
        let (session, sftp) = open_sftp_with_session(st, &context.connection).await?;
        let result = tokio::time::timeout(BROWSER_LISTING_TIMEOUT, async {
            ensure_operation_session(st, context, &session)
                .await
                .map_err(RemoteDirectoryBrowseError::unavailable)?;
            validate_browser_node(
                sftp.try_node_kind(requested_path)
                    .await
                    .map_err(browser_sftp_error)?,
            )?;
            let canonical = sftp
                .canonicalize(requested_path)
                .await
                .map_err(browser_sftp_error)?;
            validate_browser_path(&canonical)?;
            if !sftp.is_dir(&canonical).await.map_err(browser_sftp_error)? {
                return Err(RemoteDirectoryBrowseError::new(
                    RemoteDirectoryBrowseErrorKind::InvalidPath,
                    "The remote path is not a directory",
                ));
            }
            let entries = sftp
                .read_dir(&canonical)
                .await
                .map_err(browser_sftp_error)?;
            validate_browser_entry_count(entries.len())?;
            let mut directories = Vec::new();
            for entry in entries {
                if !valid_sftp_child_name(&entry.name) {
                    continue;
                }
                let path = join_posix(&canonical, &entry.name);
                let browsable =
                    entry.is_dir || (entry.is_symlink && sftp.is_dir(&path).await.unwrap_or(false));
                if !browsable {
                    continue;
                }
                directories.push(RemoteDirectoryEntry {
                    path,
                    name: entry.name,
                    is_symlink: entry.is_symlink,
                });
            }
            directories.sort_by(|a, b| natural_cmp(&a.name, &b.name));
            Ok(RemoteDirectoryListing {
                canonical_path: canonical,
                directories,
                connection_epoch: session.connection_epoch().get(),
                connection_fingerprint: context.connection_fingerprint,
            })
        })
        .await
        .unwrap_or_else(|_| {
            Err(RemoteDirectoryBrowseError::unavailable(
                "The remote folder listing timed out",
            ))
        });
        // Recheck even failures on the session that produced them. An epoch
        // mismatch never authorizes a picker to adopt a replacement session.
        let authority = ensure_operation_session(st, context, &session).await;
        sftp.close().await;
        Ok(authority
            .map_err(RemoteDirectoryBrowseError::unavailable)
            .and(result))
    })
    .map_err(RemoteDirectoryBrowseError::unavailable)?
}

fn validate_browser_path(path: &str) -> Result<(), RemoteDirectoryBrowseError> {
    if !path.starts_with('/') || path.contains('\0') {
        return Err(RemoteDirectoryBrowseError::new(
            RemoteDirectoryBrowseErrorKind::InvalidPath,
            "An absolute remote directory path is required",
        ));
    }
    Ok(())
}

fn validate_browser_node(kind: Option<SftpNodeKind>) -> Result<(), RemoteDirectoryBrowseError> {
    match kind {
        Some(SftpNodeKind::Directory | SftpNodeKind::Symlink) => Ok(()),
        None | Some(SftpNodeKind::File | SftpNodeKind::Other) => {
            Err(RemoteDirectoryBrowseError::new(
                RemoteDirectoryBrowseErrorKind::InvalidPath,
                "The remote path is missing or is not a directory",
            ))
        }
    }
}

pub(crate) fn validate_browser_entry_count(count: usize) -> Result<(), RemoteDirectoryBrowseError> {
    if count > BROWSER_ENTRY_LIMIT {
        return Err(RemoteDirectoryBrowseError::unavailable(format!(
            "The folder exceeds the browser limit of {BROWSER_ENTRY_LIMIT} entries"
        )));
    }
    Ok(())
}

fn browser_sftp_error(error: SftpTransferError) -> RemoteDirectoryBrowseError {
    // This API only retains Transport/Sftp plus a string. Permission status
    // codes are not available here; do not guess from localized diagnostics.
    RemoteDirectoryBrowseError::unavailable(error.message())
}

/// 在远程项目目录中新建文件或文件夹。
pub fn create_entry_at_epoch(
    conn: &SshConnection,
    expected_epoch: u64,
    project_root: &str,
    parent_dir: &str,
    name: &str,
    is_dir: bool,
) -> Result<String, String> {
    create_entry_with_epoch(
        conn,
        project_root,
        parent_dir,
        name,
        is_dir,
        Some(expected_epoch),
    )
}

fn create_entry_with_epoch(
    conn: &SshConnection,
    project_root: &str,
    parent_dir: &str,
    name: &str,
    is_dir: bool,
    expected_epoch: Option<u64>,
) -> Result<String, String> {
    if !valid_remote_name(name) {
        return Err(format!("文件名无效: {name}"));
    }
    let st = state();
    st.block_on(async move {
        let (session, sftp) = open_sftp_with_session(st, conn).await?;
        let pin = FileSessionPin::new(st, conn, expected_epoch, &session);
        let result = async {
            pin.check().await?;
            let parent = validate_remote_dir_under_root(&sftp, project_root, parent_dir).await?;
            let target = join_posix(&parent, name);
            pin.check().await?;
            if is_dir {
                sftp.create_dir(&target)
                    .await
                    .map_err(|e| format!("创建远程文件夹失败: {}", e.message()))?;
            } else {
                sftp.create_file(&target)
                    .await
                    .map_err(|e| format!("创建远程文件失败: {}", e.message()))?;
            }
            Ok(target)
        }
        .await;
        sftp.close().await;
        pin.finish(result).await
    })
}

/// 重命名远程条目；新名称只允许单个 POSIX basename。
pub fn rename_entry_at_epoch(
    conn: &SshConnection,
    expected_epoch: u64,
    project_root: &str,
    path: &str,
    new_name: &str,
) -> Result<String, String> {
    rename_entry_with_epoch(conn, project_root, path, new_name, Some(expected_epoch))
}

fn rename_entry_with_epoch(
    conn: &SshConnection,
    project_root: &str,
    path: &str,
    new_name: &str,
    expected_epoch: Option<u64>,
) -> Result<String, String> {
    if !valid_remote_name(new_name) {
        return Err(format!("文件名无效: {new_name}"));
    }
    let st = state();
    st.block_on(async move {
        let (session, sftp) = open_sftp_with_session(st, conn).await?;
        let pin = FileSessionPin::new(st, conn, expected_epoch, &session);
        let result = async {
            pin.check().await?;
            let source = validate_remote_leaf_under_root(&sftp, project_root, path).await?;
            let (parent, _) = split_posix_leaf(&source)?;
            let target = join_posix(parent, new_name);
            pin.check().await?;
            sftp.rename(&source, &target)
                .await
                .map_err(|e| format!("重命名远程条目失败: {}", e.message()))?;
            Ok(target)
        }
        .await;
        sftp.close().await;
        pin.finish(result).await
    })
}

/// 连接自检:只探到远程 `$HOME` 为止,返回它和承载该结果的精确会话 epoch。
///
/// 项目引导用它验证所选主机并取得规范化 home；失败文案与真实目录访问同源。
pub fn probe_connection(conn: &SshConnection) -> Result<RemoteConnectionProbe, String> {
    let st = state();
    st.block_on(async move {
        let (session, sftp) = open_sftp_with_session(st, conn).await?;
        let connection_epoch = session.connection_epoch().get();
        let context = RemoteProjectContext::new(
            conn.clone(),
            super::connection_fingerprint(conn),
            Some(connection_epoch),
        );
        let result = async {
            ensure_operation_session(st, &context, &session).await?;
            // The legacy connection-ID cache cannot establish this session's
            // authenticated home, including after a reconnect or reconfiguration.
            let canonical_home = sftp
                .canonicalize(".")
                .await
                .map_err(|error| format!("远程路径无效: {}", error.message()))?;
            if !sftp
                .is_dir(&canonical_home)
                .await
                .map_err(|error| format!("远程路径不可访问: {}", error.message()))?
            {
                return Err(format!("远程路径不是目录: {canonical_home}"));
            }
            Ok(RemoteConnectionProbe {
                canonical_home,
                connection_epoch,
            })
        }
        .await;
        sftp.close().await;
        ensure_operation_session(st, &context, &session)
            .await
            .and(result)
    })
}

#[cfg(test)]
mod browser_tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    #[ignore = "requires the Actions disposable loopback sshd fixture and isolated HOME"]
    fn actual_loopback_sftp_browser_preserves_source_and_epoch() {
        let (connection, fixture_root) = crate::remote_ssh::loopback_ssh_fixture().unwrap();
        let root = fixture_root.join(format!("browser-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        for name in [
            ".git",
            ".hidden",
            "dir2",
            "dir10",
            "empty",
            r"a\b:folder",
            "trailing ",
        ] {
            std::fs::create_dir(root.join(name)).unwrap();
        }
        std::fs::create_dir(root.join("dir2/nested")).unwrap();
        std::fs::write(root.join("file"), b"read-only sentinel").unwrap();
        std::os::unix::fs::symlink(root.join("dir2"), root.join("directory-link")).unwrap();
        std::os::unix::fs::symlink(root.join("file"), root.join("file-link")).unwrap();
        std::os::unix::fs::symlink(root.join("missing"), root.join("broken-link")).unwrap();
        let root_text = root.to_str().unwrap();
        let fingerprint = crate::remote_ssh::connection_fingerprint(&connection);
        let st = state();
        let (epoch, authenticated_home) = st
            .block_on(async {
                let (session, sftp) = open_sftp_with_session(st, &connection).await?;
                let epoch = session.connection_epoch().get();
                let home = sftp
                    .canonicalize(".")
                    .await
                    .map_err(|error| error.message().to_string())?;
                sftp.close().await;
                Ok((epoch, home))
            })
            .unwrap();
        assert_ne!(authenticated_home, root_text);
        lock(&st.home_cache).insert(connection.id.clone(), root_text.to_string());
        let probe = probe_connection(&connection).unwrap();
        assert_eq!(probe.canonical_home, authenticated_home);
        assert_eq!(probe.connection_epoch, epoch);
        assert_eq!(
            lock(&st.home_cache).get(&connection.id).cloned(),
            Some(root_text.to_string())
        );
        let context = RemoteProjectContext::new(connection.clone(), fingerprint, Some(epoch));
        let listing = browse_directory(&context, root_text).unwrap();
        assert_eq!(listing.canonical_path, root_text);
        assert_eq!(listing.connection_epoch, epoch);
        assert_eq!(listing.connection_fingerprint, fingerprint);
        let names = listing
            .directories
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 8);
        for name in [
            ".git",
            ".hidden",
            "dir2",
            "dir10",
            "empty",
            r"a\b:folder",
            "trailing ",
            "directory-link",
        ] {
            assert!(names.contains(&name), "{name:?}");
        }
        assert!(
            names.iter().position(|name| *name == "dir2")
                < names.iter().position(|name| *name == "dir10")
        );
        assert!(!names.contains(&"nested"));
        let link = listing
            .directories
            .iter()
            .find(|entry| entry.name == "directory-link")
            .unwrap();
        assert!(link.is_symlink);
        assert_eq!(
            browse_directory(&context, &link.path)
                .unwrap()
                .canonical_path,
            root.join("dir2").to_str().unwrap()
        );
        for name in [r"a\b:folder", "trailing ", "empty"] {
            let entry = listing
                .directories
                .iter()
                .find(|entry| entry.name == name)
                .unwrap();
            let empty = browse_directory(&context, &entry.path).unwrap();
            assert!(empty.directories.is_empty());
            assert_eq!(empty.canonical_path, root.join(name).to_str().unwrap());
        }
        for name in ["file", "file-link", "missing"] {
            assert_eq!(
                browse_directory(&context, root.join(name).to_str().unwrap())
                    .unwrap_err()
                    .kind,
                RemoteDirectoryBrowseErrorKind::InvalidPath
            );
        }
        let wrong_fingerprint =
            RemoteProjectContext::new(connection.clone(), fingerprint ^ 1, Some(epoch));
        assert_eq!(
            browse_directory(&wrong_fingerprint, root_text)
                .unwrap_err()
                .kind,
            RemoteDirectoryBrowseErrorKind::Unavailable
        );

        let (replacement_epoch, replacement_home) = st
            .block_on(async {
                let (session, sftp) = open_sftp_with_session(st, &connection).await?;
                ensure_operation_session(st, &context, &session).await?;
                sftp.close().await;
                assert!(
                    crate::remote_ssh::evict_session_if_same(
                        st,
                        &st.pool(),
                        &connection.id,
                        &session
                    )
                    .await
                );
                let (replacement, replacement_sftp) =
                    open_sftp_with_session(st, &connection).await?;
                assert!(
                    ensure_operation_session(st, &context, &session)
                        .await
                        .is_err()
                );
                assert!(
                    ensure_operation_session(st, &context, &replacement)
                        .await
                        .is_err()
                );
                let epoch = replacement.connection_epoch().get();
                let home = replacement_sftp
                    .canonicalize(".")
                    .await
                    .map_err(|error| error.message().to_string())?;
                replacement_sftp.close().await;
                Ok((epoch, home))
            })
            .unwrap();
        assert_ne!(epoch, replacement_epoch);
        let probe = probe_connection(&connection).unwrap();
        assert_eq!(probe.canonical_home, replacement_home);
        assert_ne!(probe.canonical_home, root_text);
        assert_eq!(probe.connection_epoch, replacement_epoch);
        assert_eq!(
            lock(&st.home_cache).get(&connection.id).cloned(),
            Some(root_text.to_string())
        );
        assert_eq!(listing.connection_epoch, epoch);
        assert_eq!(
            browse_directory(&context, root_text).unwrap_err().kind,
            RemoteDirectoryBrowseErrorKind::Unavailable
        );
        // Even a proven non-directory must reject the stale source before probing it.
        assert_eq!(
            browse_directory(&context, root.join("file").to_str().unwrap())
                .unwrap_err()
                .kind,
            RemoteDirectoryBrowseErrorKind::Unavailable
        );
        let replacement =
            RemoteProjectContext::new(connection, fingerprint, Some(replacement_epoch));
        let current = browse_directory(&replacement, root_text).unwrap();
        assert_eq!(current.connection_epoch, replacement_epoch);
        assert_eq!(current.connection_fingerprint, fingerprint);
        assert_eq!(current.directories.len(), listing.directories.len());
        assert_eq!(
            std::fs::read(root.join("file")).unwrap(),
            b"read-only sentinel"
        );
        assert!(!root.join("empty/.git").exists());
        assert_eq!(std::fs::read_dir(root.join(".git")).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 11);
        lock(&st.home_cache).remove(&replacement.connection.id);
    }

    #[test]
    fn browser_requires_absolute_paths_without_rewriting_posix_names() {
        for path in [
            "/",
            "/home/User/.config",
            r"/names/a\b:folder",
            "/with spaces/ ",
            "/link/../peer",
        ] {
            assert!(validate_browser_path(path).is_ok());
        }
        for path in ["", "~", "relative/path", "C:\\client", "/nul\0path"] {
            assert_eq!(
                validate_browser_path(path).unwrap_err().kind,
                RemoteDirectoryBrowseErrorKind::InvalidPath
            );
        }
    }

    #[test]
    fn browser_only_attempts_directories_and_links_and_keeps_hidden_names() {
        assert!(validate_browser_node(Some(SftpNodeKind::Directory)).is_ok());
        assert!(validate_browser_node(Some(SftpNodeKind::Symlink)).is_ok());
        for kind in [None, Some(SftpNodeKind::File), Some(SftpNodeKind::Other)] {
            assert_eq!(
                validate_browser_node(kind).unwrap_err().kind,
                RemoteDirectoryBrowseErrorKind::InvalidPath
            );
        }
        assert!(valid_sftp_child_name(".git"));
        assert!(valid_sftp_child_name(".config"));
        assert!(!valid_sftp_child_name(".."));
        assert!(!valid_sftp_child_name("nested/child"));
    }

    #[test]
    fn browser_limits_entries_and_preserves_unclassified_sftp_failures() {
        assert!(validate_browser_entry_count(BROWSER_ENTRY_LIMIT).is_ok());
        assert!(validate_browser_entry_count(BROWSER_ENTRY_LIMIT + 1).is_err());
        for error in [
            SftpTransferError::Sftp("Permission denied".into()),
            SftpTransferError::Sftp("No such file".into()),
            SftpTransferError::Transport("disconnected".into()),
        ] {
            assert_eq!(
                browser_sftp_error(error).kind,
                RemoteDirectoryBrowseErrorKind::Unavailable
            );
        }
    }
}
