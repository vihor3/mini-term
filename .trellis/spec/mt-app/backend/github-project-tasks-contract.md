# GitHub Project Tasks Contract

## Scenario: Account-scoped Issues and Pull Requests on the execution host

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
pub fn known_accounts_plan(host: &str) -> Result<CommandPlan, AccountError>;
pub fn parse_known_accounts(host: &str, output: &CommandOutput)
    -> Result<KnownGitHubAccounts, AccountError>;
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

pub struct AccountHostResult<T> {
    pub result: Result<T, AccountExecutionError>,
    pub observed_connection_epoch: Option<u64>,
}
pub fn discover_accounts(snapshot: &ProjectExecutionSnapshot, host: &str,
    control: &AccountExecutionControl) -> AccountHostResult<KnownGitHubAccounts>;
pub fn execute_selected_account(snapshot: &ProjectExecutionSnapshot,
    plan: &SelectedAccountRequestPlan, control: &AccountExecutionControl)
    -> AccountHostResult<CommandOutput>;

pub fn open_github_work_item(
    service: Entity<GitHubTaskService>,
    request: OpenGitHubWorkItem,
    window: &mut Window,
    cx: &mut App,
);
```

`AccountExecutionControl::new` takes a 100ms..120s timeout, an explicit
`AccountCancellation`, and the captured optional connection epoch. Capability
probes use the same control and result envelope. Private credential bytes never
inhabit a `CommandPlan`, general command result, or configuration field.

`AppConfig::tasks_account_selections` defaults to an empty vector. Each entry
contains a normalized login and `TasksAccountScope`: root project, execution
host ID, stable Local/WSL/SSH backend identity, and normalized GitHub hostname.
Credentials, credential fingerprints, and transient SSH epochs are not persisted.

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
- Origin discovery alone uses ordinary host command execution. Every `gh`
  stage uses the dedicated account executor: capability probes, structured
  host-specific enumeration, named-account identity, and list/detail requests.
  Enumeration uses neither `--active` nor `--show-token`; allowlist parsed
  identity/state fields and keep broken peers visible. Never return raw auth
  status or credential diagnostics through a general result.
- The Tasks toolbar owns account selection. A sole successful known account
  can initialize an absent selection; multiple known accounts require explicit
  choice. A stored-but-missing, invalid, or broken choice never falls back to
  another account. Sibling worktrees share the root project's account scope,
  but other projects/backends/GitHub hosts do not. Global `gh` active-account
  changes are informational only and never rewrite Tasks state. Tasks never
  invokes `gh auth switch`, login, or a config write; Git credentials are separate.
- Native lookup captures `gh auth token --hostname <host> --user <exact-login>`
  in bounded private non-Debug buffers and passes it only to the child's auth
  environment. WSL/SSH execute lookup, identity proofs and data commands wholly
  inside a Python 3.8+ isolated stdlib envelope on that host. The token never
  returns over SSH or enters shell/client argv, a temporary script, or a file.
  Missing host Python is `HostHelperUnavailable`, not a local fallback/install.
- Remove inherited auth/debug/host/repo and shell-startup overrides; set only
  the applicable request auth variable (`GH_TOKEN` for github.com and its
  supported ghe.com subdomains, `GH_ENTERPRISE_TOKEN` for other GHES hosts).
  Preserve exact login spelling for credential lookup, normalized identity for
  comparisons. Before/after data identity proofs use the SAME captured credential.
  Public output rejects an exact credential echo, and credential errors reduce
  to static categories without raw stdout/stderr.
- The ordered pipeline revalidates origin and selected identity around data
  access, including cached list/detail requests. It passes the observed epoch
  to every subsequent account stage and preserves epochs on errors. Cancellation
  invalidates publication and stops later stages; native process groups/Windows
  Jobs and the host envelope own child cleanup. A failed cleanup acknowledgement
  is explicit, not reported as success or safe rollback. The bounded ordinary
  origin read has before/after cancellation checks, not physical cancellation.
- Foreground access is explicit, not a consequence of rendering or a service
  notification. Showing Tasks, activating its worktree/mode and opening or
  reactivating a WorkItem allocate owned accesses and revalidate origin and the
  selected account. Until that access proves current, cached content is stale
  and inert. Passive notifications must not cause an unbounded refetch loop.
- `GitHubWorkItemViewer::on_activated(&mut self, &mut Context<Self>)` starts a
  bounded detail access without replacing the tab/viewer or resetting scroll.
  The viewer retains its returned request receipt; another access or delayed
  response cannot reactivate its cache. New viewers access in the constructor.
  Tasks list row/menu callbacks additionally retain `list_access_id`; merely
  showing a known-identity list does not cancel independent detail accesses.
  Actual source/account/auth changes still invalidate both.
- Discovery/source identity includes execution host, root project/source,
  backend, exact `WorktreeId`, and exact canonical worktree path. Sibling
  worktrees therefore cannot reuse one another's unverified `origin` result.
- Only after each exact source independently discovers the same normalized
  repository and account may completed list/detail data share a
  `RepositoryCacheKey` scoped by execution host/root/backend plus repository,
  lowercase account, and auth generation. Mode, filter, selection, scroll, and
  workbench preview/permanent state remain keyed by `WorktreeId`.
  A new read can cancel/restart an older loading slot; there is no promised
  shared-fetch waiter subsystem. Independent source proof precedes cache reuse.
- Every completion validates request ID, auth generation, repository cache key,
  current source signature, re-discovered repository, re-probed account, and
  observed SSH connection epoch before publishing. The first observed SSH epoch
  may replace a captured pre-connect epoch; any later epoch change inside the
  same pipeline rejects the result. A changed host, distro, connection
  fingerprint, root project, remote, selected account, or Retry makes old work
  inert. An unrelated global active-account change is not an invalidation event.
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
| No known account on the host | Show exact host label and manual login Copy/Retry |
| Multiple known accounts and no stored choice | Show Tasks selector; no data request |
| Chosen account missing, revoked, or inaccessible | Retain choice and distinct error; no active/sole-peer fallback |
| Required JSON/named-user capability missing | Explicit unsupported capability; no alternate auth strategy |
| Selected token lacks a read scope | `ScopeRequired`; Retry available |
| Global gh active account changes | Leave Tasks choice and request ownership unchanged |
| Tasks choice changes | Invalidate only that exact account scope; never change global gh |
| WSL/SSH lacks Python 3.8+ | `HostHelperUnavailable`; native Windows remains Python-free |
| Credential lookup/token echo/cleanup fails | Static sanitized error, no token or raw diagnostics returned |
| API rate limit or network failure | Preserve same-identity last-known rows when present |
| JSON is truncated, invalid UTF-8, malformed, or has an unknown state | Reject as `MalformedResponse` |
| First SSH stage reconnects before execution | Adopt its observed epoch for the request source |
| SSH epoch changes after a stage was observed | Reject completion and do not publish rows/detail |
| User switches worktree while list/detail is running | Exact-source data may finish in its bucket; presentation is not changed for the new worktree |
| Retry occurs while an old request is running | New auth/request generations win; old completion is inert |
| Sibling worktrees expose different `origin` values | Keep discovery/results separate and derive different repository cache keys |
| Sibling worktrees independently prove the same normalized repository/account/auth generation | Share downstream list/detail cache only after those proofs |
| Same path has two different `WorktreeId` values | Keep discovery source, Tasks UI, and detail tabs separate |
| Detail body contains HTML, images, or links | Render inert visible text; perform no navigation or asset load |
| Rollback variable equals `0` | Render the prior unavailable placeholder and start no Tasks probe |

### 5. Good / Base / Bad Cases

- Good: Main and linked worktrees independently prove their source before using
  the same completed repository/account cache, while returning to different
  modes, filters, selected rows, scroll positions and detail previews.
- Good: The first SSH command reconnects and establishes a newer epoch, then all
  later stages stay on it. If a later stage reports another epoch, the result is
  rejected even when project ID and path text are unchanged.
- Good: Project A chooses Alice and project B chooses Bob on one device. Both
  issue requests keep their choice while the device's global active user changes.
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
- Account tests assert official host-map framing, mixed valid/broken accounts,
  duplicate/schema/count/output bounds, exact lookup spelling, independent
  selection/default persistence, and both directions of no auth synchronization.
- Production executor fixtures use a synthetic gh binary and sentinel token:
  assert private native pipes, Windows suspended-child/Job ownership, inherited
  environment removal, same-credential proofs, bounded cancellation/timeout,
  descendant retirement and no secret in argv/results/config/diagnostics.
- Linux authenticated loopback SSH uses the shared runner-only fixture, isolated
  HOME/keys and a proven synthetic gh path. Explicit ignored tests inspect actual
  channel stdout/stderr and epoch replacement. Ordinary workspace tests do not
  execute this gate. POSIX envelope tests are not actual WSL transport evidence.
- Error tests distinguish client missing, auth, wrong account/host, scope, rate,
  offline, not found, malformed, and generic failure, including WSL/SSH
  command-not-found diagnostics.
- Execution-host fixtures run both list and detail pipelines for Local, WSL,
  and SSH snapshots; every discovery/auth/data/context stage must plan against
  that selected backend. Separate tests assert structured argv, hostile SSH
  quoting, NUL rejection, and no fallback.
- Cache/generation tests assert exact worktree/path discovery signatures;
  downstream sharing only after normalized repository/account proof; distinct
  root, distro, connection fingerprint, and epoch scopes; and rejection of
  changed remote, account, auth generation, request ID, or mid-pipeline SSH
  epoch.
- Workbench tests assert same-item tabs differ by `WorktreeId`, one preview per
  worktree is replaceable, double-click promotes, and close is isolated.
- Markdown tests reparse sanitized GitHub bodies and assert no disallowed HTML,
  image, definition, or link replacement remains.
- Run focused tests, workspace check, Clippy, and Windows MSVC checks only in GitHub Actions.

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
Selected data then uses `execute_selected_account`, never the ordinary command
runner or `gh auth switch`. Source-backed contracts and authored tests do not
establish a passing Actions/native gate; record those separately for the exact SHA.
