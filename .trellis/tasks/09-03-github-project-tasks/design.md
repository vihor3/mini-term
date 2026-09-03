# Technical Design

## Architecture

```text
GitHubTasksPanel / GitHubWorkItemViewer
                  |
          GitHubTaskService
      source/cache/generation fences
                  |
        ExecutionHostCommandRunner
       /              |              \
 native process   wsl.exe --exec   pooled SSH exec
                  |
              mt-github
 URL parsing / command plans / JSON / error classification
```

`mt-github` is a UI- and transport-free domain crate. It owns repository and
work-item data types, pure parsers, command-plan builders, output limits, and
error classification. It receives completed command output and never reads
configuration, spawns a process, opens a browser, or handles credentials.

`mt-app` resolves a project to an immutable execution snapshot containing the
root project ID, `ExecutionHostId`, canonical worktree path, backend identity,
and user-facing host label. A single runner executes a structured program/argv
request on that snapshot. Local and WSL remain argv-based; SSH uses one
allowlisted POSIX serializer before the existing bounded remote exec API.

## Execution Snapshot

```text
ExecutionSnapshot {
  project_id, root_project_id, worktree_id,
  execution_host_id, canonical_path,
  backend: Local | Wsl { distro } | Ssh { connection, fingerprint, epoch },
  source_signature, host_label
}
```

The snapshot is captured on the GPUI thread and passed to background work. It
contains no token. Every completion re-resolves the current project binding and
compares the source signature before changing active presentation.

## Discovery And Authentication

The pipeline is ordered and bounded:

1. `git remote get-url origin` in the execution-host worktree.
2. Parse and normalize a GitHub or GHES remote into `host/owner/repo`.
3. `gh --version` to distinguish missing client.
4. `gh auth status --hostname <host>` plus a read-only account probe.
5. Issue and PR list commands with explicit JSON field lists and full
   `[host/]owner/repo` identity.
6. Detail commands on demand, again using JSON only.

Nonzero command output is classified without copying raw stdout/stderr into
persistent state. Diagnostics are bounded. Authentication source may be shown
as a nonsecret category, never as a value.

## Cache And Presentation Ownership

```text
ProjectSourceKey = ExecutionHostId + root ProjectId + source signature
GitHubCacheKey   = ProjectSourceKey + GitHubRepoIdentity + auth generation
WorktreeUiState  = WorktreeId -> mode/filter/selection/scroll
DetailTabKey     = WorktreeId + GitHubRepoIdentity + kind + number
```

A project-source record owns discovery generation and the current normalized
repository. A repository cache owns auth/list state, last-known rows, fetch
request ID, and detail cache. A loading cache is reused by sibling worktrees;
it does not start duplicate commands.

Retry increments auth and fetch generations. A source change retires the old
project-source record. Late callbacks may update only their exact inactive cache
bucket and never replace active rows. Last-known data is shown only when the
full cache identity still matches.

Each worktree keeps its own UI state. The selected context tab remains owned by
`Workspace`, so a worktree switch while Tasks is active remains on Tasks.

## Workbench Integration

`WorkbenchArea` gains a work-item tab collection beside document tabs. Both
render in the same tab strip after Terminal and are stored inside the existing
`WorktreeId` bucket. Work-item previews have the same replace/promote/close
semantics as document previews but are read-only, so promotion occurs only on
preview-tab double-click or explicit pin.

`GitHubWorkItemViewer` starts in loading state, requests an exact repository
item through the shared service, and fences completion by its immutable tab key
and execution source. It renders sanitized Markdown text with raw HTML disabled
and exposes no browser action.

## UI States

The Tasks panel uses a compact toolbar with an Issue/PR segmented control,
state filter menu, and refresh icon. Rows show number, title, state, author,
labels, and updated time. Empty/error surfaces occupy the panel body and do not
use nested cards.

Auth-required state contains only host label, command text, Copy, and Retry.
Other failure states have a short diagnosis and Retry where meaningful. The
panel never creates a terminal or invokes a URL opener.

## Compatibility And Rollback

`MINI_TERM_GITHUB_PROJECT_TASKS=0` swaps the Tasks body back to the current
placeholder. The new crate, workbench tab type, and cache state remain harmless
and no schema downgrade is required. Files, Git, Sessions, terminal runtime, and
Agent tracking are independent.
