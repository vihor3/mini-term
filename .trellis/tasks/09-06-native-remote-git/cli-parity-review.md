# Isolated CLI History Parity Review

Source review COMPLETE. The two CLI files are RELEASED for main to stage and
rerun Actions independently of the ongoing backend review. No reviewer source
edits were needed, and no public API changed.

## Findings (fixed)

- File: `crates/mt-project/src/git/cli/parse.rs:429`.
- Issue: the CLI body retained its final LF whereas the existing local
  `git.rs:593` DTO uses `commit.body()` and omits boundary whitespace. Main
  reported this as the sole mt-project failure in Actions run `33998882225`,
  Linux job `101394109884` (169 passed, one failed).
- Fix reviewed: Turing trims space, HT, LF, CR, VT and FF only at the body
  boundaries, after complete framing and strict UTF-8 checks. Empty/ASCII-only
  bodies remain None; interior formatting and Unicode whitespace are preserved.
  Limits, parent cursors, subject/author/timestamp parsing are unchanged.
- Source evidence: Cargo.lock pins `libgit2-sys 0.17.0+1.8.1`;
  [libgit2 1.8.1 commit_body](https://github.com/libgit2/libgit2/blob/v1.8.1/src/libgit2/commit.c#L619)
  trims the body boundaries, and its
  [character classification](https://github.com/libgit2/libgit2/blob/v1.8.1/src/util/ctype_compat.h#L40)
  defines these six ASCII characters explicitly on Windows and uses C isspace
  on Unix. The Actions fixture uses the isolated C-locale Git runner.

## Findings (not fixed)

No additional issue found in this isolated source delta. This review does not
claim wider parity for arbitrary locale/encoding or subject/body separator
edge cases outside the reported failure.

## Tests Reviewed

- `history_body_matches_libgit2_ascii_boundary_whitespace_semantics` directly
  exercises the production parser with empty, whitespace-only, trailing-LF,
  interior-indentation and non-ASCII boundary cases.
- `actual_history_body_boundary_whitespace_matches_existing_local_dtos`
  authors disposable commits without Git commit-message cleanup, then compares
  full serialized DTOs from production CLI plans/parsers and the existing
  local libgit2 path.
- The original `actual_history_filters_without_checkout_and_continues_from_both_merge_parents`
  full DTO parity assertion is retained; only two explicit trailing-LF expected
  values are corrected. No assertion was removed, weakened or ignored.

## Verification

- Lint: UNRUN for this delta; main reports the prior run's compile/Clippy pass.
- TypeCheck: UNRUN for this delta.
- Tests: UNRUN for this delta, including both new regressions.
- Formatting/whitespace: UNRUN. No local tools, parser fixtures, probes or Git
  writes ran. Main must obtain exact-commit Actions results after staging the
  two files; the earlier run does not verify this patch or the backend slice.
