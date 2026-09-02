# Worktree Identity Contract

## Scenario: Resolve stable local and provisional worktree identity

### 1. Scope / Trigger

Use this contract whenever a configured project is converted into execution
host, repository, and worktree identity. Canonicalization belongs to the host
that owns the path. WSL/SSH configuration resolution is provisional and must
remain replaceable by an authenticated remote runtime binding.

### 2. Signatures

```rust
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

pub struct ResolvedWorktreeIdentity {
    pub execution_host_id: ExecutionHostId,
    pub repo_id: RepoId,
    pub worktree_id: WorktreeId,
    pub canonical_worktree_path: String,
    pub canonical_git_common_dir: Option<String>,
    pub source: WorktreeIdentitySource,
}

pub fn local_execution_host_id(install: &HostInstallId) -> ExecutionHostId;
pub fn resolve_local(
    install: &HostInstallId,
    worktree_path: &Path,
) -> Result<ResolvedWorktreeIdentity>;
pub fn resolve_provisional_local(
    install: &HostInstallId,
    host_visible_path: &str,
) -> Result<ResolvedWorktreeIdentity>;
pub fn resolve_provisional_wsl(
    install: &HostInstallId,
    distro: &str,
    host_visible_path: &str,
) -> Result<ResolvedWorktreeIdentity>;
pub fn resolve_provisional_ssh(
    install: &HostInstallId,
    stable_connection_id: &str,
    remote_path: &str,
) -> Result<ResolvedWorktreeIdentity>;
```

### 3. Contracts

- `ExecutionHostId` is host-qualified by `HostInstallId`; project names,
  branch names, process IDs, and display labels never participate.
- Local Git `RepoId` derives from execution host plus canonical Git common
  directory. Main and linked worktrees therefore share a repository identity.
- Local Git `WorktreeId` derives from `RepoId` plus canonical worktree path.
  Different linked worktrees in one repository must differ.
- Local non-Git directories derive repository and worktree identity from the
  canonical directory in the local host context.
- Provisional WSL identity uses normalized distro plus normalized absolute
  POSIX path. A UNC path naming a different distro is rejected.
- Provisional SSH identity uses the stable configured connection ID plus a
  normalized absolute remote POSIX path. Connection display names are not an
  identity source.
- `AuthoritativeRemoteGit` and `AuthoritativeRemoteDirectory` are installed only
  from an authenticated runtime snapshot. They use the verified execution host,
  canonical remote path, and canonical Git common directory when present.
- A changed remote host key or remote install ID produces a different
  `ExecutionHostId`; configuration IDs and labels never override that result.
- Resolution functions are pure for provisional inputs and perform no network
  calls. They do not claim remote host-key or runtime authority.
- AppStore may reuse a persisted binding as `PersistedFallback` when local,
  WSL, or SSH facts are temporarily unavailable. That fallback must be
  transactionally replaceable when stronger facts arrive.

### 4. Validation & Error Matrix

| Condition | Required behavior |
|-----------|-------------------|
| Local path cannot be canonicalized or is not a directory | Return an error; AppStore may reuse a valid persisted binding |
| `.git` exists but repository open fails | Return an error, not a non-Git identity |
| Local directory has no `.git` marker | Resolve as `LocalDirectory` |
| WSL distro is empty or contains NUL | Reject input |
| WSL UNC distro differs from requested distro | Reject input |
| WSL/SSH path is not absolute POSIX | Reject input |
| SSH connection ID is empty or contains NUL | Reject input |
| Authenticated remote Git common directory is present | Resolve as `AuthoritativeRemoteGit` using that directory for `RepoId` |
| Authenticated remote folder has no Git repository | Resolve as `AuthoritativeRemoteDirectory` using the canonical folder path |
| Same install/common-dir/worktree path is resolved twice | Return identical derived identities |
| Same repository has two linked worktree paths | Return same `RepoId`, different `WorktreeId` |

### 5. Good / Base / Bad Cases

- Good: `/repo` and `/repo-feature` share the canonical common Git directory,
  so they share `RepoId` but retain independent workbench IDs.
- Good: A temporarily unavailable SSH path keeps its persisted provisional
  binding until a future remote runtime supplies authenticated facts.
- Base: A canonical non-Git directory remains stable across application
  restarts on the same installation.
- Bad: Hash `project_id + branch_name`; recreating or renaming configuration
  would lose the existing workbench.
- Bad: Treat an SSH connection label as verified remote host identity.
- Bad: On Git open failure, silently downgrade a directory containing `.git`
  to non-Git identity.

### 6. Tests Required

- Golden identity derivation verifies deterministic host/repository/worktree
  values across clean starts.
- Real-Git fixtures cover main, linked, detached, locked, and prunable rows;
  main and linked paths assert shared common-dir identity.
- Non-Git directory tests assert stable host-qualified identity.
- Provisional local/WSL/SSH tests assert normalization and source markers.
- Authoritative remote source serde tests freeze camel-case storage names and
  assert both remote variants report `is_authoritative()`.
- Invalid absolute-path, NUL, empty ID, and mismatched WSL distro cases fail.
- Persisted fallback tests cover local and SSH resolution failure without
  identity churn.

### 7. Wrong vs Correct

#### Wrong

```rust
let worktree_id = hash(project.id, project.name, current_branch);
```

Compatibility presentation fields change independently of the underlying
worktree and cannot provide stable routing.

#### Correct

```rust
let host = ExecutionHostId::derive("local", install_id);
let repo = RepoId::derive(&host, canonical_git_common_dir);
let worktree = WorktreeId::derive(&repo, canonical_worktree_path, None);
```

Canonical host-owned filesystem facts define repository/worktree identity;
configuration records merely bind to that stable result.
