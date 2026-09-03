# GitHub Project Tasks

## Goal

Replace the Tasks placeholder with a read-only GitHub Issues and Pull Requests
surface that always executes Git and GitHub CLI commands on the active project's
own execution host. Main and linked worktrees share project/repository network
data while retaining independent Tasks presentation and workbench detail tabs.

## Background

- The Orca shell and top `Files / Git / Tasks / Sessions` route already exist.
- Stable project/worktree bindings provide `ExecutionHostId` and canonical paths.
- SSH projects already use an authenticated pooled session with bounded exec;
  WSL projects launch through `wsl.exe`; local projects use native processes.
- The user explicitly chose manual GitHub CLI authentication. mini-term must not
  execute login, create a terminal for login, or open a browser.

## Requirements

- Add a domain layer for normalized `GitHubRepoIdentity(host, owner, repo)`,
  structured command plans, Issue/PR list and detail models, JSON parsing, and
  stable error classification.
- Discover the Git remote on the project execution host. Never infer repository
  identity from project names, display paths, or a client-side directory with
  the same spelling.
- Execute native projects with local `git` and `gh`, WSL projects inside the
  selected distro, and SSH projects through the existing authenticated bounded
  exec channel. A failed WSL/SSH command must never fall back to local tools or
  credentials.
- Use structured argv at the Local and WSL boundaries. SSH may serialize argv
  only through one tested POSIX quoting function; remote/project/user text is
  data and must not become shell syntax.
- Probe CLI availability and target-host authentication before fetching data.
  Distinguish at least: no GitHub remote, client missing, auth required, wrong
  host/account, scope required, rate limited, offline/disconnected, not found,
  malformed response, and ready.
- For auth-required state, show the exact execution host label and
  `gh auth login --hostname <host>` with Copy and Retry. Do not run the command,
  open a browser, create/focus a terminal, or persist credentials.
- Fetch Issues and Pull Requests as structured JSON and render them inside
  mini-term. Treat title, body, author, labels, URL, remote text, stderr, and JSON
  strings as untrusted content; do not render raw HTML or execute links.
- Cache project data by execution host, root project identity, normalized GitHub
  repository, and auth generation. Main and linked worktrees of one project
  share list/detail data and in-flight work.
- Preserve filter, selected row, list scroll, and active Issue/PR mode per
  `WorktreeId`. Switching worktrees keeps the global Tasks context tab selected
  while restoring that worktree's presentation state.
- Fence every async completion by source signature, project/repository identity,
  auth generation, request generation, and expected worktree where it mutates
  presentation. Host, connection, distro, remote URL, account, or Retry changes
  invalidate prior work.
- Single-clicking a row opens or replaces a worktree-scoped read-only detail
  preview in the central unified tab strip. Double-clicking its tab promotes it
  to permanent; closing or pinning one worktree's detail never affects another.
- Offline/rate-limit/transient failures may retain a timestamped last-known list
  for the same exact cache identity. They must not contaminate another project
  and must not disable Files, Git, or Sessions.
- `MINI_TERM_GITHUB_PROJECT_TASKS=0` restores the existing unavailable Tasks
  placeholder without removing newer identities or cache state.

## Acceptance Criteria

- [ ] Local, WSL, and SSH fixtures prove all discovery, auth, list, and detail commands execute on the intended host with no credential fallback.
- [ ] HTTPS, SSH URL, scp-style URL, GitHub Enterprise host, `.git` suffix, malformed URL, and hostile input parsing are covered by tests.
- [ ] Main and linked worktrees share one project/repository fetch while retaining separate mode, filter, selection, scroll, and detail preview state.
- [ ] Switching project, host, distro, connection fingerprint, remote URL, account, or auth generation rejects stale list/detail completions.
- [ ] Issues and PRs render from JSON; malicious text remains inert and Markdown details do not render raw HTML.
- [ ] Clicking an Issue/PR opens a worktree-scoped internal preview tab and ordinary viewing never invokes a URL opener.
- [ ] Client-missing, auth-required, scope, rate-limit, disconnected, non-GitHub, empty, and malformed states have distinct UI.
- [ ] Auth-required UI shows the correct host and command, Copy works, Retry reprobes, and no login/browser/terminal side effect exists.
- [ ] Docker-only focused tests, workspace check, Clippy, and Windows MSVC check pass.

## Out Of Scope

- Creating, editing, closing, commenting on, assigning, merging, or reviewing
  Issues and Pull Requests.
- GitHub Projects boards, Actions, notifications, releases, Linear, or Jira.
- Embedded web authentication, OAuth token handling, token transport, or browser
  navigation.
- Persisting GitHub response bodies or credentials across application restarts.
