# Markdown, HTML, and Remote Image Research

## Current data flow

Remote Markdown blocks currently follow:

```text
split_md_blocks
  -> MdSegment::Text / Table cell
  -> sanitize_remote_markdown
  -> sanitize_remote_html_urls (whole string scan)
  -> TextView::markdown
```

AI session Markdown currently follows:

```text
sanitize_markdown_with_policy(ImagePolicy::Disabled)
  -> sanitize_untrusted_html_urls(whole string, allow_external=false)
  -> TextView::markdown
```

The second string-wide pass is the regression source. Markdown AST `Code`, `InlineCode`, and normal `Text` nodes are safe, but the later HTML attribute scanner cannot distinguish them from a real `mdast::Html` node.

## AST-scoped HTML sanitization

`collect_untrusted_markdown_replacements` already owns byte-range replacement for `Link`, `Image`, `ImageReference`, and `Definition`. Add `MarkdownNode::Html` to this same collector:

1. sanitize only `html.value`;
2. replace the exact AST position when the value changes;
3. do not traverse or scan fenced code, inline code, or ordinary text;
4. remove the whole-Markdown `sanitize_untrusted_html_urls` calls.

Raw `.html` file preview is not Markdown and must continue to use the standalone HTML scanner.

The HTML URL policy needs independent link/resource switches:

- local trusted HTML: existing rewrite behavior;
- remote Markdown/raw HTML: allow deliberate HTTP(S)/mail/tel/fragment links, block automatic external `src` / `poster` resources;
- AI session Markdown: block raw-HTML external links and all raw-HTML resources; normal Markdown links remain governed by the AST link policy.

## Visible label preservation

`markdown_safe_plain_label` already escapes every ASCII punctuation character with `\`. The preceding allowlist/fallback branch is therefore redundant and destructive. Keep fallback only for empty/whitespace-only labels, then escape punctuation for every non-empty label.

Regression cases:

- `[main.rs](src/main.rs)` -> visible `main\.rs` plain text after unsafe destination removal;
- `![截图(1).png](./a.png)` -> visible `截图\(1\)\.png` plain text;
- decoded entity labels must remain plain after reparsing and must not create a new active Markdown node.

## Remote image loading

There are two rendering paths:

- pure top-level image paragraphs are extracted as `MdBlock::Images` and rendered by `FileViewer`;
- inline/container/reference images remain in `MdBlock::Text` and are rendered by `TextView::markdown`.

To guarantee that opening an untrusted remote document issues no image request:

1. remote Text/Table sanitization must disable all image nodes, reducing inline/container images to escaped alt text;
2. pure image blocks may remain custom-rendered, but `FileViewer` must show a placeholder until that document explicitly approves the URL;
3. approval is per `FileViewer` document and URL, held in a `HashSet<String>`; clicking inserts the URL and re-renders;
4. local Markdown continues to auto-load local and HTTP(S) images;
5. remote raw HTML `src` / `poster` must be neutralized because it has no click-to-load surface.

The existing `PreviewHttpClient` remains the byte/timeout/size boundary after a deliberate load.

## Tests needed

- fenced HTML and JSX-like source retain exact `href` / `src` text;
- real `mdast::Html` nodes still sanitize unsafe attributes;
- punctuation-rich link/image labels survive downgrade and reparse safely;
- remote inline images do not survive as active image nodes;
- remote pure image blocks remain placeholders before approval and only call the URI asset path after approval (factor the decision into a pure helper where needed);
- local image behavior is unchanged.
