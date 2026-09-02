//! Authenticated SSH execution-host identity and bounded remote inventory.
//!
//! The runtime deliberately reuses one pooled russh session. SFTP and exec
//! operations open independent channels, so identity bootstrap and inventory
//! do not create a second transport or hold the session handle across slow I/O.

use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use mt_identity::{ExecutionHostId, HostInstallId, RepoId, WorktreeId};

use crate::pool::{BoundedExecOutput, BoundedExecState, CachedSession};
use crate::sftp::{SftpBoundedFileRead, SftpHandle, SftpNodeKind};
use crate::{run_bounded_exec_on_session, SftpTransferError};

pub const REMOTE_RUNTIME_PROTOCOL_VERSION: u32 = 1;
const INSTALL_ID_MAX_BYTES: usize = 128;
const RUNTIME_OUTPUT_CAP_BYTES: usize = 16 * 1024;
const HEARTBEAT_COMMAND: &str = "printf 'mini-term-runtime-v1\\n'";
const HEARTBEAT_OUTPUT: &[u8] = b"mini-term-runtime-v1\n";
const TOOL_PROBE_COMMAND: &str = "if command -v git >/dev/null 2>&1; then printf 'git=1\\n'; else printf 'git=0\\n'; fi; if command -v gh >/dev/null 2>&1; then printf 'gh=1\\n'; else printf 'gh=0\\n'; fi; if command -v claude >/dev/null 2>&1; then printf 'claude=1\\n'; else printf 'claude=0\\n'; fi; if command -v codex >/dev/null 2>&1; then printf 'codex=1\\n'; else printf 'codex=0\\n'; fi; if command -v opencode >/dev/null 2>&1; then printf 'opencode=1\\n'; else printf 'opencode=0\\n'; fi; if command -v pi >/dev/null 2>&1; then printf 'pi=1\\n'; else printf 'pi=0\\n'; fi; if command -v grok >/dev/null 2>&1; then printf 'grok=1\\n'; else printf 'grok=0\\n'; fi; printf 'shell=1\\n'";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteRuntimeCapabilities {
    pub git: bool,
    pub gh: bool,
    pub claude: bool,
    pub codex: bool,
    pub opencode: bool,
    pub pi: bool,
    pub grok: bool,
    pub shell: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRuntimeIdentity {
    pub protocol_version: u32,
    pub host_install_id: HostInstallId,
    pub host_key_fingerprint: String,
    pub execution_host_id: ExecutionHostId,
    pub connection_epoch: u64,
    pub canonical_home: String,
    pub permissions_hardened: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRuntimeSnapshot {
    pub identity: RemoteRuntimeIdentity,
    pub canonical_worktree_path: String,
    pub canonical_git_common_dir: Option<String>,
    pub repo_id: RepoId,
    pub worktree_id: WorktreeId,
    pub capabilities: RemoteRuntimeCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteRuntimeErrorKind {
    Transport,
    State,
    Protocol,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRuntimeError {
    kind: RemoteRuntimeErrorKind,
    message: String,
    retryable: bool,
    retire_session: bool,
}

impl RemoteRuntimeError {
    fn transport(message: impl Into<String>) -> Self {
        Self {
            kind: RemoteRuntimeErrorKind::Transport,
            message: message.into(),
            retryable: true,
            retire_session: true,
        }
    }

    fn transient(message: impl Into<String>, retire_session: bool) -> Self {
        Self {
            kind: RemoteRuntimeErrorKind::Transport,
            message: message.into(),
            retryable: true,
            retire_session,
        }
    }

    fn state(message: impl Into<String>) -> Self {
        Self {
            kind: RemoteRuntimeErrorKind::State,
            message: message.into(),
            retryable: false,
            retire_session: false,
        }
    }

    fn protocol(message: impl Into<String>) -> Self {
        Self {
            kind: RemoteRuntimeErrorKind::Protocol,
            message: message.into(),
            retryable: false,
            retire_session: false,
        }
    }

    fn from_sftp(error: SftpTransferError) -> Self {
        if error.is_transport() {
            Self::transport(error.message())
        } else {
            Self::state(error.message())
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub const fn should_retry(&self) -> bool {
        self.retryable
    }

    pub const fn requires_session_retirement(&self) -> bool {
        self.retire_session
    }
}

impl fmt::Display for RemoteRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RemoteRuntimeError {}

pub async fn inspect_remote_runtime(
    session: Arc<CachedSession>,
    requested_worktree_path: &str,
    request_timeout: Duration,
) -> Result<RemoteRuntimeSnapshot, RemoteRuntimeError> {
    let sftp = tokio::time::timeout(
        request_timeout,
        SftpHandle::open_on_session(session.clone(), request_timeout),
    )
    .await
    .map_err(|_| RemoteRuntimeError::transient("remote runtime SFTP open timed out", true))?
    .map_err(RemoteRuntimeError::from_sftp)?;

    let result = inspect_with_sftp(
        session.as_ref(),
        &sftp,
        requested_worktree_path,
        request_timeout,
    )
    .await;
    sftp.close().await;
    result
}

pub async fn remote_runtime_heartbeat(
    session: &CachedSession,
    timeout: Duration,
) -> Result<(), RemoteRuntimeError> {
    let output = run_runtime_exec(session, HEARTBEAT_COMMAND, timeout).await?;
    require_success(&output, "remote runtime heartbeat")?;
    if output.stdout != HEARTBEAT_OUTPUT {
        return Err(RemoteRuntimeError::protocol(
            "remote runtime heartbeat returned an unexpected response",
        ));
    }
    Ok(())
}

async fn inspect_with_sftp(
    session: &CachedSession,
    sftp: &SftpHandle,
    requested_worktree_path: &str,
    request_timeout: Duration,
) -> Result<RemoteRuntimeSnapshot, RemoteRuntimeError> {
    let canonical_home = canonicalize_absolute(sftp, ".", "remote home").await?;
    let runtime_root = join_posix(&canonical_home, ".mini-term");
    let runtime_version_dir = join_posix(&runtime_root, "runtime-v1");
    ensure_state_directory(sftp, &runtime_root).await?;
    ensure_state_directory(sftp, &runtime_version_dir).await?;

    let install_path = join_posix(&runtime_version_dir, "install-id");
    let host_install_id = load_or_create_install_id(sftp, &install_path).await?;
    let root_hardened = sftp.set_permissions(&runtime_root, 0o700).await.is_ok();
    let version_hardened = sftp
        .set_permissions(&runtime_version_dir, 0o700)
        .await
        .is_ok();
    let install_hardened = sftp.set_permissions(&install_path, 0o600).await.is_ok();
    let permissions_hardened = root_hardened && version_hardened && install_hardened;

    let canonical_worktree_path =
        canonicalize_absolute(sftp, requested_worktree_path, "remote worktree").await?;
    if !sftp
        .is_dir(&canonical_worktree_path)
        .await
        .map_err(RemoteRuntimeError::from_sftp)?
    {
        return Err(RemoteRuntimeError::state(
            "remote worktree path is not a directory",
        ));
    }

    remote_runtime_heartbeat(session, request_timeout).await?;
    let capabilities = probe_capabilities(session, request_timeout).await?;
    let canonical_git_common_dir = if capabilities.git {
        discover_git_common_dir(session, sftp, &canonical_worktree_path, request_timeout).await?
    } else {
        None
    };

    if canonical_git_common_dir.is_none() {
        let git_marker = join_posix(&canonical_worktree_path, ".git");
        if sftp
            .try_node_kind(&git_marker)
            .await
            .map_err(RemoteRuntimeError::from_sftp)?
            .is_some()
        {
            return Err(RemoteRuntimeError::state(
                "remote path contains a Git marker but repository discovery failed",
            ));
        }
    }

    let host_key_fingerprint = session.host_key_fingerprint().to_string();
    let execution_host_id = ExecutionHostId::derive(&host_key_fingerprint, &host_install_id);
    let repo_path = canonical_git_common_dir
        .as_deref()
        .unwrap_or(&canonical_worktree_path);
    let repo_id = RepoId::derive(&execution_host_id, repo_path);
    let worktree_id = WorktreeId::derive(&repo_id, &canonical_worktree_path, None);

    Ok(RemoteRuntimeSnapshot {
        identity: RemoteRuntimeIdentity {
            protocol_version: REMOTE_RUNTIME_PROTOCOL_VERSION,
            host_install_id,
            host_key_fingerprint,
            execution_host_id,
            connection_epoch: session.connection_epoch().get(),
            canonical_home,
            permissions_hardened,
        },
        canonical_worktree_path,
        canonical_git_common_dir,
        repo_id,
        worktree_id,
        capabilities,
    })
}

async fn canonicalize_absolute(
    sftp: &SftpHandle,
    path: &str,
    label: &str,
) -> Result<String, RemoteRuntimeError> {
    let canonical = sftp
        .canonicalize(path)
        .await
        .map_err(RemoteRuntimeError::from_sftp)?;
    validate_absolute_path(&canonical, label)?;
    Ok(canonical)
}

fn validate_absolute_path(path: &str, label: &str) -> Result<(), RemoteRuntimeError> {
    if !path.starts_with('/') || path.contains('\0') || path.contains('\n') || path.contains('\r') {
        return Err(RemoteRuntimeError::protocol(format!(
            "{label} is not a canonical absolute POSIX path"
        )));
    }
    Ok(())
}

fn join_posix(parent: &str, child: &str) -> String {
    if parent == "/" {
        format!("/{child}")
    } else {
        format!("{}/{child}", parent.trim_end_matches('/'))
    }
}

async fn ensure_state_directory(sftp: &SftpHandle, path: &str) -> Result<(), RemoteRuntimeError> {
    match sftp
        .try_node_kind(path)
        .await
        .map_err(RemoteRuntimeError::from_sftp)?
    {
        Some(SftpNodeKind::Directory) => Ok(()),
        None => {
            sftp.create_dir_all(path)
                .await
                .map_err(RemoteRuntimeError::from_sftp)?;
            match sftp
                .try_node_kind(path)
                .await
                .map_err(RemoteRuntimeError::from_sftp)?
            {
                Some(SftpNodeKind::Directory) => Ok(()),
                Some(kind) => Err(RemoteRuntimeError::state(format!(
                    "remote runtime state path has unexpected type {kind:?}"
                ))),
                None => Err(RemoteRuntimeError::state(
                    "remote runtime state directory was not created",
                )),
            }
        }
        Some(kind) => Err(RemoteRuntimeError::state(format!(
            "remote runtime state path has unexpected type {kind:?}"
        ))),
    }
}

async fn load_or_create_install_id(
    sftp: &SftpHandle,
    install_path: &str,
) -> Result<HostInstallId, RemoteRuntimeError> {
    match sftp
        .try_node_kind(install_path)
        .await
        .map_err(RemoteRuntimeError::from_sftp)?
    {
        Some(SftpNodeKind::File) => read_install_id(sftp, install_path).await,
        Some(kind) => Err(RemoteRuntimeError::state(format!(
            "remote runtime install identity has unexpected type {kind:?}"
        ))),
        None => {
            let candidate = HostInstallId::new();
            let bytes = format!("{}\n", candidate.as_str());
            match sftp.write_new_file(install_path, bytes.as_bytes()).await {
                Ok(()) => {
                    let observed = read_install_id(sftp, install_path).await?;
                    if observed != candidate {
                        return Err(RemoteRuntimeError::state(
                            "remote runtime install identity changed during exclusive creation",
                        ));
                    }
                    Ok(observed)
                }
                Err(create_error) => {
                    read_install_id(sftp, install_path)
                        .await
                        .map_err(|read_error| {
                            RemoteRuntimeError::state(format!(
                        "remote runtime install identity could not be created or read: {}; {}",
                        create_error.message(),
                        read_error.message()
                    ))
                        })
                }
            }
        }
    }
}

async fn read_install_id(
    sftp: &SftpHandle,
    install_path: &str,
) -> Result<HostInstallId, RemoteRuntimeError> {
    match sftp
        .read_file_bounded(install_path, INSTALL_ID_MAX_BYTES)
        .await
        .map_err(RemoteRuntimeError::from_sftp)?
    {
        SftpBoundedFileRead::Complete(bytes) => parse_install_id_bytes(&bytes),
        SftpBoundedFileRead::TooLarge => Err(RemoteRuntimeError::state(
            "remote runtime install identity exceeds its size limit",
        )),
    }
}

fn parse_install_id_bytes(bytes: &[u8]) -> Result<HostInstallId, RemoteRuntimeError> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        RemoteRuntimeError::state("remote runtime install identity is not valid UTF-8")
    })?;
    let value = text.strip_suffix('\n').unwrap_or(text);
    if value.is_empty()
        || value != value.trim()
        || value.contains('\n')
        || value.contains('\r')
        || value.contains('\0')
    {
        return Err(RemoteRuntimeError::state(
            "remote runtime install identity is malformed",
        ));
    }
    HostInstallId::from_str(value)
        .map_err(|_| RemoteRuntimeError::state("remote runtime install identity is malformed"))
}

async fn probe_capabilities(
    session: &CachedSession,
    timeout: Duration,
) -> Result<RemoteRuntimeCapabilities, RemoteRuntimeError> {
    let output = run_runtime_exec(session, TOOL_PROBE_COMMAND, timeout).await?;
    require_success(&output, "remote runtime tool probe")?;
    parse_capabilities(&output.stdout)
}

fn parse_capabilities(bytes: &[u8]) -> Result<RemoteRuntimeCapabilities, RemoteRuntimeError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| RemoteRuntimeError::protocol("remote tool inventory is not valid UTF-8"))?;
    let mut result = RemoteRuntimeCapabilities::default();
    let mut seen = std::collections::HashSet::new();
    for line in text.lines() {
        let (name, value) = line
            .split_once('=')
            .ok_or_else(|| RemoteRuntimeError::protocol("remote tool inventory is malformed"))?;
        if !seen.insert(name) || !matches!(value, "0" | "1") {
            return Err(RemoteRuntimeError::protocol(
                "remote tool inventory is malformed",
            ));
        }
        let present = value == "1";
        match name {
            "git" => result.git = present,
            "gh" => result.gh = present,
            "claude" => result.claude = present,
            "codex" => result.codex = present,
            "opencode" => result.opencode = present,
            "pi" => result.pi = present,
            "grok" => result.grok = present,
            "shell" => result.shell = present,
            _ => {
                return Err(RemoteRuntimeError::protocol(
                    "remote tool inventory contains an unknown field",
                ));
            }
        }
    }
    const REQUIRED: [&str; 8] = [
        "git", "gh", "claude", "codex", "opencode", "pi", "grok", "shell",
    ];
    if REQUIRED.iter().any(|name| !seen.contains(name)) || !text.ends_with('\n') {
        return Err(RemoteRuntimeError::protocol(
            "remote tool inventory is incomplete",
        ));
    }
    Ok(result)
}

async fn discover_git_common_dir(
    session: &CachedSession,
    sftp: &SftpHandle,
    worktree_path: &str,
    timeout: Duration,
) -> Result<Option<String>, RemoteRuntimeError> {
    let quoted = shell_quote(worktree_path);
    let absolute_command =
        format!("git -C {quoted} rev-parse --path-format=absolute --git-common-dir");
    let absolute = run_runtime_exec(session, &absolute_command, timeout).await?;
    if command_succeeded(&absolute)? {
        let path = parse_single_path(&absolute.stdout, "Git common directory")?;
        return canonicalize_git_common_dir(sftp, worktree_path, &path)
            .await
            .map(Some);
    }

    let legacy_command = format!("cd {quoted} && git rev-parse --git-common-dir");
    let legacy = run_runtime_exec(session, &legacy_command, timeout).await?;
    if !command_succeeded(&legacy)? {
        return Ok(None);
    }
    let path = parse_single_path(&legacy.stdout, "Git common directory")?;
    canonicalize_git_common_dir(sftp, worktree_path, &path)
        .await
        .map(Some)
}

async fn canonicalize_git_common_dir(
    sftp: &SftpHandle,
    worktree_path: &str,
    path: &str,
) -> Result<String, RemoteRuntimeError> {
    let candidate = if path.starts_with('/') {
        path.to_string()
    } else {
        join_posix(worktree_path, path)
    };
    let canonical = canonicalize_absolute(sftp, &candidate, "Git common directory").await?;
    if !sftp
        .is_dir(&canonical)
        .await
        .map_err(RemoteRuntimeError::from_sftp)?
    {
        return Err(RemoteRuntimeError::state(
            "Git common directory is not a directory",
        ));
    }
    Ok(canonical)
}

fn parse_single_path(bytes: &[u8], label: &str) -> Result<String, RemoteRuntimeError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| RemoteRuntimeError::protocol(format!("{label} is not valid UTF-8")))?;
    let value = text.strip_suffix('\n').unwrap_or(text);
    if value.is_empty()
        || value != value.trim()
        || value.contains('\n')
        || value.contains('\r')
        || value.contains('\0')
    {
        return Err(RemoteRuntimeError::protocol(format!(
            "{label} response is malformed"
        )));
    }
    Ok(value.to_string())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

async fn run_runtime_exec(
    session: &CachedSession,
    command: &str,
    timeout: Duration,
) -> Result<BoundedExecOutput, RemoteRuntimeError> {
    let output = run_bounded_exec_on_session(session, command, timeout, RUNTIME_OUTPUT_CAP_BYTES)
        .await
        .map_err(RemoteRuntimeError::transport)?;
    classify_exec_output(output)
}

fn classify_exec_output(
    output: BoundedExecOutput,
) -> Result<BoundedExecOutput, RemoteRuntimeError> {
    if output.requires_session_retirement() {
        return Err(RemoteRuntimeError::transient(
            "remote runtime command left the SSH channel state uncertain",
            true,
        ));
    }
    if output.timed_out {
        return Err(RemoteRuntimeError::transient(
            "remote runtime command timed out",
            false,
        ));
    }
    if output.stdout_truncated || output.stderr_truncated {
        return Err(RemoteRuntimeError::protocol(
            "remote runtime command exceeded its output limit",
        ));
    }
    if output.state != BoundedExecState::Started {
        return Err(RemoteRuntimeError::state(
            "remote server rejected the runtime command",
        ));
    }
    Ok(output)
}

fn command_succeeded(output: &BoundedExecOutput) -> Result<bool, RemoteRuntimeError> {
    output
        .exit_code
        .map(|code| code == 0)
        .ok_or_else(|| RemoteRuntimeError::protocol("remote command returned no exit status"))
}

fn require_success(output: &BoundedExecOutput, label: &str) -> Result<(), RemoteRuntimeError> {
    if command_succeeded(output)? {
        Ok(())
    } else {
        Err(RemoteRuntimeError::state(format!("{label} failed")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install_id() -> HostInstallId {
        "install-v1:123e4567-e89b-42d3-a456-426614174000"
            .parse()
            .unwrap()
    }

    #[test]
    fn install_identity_parser_is_canonical_and_single_line() {
        let canonical = format!("{}\n", install_id());
        assert_eq!(
            parse_install_id_bytes(canonical.as_bytes()).unwrap(),
            install_id()
        );
        assert!(parse_install_id_bytes(format!(" {}", install_id()).as_bytes()).is_err());
        assert!(parse_install_id_bytes(format!("{}\nextra", install_id()).as_bytes()).is_err());
        assert!(parse_install_id_bytes(b"install-v1:not-a-uuid\n").is_err());
        assert!(parse_install_id_bytes(&[0xff]).is_err());
    }

    #[test]
    fn shell_arguments_are_single_quoted_without_interpolation() {
        assert_eq!(shell_quote("/srv/repo"), "'/srv/repo'");
        assert_eq!(shell_quote("/srv/a'b"), "'/srv/a'\\''b'");
    }

    #[test]
    fn capability_inventory_requires_every_known_field_once() {
        let inventory = b"git=1\ngh=0\nclaude=1\ncodex=1\nopencode=0\npi=0\ngrok=0\nshell=1\n";
        let parsed = parse_capabilities(inventory).unwrap();
        assert!(parsed.git && parsed.claude && parsed.codex && parsed.shell);
        assert!(!parsed.gh && !parsed.opencode && !parsed.pi && !parsed.grok);
        assert!(parse_capabilities(b"git=1\n").is_err());
        assert!(parse_capabilities(b"git=1\ngit=0\n").is_err());
        assert!(parse_capabilities(b"unknown=1\n").is_err());
    }

    #[test]
    fn path_parser_rejects_ambiguous_output() {
        assert_eq!(
            parse_single_path(b"/srv/repo/.git\n", "path").unwrap(),
            "/srv/repo/.git"
        );
        assert!(parse_single_path(b"/one\n/two\n", "path").is_err());
        assert!(parse_single_path(b" /one\n", "path").is_err());
        assert!(parse_single_path(&[0xff], "path").is_err());
    }

    #[test]
    fn remote_ids_are_stable_across_connection_epochs() {
        let host_a = ExecutionHostId::derive("SHA256:server", &install_id());
        let host_b = ExecutionHostId::derive("SHA256:server", &install_id());
        let repo_a = RepoId::derive(&host_a, "/srv/repo/.git");
        let repo_b = RepoId::derive(&host_b, "/srv/repo/.git");
        assert_eq!(host_a, host_b);
        assert_eq!(repo_a, repo_b);
        assert_eq!(
            WorktreeId::derive(&repo_a, "/srv/repo", None),
            WorktreeId::derive(&repo_b, "/srv/repo", None)
        );
        assert_ne!(
            ExecutionHostId::derive("SHA256:other", &install_id()),
            host_a
        );
    }

    #[test]
    fn uncertain_or_unbounded_exec_results_fail_closed() {
        let uncertain = BoundedExecOutput {
            state: BoundedExecState::ChannelOpenUnknown,
            timed_out: true,
            command_started: true,
            ..BoundedExecOutput::default()
        };
        let error = classify_exec_output(uncertain).unwrap_err();
        assert!(error.should_retry());
        assert!(error.requires_session_retirement());

        let truncated = BoundedExecOutput {
            state: BoundedExecState::Started,
            stdout_truncated: true,
            command_started: true,
            exit_code: Some(0),
            ..BoundedExecOutput::default()
        };
        let error = classify_exec_output(truncated).unwrap_err();
        assert!(!error.should_retry());
        assert!(!error.requires_session_retirement());
    }
}
