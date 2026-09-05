# Navigation CI Follow-Up

Narrow correction while the active task is `09-06-native-agent-ownership-status`.
Read navigation `final-review.md`, the current affected-file diffs, quality
constraints, and pinned GPUI 0.2.2 `shared_string.rs`. No agents were spawned.

## Findings (fixed)

- File: `crates/mt-app/src/menu.rs:650`
  Issue: Reported Actions run `33992637822`, Windows job `101377466941`,
  candidate `d50f616`, failed with E0308 at line 651. `SharedString` dereferences
  to `ArcCow<str>`, so `Option::as_deref()` produces `Option<&ArcCow<str>>`, not
  the `Option<&str>` expected by the comparison.
  Fix: Borrow the stored SharedString and compare its explicit `as_str()` result
  with the requested anchor. Missing/open-unanchored menus still return false;
  anchored menus still compare exact text. No allocation, helper, or API change.
- File: `crates/mt-ui/src/terminal/search_bar.rs:136`
  Issue: Actions also reported unused `StatefulInteractiveElement as _`.
  Fix: Removed only that import; preserved the other imports and applied format.

## Findings (not fixed)

- `crates/mt-terminal-host/src/server.rs`, `restore_with_timeout_after_spawn`:
  reported Windows test-build dead-code warning for the Unix-used `#[cfg(test)]`
  helper. Recorded only, per main's explicit ownership boundary.
- Reported mt-project unused test-helper warnings are baseline and remain for
  their owning task. No mt-project or host source was changed.

## Verification

- Corrected-source lint, type-check/build, tests, formatting, whitespace,
  codegen, fixtures/probes, and native acceptance: NOT RUN; Actions only.
- Existing `anchored_menu_uses_the_trigger_bottom_edge` test is retained. No
  artificial comparison helper or duplicated source-expression test was added;
  compilation of the actual production method is the regression gate for E0308.
- Read-only `gh run view --job ... --log-failed` returned HTTP 404 for the given
  job under the origin repository. The diagnostic above is attributed to main's
  supplied Actions report, not an independently retrieved successful run.
- Main must rerun the existing Actions gates for the exact corrected commit.
  No rerun, commit, staging, local compiler/tool execution, or app launch occurred.

Reviewer changed only `menu.rs`, the explicitly authorized search-bar import,
and this report. Existing Actions-derived rustfmt/i18n patches were retained;
`icon_tooltip.rs`, current Agent work, specs, and task metadata were untouched.
