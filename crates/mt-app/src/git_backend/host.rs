use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use mt_github::{CommandExecutionErrorKind, CommandOutput, CommandPlan};
use mt_project::git::cli::{self, CommandEffect, GitCommand};
use mt_ssh::SftpNodeKind;
use serde::Deserialize;

use crate::execution_host::{self, ExecutionBackend, ProjectExecutionSnapshot};
use crate::remote_ssh::{self, RemoteProjectContext};

use super::{GitError, GitErrorKind, GitResult, OwnedContent};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub(super) enum NodeKind {
    File,
    Directory,
    Symlink,
    Other,
}

impl From<SftpNodeKind> for NodeKind {
    fn from(kind: SftpNodeKind) -> Self {
        match kind {
            SftpNodeKind::File => Self::File,
            SftpNodeKind::Directory => Self::Directory,
            SftpNodeKind::Symlink => Self::Symlink,
            SftpNodeKind::Other => Self::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dispatch {
    NotDispatched,
    Completed,
    Uncertain,
}

pub(super) fn process_dispatch(backend: &ExecutionBackend, output: &CommandOutput) -> Dispatch {
    // WSL launcher loss is not a Git exit or proof that guest work stopped.
    if output.timed_out
        || output.exit_code.is_none()
        || matches!(backend, ExecutionBackend::Wsl { .. }) && output.exit_code == Some(-1)
    {
        Dispatch::Uncertain
    } else {
        Dispatch::Completed
    }
}

pub(super) struct Attempt {
    pub dispatch: Dispatch,
    pub output: Option<CommandOutput>,
    pub error: Option<GitError>,
}

impl Attempt {
    pub fn failure(dispatch: Dispatch, error: GitError) -> Self {
        Self {
            dispatch,
            output: None,
            error: Some(error),
        }
    }

    pub fn checked(self, plan: &GitCommand) -> GitResult<Vec<u8>> {
        if let Some(error) = self.error {
            return Err(error);
        }
        let output = self
            .output
            .ok_or_else(|| GitError::unavailable("Git output is unavailable"))?;
        if self.dispatch != Dispatch::Completed {
            return Err(GitError::unavailable(
                "Git command did not complete on its owning host",
            ));
        }
        if output.exit_code == Some(127) {
            return Err(GitError::new(
                GitErrorKind::Unavailable,
                "A required command is unavailable on the selected host",
            ));
        }
        if output.exit_code == Some(129) {
            return Err(GitError::new(
                GitErrorKind::Unsupported,
                "The selected host does not support this Git command",
            ));
        }
        plan.checked_stdout(cli::CapturedOutput {
            stdout: &output.stdout,
            stderr: &output.stderr,
            exit_code: output.exit_code,
            timed_out: output.timed_out,
            stdout_truncated: output.stdout_truncated,
            stderr_truncated: output.stderr_truncated,
        })
        .map_err(GitError::domain)?;
        Ok(output.stdout)
    }
}

pub(super) trait Host: Send + Sync {
    fn snapshot(&self) -> &ProjectExecutionSnapshot;
    fn run(&self, cwd: &str, plan: &GitCommand) -> Attempt;
    fn canonical_directory(&self, path: &str) -> GitResult<String>;
    fn kind(&self, path: &str) -> GitResult<Option<NodeKind>>;
    fn directories(&self, path: &str) -> GitResult<Vec<String>>;
    fn directory_empty(&self, path: &str) -> GitResult<bool>;
    fn file_kind(&self, root: &str, relative: &str) -> GitResult<Option<NodeKind>>;
    fn read_file(&self, root: &str, relative: &str) -> GitResult<OwnedContent>;
    fn remove_file(&self, root: &str, relative: &str) -> Attempt;
}

pub(super) struct ExecutionGitHost(pub ProjectExecutionSnapshot);

impl ExecutionGitHost {
    fn remote(&self) -> RemoteProjectContext {
        match &self.0.backend {
            ExecutionBackend::Ssh {
                connection,
                connection_fingerprint,
                connection_epoch,
            } => RemoteProjectContext::new(
                connection.clone(),
                *connection_fingerprint,
                *connection_epoch,
            ),
            _ => unreachable!("remote adapter requires SSH"),
        }
    }

    fn wsl_fs(
        &self,
        operation: &str,
        root: &str,
        relative: &str,
        limit: usize,
    ) -> GitResult<Vec<u8>> {
        let plan = auxiliary(
            "python3",
            &["-c", WSL_FS_SCRIPT, operation, root, relative],
            limit,
        );
        self.run(root, &plan).checked(&plan)
    }
}

impl Host for ExecutionGitHost {
    fn snapshot(&self) -> &ProjectExecutionSnapshot {
        &self.0
    }

    fn run(&self, cwd: &str, plan: &GitCommand) -> Attempt {
        match &self.0.backend {
            ExecutionBackend::Ssh { .. } => {
                let result = match remote_ssh::run_git_panel_command(&self.remote(), cwd, plan) {
                    Ok(result) => result,
                    Err(message) => {
                        return Attempt::failure(
                            Dispatch::NotDispatched,
                            GitError::unavailable(message),
                        );
                    }
                };
                let error = result.transport_error.or(result.authority_error);
                let Some(output) = result.output else {
                    return Attempt::failure(
                        Dispatch::Uncertain,
                        GitError::unavailable(
                            error.unwrap_or_else(|| "SSH Git reply is missing".into()),
                        ),
                    );
                };
                let dispatch = if !output.state.may_have_started() {
                    Dispatch::NotDispatched
                } else if error.is_none()
                    && !output.timed_out
                    && !output.requires_session_retirement()
                    && output.exit_code.is_some()
                {
                    Dispatch::Completed
                } else {
                    Dispatch::Uncertain
                };
                Attempt {
                    dispatch,
                    output: Some(CommandOutput {
                        stdout: output.stdout,
                        stderr: output.stderr,
                        exit_code: output.exit_code.and_then(|code| i32::try_from(code).ok()),
                        timed_out: output.timed_out,
                        stdout_truncated: output.stdout_truncated,
                        stderr_truncated: output.stderr_truncated,
                    }),
                    error: error.map(GitError::unavailable),
                }
            }
            ExecutionBackend::Local | ExecutionBackend::Wsl { .. } => {
                let mut snapshot = self.0.clone();
                snapshot.canonical_path = cwd.to_string();
                let command = CommandPlan::new(plan.program, plan.args.clone());
                match execution_host::execute_host_command(
                    &snapshot,
                    &command,
                    plan.timeout,
                    plan.stdout_limit.max(plan.stderr_limit),
                ) {
                    Ok(mut result) => {
                        let output = &mut result.output;
                        if output.stdout.len() > plan.stdout_limit {
                            output.stdout.truncate(plan.stdout_limit);
                            output.stdout_truncated = true;
                        }
                        if output.stderr.len() > plan.stderr_limit {
                            output.stderr.truncate(plan.stderr_limit);
                            output.stderr_truncated = true;
                        }
                        Attempt {
                            dispatch: process_dispatch(&snapshot.backend, output),
                            output: Some(result.output),
                            error: None,
                        }
                    }
                    Err(error) => Attempt::failure(
                        if error.kind == CommandExecutionErrorKind::ProgramNotFound {
                            Dispatch::NotDispatched
                        } else {
                            Dispatch::Uncertain
                        },
                        GitError::unavailable(error.message),
                    ),
                }
            }
        }
    }

    fn canonical_directory(&self, path: &str) -> GitResult<String> {
        match &self.0.backend {
            ExecutionBackend::Local => {
                let path = std::fs::canonicalize(path).map_err(GitError::io)?;
                if !std::fs::metadata(&path).map_err(GitError::io)?.is_dir() {
                    return Err(GitError::invalid("Git source path is not a directory"));
                }
                native_string(&native_canonical_spelling(path)?)
            }
            ExecutionBackend::Wsl { .. } => {
                serde_json::from_slice(&self.wsl_fs("canonical", path, "", 64 * 1024)?)
                    .map_err(GitError::domain)
            }
            ExecutionBackend::Ssh { .. } => {
                remote_ssh::git_canonical_directory(&self.remote(), path)
                    .map_err(GitError::unavailable)
            }
        }
    }

    fn kind(&self, path: &str) -> GitResult<Option<NodeKind>> {
        match &self.0.backend {
            ExecutionBackend::Local => local_kind(Path::new(path)),
            ExecutionBackend::Wsl { .. } => {
                serde_json::from_slice(&self.wsl_fs("kind", "/", path, 4096)?)
                    .map_err(GitError::domain)
            }
            ExecutionBackend::Ssh { .. } => remote_ssh::git_path_kind(&self.remote(), path)
                .map(|kind| kind.map(Into::into))
                .map_err(GitError::unavailable),
        }
    }

    fn directories(&self, path: &str) -> GitResult<Vec<String>> {
        match &self.0.backend {
            ExecutionBackend::Local => {
                let mut directories = Vec::new();
                for (index, entry) in std::fs::read_dir(path).map_err(GitError::io)?.enumerate() {
                    if index >= cli::MAX_RECORDS {
                        return Err(GitError::invalid(
                            "Git discovery directory exceeds its entry limit",
                        ));
                    }
                    let entry = entry.map_err(GitError::io)?;
                    if entry.file_type().map_err(GitError::io)?.is_dir() {
                        directories.push(native_string(&entry.path())?);
                    }
                }
                directories.sort();
                Ok(directories)
            }
            ExecutionBackend::Wsl { .. } => serde_json::from_slice(&self.wsl_fs(
                "directories",
                path,
                "",
                cli::MAX_LIST_BYTES,
            )?)
            .map_err(GitError::domain),
            ExecutionBackend::Ssh { .. } => {
                let listing = remote_ssh::browse_directory(&self.remote(), path)
                    .map_err(|error| GitError::unavailable(error.message))?;
                if listing.canonical_path != path {
                    return Err(GitError::stale());
                }
                Ok(listing
                    .directories
                    .into_iter()
                    .filter(|entry| !entry.is_symlink)
                    .map(|entry| entry.path)
                    .collect())
            }
        }
    }

    fn directory_empty(&self, path: &str) -> GitResult<bool> {
        match &self.0.backend {
            ExecutionBackend::Local => {
                match std::fs::read_dir(path).map_err(GitError::io)?.next() {
                    None => Ok(true),
                    Some(Ok(_)) => Ok(false),
                    Some(Err(error)) => Err(GitError::io(error)),
                }
            }
            ExecutionBackend::Wsl { .. } => {
                serde_json::from_slice(&self.wsl_fs("empty", path, "", 4096)?)
                    .map_err(GitError::domain)
            }
            ExecutionBackend::Ssh { .. } => {
                remote_ssh::git_directory_empty(&self.remote(), path).map_err(GitError::unavailable)
            }
        }
    }

    fn file_kind(&self, root: &str, relative: &str) -> GitResult<Option<NodeKind>> {
        match &self.0.backend {
            ExecutionBackend::Local => checked_local_leaf(root, relative).map(|(_, kind)| kind),
            ExecutionBackend::Wsl { .. } => {
                serde_json::from_slice(&self.wsl_fs("file_kind", root, relative, 4096)?)
                    .map_err(GitError::domain)
            }
            ExecutionBackend::Ssh { .. } => {
                remote_ssh::git_file_kind(&self.remote(), root, relative)
                    .map(|kind| kind.map(Into::into))
                    .map_err(GitError::unavailable)
            }
        }
    }

    fn read_file(&self, root: &str, relative: &str) -> GitResult<OwnedContent> {
        let kind = self.file_kind(root, relative)?;
        match kind {
            None => return Ok(OwnedContent::Missing),
            Some(NodeKind::Symlink) => {
                let bytes = match &self.0.backend {
                    ExecutionBackend::Local => {
                        let target = std::fs::read_link(checked_local_leaf(root, relative)?.0)
                            .map_err(GitError::io)?;
                        #[cfg(unix)]
                        {
                            use std::os::unix::ffi::OsStrExt;
                            target.as_os_str().as_bytes().to_vec()
                        }
                        #[cfg(not(unix))]
                        {
                            native_string(&target)?.into_bytes()
                        }
                    }
                    ExecutionBackend::Wsl { .. } => {
                        self.wsl_fs("readlink", root, relative, cli::MAX_PATH_BYTES)?
                    }
                    ExecutionBackend::Ssh { .. } => {
                        let target = remote_ssh::join_posix(root, relative);
                        let plan =
                            auxiliary("readlink", &["-n", "--", &target], cli::MAX_PATH_BYTES);
                        self.run(root, &plan).checked(&plan)?
                    }
                };
                if self.file_kind(root, relative)? != kind {
                    return Err(GitError::stale());
                }
                return Ok(OwnedContent::Bytes(bytes));
            }
            Some(NodeKind::File) => {}
            Some(_) => {
                return Err(GitError::new(
                    GitErrorKind::Unsupported,
                    "Git working entry is not a file or symlink",
                ));
            }
        }
        let result = match &self.0.backend {
            ExecutionBackend::Local => {
                let (target, _) = checked_local_leaf(root, relative)?;
                let mut options = std::fs::OpenOptions::new();
                options.read(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::OpenOptionsExt;
                    options.custom_flags(0x0020_0000);
                }
                let file = options.open(target).map_err(GitError::io)?;
                if !file.metadata().map_err(GitError::io)?.is_file() {
                    return Err(GitError::stale());
                }
                let mut bytes = Vec::new();
                file.take(cli::MAX_BLOB_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(GitError::io)?;
                OwnedContent::bounded(bytes)
            }
            ExecutionBackend::Wsl { .. } => OwnedContent::bounded(self.wsl_fs(
                "read",
                root,
                relative,
                cli::MAX_BLOB_BYTES + 1,
            )?),
            ExecutionBackend::Ssh { .. } => {
                match remote_ssh::git_read_regular_file(&self.remote(), root, relative)
                    .map_err(GitError::unavailable)?
                {
                    None => OwnedContent::Missing,
                    Some(mt_ssh::SftpBoundedFileRead::Complete(bytes)) => {
                        OwnedContent::Bytes(bytes)
                    }
                    Some(mt_ssh::SftpBoundedFileRead::TooLarge) => OwnedContent::TooLarge,
                }
            }
        };
        if self.file_kind(root, relative)? != kind {
            return Err(GitError::stale());
        }
        Ok(result)
    }

    fn remove_file(&self, root: &str, relative: &str) -> Attempt {
        match &self.0.backend {
            ExecutionBackend::Local => {
                let target = match checked_local_leaf(root, relative) {
                    Ok((target, Some(NodeKind::File | NodeKind::Symlink))) => target,
                    Ok(_) => return Attempt::failure(Dispatch::NotDispatched, GitError::stale()),
                    Err(error) => return Attempt::failure(Dispatch::NotDispatched, error),
                };
                match std::fs::remove_file(target) {
                    Ok(()) => completed_empty(),
                    Err(error) => Attempt::failure(Dispatch::Uncertain, GitError::io(error)),
                }
            }
            ExecutionBackend::Wsl { .. } => {
                let mut plan = auxiliary(
                    "python3",
                    &["-c", WSL_FS_SCRIPT, "remove", root, relative],
                    4096,
                );
                plan.effect = CommandEffect::Mutation;
                self.run(root, &plan)
            }
            ExecutionBackend::Ssh { .. } => {
                let result = remote_ssh::git_remove_file(&self.remote(), root, relative);
                match result.error {
                    None => completed_empty(),
                    Some(message) => Attempt::failure(
                        if result.dispatched {
                            Dispatch::Uncertain
                        } else {
                            Dispatch::NotDispatched
                        },
                        GitError::unavailable(message),
                    ),
                }
            }
        }
    }
}

fn completed_empty() -> Attempt {
    Attempt {
        dispatch: Dispatch::Completed,
        output: Some(CommandOutput {
            stdout: Vec::new(),
            stderr: Vec::new(),
            exit_code: Some(0),
            timed_out: false,
            stdout_truncated: false,
            stderr_truncated: false,
        }),
        error: None,
    }
}

pub(super) fn auxiliary(program: &'static str, args: &[&str], limit: usize) -> GitCommand {
    GitCommand {
        program,
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
        timeout: Duration::from_secs(30),
        stdout_limit: limit,
        stderr_limit: 64 * 1024,
        effect: CommandEffect::ReadOnly,
    }
}

fn native_string(path: &Path) -> GitResult<String> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| GitError::invalid("Git path is not valid UTF-8"))
}

fn native_canonical_spelling(path: PathBuf) -> GitResult<PathBuf> {
    native_string(&path)?;
    Ok(mt_project::fs::strip_verbatim_prefix(path))
}

fn local_kind(path: &Path) -> GitResult<Option<NodeKind>> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(GitError::io(error)),
    };
    Ok(Some(if metadata.file_type().is_symlink() {
        NodeKind::Symlink
    } else if metadata.is_dir() {
        NodeKind::Directory
    } else if metadata.is_file() {
        NodeKind::File
    } else {
        NodeKind::Other
    }))
}

fn checked_local_leaf(root: &str, relative: &str) -> GitResult<(PathBuf, Option<NodeKind>)> {
    cli::validate_repo_path(relative).map_err(GitError::domain)?;
    if !Path::new(relative)
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
        || cfg!(windows) && relative.contains(['\\', ':'])
    {
        return Err(GitError::invalid(
            "Git path is not an exact native file path",
        ));
    }
    let root_path = Path::new(root);
    if native_canonical_spelling(std::fs::canonicalize(root_path).map_err(GitError::io)?)?
        != root_path
    {
        return Err(GitError::stale());
    }
    let target = root_path.join(relative);
    let mut parent = root_path.to_path_buf();
    let parts: Vec<_> = relative.split('/').collect();
    for part in &parts[..parts.len() - 1] {
        parent.push(part);
        match local_kind(&parent)? {
            None => return Ok((target, None)),
            Some(NodeKind::Directory) => {}
            Some(_) => return Err(GitError::invalid("Git file parent is not a real directory")),
        }
        if native_canonical_spelling(std::fs::canonicalize(&parent).map_err(GitError::io)?)?
            != parent
        {
            return Err(GitError::stale());
        }
    }
    Ok((target.clone(), local_kind(&target)?))
}

// A WSL-only stdlib helper, invoked with structured argv in that distribution.
// File bytes stay raw; metadata/listings use JSON, with strict UTF-8 paths.
const WSL_FS_SCRIPT: &str = r#"
import json, os, stat, sys
op, root, rel = sys.argv[1:]
def emit(value):
    sys.stdout.buffer.write(json.dumps(value, ensure_ascii=False).encode('utf-8'))
def kind(path):
    try: mode = os.lstat(path).st_mode
    except FileNotFoundError: return None
    return 'Symlink' if stat.S_ISLNK(mode) else 'Directory' if stat.S_ISDIR(mode) else 'File' if stat.S_ISREG(mode) else 'Other'
def leaf():
    if os.path.realpath(root, strict=True) != root: raise RuntimeError('changed root')
    parts = rel.split('/')
    if any(p in ('', '.', '..', '.git') for p in parts): raise RuntimeError('invalid leaf')
    parent = root
    for part in parts[:-1]:
        parent = os.path.join(parent, part)
        k = kind(parent)
        if k is None: return os.path.join(root, rel), None
        if k != 'Directory' or os.path.realpath(parent, strict=True) != parent: raise RuntimeError('changed parent')
    path = os.path.join(root, rel)
    return path, kind(path)
if op == 'canonical':
    path = os.path.realpath(root, strict=True)
    if not os.path.isdir(path): raise RuntimeError('not directory')
    emit(path)
elif op == 'kind': emit(kind(rel))
elif op in ('directories', 'empty'):
    if os.path.realpath(root, strict=True) != root: raise RuntimeError('changed directory')
    result = []
    with os.scandir(root) as entries:
        for i, e in enumerate(entries):
            if i >= 20000: raise RuntimeError('directory entry limit')
            e.name.encode('utf-8')
            if op == 'empty': result.append(True); break
            if e.is_dir(follow_symlinks=False): result.append(e.path)
    emit(not result if op == 'empty' else sorted(result))
else:
    path, k = leaf()
    if op == 'file_kind': emit(k)
    elif op == 'readlink':
        if k != 'Symlink': raise RuntimeError('not symlink')
        sys.stdout.buffer.write(os.readlink(os.fsencode(path)))
    elif op == 'read':
        if k != 'File': raise RuntimeError('not regular file')
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(fd, 'rb') as f:
            if not stat.S_ISREG(os.fstat(f.fileno()).st_mode): raise RuntimeError('changed file')
            sys.stdout.buffer.write(f.read(1048577))
    elif op == 'remove':
        if k not in ('File', 'Symlink'): raise RuntimeError('not removable file')
        os.unlink(path)
    else: raise RuntimeError('unknown operation')
"#;
