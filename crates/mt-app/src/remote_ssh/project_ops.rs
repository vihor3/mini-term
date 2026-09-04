use std::sync::Arc;
use std::time::Duration;

use mt_config::SshConnection;
use mt_github::CommandPlan;
use mt_ssh::{BoundedExecOutput, CachedSession, SftpHandle, SftpNodeKind};

use crate::execution_host::serialize_posix_argv;

use super::paths::{expand_tilde, join_posix, normalize_absolute_posix, valid_remote_name};
use super::{
    RemoteSshState, acquire_session, connection_fingerprint, evict_session_if_same,
    open_sftp_with_session, remote_home, state,
};

const GIT_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const GIT_MUTATION_TIMEOUT: Duration = Duration::from_secs(180);
const GIT_OUTPUT_CAP: usize = 64 * 1024;
const GIT_ERROR_DETAIL_LIMIT: usize = 4 * 1024;

#[derive(Clone, Debug)]
pub struct RemoteProjectContext {
    pub connection: SshConnection,
    pub connection_fingerprint: u64,
    pub expected_connection_epoch: Option<u64>,
}

impl RemoteProjectContext {
    pub fn new(
        connection: SshConnection,
        connection_fingerprint: u64,
        expected_connection_epoch: Option<u64>,
    ) -> Self {
        Self {
            connection,
            connection_fingerprint,
            expected_connection_epoch,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if connection_fingerprint(&self.connection) != self.connection_fingerprint {
            return Err("SSH connection configuration changed before dispatch".into());
        }
        if self.expected_connection_epoch.is_none() {
            return Err("SSH onboarding operation has no authenticated connection epoch".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteProbeProvenance {
    OperationEpoch,
    PostconditionVerifiedAfterUncertainDispatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteGitRelationship {
    NotGit,
    RepositoryRoot {
        top_level: String,
        common_dir: String,
    },
    NestedInRepository {
        top_level: String,
        common_dir: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemotePathProbe {
    pub canonical_path: String,
    pub directory_empty: Option<bool>,
    pub git: RemoteGitRelationship,
    pub connection_epoch: u64,
    pub connection_fingerprint: u64,
    pub provenance: RemoteProbeProvenance,
}

/// A failed recovery probe owns an epoch only when that exact session is still current.
#[derive(Clone, Debug)]
pub struct RemoteRecoveryProbeError {
    pub message: String,
    pub connection_epoch: Option<u64>,
    pub connection_fingerprint: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteTargetState {
    Absent(String),
    EmptyDirectory(String),
    NonEmptyDirectory(String),
    Other(String),
}

#[derive(Clone, Debug)]
pub struct RemoteTargetProbe {
    pub state: RemoteTargetState,
    pub connection_epoch: u64,
    pub connection_fingerprint: u64,
}

#[derive(Clone, Debug)]
pub struct RemoteMutationOutcome {
    pub output: Option<BoundedExecOutput>,
    pub transport_error: Option<String>,
    pub authority_error: Option<String>,
    pub connection_epoch: u64,
    pub connection_fingerprint: u64,
}

struct RemoteProbeRequest<'a> {
    path: &'a str,
    include_empty: bool,
    inspect_git: bool,
    provenance: RemoteProbeProvenance,
}

pub fn probe_existing_directory(
    context: &RemoteProjectContext,
    path: &str,
    include_empty: bool,
    inspect_git: bool,
) -> Result<RemotePathProbe, String> {
    probe_existing_directory_with_provenance(
        context,
        path,
        include_empty,
        inspect_git,
        RemoteProbeProvenance::OperationEpoch,
    )
}

pub fn probe_existing_directory_after_uncertain_dispatch(
    context: &RemoteProjectContext,
    path: &str,
    include_empty: bool,
    inspect_git: bool,
) -> Result<RemotePathProbe, RemoteRecoveryProbeError> {
    context
        .validate()
        .map_err(|message| RemoteRecoveryProbeError {
            message,
            connection_epoch: None,
            connection_fingerprint: context.connection_fingerprint,
        })?;
    let st = state();
    let result = st.block_on(async {
        let recovery = async {
            let (session, sftp) = open_sftp_with_session(st, &context.connection)
                .await
                .map_err(|message| RemoteRecoveryProbeError {
                    message,
                    connection_epoch: None,
                    connection_fingerprint: context.connection_fingerprint,
                })?;
            let result = probe_on_session(
                st,
                context,
                &session,
                &sftp,
                RemoteProbeRequest {
                    path,
                    include_empty,
                    inspect_git,
                    provenance:
                        RemoteProbeProvenance::PostconditionVerifiedAfterUncertainDispatch,
                },
            )
            .await;
            let failure_epoch = if result.is_err()
                && ensure_current(st, &context.connection, &session)
                    .await
                    .is_ok()
            {
                Some(session.connection_epoch().get())
            } else {
                None
            };
            sftp.close().await;
            result.map_err(|message| RemoteRecoveryProbeError {
                message,
                connection_epoch: failure_epoch,
                connection_fingerprint: context.connection_fingerprint,
            })
        }
        .await;
        Ok(recovery)
    });
    match result {
        Ok(recovery) => recovery,
        Err(message) => Err(RemoteRecoveryProbeError {
            message,
            connection_epoch: None,
            connection_fingerprint: context.connection_fingerprint,
        }),
    }
}

fn probe_existing_directory_with_provenance(
    context: &RemoteProjectContext,
    path: &str,
    include_empty: bool,
    inspect_git: bool,
    provenance: RemoteProbeProvenance,
) -> Result<RemotePathProbe, String> {
    context.validate()?;
    let st = state();
    st.block_on(async {
        let (session, sftp) = open_sftp_with_session(st, &context.connection).await?;
        let result = probe_on_session(
            st,
            context,
            &session,
            &sftp,
            RemoteProbeRequest {
                path,
                include_empty,
                inspect_git,
                provenance,
            },
        )
        .await;
        sftp.close().await;
        result
    })
}

pub fn probe_target(
    context: &RemoteProjectContext,
    canonical_parent: &str,
    name: &str,
) -> Result<RemoteTargetProbe, String> {
    context.validate()?;
    if !valid_remote_name(name) {
        return Err("remote project name must be one safe POSIX basename".into());
    }
    let st = state();
    st.block_on(async {
        let (session, sftp) = open_sftp_with_session(st, &context.connection).await?;
        let result = async {
            ensure_operation_session(st, context, &session).await?;
            let parent = canonical_directory(&sftp, canonical_parent).await?;
            ensure_operation_session(st, context, &session).await?;
            let target = join_posix(&parent, name);
            let state = match sftp
                .try_node_kind(&target)
                .await
                .map_err(|error| error.message().to_string())?
            {
                None => RemoteTargetState::Absent(target),
                Some(SftpNodeKind::Directory) => {
                    ensure_operation_session(st, context, &session).await?;
                    let canonical = canonical_directory(&sftp, &target).await?;
                    ensure_operation_session(st, context, &session).await?;
                    if sftp
                        .read_dir(&canonical)
                        .await
                        .map_err(|error| error.message().to_string())?
                        .is_empty()
                    {
                        RemoteTargetState::EmptyDirectory(canonical)
                    } else {
                        RemoteTargetState::NonEmptyDirectory(canonical)
                    }
                }
                Some(_) => RemoteTargetState::Other(target),
            };
            ensure_operation_session(st, context, &session).await?;
            Ok(RemoteTargetProbe {
                state,
                connection_epoch: session.connection_epoch().get(),
                connection_fingerprint: context.connection_fingerprint,
            })
        }
        .await;
        sftp.close().await;
        result
    })
}

pub fn create_directory_exclusive(
    context: &RemoteProjectContext,
    canonical_target: &str,
) -> Result<u64, String> {
    context.validate()?;
    let st = state();
    st.block_on(async {
        let (session, sftp) = open_sftp_with_session(st, &context.connection).await?;
        let result = async {
            ensure_operation_session(st, context, &session).await?;
            if sftp
                .try_node_kind(canonical_target)
                .await
                .map_err(|error| error.message().to_string())?
                .is_some()
            {
                return Err(format!(
                    "remote project target already exists: {canonical_target}"
                ));
            }
            ensure_operation_session(st, context, &session).await?;
            sftp.create_dir(canonical_target)
                .await
                .map_err(|error| error.message().to_string())?;
            ensure_operation_session(st, context, &session).await?;
            Ok(session.connection_epoch().get())
        }
        .await;
        sftp.close().await;
        result
    })
}

pub fn remove_empty_directory(
    context: &RemoteProjectContext,
    canonical_target: &str,
) -> Result<u64, String> {
    context.validate()?;
    let st = state();
    st.block_on(async {
        let (session, sftp) = open_sftp_with_session(st, &context.connection).await?;
        let result = async {
            ensure_operation_session(st, context, &session).await?;
            if sftp
                .node_kind(canonical_target)
                .await
                .map_err(|error| error.message().to_string())?
                != SftpNodeKind::Directory
            {
                return Err(format!(
                    "remote cleanup target is not a directory: {canonical_target}"
                ));
            }
            ensure_operation_session(st, context, &session).await?;
            if !sftp
                .read_dir(canonical_target)
                .await
                .map_err(|error| error.message().to_string())?
                .is_empty()
            {
                return Err(format!(
                    "remote cleanup target is not empty: {canonical_target}"
                ));
            }
            ensure_operation_session(st, context, &session).await?;
            sftp.remove_dir(canonical_target)
                .await
                .map_err(|error| error.message().to_string())?;
            ensure_operation_session(st, context, &session).await?;
            Ok(session.connection_epoch().get())
        }
        .await;
        sftp.close().await;
        result
    })
}

pub fn run_git(
    context: &RemoteProjectContext,
    cwd: &str,
    plan: &CommandPlan,
) -> Result<RemoteMutationOutcome, String> {
    context.validate()?;
    let command = serialize_posix_argv(
        std::iter::once(plan.program.as_str()).chain(plan.args.iter().map(String::as_str)),
    )
    .map_err(|error| error.message)?;
    let quoted_cwd = serialize_posix_argv([cwd]).map_err(|error| error.message)?;
    let command = format!("cd {quoted_cwd} && exec {command}");
    let st = state();
    st.block_on(async {
        let pool = st.pool();
        let session = acquire_session(st, &pool, &context.connection).await?;
        ensure_operation_session(st, context, &session).await?;
        let output = match mt_ssh::run_bounded_exec_on_session(
            session.as_ref(),
            &command,
            GIT_MUTATION_TIMEOUT,
            GIT_OUTPUT_CAP,
        )
        .await
        {
            Ok(output) => output,
            Err(error) => {
                evict_session_if_same(st, &pool, &context.connection.id, &session).await;
                return Ok(RemoteMutationOutcome {
                    output: None,
                    transport_error: Some(error),
                    authority_error: None,
                    connection_epoch: session.connection_epoch().get(),
                    connection_fingerprint: context.connection_fingerprint,
                });
            }
        };
        let authority_error = if output.requires_session_retirement() {
            evict_session_if_same(st, &pool, &context.connection.id, &session).await;
            None
        } else {
            ensure_operation_session(st, context, &session).await.err()
        };
        Ok(RemoteMutationOutcome {
            output: Some(output),
            transport_error: None,
            authority_error,
            connection_epoch: session.connection_epoch().get(),
            connection_fingerprint: context.connection_fingerprint,
        })
    })
}

async fn probe_on_session(
    st: &RemoteSshState,
    context: &RemoteProjectContext,
    session: &Arc<CachedSession>,
    sftp: &SftpHandle,
    request: RemoteProbeRequest<'_>,
) -> Result<RemotePathProbe, String> {
    let RemoteProbeRequest {
        path,
        include_empty,
        inspect_git,
        provenance,
    } = request;
    ensure_probe_session(st, context, session, provenance).await?;
    let requested = if path.trim().is_empty() || path.trim() == "~" || path.trim().starts_with("~/")
    {
        ensure_probe_session(st, context, session, provenance).await?;
        let home = remote_home(st, sftp, &context.connection.id).await?;
        expand_tilde(path.trim(), &home)
    } else {
        path.to_string()
    };
    ensure_probe_session(st, context, session, provenance).await?;
    let canonical = canonical_directory(sftp, &requested).await?;
    let directory_empty = if include_empty {
        ensure_probe_session(st, context, session, provenance).await?;
        Some(
            sftp.read_dir(&canonical)
                .await
                .map_err(|error| error.message().to_string())?
                .is_empty(),
        )
    } else {
        None
    };
    let git = if inspect_git {
        probe_git_relationship(st, context, session, sftp, &canonical, provenance).await?
    } else {
        RemoteGitRelationship::NotGit
    };
    ensure_probe_session(st, context, session, provenance).await?;
    Ok(RemotePathProbe {
        canonical_path: canonical,
        directory_empty,
        git,
        connection_epoch: session.connection_epoch().get(),
        connection_fingerprint: context.connection_fingerprint,
        provenance,
    })
}

async fn probe_git_relationship(
    st: &RemoteSshState,
    context: &RemoteProjectContext,
    session: &Arc<CachedSession>,
    sftp: &SftpHandle,
    canonical: &str,
    provenance: RemoteProbeProvenance,
) -> Result<RemoteGitRelationship, String> {
    ensure_probe_session(st, context, session, provenance).await?;
    let marker = sftp
        .try_node_kind(&join_posix(canonical, ".git"))
        .await
        .map_err(|error| error.message().to_string())?
        .is_some();
    let modern = remote_git_probe_command(canonical, true)?;
    ensure_probe_session(st, context, session, provenance).await?;
    let mut output = mt_ssh::run_bounded_exec_on_session(
        session.as_ref(),
        &modern,
        GIT_PROBE_TIMEOUT,
        GIT_OUTPUT_CAP,
    )
    .await
    .map_err(|error| error.to_string())?;
    let legacy = output.state == mt_ssh::BoundedExecState::Started
        && !output.requires_session_retirement()
        && !output.timed_out
        && !output.stdout_truncated
        && !output.stderr_truncated
        && output.exit_code == Some(129)
        && String::from_utf8_lossy(&output.stderr).contains("path-format");
    if legacy {
        ensure_probe_session(st, context, session, provenance).await?;
        output = mt_ssh::run_bounded_exec_on_session(
            session.as_ref(),
            &remote_git_probe_command(canonical, false)?,
            GIT_PROBE_TIMEOUT,
            GIT_OUTPUT_CAP,
        )
        .await
        .map_err(|error| error.to_string())?;
    }
    if output.requires_session_retirement() {
        evict_session_if_same(st, &st.pool(), &context.connection.id, session).await;
        return Err("SSH repository probe left the exact session uncertain".into());
    }
    if output.state != mt_ssh::BoundedExecState::Started {
        return Err("SSH server did not confirm the Git repository probe".into());
    }
    if output.timed_out || output.stdout_truncated || output.stderr_truncated {
        return Err("SSH repository probe returned incomplete output".into());
    }
    if output.exit_code == Some(127) {
        return Err("Git is unavailable on the selected SSH host".into());
    }
    if output.exit_code != Some(0) {
        return classify_git_probe_failure(output.exit_code, &output.stderr, marker, canonical);
    }
    let (top, common) = parse_git_paths(&output.stdout)?;
    ensure_probe_session(st, context, session, provenance).await?;
    let top = canonical_directory(sftp, top).await?;
    ensure_probe_session(st, context, session, provenance).await?;
    let common = if common.starts_with('/') {
        sftp.canonicalize(common)
            .await
            .map_err(|error| error.message().to_string())?
    } else {
        sftp.canonicalize(&join_posix(canonical, common))
            .await
            .map_err(|error| error.message().to_string())?
    };
    ensure_probe_session(st, context, session, provenance).await?;
    Ok(if top == canonical {
        RemoteGitRelationship::RepositoryRoot {
            top_level: top,
            common_dir: common,
        }
    } else {
        RemoteGitRelationship::NestedInRepository {
            top_level: top,
            common_dir: common,
        }
    })
}

async fn canonical_directory(sftp: &SftpHandle, path: &str) -> Result<String, String> {
    let normalized = normalize_absolute_posix(path)?;
    let canonical = sftp
        .canonicalize(&normalized)
        .await
        .map_err(|error| error.message().to_string())?;
    if sftp
        .node_kind(&canonical)
        .await
        .map_err(|error| error.message().to_string())?
        != SftpNodeKind::Directory
    {
        return Err(format!("remote path is not a directory: {canonical}"));
    }
    Ok(canonical)
}

async fn ensure_current(
    st: &RemoteSshState,
    connection: &SshConnection,
    session: &Arc<CachedSession>,
) -> Result<(), String> {
    if !st.pool().is_current_session(&connection.id, session).await
        || !st.connection_epoch_is_current(&connection.id, session.connection_epoch().get())
    {
        return Err("SSH operation result was superseded by a newer authenticated session".into());
    }
    Ok(())
}

async fn ensure_operation_session(
    st: &RemoteSshState,
    context: &RemoteProjectContext,
    session: &Arc<CachedSession>,
) -> Result<(), String> {
    let expected_epoch = context.expected_connection_epoch.ok_or_else(|| {
        "SSH onboarding operation has no authenticated connection epoch".to_string()
    })?;
    let actual_epoch = session.connection_epoch().get();
    if !operation_epoch_matches(Some(expected_epoch), actual_epoch) {
        return Err(format!(
            "SSH connection epoch changed before onboarding work (expected {expected_epoch}, got {actual_epoch})"
        ));
    }
    ensure_current(st, &context.connection, session).await
}

async fn ensure_probe_session(
    st: &RemoteSshState,
    context: &RemoteProjectContext,
    session: &Arc<CachedSession>,
    provenance: RemoteProbeProvenance,
) -> Result<(), String> {
    match provenance {
        RemoteProbeProvenance::OperationEpoch => {
            ensure_operation_session(st, context, session).await
        }
        RemoteProbeProvenance::PostconditionVerifiedAfterUncertainDispatch => {
            ensure_current(st, &context.connection, session).await
        }
    }
}

fn operation_epoch_matches(expected_epoch: Option<u64>, actual_epoch: u64) -> bool {
    expected_epoch == Some(actual_epoch)
}

fn remote_git_probe_command(path: &str, modern: bool) -> Result<String, String> {
    let mut argv = vec!["git", "-C", path, "rev-parse"];
    if modern {
        argv.push("--path-format=absolute");
    }
    argv.extend(["--show-toplevel", "--git-common-dir"]);
    serialize_posix_argv(argv).map_err(|error| error.message)
}

fn parse_git_paths(stdout: &[u8]) -> Result<(&str, &str), String> {
    let stdout = std::str::from_utf8(stdout)
        .map_err(|_| "SSH Git probe returned non-UTF-8 paths".to_string())?;
    let lines: Vec<&str> = stdout.lines().collect();
    if lines.len() != 2
        || lines
            .iter()
            .any(|line| line.is_empty() || line.contains('\0'))
    {
        return Err("SSH Git probe returned an ambiguous path set".into());
    }
    Ok((lines[0], lines[1]))
}

fn classify_git_probe_failure(
    exit_code: Option<u32>,
    stderr: &[u8],
    marker_present: bool,
    canonical_path: &str,
) -> Result<RemoteGitRelationship, String> {
    let exit_code = exit_code.and_then(|code| i32::try_from(code).ok());
    if !marker_present
        && crate::project_onboarding::ops::is_proven_not_git_repository(exit_code, stderr)
    {
        return Ok(RemoteGitRelationship::NotGit);
    }
    let detail =
        crate::project_onboarding::ops::bounded_lossy_diagnostic(stderr, GIT_ERROR_DETAIL_LIMIT);
    let detail = if detail.is_empty() {
        String::new()
    } else {
        format!("; {detail}")
    };
    Err(format!("Git could not inspect {canonical_path}{detail}"))
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
            RemoteGitRelationship::NotGit
        );

        let error = classify_git_probe_failure(
            Some(128),
            b"fatal: detected dubious ownership in repository at '/repo'",
            false,
            "/repo",
        )
        .unwrap_err();
        assert!(error.contains("dubious ownership"));
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

        assert!(error.contains("Git could not inspect /repo"));
    }

    #[test]
    fn normal_operation_epoch_pin_requires_the_original_exact_epoch() {
        assert!(operation_epoch_matches(Some(7), 7));
        assert!(!operation_epoch_matches(Some(7), 8));
        assert!(!operation_epoch_matches(None, 7));
    }
}
