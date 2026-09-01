# PR #56 third review — 2026-08-29

Source: maintainer comment `issuecomment-5460045675`.

## Blocking security finding

The URL scanner skips apparent raw-text elements from token names alone, while html5ever
only switches tokenizer state after the tree builder actually inserts the element.
Insertion modes such as `InSelect` and `InTemplate` may discard `title`, `plaintext`, or
other start tags and leave following `img`/`a` markup active. Reviewed payloads include:

```html
<div><select><title></select><img src="https://attacker.example/beacon.png"></title></div>
```

```html
<select><plaintext></select><img src="https://attacker.example/b2.png"><a href="file:///C:/Windows/notepad.exe">open</a>
```

```html
<template><col><title></template><img src="https://attacker.example/b3.png"></title>
```

Decision: do not use a lexical URL scanner as the security boundary. Remote/session raw
HTML becomes inert visible text; standalone remote HTML opens as source. Trusted local
HTML preview remains.

## Required fixes

1. `mt-ssh/src/sftp.rs`: a deterministic backup left after successful promotion blocks
   every later save. Target-present + backup-present is stale cleanup residue and may be
   removed before continuing; target-missing + backup-present remains ambiguous and must
   be preserved/refused.
2. `file_viewer.rs`: a refresh failure after content is loaded must not replace the
   editor with the full-page error branch or hide an in-flight draft. Show a warning and
   retain editor/content/focus.
3. `search_modal.rs`: delayed single-click overlay close restores focus to the old
   terminal. After close, re-activate/focus the current workbench document under existing
   identity checks.

## Non-blocking notes

Watcher sharing, HTML preview caching, canonicalized document identity, rename/delete tab
coordination, save-as/local-copy escape hatches, error-language consistency, and wider
state-machine tests remain recorded but are outside this review-fix iteration.
