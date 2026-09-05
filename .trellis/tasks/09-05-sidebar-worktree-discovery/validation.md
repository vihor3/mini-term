# Worktree Visibility Validation Record

Recorded: 2026-09-05 (Asia/Shanghai)

## Implementation Handoff

The implementation agent completed source changes against baseline
`b166bd2b65549ef14bf0d6c3b9e1b31afb4f70e8`. Independent static review is in
progress. No compiler, test, formatter, generator, or native acceptance result
is claimed yet. No local validation command was run during implementation.

Changes are confined to `mt-config` typed exclusions and serialization,
`mt-app` settings/visibility/catalog/sidebar/AppStore integration, locale
inputs, and required `ProjectConfig` construction sites in `mt-app` and
`mt-layout`. Both Cargo lockfiles remain outside the planned write scope.

## Source Coverage

- `worktree_visibility::tests`: uncapped default-show inventories, manual
  exclusions, invalid/recovered/offline rows, draft no-op/cancel/merge,
  source/host/root/POSIX isolation, and reconnect/display-name stability.
- `project_settings::tests`: invalid rows unchecked and disabled, saved
  exclusions recoverable, new rows checked, unresolved rows non-editable,
  and list cursor scrolling.
- `store::projects::project_onboarding_tests::visibility_`: source revalidation,
  settings merge ownership, exact project targeting, and active runtime
  preservation.
- `worktree_catalog::tests`: healthy refresh presentation and registration
  eligibility remain separate; genuine degradation survives retries.
- `mt-config` config/database tests: old records default to no exclusions and
  exclusions round-trip through the existing ConfigDb JSON owner.

These are test-source references, not execution evidence. The existing Actions
workspace test job includes these modules without adding a local test path.

## Required Actions

After scoped commit/push approval, use the current tracking fork's `CI` and
`Windows Package` workflows. Record exact `headSha`, run URLs/conclusions, and
artifact identity. Apply generated dictionary and formatting diagnostic patches
from Actions only. The 14 new locale keys require generated dictionary
synchronization; do not run the generator locally.

## First Static Review

- Fixed the i18n expected-count source from 938 to 952; generated dictionary
  synchronization remains Actions-only.
- Found a WSL alias fallback row that could remain permanently non-editable.
  The main-session design decision is a distinct source-qualified configured
  project exclusion, not a new physical-path resolver or identity rebind.
  The design/spec now require dual-key effective visibility and Unhide, exact
  configuration fencing, and canonical-entry serialization compatibility.
  The correction and its static follow-up review are complete at source level.
  Resolved rows honor both exclusions; Unhide clears both. Saved-only cleanup
  removes an exact stored choice without requiring a removed child to resolve,
  while a live edited row still rejects later configuration changes.
- The first reviewer released `orca_sidebar.rs` to the status implementer;
  the WSL correction must not change that file.
- The follow-up reviewer reported no additional confirmed defects in its
  bounded static pass. No compilation, test, formatter, generator, or runtime
  check ran. Alias deduplication remains intentionally out of scope.

## Native Acceptance Pending

Use the matching Actions-produced Windows build to inspect per-project menu
targeting, Save/Cancel, keyboard/focus, long-list clipping, hide/restart/unhide,
all-hidden recovery, and unchanged active terminal ownership. The repository
has no GPUI end-to-end window harness. Static review and a successful installer
extraction are not this native interaction evidence.

The status child may edit disjoint process/state modules during review, but
must wait for explicit handoff before touching the shared sidebar. This child
remains in progress until required acceptance is recorded.
