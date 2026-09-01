# PR #56 fourth review evidence

Source: maintainer comment `5461792633`, created 2026-08-29T10:26:31Z.

## Blocking reproduction

Single-pass AST replacement can change CommonMark block structure. HTML block types 1–5
become escaped paragraphs; a following four-space-indented line can then become a lazy
paragraph continuation during the renderer's second GFM parse. Markdown `Image` or `Link`
nodes that were originally inside an indented `Code` node can therefore become active.

Representative payload:

```markdown
<!-- c -->
    ![beacon](https://attacker.example/img.png)
```

Variants include `<pre></pre>`, `<!DOCTYPE html>`, `<?php ?>`, `<![CDATA[x]]>`, and
`<style></style>`, followed by HTTP(S) images, `file://` images/links, or raw HTML.

## Required contract

- Reparse and sanitize until no replacements remain.
- Bound the loop; parser failure or non-convergence renders the original source as one
  indented code block.
- Tests reparse the production output and assert the replacement collector is empty.
- Test-only legacy scanner wrappers must not be present in production namespaces.
- A successful remote save clears a stale refresh warning; failures preserve it.

## Non-blocking records

- Remote standalone HTML is intentionally source-only; call this out in the PR response.
- Session `<br>` rendering may be considered separately and is not required by this review.
- The deterministic SFTP backup race is harmless but may emit a rare cleanup warning.
- Search single-click focus handoff is code-verified but still requires real GUI validation.
