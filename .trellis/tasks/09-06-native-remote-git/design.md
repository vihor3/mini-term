# Execution-Host Git Panel

## Action Surface and Domain Boundary

The [source action matrix](../09-05-sidebar-agent-status/research/09-06-remote-git-action-surface.md)
is the implementation inventory. Remote parity includes repository/ref discovery,
staged/unstaged/untracked status, stage/unstage/all, confirmed discard, commit,
pull/push, branch-filtered history, working/index/commit diffs, and the reachable
Worktree Management dialog. Branch selection is a history filter, not checkout.
Do not add checkout/stash/rebase/etc. commands absent from the current panel.

Retain shared `mt_project::git` DTOs and narrowly expose pure diff construction
where needed. Add structured host command plans and byte-safe machine-output
parsers inside the existing domain boundary; keep SSH/GPUI out of it. Preserve
current local implementations behind an explicit backend choice. Route every
remote child view/dialog/action through the selected host; removing the top-level
unsupported guard before all children are host-aware would cause local fallback.

## Source and Execution Ownership

Capture `ProjectExecutionSnapshot`, WorktreeId, project/root/backend/fingerprint,
authenticated epoch, selected canonical repository/common directory, and a
request/operation ID. Diff/history requests also capture branch/hash/paths;
dialogs have an instance owner. Revalidate before dispatch and before publishing.
A reused path or same WorktreeId across reconnect is insufficient authority.

Reuse bounded read execution and the existing epoch-pinned remote Git mutation
pattern. Preserve its dispatch/outcome information rather than reducing every
failure to an ordinary command exit. Local and WSL keep structured arguments;
SSH uses the existing quoting boundary. Git pathspec/option safety is separate
from shell escaping: literal paths, suitable separators, and validated object/ref
identities are required. Missing Git/capability, unavailable source, truncation,
invalid framing, and permission failure are explicit, never a clean repository.

## Mutation Lifecycle

Keep a source-owned coordinator for conflicting Git-panel writes, scoped by
authenticated repository/common directory and affected worktree. Operations
outlive their view if already dispatched; hiding the panel or changing project
cannot release a write lock or make the next view own its completion.

Represent known-not-dispatched/rejected, running, completed, and unknown-effect
outcomes. Cancellation before dispatch prevents work; cancellation or connection
loss after dispatch does not prove rollback. Never automatically retry commit,
pull, discard, or worktree mutations after possible effects. Reconcile status,
HEAD/refs, and inventory on the original source, including nonzero exits or
partial effects. An inactive matching bucket becomes refresh-needed, not Loading.
An old completion cannot clear a newer draft, spinner, dialog, or busy token.

Keep the execution host's Git hooks, signing, user config, and credentials.
Tasks account choice supplies no token/config to these operations. Bound prompt
or signing waits and surface the error instead of silently changing configuration.

## Semantic Compatibility

- Preserve separate index/worktree state, partial staging, rename old paths,
  conflicts, ignored exclusions, and unborn HEAD behavior. Stage All retains the
  existing repository-wide effect, not just the clicked visual group.
- Existing unstaged diff compares working bytes against HEAD, while staged diff
  compares index against HEAD. Preserve this distinction; default `git diff`
  alone is not equivalent. Bound both old/new blobs and handle missing/deleted,
  binary, root-commit, and renamed file cases explicitly.
- Preserve paginated history's parent-based continuation and graph deduplication,
  not a naive offset or first-parent-only walk. Commit diff uses the first parent
  or the empty tree for a root commit.
- Discard keeps explicit confirmation and exact paths, including its current
  tracked-file effect on staged content. Never replace it with global reset/clean.

## Worktree Management

Reuse the existing POSIX worktree parser/catalog authority and the new host-aware
directory browser for remote destinations. Open/Add/Switch use captured project
and host-qualified location/registration, not active-project fallback or local
`find_project_by_path`. Create registers only a verified result for its exact
still-live dialog owner; uncertain creation first requires read-only reconciliation.

Remove retains main-worktree exclusion, explicit Force choice, confirmation,
dirty-document guards, and exact target-terminal checks. Do not close replacement
or unrelated PTYs or delete project configuration because an old request returned.
Prune may clean only with complete current remote inventory and unambiguous host
absence, never a local filesystem check or an offline/permission error. Keep
manual worktree visibility distinct from destructive cleanup.

## Risks and Limits

CLI formats/ref/object/filename edge cases need Actions fixtures. Same-account
remote path replacement and external Git can still race; fail closed on changed
authority, index lock, HEAD/status mismatch, or uncertain effects. Do not claim
atomic rollback of user hooks or remote commands. Existing unrelated local
defects are not a license for a broad Git rewrite; extracted shared behavior must
have parity regressions and scoped changes.
