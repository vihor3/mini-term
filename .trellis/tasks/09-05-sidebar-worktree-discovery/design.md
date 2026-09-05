# Project Worktree Visibility Design

## Decision and Boundaries

The user chose default-show for every newly discovered valid worktree. Store
only explicit manual hiding; do not port Orca's ownership, scratch-directory,
or import-required default-hide rules. Invalid registrations are excluded
independently of manual preferences. There is no numeric row cap.

`WorktreeCatalog` remains the single discovery owner. Its raw groups and exact
target resolution stay complete for Quick Open, management, and settings.
`OrcaProjectSidebar` consumes a visibility projection derived from those same
groups and project preferences. Rendering and checkbox changes do not scan
Git/SSH or create project registrations.

Owning modules are `mt-config::ProjectConfig`, AppStore project setters,
`worktree_catalog.rs`, `orca_sidebar.rs`, and a small project-settings form
module using the existing menu, guarded dialog, and checkbox helpers. Do not
refactor the old ProjectList or global settings UI to introduce this entry.

## Preference Contract

Add a typed, serde-defaulted hidden-worktree collection on the owning root
project. Missing/empty preferences mean no manual hiding. Let the existing
JSON-per-project `config.db` storage serialize it; no new database, SQL table,
sidecar projection field, dependency, or Orca metadata import is required.

An exclusion identifies the root project's configured execution source and a
typed location: either a normalized canonical worktree path or an existing
configured project ID plus its normalized configured path. The configured
variant is a sidebar preference only and never claims canonical Git identity.
Source qualification includes Local/WSL/
SSH namespace, WSL distro or SSH connection, and the known stable execution
host identity. Reuse existing identity/path helpers. Bind choices to the root
source so repointing a project or changing its host cannot silently transfer
them to a different checkout. Do not use branch names, display names, array
indices, remote connection epochs, or transient `unavailable` row keys.

Use a last-confirmed source/path binding during an offline refresh, preserving
its exclusions without claiming current Git authority. When identity has never
been established, retain saved preferences and disable edits to unresolved
rows until an unambiguous source/path exists. Never persist provisional UI row
keys as if they were canonical identities. Field/type spelling can follow the
nearest local helper; these identity and compatibility rules are mandatory.

Static review found that a WSL configured UNC alias may remain separate from
the canonical Git row. The narrowly scoped correction is the distinct
`ConfiguredProject` preference location, not an alias resolver or a runtime
binding change. A known configured row can therefore be hidden without
claiming its provisional path is canonical. Preserve unknown SSH-host
restrictions, exact root/source ownership, and normalized configured paths.
When a row later resolves, check both applicable exclusions and make its
checkbox use that same rule; Unhide clears both through the edit-only merge.
Revalidate configured project/path ownership on Save. Retain readability of
the original canonical-entry JSON shape. Alias deduplication is not added.

## Sidebar Predicate

For each raw row:

1. A `prunable` registration or positively known missing path is excluded.
2. An exact source/path match in the user's hidden collection is excluded.
3. Otherwise the row is shown, including valid external, detached, locked, or
   Agent-created worktrees. Existing selection-safety rules still apply to
   bare or otherwise unselectable records.

Unknown path state, an unsupported scan, or loss of SSH connectivity is not
proof of invalidity. Preserve current configured fallback and last-known
inventory contracts. Scans do not delete hidden preferences: if a manually
hidden path disappears and returns, it remains hidden; a recovered invalid
path with no manual exclusion becomes visible again.

Any valid worktree can be manually hidden, including the selected one. Keep
the project header/settings entry available even if no rows are visible. Do
not switch the active workbench, close terminals/documents, stop Agents, or
rebind identities. Existing global navigation remains available and is not
filtered by this sidebar-specific preference.

## Project Settings Interaction

- Add a fixed-size per-project ellipsis tool button with a tooltip. Stop
  propagation so it does not collapse the project or switch the workbench.
- Capture the clicked root project and execution-source identity. Open the
  existing anchored menu with a Project Settings action on Local, WSL, and SSH.
- Use an entity-backed `prompt::open_guarded` form. Show a bounded, scrollable
  list with checkbox, branch/name, path/host, and state. Valid new rows start
  checked; manually hidden rows are unchecked and remain available here.
- Invalid records can appear as disabled state rows in the settings inventory;
  checking them must not bypass invalidity or registration safety. No prune or
  delete action is added.
- Save applies the changed checkboxes through one AppStore setter and the
  existing config writer. Cancel/Escape discards the draft and restores focus.
  Do not promise synchronous disk acknowledgement from the queued writer.
- Merge only edited identities into the current hidden collection; preserve
  exclusions for undisplayed/offline/removed rows. New rows discovered while
  the form is open remain default-visible rather than implicitly unchecked.
- Revalidate the captured root/source before Save. A removed or reconfigured
  project rejects the stale save with a bounded error. Preserve unrelated
  project fields changed while the dialog was open.

## Refresh Presentation

Routine scans currently call `mark_snapshot_last_known` before any failure,
which changes the warning dot shown in the Agent/status lane. Separate normal
in-flight progress from genuine degraded snapshot state:

- Preserve the last successful snapshot and its existing warning while an
  ordinary same-owner refresh is in flight. In-flight state remains on the
  catalog entry, not a synthetic failure written into the data snapshot.
- Keep activation safety independent: derive effective authority/eligibility
  using current owner, generation, epoch, and refresh state. Do not make an
  unconfigured target registerable during a refresh if it was previously
  fenced; keep existing exact-target revalidation intact.
- Derive `last_known`/warning presentation from actual failure or invalidated
  ownership, not merely effective activation authority. Existing real warnings
  remain visible during retries and clear only after an accepted fresh result.
- Show routine progress separately at the project header in a stable-sized
  lane. It must not change Agent activity, attention, or warning colors.

Use typed state/flags, not matching the text of warning strings. The status
child owns semantic Agent markers; coordinate edits to the shared sidebar.

## Compatibility and Rollback

Old configurations show all valid rows and need no eager migration. Existing
projects, layout databases, identities, sessions, and lockfiles are preserved.
The only new default visual difference is invalid-row exclusion. Same-upstream
independent checkouts remain separate, including AICOS and v26.019.

Rollback can disable/remove the sidebar visibility projection while retaining
stored preferences and raw data. An older binary may drop an unknown optional
project field when it rewrites configuration; cross-version downgrade retention
is not added to the persistence protocol in this task. No rollback step deletes
Git registrations or user directories.

## Validation

Cover default-show and newly discovered rows; individual hide/unhide; all rows
hidden; invalid/recovered/offline cases; config database round-trip; Save versus
Cancel; branch-name changes; same path on different hosts; reconnect epochs;
stale dialog saves; settings opened on a non-active project; and active-session
preservation. Use separate fixtures for v26.019 (three valid) and AICOS (twelve
registrations, three prunable). Test routine refresh, actual failure, retry,
and owner change separately, including unchanged registration safety.

All CI, compilation, tests/fixtures, lint/format/whitespace checks, generation,
packaging, and automated validation run exclusively in GitHub Actions as a
hard user constraint. Any manual native Windows acceptance uses an
Actions-produced artifact and inspects menu targeting, long-list clipping/
focus, persistence, and repeated refreshes. Do not use local/SSH/container
test substitutes. This GPUI surface is not a browser application.
