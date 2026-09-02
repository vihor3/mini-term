# Research: Existing file editor, syntax highlighting, and Markdown capabilities

- Query: Identify existing dependencies and components that can support a terminal-area file page with viewing/editing, syntax highlighting, indentation assistance, and rendered Markdown; recommend the smallest implementation.
- Scope: mixed (repository source, locked dependency metadata, upstream crate documentation)
- Date: 2026-08-27

## Findings

### Executive conclusion

Items 2–4 of the new request are already implemented for **local files**, but the implementation is hosted in a modal dialog rather than in the terminal area:

- A file-tree click calls the existing viewer at `crates/mt-app/src/file_tree.rs:754-775`.
- The viewer builds `gpui_component::input::InputState::code_editor`, with language selection, line numbers, soft wrapping, syntax highlighting, search, and the component's automatic indentation at `crates/mt-app/src/file_viewer.rs:17-23` and `crates/mt-app/src/file_viewer.rs:1548-1578`.
- Markdown files start in rendered preview mode because `preview` is initialized to `true` at `crates/mt-app/src/file_viewer.rs:1411-1437`, and the content branch selects `render_markdown` at `crates/mt-app/src/file_viewer.rs:2402-2408`.
- Markdown is rendered through `TextView::markdown`, with selectable text, custom typography, tables, images, and a cached block split at `crates/mt-app/src/file_viewer.rs:2182-2218` and `crates/mt-app/src/file_viewer.rs:2261-2321`.

Therefore the smallest implementation should **reuse and re-host `FileViewer`**, not add another editor, parser, or syntax highlighter.

### Existing dependency stack

1. `gpui-component = 0.5.1` is already the project component library (`Cargo.toml:40-43`, `Cargo.lock:2756-2779`). `mt-app` already enables its `tree-sitter-languages` feature specifically for the file viewer at `crates/mt-app/Cargo.toml:12-23`.
2. The locked component dependency already brings:
   - `ropey` for editor text storage (`Cargo.lock:2777-2779`, version `2.0.0-beta.1` at `Cargo.lock:6002-6008`);
   - `markdown` and `html5ever` for rich-text parsing (`Cargo.lock:2769-2773`, versions at `Cargo.lock:3180-3191` and `Cargo.lock:4030-4036`);
   - `tree-sitter 0.25.10` and approximately 30 language grammar crates (`Cargo.lock:2788-2817`, `Cargo.lock:7879-7890`).
3. The application already imports the editor and rich-text APIs directly at `crates/mt-app/src/file_viewer.rs:61-76`.
4. No direct `syntect`, `pulldown-cmark`, `comrak`, Monaco, CodeMirror, or second rope/editor dependency exists or is needed.

Recommendation: do not add dependencies for this feature. Adding a second Markdown parser or editor would duplicate already-shipped parsing, theme, input, IME, search, and text-buffer behavior.

### Syntax highlighting and indentation

- Filename-to-language mapping already covers Rust, TypeScript/TSX, JavaScript, JSON, Python, Go, Ruby, Java, C/C++, CSS, HTML, shell, TOML, YAML, Markdown, SQL, Swift, Zig, Elixir, Scala, protobuf, GraphQL, diff, CMake, EJS, and ERB at `crates/mt-app/src/file_viewer.rs:191-244`.
- Unknown extensions deliberately fall back to plain text at `crates/mt-app/src/file_viewer.rs:194-195` and `crates/mt-app/src/file_viewer.rs:243-244`.
- `InputState::code_editor(lang)` is the existing code-editor mode; the module records that it supplies syntax highlighting, automatic indentation, line numbers, indentation guides, and Ctrl+F search at `crates/mt-app/src/file_viewer.rs:17-23`.
- The editor is instantiated with line numbers and wrapping rules at `crates/mt-app/src/file_viewer.rs:1548-1558`. Markdown and text wrap; code does not (`crates/mt-app/src/file_viewer.rs:105-108`).
- One important preservation rule is CRLF round-tripping. The current viewer normalizes input to LF and restores the original dominant line ending on save (`crates/mt-app/src/file_viewer.rs:25-35`, `crates/mt-app/src/file_viewer.rs:141-188`, `crates/mt-app/src/file_viewer.rs:1654-1664`). Re-hosting must not bypass this logic.

### Theme integration

- The application already installs a global `HighlightTheme` shared by the editor and Markdown fenced code blocks at `crates/mt-app/src/theme.rs:181-203`.
- The theme maps the application palette into syntax categories and editor line-number/active-line colors at `crates/mt-app/src/theme.rs:233-308`.
- The file editor uses the same UI font preference/fallback chain as the rest of the application at `crates/mt-app/src/file_viewer.rs:2410-2436`.

Recommendation: the terminal-area page should mount the same `FileViewer` entity under the existing application theme. It should not create a separate editor theme or hard-code language colors.

### Markdown preview behavior

- Markdown extensions are recognized at `crates/mt-app/src/file_viewer.rs:85-88`.
- Preview mode is the default for Markdown and HTML (`crates/mt-app/src/file_viewer.rs:1425-1427`), with a preview/source segmented control at `crates/mt-app/src/file_viewer.rs:1884-1924`.
- Preview uses current unsaved draft content when switching from source to preview (`crates/mt-app/src/file_viewer.rs:1906-1913`), which gives the Typora-like edit/preview loop without writing to disk first.
- Markdown typography is already customized to the application rather than using raw component defaults (`crates/mt-app/src/file_viewer.rs:2221-2253`). Tables and standalone images have project-specific rendering because the component's stock table/image behavior was insufficient (`crates/mt-app/src/file_viewer.rs:310-325`, `crates/mt-app/src/file_viewer.rs:1046-1060`).
- The application registers a preview HTTP client so Markdown/HTML images can load from rewritten local `file://` URLs and network URLs (`crates/mt-app/src/main.rs:2005-2012`).

Existing limitations that should remain explicit:

- Markdown link handling is controlled internally by `gpui-component`, so local-document navigation, anchor scrolling, and external-link confirmation cannot currently be intercepted (`crates/mt-app/src/file_viewer.rs:37-42`).
- HTML preview is intentionally a simplified rich-text view without CSS or JavaScript, not a browser (`crates/mt-app/src/file_viewer.rs:43-47`, `crates/mt-app/src/file_viewer.rs:2323-2368`). This is not a blocker for the Markdown request.

### Current host mismatch: modal versus terminal-area page

The viewer is presently a singleton modal:

- `file_viewer::open` tracks a singleton and opens a guarded 90vw × 80vh dialog at `crates/mt-app/src/file_viewer.rs:1260-1329`.
- Close/Esc behavior directly calls the modal close path at `crates/mt-app/src/file_viewer.rs:1332-1335` and `crates/mt-app/src/file_viewer.rs:1690-1711`.
- Its top-level `Render` body is otherwise self-contained (`crates/mt-app/src/file_viewer.rs:2451-2488`), so most rendering and editor state can be reused unchanged.

The terminal layout model should not be generalized into a file-pane enum for the MVP:

- `PaneState` is explicitly a terminal tab and stores shell, PTY, AI-session, cwd, and status data (`crates/mt-app/src/tree.rs:90-119`).
- `SplitNode::Leaf` stores only `Vec<PaneState>` (`crates/mt-app/src/tree.rs:240-255`).
- Layout persistence serializes every leaf item as `SavedPane` shell/cwd/AI state (`crates/mt-app/src/persist.rs:20-49`) and restores it by resolving a shell and constructing a terminal pane (`crates/mt-app/src/persist.rs:92-130`).
- `TerminalArea::render_leaf` resolves an active pane to `pty_id` and then to `Entity<TerminalPane>` (`crates/mt-app/src/terminal_area.rs:2287-2329`). The store similarly indexes only terminal entities by PTY id (`crates/mt-app/src/store/mod.rs:336-343`).

Changing `PaneState` into a terminal/file union would cascade through layout serialization, PTY lifecycle, AI status, mobile relay snapshots, tab menus, drag/drop, split operations, focus handling, and reconnect behavior. That is substantially larger and riskier than the requested UI feature.

### Recommended smallest host design

Use an **ephemeral document-page layer above the existing terminal split tree**:

1. Keep `PaneState`, `SplitNode`, and saved terminal layout unchanged.
2. Add runtime-only document state (open documents + active document) owned by `AppStore` or a dedicated document controller. The file tree and global search need a shared entry point, so terminal-area-local state alone is insufficient.
3. Reuse one `Entity<FileViewer>` per open file tab, or one entity plus navigation if only one file page is required. For the wording “单独开一页”, a small tab collection is preferable: clicking an already-open path activates it; clicking a new path creates a page; closing removes only that page.
4. In `TerminalArea::render`, when a document page is active, render a document tab strip plus the `FileViewer` entity in the terminal-area body. Keep terminal entities alive in `AppStore`; switching away should not kill PTYs or alter the saved split tree.
5. Refactor modal-specific concerns out of `FileViewer`:
   - replace direct global `CURRENT`/`close_guarded` calls with a host mode or close callback;
   - keep the existing dirty-state confirmation, save, reload, CRLF, editor, Markdown, and watcher code unchanged;
   - keep the existing modal entry point only if global-search compatibility still needs it, otherwise route both file tree and search to the document controller.
6. Runtime document tabs should initially be non-persistent. Persisting unsaved/open editor tabs introduces crash recovery and stale-path semantics that are not required by the request.

This design produces the requested “file opens as its own page in the terminal side” behavior without changing terminal persistence or PTY semantics.

### Local and remote scope

Current viewer behavior is local-only. The file tree explicitly shows “remote preview unsupported” for remote projects at `crates/mt-app/src/file_tree.rs:762-770`. The viewer reads and writes through local project-root-validated functions (`crates/mt-app/src/file_viewer.rs:1507-1529`, `crates/mt-app/src/file_viewer.rs:1654-1685`). Local file IO enforces an in-project path boundary, UTF-8/binary detection, and a 1 MiB cap at `crates/mt-project/src/fs.rs:349-399`.

If the user expects remote file viewing/editing as part of this request, add a source/backend abstraction instead of teaching the UI to special-case SSH:

- `DocumentSource::Local { project_root, path }`
- `DocumentSource::Remote { connection, project_root, path }`

Existing SFTP support can read bounded bytes with `read_head`/`read_from_offset` (`crates/mt-ssh/src/sftp.rs:193-237`), but it does not expose an in-memory overwrite API. It has safe staged replacement primitives internally and uses them for local-file uploads (`crates/mt-ssh/src/sftp.rs:403-458`, `crates/mt-ssh/src/sftp.rs:460-545`). A robust remote editor should add a bounded `write_file_contents` primitive that writes bytes to a unique sibling staging file and promotes it through the existing replacement/rollback path. It should not save by creating an ad-hoc local temp file and calling upload.

Remote documents cannot use `FsWatcher`; remote external-change handling must be defined separately (manual reload, stat polling, or optimistic version check before save). The smallest safe MVP is read/edit/save with a pre-save metadata/content check and explicit reload, not a fake watcher.

Critical scope caveat: the new request does not explicitly say whether items 2–4 must work for remote files, but the surrounding feature is remote file management. Implementation planning should treat remote viewing/editing as an explicit acceptance decision rather than silently retaining the current “unsupported” alert.

### Performance and lifecycle risks

- Enabling `tree-sitter-languages` already adds many C-backed grammar build scripts; that build cost is already accepted and locked (`crates/mt-app/Cargo.toml:14-23`). Reusing it adds no new dependency cost.
- Markdown parsing/highlighting occurs synchronously on the UI thread on first render; another use of the same component documents that behavior at `crates/mt-app/src/session_panel.rs:71-77`. The viewer caches its own block preprocessing (`crates/mt-app/src/file_viewer.rs:373-385`, `crates/mt-app/src/file_viewer.rs:2182-2218`), but `TextView` construction remains non-virtualized for the document (`crates/mt-app/src/file_viewer.rs:2266-2319`). Pathological large Markdown/table documents can still stall the UI.
- The local IO layer caps view/edit files at 1 MiB (`crates/mt-project/src/fs.rs:357-399`). Keep that cap for the MVP and apply the same cap to remote reads/writes. Do not remove it while moving the view into the terminal area.
- Each `FileViewer` owns an `FsWatcher`, async task, and editor subscription (`crates/mt-app/src/file_viewer.rs:1376-1381`). Multiple document tabs therefore require deterministic drop/removal on close; the watcher cleanup currently occurs in `Drop` (`crates/mt-app/src/file_viewer.rs:2491-2496`).
- Current singleton navigation intentionally replaces a file without asking about unsaved changes (`crates/mt-app/src/file_viewer.rs:1442-1458`). Multi-tab hosting should not reuse that behavior for closing/replacing tabs; closing a dirty tab must keep the existing confirmation path.

### License and supply-chain notes

- The workspace declares MIT (`Cargo.toml:19-25`, root `LICENSE`).
- `gpui-component` 0.5.1 is already shipped and locked. Its upstream repository declares Apache-2.0, which is permissive and compatible with this use. The editor/Markdown implementation therefore introduces no new direct license category.
- `ropey`, `markdown`, `html5ever`, Tree-sitter, and the language grammars are already transitive entries in `Cargo.lock`; reusing them creates no new dependency review surface.
- Do not switch to an unpinned git revision of `gpui-component` for this feature. The repository deliberately uses crates.io releases (`Cargo.toml:27-43`), and upstream dependency/license composition may change independently of the locked 0.5.1 release.
- A future dependency upgrade should run the project's normal license/advisory audit in GitHub Actions. This research did not run Cargo locally, in accordance with the user constraint.

### Validation shape under the GitHub-Actions-only constraint

Implementation can be split so important behavior is testable by pure Rust tests in CI:

- document tab deduplication and active-tab/close selection;
- host-close policy for clean/dirty documents;
- local versus remote backend routing;
- filename-to-language and Markdown-default-preview behavior (many tests already exist in `file_viewer.rs`);
- remote size/binary checks and staged-save rollback;
- no mutation of `SplitNode`/saved terminal layout when opening or closing a file page.

Local verification should remain static (`git diff --check`, searches, i18n generation if keys change). Rust formatting, compilation, Clippy, and tests should run only through GitHub Actions.

## Files Found

- `crates/mt-app/src/file_viewer.rs` — complete local file viewer/editor, Markdown renderer, save/dirty/watcher logic, language mapping.
- `crates/mt-app/src/file_tree.rs` — local click integration and current remote-preview rejection.
- `crates/mt-app/src/search_modal.rs` — second viewer entry point with search-result line highlighting.
- `crates/mt-app/src/theme.rs` — shared syntax theme for editor and Markdown code blocks.
- `crates/mt-app/src/terminal_area.rs` — terminal-only rendering assumptions and candidate host location.
- `crates/mt-app/src/tree.rs` — terminal pane/split data model.
- `crates/mt-app/src/persist.rs` — terminal-only saved-layout contract.
- `crates/mt-app/src/store/mod.rs` and `crates/mt-app/src/store/panes.rs` — PTY-indexed terminal entity ownership.
- `crates/mt-project/src/fs.rs` — safe local read/write, UTF-8/binary detection, and 1 MiB limit.
- `crates/mt-ssh/src/sftp.rs` — bounded remote reads and staged remote replacement primitives.
- `crates/mt-app/Cargo.toml`, `Cargo.toml`, `Cargo.lock` — dependency features, versions, and lock state.

## External References

- gpui-component upstream repository and license: `https://github.com/longbridge/gpui-component`
- gpui-component crate documentation: `https://docs.rs/gpui-component/0.5.1/gpui_component/`
- gpui-component code editor example: `https://longbridge.github.io/gpui-component/story/input-code-editor.html`
- gpui-component Markdown example: `https://longbridge.github.io/gpui-component/story/text-markdown.html`
- Ropey repository/license: `https://github.com/cessen/ropey`
- markdown-rs documentation: `https://docs.rs/markdown/1.0.0/markdown/`

## Related Specs

- `.trellis/spec/mt-app/backend/quality-guidelines.md` — background operation ownership, local/remote capability separation, GitHub Actions-only Rust verification.
- `.trellis/spec/guides/index.md` — cross-layer and code-reuse checks; this feature should reuse the existing viewer and avoid widening terminal persistence.
- Other inspected mt-app/mt-ui directory and error-handling specs are placeholders and add no additional project-specific constraints.

## Caveats / Not Found

- No existing document-tab controller or non-terminal pane kind was found.
- No existing remote full-file editor API or remote filesystem watcher was found.
- No need for a new syntax highlighter, Markdown parser, or editor widget was found.
- Whether remote files must support viewing/editing is not explicit in the latest wording and must be resolved in the implementation acceptance criteria.
- No local Cargo command was run; dependency behavior was established from source usage, manifests, lockfile, and upstream documentation.
