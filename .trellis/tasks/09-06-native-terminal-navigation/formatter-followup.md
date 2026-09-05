# Navigation Formatter CI Follow-Up

Bounded trellis-check source review on 2026-09-06 while the active task is
`09-06-native-tasks-gh-accounts`. No agents were spawned. This is source review,
not passing automated verification. Main owns commits, push, Actions, and specs.

## Findings And Fix

- P1, fixed in source: `check_changed_rustfmt.mjs` reconstructed the applicable
  `RUSTFMT_PATCH_PATH` artifact from independently selected `-U0` hunks. A move
  could lose its baseline-side insertion, and retained hunk positions still
  assumed omitted edits had occurred. Applying this artifact with
  `--unidiff-zero` was neither semantically complete nor position-safe.
- Both existing artifact paths now receive the same complete `-U3` diffs for
  files with changed-line formatting differences. Every formatting hunk in each
  selected file is retained, including baseline hunks. The compatibility name
  `changed-rustfmt.patch` is preserved, but its console description explicitly
  identifies complete per-file formatting. No filtered applicable patch remains.
- Changed-line intersection still controls failure and console hunk diagnostics.
  Baseline-only files remain excluded, and ignored-baseline counting is unchanged.
  The workflow change only adds the new Node regression step after Setup Rust;
  no existing formatting, generated-code, compilation, or other gate was weakened.

## Applied Navigation Source Audit

- Read the navigation `ci-followup.md`, `final-review.md`, relevant quality
  context, script/workflow, all 26 Rust paths changed in `d50f616..c90ff13`, and
  the actual run `33992637822` artifacts under
  `/home/leo/.cache/mini-term/actions/33992637822/rustfmt/`:
  `changed-rustfmt.patch` and `full-rustfmt.patch`. Token-level Git diff was a
  source-reading aid, not an automated equivalence check.
- Confirmed the two known corruptions in committed `store/panes.rs`: the deleted
  `super::{AppStore, ProjectState, TerminalJumpTarget}` import lacked its moved
  insertion, and `states` was declared after `request.aliases` used it. The full
  artifact retains the import after `super::pure` and declares `states` first.
  Rawls's current source contains both repairs. This reviewer did not edit panes
  or touch the concurrent Agent title-event code.
- Compared the nontrivial `terminal_area.rs` ForkSession match arm directly with
  the full artifact: one guarded fork callback remains in its own match arm.
- Compared the `title_bar.rs` viewport canvas directly: notification remains
  inside the scroll-delta condition, inside selected-index and width/reveal
  conditions, all inside `this.update`. No closing scope was lost or relocated.
- Compared the host cleanup test directly: close/create/restore/session each
  still assert HostBusy, in order, followed by retained incarnation and restart
  cleanup assertions. The panes completion Remove/Conflict assertions also
  retain the full artifact's order and arguments.
- No further partial-patch token loss or semantic movement was identified in
  the bounded committed navigation audit. The explicit menu anchor fix, unused
  search-bar import removal, and generated i18n wording changes were preserved.
  No additional owner correction is requested. This does not establish that the
  current combined tree compiles or passes native acceptance.

## Authored Regressions

Four production-CLI fixture tests in `tests/changedRustfmt.test.cjs`, all UNRUN:

1. Import deletion on a changed line and reinsertion on baseline lines: retain
   both sides and all remaining formatting in that file.
2. Declaration deletion/insertion split across separate `-U0` hunks, after an
   ignored baseline offset change: preserve `states` before its first use.
3. Baseline-only formatting, an untouched unformatted file, and an already
   formatted source edit: exit zero, no patch, and no source mutation.
4. Multiple affected files: include complete patches for both while excluding
   baseline-only and untouched files.

The affected-file tests invoke the real script and real rustfmt in disposable
Actions repositories, apply both artifacts with ordinary `git apply` without
`--unidiff-zero`, and compare every file with the complete expected rustfmt output
or its unchanged source. No fixture, Node process, formatter, or patch application
was executed locally.

## Changed Paths And Pending Gates

- `.github/scripts/check_changed_rustfmt.mjs`
- `tests/changedRustfmt.test.cjs`
- `.github/workflows/ci.yml` (new Actions test step only)
- `.trellis/tasks/09-06-native-terminal-navigation/formatter-followup.md`

Automated verification: NOT RUN. Main must obtain exact-corrected-commit Actions
evidence for the new Node step and all existing Linux/Windows compilation,
metadata, test, lint, formatting, generated-i18n, sidecar, and whitespace gates.
Navigation/native gates in `final-review.md` remain pending on Actions-produced
artifacts. The earlier host Windows-only unused test helper and mt-project
baseline warnings remain with their owners. No local checks, app execution,
staging, commits, resets, or CI dispatch occurred. Old partial artifacts remain
unsafe; fixing their producer does not repair previously applied source edits.

## Bug Analysis: Partial Formatting Artifacts

### 1. Root Cause Category

- E, implicit assumption: independently relevant formatter hunks were assumed
  to compose into a safe transformation. B, cross-layer artifact contract, and
  D, missing application fixtures, allowed that assumption to reach main.

### 2. Why Fixes Failed

1. Main trusted the selected-hunk artifact and relaxed application context.
   That lost a moved import and mispositioned a later declaration.
2. Follow-up blank-line repairs addressed formatting symptoms, not artifact
   completeness. Compilation later exposed the concrete source damage.

### 3. Prevention Mechanisms

| Priority | Mechanism | Status |
| --- | --- | --- |
| P0 | Complete contextual artifacts; relevance gates files, not repair hunks | Implemented; Actions pending |
| P0 | Real script/artifact application fixtures | Four authored; Actions pending |
| P0 | Compare applied navigation source with original full artifact | Source-reviewed; two repairs committed separately |
| P1 | Document producer/consumer rule in release staging contract | Written by main |

### 4. Systematic Expansion

Do not reuse diagnostic-range filtering as a patch generator for imports,
formatters, code generators, or other multi-hunk transformations. Preserve
source ownership when applying artifacts to a worktree with ongoing changes.

### 5. Knowledge Capture

- Updated the release-staging contract and navigation check manifest.
- This report retains the affected source and exact original artifact evidence.
- No `src/templates` tree exists here; no template copy was created.
- Automated acceptance is still pending, not implied by this analysis.
