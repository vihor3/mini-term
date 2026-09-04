use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mt_github::{CommandExecutionErrorKind, CommandOutput, CommandPlan};

use crate::execution_host::{PreProjectLocalContext, execute_pre_project_local_command};

use super::model::{
    GitRelationship, HostPathProbe, OnboardingError, OnboardingErrorKind, ProjectLocationKey,
    TargetState,
};
use super::ops::{
    HostCommandDispatch, HostCommandOutcome, ProjectHostOps, bounded_lossy_diagnostic,
    is_proven_not_git_repository, validate_portable_basename,
};

const GIT_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const GIT_MUTATION_TIMEOUT: Duration = Duration::from_secs(180);
const GIT_OUTPUT_CAP: usize = 64 * 1024;
const GIT_ERROR_DETAIL_LIMIT: usize = 4 * 1024;

#[derive(Clone, Debug, Default)]
pub struct LocalProjectOps;

impl LocalProjectOps {
    fn canonical_directory(path: &str) -> Result<PathBuf, OnboardingError> {
        if path.is_empty() || path.contains('\0') || !Path::new(path).is_absolute() {
            return Err(OnboardingError::new(
                OnboardingErrorKind::Validation,
                "local project path must be a non-empty absolute path",
            ));
        }
        let canonical = fs::canonicalize(path)
            .map(mt_project::fs::strip_verbatim_prefix)
            .map_err(|error| {
                OnboardingError::new(
                    OnboardingErrorKind::Validation,
                    format!("local directory is not accessible: {path}: {error}"),
                )
            })?;
        if !fs::metadata(&canonical)
            .map_err(|error| {
                OnboardingError::new(
                    OnboardingErrorKind::Validation,
                    format!("local directory cannot be inspected: {}: {error}", canonical.display()),
                )
            })?
            .is_dir()
        {
            return Err(OnboardingError::new(
                OnboardingErrorKind::Validation,
                format!("local path is not a directory: {}", canonical.display()),
            ));
        }
        Ok(canonical)
    }

    fn directory_empty(path: &Path) -> Result<bool, OnboardingError> {
        let mut entries = fs::read_dir(path).map_err(|error| {
            OnboardingError::new(
                OnboardingErrorKind::Validation,
                format!("local directory cannot be read: {}: {error}", path.display()),
            )
        })?;
        Ok(entries.next().is_none())
    }

    fn git_relationship(canonical: &Path) -> Result<GitRelationship, OnboardingError> {
        let canonical_string = canonical.to_string_lossy().to_string();
        let context = PreProjectLocalContext::from_host_path(&canonical_string)
            .map_err(map_command_error)?;
        let marker_present = git_marker_present(canonical)?;
        let modern = CommandPlan::new(
            "git",
            [
                "rev-parse",
                "--path-format=absolute",
                "--show-toplevel",
                "--git-common-dir",
            ],
        );
        let mut output = execute_pre_project_local_command(
            &context,
            &modern,
            GIT_PROBE_TIMEOUT,
            GIT_OUTPUT_CAP,
        )
        .map_err(map_command_error)?;
        let legacy = !output.timed_out
            && !output.stdout_truncated
            && !output.stderr_truncated
            && output.exit_code == Some(129)
            && String::from_utf8_lossy(&output.stderr).contains("path-format");
        if legacy {
            output = execute_pre_project_local_command(
                &context,
                &CommandPlan::new(
                    "git",
                    ["rev-parse", "--show-toplevel", "--git-common-dir"],
                ),
                GIT_PROBE_TIMEOUT,
                GIT_OUTPUT_CAP,
            )
            .map_err(map_command_error)?;
        }
        if output.timed_out || output.stdout_truncated || output.stderr_truncated {
            return Err(OnboardingError::new(
                OnboardingErrorKind::GitFailure,
                format!("Git repository probe was incomplete at {canonical_string}"),
            ));
        }
        if output.exit_code != Some(0) {
            return classify_git_probe_failure(
                output.exit_code,
                &output.stderr,
                marker_present,
                &canonical_string,
            );
        }
        let (top_level, common_dir) = parse_git_paths(&output.stdout)?;
        let (top_level, common_dir) = canonicalize_git_paths(
            &context,
            &canonical_string,
            top_level,
            common_dir,
            legacy,
        )?;
        if mt_project::worktree::normalize_path_for_comparison(&canonical_string)
            == mt_project::worktree::normalize_path_for_comparison(&top_level)
        {
            Ok(GitRelationship::RepositoryRoot {
                top_level,
                common_dir,
            })
        } else {
            Ok(GitRelationship::NestedInRepository {
                top_level,
                common_dir,
            })
        }
    }
}

impl ProjectHostOps for LocalProjectOps {
    fn probe_existing_directory(
        &self,
        path: &str,
        include_empty: bool,
        inspect_git: bool,
    ) -> Result<HostPathProbe, OnboardingError> {
        let canonical = Self::canonical_directory(path)?;
        let directory_empty = include_empty.then(|| Self::directory_empty(&canonical)).transpose()?;
        let git = if inspect_git {
            Self::git_relationship(&canonical)?
        } else {
            GitRelationship::NotGit
        };
        Ok(HostPathProbe {
            canonical_path: canonical.to_string_lossy().to_string(),
            directory_empty,
            git,
            observed_connection_epoch: None,
        })
    }

    fn probe_target(&self, parent: &str, name: &str) -> Result<TargetState, OnboardingError> {
        self.validate_basename(name)?;
        let target = Self::canonical_directory(parent)?.join(name);
        let canonical_target = target.to_string_lossy().to_string();
        match fs::symlink_metadata(&target) {
            Ok(metadata) if metadata.file_type().is_dir() => {
                let canonical = Self::canonical_directory(&canonical_target)?;
                let canonical_target = canonical.to_string_lossy().to_string();
                if Self::directory_empty(&canonical)? {
                    Ok(TargetState::EmptyDirectory { canonical_target })
                } else {
                    Ok(TargetState::NonEmptyDirectory { canonical_target })
                }
            }
            Ok(_) => Ok(TargetState::Other { canonical_target }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(TargetState::Absent { canonical_target })
            }
            Err(error) => Err(OnboardingError::new(
                OnboardingErrorKind::Validation,
                format!("local target cannot be inspected: {canonical_target}: {error}"),
            )),
        }
    }

    fn create_directory_exclusive(&self, target: &str) -> Result<(), OnboardingError> {
        fs::create_dir(target).map_err(|error| {
            OnboardingError::new(
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    OnboardingErrorKind::Collision
                } else {
                    OnboardingErrorKind::Validation
                },
                format!("could not create local project directory {target}: {error}"),
            )
        })
    }

    fn remove_empty_directory(&self, target: &str) -> Result<(), OnboardingError> {
        fs::remove_dir(target).map_err(|error| {
            OnboardingError::new(
                OnboardingErrorKind::PostconditionFailed,
                format!("could not remove empty local directory {target}: {error}"),
            )
        })
    }

    fn run_git(&self, cwd: &str, plan: &CommandPlan) -> Result<HostCommandOutcome, OnboardingError> {
        let context = PreProjectLocalContext::from_host_path(cwd).map_err(map_command_error)?;
        let output = execute_pre_project_local_command(
            &context,
            plan,
            GIT_MUTATION_TIMEOUT,
            GIT_OUTPUT_CAP,
        )
        .map_err(map_command_error)?;
        Ok(local_command_outcome(output))
    }

    fn location_key(&self, path: &str) -> Result<ProjectLocationKey, OnboardingError> {
        Ok(ProjectLocationKey::Local {
            normalized_canonical_path: mt_project::worktree::normalize_path_for_comparison(path),
        })
    }

    fn join_path(&self, parent: &str, name: &str) -> String {
        Path::new(parent).join(name).to_string_lossy().to_string()
    }

    fn validate_basename(&self, name: &str) -> Result<(), OnboardingError> {
        validate_portable_basename(name)
    }
}

fn local_command_outcome(output: CommandOutput) -> HostCommandOutcome {
    HostCommandOutcome {
        dispatch: HostCommandDispatch::Completed,
        exit_code: output.exit_code,
        timed_out: output.timed_out,
        stdout_truncated: output.stdout_truncated,
        stderr_truncated: output.stderr_truncated,
        stderr: output.stderr,
        observed_connection_epoch: None,
    }
}

fn map_command_error(error: mt_github::CommandExecutionError) -> OnboardingError {
    OnboardingError::new(
        if error.kind == CommandExecutionErrorKind::ProgramNotFound {
            OnboardingErrorKind::GitUnavailable
        } else {
            OnboardingErrorKind::GitFailure
        },
        error.message,
    )
}

fn classify_git_probe_failure(
    exit_code: Option<i32>,
    stderr: &[u8],
    marker_present: bool,
    canonical_path: &str,
) -> Result<GitRelationship, OnboardingError> {
    if !marker_present && is_proven_not_git_repository(exit_code, stderr) {
        return Ok(GitRelationship::NotGit);
    }
    let kind = if exit_code == Some(127) {
        OnboardingErrorKind::GitUnavailable
    } else {
        OnboardingErrorKind::GitFailure
    };
    let detail = bounded_lossy_diagnostic(stderr, GIT_ERROR_DETAIL_LIMIT);
    let detail = if detail.is_empty() {
        String::new()
    } else {
        format!("; {detail}")
    };
    Err(OnboardingError::new(
        kind,
        format!("Git could not inspect {canonical_path}{detail}"),
    ))
}

fn git_marker_present(canonical_path: &Path) -> Result<bool, OnboardingError> {
    let marker = canonical_path.join(".git");
    match fs::symlink_metadata(&marker) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(OnboardingError::new(
            OnboardingErrorKind::GitFailure,
            format!("Git marker cannot be inspected at {}: {error}", marker.display()),
        )),
    }
}

fn parse_git_paths(stdout: &[u8]) -> Result<(&str, &str), OnboardingError> {
    let stdout = std::str::from_utf8(stdout).map_err(|_| {
        OnboardingError::new(OnboardingErrorKind::GitFailure, "Git returned non-UTF-8 paths")
    })?;
    let lines: Vec<&str> = stdout.lines().collect();
    if lines.len() != 2 || lines.iter().any(|line| line.is_empty() || line.contains('\0')) {
        return Err(OnboardingError::new(
            OnboardingErrorKind::GitFailure,
            "Git repository probe returned an ambiguous path set",
        ));
    }
    Ok((lines[0], lines[1]))
}

fn canonicalize_git_paths(
    context: &PreProjectLocalContext,
    cwd: &str,
    top: &str,
    common: &str,
    legacy: bool,
) -> Result<(String, String), OnboardingError> {
    match context {
        PreProjectLocalContext::Native { .. } => {
            let common = if legacy && !Path::new(common).is_absolute() {
                Path::new(cwd).join(common)
            } else {
                PathBuf::from(common)
            };
            Ok((canonicalize_git_path(Path::new(top))?, canonicalize_git_path(&common)?))
        }
        PreProjectLocalContext::Wsl { distro, cwd } => {
            let common = if legacy && !common.starts_with('/') {
                format!("{}/{}", cwd.trim_end_matches('/'), common)
            } else {
                common.to_string()
            };
            Ok((wsl_unc(distro, top)?, wsl_unc(distro, &common)?))
        }
    }
}

fn canonicalize_git_path(path: &Path) -> Result<String, OnboardingError> {
    fs::canonicalize(path)
        .map(mt_project::fs::strip_verbatim_prefix)
        .map(|path| path.to_string_lossy().to_string())
        .map_err(|error| {
            OnboardingError::new(
                OnboardingErrorKind::GitFailure,
                format!("Git path cannot be canonicalized: {}: {error}", path.display()),
            )
        })
}

fn wsl_unc(distro: &str, path: &str) -> Result<String, OnboardingError> {
    if !path.starts_with('/') || path.contains('\0') {
        return Err(OnboardingError::new(
            OnboardingErrorKind::GitFailure,
            "Git returned an invalid WSL path",
        ));
    }
    let tail = path.trim_start_matches('/').replace('/', "\\");
    Ok(if tail.is_empty() {
        format!(r"\\wsl.localhost\{distro}")
    } else {
        format!(r"\\wsl.localhost\{distro}\{tail}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_not_repository_response_is_not_git() {
        assert_eq!(
            classify_git_probe_failure(
                Some(128),
                b"fatal: not a git repository (or any parent): .git",
                false,
                "/repo",
            )
            .unwrap(),
            GitRelationship::NotGit
        );

        let error = classify_git_probe_failure(
            Some(128),
            b"fatal: detected dubious ownership in repository at '/repo'",
            false,
            "/repo",
        )
        .unwrap_err();
        assert_eq!(error.kind, OnboardingErrorKind::GitFailure);
    }

    #[test]
    fn git_marker_prevents_not_git_downgrade() {
        let error = classify_git_probe_failure(
            Some(128),
            b"fatal: not a git repository (or any parent): .git",
            true,
            "/repo",
        )
        .unwrap_err();

        assert_eq!(error.kind, OnboardingErrorKind::GitFailure);
    }

    #[test]
    fn local_directory_probe_rejects_relative_paths_before_resolution() {
        for path in ["", ".", "relative/repo", "relative\0repo"] {
            let error = LocalProjectOps::canonical_directory(path).unwrap_err();
            assert_eq!(error.kind, OnboardingErrorKind::Validation);
            assert!(error.message.contains("absolute path"));
        }
    }

    #[cfg(windows)]
    #[test]
    fn onboarding_canonical_paths_strip_windows_verbatim_prefixes() {
        let root = std::env::temp_dir().join(format!(
            "mt-app-onboarding-canonical-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let directory = LocalProjectOps::canonical_directory(&root.to_string_lossy()).unwrap();
        assert!(!directory.to_string_lossy().starts_with(r"\\?\"));

        let git_path = canonicalize_git_path(&root).unwrap();
        assert!(!git_path.starts_with(r"\\?\"));

        let _ = fs::remove_dir_all(root);
    }
}
