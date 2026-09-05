# Improve project folder browsing and Files interactions

## Goal

Resolve feedback items 11, 12, and 13: full project directory browser, scrollable Files panel, contextual creation, and drag-only uploads.

## Requirements

- Own parent requirements R11, R12, and R13. Inherit all constraints and scope
  decisions in [the parent PRD](../09-06-native-ui-remote-feedback/prd.md).
- Supply a full add-project directory browser with location breadcrumbs,
  home/up, path/search input, a bounded scrollable directory list, and explicit
  cancel/select actions on the chosen execution host.
- Correct Files list sizing/input routing so all rows can be reached by scrolling.
- Keep only Refresh in the Files header. Preserve applicable item actions in
  file/folder context menus and add file/folder creation to both row types.
  A file's creation target is its parent directory; blank space targets the
  displayed directory and exposes only new file/folder.
- Remove upload actions from header and menus. Retain drag upload with exact
  source/target fencing and existing conflict/error handling.

## Evidence

- `crates/mt-app/src/remote_directory_picker.rs:95` is the current simple remote
  picker; `:206` renders navigation without the reference's complete path UI.
- `crates/mt-app/src/file_tree/mod.rs:1999` gives the list scroll overflow, but
  `:2000` wraps it in a relative shell whose child-height and hit routing require
  inspection. This is a source candidate, not a reproduced native root cause.
- `crates/mt-app/src/file_tree/menu.rs` currently limits creation to directories
  and exposes remote uploads. Existing drag paths already preserve target leases.

## Acceptance Criteria

- [ ] Add-project browsing handles path entry, home/up/breadcrumb movement,
  filtering, empty/loading/error states, and cancellation without stale results (R11).
- [ ] Files reaches its final row with pointer scrolling and its scrollbar (R12).
- [ ] File/folder/blank context menus expose exactly the applicable creation and
  item actions, with correct parent or directory targets (R13).
- [ ] No upload menu/header action remains; dropping files still uploads to the
  intended host/path and retains conflict and cancellation handling (R13).

## Out of Scope

A second project-registration system, recursive host-wide filesystem search,
changing document save semantics, or adding replacement upload-menu commands.

## Risks

The scroll shell is a source-level candidate, not native reproduction. Browser
navigation must honor Windows/WSL/remote path semantics and cancellation ownership.

## Planning Status

PRD, design, execution plan, and curated context are prepared for the parent final
review, not implementation approval. Do not run the application, automated UI
checks, builds, or tests locally; all such execution belongs in GitHub Actions.
