# Bug Analysis: parser-boundary sanitizer and remote editor state gaps

### 1. Root Cause Category

- **Category**: E (Implicit Assumption), B (Cross-Layer Contract), and D (Test
  Coverage Gap).
- **Specific Cause**: the sanitizer assumed HTML tokenizer state could be
  reconstructed from tag names without the html5ever tree builder's insertion
  modes. After raw HTML was made inert with only `<>&` escaping, it also assumed
  the replacement would remain plain text through the next GFM parse; embedded
  Markdown image/link syntax disproved that assumption. Remote refresh and
  search focus similarly lacked an explicit contract between async completion,
  overlay lifecycle, and the already-loaded editor. SFTP read/replacement paths
  classified the same deterministic backup state differently.

### 2. Why Fixes Failed

1. **AST-scoped scanner fix**: correctly stopped fenced/inline code corruption,
   but still delegated real raw HTML to a lexical scanner that could not model
   tree-builder insertion modes.
2. **Raw-text and foreign-content patches**: covered observed payload families,
   but expanded a second parser instead of removing the parser boundary.
3. **Escape `<>&`**: prevented HTML recreation but left Markdown punctuation
   active during the renderer's second GFM parse.
4. **Initial UI guards**: preserved data in memory, but presentation/focus
   contracts were incomplete, so error precedence and dialog focus-back hid or
   disabled the correct editor.

### 3. Prevention Mechanisms

| Priority | Mechanism | Specific Action | Status |
|----------|-----------|-----------------|--------|
| P0 | Architecture | Never render untrusted raw HTML; remote HTML is source-only and Markdown raw HTML becomes inert visible text | DONE |
| P0 | Test coverage | Reparse transformed Markdown and assert no HTML/image/resource node, including Markdown syntax embedded inside raw HTML | DONE |
| P0 | State matrix | Classify target + deterministic backup together and verify cleanup by post-operation observation | DONE |
| P1 | Async contract | Separate fatal initial-load error from refresh warning and explicitly hand focus back after overlay close | DONE |
| P1 | Code review | Treat every sanitizer output parser as another trust boundary; test the consumer AST, not only transformed bytes | DONE |

### 4. Systematic Expansion

- **Similar Issues**: any transformed text later consumed by Markdown, HTML,
  shell, URL, or template parsers can reactivate syntax that looked inert in
  the first representation.
- **Design Improvement**: prefer making dangerous representations unreachable
  over reproducing third-party parser recovery rules.
- **Process Improvement**: security regression tests must assert the final
  consumer structure and cover malformed input plus cross-syntax payloads.
- **Knowledge Gap**: tokenizer state and DOM/tree output are not equivalent;
  html5ever insertion modes and Markdown's second parse are part of the actual
  security model.

### 5. Knowledge Capture

- [x] Update `mt-app/backend/file-workbench-contract.md` with inert raw HTML,
  source-only remote HTML, refresh warning, and focus handoff contracts.
- [x] Update `mt-ssh/backend/remote-document-io-contract.md` with stale backup
  cleanup and verified-absence rules.
- [x] Add reviewed html5ever payloads and embedded Markdown resource syntax to
  product regression tests.
- [x] Keep all Trellis updates local and uncommitted per user instruction.

Template sync and Trellis commits are intentionally skipped because the user
explicitly requires `.trellis` not to be submitted with this PR.
