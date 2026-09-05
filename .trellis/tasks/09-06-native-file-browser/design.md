# Directory Browser and Files Interactions

## Browser

Extend the existing host-aware onboarding picker rather than adding a separate
project registration path. Present a bounded dialog with home/up icon tools,
clickable path segments, path/filter input, scrollable directories, and explicit
Cancel/Select actions. Keep the execution-host label and selected path inspectable.

Local/WSL/SSH listing uses each host's current I/O/path semantics. Windows drive
roots and UNC/WSL paths are not normalized as remote POSIX paths. Remote browsing
never falls back to the client's filesystem. Use the existing validated home and
host identity; no concatenated shell listing or recursive device-wide search.

Typing filters the loaded directory listing; explicit path submission navigates
to a directory. Navigation has loading, empty, permission, unavailable, and
invalid-path states, with bounded text and scrolling. Back/cancel, a new path,
host change, or form operation invalidates old picker requests. Select delegates
to the existing directory-only probe and registration flow; browsing never runs
`git init` or persists a project itself.

## Files Layout

Retain one worktree-owned FileTree entity/cache path and correct the sizing chain
from context-sidebar flex child through scroll shell to the list. Give the list
bounded flex height and `min_h(0)` where needed. The scrollbar's overlay must not
capture the list's wheel events or block row/blank context and drop targets.
Use measured source constraints and Actions UI checks, not a guessed fixed
viewport height or a second scroll container that obscures the original bug.

## Context Targets and Upload

Use one typed context target derived from the captured row and source lease:
directory row -> that directory; file row -> its parent; blank -> displayed root.
Keep existing applicable open/copy/cut/paste/rename/delete/download safeguards
and applicability checks. Creation exists for either row type; blank space has
only new-file/new-folder. These menus describe the captured target, not whatever
is selected when a delayed callback runs.

Remove header/menu upload entry points; keep Refresh as the only Files header
tool. Existing external-path drag upload stays the sole upload entry and reuses
current conflict, cancellation, progress, containment, and connection fencing.
Do not weaken symlink/root checks when resolving a file's parent. Directory
browsing and file operations must survive source changes without a late action
targeting the newly selected project.

## Integration and Compatibility

R17 shares only right-tool selection. Expanded directories, scroll, errors,
selection, clipboard targets, and document pages remain independently scoped.
Existing workbench preview/permanent/dirty document behavior remains unchanged.
Use existing mt-ui icons and shared hover timing; do not introduce a second
icon library or an upload command hidden elsewhere in the same Files surface.
