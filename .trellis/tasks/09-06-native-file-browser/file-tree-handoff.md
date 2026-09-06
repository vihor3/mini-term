# FileTree Handoff: R12 / R13

## Status and Boundary

Implementation is written; acceptance is pending Actions. No tests, fixtures,
compilation, Cargo metadata, lint, formatting, generators, whitespace checks,
application launch, or automated UI checks were run locally. No Git staging,
commit, push, reset, or remote user-device testing was performed.

Changed files:

- `crates/mt-app/src/file_tree/mod.rs`
- `crates/mt-app/src/file_tree/menu.rs`
- `crates/mt-app/src/file_tree/ops.rs`
- `crates/mt-app/src/file_tree/tests.rs`
- `crates/mt-i18n/locales/fileTree.ts`
- This handoff.

`main.rs` did not require editing. The global ContextPanel selection, navigation
targets, store/sidebar, Agent work, directory picker/onboarding, remote SSH
implementation, shared libraries, workflows, and specs were not changed.

## Sizing Finding

Source chain reviewed: `Workspace::render_context_sidebar` gives its content
slot `flex_1 + min_h(0) + overflow_hidden`; FileTree fills that slot with
`size_full + flex_col`. The old scroll shell was a default block `div` with
`relative + flex_1 + min_h_0`, while its list relied on `flex_1` without a flex
parent or explicit height. Thus the list's flex sizing did not constrain its
block-layout height. This is a source-level cause candidate, not a reproduced
native scroll failure or verified native fix.

The replacement shell is explicitly a flex column with bounded, shrinkable
height and width. Its list has `flex_1 + min_h(0)`, owns the existing scoped
ScrollHandle, and is the only scrolling element. Rows do not flex-shrink.
Status strips stay outside the list with nonshrinking height. No fixed viewport
height or second scroller was introduced.

The pinned gpui-component scrollbar is 16px wide. The overlay is now limited to
that right-hand gutter, with explicit top/right/bottom placement, and the list
reserves the same gutter. It does not add mouse occlusion or a wheel handler.
Source inspection of the existing full-area scrollbar found a normal hitbox
and a wheel observer that does not stop propagation; do not report wheel
interception as a proven prior root cause.

Pinned source was read without executing it:

- [gpui 0.2.2 source archive](https://static.crates.io/crates/gpui/gpui-0.2.2.crate):
  `src/style.rs` default block display; `src/elements/div.rs` scrolling and
  `scrollbar_width`; `src/styled.rs` flex sizing.
- [gpui-component 0.5.1 source archive](https://static.crates.io/crates/gpui-component/gpui-component-0.5.1.crate):
  `src/scroll/scrollbar.rs` bar width, absolute sizing, hitbox and event behavior.

## Interaction Changes

- Header contains only Refresh, using mt-ui's existing refresh glyph and
  `IconTooltips::button/group` with the shared 500ms cold/warm hover policy.
  Search/editor/paste/create/upload header tools and unused local shapes were
  removed. Global search and editor settings outside Files are untouched.
- Both row kinds have New File and New Folder. Existing applicable row actions
  remain: local file default-open, copy, directory paste, remote download,
  relative/absolute path copying, local reveal, terminal open, rename, delete,
  and local changed-file diff. There was no FileTree Cut action before this
  change; no new move protocol was introduced.
- Blank space has exactly New File and New Folder, targeting the displayed
  project root. Broken remote sources do not gain usable local actions.
- Upload menu actions and the file chooser entry point were removed. External
  path drops are the sole FileTree upload entry. Internal path-to-terminal
  dragging remains separate and unchanged.
- File double-click still opens Rename; file single-click previews and directory
  single-click toggles expansion. Rename is deferred out of the entity update
  lease before opening its source-validated prompt.

## Target API and Ownership

The new private `FileTreeContextTarget` captures a typed `Row` or `Blank`, full
`FileOperationContext`, WorktreeId, and observed SSH epoch. `directory()` is the
shared destination selector for creation, terminal opening, and external drops:
directory row -> itself; file row -> its parent; blank -> captured root. Local
paths use `Path::parent`; SSH paths use existing `parent_posix`, preserving
remote backslashes on Windows. Selection and hover are never fallback targets.

`matches_source`/`is_current` check project/root/backend fingerprint/generation,
both store and FileTree worktree ownership, and observed epoch. Row/blank menu
callbacks, rename/delete/create prompt callbacks, selection/click handlers, and
drop dispatch use that captured target. Upload preflight completion and conflict
choice also reject an observed reconnect before proceeding.

Private APIs changed:

- `file_menu_actions` and `file_menu` consume the typed target; no separate
  `background_menu` or header action-capability matrix remains.
- `new_entry_prompt`, `open_rename_prompt`, `open_entry_in_terminal`, and
  `start_upload` receive the typed target instead of separate row/path/context
  arguments. No external consumer depended on these private APIs.
- `FileTree::drop_external_paths` centralizes the exact-source check and deferred
  upload needed to avoid GPUI double leasing during native OLE Drop callbacks.
- `create_entry_at_target` is the production dispatch helper exercised by
  tests. It requires a single basename, uses structured local `PathBuf::join`,
  fails closed on missing/mismatched remote connections, and delegates all
  canonical containment, symlink-parent, and exclusive-create rules to the
  existing Local/SSH I/O helpers.

The existing per-worktree cache and ScrollHandle swap path remain unchanged.
Operation-busy ownership, clipboard generation checks, watcher suppression,
conflict/cancellation handling, staged/bounded transfers, and download-directory
validation remain in their original operation/backend paths.

## Regressions Written, Not Run

- Exact local/remote file/folder action matrices and blank creation-only menus.
- Shared creation/drop destinations and distinct rename/delete refresh parents.
- POSIX backslash filenames and Windows drive/UNC/WSL UNC parent semantics.
- Each mismatched source field, same-path worktree changes, ABA generations,
  observed reconnect/disconnect, and stale clipboard/recursive paste fencing.
- Existing same-path worktree cache regression now also restores B after A,
  checking separate rows, Git status, selection, error, and scroll both ways.
- Production layout-builder assertions for the flex height chain, one list
  scroll owner, nonshrinking rows, and gutter-only absolute scrollbar overlay.
  These assertions do not prove native layout or input delivery.
- Local production-create fixtures check file-parent/directory/root placement,
  same-name preservation, invalid basename rejection before I/O, no remote-to-
  local fallback, and Unix symlink parent escape rejection while a clicked
  leaf symlink still creates safely in its own parent.
- Existing click, watcher/request, source identity, compact-directory, and Git
  presentation regressions remain present.

## Main Integration and Remaining Acceptance

1. Run the normal Linux and Windows Actions gates for the exact integrated
   product commit. Tests are under `file_tree::tests` in mt-app's `mini-term`
   binary. The current Windows workflow only runs onboarding/SSH-focused app
   tests, so the new Windows-only path test needs an Actions FileTree test run
   in addition to Windows compilation. No local substitute is allowed.
2. Generate i18n only in Actions and apply its diagnostic patch. This slice
   adds three keys per language under `fileTree.operation`: `invalidName`,
   `invalidTarget`, `sourceUnavailable`. Main must update the combined
   `EXPECTED_ENTRIES_PER_LANG` count and mt-app `USED_KEYS` list in the files
   outside this slice. Legacy unused upload/editor locale entries were retained
   to avoid unrelated dictionary churn; they are not runtime entry points.
3. Have main dispatch Trellis check. This implementer did not spawn agents or
   claim an independent check pass.
4. Actions-hosted native acceptance remains required for final-row wheel and
   scrollbar access, narrow/high-DPI layouts, hover/context targets, empty-list
   blank targets, refreshed/restored cache positions, and Files/Git/Sessions
   switching across same-path worktrees. Check cold/warm tooltip timing and
   both double-click rename gestures.
5. Exercise external file/directory drops on file/directory/blank targets,
   conflicts (all policies), cancellation, errors, progress, source switches,
   reconnects, and upload-command absence using Actions-produced artifacts.

Known backend boundary: existing `upload_conflicts`/`upload_paths` acquire SFTP
sessions internally and have no expected-epoch argument. The added UI checks
reject changed observed epochs but do not pin a backend acquisition racing a
reconnect after dispatch. Those APIs and transfer semantics were deliberately
left unchanged in the reserved remote_ssh layer. Broader end-to-end epoch
pinning requires separately coordinated backend work; do not claim it here.
