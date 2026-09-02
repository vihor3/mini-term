# File Workbench Contract

## Scenario: Local and remote documents in the main workbench

### 1. Scope / Trigger

Use this contract whenever a file is opened, focused, searched, refreshed, or
saved from the main content area. It applies to local and SSH projects and to
every deferred or asynchronous completion that can change the active page.

### 2. Signatures

```rust
pub enum DocumentSource {
    Local {
        project_id: String,
        project_root: PathBuf,
        path: PathBuf,
    },
    Remote {
        project_id: String,
        connection: SshConnection,
        project_root: String,
        path: PathBuf,
    },
}

pub fn open_active_file(
    store: Entity<AppStore>,
    path: PathBuf,
    highlight_line: Option<u32>,
    window: &mut Window,
    cx: &mut App,
);

pub fn close_document_source(
    expected_worktree_id: WorktreeId,
    source: DocumentSource,
    window: &mut Window,
    cx: &mut App,
);
pub fn is_document_active(
    expected_worktree_id: &WorktreeId,
    source: &DocumentSource,
    cx: &App,
) -> bool;
pub fn reactivate_active_document(
    expected_project_id: &str,
    expected_worktree_id: &WorktreeId,
    window: &mut Window,
    cx: &mut App,
);
pub fn reactivate_active_page(
    expected_project_id: &str,
    expected_worktree_id: &WorktreeId,
    window: &mut Window,
    cx: &mut App,
);

enum DocumentTabState {
    Preview,
    Permanent,
}

fn sanitize_untrusted_markdown(source: &str) -> String;
fn apply_markdown_replacements(
    source: &str,
    replacements: Vec<MarkdownReplacement>,
) -> String;
```

Remote editor service boundary:

```rust
pub fn read_file_content(
    conn: &SshConnection,
    project_root: &str,
    path: &str,
) -> Result<RemoteFileReadResult, String>;

pub fn save_file_content(
    conn: &SshConnection,
    project_root: &str,
    path: &str,
    content: &str,
    expected: &RemoteFileBaseline,
    force: bool,
) -> Result<RemoteFileSaveResult, String>;
```

### 3. Contracts

- A document identity is `WorktreeId` + backend identity + normalized path.
  The source project ID remains beside the tab for I/O/binding validation. The
  remote backend includes connection ID and fingerprint. Local paths are
  case-folded only on Windows; remote paths stay case-sensitive.
- Tabs are runtime state scoped to a worktree. Switching to a document hides the
  terminal view but must not destroy terminal entities or PTY sessions.
- Each worktree bucket remembers its active workbench page independently. After
  `AppStore::active_project_id` changes,
  `reactivate_active_page(expected_project_id, expected_worktree_id, ...)` must
  re-check both IDs and then focus the remembered terminal pane or document. It
  must not reuse the page or focus target from another worktree.
- Each worktree bucket has at most one replaceable `Preview` tab. A new
  file replaces an existing clean preview at the same tab index. A dirty
  preview is promoted to `Permanent` before the new preview is appended. Editing
  a preview or double-clicking its tab also promotes it. Double-clicking a file
  tree row remains the rename gesture and must not promote a workbench tab.
- Deferred close, focus, search, and reload callbacks capture a concrete
  `DocumentSource` and originating `WorktreeId`. Before acting, they re-check
  project binding, active worktree/document, active dialog, and overlay
  ownership. A late callback may update its own document data but may not close
  or focus a newly active tab.
- Remote connection edits invalidate an old tab identity. The tab may display
  its existing draft, but new saves are refused and late results from the old
  connection may not mutate current UI state.
- Editable remote text has an opaque `RemoteFileBaseline`. Binary and oversized
  reads have no baseline and no reusable hidden editor.
- Remote Markdown and session history are untrusted rich text. Sanitization is
  scoped to Markdown AST nodes and source byte ranges; never scan the complete
  Markdown string for HTML attributes because fenced and inline code must stay
  byte-for-byte unchanged.
- Every untrusted `mdast::Html` node is replaced with visible inert source before
  `TextView::markdown`. Escape every ASCII punctuation character, not only
  `<`, `>`, and `&`: raw HTML attribute text may itself contain Markdown image
  or link syntax that would become active during the renderer's second GFM
  parse. A single replacement pass is not sufficient because escaping an HTML
  block can turn following indented code into a lazy paragraph continuation.
  Reparse, collect, and replace until one GFM parse yields no replacements. The
  loop is bounded to four passes; parser failure or non-convergence renders the
  original source as one visible indented code block. The string returned to
  `TextView::markdown` must already be a fixed point under the same collector.
- Untrusted Markdown that enters `TextView` contains no active image or image
  reference nodes. A pure top-level remote HTTP(S) image is rendered by the
  custom image path as a non-networking placeholder and reaches the shared HTTP
  loader only after per-document, per-URL user approval. Local Markdown keeps
  automatic image loading.
- Standalone remote `.html` files are source-only and never reach
  `TextView::html`. Trusted local HTML may use the simplified rich preview and
  local URL rewriting. A lexical HTML scanner is not an untrusted-content
  security boundary because tokenizer state also depends on html5ever tree-
  builder insertion modes.
- A failed remote re-activation refresh is fatal only when the tab has no usable
  result or editor. If content is already loaded, preserve the result, editor,
  draft, selection, and focus capability; display a separate refresh warning.
  A successful refresh clears only that warning and must not erase an existing
  committed-save cleanup warning. A later remote save returning `Saved` also
  clears the refresh warning because it proves the connection recovered;
  `ExternalChange` and save errors preserve it.
- Remote image files go directly to the download/fallback surface; opening them
  must not perform an unnecessary full SFTP read.
- Global search is local-filesystem-only until a remote search backend exists.
  Results are bound to the producing local project ID, root, and `WorktreeId`;
  project changes or rebinds cancel and clear the search rather than
  reinterpreting old relative paths.
- The search entity is process-global and lazily created. Closing the overlay
  preserves query, results, count, and the running search task. A single result
  click opens the workbench file and schedules overlay close; a second click
  within the platform double-click interval cancels that close and opens the
  configured external editor. Windows uses `GetDoubleClickTime() + 50ms`; other
  platforms use the project's 500ms grace interval. After the delayed single-
  click close succeeds, reactivate the current document only for the captured
  project/worktree so focus cannot fall back to a hidden terminal pane.
  Project/worktree/search generation checks reject stale close tasks.
- Remote downloads keep project ID, root, connection ID, and fingerprint
  checks. Any mismatch fails closed with a localized toast; never substitute
  the currently active connection. A dirty document that prevents automatic
  project/worktree removal also retains the project and emits a localized,
  deduplicated explanation.

### 4. Validation & Error Matrix

| Condition | Required behavior |
|-----------|-------------------|
| No active project | Do not open or mutate a workbench document |
| Remote project has no current connection | Show the existing connection error; do not create a tab |
| Late load belongs to a background tab/project | Update only that document; do not take focus |
| Active dialog or blocking overlay exists | Deferred focus is skipped |
| Remote content is binary or over the editor limit | Remove stale editor state and show download/fallback |
| Current remote bytes differ from baseline | Return/display `ExternalChange`; do not overwrite |
| User explicitly force-saves | Skip byte equality only; keep identity, type, root, and size checks |
| Connection fingerprint no longer matches | Mark source invalid and refuse save |
| Search is opened for an SSH project | Do not start a local filesystem search; show an informational toast |
| Download project/root/connection identity differs | Do not start transfer or fall back; show the context-changed error toast |
| Automatic project removal finds dirty documents | Keep the project and document; show the removal-blocked toast without stacking duplicates |
| Unapproved remote Markdown image | Render a clickable placeholder without calling `window.use_asset` |
| Untrusted raw HTML contains malformed select/template/raw-text recovery | Render escaped visible source; no HTML/image node reaches the rich renderer |
| Raw HTML text contains `![x](https://...)` or `[x](file://...)` | Escape Markdown punctuation before the second parse; do not recreate an active image/link |
| Escaped HTML changes following indented code into active Markdown | Continue parse/replace passes until the collector is empty; never return an unverified transformed string |
| Markdown parse fails or four passes do not reach a fixed point | Render the original input as one indented code block |
| Standalone remote `.html` is opened | Show source/editor mode only; never call `TextView::html` |
| Refresh fails after remote content loaded | Keep editor/result/draft visible and set the refresh warning; do not select the fatal error branch |
| Remote save returns `Saved` after a refresh failure | Clear `refresh_warning`; preserve any independent save-cleanup warning |
| Remote save conflicts or fails after a refresh failure | Preserve `refresh_warning` and report the save outcome independently |
| Search single-click delay closes the overlay | Reactivate only the captured project/worktree document scope |
| Worktree switch completes for a different project generation | Do not focus either the old worktree page or the stale requested worktree |
| Target worktree last showed a terminal | Restore that worktree's active pane and terminal page |
| Target worktree last showed a document | Restore that worktree's remembered document and call its activation path |
| New file opens while a clean preview exists | Replace that preview in place and keep exactly one replaceable preview |
| New file opens while the preview is dirty | Promote the dirty tab, append the new preview, and preserve the draft |
| User edits or double-clicks a preview tab | Promote it to permanent before a later file open |
| User double-clicks a file tree row | Open the shared rename prompt; do not pin or promote the preview tab |

### 5. Good / Base / Bad Cases

- Good: Open the same remote path twice under the same project and connection;
  reuse one tab and focus it only if it is still active when the deferred action
  runs.
- Base: Switch from a file tab to the terminal; the terminal session remains
  alive and receives focus only after page and overlay checks pass.
- Good: A remote refresh times out while the user has started typing; the draft
  remains visible and editable with a warning banner.
- Good: An HTML block followed by a four-space-indented image is cleaned, then
  reparsed and cleaned again until no active image/link/HTML node remains.
- Base: An already-safe Markdown document reaches the fixed point on its first
  parse and is returned byte-for-byte unchanged.
- Good: A successful save after a failed refresh removes the stale refresh
  warning while retaining a separate backup-cleanup warning, if present.
- Good: Worktree A can keep a terminal active while worktree B keeps a document
  active; switching between them restores the corresponding route and focus.
- Base: Repeated single-clicks on different file rows reuse one clean preview
  position inside the active worktree bucket.
- Bad: Store one process-global preview key or active page; opening or switching
  files in one worktree would overwrite the visible state of another worktree.
- Bad: Capture only "the active document" in a deferred Ctrl/Cmd+W callback;
  a quick tab switch can close the wrong file.
- Bad: Render a remote Markdown relative link through the local URL opener;
  the remote document can point the client at local files.
- Bad: Patch a hand-written HTML tokenizer until it matches a few html5ever
  examples; tree-builder insertion modes can still make skipped text active.
- Bad: Sanitize only the original AST and trust the transformed text without
  reparsing; block-structure changes can reactivate syntax that was inside code.

### 6. Tests Required

- Document-key equality for local Windows paths and case-sensitive remote paths.
- Deferred close/focus rejection after project or tab switches.
- Remote connection-fingerprint invalidation and stale async completion rejection.
- Binary/oversized transition clears any prior editor entity and baseline.
- Markdown inline, reference, autolink, raw HTML, unmatched-backtick, and fenced
  code cases; assert local URLs never reach render output and code examples stay
  unchanged.
- Raw HTML regression payloads for `select/title`, `select/plaintext`, and
  `template/col/title`; reparse sanitized Markdown and assert no raw HTML,
  image, or image-reference node. Include raw HTML attribute text containing
  Markdown image/link syntax. Fenced and inline HTML/JSX examples stay exact.
- HTML block types 1–5 (`pre`/`style`, comment, processing instruction,
  declaration, CDATA) followed by four-space-indented HTTP(S) images,
  `file://` images/links, and raw HTML. Run both remote and session production
  sanitizers, reparse with GFM, and assert the replacement collector is empty.
- Fixed-point fallback coverage forces a pass-limit exhaustion and asserts the
  original source becomes an inert, reparsable indented code block. Safe input
  must remain byte-for-byte unchanged.
- Preview capability tests assert remote HTML is source-only while local HTML
  retains simplified preview and trusted local URL rewriting.
- Remote image consent decision and resource identity; assert no URI asset load
  before approval, one shared decoded image after approval, and unchanged local
  auto-loading behavior.
- Local search result identity across project switches; assert SSH projects do
  not call the local search engine, overlay reopen preserves state, and click
  count selects workbench preview versus external editor. Assert delayed close
  hands focus only to the same active project/worktree/document generation.
- Remote refresh failure decision tests cover no-content fatal state and loaded
  result/editor warning state; successful refresh preserves save warnings.
  Remote save state tests assert only `Saved` clears the refresh warning while
  conflict and error paths retain it.
- Download-context mismatch coverage for project, root, connection ID, and
  fingerprint; assert each path reports the same localized failure and starts
  no transfer.
- Preview insertion tests assert replacement at the same index, dirty-preview
  promotion before append, and no second replaceable preview per project bucket.
- File-tree click tests assert single-click opens a file preview, double-click
  opens rename, and directory single-click still toggles expansion.
- Worktree activation tests or an integration smoke test must assert that the
  expected project ID is revalidated and its remembered terminal/document page
  receives focus rather than the previously active worktree.
- GitHub Actions must pass changed-line rustfmt, generated i18n, workspace check,
  sidecar check, changed-line Clippy, workspace tests, and whitespace checks.

### 7. Wrong vs Correct

#### Wrong

```rust
window.defer(cx, |window, cx| {
    close_active_document(window, cx);
});
```

The callback resolves "active" after the user may have changed tabs.

#### Correct

```rust
let worktree_id = self.worktree_id.clone();
let source = self.source.clone();
window.defer(cx, move |window, cx| {
    close_document_source(worktree_id, source, window, cx);
});
```

The worktree and source identities are captured before yielding and revalidated
by the workbench before the close is applied.

For preview tabs, do not delete a dirty draft or share preview state across
worktrees:

```rust
// Wrong: replacing the first preview without checking dirty state.
tabs[preview_index] = new_tab;

// Correct: preserve the draft, then create the next replaceable preview.
if tabs[preview_index].document.read(cx).is_dirty() {
    tabs[preview_index].state = DocumentTabState::Permanent;
    tabs.push(new_tab);
} else {
    tabs[preview_index] = new_tab;
}
```

For untrusted rich text, the corresponding forbidden/correct boundary is:

```rust
// Wrong: still depends on a lexical scanner matching the renderer parser.
let rendered = sanitize_untrusted_html_urls(raw_html);

// Correct: the second Markdown parse receives inert visible source.
let rendered = escape_all_ascii_markdown_punctuation(raw_html);
```

Do not stop after applying that replacement once:

```rust
// Wrong: the replacement may change CommonMark block structure.
let rendered = apply_markdown_replacements(source, replacements);

// Correct: return only a verified fixed point; otherwise fail closed.
for _ in 0..4 {
    let ast = parse_gfm(&current)?;
    let replacements = collect_untrusted_replacements(&ast);
    if replacements.is_empty() {
        return current;
    }
    current = apply_markdown_replacements(&current, replacements);
}
return markdown_as_indented_code(source);
```
