//! Authoritative local Git worktree discovery.
//!
//! Git porcelain is the source of truth. Callers that may remove persisted
//! state must check [`WorktreeScan::authoritative`] before treating absence as
//! evidence that a worktree registration disappeared.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

mod catalog;
pub mod identity;
mod porcelain;

pub use catalog::{current_generation, invalidate, scan};
pub use identity::{
    LOCAL_HOST_FINGERPRINT, ResolvedWorktreeIdentity, WorktreeIdentitySource,
    local_execution_host_id, resolve_local, resolve_provisional_local, resolve_provisional_ssh,
    resolve_provisional_wsl,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeScanSource {
    PorcelainZ,
    PorcelainText,
    LastKnown,
    Libgit2Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitAnnotation {
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorktreePathState {
    Present,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeFact {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch_ref: Option<String>,
    pub is_main: bool,
    pub is_detached: bool,
    pub is_bare: bool,
    pub is_sparse: bool,
    pub locked: Option<GitAnnotation>,
    pub prunable: Option<GitAnnotation>,
    pub path_state: WorktreePathState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeScan {
    pub generation: u64,
    pub source: WorktreeScanSource,
    pub authoritative: bool,
    pub worktrees: Vec<WorktreeFact>,
    pub warning: Option<String>,
}

/// Normalize a local path for comparison across persisted strings and Git
/// output. Windows paths are case-insensitive; POSIX paths retain case.
pub fn normalize_path_for_comparison(path: &str) -> String {
    if cfg!(windows) {
        let unified: String = path
            .chars()
            .map(|c| if c == '\\' { '/' } else { c })
            .collect();
        unified.trim_end_matches('/').to_lowercase()
    } else {
        let trimmed = path.trim_end_matches('/');
        if trimmed.is_empty() && path.starts_with('/') {
            "/".to_string()
        } else {
            trimmed.to_string()
        }
    }
}

pub fn paths_equal(left: &Path, right: &Path) -> bool {
    normalize_path_for_comparison(&left.to_string_lossy())
        == normalize_path_for_comparison(&right.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_comparison_preserves_posix_case() {
        if cfg!(windows) {
            assert_eq!(
                normalize_path_for_comparison(r"D:\Git\Repo\"),
                "d:/git/repo"
            );
        } else {
            assert_eq!(
                normalize_path_for_comparison("/home/U/Proj/"),
                "/home/U/Proj"
            );
            assert_ne!(
                normalize_path_for_comparison("/home/U/Proj"),
                normalize_path_for_comparison("/home/u/proj")
            );
            assert_eq!(normalize_path_for_comparison(r"/tmp/a\b"), r"/tmp/a\b");
            assert_ne!(
                normalize_path_for_comparison(r"/tmp/a\b"),
                normalize_path_for_comparison("/tmp/a/b")
            );
        }
    }
}
