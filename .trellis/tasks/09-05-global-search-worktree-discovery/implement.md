# Implementation Plan

## Entry Gate

- Do not run `task.py start`, edit product code, or dispatch implementation
  until the user explicitly approves the final planning summary produced from
  `prd.md` and `design.md`.
- At implementation start, load the task manifests and the package specs listed
  in `implement.jsonl` before touching each layer.
- Preserve all unrelated dirty/untracked files. Stage only task-owned source,
  tests, i18n output, and task documentation.
- Do not run Cargo, tests, formatting, generated-code checks, packaging, or
  Docker locally. GitHub Actions is the only executable validation authority.

## Phase 1. Shared Porcelain Capture API

- [x] Expose the existing strict NUL/text worktree porcelain parsers through a
      small public `mt-project::worktree` API; do not duplicate parsing in
      `mt-app`.
- [x] Keep existing local catalog behavior and compatibility callers unchanged.
- [x] Add parser API tests for NUL/text parity, malformed bytes, C quoting,
      duplicate/conflicting fields, and unknown fields.
- [x] Update the worktree catalog spec only if the public capture contract adds
      a reusable invariant not already recorded.

Rollback point: parser visibility/API commit. It must be independently harmless
to current local callers.

## Phase 2. Workspace-Owned Worktree Catalog

- [x] Add `worktree_catalog.rs` with root-target construction, snapshots,
      last-known degradation, per-root single-flight, dirty rerun, bounded
      concurrency, and foreground polling.
- [x] Build exactly one scan target from each added top-level project folder.
      Never recursively search that folder or enumerate unrelated host repos;
      child worktrees do not start independent scans.
- [x] Cover main-worktree and linked-worktree anchors returning the same related
      inventory, plus non-Git anchors remaining a single configured row.
- [x] Route Native Local to `mt_project::worktree::scan`.
- [x] Route WSL/SSH to `execute_host_command` with NUL porcelain first and
      exit-129-only text fallback.
- [x] Reject timeout, truncation, non-zero, malformed, stale generation,
      fingerprint/path/root mismatch, and stale SSH epoch.
- [x] Add one tested WSL POSIX-to-host-visible path conversion helper at the
      execution/path boundary instead of copying onboarding's private helper.
- [x] Project configured rows and discovered facts into one immutable,
      host-qualified group/row model used by all consumers.
- [x] Preserve prior snapshots and configured fallback rows while refreshing or
      after failure.
- [x] Instantiate one catalog in `Workspace` and make its lifecycle match the
      window/store lifecycle.

Rollback point: catalog entity can exist without changing sidebar rendering.

## Phase 3. Exact Activation and Child Registration

- [x] Add a public exact terminal-target projection and activation boundary in
      `AppStore`; include project, worktree, tab, pane, logical session, and
      expected incarnation.
- [x] Revalidate every field immediately before activation, then reveal the
      terminal page. A missing target must not create a replacement pane.
- [x] Extend centralized project registration with explicit top-level versus
      child-worktree placement.
- [x] Keep existing onboarding behavior source-compatible through a wrapper or
      unchanged call shape.
- [x] Validate root existence, top-level ownership, Local/WSL/SSH host match,
      SSH connection match, WSL distribution match, and canonical location
      dedupe before child insertion.
- [x] Ensure a discovered child does not enter `projectTree`, receives
      `parent_project_id`, and returns the exact `ProjectId + WorktreeId` used
      for reactivation.
- [x] Cover duplicate selection and existing top-level alias behavior without
      silently reparenting user configuration.

Rollback point: registration/activation API commit, before UI consumers switch.

## Phase 4. Sidebar Catalog Integration

- [x] Remove sidebar-owned scan state only after shared-catalog tests cover the
      existing local behavior.
- [x] Render Local, WSL, and SSH worktree rows from the shared group model.
- [x] Preserve project order, collapse state, main-first ordering, configured
      fallback behavior, status dots, inline Agent rows, and current styling.
- [x] Add host badges and distinct sparse/detached/locked/prunable/
      disconnected states without changing row dimensions.
- [x] On discovered-row selection, revalidate the catalog owner, register when
      needed, and reactivate the returned worktree.
- [x] Change sidebar Search to emit `OpenJumpPalette` rather than opening file
      search directly.

Rollback point: restore the previous sidebar projection while retaining the
shared catalog and registration tests.

## Phase 5. Global Jump Palette

- [x] Replace the project-only switcher implementation with a global jump
      palette while preserving the `SwitchProject` action and hotkey ID.
- [x] Build exhaustive Agent, terminal, worktree, setting, and action items from
      in-memory store/catalog projections.
- [x] Suppress a duplicate terminal row when its pane is represented by a
      current Agent chat target.
- [x] Implement bounded Unicode-safe query normalization, ranking, stable
      tie-breaking, query selection reset, and result caps/overflow hints.
- [x] Implement process-local MRU capture and freeze empty-query ordering for
      each palette open.
- [x] Implement inline type/host/project filtering and reconcile removed filter
      options.
- [x] Render the Orca-aligned 900 px top-centered modal, section headers, row
      metadata, direct-number hints, and keyboard footer.
- [x] Add up/down, Enter, Esc, Tab, and Ctrl+1..9 behavior using the existing
      GPUI input/action precedence pattern.
- [x] Centralize opening in `Workspace`; route workspace-owned commands only
      after close and suppressing focus restoration.
- [x] Reuse `activate_agent_run`, exact terminal activation, and catalog-owned
      worktree activation. Add concise stale-target toasts.
- [x] Preserve `Ctrl+Shift+F` file search and update visible labels/i18n so the
      old project-switcher wording no longer misdescribes `Ctrl+Shift+P`.

Rollback point: restore `project_switcher::open` at the shortcut/event call
sites; no persisted palette state exists.

## Phase 6. Focused Test Matrix

- [x] Parser exposure and captured-output authority tests.
- [x] Catalog Local/WSL/SSH success and failure matrix, source/epoch fencing,
      single-flight, queued rerun, concurrency cap, and last-known preservation.
- [x] Row projection tests for main/linked/configured/unconfigured, host/path
      collisions, POSIX case, WSL conversion, and Git flags.
- [x] Registration tests for Local/WSL/SSH children, parent validation,
      dedupe, repeated selection, and top-level alias preservation.
- [x] Terminal exact-route tests varying one identity component at a time.
- [x] Palette pure tests for Unicode, 2 KiB bound, ranking, dedupe, filters,
      recency freeze, direct-number indexing, and section caps.
- [x] Palette/sidebar interaction tests for both open entrypoints, keyboard,
      focus return, stale targets, shared catalog updates, and independent
      worktree workbench restoration.
- [x] Update generated i18n inputs/used-key lists only through repository-owned
      source changes; executable generation remains in Actions.

## Phase 7. Local Static Review

Allowed local checks:

```bash
python3 .trellis/scripts/task.py validate .trellis/tasks/09-05-global-search-worktree-discovery
git diff --check
git status --short --branch
git diff -- <task-owned paths>
```

- [x] Review source ownership, stale-result fences, error bounds, focus behavior,
      and compatibility manually before committing.
- [x] Confirm no repository `target/`, Docker container/image/cache, installer,
      or other local build artifact was created.
- [x] Commit only task-owned changes and push the exact product commit.

## Phase 8. GitHub Actions Validation and Package

- [x] Observe the `CI` workflow for the product commit and verify its `headSha`
      matches the pushed full SHA.
- [x] Require Linux workspace check, Clippy, tests, changed-line rustfmt,
      generated i18n, whitespace, locked metadata, and Windows MSVC jobs to pass.
- [x] If Actions publishes a diagnostic patch, inspect and apply only the
      task-owned correction, commit, push, and validate the new SHA.
- [x] Dispatch `Windows Package` for the final passing product commit.
- [x] Verify package workflow `headSha`, conclusion, artifact name/id/size/
      digest, installer filename/hash, payload count, and feature markers.
- [x] Record run IDs and exact SHA in `validation.md` and `task.json` metadata.

## Phase 9. Completion and Trellis Sync

- [x] Run `trellis-check` against the final product diff only.
- [x] Update relevant specs for reusable catalog/palette contracts discovered
      during implementation; do not broaden unrelated specs.
- [x] Mark every PRD acceptance criterion only when evidence exists.
- [x] Record residual risks, especially lack of a physical Windows UI smoke if
      it remains unperformed.
- [ ] Commit validation/spec/task updates, archive the task, and verify the
      archived task metadata, commit SHA, CI run, package run, and progress all
      agree.
