# Orca Worktree Visibility Research

## Scope and Evidence

Investigated on 2026-09-05 using the local Orca source checkout at
`/home/leo/orca`, commit `5aa02ead59a4f34a186c3e8814558b5795260ee9`, and
read-only Git commands against the user's two example checkouts. This is
research only; no product code, Git registrations, branches, or Orca user
settings were changed. Orca dependencies are absent, so its tests and UI were
not executed. Existing source tests were inspected as supporting evidence.

## The Two Screenshots Refer to Different Local Git Inventories

Both `/home/leo/ML307H-AICOS` and `/home/leo/cyberbase-v26.019` have the same
`origin`: `https://github.com/AiSpea/ML307H-BASE.git`. The user's statement that
they are the same upstream repository is correct. However, they have separate
`.git` directories and separate local branch/worktree registrations.
`cyberbase-v26.019` additionally has a `cyberbase-old` remote pointing at
`/home/leo/ML307H-AICOS`.

| Local checkout | Local branch refs | Registered worktrees | Prunable registrations |
| --- | ---: | ---: | ---: |
| `/home/leo/ML307H-AICOS` | 21 | 12 | 3 |
| `/home/leo/cyberbase-v26.019` | 5 | 3 | 0 |

The initial statement about five local branches was scoped only to the second
checkout, not the entire shared upstream or the AICOS checkout. It must not be
used to explain the AICOS screenshot.

Commands used for each checkout:

```bash
git -C <checkout> rev-parse --show-toplevel --git-dir --git-common-dir
git -C <checkout> config --get-regexp '^remote\..*\.url$'
git -C <checkout> for-each-ref '--format=%(refname:short)' refs/heads
git -C <checkout> worktree list --porcelain
```

The first screenshot,
`/home/leo/.cache/tmp/orca-paste-1788582316064-c5b9f242-47ba-495f-999e-f31dfa8d453a.png`,
shows `cyberbase-v26.019` and exactly these three registered worktrees:

| Directory | Checked-out branch |
| --- | --- |
| `/home/leo/cyberbase-v26.019` | `cyberbase-v26.019.00049` |
| `/home/leo/cyberbase-v26.019-yongtong-dialog-toy` | `codex/yongtong-dialog-toy-port` |
| `/home/leo/cyberbase-v26.019-neural-aec` | `codex/integrate-neural-aec-worktree` |

This correspondence does not prove a numerical limit, a recent-branch rule, or
that Orca applied any additional hiding to this particular three-worktree
inventory. The screenshot's saved visibility settings and metadata were not
read, so the precise reason each external worktree was opted in remains
unverified.

The second screenshot,
`/home/leo/.cache/tmp/orca-paste-1788610151597-b1aebdbd-ae15-4afb-9bcc-1c88e16692e4.png`,
shows mini-term's `ML307H-AICOS` group, with rows matching its separate worktree
inventory. `aicos-brookesia` is visibly marked `prunable`. Git also reports
`/home/leo/cyberbase-vad` and `/home/leo/watch` as prunable; all three have a
gitdir registration pointing to a missing location. The other nine
registrations are not marked prunable. No claim is made that Orca would show
exactly three rows for AICOS under its current unknown settings.

## Orca Data Flow

1. Git discovery reads worktree registrations, not all local or remote branch
   refs: `src/main/git/worktree-list-reader.ts:194` uses
   `git worktree list --porcelain -z` with a compatibility fallback.
2. `src/main/ipc/worktrees/listing/ssh-worktree-fallback.ts:136`,
   `buildDetectedGitWorktrees`, first removes `prunable` registrations and
   deduplicates paths, then derives ownership and visibility using host-scoped
   metadata and repository settings.
3. `src/main/ipc/worktrees/listing/register-worktree-catalog-handlers.ts:138`
   and `:224` filter detected entries by `visible` for the normal worktree
   catalog. The separate `worktrees:listDetected` route preserves discovery
   information for management and import affordances.
4. Sidebar presentation applies additional user filters, such as sleeping,
   detached, automation-created, CLI-created, and host scope:
   `src/renderer/src/components/sidebar/worktree-list/listing/use-filters.ts:10`.
   The core catalog policy is not a fixed count or a requirement to have an
   active Agent session.

## Core Visibility Precedence

`src/shared/worktree-visibility-resolution.ts:14`, `shouldShowWorktree`, checks
these rules in order, after the prunable exclusion:

1. The repository's selected checkout path, or `orca-managed` ownership,
   is visible. The selected checkout is `repo.path`, not whichever worktree
   has current UI focus, and is not necessarily Git's main worktree.
2. A path in `repo.importedExternalWorktreePaths` is visible, including an
   explicitly imported scratch worktree.
3. A matched built-in or custom visibility source follows its source policy.
4. Other `agent-scratch` worktrees follow `agentWorktreeVisibility`, defaulting
   to hidden.
5. `unknown-legacy` ownership in a legacy repository stays visible.
6. Other external worktrees follow the repository override, then the global
   external default, then compatibility fallback: new repositories hide;
   legacy repositories show.

Ownership is derived in `src/shared/worktree/ownership.ts:111`. Strong
creation/legacy metadata can prove managed ownership; a directory name,
branch prefix, generic discovery metadata, or mere placement under a
workspace root does not prove that Orca created a worktree.

`src/shared/worktree/visibility-sources.ts:18` defines the built-in scratch
sources `.claude/worktrees` and `.gsd-workspaces`. Matching is anchored to known
checkout roots and uses path boundaries. An explicitly configured base at or
inside the matched scratch root can exempt its worktrees from the built-in
classification. Do not hide all branches named `codex/*` or all directories
whose names happen to contain an Agent name.

`src/shared/external-worktree-visibility.ts:66` resolves external defaults.
Migration preserves existing repositories' previous visible behavior while
initializing the new global default to `hide`. New repository registration
explicitly sets `externalWorktreeVisibilityLegacy: false` in
`src/main/ipc/repos/local-repo-registration.ts:87`.

Inspected tests include:

- `src/shared/worktree/ownership.test.ts:154`: a metadata-free nested workspace
  is external and hidden under the new-repository policy.
- `src/shared/worktree/ownership.test.ts:313`: a selected linked checkout stays
  visible while an unselected Git main checkout can be hidden.
- `src/shared/worktree/ownership.test.ts:342`: new/legacy defaults, override
  precedence, and migration compatibility.

## Hidden Entries Remain Discoverable

`src/renderer/src/components/sidebar/worktree-list/listing/use-external-worktree-cards.ts:23`
uses the detected inventory for hidden-worktree notices.
`src/renderer/src/components/sidebar/new-external-worktrees-inbox-actions.ts:104`
imports chosen paths into `importedExternalWorktreePaths` and refreshes an
authoritative snapshot; a failed refresh rolls back the metadata update.
There are also show-external, keep-hidden, and suppress-discovery-notice
actions. Hiding is a presentation preference, not Git deletion or pruning.

For SSH disconnection, the existing reference has a separate persisted,
host-qualified metadata fallback in
`src/main/ipc/worktrees/listing/ssh-worktree-fallback.ts:92`. Do not treat an
offline fallback as a new authoritative inventory or apply destructive
cleanup based on it.

## Same Upstream Does Not Mean One Sidebar Checkout Group

`src/renderer/src/components/sidebar/worktree-list/grouping/project-grouping.ts:71`
tracks distinct user checkout paths per project and execution surface.
`getProjectGroupingForRepo` at `:122` emits setup-specific headers when more
than one independent user checkout shares the same project/host surface.
Thus even a common upstream-derived project identity does not require merging
the AICOS and v26.019 local inventories into one checkout list.

## Mini-term Difference and Planning Implications

- `crates/mt-app/src/worktree_catalog.rs:1070`, `build_groups`, currently
  projects every discovered `WorktreeFact` into a catalog row. It already
  separates branch refs from worktrees; the defect is not an all-branch-ref
  enumeration in this path.
- `crates/mt-app/src/worktree_catalog.rs:1190`, `row_from_fact`, carries the
  prunable state and normally makes an unconfigured invalid row unselectable,
  but does not remove it from the list.
- `crates/mt-app/src/orca_sidebar.rs:816` renders every catalog row in an
  expanded group. There is no comparable ownership/visibility projection.

The proposed change should preserve authoritative discovery and stable
host/path identity, then derive a selective navigation view. Retain the raw
inventory for worktree management and diagnostics. Existing configured
projects and sessions must not be deleted or silently reclassified as new
unmanaged discoveries, and refresh failures must not erase last-known state.
Whether hidden entries also remain available through Quick Open needs to be
made explicit when planning the shared catalog consumers.

The subsequent user decision resolves the default: newly discovered valid
worktrees remain automatically visible, and unwanted worktrees are hidden
manually in project settings. Do not port Orca's ownership-based default-hide
policy. Preserve existing configured projects, use explicit hidden choices,
and keep invalid worktrees excluded from default sidebar presentation. This
decision overrides the earlier recommendation to hide unselected worktrees.
