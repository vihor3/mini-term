//! Git panel transport. Every operation is pinned before remote inspection;
//! only explicit read readiness may acquire a fresh authenticated epoch.

use std::sync::Arc;
use std::time::Duration;

use mt_config::SshConnection;
use mt_project::git::cli::{self, GitCommand};
use mt_ssh::{CachedSession, SftpBoundedFileRead, SftpHandle, SftpNodeKind};

use crate::execution_host::serialize_posix_argv;

use super::project_ops::{RemoteMutationOutcome, RemoteProjectContext, ensure_operation_session};
use super::{acquire_session, connection_fingerprint, evict_session_if_same, state};

const FILE_TIMEOUT: Duration = Duration::from_secs(30);

pub fn git_connection_epoch(connection: &SshConnection, fingerprint: u64) -> Result<u64, String> {
    if connection_fingerprint(connection) != fingerprint {
        return Err("Git source connection configuration changed".into());
    }
    let st = state();
    st.block_on(async {
        let session = acquire_session(st, &st.pool(), connection).await?;
        let epoch = session.connection_epoch().get();
        let context = RemoteProjectContext::new(connection.clone(), fingerprint, Some(epoch));
        ensure_operation_session(st, &context, &session).await?;
        Ok(epoch)
    })
}

/// Unlike the Tasks facade, retain the original exec state and cleanup facts.
/// Redirection closes the program's stdin without overriding Git credentials,
/// hooks, signing or user configuration.
pub fn run_git_panel_command(
    context: &RemoteProjectContext,
    cwd: &str,
    plan: &GitCommand,
) -> Result<RemoteMutationOutcome, String> {
    context.validate()?;
    let cwd = serialize_posix_argv([cwd]).map_err(|error| error.message)?;
    let argv = serialize_posix_argv(
        std::iter::once(plan.program).chain(plan.args.iter().map(String::as_str)),
    ).map_err(|error| error.message)?;
    let command = format!("cd {cwd} && exec {argv} </dev/null");
    let st = state();
    st.block_on(async {
        let pool = st.pool();
        let session = acquire_session(st, &pool, &context.connection).await?;
        ensure_operation_session(st, context, &session).await?;
        let output = mt_ssh::run_bounded_exec_on_session(
            &session, &command, plan.timeout, plan.stdout_limit.max(plan.stderr_limit),
        ).await;
        let mut outcome = RemoteMutationOutcome {
            output: None,
            transport_error: None,
            authority_error: None,
            connection_epoch: session.connection_epoch().get(),
            connection_fingerprint: context.connection_fingerprint,
        };
        match output {
            Ok(mut output) => {
                if output.stdout.len() > plan.stdout_limit {
                    output.stdout.truncate(plan.stdout_limit);
                    output.stdout_truncated = true;
                }
                if output.stderr.len() > plan.stderr_limit {
                    output.stderr.truncate(plan.stderr_limit);
                    output.stderr_truncated = true;
                }
                if output.requires_session_retirement() || output.timed_out {
                    evict_session_if_same(st, &pool, &context.connection.id, &session).await;
                } else {
                    outcome.authority_error = ensure_operation_session(st, context, &session).await.err();
                }
                outcome.output = Some(output);
            }
            Err(error) => {
                outcome.transport_error = Some(error);
                evict_session_if_same(st, &pool, &context.connection.id, &session).await;
            }
        }
        Ok(outcome)
    })
}

async fn open_pinned(context: &RemoteProjectContext) -> Result<(Arc<CachedSession>, SftpHandle), String> {
    context.validate()?;
    let st = state();
    let session = acquire_session(st, &st.pool(), &context.connection).await?;
    ensure_operation_session(st, context, &session).await?;
    // No open/reconnect retry may move an operation to another session.
    let sftp = match tokio::time::timeout(FILE_TIMEOUT, SftpHandle::open_on_session(session.clone(), FILE_TIMEOUT)).await {
        Ok(Ok(sftp)) => sftp,
        Ok(Err(error)) => return Err(error.message().to_string()),
        Err(_) => {
            evict_session_if_same(st, &st.pool(), &context.connection.id, &session).await;
            return Err("Git SFTP channel readiness timed out".into());
        }
    };
    if let Err(error) = ensure_operation_session(st, context, &session).await {
        close_bounded(sftp).await;
        return Err(error);
    }
    Ok((session, sftp))
}

async fn close_bounded(sftp: SftpHandle) {
    let _ = tokio::time::timeout(Duration::from_secs(2), sftp.close()).await;
}

pub fn git_canonical_directory(context: &RemoteProjectContext, path: &str) -> Result<String, String> {
    state().block_on(async {
        let (session, sftp) = open_pinned(context).await?;
        let result = tokio::time::timeout(FILE_TIMEOUT, async {
            let canonical = sftp.canonicalize(path).await.map_err(|error| error.message().to_string())?;
            if !sftp.is_dir(&canonical).await.map_err(|error| error.message().to_string())? {
                return Err("Git source path is not a directory".into());
            }
            ensure_operation_session(state(), context, &session).await?;
            Ok(canonical)
        }).await.map_err(|_| "Git directory probe timed out".to_string());
        close_bounded(sftp).await;
        result?
    })
}

pub fn git_path_kind(context: &RemoteProjectContext, path: &str) -> Result<Option<SftpNodeKind>, String> {
    state().block_on(async {
        let (session, sftp) = open_pinned(context).await?;
        let result = tokio::time::timeout(FILE_TIMEOUT, sftp.try_node_kind(path)).await
            .map_err(|_| "Git path probe timed out".to_string())
            .and_then(|result| result.map_err(|error| error.message().to_string()));
        let authority = ensure_operation_session(state(), context, &session).await;
        close_bounded(sftp).await;
        authority.and(result)
    })
}

pub fn git_directory_empty(context: &RemoteProjectContext, path: &str) -> Result<bool, String> {
    state().block_on(async {
        let (session, sftp) = open_pinned(context).await?;
        let result = tokio::time::timeout(FILE_TIMEOUT, sftp.read_dir(path)).await
            .map_err(|_| "Git directory probe timed out".to_string())
            .and_then(|result| result.map_err(|error| error.message().to_string()))
            .and_then(|entries| {
                if entries.len() > super::dirs::BROWSER_ENTRY_LIMIT {
                    Err("Git directory probe exceeded its entry limit".into())
                } else {
                    Ok(entries.is_empty())
                }
            });
        let authority = ensure_operation_session(state(), context, &session).await;
        close_bounded(sftp).await;
        authority.and(result)
    })
}

/// Verify each parent separately without the editor basename restrictions.
/// An absent ancestor proves the leaf absent; errors never prove absence.
async fn checked_leaf(
    sftp: &SftpHandle,
    root: &str,
    relative: &str,
) -> Result<(String, Option<SftpNodeKind>), String> {
    cli::validate_repo_path(relative).map_err(|error| error.to_string())?;
    let canonical = sftp.canonicalize(root).await.map_err(|error| error.message().to_string())?;
    if canonical != root || !sftp.is_dir(root).await.map_err(|error| error.message().to_string())? {
        return Err("Git working directory authority changed".into());
    }
    let target = super::join_posix(root, relative);
    let mut parent = root.to_string();
    let components: Vec<_> = relative.split('/').collect();
    for component in &components[..components.len() - 1] {
        parent = super::join_posix(&parent, component);
        match sftp.try_node_kind(&parent).await.map_err(|error| error.message().to_string())? {
            None => return Ok((target, None)),
            Some(SftpNodeKind::Directory) => {}
            Some(_) => return Err("Git file parent is not a real directory".into()),
        }
        if sftp.canonicalize(&parent).await.map_err(|error| error.message().to_string())? != parent {
            return Err("Git file parent changed or escaped the repository".into());
        }
    }
    let kind = sftp.try_node_kind(&target).await.map_err(|error| error.message().to_string())?;
    Ok((target, kind))
}

pub fn git_file_kind(
    context: &RemoteProjectContext, root: &str, relative: &str,
) -> Result<Option<SftpNodeKind>, String> {
    state().block_on(async {
        let (session, sftp) = open_pinned(context).await?;
        let result = tokio::time::timeout(FILE_TIMEOUT, checked_leaf(&sftp, root, relative)).await
            .map_err(|_| "Git file containment probe timed out".to_string())
            .and_then(|result| result);
        let authority = ensure_operation_session(state(), context, &session).await;
        close_bounded(sftp).await;
        authority.and(result.map(|(_, kind)| kind))
    })
}

pub fn git_read_regular_file(
    context: &RemoteProjectContext, root: &str, relative: &str,
) -> Result<Option<SftpBoundedFileRead>, String> {
    state().block_on(async {
        let (session, sftp) = open_pinned(context).await?;
        let result = tokio::time::timeout(FILE_TIMEOUT, async {
            let (target, kind) = checked_leaf(&sftp, root, relative).await?;
            match kind {
                None => return Ok(None),
                Some(SftpNodeKind::File) => {}
                Some(_) => return Err("Git working file is not a regular file".into()),
            }
            let bytes = sftp.read_file_bounded(&target, cli::MAX_BLOB_BYTES).await
                .map_err(|error| error.message().to_string())?;
            let (after, kind) = checked_leaf(&sftp, root, relative).await?;
            if target != after || kind != Some(SftpNodeKind::File) {
                return Err("Git working file changed during read".into());
            }
            ensure_operation_session(state(), context, &session).await?;
            Ok(Some(bytes))
        }).await.map_err(|_| "Git working file read timed out".to_string());
        close_bounded(sftp).await;
        result?
    })
}

#[derive(Debug)]
pub struct RemoteGitFileRemoval {
    pub dispatched: bool,
    pub error: Option<String>,
}

/// The caller owns the source write lease and rechecks its byte/index baseline
/// immediately before calling. SFTP remove_file never recursively removes a
/// replacement directory and does not follow a leaf symlink.
pub fn git_remove_file(
    context: &RemoteProjectContext, root: &str, relative: &str,
) -> RemoteGitFileRemoval {
    let mut dispatched = false;
    let result = state().block_on(async {
        let (session, sftp) = open_pinned(context).await?;
        let result = tokio::time::timeout(FILE_TIMEOUT, async {
            let (target, kind) = checked_leaf(&sftp, root, relative).await?;
            if !matches!(kind, Some(SftpNodeKind::File | SftpNodeKind::Symlink)) {
                return Err("Git discard target disappeared or is no longer a file".into());
            }
            ensure_operation_session(state(), context, &session).await?;
            dispatched = true;
            sftp.remove_file(&target).await.map_err(|error| error.message().to_string())?;
            let (_, kind) = checked_leaf(&sftp, root, relative).await?;
            if kind.is_some() {
                return Err("Git discard destination changed after removal".into());
            }
            ensure_operation_session(state(), context, &session).await
        }).await.map_err(|_| "Git discard outcome is uncertain after timeout".to_string());
        close_bounded(sftp).await;
        result?
    });
    RemoteGitFileRemoval { dispatched, error: result.err() }
}

/// Shared read-only test configuration; the Actions job owns sshd, keys and
/// isolated HOME. Never accepts an arbitrary endpoint or runs setup commands.
#[cfg(all(test, unix))]
pub(crate) fn loopback_ssh_fixture() -> Result<(SshConnection, std::path::PathBuf), String> {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
        return Err("Loopback SSH fixtures may only run in GitHub Actions".into());
    }
    let variable = |name| std::env::var(name).map_err(|_| format!("Missing loopback fixture variable: {name}"));
    let root = std::fs::canonicalize(variable("MT_TEST_SSH_ROOT")?).map_err(|_| "Loopback fixture root is unavailable")?;
    let runner = std::fs::canonicalize(variable("RUNNER_TEMP")?).map_err(|_| "Actions temporary directory is unavailable")?;
    let home = std::fs::canonicalize(variable("HOME")?).map_err(|_| "Loopback client home is unavailable")?;
    if !root.starts_with(&runner) || root == runner || home != root.join("client-home")
        || std::fs::read(root.join(".fixture-only")).map_err(|_| "Loopback fixture marker is missing")? != b"mini-term Actions loopback\n" {
        return Err("Loopback fixture must own an isolated Actions directory and client HOME".into());
    }
    let key = std::fs::canonicalize(PathBuf::from(variable("MT_TEST_SSH_KEY")?)).map_err(|_| "Loopback fixture key is missing")?;
    if !key.starts_with(&root) { return Err("Loopback key must belong to the fixture".into()); }
    let port = variable("MT_TEST_SSH_PORT")?.parse::<u16>().map_err(|_| "Invalid loopback fixture port")?;
    if port < 1024 { return Err("Loopback fixture requires an unprivileged port".into()); }
    let user = variable("MT_TEST_SSH_USER")?;
    if user.is_empty() || !user.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte)) { return Err("Invalid loopback fixture user".into()); }
    let id = format!("actions-loopback-{}-{}", std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed));
    Ok((SshConnection {
        id, name: "Actions loopback".into(), host: "127.0.0.1".into(), port, user,
        password: None, identity_file: Some(key.to_str().ok_or("Loopback key path is not UTF-8")?.to_owned()), group: None,
    }, root))
}
