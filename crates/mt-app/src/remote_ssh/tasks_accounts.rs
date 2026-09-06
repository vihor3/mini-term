//! Tasks credential envelope transport. Only nonsecret source/argv cross SSH.

use std::time::Duration;

use mt_github::AccountError;
use mt_ssh::russh::ChannelMsg;
use tokio::io::AsyncWriteExt;

use crate::execution_host::{ExecutionBackend, ProjectExecutionSnapshot};
use crate::tasks_account_executor::{
    AccountExecutionControl, AccountExecutionError, AccountHostResult,
};

use super::{acquire_session, evict_session_if_same, state};

const CLOSE_GRACE: Duration = Duration::from_millis(500);
const CANCEL_GRACE: Duration = Duration::from_secs(2);

pub(crate) fn run_tasks_account_envelope(
    snapshot: &ProjectExecutionSnapshot,
    command: &str,
    control: &AccountExecutionControl,
    wire_cap: usize,
) -> AccountHostResult<Vec<u8>> {
    let ExecutionBackend::Ssh {
        connection,
        connection_fingerprint,
        ..
    } = &snapshot.backend
    else {
        return AccountHostResult {
            result: Err(AccountExecutionError::InvalidContext),
            observed_connection_epoch: None,
        };
    };
    let st = state();
    let runtime = match st.runtime() {
        Ok(runtime) => runtime,
        Err(_) => {
            return AccountHostResult {
                result: Err(AccountError::Offline.into()),
                observed_connection_epoch: None,
            };
        }
    };
    runtime.block_on(async {
        let deadline = tokio::time::Instant::now() + control.timeout();
        let pool = st.pool();
        let session = tokio::select! {
            biased;
            _ = control.cancellation().cancelled() => Err(AccountExecutionError::Cancelled),
            result = tokio::time::timeout_at(deadline, acquire_session(st, &pool, connection)) => {
                match result {
                    Ok(Ok(session)) => Ok(session),
                    Ok(Err(_)) => Err(AccountError::Offline.into()),
                    Err(_) => Err(AccountExecutionError::TimedOut),
                }
            }
        };
        let session = match session {
            Ok(session) => session,
            Err(error) => return AccountHostResult { result: Err(error), observed_connection_epoch: None },
        };
        let epoch = session.connection_epoch().get();
        let result = async {
            if super::connection_fingerprint(connection) != *connection_fingerprint
                || control.expected_connection_epoch().is_some_and(|expected| expected != epoch)
                || !pool.is_current_session(&connection.id, &session).await {
                return Err(AccountExecutionError::ContextChanged);
            }
            let opened = tokio::select! {
                biased;
                _ = control.cancellation().cancelled() => Err(AccountExecutionError::Cancelled),
                opened = tokio::time::timeout_at(deadline, async {
                    let handle = session.lock().await;
                    handle.channel_open_session().await
                }) => match opened {
                    Ok(Ok(channel)) => Ok(channel),
                    Ok(Err(_)) => Err(AccountError::Offline.into()),
                    Err(_) => Err(AccountExecutionError::TimedOut),
                }
            };
            let mut channel = match opened {
                Ok(channel) => channel,
                Err(error) => {
                    // Channel-open cancellation can leave an unknown server channel.
                    evict_session_if_same(st, &pool, &connection.id, &session).await;
                    return Err(error);
                }
            };
            let enqueued = tokio::select! {
                biased;
                _ = control.cancellation().cancelled() => Err(AccountExecutionError::Cancelled),
                result = tokio::time::timeout_at(deadline, channel.exec(true, command)) => match result {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(_)) => Err(AccountError::Offline.into()),
                    Err(_) => Err(AccountExecutionError::TimedOut),
                }
            };
            if let Err(error) = enqueued {
                let _ = tokio::time::timeout(CLOSE_GRACE, channel.close()).await;
                evict_session_if_same(st, &pool, &connection.id, &session).await;
                return Err(error);
            }
            let mut writer = channel.make_writer();
            let received = {
                let receive = async {
                    let mut bytes = Vec::with_capacity(wire_cap.min(16 * 1024));
                    let mut stderr_len = 0usize;
                    #[cfg(test)]
                    let mut stderr = Vec::new();
                    while let Some(message) = channel.wait().await {
                        match message {
                            ChannelMsg::Data { data } => {
                                if data.len() > wire_cap.saturating_sub(bytes.len()) {
                                    return Err(AccountExecutionError::Protocol);
                                }
                                bytes.extend_from_slice(&data);
                            }
                            ChannelMsg::ExtendedData { data, .. } => {
                                stderr_len = stderr_len.saturating_add(data.len());
                                if stderr_len > mt_github::COMMAND_OUTPUT_LIMIT {
                                    return Err(AccountExecutionError::Protocol);
                                }
                                #[cfg(test)]
                                stderr.extend_from_slice(&data);
                            }
                            ChannelMsg::Failure => return Err(AccountError::CommandFailed.into()),
                            ChannelMsg::Eof | ChannelMsg::Close => break,
                            _ => {}
                        }
                    }
                    #[cfg(test)]
                    {
                        // These assertions inspect actual loopback channel bytes,
                        // including shell tracing, not the envelope source string.
                        assert!(!String::from_utf8_lossy(&bytes).contains("fixture_credential_"));
                        assert!(!String::from_utf8_lossy(&stderr).contains("fixture_credential_"));
                    }
                    Ok(bytes)
                };
                tokio::pin!(receive);
                enum Progress {
                    Complete(Result<Vec<u8>, AccountExecutionError>),
                    Stop(AccountExecutionError),
                }
                let progress = tokio::select! {
                    biased;
                    _ = control.cancellation().cancelled() => Progress::Stop(AccountExecutionError::Cancelled),
                    _ = tokio::time::sleep_until(deadline) => Progress::Stop(AccountExecutionError::TimedOut),
                    output = &mut receive => Progress::Complete(output),
                };
                match progress {
                    Progress::Complete(Ok(bytes)) => Ok(bytes),
                    Progress::Complete(Err(_)) => {
                        let _ = tokio::time::timeout(CLOSE_GRACE, writer.write_all(b"x")).await;
                        Err(AccountExecutionError::CleanupFailed)
                    }
                    Progress::Stop(error) => {
                        // Do not drop an executing credential envelope without notifying
                        // its owner. This pipe is separate from the gh child's null stdin.
                        let sent = matches!(
                            tokio::time::timeout(CLOSE_GRACE, writer.write_all(b"x")).await,
                            Ok(Ok(()))
                        );
                        if sent {
                            match tokio::time::timeout(CANCEL_GRACE, &mut receive).await {
                                Ok(Ok(bytes)) if crate::tasks_account_executor::host_reply_confirms_cleanup(&bytes) => Err(error),
                                _ => Err(AccountExecutionError::CleanupFailed),
                            }
                        } else {
                            Err(AccountExecutionError::CleanupFailed)
                        }
                    }
                }
            };
            // A complete helper reply plus EOF already acknowledges child cleanup.
            // russh can close its sender at EOF, making a reciprocal close fail.
            let _ = tokio::time::timeout(CLOSE_GRACE, channel.close()).await;
            match received {
                Ok(bytes) => finish_success(st, &pool, connection, &session, bytes, control, deadline).await,
                Err(error) => {
                    evict_session_if_same(st, &pool, &connection.id, &session).await;
                    Err(error)
                }
            }
        }.await;
        AccountHostResult { result, observed_connection_epoch: Some(epoch) }
    })
}

async fn finish_success(
    st: &super::RemoteSshState,
    pool: &mt_ssh::SshPool,
    connection: &mt_config::SshConnection,
    session: &std::sync::Arc<mt_ssh::CachedSession>,
    bytes: Vec<u8>,
    control: &AccountExecutionControl,
    deadline: tokio::time::Instant,
) -> Result<Vec<u8>, AccountExecutionError> {
    let epoch = session.connection_epoch().get();
    if control.cancellation().is_cancelled() {
        return Err(AccountExecutionError::Cancelled);
    }
    if tokio::time::Instant::now() >= deadline {
        return Err(AccountExecutionError::TimedOut);
    }
    if !pool.is_current_session(&connection.id, session).await
        || !st.connection_epoch_is_current(&connection.id, epoch)
    {
        return Err(AccountExecutionError::ContextChanged);
    }
    if !crate::tasks_account_executor::host_reply_confirms_cleanup(&bytes) {
        evict_session_if_same(st, pool, &connection.id, session).await;
        return Err(AccountExecutionError::CleanupFailed);
    }
    session.touch();
    Ok(bytes)
}

#[cfg(all(test, unix))]
#[test]
#[ignore = "requires the Actions disposable loopback sshd fixture and isolated HOME"]
fn tasks_account_executor_ssh_sentinels_cleanup_and_epoch_pipeline() {
    let (connection, root) = super::git_ops::loopback_ssh_fixture().unwrap();
    crate::tasks_account_executor::tests::exercise_ssh_fixture(&connection, &root, || {
        let st = state();
        st.runtime().unwrap().block_on(async {
            let pool = st.pool();
            let session = acquire_session(st, &pool, &connection).await.unwrap();
            assert!(evict_session_if_same(st, &pool, &connection.id, &session).await);
        });
    });
}
