# R11 Onboarding Browser Handoff

## Implementation Complete; Verification Pending

R11 is implemented in the assigned files plus the explicitly authorized remote
browser API. R12/R13 and shared application navigation are untouched.

## Files Changed

- `crates/mt-app/src/remote_directory_picker.rs`: Local/WSL/SSH browser, source-
  and request-owned navigation, path submission/filtering, home/up/breadcrumbs,
  host/drive/WSL locations menu, hidden folder rows, selection, cancellation,
  bounded dialog/list, scrollbar gutter, inspectable paths, shared IconTooltips.
- `crates/mt-app/src/project_onboarding/model.rs`: browser source/location,
  listing, and typed error data, separate from registration authority.
- `crates/mt-app/src/project_onboarding/local.rs`: read-only native/UNC/WSL
  listing, installed drive/distribution locations, WSL home lookup through the
  existing structured execution-host command API, and local error classification.
- `crates/mt-app/src/project_onboarding/view.rs`: one browser for all existing
  picker entry points, captured canonical SSH home, form-owner guard callbacks,
  cancel/edit/operation invalidation, and retained authoritative selection flow.
- `crates/mt-app/src/remote_ssh/dirs.rs`: exact-session browser API below.
- `crates/mt-app/src/remote_ssh/project_ops.rs`: only two `pub(super)` visibility
  changes for existing context/session guards. No mutation logic changes.
- `crates/mt-i18n/locales/projectOnboarding.ts`: English and Chinese picker text.
- This handoff. No generated `dict.rs`, Git/index, workflow, spec, or metadata edits.

## API Changes

`remote_directory_picker::open` now receives `DirectoryPickerOptions`, a pure
parent-owner guard, selection callback, and cancellation callback. The only
consumer remains unified project onboarding.

`remote_ssh::browse_directory(&RemoteProjectContext, absolute_path)` returns
`Result<RemoteDirectoryListing, RemoteDirectoryBrowseError>`; the listing now
includes `connection_epoch` and `connection_fingerprint`. No unpinned browser
overload remains. FileTree's `list_directory` is unchanged.

All selection paths still call `apply_picker_selection`. Add Existing runs the
existing directory-only operation/probe and centralized registration. Clone and
Create parent selections stay in their existing validation flows. Browsing never
runs Git, creates files, or persists a project.

## Remote I/O Coordination Resolved

Main authorized the narrow browser API in `remote_ssh/dirs.rs` and visibility-only
changes to `RemoteProjectContext::validate` and `ensure_operation_session`.
`remote_ssh::browse_directory` now preserves hidden directory and symlink handling:

- Accepts `RemoteProjectContext` and an absolute path.
- Validates the captured fingerprint and authenticated epoch before filesystem I/O
  and after the listing, using the same session throughout.
- Returns the canonical path, directory entries, exact producing epoch, and
  fingerprint. Listing work has a 30-second deadline and a 20,000-entry gate.
- Reports typed missing/non-directory/invalid-path evidence as InvalidPath.
  `SftpTransferError` retains only Transport/Sftp plus text; opaque SFTP failures
  stay Unavailable, with bounded detail. No localized-string classification.

The browser's own modal/form, request, source, and epoch guards remain required.
No mutations or FileTree listing behavior changed in the remote service.

## Tests and Checks

Authored, NOT RUN:

- Request replacement, A/B/A paths, all source-key components, returned SSH
  epoch/fingerprint, cancellation, request overflow, and stale/unfinished selection.
- Form instance, page/back, create mode, new request, active operation, host
  round trips, and SSH readiness/epoch invalidation using production owner helpers.
- Filter-only typing, hidden names, explicit navigation, long breadcrumbs,
  case-sensitive POSIX names, Windows drive/UNC roots, WSL POSIX-to-UNC selection,
  and no remote-to-local fallback.
- Read-only local fixtures for hidden/empty directories, file exclusion, natural
  sorting, one-level listing, intact contents, and directory/file/broken symlinks.
- Typed local permission/invalid/unavailable errors; structured remote node/path
  validation, entry limits, and opaque SFTP errors that are never string-classified.

Only source reads and read-only Git diff/status review were performed. Builds,
Cargo metadata, tests/fixtures, lint, formatting/checks, i18n generation, whitespace
checks, app launches, SSH probes, and native acceptance are all UNRUN locally.
Main must run the exact-commit GitHub Actions gates and integrate generated i18n
and formatter output through that workflow.

## Remaining Gates

- Windows Actions compilation and artifact acceptance for drive enumeration,
  drive/UNC roots, WSL home/links/mounts, inaccessible shares, and distributions.
- Actions-produced native acceptance for narrow/high-DPI geometry, long path
  inspection, last-row wheel/thumb access, double-click navigation, Enter path
  submission, Escape/Cancel, tooltip timing, and host changes during listings.
- SFTP runtime acceptance for denied/missing/empty directories, symlinks,
  reconnect/reconfiguration during a listing, and selection after a stale request.
- SFTP `read_dir` materializes one directory before the entry-count guard, so
  the guard bounds accepted results/UI but does not impose a streaming wire-memory
  cap. The existing transport API has no bounded readdir primitive. Strengthening
  that resource boundary would require separate `mt-ssh` work.
- Opaque SFTP permission failures intentionally show Unavailable with bounded
  original detail. A dedicated remote PermissionDenied state needs a typed SFTP
  status-code API; local permission failures already have their distinct state.

No Actions run or native success is claimed by this handoff.
