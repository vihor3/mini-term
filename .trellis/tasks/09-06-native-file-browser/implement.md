# Files Execution Plan

Inherit parent approval and Actions-only execution gates. Follow navigation and
Agent integration for serialized shared sidebar/workbench files.

## Implementation Order

- [ ] Read onboarding, worktree-context, file-workbench, and remote I/O contracts.
- [ ] Extend directory picker state/navigation and Local/WSL/SSH listing adapters
  with exact modal/request/host ownership and directory-only selection validation.
- [ ] Implement the full browser dialog with breadcrumbs, path/filter input,
  home/up, bounded directory scrolling, cancellation, and clear failure states.
- [ ] Correct FileTree height/wheel/scrollbar routing without changing scope caches.
- [ ] Centralize row/blank creation targets, preserve applicable item actions,
  remove upload/header actions, and retain safe external-path drop handling.
- [ ] Add regressions and have main dispatch Trellis check before integration.

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
