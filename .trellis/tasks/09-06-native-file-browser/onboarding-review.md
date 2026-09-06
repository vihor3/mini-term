# R11 Onboarding Browser Review

## Status

Independent direct trellis-check source review and bounded fixes complete.
R11 and the separately approved FileTree row-relative followup are released to
main. Source-complete is not compiled, runtime-tested or native-accepted.

Loaded Files onboarding-handoff, PRD/design/implement/check.jsonl, before-dev and
check guidance, relevant indexes/shared guides and the main-updated onboarding
contract. Read Git backend-review.md for the subsequent row.rel authorization.
No child agents, local automation, app launches, fixtures/probes or Git writes.

## Findings (fixed)

1. **P1: authenticated Home could be restamped from an old connection-ID cache.**
   `remote_ssh/dirs.rs::probe_connection` now resolves `canonicalize(".")`
   directly on the acquired session. Existing `ensure_operation_session`
   guards run before I/O and after close/completion, including failures.
   Main explicitly approved this extra function body. Its signature and all
   legacy `remote_home`/home_cache callers are unchanged; no mod.rs edit.
   The actual browser fixture poisons the ID-only cache with a different,
   existing directory and checks fresh authenticated home on both sessions.
   Expected home is independently read from SFTP's initial cwd, never assumed
   from fixture SetEnv HOME. The legacy cache remains unchanged by the probe.

2. **P1: WSL-to-native mapping could redirect or reinterpret path data.**
   `project_onboarding/model.rs::wsl_browser_host_path` validates distribution
   and components before native I/O and builds the UNC spelling without native
   Path::join on unchecked POSIX text. It rejects native prefix injection,
   literal backslashes/colon, unrepresentable components and share-root escape.
   `local.rs` resolves explicit POSIX parent components with read-only `pwd -P`
   inside the captured distribution through the existing structured command
   helper, before Win32 normalization. Ordinary paths retain native listing
   and typed I/O errors. Home still uses structured printenv; the output parser
   removes only one record terminator and rejects control/invalid Unicode data.
   Canonical paths are checked before the shared lossy prefix helper; native
   Windows verbatim-only names that registration cannot preserve fail closed.
   No execution_host, registration, command framework or mutation API changed.

3. **P2: delayed callbacks could adopt successor state or close its modal.**
   `remote_directory_picker.rs::DirectorySelection` captures the exact request
   and selected location at render. Select rechecks that capture, listing
   membership/source, parent owner, readiness and epoch. Go, locations and row
   callbacks require the captured live request. Close is idempotent and old
   Cancel callbacks cannot close a newly opened picker with the same kind/ID.
   `project_onboarding/view.rs` captures form context before folder-button
   request allocation; page/mode/host ABA or modal reopen cannot lend new
   authority to an old button. Parent close rejects closed/non-top forms.

4. **P2: nested dialog offsets were omitted from the viewport budget.**
   The picker reserves gpui-component 0.5.1's nested-layer/animation/border
   offsets. The fixed body and selected-path strip explicitly do not shrink;
   the toolbar has a zero minimum width. A production geometry-helper test
   covers a 320px-wide viewport at 400/480/600/800/1080px heights. This is not
   a screenshot or native layout test.

## Source Review

- Local drive/UNC and WSL identity stay distinct from SSH. SSH requests/results
  carry exact connection ID/fingerprint/producing epoch and request ownership;
  canonicalization may change path, never source. Cancellation/overflow are
  terminal for that picker instance. No local fallback is present.
- Browsing remains read-only, one level, hidden-inclusive, naturally sorted,
  with browsable directory links and typed error/empty/loading states. Typing
  filters loaded names; Enter/Go issue a new request. Home/up/breadcrumbs and
  Local drive/distribution places are retained.
- Select still delegates to apply_picker_selection and the existing directory
  probe/registration or parent-field flow. Browser reads never initialize Git,
  register a project or persist a selection directly.
- Layout retains one bounded primary list, flex_1/min_h(0) through its shell,
  fixed nonshrinking rows and a scrollbar overlay confined to the 16px gutter.
  Long breadcrumbs/selected paths scroll horizontally; rows/host are inspectable.
- Inspected the pinned dependency's [InputState](https://raw.githubusercontent.com/longbridge/gpui-component/v0.5.1/crates/ui/src/input/state.rs)
  and [Dialog](https://docs.rs/gpui-component/0.5.1/src/gpui_component/dialog.rs.html)
  source: single-line Enter emits PressEnter; dialog on_ok(false) prevents
  accidental confirmation, and the default input Escape route propagates to
  dialog cancellation. Stable row IDs retain the double-click load callback.
  Actual focus/event dispatch, tooltip timing and wheel/thumb behavior remain
  native acceptance gates, not inferred successes.

## Findings (not fixed)

- SFTP read_dir still materializes before the 20,000-entry acceptance gate;
  its opaque permission failures remain Unavailable. No new transport API or
  localized-text classification was introduced. The 30-second listing budget
  is not a new end-to-end connection/close deadline.
- WSL/Windows names not faithfully representable by the existing UNC/String
  registration boundary are rejected or omitted, not rewritten. Complete
  arbitrary POSIX-byte/Windows-verbatim registration needs separate design.
  Opaque WSL command failures stay Unavailable; normal native I/O retains its
  PermissionDenied/InvalidPath distinction.
- Extremely short viewports below the fixed toolbar/footer budget still need
  an agreed minimum geometry or compact-layout policy. The authored 400px-and-up
  geometry checks do not establish that policy or native acceptance.
- Real WSL drives/mounts/links/home, denied shares, reconnect mid-readdir,
  delayed GPUI events, IME/Enter/Escape, narrow/high-DPI and last-row access are
  unverified. No app was launched or automated UI run.
- The separate legacy mt-project Git status path conversion remains a Git-owner
  followup recorded in file-tree-review.md. The authorized row.rel target
  correction is complete; no full legacy-status parity is claimed.

## Changed Paths / API Handoff

R11 writes: remote_directory_picker.rs, project_onboarding/{model,local,view}.rs,
remote_ssh/dirs.rs browser tests plus the approved probe_connection body, and
this report. FileTree followup: file_tree/{mod,tests}.rs and file-tree-review.md.
Files pinned APIs, other dirs.rs behavior, project_ops.rs visibility/mutations,
Turing's Git modules and every other owner remain untouched by this pass.

DirectoryPickerOptions, public picker open, browse_directory and probe_connection
signatures are unchanged for onboarding/Kepler. Locale delta: **zero**. Main owns
registry/count/generated dictionaries, workflows, spec synchronization and staging.
No further shared-file ownership request remains for the approved R11 fixes.

## Verification / Actions Handoff

- Lint / TypeCheck / compilation / Cargo metadata: **UNRUN**, Actions-only.
- Tests / fixtures / formatting / whitespace / i18n generation: **UNRUN**.
- Native acceptance / automated UI / app launch: **UNRUN**.
- Focused production-helper filters: remote_directory_picker::tests;
  project_onboarding::local::tests::browser_; project_onboarding::model::tests::browser_;
  project_onboarding::view::tests::browser_; remote_ssh::dirs::browser_tests;
  file_tree::tests::row_relative_paths. Include the Local browser filter on
  Windows for surrogate and verbatim-path regressions; no local FS fixture ran.

Exact authenticated **Actions-only** command, already relayed to main:

```sh
cargo test -p mt-app remote_ssh::dirs::browser_tests::actual_loopback_sftp_browser_preserves_source_and_epoch -- --ignored --exact --test-threads=1
```

Uses only the existing guarded loopback_ssh_fixture and an owned browser-UUID
child. Covers actual SFTP hidden/empty/link/POSIX-name listings, no writes/Git
init, missing/file errors, exact producer metadata, poisoned-home cache, real
pool retirement/replacement and stale-epoch rejection before another path probe.
No duplicated server/environment/key setup and no real user-host endpoint.
It does not interrupt an in-flight listing or exercise native dialogs.
