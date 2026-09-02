# Research: File-tree header quick actions

- Query: Determine the exact implementation path for upload file, upload folder, paste, new file, and new folder buttons beside the file-tree refresh button.
- Scope: internal
- Date: 2026-08-27

## Findings

### Files found

- `/tmp/mini-term-pr-review-worktree/crates/mt-app/src/file_tree.rs` — owns the file-tree header, operation identity, clipboard, upload picker, paste, create prompts, row/background menus, and operation busy state.
- `/tmp/mini-term-pr-review-worktree/crates/mt-app/src/file_ops.rs` — defines `FileOperationContext`, backend identity, clipboard compatibility, and recursive-copy rejection.
- `/tmp/mini-term-pr-review-worktree/crates/mt-ui/src/icons/vector.rs` — supplies the unit-square vector shape DSL and supports composing a base icon with an overlay.
- `/tmp/mini-term-pr-review-worktree/crates/mt-i18n/locales/fileTree.ts` — already contains Chinese and English labels for all five requested actions.
- `.trellis/spec/mt-app/backend/quality-guidelines.md` — requires operation/source identity checks and a shared operation lock for local/remote file operations.
- `.trellis/spec/guides/code-reuse-thinking-guide.md` — requires reusing existing operation entry points rather than duplicating transfer/create logic.

### Current header insertion point

- The header action group is built at `crates/mt-app/src/file_tree.rs:2435-2490`; refresh is the stable insertion anchor at `:2465-2484`.
- The group already uses fixed 26×26 buttons from `header_button` at `:2327-2340`, with unique static IDs, tooltips, and 13 px `VectorIcon`s.
- The requested order can be implemented immediately after refresh: upload file, upload folder, paste, new file, new folder. Upload actions should only be rendered for a connected SSH backend; paste/new-file/new-folder apply to both local and connected remote roots.
- `is_remote` deliberately counts broken SSH projects as remote (`:631-638`), so it is not sufficient for upload visibility. Match `FileOperationContext.backend` against `FileBackendIdentity::Remote { .. }`; treat `BrokenRemote` as unavailable.

### Root action context and connection

- `FileTree::operation_context` (`file_tree.rs:417-437`) is the authoritative snapshot: project ID, root, backend/connection fingerprint, and source generation.
- Root-targeted toolbar actions should use `context.root.clone()` as their destination. This is identical to the background-menu implementation at `file_tree.rs:2131-2242`.
- Resolve the context again inside each click listener rather than capturing a render-time context. This avoids stale project/connection/generation state. Existing operation functions revalidate it again before mutation.
- Resolve the SSH connection at click time with `FileTree::remote_conn` (`file_tree.rs:401-408`). Pass it only to `new_entry_prompt`; upload helpers fetch and validate the live connection themselves.

### Existing action entry points to reuse

- Paste: call `paste_file_clipboard(entity, context, context.root.clone(), window, cx)`; implementation and identity checks are at `file_tree.rs:1357-1444`.
- Upload file: call `choose_upload_paths(entity, context, context.root.clone(), false, window, cx)`; picker semantics are at `file_tree.rs:1603-1661`.
- Upload folder: same call with `directories = true`.
- New file: call `new_entry_prompt(entity, context, remote_conn, context.root.clone(), false, window, cx)`; implementation is at `file_tree.rs:2245-2315`.
- New folder: same call with `is_dir = true`.
- These calls exactly match the root background-menu actions at `file_tree.rs:2140-2185` and `:2206-2240`; no new transfer, conflict, or create path is needed.

### Clipboard and disabled state

- Clipboard validity is `file_clipboard.can_paste_into(&context)` (`file_ops.rs:37-44`). It binds paste to the same project, root, backend/SSH fingerprint, and source generation, and rejects a broken remote backend.
- For a root paste target, no additional `would_copy_into_itself` check is required for button enablement; `paste_file_clipboard` still performs the authoritative recursive-copy check before execution (`file_tree.rs:1381-1389`).
- `operation_busy` is the shared mutation/transfer lock (`file_tree.rs:106-113`). `begin_tree_preflight` and `spawn_tree_op` reject concurrent work and show the existing busy alert (`:1097-1133`, `:1177-1214`).
- Recommended toolbar availability:
  - upload file/folder: visible only for `Remote { .. }`, enabled only when `!operation_busy`;
  - paste: visible for local and connected remote, enabled only when `!operation_busy && can_paste`;
  - new file/folder: visible for local and connected remote, enabled only when `!operation_busy`;
  - broken remote: render no upload buttons and disable paste/new buttons, or omit all mutation buttons consistently.
- GPUI `Div` has no menu-style `.disabled(...)` behavior in this code. Refactor `header_button` to accept a disabled flag (or add a sibling helper), apply reduced opacity, attach pointer/hover styling only when enabled, and conditionally attach the click listener. Keep click-time identity validation even when the rendered button was enabled.

### Icon implementation

- There are no shared generic upload/paste/new-file/new-folder command icons in `mt-ui::icons`; the current file-tree header defines its search/refresh/caret shapes locally (`file_tree.rs:2342-2386`).
- `VectorIcon::overlay` composes a base table with a second shape table (`crates/mt-ui/src/icons/vector.rs:291-295`). The lowest-duplication implementation is to add file-local reusable shape tables:
  - file outline;
  - folder outline;
  - upward-upload mark;
  - plus mark;
  - clipboard/paste outline.
- Compose file + upload, folder + upload, file + plus, and folder + plus with `.overlay(...)`; render paste from its standalone table. This follows the existing unit-square vector convention and keeps all five icons visually consistent at 13 px.
- `crate::activity_bar::UPDATE` is a public upward-arrow glyph (`activity_bar.rs:446-476`), but it is tied to the activity-bar update affordance and lacks file/folder context. A small file-tree-local upload overlay avoids cross-feature coupling while still reusing the vector composition primitive.

### Tooltip and i18n reuse

- Existing keys cover every tooltip: `fileTree.menu.uploadFiles`, `uploadFolder`, `paste`, `newFile`, and `newFolder` (`crates/mt-i18n/locales/fileTree.ts:3-17`, English at `:102-116`).
- All five keys are already registered in `crates/mt-app/src/i18n.rs:246-254`. No locale or generated dictionary change is required if these menu labels are reused.

### Concrete implementation map

1. In `file_tree.rs`, add reusable local vector shape tables beside `SEARCH_SHAPES`/`REFRESH_SHAPES`.
2. Make the header button constructor support enabled/disabled styling without attaching interaction for disabled actions; update the existing search/refresh calls accordingly.
3. During render, derive a lightweight availability snapshot from `self.operation_context(cx)`, `self.file_clipboard`, and `self.operation_busy`; use backend matching rather than `is_remote` for connected-remote capability.
4. Insert the five buttons after refresh in the requested order, conditionally rendering the two upload buttons only for a connected remote backend.
5. In every listener, obtain the current `operation_context`, root, and connection from `this` and dispatch to the existing root action functions listed above.
6. Reuse the existing `fileTree.menu.*` strings for tooltips, so no i18n regeneration is needed.
7. Add pure tests for any new availability/order helper if one is introduced. Per task constraints, Rust formatting/compile/Clippy/tests run only in GitHub Actions; local verification is static diff review and `git diff --check`.

## External references

- None. The repository's existing GPUI and file-operation patterns fully determine the implementation.

## Related specs

- `.trellis/spec/mt-app/backend/quality-guidelines.md`: preserve source identity, shared busy ownership, and local/remote capability separation.
- `.trellis/spec/guides/code-reuse-thinking-guide.md`: route toolbar commands through the existing background-menu operation functions.

## Caveats / Not Found

- The current header comment says “three” icon buttons (`file_tree.rs:2327`); it should be generalized when the toolbar expands.
- Six compact remote actions (refresh plus five new buttons) occupy about 176 px at the current 26 px size and 4 px gap. The project title already flexes/truncates, but very narrow panes should be checked visually in CI artifacts/manual review.
- The existing upload picker opens before `begin_tree_preflight`; disabling toolbar actions while `operation_busy` prevents opening redundant pickers from the new persistent buttons, while the existing right-click path remains protected at transfer start.
- This research covers only the requested file-tree header actions. File viewer/editor, syntax highlighting, indentation assistance, and rendered Markdown need separate cross-layer research and design.
