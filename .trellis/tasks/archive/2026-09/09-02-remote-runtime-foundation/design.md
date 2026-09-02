# Technical Design

## Boundary

```text
verified SSH transport
  -> pooled CachedSession(host-key fingerprint, connection epoch)
     -> independent SFTP / exec channels
        -> remote install identity + heartbeat + worktree inventory
           -> generation-fenced AppStore reconciliation
```

`mt-ssh` owns transport truth. `mt-project` continues to own identity source
semantics, `mt-layout` owns transactional binding/layout migration, and
`mt-app` owns request scheduling and presentation. This child does not parse or
project provider Agent events.

## Authenticated Session Identity

`MtClient::check_server_key` computes the canonical SHA-256 public-key
fingerprint only after the existing known-host policy accepts the key. The
handler writes it into a one-assignment slot shared with `build_session`.
Authentication must succeed before `CachedSession` is returned.

Each built session receives a monotonically increasing `ConnectionEpoch` from
the pool. Concurrent candidates may consume unused epoch numbers, but the
cached winner has one immutable epoch. The session exposes:

```rust
pub fn host_key_fingerprint(&self) -> &str;
pub fn connection_epoch(&self) -> u64;
```

Neither value contains credentials. Pool reuse compares the full existing
connection identity, so a changed endpoint/user/credential set cannot inherit
the prior epoch or runtime snapshot.

## Remote Bootstrap

The runtime bootstrap uses SFTP on the authenticated session:

1. canonicalize the login home;
2. create `<home>/.mini-term/runtime-v1`;
3. inspect `install-id` without following a symlink;
4. if absent, exclusive-create a locally generated canonical `HostInstallId`;
5. if create races, re-read and accept only a valid winner;
6. attempt owner-only permissions and report whether hardening succeeded.

The install ID is an identity salt, not a bearer secret. Invalid, oversized,
non-regular, or ambiguous state is never replaced automatically.

`ExecutionHostId::derive(verified_host_key_fingerprint, host_install_id)` is
the only authoritative remote host derivation.

## Runtime Protocol

`mt-ssh::runtime` defines protocol v1 data types:

```rust
RemoteRuntimeIdentity {
  protocol_version,
  host_install_id,
  host_key_fingerprint,
  execution_host_id,
  connection_epoch,
  canonical_home,
  permissions_hardened,
}

RemoteRuntimeSnapshot {
  identity,
  canonical_worktree_path,
  canonical_git_common_dir,
  repo_id,
  worktree_id,
  capabilities,
}
```

The initial implementation uses the authenticated SSH connection itself as the
multiplexer. Bootstrap and path canonicalization use SFTP; heartbeat and Git or
tool probes use separate bounded exec channels. It deliberately does not merge
with the local `mt-ssh-cli` daemon or terminal-host process.

Remote commands are fixed plans with POSIX single-quoted arguments. Output caps,
timeouts, exit status, UTF-8 validation, and one-value parsing are mandatory.
Git common-dir discovery tries the absolute path form first and a bounded legacy
fallback second. A directory with no usable Git repository remains an
authoritative remote directory whose repo ID is derived from its canonical
path.

## App Reconciliation

`RemoteRuntimeProjectState` is keyed by compatibility project ID and records:

```text
request generation | connection fingerprint | connectivity | snapshot/error
```

A probe captures project ID, remote path, connection ID, and the process-local
connection-configuration fingerprint before yielding. Completion applies only
when every captured value and generation still match.

An authoritative result becomes a `ProjectWorktreeBinding` with source
`authoritativeRemoteGit` or `authoritativeRemoteDirectory`. The existing
`LayoutStore::reconcile_worktree_layouts` transaction copies the old worktree
layout only when the destination has no newer row and never deletes the source.

A changed binding is installed only while that project's PTY set is empty. This
ensures terminal routes and environment attestations are never relabeled after
spawn. Startup and project activation probe before hydration; failure falls
back to the current provisional SSH behavior and remains retryable.

## Heartbeat and Reconnect

Heartbeat runs a fixed bounded command on the exact cached session. A failed
transport retires only that `Arc<CachedSession>`; reacquisition creates a higher
epoch. Runtime state tracks connectivity separately from future Agent activity.

The next child may schedule periodic heartbeats and consume connection epochs
for Agent replay fencing. This child provides the tested primitive and performs
an initial identity/inventory probe on remote project activation.

## Rollback

`MINI_TERM_REMOTE_RUNTIME=0` skips remote runtime probing and leaves existing
provisional bindings, SSH PTY launch, SFTP, and session scanning unchanged.
Persisted authoritative IDs remain readable and are not deleted when the gate
is disabled.
