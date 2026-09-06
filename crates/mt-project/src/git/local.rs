//! Native libgit2 reads. No host command execution or remote path semantics.

use std::io::Read;
use std::path::Path;

use anyhow::{Result, anyhow, bail, ensure};
use git2::{ErrorCode, ObjectType, Oid, Repository, Status, StatusOptions};

use super::cli::{
    self, BlobLookup, GitRef, HeadState, ObjectId, RepositoryAuthority, RepositoryStatus,
};
use super::{BranchInfo, ChangeFileStatus, CommitFileInfo, GitCommitInfo};

pub fn authority(path: &Path) -> Result<RepositoryAuthority> {
    let repo = Repository::open(path)?;
    let root = repo
        .workdir()
        .ok_or_else(|| anyhow!("Bare repositories are not supported"))?;
    let git_dir = std::fs::canonicalize(repo.path())?;
    // git2 0.19 exposes repo.path(), not git_repository_commondir. Inspect
    // only this bounded metadata file; permission/decoding failures are errors.
    let common = match std::fs::File::open(git_dir.join("commondir")) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(cli::MAX_PATH_BYTES as u64 + 2)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() <= cli::MAX_PATH_BYTES + 1,
                "Git common directory path exceeds its limit"
            );
            // Match libgit2 lookup_commondir's git_str_rtrim, including CRLF.
            let value = std::str::from_utf8(&bytes)?.trim_end_matches(|character| {
                matches!(character, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
            });
            ensure!(
                !value.is_empty() && !value.contains('\0'),
                "Invalid Git common directory metadata"
            );
            let value = Path::new(value);
            if value.is_absolute() {
                value.to_owned()
            } else {
                git_dir.join(value)
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !repo.is_worktree() => {
            git_dir.clone()
        }
        Err(error) => return Err(error.into()),
    };
    Ok(RepositoryAuthority {
        worktree_root: canonical_directory(root)?,
        git_dir: canonical_directory(&git_dir)?,
        common_dir: canonical_directory(&common)?,
    })
}

fn canonical_directory(path: &Path) -> Result<String> {
    let path = std::fs::canonicalize(path)?;
    ensure!(
        std::fs::metadata(&path)?.is_dir(),
        "Git authority path is not a directory"
    );
    ensure!(path.to_str().is_some(), "Git native path is not UTF-8");
    let path = crate::fs::strip_verbatim_prefix(path);
    let path = path
        .to_str()
        .ok_or_else(|| anyhow!("Git native path is not UTF-8"))?;
    ensure!(
        path.len() <= cli::MAX_PATH_BYTES,
        "Git native path exceeds its limit"
    );
    Ok(path.to_owned())
}

fn oid(value: &ObjectId) -> Result<Oid> {
    ensure!(
        value.as_str().len() == 40,
        "This libgit2 version does not support SHA-256 repositories"
    );
    Ok(Oid::from_str(value.as_str())?)
}

fn object_id(value: Oid) -> Result<ObjectId> {
    ObjectId::parse(&value.to_string())
}

fn head_in(repo: &Repository) -> Result<HeadState> {
    match repo.head() {
        Ok(head) => {
            let branch = if repo.head_detached()? {
                None
            } else {
                Some(GitRef::parse(std::str::from_utf8(head.name_bytes())?)?)
            };
            let target = head
                .target()
                .ok_or_else(|| anyhow!("Git HEAD has no object target"))?;
            checked_commit(repo, target)?;
            Ok(HeadState {
                oid: Some(object_id(target)?),
                branch,
            })
        }
        Err(error) if error.code() == ErrorCode::UnbornBranch => {
            let head = repo.find_reference("HEAD")?;
            let target = head
                .symbolic_target_bytes()
                .ok_or_else(|| anyhow!("Unborn HEAD is not symbolic"))?;
            let branch = GitRef::parse(std::str::from_utf8(target)?)?;
            ensure!(!branch.is_remote(), "Unborn HEAD must name a local branch");
            Ok(HeadState {
                oid: None,
                branch: Some(branch),
            })
        }
        Err(error) => Err(error.into()),
    }
}

pub fn head(path: &Path) -> Result<HeadState> {
    head_in(&Repository::open(path)?)
}

/// Reuse the existing status projection, but never turn invalid bytes into a
/// path, or turn embedded repositories into ordinary files.
pub fn status(path: &Path) -> Result<RepositoryStatus> {
    let repo = Repository::open(path)?;
    let head = head_in(&repo)?;
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false)
        .include_unreadable(true);
    let statuses = repo.statuses(Some(&mut options))?;
    ensure!(
        statuses.len() <= cli::MAX_RECORDS,
        "Too many Git status records"
    );
    let mut changes = Vec::new();
    let mut untracked_directories = Vec::new();
    let mut bytes = 0;
    for entry in statuses.iter() {
        let flags = entry.status();
        ensure!(
            !flags.contains(Status::WT_UNREADABLE),
            "Git working entry is unreadable"
        );
        let staged = super::map_staged_status(flags);
        let unstaged = super::map_unstaged_status(flags, head.oid.is_none());
        if staged.is_none() && unstaged.is_none() {
            continue;
        }
        let reported = std::str::from_utf8(entry.path_bytes())?;
        if flags.contains(Status::WT_NEW) && reported.ends_with('/') {
            cli::validate_repo_path(&reported[..reported.len() - 1])?;
            bytes += reported.len();
            ensure!(
                bytes <= cli::MAX_LIST_BYTES,
                "Git status paths exceed their byte limit"
            );
            untracked_directories.push(reported.to_owned());
            continue;
        }
        let rename = if flags.contains(Status::WT_RENAMED) {
            entry.index_to_workdir()
        } else if flags.contains(Status::INDEX_RENAMED) {
            entry.head_to_index()
        } else {
            None
        };
        let (path, old_path) = if let Some(delta) = rename {
            (
                diff_path(delta.new_file())?,
                Some(diff_path(delta.old_file())?),
            )
        } else {
            (reported.to_owned(), None)
        };
        validate_native_repo_path(&path)?;
        if let Some(old) = &old_path {
            validate_native_repo_path(old)?;
        }
        bytes += path.len() + old_path.as_ref().map_or(0, String::len);
        ensure!(
            bytes <= cli::MAX_LIST_BYTES,
            "Git status paths exceed their byte limit"
        );
        let label = staged
            .as_ref()
            .or(unstaged.as_ref())
            .map(super::status_label)
            .unwrap_or("")
            .to_owned();
        changes.push(ChangeFileStatus {
            path,
            old_path,
            staged_status: staged,
            unstaged_status: unstaged,
            status_label: label,
        });
    }
    ensure!(
        head_in(&repo)? == head,
        "Git HEAD changed during status read"
    );
    Ok(RepositoryStatus {
        head,
        changes,
        untracked_directories,
    })
}

pub fn branches(path: &Path) -> Result<Vec<BranchInfo>> {
    let repo = Repository::open(path)?;
    let head = head_in(&repo)?;
    let mut branches = Vec::new();
    let mut bytes = 0;
    for entry in repo.branches(None)? {
        let (branch, _) = entry?;
        let reference = GitRef::parse(std::str::from_utf8(branch.get().name_bytes())?)?;
        if reference.is_remote() && reference.short_name().ends_with("/HEAD") {
            continue;
        }
        let Some(target) = branch.get().target() else {
            continue;
        };
        let target = object_id(target)?;
        bytes += reference.as_str().len() + target.as_str().len();
        ensure!(
            branches.len() < cli::MAX_RECORDS && bytes <= cli::MAX_LIST_BYTES,
            "Git branch inventory exceeds its limit"
        );
        branches.push(BranchInfo {
            name: reference.short_name().to_owned(),
            is_head: !reference.is_remote() && head.oid.as_ref() == Some(&target),
            is_remote: reference.is_remote(),
            commit_hash: target.as_str().to_owned(),
        });
    }
    Ok(branches)
}

pub fn resolve_ref(path: &Path, reference: &GitRef) -> Result<ObjectId> {
    let repo = Repository::open(path)?;
    let reference = repo.find_reference(reference.as_str())?.resolve()?;
    let target = reference
        .target()
        .ok_or_else(|| anyhow!("Git ref has no object target"))?;
    checked_commit(&repo, target)?;
    object_id(target)
}

fn checked_commit(repo: &Repository, id: Oid) -> Result<git2::Commit<'_>> {
    let (size, kind) = repo.odb()?.read_header(id)?;
    ensure!(
        kind == ObjectType::Commit && size <= cli::MAX_LIST_BYTES,
        "Git commit is invalid or exceeds its byte limit"
    );
    Ok(repo.find_commit(id)?)
}

pub fn parents(path: &Path, commit: &ObjectId) -> Result<Vec<ObjectId>> {
    let repo = Repository::open(path)?;
    let commit = checked_commit(&repo, oid(commit)?)?;
    ensure!(
        commit.parent_count() as usize <= cli::MAX_LOG_COMMITS,
        "Too many Git parent tips"
    );
    commit.parent_ids().map(object_id).collect()
}

pub fn history(path: &Path, tips: &[ObjectId], limit: usize) -> Result<Vec<GitCommitInfo>> {
    ensure!(
        (1..=cli::MAX_LOG_COMMITS).contains(&limit) && tips.len() <= cli::MAX_LOG_COMMITS,
        "Invalid Git history page bounds"
    );
    let repo = Repository::open(path)?;
    let mut walk = repo.revwalk()?;
    walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)?;
    for tip in tips {
        checked_commit(&repo, oid(tip)?)?;
        walk.push(oid(tip)?)?;
    }
    let mut result = Vec::new();
    let mut bytes = 0;
    for id in walk.take(limit) {
        let commit = checked_commit(&repo, id?)?;
        let message = std::str::from_utf8(commit.message_bytes())?;
        let author = commit.author();
        ensure!(
            commit.parent_count() as usize <= cli::MAX_LOG_COMMITS,
            "Too many Git parent tips"
        );
        bytes += message.len() + author.name_bytes().len() + commit.parent_count() as usize * 40;
        ensure!(
            bytes <= cli::MAX_LIST_BYTES,
            "Git history page exceeds its byte limit"
        );
        let author = std::str::from_utf8(author.name_bytes())?.to_owned();
        let hash = commit.id().to_string();
        result.push(GitCommitInfo {
            short_hash: hash[..7].to_owned(),
            hash,
            message: commit.summary().unwrap_or("").to_owned(),
            body: commit.body().map(str::to_owned),
            author,
            timestamp: commit.time().seconds(),
            parent_hashes: commit.parent_ids().map(|id| id.to_string()).collect(),
        });
    }
    Ok(result)
}

fn diff_path(file: git2::DiffFile<'_>) -> Result<String> {
    let bytes = file
        .path_bytes()
        .ok_or_else(|| anyhow!("Git delta path is missing"))?;
    let path = std::str::from_utf8(bytes)?;
    validate_native_repo_path(path)?;
    Ok(path.to_owned())
}

pub fn commit_files(path: &Path, commit: &ObjectId) -> Result<Vec<CommitFileInfo>> {
    let repo = Repository::open(path)?;
    let commit = checked_commit(&repo, oid(commit)?)?;
    let tree = checked_tree(&repo, commit.tree_id())?;
    let parent = if commit.parent_count() > 0 {
        Some(checked_tree(
            &repo,
            checked_commit(&repo, commit.parent_id(0)?)?.tree_id(),
        )?)
    } else {
        None
    };
    let diff = repo.diff_tree_to_tree(parent.as_ref(), Some(&tree), None)?;
    ensure!(
        diff.deltas().len() <= cli::MAX_RECORDS,
        "Too many Git commit files"
    );
    let mut files = Vec::new();
    let mut bytes = 0;
    for delta in diff.deltas() {
        let path = diff_path(delta.new_file())?;
        let old_path = if delta.status() == git2::Delta::Renamed {
            Some(diff_path(delta.old_file())?)
        } else {
            None
        };
        bytes += path.len() + old_path.as_ref().map_or(0, String::len);
        ensure!(
            bytes <= cli::MAX_LIST_BYTES,
            "Git commit files exceed their byte limit"
        );
        let status = match delta.status() {
            git2::Delta::Added => "added",
            git2::Delta::Deleted => "deleted",
            git2::Delta::Renamed => "renamed",
            _ => "modified",
        };
        files.push(CommitFileInfo {
            path,
            old_path,
            status: status.to_owned(),
        });
    }
    Ok(files)
}

fn checked_tree(repo: &Repository, id: Oid) -> Result<git2::Tree<'_>> {
    let (size, kind) = repo.odb()?.read_header(id)?;
    ensure!(
        kind == ObjectType::Tree && size <= cli::MAX_LIST_BYTES,
        "Git tree is invalid or exceeds its byte limit"
    );
    Ok(repo.find_tree(id)?)
}

pub fn validate_native_repo_path(path: &str) -> Result<()> {
    cli::validate_repo_path(path)?;
    ensure!(
        !cfg!(windows) || !path.contains(['\\', ':']),
        "Git path cannot round-trip as a native Windows filename"
    );
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlobContent {
    Missing,
    Bytes(Vec<u8>),
    TooLarge,
}

/// Resolve exact entries, check ODB type/size BEFORE materializing any blob.
/// Working bytes belong to the app's no-follow bounded filesystem adapter.
pub fn content(path: &Path, lookup: &BlobLookup) -> Result<BlobContent> {
    let repo = Repository::open(path)?;
    let id = match lookup {
        BlobLookup::Empty => return Ok(BlobContent::Missing),
        BlobLookup::Worktree { .. } => {
            bail!("Working bytes require the native host filesystem adapter")
        }
        BlobLookup::Tree { commit, path } => {
            validate_native_repo_path(path)?;
            let commit = checked_commit(&repo, oid(commit)?)?;
            let mut tree = checked_tree(&repo, commit.tree_id())?;
            let parts: Vec<_> = path.split('/').collect();
            let mut found = None;
            for (index, part) in parts.iter().enumerate() {
                let Some(entry) = tree.get_name(part) else {
                    return Ok(BlobContent::Missing);
                };
                let id = entry.id();
                let mode = entry.filemode();
                drop(entry);
                if index + 1 == parts.len() {
                    ensure!(
                        matches!(mode, 0o100644 | 0o100755 | 0o120000),
                        "Git entry is not a blob"
                    );
                    found = Some(id);
                } else {
                    ensure!(mode == 0o040000, "Git path ancestor is not a tree");
                    tree = checked_tree(&repo, id)?;
                }
            }
            found.ok_or_else(|| anyhow!("Missing Git tree path"))?
        }
        BlobLookup::Index { path } => {
            validate_native_repo_path(path)?;
            let index = repo.index()?;
            ensure!(
                index.len() <= cli::MAX_RECORDS,
                "Git index exceeds its entry limit"
            );
            // get_path follows core.ignorecase; object lookup requires exact
            // repository bytes, independently of the working filesystem.
            let mut found = None;
            for entry in index
                .iter()
                .filter(|entry| entry.path.as_slice() == path.as_bytes())
            {
                ensure!(entry.flags & 0x3000 == 0, "Git index path is conflicted");
                ensure!(found.replace(entry).is_none(), "Duplicate Git index path");
            }
            let Some(entry) = found else {
                return Ok(BlobContent::Missing);
            };
            ensure!(
                matches!(entry.mode, 0o100644 | 0o100755 | 0o120000),
                "Git index entry is not a blob"
            );
            entry.id
        }
    };
    let (size, kind) = repo.odb()?.read_header(id)?;
    ensure!(kind == ObjectType::Blob, "Git object is not a blob");
    if size > cli::MAX_BLOB_BYTES {
        return Ok(BlobContent::TooLarge);
    }
    let blob = repo.find_blob(id)?;
    ensure!(
        blob.content().len() == size,
        "Git blob size changed during read"
    );
    Ok(BlobContent::Bytes(blob.content().to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "mt-local-git-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Repository::init(&path).unwrap();
            Self(path)
        }
        fn commit(&self, bytes: &[u8]) -> ObjectId {
            std::fs::write(self.0.join("file"), bytes).unwrap();
            let repo = Repository::open(&self.0).unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("file")).unwrap();
            index.write().unwrap();
            let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
            let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
            object_id(
                repo.commit(Some("HEAD"), &signature, &signature, "root", &tree, &[])
                    .unwrap(),
            )
            .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn native_authority_unborn_and_qualified_history_need_no_git_process() {
        let fixture = Fixture::new();
        let authority = authority(&fixture.0).unwrap();
        assert_eq!(
            authority.worktree_root,
            canonical_directory(&fixture.0).unwrap()
        );
        assert_eq!(authority.git_dir, authority.common_dir);
        assert!(head(&fixture.0).unwrap().oid.is_none());
        let commit = fixture.commit(b"root\n");
        let reference = head(&fixture.0).unwrap().branch.unwrap();
        assert_eq!(resolve_ref(&fixture.0, &reference).unwrap(), commit);
        assert!(parents(&fixture.0, &commit).unwrap().is_empty());
        assert_eq!(
            history(&fixture.0, &[commit], 30).unwrap()[0].message,
            "root"
        );
        assert!(history(&fixture.0, &[], 30).unwrap().is_empty());
    }

    #[test]
    fn native_linked_authority_matches_libgit2_common_directory_suffix_semantics() {
        let fixture = Fixture::new();
        fixture.commit(b"root\n");
        let repo = Repository::open(&fixture.0).unwrap();
        repo.config()
            .unwrap()
            .set_str("user.name", "Fixture")
            .unwrap();
        repo.config()
            .unwrap()
            .set_str("user.email", "fixture@example.invalid")
            .unwrap();
        let linked = fixture.0.join("linked");
        repo.worktree("topic", &linked, None).unwrap();
        let metadata = Repository::open(&linked).unwrap().path().join("commondir");
        let value = std::fs::read_to_string(&metadata).unwrap();
        let value = value.trim_end_matches(|character| {
            matches!(character, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
        });
        let expected = canonical_directory(repo.path()).unwrap();
        for suffix in ["\n", "\r\n", " \t\r\n\x0b\x0c"] {
            std::fs::write(&metadata, format!("{value}{suffix}")).unwrap();
            assert!(Repository::open(&linked).unwrap().is_worktree());
            let found = authority(&linked).unwrap();
            assert!(found.is_linked_worktree());
            assert_eq!(found.common_dir, expected);
        }
    }

    #[test]
    fn native_blob_bounds_are_checked_for_old_and_index_objects() {
        let fixture = Fixture::new();
        let commit = fixture.commit(&vec![b'x'; cli::MAX_BLOB_BYTES + 1]);
        assert_eq!(
            content(
                &fixture.0,
                &BlobLookup::Tree {
                    commit,
                    path: "file".into()
                }
            )
            .unwrap(),
            BlobContent::TooLarge
        );
        assert_eq!(
            content(
                &fixture.0,
                &BlobLookup::Index {
                    path: "file".into()
                }
            )
            .unwrap(),
            BlobContent::TooLarge
        );
        assert_eq!(
            content(
                &fixture.0,
                &BlobLookup::Index {
                    path: "absent".into()
                }
            )
            .unwrap(),
            BlobContent::Missing
        );
    }

    #[test]
    fn native_index_lookup_is_exact_even_when_libgit2_ignores_case() {
        let fixture = Fixture::new();
        fixture.commit(b"exact bytes\n");
        let repo = Repository::open(&fixture.0).unwrap();
        repo.config()
            .unwrap()
            .set_bool("core.ignorecase", true)
            .unwrap();
        assert!(
            repo.index()
                .unwrap()
                .get_path(Path::new("FILE"), 0)
                .is_some()
        );
        assert_eq!(
            content(
                &fixture.0,
                &BlobLookup::Index {
                    path: "FILE".into()
                }
            )
            .unwrap(),
            BlobContent::Missing
        );
        assert_eq!(
            content(
                &fixture.0,
                &BlobLookup::Index {
                    path: "file".into()
                }
            )
            .unwrap(),
            BlobContent::Bytes(b"exact bytes\n".to_vec())
        );
    }

    #[test]
    fn native_index_lookup_rejects_conflicts_and_gitlinks() {
        let fixture = Fixture::new();
        fixture.commit(b"root\n");
        let repo = Repository::open(&fixture.0).unwrap();
        let mut index = repo.index().unwrap();
        let mut entry = index.get_path(Path::new("file"), 0).unwrap();
        index.clear().unwrap();
        entry.flags = (entry.flags & !0x3000) | 0x1000;
        index.add(&entry).unwrap();
        index.write().unwrap();
        assert!(
            content(
                &fixture.0,
                &BlobLookup::Index {
                    path: "file".into()
                }
            )
            .is_err()
        );
        index.clear().unwrap();
        entry.flags &= !0x3000;
        entry.mode = 0o160000;
        entry.id = repo.head().unwrap().target().unwrap();
        index.add(&entry).unwrap();
        index.write().unwrap();
        assert!(
            content(
                &fixture.0,
                &BlobLookup::Index {
                    path: "file".into()
                }
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn native_status_rejects_non_utf8_and_keeps_embedded_directories_separate() {
        use std::os::unix::ffi::OsStringExt;
        let fixture = Fixture::new();
        fixture.commit(b"root\n");
        let nested = Fixture(fixture.0.join("nested"));
        Repository::init(&nested.0).unwrap();
        nested.commit(b"nested\n");
        let state = status(&fixture.0).unwrap();
        assert!(
            state
                .untracked_directories
                .iter()
                .any(|path| path == "nested/")
        );
        assert!(state.changes.iter().all(|change| change.path != "nested/"));
        std::fs::write(
            fixture.0.join(std::ffi::OsString::from_vec(vec![0xff])),
            b"invalid",
        )
        .unwrap();
        assert!(status(&fixture.0).is_err());
    }
}
