# Files Execution Plan

Inherit parent approval and Actions-only execution gates. Follow navigation and
Agent integration for serialized shared sidebar/workbench files.

## Implementation Order

- [x] Read onboarding, worktree-context, file-workbench, and remote I/O contracts.
- [x] Extend directory picker state/navigation and Local/WSL/SSH listing adapters
  with exact modal/request/host ownership and directory-only selection validation.
- [x] Implement the full browser dialog with breadcrumbs, path/filter input,
  home/up, bounded directory scrolling, cancellation, and clear failure states.
- [x] Correct FileTree height/wheel/scrollbar routing without changing scope caches.
- [x] Centralize row/blank creation targets, preserve applicable item actions,
  remove upload/header actions, and retain safe external-path drop handling.
- [x] Add regressions and have main dispatch Trellis check before integration.
- [x] Complete R11 review fixes and the literal row-relative path follow-up.
- [ ] Obtain exact-SHA Actions/native acceptance.

Checked items indicate source implementation, not passing verification. Both
slices have authored tests. FileTree's independent review fixed producing listing
provenance, every retained mutation/download pin and busy/presentation ownership.
R11 independent review completed captured Select/Cancel/form ownership,
Windows UNC/WSL mapping validation and current-session authenticated Home.
FileTree row-relative paths preserve literal POSIX backslashes. All Actions/native
gates remain open; handoffs record exact files and known transport limitations.

## Actions-Only Cases

- Local roots/drives, WSL/SSH POSIX navigation, hidden/empty directories, denied
  access, invalid/long paths, rapid breadcrumb changes, cancellation, and reconnect.
- Select folder while an older listing is pending; switch host/project; prove
  no late result registers or operates on a different source.
- Lists taller than the panel, narrow/high-DPI windows, last-row wheel and
  scrollbar access, row hover/context, and blank-area hit targets.
- File/folder/blank menu action matrices and correct creation parent, existing
  name, invalid name, symlink containment, and stale clipboard/action ownership.
- Drag upload into row/blank targets, conflicts, failures, cancellation, and
  source changes; absence of upload header/menu commands.

## Risks and Acceptance

Primary files: remote directory picker, onboarding view, file tree layout/menu,
and source-aware I/O helpers. Do not rewrite registration or remote save protocols.
Source candidates for the scroll failure are not native reproduction; verify
interaction with Actions-produced artifacts and record remaining native gaps.
