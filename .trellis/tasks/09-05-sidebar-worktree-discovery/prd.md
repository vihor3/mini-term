# Project Worktree Visibility and Settings

## Goal

Automatically show valid worktrees while letting users hide unwanted rows in
project settings, exclude invalid entries, and remove misleading warning
changes during ordinary remote catalog refresh.

## Background

The user requested invalid entries hidden by default and per-project settings,
then explicitly chose default-show for new valid worktrees. The earlier
recommendation to hide unselected external worktrees is superseded.

The settings reference is
`/home/leo/.cache/tmp/orca-paste-1788610690574-105422f2-fedb-43be-9b19-075c39de865e.png`.
The decoded worktree screenshots show Orca's `cyberbase-v26.019` and
mini-term's `ML307H-AICOS`, including a prunable entry.

Read-only Git checks on 2026-09-05 found 21 local branches and 12 worktree
registrations in AICOS, three prunable; v26.019 has five branches and three
worktrees. They share an upstream but have separate local `.git` directories.
The earlier five-branch statement applies only to v26.019. Its three worktrees
match the first image; this establishes neither a row cap nor AICOS's saved
Orca settings.

[Orca research](research/orca-visibility.md) records discovery/ownership/source/
import rules and test evidence. Orca's new-repository external/scratch default
hiding is not the policy requested for mini-term.
[Settings research](research/project-visibility-settings.md) records existing
menu/form/persistence integration.

## Requirements

- W1: Distinguish branch refs, raw worktree inventory, and sidebar visibility.
  Current `crates/mt-app/src/worktree_catalog.rs:1070` discovers worktrees,
  `row_from_fact` at `:1190` carries invalidity, and
  `crates/mt-app/src/orca_sidebar.rs:816` renders every row.
- W2: Show new valid rows automatically. Hide only invalid or explicitly
  user-hidden rows, without numeric caps or ownership/Agent-directory filters.
- W3: Preserve authoritative discovery, stable host-qualified identity,
  last-known state, projects, sessions, and runtime ownership.
- W4: Keep separate local inventories separate even when origin URLs match.
- W5: Exclude prunable and positively identified missing worktrees. An outage,
  unknown path state, or failed refresh is not proof of invalidity. Do not
  remove stored preferences, metadata, or Git registrations.
- W6: Add per-project menu/settings with individual visibility checkboxes,
  branch/path/host disambiguation, and a reachable recovery entry when rows
  are hidden. A blanket hide-external toggle alone is insufficient.
- W7: Persist choices per root project and execution source. Save applies edited
  choices; Cancel does not. Refresh/restart/branch-name changes preserve them;
  no checkbox operation closes terminals, stops Agents, or rebinds identity.
- W8: Keep raw discovery available to settings/management and exact activation.
  This is a sidebar preference, not a global search or lifecycle restriction.
- W9: Separate normal refresh progress from degraded warning state.
  `crates/mt-app/src/worktree_catalog.rs:420` currently demotes every scan,
  and `crates/mt-app/src/orca_sidebar.rs:541` renders its last-known yellow dot
  in the status lane. Stabilize that presentation without weakening authority
  or stale-target rejection.
- W10: Inherit the parent's hard Actions-only constraint for all CI,
  compilation, tests/fixtures, lint/format, generation, and packaging. Local
  work is limited to editing, reading, and static Git diff/status review.

## Acceptance Criteria

- [ ] Extra branch refs alone produce no sidebar rows (W1).
- [ ] New valid external/Agent-created worktrees show automatically; exclusions
  are invalidity or manual choice, never a three-row cap (W2/W5).
- [ ] v26.019's three valid rows and AICOS's nine non-prunable registrations
  appear initially; AICOS's three prunable rows do not (W2/W4/W5).
- [ ] Visible rows and the selected workbench retain correct Local/WSL/SSH
  navigation; manual hiding does not destroy access through settings or
  existing global navigation (W3/W6/W8).
- [ ] Local/WSL/SSH project menus target the clicked project; every hidden row
  can be restored without re-adding the project, including an all-hidden group
  (W6).
- [ ] Save/restart/refresh preserve choices, Cancel changes none, later new
  worktrees still appear, and same paths on different hosts stay isolated (W7).
- [ ] Hiding an active worktree closes no session and changes no raw inventory,
  Git data, or exact runtime route (W3/W7/W8).
- [ ] Offline and unknown state preserve valid last-known rows and choices;
  invalidity does not erase a saved exclusion (W3/W5/W7).
- [ ] Healthy repeated refreshes with Idle Agent state do not repeatedly
  introduce warning/activity dots; real failure remains visible, and stale
  registration/activation tests still reject unsafe targets (W3/W9).
- [ ] Automated checks and the native acceptance build come exclusively from
  GitHub Actions for the exact product commit (W10).

## Dependencies and Out of Scope

This child owns project settings, visibility, and discovery freshness markers.
The status child owns process recognition and semantic Agent state. Each can
be checked independently; parent native startup validation needs both. Execute
this child first and hand off shared `orca_sidebar.rs` boundaries before the
status child's changes.

No Git creation/checkout/deletion/pruning, Orca modification/import, global
search restriction, full menu copy, or unrelated UI/persistence refactor.

## Risks and Planning Status

Incomplete remote identity cannot become a durable preference key, and a
routine refresh must not weaken activation safety to stabilize the UI. The
design defines source/path identity, offline handling, and effective authority.
All product decisions are resolved. The user approved the final parent summary
on 2026-09-05; this child is authorized for implementation under the Actions-only
constraint.
