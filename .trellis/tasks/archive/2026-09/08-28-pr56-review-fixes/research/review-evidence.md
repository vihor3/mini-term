# PR #56 Review Evidence

## Source

- PR: `dreamlonglll/mini-term#56`
- Maintainer comment: `issuecomment-5449128912`, 2026-08-28.
- Feature worktree: `/tmp/mini-term-file-workbench`
- Head at planning start: `49e9609fb64ac838458e1aa888587d044b68111d`

## Required fixes

1. `sanitize_session_markdown` applies `sanitize_untrusted_html_urls` to the complete Markdown source. The scanner does not understand Markdown structure and therefore rewrites `href` / `src` text inside fenced HTML or JSX examples.
2. `markdown_safe_plain_label` falls back to `link` / `image` whenever the visible label contains punctuation, losing useful text such as `main.rs` or `截图(1).png`.

## Requested adjustments

3. Remote documents must not automatically fetch HTTP(S) images. The maintainer recommends a clickable placeholder for deliberate loading.
4. Global search should preserve search state, restore single-click preview / double-click external editor, and show feedback when invoked for an SSH project.
5. Download identity mismatches must not silently return.
6. Markdown images must be constrained by the workbench content column, not the full window viewport.
7. Dirty documents that block stale-worktree project removal must produce visible feedback.
8. The close-confirmation preview limit of five items is acceptable if documented as an intentional readability decision.

## Existing task history

- `.trellis/tasks/archive/2026-08/08-27-pr-review-fixes/prd.md` records the prior accepted behavior that global-search single click opens the built-in preview and double click invokes the configured external editor.
- That archived task also records the standing constraints that `.trellis` must not enter product commits and Rust compilation/tests run only in GitHub Actions.
- `trellis mem` searches for the three review topics returned no matching past dialogue; repository history and archived task artifacts are therefore the authoritative evidence.

## Scope decision

Keep one task rather than parent/child tasks. Four review items converge on `file_viewer.rs`, and the remaining items share i18n/generated-dictionary and final CI gates. Splitting would create overlapping file ownership without independent delivery value.
