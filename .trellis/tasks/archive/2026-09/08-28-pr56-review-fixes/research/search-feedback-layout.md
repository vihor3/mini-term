# Search, Feedback, and Layout Research

## Search state and click semantics

Current branch behavior closes `GLOBAL_SEARCH` on every result click and constructs a new `SearchModal` on every open. Closing drops the entity, query, results, and in-flight state. It also removed the pinned `ResultAction` test from `origin/main`.

Recommended design:

- install one lazy `GlobalSearchModal(Entity<SearchModal>)` and reuse it across overlay opens;
- closing the overlay hides the view but does not destroy query/results;
- project observation continues to reset results when the producing project/root is no longer active;
- restore `ResultAction`: click count 1 previews in the workbench, count >=2 opens the configured external editor;
- because closing the dialog on click 1 would prevent click 2 from reaching the row, open the workbench tab immediately behind the overlay and delay overlay closure for the platform double-click window; click 2 cancels the pending close, launches the external editor, and closes the overlay;
- reopening Ctrl/Cmd+Shift+F restores the previous query/results; an active search may continue while the overlay is hidden.

For an SSH project, `open` must push an informational localized toast rather than silently returning. No remote filesystem search backend is added.

## Download identity mismatch

`file_tree::download_remote_file` intentionally rechecks project id, project root, connection id, and connection fingerprint before delegating to the existing download pipeline. Every failed check currently returns silently.

Keep every identity guard. On failure, use the global store/project metadata and the existing custom toast path to show a localized “download context changed; reopen or refresh the file” message. The toast must not start a transfer or fall back to the current connection.

## Dirty worktree project removal

Explicit project/worktree removal already shows `fileViewer.projectRemovalBlocked` before calling `AppStore::remove_project`. Automatic stale-worktree reconciliation calls `remove_project` directly. The new store-level dirty guard therefore protects data but leaves a stale row with no explanation.

At the store guard, resolve the project name before returning and push the existing blocked-removal message as an error/warning toast. Explicit paths keep their alert; the store toast is the race/automatic-cleanup backstop.

## Markdown image width

`preview_avail_width` subtracts padding from `window.viewport_size()` and caps at 860px. In the workbench, the actual Markdown content column may be much narrower because the file tree and right drawer consume width.

GPUI 0.2.2 `Styled` provides `max_w_full`, which resolves to 100% of the parent width. The safe layout boundary is therefore the actual `div().max_w(860px).w_full()` content column, not the window viewport.

Implementation direction:

- retain 860px only as the intrinsic design cap used by `image_display_width`;
- remove viewport-derived available width;
- add `max_w_full()` / `min_w_0()` to image, placeholder, row, and linked-image wrappers as appropriate;
- let flex-wrap use the real parent width; a too-wide single image shrinks and multiple large images wrap instead of overflowing.

Primary API reference: `gpui 0.2.2::Styled::max_w_full` on docs.rs.

## Close confirmation limit

`CLOSE_RISK_PREVIEW_LIMIT = 5` and `close_risk_preview` show the first five names plus localized remaining count. This avoids unbounded text in a 360px confirmation dialog and matches the conflict-dialog truncation style from the prior review task. No product behavior change is required; add or retain a pure test and explain the decision in the PR review reply.

## Tests needed

- `result_action` single/double/triple mapping;
- persisted search entity resets results on project identity changes but not on simple overlay close/reopen;
- remote-search invocation selects the feedback path and does not start local search;
- download mismatch and dirty-project removal select visible-feedback paths without mutating state;
- close-risk preview contains five names and the correct remaining count;
- image elements and wrappers carry parent-width clamps (static review plus GitHub Actions compilation).
