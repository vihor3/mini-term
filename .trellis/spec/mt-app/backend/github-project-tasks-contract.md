# GitHub Project Tasks Contract

## Scenario: Read-only Issues and Pull Requests on the project execution host

### 1. Scope / Trigger

Use this contract when the Tasks context panel discovers a GitHub repository,
checks GitHub CLI authentication, lists Issues or Pull Requests, or opens an
internal work-item detail tab. It applies to local, WSL, and SSH projects,
linked worktrees, connection replacement, Retry, and every delayed command
completion.

### 2. Signatures

Domain plans and parsing stay transport-free in `mt-github`:

```rust
pub struct GitHubRepoIdentity { /* host + owner + repo */ }
pub fn parse_remote_url(input: &str) -> Result<GitHubRepoIdentity, RemoteParseError>;

pub struct CommandPlan {
    pub program: String,
    pub args: Vec<String>,
}

pub fn discover_remote_plan() -> CommandPlan;
pub fn version_plan() -> CommandPlan;
pub fn auth_status_plan(host: &str) -> CommandPlan;
pub fn account_plan(host: &str) -> CommandPlan;
pub fn list_plan(repo: &GitHubRepoIdentity, kind: WorkItemKind) -> CommandPlan;
pub fn detail_plan(
    repo: &GitHubRepoIdentity,
    kind: WorkItemKind,
    number: u64,
) -> CommandPlan;
```

Application routing snapshots and dispatch:

```rust
pub struct ProjectExecutionSnapshot {
    pub project_id: String,
    pub root_project_id: String,
    pub worktree_id: WorktreeId,
    pub execution_host_id: ExecutionHostId,
    pub canonical_path: String,
    pub root_source_path: String,
    pub backend: ExecutionBackend,
    pub host_label: String,
}

pub fn execute_host_command(
    snapshot: &ProjectExecutionSnapshot,
    plan: &CommandPlan,
    timeout: Duration,
    output_cap: usize,
) -> Result<HostCommandResult, CommandExecutionError>;

pub fn open_github_work_item(
    service: Entity<GitHubTaskService>,
    request: OpenGitHubWorkItem,
    window: &mut Window,
    cx: &mut App,
);
```

Rollback environment:

```text
MINI_TERM_GITHUB_PROJECT_TASKS=0
```

### 3. Contracts

- Git and `gh` always execute on the active project's execution host. Native
  projects use a local process with `current_dir`; WSL uses `wsl.exe` with an
  explicit distro, `--cd`, and `--exec`; SSH uses the existing authenticated
  pooled session. WSL or SSH failure never falls back to local commands or
  credentials.
- Local and WSL preserve program and argv as separate values. SSH serialization
  is allowed only through the tested POSIX single-quote encoder, then wrapped as
  `cd <quoted-worktree> && exec <quoted-argv>`.
- Repository identity comes only from `git remote get-url origin` on that host.
  Project names, display paths, and client-side same-spelling folders are not
  repository evidence.
- The ordered pipeline is bounded: discover remote, probe `gh`, check auth for
  the normalized host, probe the active account as JSON, then list or view with
  explicit JSON fields. Before publishing, re-discover `origin` and re-probe the
  account. Cached list and detail requests perform the same context probe both
  before and after the data command. No command uses `--web`, prints a token, or
  runs login.
- Cache identity is execution-host source signature + root project + normalized
  repository + account + auth generation. Sibling worktrees share list/detail
  data and in-flight work, while mode, filter, selection, scroll, and workbench
  preview/permanent state remain keyed by `WorktreeId`.
- Every completion validates request ID, auth generation, repository cache key,
  current source signature, re-discovered repository, re-probed account, and
  observed SSH connection epoch before publishing. The first observed SSH epoch
  may replace a captured pre-connect epoch; any later epoch change inside the
  same pipeline rejects the result. A changed host, distro, connection
  fingerprint, root project, remote, account, or Retry makes old work inert.
- Offline, rate-limit, and generic transient failures may retain last-known rows
  only for the same complete cache identity. Auth, repository, account, malformed
  response, and not-found errors do not borrow another identity's data. Rows from
  a loading or invalidated source are visible only as stale context and are not
  clickable until that exact repository/account cache is Ready again.
- Auth-required UI shows the exact execution-host label and the inert text
  `gh auth login --hostname <normalized-host>`, with Copy and Retry only. It
  never opens a browser, creates or focuses a terminal, invokes login, or stores
  credentials.
- Work-item title, body, author, labels, URL, stdout, stderr, and JSON text are
  untrusted. Structured fields render as text. Detail Markdown reaches
  `TextView::markdown` only after raw HTML, images, reference definitions, and
  every link target are converted to inert visible text.
- A row click opens one worktree-scoped read-only preview in the unified tab
  strip. A second row replaces only the clean work-item preview for that
  worktree. Double-clicking the preview tab promotes it; closing or promoting a
  tab never changes another worktree. No work-item surface invokes its URL.
- Only the exact environment value `0` restores the old Tasks placeholder.
  Files, Git, Sessions, worktree identity, and cached runtime state remain
  independent.

### 4. Validation & Error Matrix

| Condition | Required behavior |
|-----------|-------------------|
| Local `gh` executable missing | `ClientMissing`; no fallback |
| WSL/SSH returns an explicit shell command-not-found diagnostic for any `gh` stage | `ClientMissing`; no local probe |
| Origin is absent, local, malformed, or unsupported | `NoGitHubRemote` |
| Host has no active `gh` authentication | Show exact host label and manual login command |
| Active token lacks a read scope | `ScopeRequired`; Retry available |
| API rate limit or network failure | Preserve same-identity last-known rows when present |
| JSON is truncated, invalid UTF-8, malformed, or has an unknown state | Reject as `MalformedResponse` |
| First SSH stage reconnects before execution | Adopt its observed epoch for the request source |
| SSH epoch changes after a stage was observed | Reject completion and do not publish rows/detail |
| User switches worktree while list/detail is running | Shared data may finish in its exact bucket; presentation is not changed for the new worktree |
| Retry occurs while an old request is running | New auth/request generations win; old completion is inert |
| Same path has two different `WorktreeId` values | Keep separate Tasks UI and detail tabs |
| Detail body contains HTML, images, or links | Render inert visible text; perform no navigation or asset load |
| Rollback variable equals `0` | Render the prior unavailable placeholder and start no Tasks probe |

### 5. Good / Base / Bad Cases

- Good: Main and linked worktrees reuse one Issue fetch, but return to different
  Issue/PR modes, filters, selected rows, scroll positions, and detail previews.
- Good: The first SSH command reconnects and establishes a newer epoch, then all
  later stages stay on it. If a later stage reports another epoch, the result is
  rejected even when project ID and path text are unchanged.
- Base: A local repository with authenticated `gh` displays an empty Issue list
  without affecting the terminal, Files, Git, or Sessions.
- Bad: Run local `gh` after WSL or SSH execution fails. That leaks the client's
  account and can show data for the wrong host.
- Bad: Key presentation by project path or repository name. Same-path worktrees
  then overwrite each other's selection and preview.
- Bad: Put `summary.url` into an opener or let GitHub Markdown links reach the
  rich renderer as active actions.

### 6. Tests Required

- Remote parser coverage for HTTPS, `ssh://`, scp syntax, GHES, `.git`, malformed,
  local, control-character, port, and shell-hostile inputs.
- Structured plan tests assert explicit repository/host, JSON fields, no token,
  no `--web`, and exact manual-login text.
- Error tests distinguish client missing, auth, wrong account/host, scope, rate,
  offline, not found, malformed, and generic failure, including WSL/SSH
  command-not-found diagnostics.
- Execution-host fixtures run both list and detail pipelines for Local, WSL,
  and SSH snapshots; every discovery/auth/data/context stage must plan against
  that selected backend. Separate tests assert structured argv, hostile SSH
  quoting, NUL rejection, and no fallback.
- Cache/generation tests assert sibling source sharing; distinct root, distro,
  connection fingerprint, and epoch signatures; and rejection of changed remote,
  account, auth generation, request ID, or mid-pipeline SSH epoch.
- Workbench tests assert same-item tabs differ by `WorktreeId`, one preview per
  worktree is replaceable, double-click promotes, and close is isolated.
- Markdown tests reparse sanitized GitHub bodies and assert no disallowed HTML,
  image, definition, or link replacement remains.
- Run focused tests, workspace check, Clippy, and Windows MSVC check in Docker.

### 7. Wrong vs Correct

#### Wrong

```rust
let rows = std::process::Command::new("gh")
    .args(["issue", "list", "--repo", project.name.as_str()])
    .output()?;
```

This uses the client machine and a presentation name, so remote projects can
silently read the wrong account or repository.

#### Correct

```rust
let snapshot = store.project_execution_snapshot(project_id)?;
let remote = execute_host_command(
    &snapshot,
    &discover_remote_plan(),
    DISCOVERY_TIMEOUT,
    COMMAND_OUTPUT_LIMIT,
)?;
let repository = parse_remote_url(std::str::from_utf8(&remote.output.stdout)?)?;
```

The execution host discovers its own normalized repository identity, and every
later completion is fenced by that immutable source and repository context.
