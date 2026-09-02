//! Stable local and provisional worktree identity resolution.
//!
//! Local paths are canonicalized on the local execution host. WSL and SSH
//! inputs are deliberately provisional and pure: they use compatibility facts
//! already persisted by the client and do not claim remote runtime authority.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use git2::Repository;
use mt_identity::{ExecutionHostId, HostInstallId, RepoId, WorktreeId};
use serde::{Deserialize, Serialize};

use super::catalog::common_git_dir;
use super::normalize_path_for_comparison;

pub const LOCAL_HOST_FINGERPRINT: &str = "local";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeIdentitySource {
    AuthoritativeLocalGit,
    LocalDirectory,
    AuthoritativeRemoteGit,
    AuthoritativeRemoteDirectory,
    ProvisionalLocal,
    ProvisionalWsl,
    ProvisionalSsh,
    PersistedFallback,
}

impl WorktreeIdentitySource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuthoritativeLocalGit => "authoritativeLocalGit",
            Self::LocalDirectory => "localDirectory",
            Self::AuthoritativeRemoteGit => "authoritativeRemoteGit",
            Self::AuthoritativeRemoteDirectory => "authoritativeRemoteDirectory",
            Self::ProvisionalLocal => "provisionalLocal",
            Self::ProvisionalWsl => "provisionalWsl",
            Self::ProvisionalSsh => "provisionalSsh",
            Self::PersistedFallback => "persistedFallback",
        }
    }

    pub const fn is_authoritative(self) -> bool {
        matches!(
            self,
            Self::AuthoritativeLocalGit
                | Self::LocalDirectory
                | Self::AuthoritativeRemoteGit
                | Self::AuthoritativeRemoteDirectory
        )
    }
}

impl std::fmt::Display for WorktreeIdentitySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedWorktreeIdentity {
    pub execution_host_id: ExecutionHostId,
    pub repo_id: RepoId,
    pub worktree_id: WorktreeId,
    pub canonical_worktree_path: String,
    pub canonical_git_common_dir: Option<String>,
    pub source: WorktreeIdentitySource,
}

pub fn local_execution_host_id(install: &HostInstallId) -> ExecutionHostId {
    ExecutionHostId::derive(LOCAL_HOST_FINGERPRINT, install)
}

pub fn resolve_local(
    install: &HostInstallId,
    worktree_path: &Path,
) -> Result<ResolvedWorktreeIdentity> {
    let canonical_input = canonicalize_directory(worktree_path)?;
    let execution_host_id = local_execution_host_id(install);

    match Repository::open(&canonical_input) {
        Ok(repository) => {
            let canonical_worktree = repository
                .workdir()
                .map(canonicalize_directory)
                .transpose()?
                .unwrap_or(canonical_input);
            let canonical_common_dir = fs::canonicalize(common_git_dir(&repository))
                .context("failed to canonicalize Git common directory")?;
            let canonical_worktree_path = local_path_string(&canonical_worktree)?;
            let canonical_git_common_dir = local_path_string(&canonical_common_dir)?;
            let repo_id = RepoId::derive(&execution_host_id, &canonical_git_common_dir);
            let worktree_id = WorktreeId::derive(&repo_id, &canonical_worktree_path, None);
            Ok(ResolvedWorktreeIdentity {
                execution_host_id,
                repo_id,
                worktree_id,
                canonical_worktree_path,
                canonical_git_common_dir: Some(canonical_git_common_dir),
                source: WorktreeIdentitySource::AuthoritativeLocalGit,
            })
        }
        Err(error) => {
            let git_marker = canonical_input.join(".git");
            match fs::symlink_metadata(&git_marker) {
                Ok(_) => Err(anyhow!(
                    "failed to open Git repository at {}: {error}",
                    canonical_input.display()
                )),
                Err(marker_error) if marker_error.kind() == std::io::ErrorKind::NotFound => {
                    let canonical_worktree_path = local_path_string(&canonical_input)?;
                    let repo_id = RepoId::derive(&execution_host_id, &canonical_worktree_path);
                    let worktree_id = WorktreeId::derive(&repo_id, &canonical_worktree_path, None);
                    Ok(ResolvedWorktreeIdentity {
                        execution_host_id,
                        repo_id,
                        worktree_id,
                        canonical_worktree_path,
                        canonical_git_common_dir: None,
                        source: WorktreeIdentitySource::LocalDirectory,
                    })
                }
                Err(marker_error) => Err(marker_error).with_context(|| {
                    format!("failed to inspect Git marker at {}", git_marker.display())
                }),
            }
        }
    }
}

pub fn resolve_provisional_wsl(
    install: &HostInstallId,
    distro: &str,
    host_visible_path: &str,
) -> Result<ResolvedWorktreeIdentity> {
    let normalized_distro = normalize_required_value(distro, "WSL distro")?.to_ascii_lowercase();
    let unc_candidate = host_visible_path.replace('/', "\\");
    let posix_path = if let Some(parsed) = mt_core::parse_wsl_unc(&unc_candidate) {
        if parsed.distro.to_ascii_lowercase() != normalized_distro {
            bail!(
                "WSL path distro {} does not match requested distro {}",
                parsed.distro,
                distro.trim()
            );
        }
        normalize_posix_absolute(&parsed.unix_path)?
    } else if host_visible_path.starts_with("//") || host_visible_path.starts_with(r"\\") {
        bail!("path is not a recognized WSL UNC path: {host_visible_path}");
    } else {
        normalize_posix_absolute(host_visible_path)?
    };
    let canonical_worktree_path = canonical_wsl_host_path(&normalized_distro, &posix_path);
    let host_fingerprint = format!("provisional-wsl:{normalized_distro}");
    Ok(provisional_identity(
        ExecutionHostId::derive(&host_fingerprint, install),
        canonical_worktree_path,
        WorktreeIdentitySource::ProvisionalWsl,
    ))
}

pub fn resolve_provisional_local(
    install: &HostInstallId,
    host_visible_path: &str,
) -> Result<ResolvedWorktreeIdentity> {
    validate_local_fallback_path(host_visible_path)?;
    let canonical_worktree_path = normalize_path_for_comparison(host_visible_path);
    if canonical_worktree_path.is_empty() {
        bail!("local fallback path must not normalize to an empty value");
    }
    Ok(provisional_identity(
        local_execution_host_id(install),
        canonical_worktree_path,
        WorktreeIdentitySource::ProvisionalLocal,
    ))
}

pub fn resolve_provisional_ssh(
    install: &HostInstallId,
    stable_connection_id: &str,
    remote_path: &str,
) -> Result<ResolvedWorktreeIdentity> {
    let stable_connection_id = normalize_required_value(stable_connection_id, "SSH connection ID")?;
    let canonical_worktree_path = normalize_posix_absolute(remote_path)?;
    let host_fingerprint = format!("provisional-ssh:{stable_connection_id}");
    Ok(provisional_identity(
        ExecutionHostId::derive(&host_fingerprint, install),
        canonical_worktree_path,
        WorktreeIdentitySource::ProvisionalSsh,
    ))
}

fn provisional_identity(
    execution_host_id: ExecutionHostId,
    canonical_worktree_path: String,
    source: WorktreeIdentitySource,
) -> ResolvedWorktreeIdentity {
    let repo_id = RepoId::derive(&execution_host_id, &canonical_worktree_path);
    let worktree_id = WorktreeId::derive(&repo_id, &canonical_worktree_path, None);
    ResolvedWorktreeIdentity {
        execution_host_id,
        repo_id,
        worktree_id,
        canonical_worktree_path,
        canonical_git_common_dir: None,
        source,
    }
}

fn canonicalize_directory(path: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path)
        .with_context(|| format!("failed to canonicalize directory {}", path.display()))?;
    let metadata = fs::metadata(&canonical)
        .with_context(|| format!("failed to inspect directory {}", canonical.display()))?;
    if !metadata.is_dir() {
        bail!("worktree path is not a directory: {}", canonical.display());
    }
    Ok(canonical)
}

fn validate_local_fallback_path(path: &str) -> Result<()> {
    if path.is_empty() {
        bail!("local fallback path must not be empty");
    }
    if path.contains('\0') {
        bail!("local fallback path must not contain NUL");
    }
    if !Path::new(path).is_absolute() {
        bail!("local fallback path must be absolute: {path}");
    }
    Ok(())
}

fn local_path_string(path: &Path) -> Result<String> {
    let value = path
        .to_str()
        .ok_or_else(|| anyhow!("canonical path is not valid UTF-8: {}", path.display()))?;
    Ok(normalize_path_for_comparison(value))
}

fn normalize_required_value<'a>(value: &'a str, label: &str) -> Result<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        bail!("{label} must not be empty");
    }
    if value.contains('\0') {
        bail!("{label} must not contain NUL");
    }
    Ok(value)
}

fn normalize_posix_absolute(path: &str) -> Result<String> {
    if path.is_empty() {
        bail!("worktree path must not be empty");
    }
    if path.contains('\0') {
        bail!("worktree path must not contain NUL");
    }
    if !path.starts_with('/') {
        bail!("worktree path must be absolute POSIX path: {path}");
    }

    let mut components = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    bail!("worktree path escapes the POSIX root: {path}");
                }
            }
            value => components.push(value),
        }
    }
    if components.is_empty() {
        Ok("/".to_string())
    } else {
        Ok(format!("/{}", components.join("/")))
    }
}

fn canonical_wsl_host_path(distro: &str, posix_path: &str) -> String {
    if posix_path == "/" {
        format!(r"\\wsl.localhost\{distro}")
    } else {
        format!(
            r"\\wsl.localhost\{distro}\{}",
            posix_path.trim_start_matches('/').replace('/', "\\")
        )
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn fixed_install() -> HostInstallId {
        "install-v1:123e4567-e89b-42d3-a456-426614174000"
            .parse()
            .unwrap()
    }

    fn unique_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("mini-term-identity-{label}-{nonce}"))
    }

    #[test]
    fn local_directory_identity_is_stable_and_host_qualified() {
        let root = unique_root("directory");
        fs::create_dir_all(&root).unwrap();

        let first = resolve_local(&fixed_install(), &root).unwrap();
        let second = resolve_local(&fixed_install(), &root).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.source, WorktreeIdentitySource::LocalDirectory);
        assert!(first.canonical_git_common_dir.is_none());

        let other_install = HostInstallId::new();
        let other_host = resolve_local(&other_install, &root).unwrap();
        assert_ne!(first.execution_host_id, other_host.execution_host_id);
        assert_ne!(first.worktree_id, other_host.worktree_id);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn main_and_linked_worktrees_share_repo_but_not_worktree_identity() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }

        let root = unique_root("git");
        let repo = root.join("repo");
        let linked = root.join("linked");
        fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]);
        run_git(&repo, &["config", "user.email", "test@example.com"]);
        run_git(&repo, &["config", "user.name", "Test"]);
        fs::write(repo.join("file.txt"), "one").unwrap();
        run_git(&repo, &["add", "file.txt"]);
        run_git(&repo, &["commit", "-m", "initial"]);
        run_git(
            &repo,
            &["worktree", "add", "-b", "feature", linked.to_str().unwrap()],
        );

        let install = fixed_install();
        let main_identity = resolve_local(&install, &repo).unwrap();
        let linked_identity = resolve_local(&install, &linked).unwrap();
        assert_eq!(
            main_identity.source,
            WorktreeIdentitySource::AuthoritativeLocalGit
        );
        assert_eq!(
            linked_identity.source,
            WorktreeIdentitySource::AuthoritativeLocalGit
        );
        assert_eq!(main_identity.repo_id, linked_identity.repo_id);
        assert_eq!(
            main_identity.canonical_git_common_dir,
            linked_identity.canonical_git_common_dir
        );
        assert_ne!(main_identity.worktree_id, linked_identity.worktree_id);
        assert_ne!(
            main_identity.canonical_worktree_path,
            linked_identity.canonical_worktree_path
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn provisional_wsl_inputs_are_pure_and_normalized() {
        let install = fixed_install();
        let old_unc =
            resolve_provisional_wsl(&install, "Ubuntu", r"\\wsl$\Ubuntu\home\User\project\")
                .unwrap();
        let canonical_unc = resolve_provisional_wsl(
            &install,
            "ubuntu",
            r"\\?\UNC\wsl.localhost\UBUNTU\home\User\project",
        )
        .unwrap();
        assert_eq!(old_unc, canonical_unc);
        assert_eq!(
            old_unc.canonical_worktree_path,
            r"\\wsl.localhost\ubuntu\home\User\project"
        );
        assert_eq!(old_unc.source, WorktreeIdentitySource::ProvisionalWsl);
        assert!(resolve_provisional_wsl(&install, "Debian", r"\\wsl$\Ubuntu\home").is_err());
    }

    #[test]
    fn provisional_local_is_pure_normalized_and_host_qualified() {
        let install = fixed_install();
        let (path, equivalent, expected) = if cfg!(windows) {
            (
                r"C:\Missing\Project\",
                "c:/missing/project",
                "c:/missing/project",
            )
        } else {
            (
                "/tmp/Missing/Project/",
                "/tmp/Missing/Project",
                "/tmp/Missing/Project",
            )
        };

        let first = resolve_provisional_local(&install, path).unwrap();
        let second = resolve_provisional_local(&install, equivalent).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.canonical_worktree_path, expected);
        assert_eq!(first.source, WorktreeIdentitySource::ProvisionalLocal);
        assert!(!first.source.is_authoritative());
        assert_eq!(
            first.repo_id,
            RepoId::derive(&local_execution_host_id(&install), expected)
        );
        assert_eq!(
            first.worktree_id,
            WorktreeId::derive(&first.repo_id, expected, None)
        );

        let other_install = HostInstallId::new();
        let other_host = resolve_provisional_local(&other_install, equivalent).unwrap();
        assert_ne!(first.execution_host_id, other_host.execution_host_id);
        assert_ne!(first.worktree_id, other_host.worktree_id);
        assert!(resolve_provisional_local(&install, "relative/path").is_err());
        assert!(resolve_provisional_local(&install, "").is_err());
        assert!(resolve_provisional_local(&install, "/bad\0path").is_err());
    }

    #[test]
    fn provisional_ssh_uses_connection_id_and_normalized_remote_path() {
        let install = fixed_install();
        let first =
            resolve_provisional_ssh(&install, "connection-1", "/srv//repo/./child/..").unwrap();
        let equivalent = resolve_provisional_ssh(&install, "connection-1", "/srv/repo").unwrap();
        let other_connection =
            resolve_provisional_ssh(&install, "connection-2", "/srv/repo").unwrap();

        assert_eq!(first, equivalent);
        assert_eq!(first.canonical_worktree_path, "/srv/repo");
        assert_eq!(first.source, WorktreeIdentitySource::ProvisionalSsh);
        assert_ne!(first.execution_host_id, other_connection.execution_host_id);
        assert_ne!(first.worktree_id, other_connection.worktree_id);
        assert!(resolve_provisional_ssh(&install, "connection-1", "srv/repo").is_err());
        assert_eq!(
            resolve_provisional_ssh(&install, "connection-1", "/srv/repo ")
                .unwrap()
                .canonical_worktree_path,
            "/srv/repo "
        );
    }

    #[test]
    fn identity_source_serializes_in_camel_case() {
        assert_eq!(
            serde_json::to_string(&WorktreeIdentitySource::AuthoritativeLocalGit).unwrap(),
            "\"authoritativeLocalGit\""
        );
        assert_eq!(
            serde_json::to_string(&WorktreeIdentitySource::PersistedFallback).unwrap(),
            "\"persistedFallback\""
        );
        assert_eq!(
            serde_json::to_string(&WorktreeIdentitySource::ProvisionalLocal).unwrap(),
            "\"provisionalLocal\""
        );
        assert_eq!(
            WorktreeIdentitySource::AuthoritativeLocalGit.as_str(),
            "authoritativeLocalGit"
        );
        assert!(WorktreeIdentitySource::LocalDirectory.is_authoritative());
        assert!(WorktreeIdentitySource::AuthoritativeRemoteGit.is_authoritative());
        assert!(WorktreeIdentitySource::AuthoritativeRemoteDirectory.is_authoritative());
        assert_eq!(
            serde_json::to_string(&WorktreeIdentitySource::AuthoritativeRemoteGit).unwrap(),
            "\"authoritativeRemoteGit\""
        );
        assert!(!WorktreeIdentitySource::ProvisionalSsh.is_authoritative());
    }

    fn run_git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
