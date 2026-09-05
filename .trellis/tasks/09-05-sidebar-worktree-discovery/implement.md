# Worktree Visibility Implementation Plan

## Gate and Ownership

- The user approved the latest complete parent planning summary on 2026-09-05.
  This child is now in progress through the Trellis implementation agent.
- Use `trellis-implement` then `trellis-check` with the curated JSONL manifests.
  Native Codex context injection is preferred; child-side loading is fallback.
- This child owns visibility/configuration and catalog freshness. The Agent
  child owns process detection/state. Neither logically depends on the other,
  but execute this child first to serialize shared `orca_sidebar.rs` edits.
- Preserve all unrelated working-tree changes and current contracts. Do not
  run Cargo, formatting, generated-code checks, or packaging locally.

## Ordered Checklist

- [x] Capture the scoped baseline and load PRD/design/spec context. Confirm
  default-show overrides the earlier research recommendation.
- [x] Add backward-compatible hidden preferences and config round-trip tests.
  Reuse source/path identity helpers and update required ProjectConfig
  construction sites without unrelated formatting or metadata churn.
- [x] Add a pure visibility predicate and draft-edit merge logic. Test new rows,
  invalid/recovered/offline states, host isolation, and retained exclusions.
- [x] Add the AppStore setter with root/source revalidation and existing writer
  ownership. Preserve other project fields and no-op-save behavior.
- [x] Add per-project ellipsis -> Project Settings with guarded, scrollable,
  checkbox-based form, Save/Cancel, clear row identity, and stale-save errors.
- [x] Apply visibility only in the sidebar. Keep raw catalog groups, Quick
  Open, management, activation, and existing terminal/Agent routes complete.
- [x] Separate ordinary refresh progress from genuine last-known warnings.
  Preserve effective registration authority and all owner/epoch fences.
- [x] Add focused menu/draft/persistence/catalog regression coverage and
  locale inputs/used-key entries through existing i18n ownership.
- [x] Review and update the navigation presentation contract to distinguish
  raw discovery, sidebar visibility, and in-flight activation safety.

Implementation and test source handed off on 2026-09-05. Checked source-writing
items do not assert executed tests. Independent static review is complete.
The Actions-generated dictionary patch was applied in `1d2ea0d`; its rerun
passed the generated dictionary and formatting gates. Complete CI and native
acceptance remain pending; see the parent validation record.
- [x] Run the Trellis reviewer on the exact scoped changes, self-fix its
  verified findings, and inspect the final diff for unrelated changes.
- [ ] Obtain CI and native UI evidence before marking the child complete.

## Validation Commands

Local source/diff review only, not execution of CI checks:

```sh
git diff --stat
```

Focused commands to execute in GitHub Actions after implementation:

```sh
cargo test --locked -p mt-config
cargo test --locked -p mt-app --bin mini-term worktree_catalog::tests
cargo test --locked -p mt-app --bin mini-term store::projects::project_onboarding_tests::visibility_
cargo test --locked -p mt-app --bin mini-term worktree_visibility::tests
cargo test --locked -p mt-app --bin mini-term project_settings::tests
```

Require the
existing `.github/workflows/ci.yml` locked workspace and sidecar checks/tests,
affected-package Clippy, changed-line formatting, generated i18n, and Windows
MSVC jobs for the exact product commit. Apply any generated diagnostics only
through the existing repository workflow; do not claim static inspection is a
passing compiler/test result.

Added filters are `worktree_visibility::tests`, `project_settings::tests`, and
`store::projects::project_onboarding_tests::visibility_`. Config/database and
catalog tests are included by the workspace job. Static review finished after
the typed configured-project exclusion correction; no additional confirmed
defect remains in that bounded review. Actions/native gates are still pending.

This restriction is a hard user constraint: compilation, all tests/fixtures,
lint/format/whitespace checks, generation, and packaging run only in Actions,
not locally, in local containers, or via manually dispatched SSH commands.

Native automated checks run in Actions; any manual acceptance uses the
Actions-produced build. Cases: AICOS invalid rows absent; all nine valid rows initially shown;
new valid row appears; hide/restart/unhide works; settings on a non-active SSH
project affects only that project; long lists remain usable; hiding the active
row preserves its workbench; repeated successful polls do not flash warning
dots; genuine failures remain visible. Record source/binary correspondence.

## Risks and Integration

Review especially `ProjectConfig` construction sites, writer ownership,
`row_key`/fallback identity, `build_groups`/`resolve_target`, and freshness
demotion. Do not weaken stale registration checks to simplify presentation.
Hand the final sidebar/catalog contracts and validation evidence to the Agent
child before its shared-file edits. Parent integration waits for both children.

Rollback is projection/config UI code only; retain preferences and Git data.
