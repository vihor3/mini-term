# Remote Runtime Contract

## Scenario: Authenticated SSH execution-host identity and bounded inventory

### 1. Scope / Trigger

Use this contract when remote project code needs verified execution-host,
repository, worktree, or tool-capability facts. `mt-ssh` owns transport truth;
callers must not infer authority from an SSH configuration ID, display name, or
remote path alone.

### 2. Signatures

```rust
pub struct ConnectionEpoch(u64);

pub struct RemoteRuntimeIdentity {
    pub protocol_version: u32,
    pub host_install_id: HostInstallId,
    pub host_key_fingerprint: String,
    pub execution_host_id: ExecutionHostId,
    pub connection_epoch: u64,
    pub canonical_home: String,
    pub permissions_hardened: bool,
}

pub struct RemoteRuntimeSnapshot {
    pub identity: RemoteRuntimeIdentity,
    pub canonical_worktree_path: String,
    pub canonical_git_common_dir: Option<String>,
    pub repo_id: RepoId,
    pub worktree_id: WorktreeId,
    pub capabilities: RemoteRuntimeCapabilities,
}

pub async fn inspect_remote_runtime(
    session: Arc<CachedSession>,
    requested_worktree_path: &str,
    request_timeout: Duration,
) -> Result<RemoteRuntimeSnapshot, RemoteRuntimeError>;

pub async fn remote_runtime_heartbeat(
    session: &CachedSession,
    timeout: Duration,
) -> Result<(), RemoteRuntimeError>;

impl SshPool {
    pub async fn is_current_session(
        &self,
        id: &str,
        expected: &Arc<CachedSession>,
    ) -> bool;
}
```

Remote state path:

```text
<canonical-home>/.mini-term/runtime-v1/install-id
```

### 3. Contracts

- `MtClient::check_server_key` records the canonical SHA-256 server-key
  fingerprint only after existing known-host policy accepts the key.
  `CachedSession` exposes it only after user authentication succeeds.
- Every newly authenticated pooled session receives one immutable,
  process-monotonic nonzero `ConnectionEpoch`. Healthy pool reuse keeps the
  epoch; reconnecting allocates a higher one. Gaps are allowed.
- The runtime install ID is a canonical `HostInstallId`, created with SFTP
  exclusive-create semantics. A create race re-reads the winner. Malformed,
  oversized, symlinked, directory, or special state is rejected and never
  replaced automatically.
- `ExecutionHostId` derives only from verified host-key fingerprint plus remote
  install ID. `RepoId` derives from canonical Git common directory when Git is
  valid, otherwise from the canonical remote directory. `WorktreeId` derives
  from repository plus canonical worktree path.
- Bootstrap/path checks use SFTP while heartbeat and inventory use independent
  bounded exec channels on the same authenticated pooled session. The session
  lock covers channel creation, not channel lifetime.
- Every command has a timeout, stdout/stderr cap, exit-status requirement, and
  strict UTF-8/schema parsing. User path arguments use POSIX single quoting.
- A completed snapshot is accepted only while its exact `Arc<CachedSession>`
  remains the cached pool winner. A transport failure may retire only the exact
  session that failed. The app may retry once. Business-state and protocol errors do not
  trigger reconnect loops.
- Runtime identity contains no password, private-key material, environment,
  prompt, or unbounded command output. The install ID is an identity salt, not
  a bearer credential.
- SFTP v3 lacks descriptor-relative `openat`/`O_NOFOLLOW`; leaf state is checked
  with `lstat` semantics and post-operation validation, but same-account path
  replacement remains a protocol limitation.

### 4. Validation & Error Matrix

| Condition | Required behavior |
|-----------|-------------------|
| Known-host key matches | Record one SHA-256 fingerprint and continue authentication |
| Known-host key mismatches | Reject before bootstrap, heartbeat, or inventory |
| Authentication fails | Do not expose a `CachedSession` or runtime identity |
| Connection epoch counter overflows | Fail closed; never reuse zero or wrap |
| Install ID is absent | Exclusive-create one canonical ID, then re-read it |
| Concurrent creator wins | Accept only the valid re-read winner |
| Install state is malformed, oversized, symlinked, or non-regular | Return non-retryable state error and preserve it |
| Remote worktree is not a canonical directory | Return non-retryable state/protocol error |
| `.git` marker exists but Git discovery fails | Fail closed instead of claiming non-Git identity |
| Bounded exec starts but times out | Retry at most once; retire only when channel state is uncertain |
| Output is truncated, non-UTF-8, duplicate, or schema-incomplete | Return non-retryable protocol error |
| Exact cached transport fails | Evict only that session instance before one retry |
| Snapshot session was replaced before return | Reject the snapshot; never publish its older epoch |

### 5. Good / Base / Bad Cases

- Good: Two reconnects to the same verified host/install pair have different
  epochs but the same execution-host, repository, and worktree IDs.
- Good: SFTP bootstrap and a heartbeat open independent channels while another
  remote operation is active.
- Base: A canonical remote non-Git directory returns an authoritative directory
  identity with bounded tool capabilities.
- Bad: Derive `ExecutionHostId` from connection ID or host text; configuration
  edits and host-key changes would silently reuse the wrong workbench.
- Bad: Overwrite a malformed install ID to make startup succeed; that destroys
  the only evidence that host identity changed or state was corrupted.

### 6. Tests Required

- Accepted key fingerprints are canonical SHA-256 and one-assignment; a second
  different key is refused.
- Epoch allocation is nonzero, monotonic, and fails on overflow.
- Install-ID parsing accepts one canonical line and rejects whitespace,
  additional lines, invalid UTF-8, and invalid IDs.
- Capability parsing requires every known field exactly once and bounded output.
- Git path parsing rejects ambiguous output; Git and non-Git derivation remains
  stable across epochs.
- Timeout/truncation tests assert retry and retirement classification.
- App facade tests assert only attempt zero may retry a retryable error.
- Pool winner tests assert a replacement `Arc` makes the prior snapshot stale.

### 7. Wrong vs Correct

#### Wrong

```rust
let host = ExecutionHostId::derive(&connection.id, local_install_id);
let epoch = connection.id.clone();
```

Configuration identity is not authenticated transport identity and cannot fence
reconnects.

#### Correct

```rust
let host = ExecutionHostId::derive(
    session.host_key_fingerprint(),
    &remote_host_install_id,
);
let epoch = session.connection_epoch();
```

The accepted server key and remote installation establish stable host identity;
the process-monotonic session epoch fences reconnect-specific work.
