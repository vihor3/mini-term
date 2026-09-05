//! Strict decoders for the paired command plans. Successful complete capture
//! is a prerequisite, including for formats whose empty output is meaningful.

use std::collections::HashSet;

use anyhow::{Result, anyhow, bail, ensure};

use super::{
    EntryKind, GitRef, HeadState, IndexEntry, MAX_LIST_BYTES, MAX_LOG_COMMITS, MAX_PATH_BYTES,
    MAX_RECORDS, MAX_WORKTREES, ObjectId, RepositoryAuthority, RepositoryStatus, TreeEntry,
    validate_absolute_path, validate_oid, validate_repo_path,
};
use crate::git::{BranchInfo, ChangeFileStatus, CommitFileInfo, GitCommitInfo, GitStatus};

fn utf8(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes).map_err(|_| anyhow!("Git output is not valid UTF-8"))
}

fn bounded(bytes: &[u8], limit: usize) -> Result<()> {
    ensure!(bytes.len() <= limit, "Git output exceeded its parser limit");
    Ok(())
}

fn nul_fields(bytes: &[u8], limit: usize) -> Result<Vec<&[u8]>> {
    bounded(bytes, MAX_LIST_BYTES)?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let body = bytes
        .strip_suffix(b"\0")
        .ok_or_else(|| anyhow!("Incomplete NUL framing"))?;
    let fields: Vec<_> = body.split(|byte| *byte == 0).take(limit + 1).collect();
    ensure!(fields.len() <= limit, "Too many Git output records");
    Ok(fields)
}

fn line(bytes: &[u8], limit: usize) -> Result<&str> {
    bounded(bytes, limit)?;
    utf8(
        bytes
            .strip_suffix(b"\n")
            .ok_or_else(|| anyhow!("Incomplete Git line"))?,
    )
}

fn path(bytes: &[u8]) -> Result<String> {
    let value = utf8(bytes)?;
    validate_repo_path(value)?;
    Ok(value.to_string())
}

fn mode(value: &str) -> Result<u32> {
    ensure!(
        value.len() == 6 && value.bytes().all(|b| (b'0'..=b'7').contains(&b)),
        "Invalid Git file mode"
    );
    let mode = u32::from_str_radix(value, 8)?;
    ensure!(
        matches!(
            mode,
            0 | 0o040000 | 0o100644 | 0o100755 | 0o120000 | 0o160000
        ),
        "Unsupported Git file mode"
    );
    Ok(mode)
}

fn decimal(value: &str) -> Result<u64> {
    ensure!(
        !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()),
        "Invalid Git number"
    );
    Ok(value.parse()?)
}

pub fn parse_repository_authority(
    worktree_root: &[u8],
    git_dir: &[u8],
    common_dir: &[u8],
) -> Result<RepositoryAuthority> {
    let parse = |bytes| -> Result<String> {
        let value = line(bytes, MAX_PATH_BYTES + 1)?;
        validate_absolute_path(value)?;
        Ok(value.to_string())
    };
    Ok(RepositoryAuthority {
        worktree_root: parse(worktree_root)?,
        git_dir: parse(git_dir)?,
        common_dir: parse(common_dir)?,
    })
}

pub fn parse_object_id(bytes: &[u8]) -> Result<ObjectId> {
    ObjectId::parse(line(bytes, 65)?)
}

fn status_code(code: u8) -> Result<Option<GitStatus>> {
    Ok(match code {
        b'.' => None,
        b'M' | b'T' => Some(GitStatus::Modified),
        b'A' | b'C' => Some(GitStatus::Added),
        b'D' => Some(GitStatus::Deleted),
        b'R' => Some(GitStatus::Renamed),
        _ => bail!("Unsupported Git status code"),
    })
}

fn submodule(value: &str) -> Result<bool> {
    let bytes = value.as_bytes();
    ensure!(
        value == "N..."
            || (bytes.len() == 4
                && bytes[0] == b'S'
                && matches!(bytes[1], b'.' | b'C')
                && matches!(bytes[2], b'.' | b'M')
                && matches!(bytes[3], b'.' | b'U')),
        "Invalid Git submodule status"
    );
    Ok(bytes[0] == b'S' && bytes[1..].iter().any(|byte| *byte != b'.'))
}

fn object_width(value: &str, width: &mut Option<usize>) -> Result<()> {
    let width = width.get_or_insert(value.len());
    ensure!(value.len() == *width, "Mixed Git object ID formats");
    Ok(())
}

fn status_hashes(values: &[&str], width: &mut Option<usize>) -> Result<()> {
    for value in values {
        validate_oid(value, true)?;
        object_width(value, width)?;
    }
    Ok(())
}

fn similarity(value: &str) -> Result<u8> {
    let (kind, score) = if let Some(score) = value.strip_prefix('R') {
        (b'R', score)
    } else if let Some(score) = value.strip_prefix('C') {
        (b'C', score)
    } else {
        bail!("Invalid Git similarity kind");
    };
    ensure!(
        score.len() <= 3 && decimal(score)? <= 100,
        "Invalid Git similarity score"
    );
    Ok(kind)
}

pub fn parse_status(bytes: &[u8]) -> Result<RepositoryStatus> {
    let records = nul_fields(bytes, MAX_RECORDS * 2 + 16)?;
    let mut records = records.into_iter();
    let mut oid: Option<Option<ObjectId>> = None;
    let mut branch: Option<Option<GitRef>> = None;
    let mut width = None;
    let mut changes = Vec::new();
    let mut untracked_directories = Vec::new();
    let mut paths = HashSet::new();
    let mut saw_entries = false;
    while let Some(record) = records.next() {
        let record = utf8(record)?;
        if let Some(header) = record.strip_prefix("# ") {
            ensure!(!saw_entries, "Git status header follows file entries");
            if let Some(value) = header.strip_prefix("branch.oid ") {
                ensure!(oid.is_none(), "Duplicate Git HEAD object header");
                oid = Some(if value == "(initial)" {
                    None
                } else {
                    Some(ObjectId::parse(value)?)
                });
                if value != "(initial)" {
                    object_width(value, &mut width)?;
                }
            } else if let Some(value) = header.strip_prefix("branch.head ") {
                ensure!(branch.is_none(), "Duplicate Git HEAD branch header");
                branch = Some(if value == "(detached)" {
                    None
                } else {
                    Some(GitRef::parse(&format!("refs/heads/{value}"))?)
                });
            }
            continue;
        }
        saw_entries = true;
        let head = oid
            .as_ref()
            .ok_or_else(|| anyhow!("Missing Git status HEAD header"))?;
        ensure!(branch.is_some(), "Missing Git status branch header");
        let (file_path, old_path, staged_status, unstaged_status) =
            if let Some(value) = record.strip_prefix("? ") {
                if let Some(directory) = value.strip_suffix('/') {
                    ensure!(
                        value.len() <= MAX_PATH_BYTES,
                        "Git directory path exceeds its limit"
                    );
                    validate_repo_path(directory)?;
                    ensure!(
                        paths.insert(directory.to_string()),
                        "Duplicate Git status path"
                    );
                    ensure!(paths.len() <= MAX_RECORDS, "Too many Git status entries");
                    untracked_directories.push(value.to_string());
                    continue;
                }
                (
                    path(value.as_bytes())?,
                    None,
                    None,
                    Some(if head.is_none() {
                        GitStatus::Added
                    } else {
                        GitStatus::Untracked
                    }),
                )
            } else if let Some(value) = record.strip_prefix("! ") {
                // Ignored directories may have one trailing slash. Never render
                // or turn ignored entries into a mutation target.
                validate_repo_path(value.strip_suffix('/').unwrap_or(value))?;
                continue;
            } else {
                let kind = record
                    .as_bytes()
                    .first()
                    .copied()
                    .ok_or_else(|| anyhow!("Empty Git status record"))?;
                let count = match kind {
                    b'1' => 9,
                    b'2' => 10,
                    b'u' => 11,
                    _ => bail!("Unsupported Git status record kind"),
                };
                let fields: Vec<_> = record.splitn(count, ' ').collect();
                ensure!(
                    fields.len() == count && fields[0].len() == 1,
                    "Incomplete Git status record"
                );
                let xy = fields[1].as_bytes();
                ensure!(xy.len() == 2, "Invalid Git XY status");
                let dirty_submodule = submodule(fields[2])?;
                if kind == b'u' {
                    ensure!(
                        matches!(fields[1], "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU"),
                        "Invalid Git conflict status"
                    );
                    for value in &fields[3..7] {
                        mode(value)?;
                    }
                    status_hashes(&fields[7..10], &mut width)?;
                    (
                        path(fields[10].as_bytes())?,
                        None,
                        Some(GitStatus::Conflicted),
                        Some(GitStatus::Conflicted),
                    )
                } else {
                    for value in &fields[3..6] {
                        mode(value)?;
                    }
                    status_hashes(&fields[6..8], &mut width)?;
                    let staged = status_code(xy[0])?;
                    let mut unstaged = status_code(xy[1])?;
                    if dirty_submodule && unstaged.is_none() {
                        unstaged = Some(GitStatus::Modified);
                    }
                    ensure!(
                        staged.is_some() || unstaged.is_some(),
                        "Unchanged Git status entry"
                    );
                    let old_path = if kind == b'2' {
                        ensure!(!fields[8].is_empty(), "Missing Git rename score");
                        let rename_kind = similarity(fields[8])?;
                        ensure!(
                            xy.contains(&rename_kind),
                            "Git rename status/score mismatch"
                        );
                        Some(path(
                            records
                                .next()
                                .ok_or_else(|| anyhow!("Missing Git rename source"))?,
                        )?)
                    } else {
                        ensure!(
                            !xy.iter().any(|c| matches!(c, b'R' | b'C')),
                            "Git rename is missing source framing"
                        );
                        None
                    };
                    (
                        path(fields[count - 1].as_bytes())?,
                        old_path,
                        staged,
                        unstaged,
                    )
                }
            };
        ensure!(paths.insert(file_path.clone()), "Duplicate Git status path");
        ensure!(
            old_path.as_deref() != Some(file_path.as_str()),
            "Git rename source equals destination"
        );
        ensure!(paths.len() <= MAX_RECORDS, "Too many Git status entries");
        let label = staged_status
            .as_ref()
            .or(unstaged_status.as_ref())
            .map(crate::git::status_label)
            .unwrap_or("")
            .to_string();
        changes.push(ChangeFileStatus {
            path: file_path,
            old_path,
            staged_status,
            unstaged_status,
            status_label: label,
        });
    }
    let head = HeadState {
        oid: oid.ok_or_else(|| anyhow!("Git status is missing branch.oid"))?,
        branch: branch.ok_or_else(|| anyhow!("Git status is missing branch.head"))?,
    };
    ensure!(
        head.oid.is_some() || head.branch.is_some(),
        "Unborn HEAD cannot be detached"
    );
    Ok(RepositoryStatus {
        head,
        changes,
        untracked_directories,
    })
}

pub fn parse_branches(bytes: &[u8], head: Option<&ObjectId>) -> Result<Vec<BranchInfo>> {
    bounded(bytes, MAX_LIST_BYTES)?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let body = bytes
        .strip_suffix(b"\n")
        .ok_or_else(|| anyhow!("Incomplete Git ref framing"))?;
    let mut branches = Vec::new();
    let mut seen = HashSet::new();
    let mut width = head.map(|head| head.as_str().len());
    for (index, record) in body.split(|byte| *byte == b'\n').enumerate() {
        ensure!(index < MAX_RECORDS, "Too many Git refs");
        let fields = nul_fields(record, 3)?;
        ensure!(fields.len() == 3, "Invalid Git ref record");
        let reference = GitRef::parse(utf8(fields[0])?)?;
        let oid = ObjectId::parse(utf8(fields[1])?)?;
        object_width(oid.as_str(), &mut width)?;
        ensure!(seen.insert(reference.clone()), "Duplicate Git ref");
        if !fields[2].is_empty() {
            GitRef::parse(utf8(fields[2])?)?;
            continue;
        }
        if reference.is_remote() && reference.short_name().ends_with("/HEAD") {
            continue;
        }
        branches.push(BranchInfo {
            name: reference.short_name().to_string(),
            // Preserve local DTO behavior: all local refs at HEAD's OID match.
            is_head: !reference.is_remote() && head == Some(&oid),
            is_remote: reference.is_remote(),
            commit_hash: oid.as_str().to_string(),
        });
    }
    Ok(branches)
}

fn parents(value: &str, commit: &ObjectId) -> Result<Vec<ObjectId>> {
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for value in value.split(' ') {
        ensure!(
            result.len() < MAX_LOG_COMMITS,
            "Too many Git commit parents"
        );
        let parent = ObjectId::parse(value)?;
        ensure!(
            parent.as_str().len() == commit.as_str().len(),
            "Mixed Git object ID formats"
        );
        ensure!(parent != *commit, "Git commit is its own parent");
        result.push(parent);
    }
    Ok(result)
}

pub fn parse_log(bytes: &[u8]) -> Result<Vec<GitCommitInfo>> {
    bounded(bytes, MAX_LIST_BYTES)?;
    let mut remaining = bytes;
    let mut commits = Vec::new();
    let mut seen = HashSet::new();
    let mut width = None;
    // --log-size counts the complete pretty-format payload before -z's record
    // terminator. Do not let embedded delimiters manufacture another commit.
    while !remaining.is_empty() {
        ensure!(commits.len() < MAX_LOG_COMMITS, "Too many Git log commits");
        let header_end = remaining
            .iter()
            .take(32)
            .position(|byte| *byte == b'\n')
            .ok_or_else(|| anyhow!("Missing Git log size header"))?;
        let size = utf8(&remaining[..header_end])?
            .strip_prefix("log size ")
            .ok_or_else(|| anyhow!("Invalid Git log size header"))?;
        let size = usize::try_from(decimal(size)?)?;
        remaining = &remaining[header_end + 1..];
        ensure!(size < remaining.len(), "Incomplete Git log payload");
        let fields = nul_fields(&remaining[..size + 1], 6)?;
        ensure!(fields.len() == 6, "Invalid Git log field framing");
        remaining = &remaining[size + 1..];
        let hash = ObjectId::parse(utf8(fields[0])?)?;
        object_width(hash.as_str(), &mut width)?;
        ensure!(seen.insert(hash.clone()), "Duplicate Git log commit");
        let parent_hashes = parents(utf8(fields[1])?, &hash)?
            .into_iter()
            .map(|oid| oid.as_str().to_string())
            .collect();
        let author = utf8(fields[2])?;
        let timestamp = utf8(fields[3])?;
        decimal(timestamp.strip_prefix('-').unwrap_or(timestamp))?;
        let message = utf8(fields[4])?;
        ensure!(
            !author.contains(['\r', '\n']) && !message.contains(['\r', '\n']),
            "Invalid Git log text framing"
        );
        let body = utf8(fields[5])?;
        commits.push(GitCommitInfo {
            hash: hash.as_str().to_string(),
            short_hash: hash.as_str()[..7].to_string(),
            message: message.to_string(),
            body: if body.is_empty() {
                None
            } else {
                Some(body.to_string())
            },
            author: if author.is_empty() {
                "unknown".to_string()
            } else {
                author.to_string()
            },
            timestamp: timestamp.parse()?,
            parent_hashes,
        });
    }
    Ok(commits)
}

pub fn parse_commit_parents(bytes: &[u8], commit: &ObjectId) -> Result<Vec<ObjectId>> {
    let value = line(bytes, MAX_LIST_BYTES)?;
    ensure!(!value.ends_with(' '), "Incomplete Git parent list");
    let (oid, rest) = value.split_once(' ').unwrap_or((value, ""));
    ensure!(
        ObjectId::parse(oid)? == *commit,
        "Git parent result belongs to another commit"
    );
    parents(rest, commit)
}

pub fn parse_commit_files(bytes: &[u8]) -> Result<Vec<CommitFileInfo>> {
    let fields = nul_fields(bytes, MAX_RECORDS * 3)?;
    let mut fields = fields.into_iter();
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    while let Some(status) = fields.next() {
        let status = utf8(status)?;
        let first_path = path(
            fields
                .next()
                .ok_or_else(|| anyhow!("Missing Git diff path"))?,
        )?;
        let (status, old_path, file_path) = match status {
            "A" => ("added", None, first_path),
            "D" => ("deleted", None, first_path),
            "M" | "T" => ("modified", None, first_path),
            value if value.starts_with(['R', 'C']) => {
                let kind = similarity(value)?;
                let destination = path(
                    fields
                        .next()
                        .ok_or_else(|| anyhow!("Missing Git diff rename destination"))?,
                )?;
                ensure!(
                    first_path != destination,
                    "Git diff rename source equals destination"
                );
                (
                    if kind == b'R' { "renamed" } else { "added" },
                    Some(first_path),
                    destination,
                )
            }
            _ => bail!("Unsupported Git commit file status"),
        };
        ensure!(
            seen.insert(file_path.clone()),
            "Duplicate Git commit file path"
        );
        ensure!(result.len() < MAX_RECORDS, "Too many Git commit files");
        result.push(CommitFileInfo {
            path: file_path,
            status: status.to_string(),
            old_path,
        });
    }
    Ok(result)
}

pub fn parse_tree_entry(bytes: &[u8], expected_path: &str) -> Result<Option<TreeEntry>> {
    validate_repo_path(expected_path)?;
    let records = nul_fields(bytes, 1)?;
    let Some(record) = records.first() else {
        return Ok(None);
    };
    let (meta, file_path) = entry_fields(record)?;
    ensure!(
        path(file_path)? == expected_path,
        "Git tree lookup matched another path"
    );
    let fields: Vec<_> = utf8(meta)?.splitn(4, ' ').collect();
    ensure!(fields.len() == 4, "Invalid Git tree entry fields");
    let mode = mode(fields[0])?;
    let kind = match (fields[1], mode) {
        ("blob", 0o100644 | 0o100755 | 0o120000) => EntryKind::Blob,
        ("tree", 0o040000) => EntryKind::Tree,
        ("commit", 0o160000) => EntryKind::Commit,
        _ => bail!("Invalid Git tree entry kind/mode"),
    };
    // Only the size column is space-padded by ls-tree -l.
    let size_field = fields[3].trim_start_matches(' ');
    let size = if kind == EntryKind::Blob {
        Some(decimal(size_field)?)
    } else {
        ensure!(size_field == "-", "Non-blob Git entry has a size");
        None
    };
    Ok(Some(TreeEntry {
        oid: ObjectId::parse(fields[2])?,
        mode,
        kind,
        size,
    }))
}

/// Conflict stages are an error, never proof that the index side is absent.
pub fn parse_index_entry(bytes: &[u8], expected_path: &str) -> Result<Option<IndexEntry>> {
    validate_repo_path(expected_path)?;
    let records = nul_fields(bytes, 3)?;
    let mut result = None;
    for record in records {
        let (meta, file_path) = entry_fields(record)?;
        ensure!(
            path(file_path)? == expected_path,
            "Git index lookup matched another path"
        );
        let fields: Vec<_> = utf8(meta)?.splitn(4, ' ').collect();
        ensure!(fields.len() == 3, "Invalid Git index entry fields");
        ensure!(
            matches!(fields[2], "0" | "1" | "2" | "3"),
            "Invalid Git index stage"
        );
        ensure!(
            fields[2] == "0",
            "Git index file has unresolved conflict stages"
        );
        ensure!(result.is_none(), "Duplicate Git index entry");
        let mode = mode(fields[0])?;
        ensure!(
            matches!(mode, 0o100644 | 0o100755 | 0o120000),
            "Git index entry is not a blob"
        );
        result = Some(IndexEntry {
            oid: ObjectId::parse(fields[1])?,
            mode,
        });
    }
    Ok(result)
}

fn entry_fields(record: &[u8]) -> Result<(&[u8], &[u8])> {
    let tab = record
        .iter()
        .position(|byte| *byte == b'\t')
        .ok_or_else(|| anyhow!("Invalid Git entry framing"))?;
    Ok((&record[..tab], &record[tab + 1..]))
}

pub fn parse_blob_size(bytes: &[u8]) -> Result<u64> {
    decimal(line(bytes, 32)?)
}

pub fn parse_worktrees(bytes: &[u8]) -> Result<Vec<crate::worktree::WorktreeFact>> {
    bounded(bytes, MAX_LIST_BYTES)?;
    ensure!(bytes.ends_with(b"\0\0"), "Incomplete Git worktree framing");
    ensure!(
        bytes.windows(2).filter(|pair| *pair == b"\0\0").count() <= MAX_WORKTREES
            && bytes.iter().filter(|byte| **byte == 0).count() <= MAX_WORKTREES * 16,
        "Too many Git worktree records or fields"
    );
    let result = crate::worktree::parse_porcelain_with_path_semantics(
        crate::worktree::WorktreePorcelainMode::Nul,
        bytes,
        crate::worktree::WorktreePathSemantics::Posix,
    )?;
    ensure!(
        !result.is_empty() && result.len() <= MAX_WORKTREES,
        "Invalid Git worktree inventory size"
    );
    let mut width = None;
    for fact in &result {
        validate_absolute_path(
            fact.path
                .to_str()
                .ok_or_else(|| anyhow!("Invalid Git worktree path encoding"))?,
        )?;
        if fact.is_bare {
            ensure!(
                fact.head.is_none() && fact.branch_ref.is_none() && !fact.is_detached,
                "Bare Git worktree has checkout fields"
            );
        } else {
            ensure!(fact.head.is_some(), "Git worktree is missing HEAD");
        }
        if let Some(head) = fact.head.as_deref() {
            validate_oid(head, true)?;
            object_width(head, &mut width)?;
        }
        if let Some(branch) = fact.branch_ref.as_deref() {
            ensure!(
                !GitRef::parse(branch)?.is_remote(),
                "Worktree has a nonlocal branch ref"
            );
        }
    }
    Ok(result)
}
