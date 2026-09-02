# Research: Terminal content tabs and embedded file editor architecture

- Query: How should clicking a local or remote file open a reusable editor page in the terminal content area, while preserving existing terminal pane/session behavior and reusing the current viewer/editor?
- Scope: internal
- Date: 2026-08-27

## Findings

### Executive conclusion

The repository already contains a mature local file viewer/editor. It already provides syntax highlighting, automatic indentation, line numbers, indentation guides, search, CRLF preservation, unsaved-change handling, and rendered Markdown/HTML preview. The missing work is architectural placement and remote I/O, not editor implementation.

The lowest-risk design is **not** to turn terminal `PaneState` into a terminal-or-document union. Instead, add a small workbench/content-tab host above `TerminalArea`: keep the existing `TerminalArea` entity alive as one content page, and host one `FileViewer`-derived entity per open document tab. This avoids changing PTY lifecycle, split-tree persistence, AI status aggregation, terminal focus/navigation, and terminal-panel semantics.

### Files found

- `/tmp/mini-term-pr-review-worktree/crates/mt-app/src/file_viewer.rs` — existing local file viewer/editor, currently implemented as a singleton modal.
- `/tmp/mini-term-pr-review-worktree/crates/mt-app/src/file_tree.rs` — file-row click entry point; explicitly rejects remote preview today.
- `/tmp/mini-term-pr-review-worktree/crates/mt-app/src/terminal_area.rs` — terminal split tree renderer and per-leaf terminal tab bars.
- `/tmp/mini-term-pr-review-worktree/crates/mt-app/src/tree.rs` — terminal-only `PaneState`, `ProjectPanel`, and `SplitNode` runtime model.
- `/tmp/mini-term-pr-review-worktree/crates/mt-app/src/store/mod.rs` — project runtime state and PTY entity map ownership.
- `/tmp/mini-term-pr-review-worktree/crates/mt-app/src/store/panes.rs` — terminal tab creation, activation, focus, hydration, PTY spawn, and disposal.
- `/tmp/mini-term-pr-review-worktree/crates/mt-app/src/store/layout.rs` — project-level terminal panel switching and persistence triggers.
- `/tmp/mini-term-pr-review-worktree/crates/mt-app/src/persist.rs` — persisted layout assumes every pane is a terminal shell.
- `/tmp/mini-term-pr-review-worktree/crates/mt-app/src/main.rs` — central three-column composition; directly mounts `TerminalArea` in the main content slot.
- `/tmp/mini-term-pr-review-worktree/crates/mt-app/src/hotkeys.rs` — workspace-wide terminal actions that need an explicit editor-active guard once the editor is no longer an overlay.
- `/tmp/mini-term-pr-review-worktree/crates/mt-app/src/remote_ssh.rs` — synchronous/background-thread SFTP service boundary and remote-root validation helpers.
- `/tmp/mini-term-pr-review-worktree/crates/mt-ssh/src/sftp.rs` — reusable SFTP handle; bounded reads and safe staged replacement primitives exist, but there is no direct atomic in-memory text write API.
- `/tmp/mini-term-pr-review-worktree/crates/mt-project/src/fs.rs` — local bounded file read and atomic file write APIs.
- `/tmp/mini-term-pr-review-worktree/crates/mt-app/Cargo.toml` — enables `gpui-component` tree-sitter language support.

### 1. Existing terminal content/tab/session model

The visible terminal content is a split tree. Every `SplitNode::Leaf` owns a `Vec<PaneState>` and an `active_pane_id`; each leaf therefore has its own terminal tab strip. Split nodes own direction, children, and percentages (`tree.rs:240-254`). `TerminalArea::render_node` recursively renders split nodes and delegates leaf rendering (`terminal_area.rs:1105-1154`).

Each `PaneState` is terminal-specific pure data: shell name, status, optional PTY id, cwd, AI session, and attention state. It intentionally does not hold the terminal entity; the PTY-backed `TerminalPane` entities live in `AppStore.terminals` keyed by `pty_id` (`tree.rs:90-119`, `store/mod.rs:5-18`).

The per-leaf tab bar assumes terminal semantics throughout: it displays terminal status/AI vendor, activates through `AppStore::activate_pane`, supports terminal drag/split behavior, closes through the AI-aware terminal close flow, and ends with a `+` that creates another terminal (`terminal_area.rs:1601-1612`, `terminal_area.rs:1660-1681`, `terminal_area.rs:1802-1834`, `terminal_area.rs:1867-1891`, `terminal_area.rs:1895-1938`). The leaf body resolves the active pane's `pty_id` to a `TerminalPane` entity and renders it (`terminal_area.rs:2287-2329`, `terminal_area.rs:2405-2415`, `terminal_area.rs:2516-2520`).

Project-level terminal panels are a second terminal-only layer. A `ProjectPanel` owns an entire split tree and corresponds to persisted `SavedTab`; the right-side `TerminalsPanel` switches among these terminal work surfaces (`tree.rs:176-199`, `store/layout.rs:94-155`, `terminals_panel.rs:388-443`). This is analogous to VS Code's terminal-instance list, not an editor-document tab model.

The central layout directly mounts `TerminalArea` in the right/main slot (`main.rs:337-352`, `main.rs:1394-1423`). Therefore the clean insertion point for document pages is a wrapper between `Workspace` and `TerminalArea`, not inside the terminal split-tree model.

### 2. Existing file viewer/editor and lifecycle semantics

`file_viewer.rs` already supplies the requested editor behavior:

- It uses `InputState::code_editor(lang)`, with the module contract documenting syntax highlighting, automatic indentation, line numbers, indentation guides, and built-in search (`file_viewer.rs:17-23`).
- Language selection covers common Rust/TS/JS/Python/Go/C/C++/HTML/YAML/Markdown/etc. types (`file_viewer.rs:191-244`).
- Editor creation explicitly enables code-editor mode, line numbers, wrapping where appropriate, and initial content (`file_viewer.rs:1538-1558`).
- Markdown and HTML start in preview mode because `preview` is initialized to `true` (`file_viewer.rs:1411-1438`).
- Markdown is rendered through `TextView::markdown` with themed syntax highlighting for code blocks, custom table/image handling, and a Typora-like constrained reading width (`file_viewer.rs:2221-2253`, `file_viewer.rs:2261-2320`).
- Source mode renders the existing code editor with monospace typography (`file_viewer.rs:2402-2436`).
- Saving preserves detected LF/CRLF endings (`file_viewer.rs:141-188`, `file_viewer.rs:1643-1687`).
- Local external modifications are watched; clean documents silently reload and dirty documents show a conflict banner (`file_viewer.rs:1596-1638`).
- Closing a dirty viewer asks for confirmation (`file_viewer.rs:1690-1712`).

However, its current lifecycle is unsuitable for multi-tab workbench use:

- A thread-local weak singleton `CURRENT` tracks the one open viewer (`file_viewer.rs:1258-1268`).
- `open` creates an overlay dialog, not an embedded page (`file_viewer.rs:1270-1329`).
- Opening another file while the singleton exists calls `navigate`, replacing the current file; this intentionally does not ask about unsaved changes (`file_viewer.rs:1280-1289`, `file_viewer.rs:1442-1458`).
- Dropping the entity removes its local directory watch (`file_viewer.rs:2491-2496`).

Recommended lifecycle for embedded tabs:

1. Key each document by `(project_id, backend identity, normalized path)`; clicking an already-open document activates/reuses it.
2. Keep a separate editor entity per tab so draft, undo history, preview/source state, and dirty state remain isolated.
3. Closing a dirty tab uses the existing unsaved confirmation; opening another file never navigates over a dirty document.
4. Keep document tabs runtime-only for the first implementation. Do not alter `SavedPane`/layout persistence until reopen-on-launch is explicitly required.
5. Project switching should select that project's last active content page while leaving other projects' documents alive, or close project documents when the project is removed.
6. A changed SSH connection fingerprint must invalidate/close the affected remote tabs or force a reload before save, preventing a stale tab from writing through a newly edited connection configuration.

There is no separate reusable `file_viewer`/editor component today: the editor state and the modal host are combined in `FileViewer`. The implementation should extract the reusable document entity/content from the overlay-specific singleton/open/close shell rather than duplicate its 2,000+ lines of behavior.

### 3. Existing local and remote open/save entry points

Local file-row clicks call `file_viewer::open(root, path, None, ...)`. Remote clicks are currently stopped with an explicit unsupported alert (`file_tree.rs:754-775`). Global local search also opens the same viewer and can pass a matching line number (`search_modal.rs:250-280`).

Local read/write is complete and guarded:

- `mt_project::fs::read_file_content` validates the target under the project root, rejects non-files, enforces a 1 MiB viewing limit, and distinguishes UTF-8 text from binary (`mt-project/src/fs.rs:349-386`).
- `mt_project::fs::write_file_content` uses the same project-root validation and limit, requires an existing regular file, and uses atomic write (`mt-project/src/fs.rs:388-399`).

Remote file operations already have the correct service boundary: `remote_ssh.rs` owns a small Tokio runtime, exposes synchronous blocking functions, and requires callers to run them on GPUI's background executor (`remote_ssh.rs:14-27`). It also centralizes pooled-session acquisition with one transport reconnect retry (`remote_ssh.rs:280-327`) and validates canonical paths against the remote project root (`remote_ssh.rs:533-590`).

What is missing for remote editing is a small pair of service APIs, for example:

- `remote_ssh::read_file_content(conn, project_root, path) -> FileContentResult`
- `remote_ssh::write_file_content(conn, project_root, path, content) -> Result<(), String>`

The read side can validate the leaf under the canonical root and use `SftpHandle::read_from_offset(path, 0, MAX_FILE_VIEW_SIZE + 1)` to distinguish over-limit data without loading unbounded content (`mt-ssh/src/sftp.rs:193-237`). It must verify the target kind is `File`, reject symlinks/special entries unless a deliberate follow-safe policy is implemented, and classify invalid UTF-8 as binary using the same result shape as local.

The save side should not round-trip through a local temporary upload file. `SftpHandle::upload_file` accepts a local path and already uses a staged sibling plus replacement (`mt-ssh/src/sftp.rs:460-545`), while its internal `replace_staged_entry` machinery provides the needed safe promotion behavior. Add an in-memory bounded text-write primitive to `SftpHandle` that creates a unique sibling, writes/flushed/shuts it down, then promotes it through the same replacement path. The app-level remote service must validate the target under the project root again immediately before saving.

Remote external-change detection does not exist. `FsWatcher` is local-only and the file tree intentionally does not register a watcher for remote projects (`file_tree.rs:10-18`). A minimal first version should either expose a manual reload/refresh path and clearly omit remote external-change warnings, or add a lightweight remote stat/mtime API and poll only while the remote tab is active. Continuous per-tab SFTP polling should not be introduced without batching/backoff.

Remote Markdown relative images are an additional edge: the current preview resolves relative resources to local paths/file URIs (`file_viewer.rs:2167-2218`). Text rendering will work after remote content loading, but relative remote images require an SSH-backed asset loader/cache or a custom scheme; treating a POSIX remote path as a local `file://` path is incorrect.

### 4. Recommended minimum change boundary

Recommended new boundary:

```text
Workspace main content slot
  -> WorkbenchArea (new)
       -> content tab strip
       -> Terminal page (existing Entity<TerminalArea>, kept alive)
       -> Local/remote document pages (Entity<FileDocument>)
```

Suggested ownership:

- `workbench_area.rs` (new): open/reuse/activate/close document tabs, active page, tab strip, unsaved-close delegation, project-scoped tab state.
- `file_viewer.rs`: refactor the current `FileViewer` into an embeddable document entity; preserve rendering/editor logic; keep a thin modal adapter only if search/modal behavior is still desired.
- `main.rs`: replace the direct `TerminalArea` child with `WorkbenchArea`; store one entity in `Workspace`.
- `file_tree.rs`: replace local-only modal call and remote unsupported alert with `workbench.open_document(...)` or an event/callback that reaches the workbench host.
- `remote_ssh.rs` + `mt-ssh/src/sftp.rs`: add validated bounded remote read and safe bounded text save.
- `hotkeys.rs` / `main.rs`: guard terminal-only workspace actions while a document page/editor owns focus; retain editor-local Ctrl/Cmd+S and Ctrl/Cmd+F behavior.
- i18n: add tab close/save/discard/reload/error strings only where existing `fileViewer` strings are insufficient.

The file tree currently owns only the store, so it cannot directly call a sibling `WorkbenchArea` entity. Prefer a small app-level event/callback registration or a shared document manager entity installed in `Workspace`; do not make the file tree import or own `TerminalArea`.

### Why not extend `PaneState` directly

Changing `PaneState` to a terminal/document union appears visually direct but has a large hidden blast radius:

- `hydrate_project` starts a PTY for every active-layout pane whose `pty_id` is missing (`store/panes.rs:530-615`). A document pane would be mistaken for an unhydrated terminal unless all hydration code becomes kind-aware.
- `spawn_pane` and terminal creation always create a PTY-backed `PaneState` (`store/panes.rs:652-666`).
- `close_pane` is explicitly a PTY disposal + terminal-layout removal path (`store/panes.rs:300-327`), and UI close goes through AI-aware terminal confirmation (`pane_actions.rs:97-130`).
- Focus, cycle, split, marker, AI status, notification, mobile relay, and project removal all iterate the same pane set and assume terminal semantics (`store/panes.rs:357-488`, `pane_actions.rs:65-82`).
- Persistence serializes every leaf pane as `SavedPane { shell_name, cwd, ai_session }` and restores each as a terminal shell (`persist.rs:20-63`, `persist.rs:92-130`).
- The terminal side panel counts and brands every pane as a terminal (`terminals_panel.rs:388-418`).

Making all of those kind-aware is possible, but it is not a minimum change and creates regression risk across PTY cleanup, AI monitoring, layout restore, keyboard navigation, and mobile relay. A workbench wrapper gives the requested UX while keeping those invariants intact.

### Risks and required checks

1. **Keyboard routing:** modal `FileViewer` currently benefits from overlay guards. Once embedded, workspace-wide terminal actions can still fire while the editor is active because they are bound in the `Workspace` context (`hotkeys.rs:38-50`, `hotkeys.rs:461-483`) and most handlers only call `yields_to_overlay` (`main.rs:785-819`, `main.rs:996-1029`). Add an explicit “document page active/editor focused” guard or deeper editor key-context bindings.
2. **Unsaved state:** do not retain current singleton `navigate` behavior; it replaces a dirty document without prompting.
3. **Remote stale writes:** bind a tab to project id + connection id + connection fingerprint/generation. Revalidate before every read/save.
4. **Remote symlinks:** current local reads follow only after a second root-containment check. Remote read/save must define equivalent behavior; safest MVP is to reject symlink leaves.
5. **Remote external edits:** local watcher semantics cannot be claimed for remote documents without a polling/stat mechanism.
6. **Markdown resources:** rendered remote Markdown text works, but relative remote images need an SSH-aware resource path.
7. **Large/binary files:** retain the existing 1 MiB and UTF-8/binary behavior for parity between local and remote.
8. **Entity lifetime:** hiding terminal content must not drop `TerminalArea`; keep its entity owned by `Workspace`/`WorkbenchArea` so PTYs and UI state continue running in the background.
9. **Project removal:** close/drop that project's document entities and their watchers/tasks before removing project state.
10. **No persistence in MVP:** persisting editor tabs would require a new schema and careful remote credential/path handling. Keep runtime-only unless the requirement explicitly includes restart restoration.

## External references

None required. The relevant editor and tab contracts are already implemented or documented in the repository. Dependency versions observed: GPUI `^0.2.2` via the workspace and `gpui-component = 0.5.1`; `mt-app` enables the `tree-sitter-languages` feature (`Cargo.toml:33-43`, `crates/mt-app/Cargo.toml:12-23`).

## Related specs

- `.trellis/spec/mt-app/backend/index.md`
- `.trellis/spec/mt-terminal/backend/index.md`
- `.trellis/spec/mt-layout/backend/index.md`
- `.trellis/spec/mt-project/backend/index.md`
- `.trellis/spec/mt-ssh/backend/index.md`
- `.trellis/spec/mt-ui/backend/index.md`

These spec indexes are currently placeholder templates and contain no additional executable project conventions beyond the source-level contracts cited above.

## Caveats / Not Found

- No existing multi-document/tab manager was found.
- No embeddable file editor separate from the modal host was found; `FileViewer` combines state, content, toolbar, singleton, and overlay lifecycle.
- No remote full-file bounded editor read API or in-memory atomic text-save API was found.
- No remote file watcher or remote mtime polling path was found.
- The current task PRD/design concern PR-review fixes and predates this expanded editor request; the main session should update planning artifacts or create a scoped child task before implementation if Trellis workflow is enforced.
