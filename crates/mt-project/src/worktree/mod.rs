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

/// Captured Git worktree porcelain encoding. Host-specific command runners
/// pass complete stdout through this boundary so every caller shares the same
/// strict field, quoting, duplicate, and main-row rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreePorcelainMode {
    Nul,
    Text,
}

/// Filesystem comparison rules for captured worktree paths. Native scans use
/// the client platform rules; WSL and SSH captures always use POSIX rules even
/// when mini-term itself runs on Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreePathSemantics {
    Native,
    Posix,
}

pub fn parse_porcelain(
    mode: WorktreePorcelainMode,
    bytes: &[u8],
) -> anyhow::Result<Vec<WorktreeFact>> {
    parse_porcelain_with_path_semantics(mode, bytes, WorktreePathSemantics::Native)
}

pub fn parse_porcelain_with_path_semantics(
    mode: WorktreePorcelainMode,
    bytes: &[u8],
    path_semantics: WorktreePathSemantics,
) -> anyhow::Result<Vec<WorktreeFact>> {
    match mode {
        WorktreePorcelainMode::Nul => {
            porcelain::parse_porcelain_z_with_path_semantics(bytes, path_semantics)
        }
        WorktreePorcelainMode::Text => {
            porcelain::parse_porcelain_text_with_path_semantics(bytes, path_semantics)
        }
    }
}

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
    fn public_porcelain_boundary_preserves_mode_parity() {
        let nul = parse_porcelain(
            WorktreePorcelainMode::Nul,
            b"worktree /repo linked\0HEAD abc\0branch refs/heads/feature\0locked busy\0unknown future\0\0",
        )
        .unwrap();
        let text = parse_porcelain(
            WorktreePorcelainMode::Text,
            b"worktree \"/repo linked\"\nHEAD abc\nbranch refs/heads/feature\nlocked \"busy\"\nunknown future\n\n",
        )
        .unwrap();
        assert_eq!(nul, text);
    }

    #[test]
    fn public_porcelain_boundary_rejects_malformed_or_conflicting_capture() {
        assert!(
            parse_porcelain(
                WorktreePorcelainMode::Nul,
                b"worktree /repo\0branch refs/heads/x\0detached\0\0",
            )
            .is_err()
        );
        assert!(
            parse_porcelain(
                WorktreePorcelainMode::Text,
                b"worktree \"/repo\\q\"\n\n",
            )
            .is_err()
        );
        assert!(
            parse_porcelain(WorktreePorcelainMode::Nul, b"worktree \xff\0\0").is_err()
        );
    }

    #[test]
    fn public_porcelain_boundary_uses_explicit_posix_path_semantics() {
        let distinct_case = b"worktree /srv/Repo\0\0worktree /srv/repo\0\0";
        assert!(
            parse_porcelain_with_path_semantics(
                WorktreePorcelainMode::Nul,
                distinct_case,
                WorktreePathSemantics::Posix,
            )
            .is_ok()
        );
        assert!(
            parse_porcelain_with_path_semantics(
                WorktreePorcelainMode::Nul,
                b"worktree /srv/repo\0\0worktree /srv/repo/\0\0",
                WorktreePathSemantics::Posix,
            )
            .is_err()
        );
        if cfg!(windows) {
            assert!(parse_porcelain(WorktreePorcelainMode::Nul, distinct_case).is_err());
        }
    }

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
