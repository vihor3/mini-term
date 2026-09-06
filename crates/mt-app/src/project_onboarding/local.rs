use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mt_github::{CommandExecutionErrorKind, CommandOutput, CommandPlan};

use crate::execution_host::{PreProjectLocalContext, execute_pre_project_local_command};

use super::model::{
    DirectoryBrowseError, DirectoryBrowseErrorKind, DirectoryEntry, DirectoryListing,
    DirectoryLocation, DirectorySource, GitRelationship, HostPathProbe, OnboardingError,
    OnboardingErrorKind, ProjectLocationKey, TargetState,
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

/// One read-only directory listing. WSL uses its existing host-visible UNC I/O
/// boundary; it never interprets the distribution's POSIX path on the client.
pub(crate) fn browse_local_directory(
    requested: &DirectoryLocation,
) -> Result<DirectoryListing, DirectoryBrowseError> {
    if matches!(&requested.source, DirectorySource::Ssh { .. })
        || (!cfg!(windows) && matches!(&requested.source, DirectorySource::Wsl { .. }))
    {
        return Err(DirectoryBrowseError::new(
            DirectoryBrowseErrorKind::Unavailable,
            "The selected filesystem is not available through local I/O",
        ));
    }
    let mut location = requested.clone();
    if location.path == "~" || location.path.starts_with("~/") {
        let home = local_browser_home(&location.source)?;
        let suffix = location.path.strip_prefix("~/").unwrap_or("");
        location.path = match &location.source {
            DirectorySource::Local => Path::new(&home)
                .join(suffix)
                .to_string_lossy()
                .into_owned(),
            DirectorySource::Wsl { .. } => crate::remote_ssh::join_posix(&home, suffix),
            DirectorySource::Ssh { .. } => unreachable!(),
        };
    }
    let mut host_path = location.host_path()?;
    if let DirectorySource::Wsl { distro } = &location.source
        && location.path.split('/').any(|component| component == "..")
    {
        // Resolve links and parent components inside WSL before Win32 can
        // normalize the UNC spelling using different filesystem semantics.
        let output = execute_pre_project_local_command(
            &PreProjectLocalContext::Wsl {
                distro: distro.clone(),
                cwd: location.path.clone(),
            },
            &CommandPlan::new("pwd", ["-P"]),
            GIT_PROBE_TIMEOUT,
            GIT_ERROR_DETAIL_LIMIT,
        )
        .map_err(|error| {
            DirectoryBrowseError::new(DirectoryBrowseErrorKind::Unavailable, error.message)
        })?;
        location.path = wsl_browser_command_path(&output)?;
        host_path = location.host_path()?;
    }
    if host_path.contains('\0') || !Path::new(&host_path).is_absolute() {
        return Err(DirectoryBrowseError::new(
            DirectoryBrowseErrorKind::InvalidPath,
            "An absolute directory path is required",
        ));
    }
    let canonical = fs::canonicalize(&host_path)
        .map_err(DirectoryBrowseError::from_io)?;
    // The shared Windows prefix helper is lossy. Reject unrepresentable
    // canonical targets before that conversion, not after it.
    browser_path_text(&canonical)?;
    let canonical = mt_project::fs::strip_verbatim_prefix(canonical);
    let location = browser_location_from_host_path(&location.source, &canonical)?;
    if !fs::metadata(&canonical)
        .map_err(DirectoryBrowseError::from_io)?
        .is_dir()
    {
        return Err(DirectoryBrowseError::new(
            DirectoryBrowseErrorKind::InvalidPath,
            "The selected path is not a directory",
        ));
    }
    let mut directories = Vec::new();
    for (index, entry) in fs::read_dir(&canonical)
        .map_err(DirectoryBrowseError::from_io)?
        .enumerate()
    {
        crate::remote_ssh::validate_browser_entry_count(index + 1).map_err(|error| {
            DirectoryBrowseError::new(DirectoryBrowseErrorKind::Unavailable, error.message)
        })?;
        let entry = entry.map_err(DirectoryBrowseError::from_io)?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if (cfg!(windows) || matches!(&location.source, DirectorySource::Wsl { .. }))
            && validate_portable_basename(&name).is_err()
        {
            continue;
        }
        let file_type = entry.file_type().map_err(DirectoryBrowseError::from_io)?;
        if !file_type.is_dir()
            && !(file_type.is_symlink()
                && fs::metadata(entry.path()).is_ok_and(|metadata| metadata.is_dir()))
        {
            continue;
        }
        directories.push(DirectoryEntry {
            name,
            location: browser_location_from_host_path(&location.source, &entry.path())?,
            is_symlink: file_type.is_symlink(),
        });
    }
    directories.sort_by(|a, b| mt_project::fs::natural_cmp(&a.name, &b.name));
    Ok(DirectoryListing {
        location,
        directories,
    })
}

fn browser_path_text(path: &Path) -> Result<&str, DirectoryBrowseError> {
    let text = path.to_str().ok_or_else(|| {
        DirectoryBrowseError::new(
            DirectoryBrowseErrorKind::InvalidPath,
            "The directory path is not valid Unicode",
        )
    })?;
    #[cfg(windows)]
    for component in path.components() {
        if let std::path::Component::Normal(name) = component
            && name.to_str().is_none_or(|name| validate_portable_basename(name).is_err())
        {
            return Err(DirectoryBrowseError::new(
                DirectoryBrowseErrorKind::InvalidPath,
                "The directory requires a verbatim path that project registration cannot preserve",
            ));
        }
    }
    Ok(text)
}

fn browser_location_from_host_path(
    source: &DirectorySource,
    path: &Path,
) -> Result<DirectoryLocation, DirectoryBrowseError> {
    let path = browser_path_text(path)?;
    let path = match source {
        DirectorySource::Wsl { distro } => {
            let wsl = mt_core::parse_wsl_unc(path)
                .filter(|wsl| wsl.distro.eq_ignore_ascii_case(distro));
            wsl.ok_or_else(|| {
                DirectoryBrowseError::new(
                    DirectoryBrowseErrorKind::Unavailable,
                    "The directory no longer belongs to the selected WSL distribution",
                )
            })?
            .unix_path
        }
        DirectorySource::Local if mt_core::parse_wsl_unc(path).is_none() => path.to_string(),
        DirectorySource::Local | DirectorySource::Ssh { .. } => {
            return Err(DirectoryBrowseError::new(
                DirectoryBrowseErrorKind::Unavailable,
                "The directory no longer belongs to the selected filesystem",
            ));
        }
    };
    let location = DirectoryLocation {
        source: source.clone(),
        path,
    };
    location.host_path()?;
    Ok(location)
}

fn local_browser_home(source: &DirectorySource) -> Result<String, DirectoryBrowseError> {
    match source {
        DirectorySource::Local => dirs::home_dir()
            .and_then(|path| path.into_os_string().into_string().ok())
            .ok_or_else(|| {
                DirectoryBrowseError::new(
                    DirectoryBrowseErrorKind::Unavailable,
                    "The local home directory is unavailable",
                )
            }),
        DirectorySource::Wsl { distro } => {
            DirectoryLocation { source: source.clone(), path: "/".into() }.host_path()?;
            let output = execute_pre_project_local_command(
                &PreProjectLocalContext::Wsl {
                    distro: distro.clone(),
                    cwd: "/".into(),
                },
                &CommandPlan::new("printenv", ["HOME"]),
                GIT_PROBE_TIMEOUT,
                GIT_ERROR_DETAIL_LIMIT,
            )
            .map_err(|error| {
                DirectoryBrowseError::new(DirectoryBrowseErrorKind::Unavailable, error.message)
            })?;
            wsl_browser_command_path(&output)
        }
        DirectorySource::Ssh { .. } => Err(DirectoryBrowseError::new(
            DirectoryBrowseErrorKind::Unavailable,
            "SSH home directories require the authenticated host probe",
        )),
    }
}

fn wsl_browser_command_path(output: &CommandOutput) -> Result<String, DirectoryBrowseError> {
    if output.timed_out
        || output.stdout_truncated
        || output.stderr_truncated
        || output.exit_code != Some(0)
    {
        return Err(DirectoryBrowseError::new(
            DirectoryBrowseErrorKind::Unavailable,
            "The selected WSL directory is unavailable",
        ));
    }
    parse_wsl_browser_path(&output.stdout)
}

fn parse_wsl_browser_path(stdout: &[u8]) -> Result<String, DirectoryBrowseError> {
    let path = std::str::from_utf8(stdout).map_err(|_| {
        DirectoryBrowseError::new(
            DirectoryBrowseErrorKind::InvalidPath,
            "The WSL directory is not valid Unicode",
        )
    })?;
    // Remove only pwd/printenv's record terminator, not characters in the path.
    let path = path.strip_suffix('\n').unwrap_or(path);
    if path.chars().any(char::is_control) {
        return Err(DirectoryBrowseError::new(
            DirectoryBrowseErrorKind::InvalidPath,
            "The WSL directory cannot be represented by a host-visible path",
        ));
    }
    crate::execution_host::normalize_absolute_posix_path(path).map_err(|message| {
        DirectoryBrowseError::new(DirectoryBrowseErrorKind::InvalidPath, message)
    })
}

pub(crate) fn local_browser_places() -> Vec<(String, DirectoryLocation)> {
    let mut places = Vec::new();
    #[cfg(windows)]
    {
        // Read the drive bitmask, not every drive's contents. No new windows
        // crate feature is needed for this kernel32 ABI, as with file identity.
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetLogicalDrives() -> u32;
        }
        let drives = unsafe { GetLogicalDrives() };
        for index in 0..26u8 {
            if drives & (1u32 << index) != 0 {
                let path = format!("{}:\\", char::from(b'A' + index));
                places.push((
                    path.clone(),
                    DirectoryLocation {
                        source: DirectorySource::Local,
                        path,
                    },
                ));
            }
        }
    }
    #[cfg(not(windows))]
    places.push((
        "/".into(),
        DirectoryLocation {
            source: DirectorySource::Local,
            path: "/".into(),
        },
    ));
    for distro in mt_project::wsl_distros::list_wsl_distros() {
        places.push((
            format!("WSL: {}", distro.name),
            DirectoryLocation {
                source: DirectorySource::Wsl { distro: distro.name },
                path: "~".into(),
            },
        ));
    }
    places
}

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
                    format!(
                        "local directory cannot be inspected: {}: {error}",
                        canonical.display()
                    ),
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
                format!(
                    "local directory cannot be read: {}: {error}",
                    path.display()
                ),
            )
        })?;
        Ok(entries.next().is_none())
    }

    fn git_relationship(canonical: &Path) -> Result<GitRelationship, OnboardingError> {
        let canonical_string = canonical.to_string_lossy().to_string();
        let context =
            PreProjectLocalContext::from_host_path(&canonical_string).map_err(map_command_error)?;
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
        let mut output =
            execute_pre_project_local_command(&context, &modern, GIT_PROBE_TIMEOUT, GIT_OUTPUT_CAP)
                .map_err(map_command_error)?;
        let legacy = !output.timed_out
            && !output.stdout_truncated
            && !output.stderr_truncated
            && output.exit_code == Some(129)
            && String::from_utf8_lossy(&output.stderr).contains("path-format");
        if legacy {
            output = execute_pre_project_local_command(
                &context,
                &CommandPlan::new("git", ["rev-parse", "--show-toplevel", "--git-common-dir"]),
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
        let (top_level, common_dir) =
            canonicalize_git_paths(&context, &canonical_string, top_level, common_dir, legacy)?;
        if same_local_directory(canonical, Path::new(&top_level))? {
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
        let directory_empty = include_empty
            .then(|| Self::directory_empty(&canonical))
            .transpose()?;
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

    fn run_git(
        &self,
        cwd: &str,
        plan: &CommandPlan,
    ) -> Result<HostCommandOutcome, OnboardingError> {
        let context = PreProjectLocalContext::from_host_path(cwd).map_err(map_command_error)?;
        let output =
            execute_pre_project_local_command(&context, plan, GIT_MUTATION_TIMEOUT, GIT_OUTPUT_CAP)
                .map_err(map_command_error)?;
        Ok(local_command_outcome(output))
    }

    fn location_key(&self, path: &str) -> Result<ProjectLocationKey, OnboardingError> {
        Ok(ProjectLocationKey::Local {
            normalized_canonical_path: crate::execution_host::normalize_host_visible_project_path(
                path,
            )
            .map_err(|message| OnboardingError::new(OnboardingErrorKind::Validation, message))?,
        })
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
            format!(
                "Git marker cannot be inspected at {}: {error}",
                marker.display()
            ),
        )),
    }
}

fn parse_git_paths(stdout: &[u8]) -> Result<(&str, &str), OnboardingError> {
    let stdout = std::str::from_utf8(stdout).map_err(|_| {
        OnboardingError::new(
            OnboardingErrorKind::GitFailure,
            "Git returned non-UTF-8 paths",
        )
    })?;
    let lines: Vec<&str> = stdout.lines().collect();
    if lines.len() != 2
        || lines
            .iter()
            .any(|line| line.is_empty() || line.contains('\0'))
    {
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
            Ok((
                canonicalize_git_path(Path::new(top))?,
                canonicalize_git_path(&common)?,
            ))
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
                format!(
                    "Git path cannot be canonicalized: {}: {error}",
                    path.display()
                ),
            )
        })
}

fn same_local_directory(left: &Path, right: &Path) -> Result<bool, OnboardingError> {
    if mt_project::worktree::paths_equal(left, right) {
        return Ok(true);
    }

    #[cfg(windows)]
    {
        let left = windows_directory_handle(left)?;
        let right = windows_directory_handle(right)?;
        Ok(left.volume_serial_number == right.volume_serial_number
            && left.file_index == right.file_index)
    }

    #[cfg(not(windows))]
    {
        Ok(false)
    }
}

#[cfg(windows)]
struct WindowsDirectoryHandle {
    _file: fs::File,
    volume_serial_number: u32,
    file_index: u64,
}

#[cfg(windows)]
fn windows_directory_handle(path: &Path) -> Result<WindowsDirectoryHandle, OnboardingError> {
    use std::mem::MaybeUninit;
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::os::windows::io::AsRawHandle as _;

    const FILE_SHARE_ALL: u32 = 0x0000_0007;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

    let file = fs::OpenOptions::new()
        .read(true)
        .access_mode(0)
        .share_mode(FILE_SHARE_ALL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|error| windows_directory_identity_error(path, error))?;
    let mut information = MaybeUninit::<WindowsByHandleFileInformation>::uninit();
    // SAFETY: `file` owns a valid open handle and `information` points to a
    // correctly sized writable BY_HANDLE_FILE_INFORMATION value.
    let succeeded =
        unsafe { get_file_information_by_handle(file.as_raw_handle(), information.as_mut_ptr()) };
    if succeeded == 0 {
        return Err(windows_directory_identity_error(
            path,
            std::io::Error::last_os_error(),
        ));
    }
    // SAFETY: GetFileInformationByHandle reported success and initialized the value.
    let information = unsafe { information.assume_init() };

    Ok(WindowsDirectoryHandle {
        _file: file,
        volume_serial_number: information.volume_serial_number,
        file_index: (u64::from(information.file_index_high) << 32)
            | u64::from(information.file_index_low),
    })
}

#[cfg(windows)]
fn windows_directory_identity_error(path: &Path, error: std::io::Error) -> OnboardingError {
    OnboardingError::new(
        OnboardingErrorKind::GitFailure,
        format!(
            "Git repository directory identity cannot be read at {}: {error}",
            path.display()
        ),
    )
}

#[cfg(windows)]
#[repr(C)]
struct WindowsFileTime {
    _low_date_time: u32,
    _high_date_time: u32,
}

#[cfg(windows)]
#[repr(C)]
struct WindowsByHandleFileInformation {
    _file_attributes: u32,
    _creation_time: WindowsFileTime,
    _last_access_time: WindowsFileTime,
    _last_write_time: WindowsFileTime,
    volume_serial_number: u32,
    _file_size_high: u32,
    _file_size_low: u32,
    _number_of_links: u32,
    file_index_high: u32,
    file_index_low: u32,
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "GetFileInformationByHandle"]
    fn get_file_information_by_handle(
        handle: std::os::windows::io::RawHandle,
        information: *mut WindowsByHandleFileInformation,
    ) -> i32;
}

fn wsl_unc(distro: &str, path: &str) -> Result<String, OnboardingError> {
    crate::execution_host::wsl_host_visible_path(distro, path).map_err(|_| {
        OnboardingError::new(
            OnboardingErrorKind::GitFailure,
            "Git returned an invalid WSL path",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_reads_one_level_including_hidden_directories_without_creating_git() {
        let root = std::env::temp_dir().join(format!("mt-browser-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join(".hidden")).unwrap();
        fs::create_dir(root.join(".git")).unwrap();
        fs::create_dir(root.join("project2")).unwrap();
        fs::create_dir(root.join("project10")).unwrap();
        fs::create_dir(root.join("project2").join("nested")).unwrap();
        fs::write(root.join("ordinary-file"), b"unchanged").unwrap();
        let listing = browse_local_directory(&DirectoryLocation {
            source: DirectorySource::Local,
            path: root.to_str().unwrap().into(),
        }).unwrap();
        assert_eq!(listing.directories.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(),
            [".git", ".hidden", "project2", "project10"]);
        let empty = browse_local_directory(&listing.directories[1].location).unwrap();
        assert!(empty.directories.is_empty());
        assert!(!root.join(".hidden").join(".git").exists());
        assert_eq!(fs::read(root.join("ordinary-file")).unwrap(), b"unchanged");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn browser_includes_directory_symlinks_but_not_files_or_broken_links() {
        let root = std::env::temp_dir().join(format!("mt-browser-links-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("directory")).unwrap();
        fs::write(root.join("file"), b"keep").unwrap();
        std::os::unix::fs::symlink(root.join("directory"), root.join("directory-link")).unwrap();
        std::os::unix::fs::symlink(root.join("file"), root.join("file-link")).unwrap();
        std::os::unix::fs::symlink(root.join("missing"), root.join("broken-link")).unwrap();
        let listing = browse_local_directory(&DirectoryLocation {
            source: DirectorySource::Local,
            path: root.to_str().unwrap().into(),
        }).unwrap();
        assert_eq!(listing.directories.len(), 2);
        assert!(listing.directories.iter().any(|entry| entry.name == "directory-link" && entry.is_symlink));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn browser_projects_wsl_identity_and_classifies_typed_local_errors() {
        let source = DirectorySource::Wsl { distro: "Ubuntu".into() };
        let location = browser_location_from_host_path(&source, Path::new(r"\\wsl.localhost\Ubuntu\home\User")).unwrap();
        assert_eq!(location.path, "/home/User");
        assert_eq!(location.source, source);
        assert!(browser_location_from_host_path(&source, Path::new(r"\\wsl$\Debian\home\User")).is_err());
        assert!(browser_location_from_host_path(&source, Path::new("/client/home")).is_err());
        for (io_kind, expected) in [
            (std::io::ErrorKind::PermissionDenied, DirectoryBrowseErrorKind::PermissionDenied),
            (std::io::ErrorKind::NotFound, DirectoryBrowseErrorKind::InvalidPath),
            (std::io::ErrorKind::NotADirectory, DirectoryBrowseErrorKind::InvalidPath),
            (std::io::ErrorKind::TimedOut, DirectoryBrowseErrorKind::Unavailable),
        ] {
            assert_eq!(DirectoryBrowseError::from_io(std::io::Error::from(io_kind)).kind, expected);
        }
    }

    #[test]
    fn browser_wsl_home_removes_only_the_record_terminator_and_never_rewrites_path_data() {
        assert_eq!(parse_wsl_browser_path(b"/home/User\n").unwrap(), "/home/User");
        assert_eq!(parse_wsl_browser_path(b"/home/User").unwrap(), "/home/User");
        assert_eq!(parse_wsl_browser_path(b"/home/with space \n").unwrap(), "/home/with space ");
        for output in [b"/home/User\n\n".as_slice(), b"/home/User\r\n", b"/home/\xff\n", b"relative\n", b"/nul\0path\n"] {
            assert_eq!(parse_wsl_browser_path(output).unwrap_err().kind, DirectoryBrowseErrorKind::InvalidPath);
        }
    }

    #[test]
    fn browser_wsl_command_requires_complete_successful_physical_path_output() {
        let output = CommandOutput {
            stdout: b"/physical/parent\n".to_vec(),
            stderr: Vec::new(),
            exit_code: Some(0),
            timed_out: false,
            stdout_truncated: false,
            stderr_truncated: false,
        };
        assert_eq!(wsl_browser_command_path(&output).unwrap(), "/physical/parent");
        for index in 0..5 {
            let mut failed = output.clone();
            match index {
                0 => failed.timed_out = true,
                1 => failed.stdout_truncated = true,
                2 => failed.stderr_truncated = true,
                3 => failed.exit_code = Some(1),
                _ => failed.exit_code = None,
            }
            assert_eq!(wsl_browser_command_path(&failed).unwrap_err().kind, DirectoryBrowseErrorKind::Unavailable);
        }
    }

    #[cfg(unix)]
    #[test]
    fn browser_rejects_non_unicode_canonical_paths_before_lossy_prefix_conversion() {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/bad-\xff".to_vec()));
        assert_eq!(browser_path_text(&path).unwrap_err().kind, DirectoryBrowseErrorKind::InvalidPath);
        assert!(browser_location_from_host_path(&DirectorySource::Local, &path).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn browser_rejects_unpaired_surrogates_before_lossy_prefix_conversion() {
        use std::os::windows::ffi::OsStringExt;
        let mut units: Vec<u16> = r"\\?\C:\directory-".encode_utf16().collect();
        units.push(0xD800);
        let path = PathBuf::from(std::ffi::OsString::from_wide(&units));
        assert_eq!(browser_path_text(&path).unwrap_err().kind, DirectoryBrowseErrorKind::InvalidPath);
        assert!(browser_location_from_host_path(&DirectorySource::Local, &path).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn browser_rejects_verbatim_only_names_before_native_registration_reinterpretation() {
        for path in [r"\\?\C:\ordinary\directory", r"\\?\UNC\server\share\ordinary", r"\\?\UNC\wsl$\Ubuntu\home\User"] {
            assert_eq!(browser_path_text(Path::new(path)).unwrap(), path);
        }
        for path in [r"\\?\C:\directory.", r"\\?\C:\directory ", r"\\?\C:\CON", r"\\?\UNC\server\share\directory."] {
            assert_eq!(browser_path_text(Path::new(path)).unwrap_err().kind, DirectoryBrowseErrorKind::InvalidPath);
        }
    }

    use crate::project_onboarding::model::OnboardingOperationResult;
    use crate::project_onboarding::ops::{
        add_existing_folder, clone_from_url, create_new_project, initialize_existing_folder,
    };

    fn assert_same_canonical_directory(actual: &str, expected: &Path) {
        let expected = LocalProjectOps::canonical_directory(&expected.to_string_lossy()).unwrap();
        assert_eq!(
            mt_project::worktree::normalize_path_for_comparison(actual),
            mt_project::worktree::normalize_path_for_comparison(&expected.to_string_lossy())
        );
    }

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

    #[test]
    fn local_directory_identity_keeps_distinct_siblings_separate() {
        let root = std::env::temp_dir().join(format!(
            "mt-app-onboarding-directory-identity-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let left = root.join("left");
        let right = root.join("right");
        fs::create_dir_all(&left).unwrap();
        fs::create_dir_all(&right).unwrap();

        assert!(same_local_directory(&left, &left).unwrap());
        assert!(!same_local_directory(&left, &right).unwrap());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_add_and_initialize_preserve_existing_files_and_nested_root() {
        let root = std::env::temp_dir().join(format!(
            "mt-app-onboarding-existing-integration-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let project = root.join("existing project");
        let child = project.join("child");
        fs::create_dir_all(&child).unwrap();
        let sentinel = project.join("sentinel.bin");
        let child_sentinel = child.join("keep.txt");
        let sentinel_bytes = b"existing project contents\0stay intact";
        fs::write(&sentinel, sentinel_bytes).unwrap();
        fs::write(&child_sentinel, b"nested contents stay intact").unwrap();
        let project_path = project.to_string_lossy().to_string();

        let added = add_existing_folder(&LocalProjectOps, &project_path).unwrap();
        let added_path = match added {
            OnboardingOperationResult::ReadyToRegister(location) => location.canonical_path,
            OnboardingOperationResult::NestedRepository { .. } => {
                panic!("Add Existing unexpectedly returned a nested repository")
            }
        };
        assert_eq!(fs::read(&sentinel).unwrap(), sentinel_bytes);
        assert!(!project.join(".git").exists());

        let initialized = initialize_existing_folder(&LocalProjectOps, &project_path).unwrap();
        let initialized_path = match initialized {
            OnboardingOperationResult::ReadyToRegister(location) => location.canonical_path,
            OnboardingOperationResult::NestedRepository { .. } => {
                panic!("Initialize Existing unexpectedly returned a nested repository")
            }
        };
        assert_eq!(
            mt_project::worktree::normalize_path_for_comparison(&initialized_path),
            mt_project::worktree::normalize_path_for_comparison(&added_path)
        );
        assert_eq!(fs::read(&sentinel).unwrap(), sentinel_bytes);
        assert!(matches!(
            LocalProjectOps::git_relationship(Path::new(&initialized_path)).unwrap(),
            GitRelationship::RepositoryRoot { .. }
        ));

        let nested =
            initialize_existing_folder(&LocalProjectOps, &child.to_string_lossy()).unwrap();
        let repository_root = match nested {
            OnboardingOperationResult::NestedRepository {
                repository_root, ..
            } => repository_root,
            OnboardingOperationResult::ReadyToRegister(_) => {
                panic!("nested folder unexpectedly became its own repository")
            }
        };
        assert_eq!(
            mt_project::worktree::normalize_path_for_comparison(&repository_root),
            mt_project::worktree::normalize_path_for_comparison(&initialized_path)
        );
        assert!(!child.join(".git").exists());
        assert_eq!(
            fs::read(&child_sentinel).unwrap(),
            b"nested contents stay intact"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_clone_creates_an_exact_repository_in_the_selected_parent() {
        let root = std::env::temp_dir().join(format!(
            "mt-app-onboarding-clone-integration-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let source = root.join("source.git");
        let destination_parent = root.join("destination");
        fs::create_dir_all(&destination_parent).unwrap();
        let source_setup = LocalProjectOps
            .run_git(
                &root.to_string_lossy(),
                &CommandPlan::new("git", ["init", "--bare", "source.git"]),
            )
            .unwrap();
        assert!(source_setup.succeeded());
        let target = destination_parent.join("cloned project");

        let cloned = clone_from_url(
            &LocalProjectOps,
            "../source.git",
            &destination_parent.to_string_lossy(),
            "cloned project",
        )
        .unwrap();
        let canonical_path = match cloned {
            OnboardingOperationResult::ReadyToRegister(location) => location.canonical_path,
            OnboardingOperationResult::NestedRepository { .. } => {
                panic!("Clone unexpectedly returned a nested repository")
            }
        };

        assert_same_canonical_directory(&canonical_path, &target);
        assert!(matches!(
            LocalProjectOps::git_relationship(&target).unwrap(),
            GitRelationship::RepositoryRoot { .. }
        ));
        assert!(source.join("HEAD").is_file());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_new_folder_creates_an_exact_git_repository() {
        let root = std::env::temp_dir().join(format!(
            "mt-app-onboarding-new-folder-integration-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let target = root.join("new project with spaces");

        let created = create_new_project(
            &LocalProjectOps,
            &root.to_string_lossy(),
            "new project with spaces",
        )
        .unwrap();
        let canonical_path = match created {
            OnboardingOperationResult::ReadyToRegister(location) => location.canonical_path,
            OnboardingOperationResult::NestedRepository { .. } => {
                panic!("New Folder unexpectedly returned a nested repository")
            }
        };

        assert_same_canonical_directory(&canonical_path, &target);
        assert!(matches!(
            LocalProjectOps::git_relationship(&target).unwrap(),
            GitRelationship::RepositoryRoot { .. }
        ));

        fs::remove_dir_all(root).unwrap();
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

    #[cfg(windows)]
    #[test]
    fn windows_file_information_layout_matches_win32_abi() {
        assert_eq!(std::mem::size_of::<WindowsByHandleFileInformation>(), 52);
        assert_eq!(std::mem::align_of::<WindowsByHandleFileInformation>(), 4);
    }

    #[cfg(windows)]
    #[test]
    fn windows_directory_identity_accepts_distinct_path_aliases() {
        let root = std::env::temp_dir().join(format!(
            "mt-app-onboarding-windows-directory-identity-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let alias = root.join(".");

        assert!(!mt_project::worktree::paths_equal(&root, &alias));
        assert!(same_local_directory(&root, &alias).unwrap());

        fs::remove_dir_all(root).unwrap();
    }
}
