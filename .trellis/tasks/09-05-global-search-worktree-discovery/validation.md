# Validation Report

Recorded: 2026-09-05 (Asia/Shanghai)
GitHub Actions completion date: 2026-09-05 (UTC)

## Outcome

Global Quick Open and automatic related-worktree discovery are complete at
product commit `9e646e195baeef6e8f80b67bb7fe416df81a9e30`. The final differential review
covered only `5ce905f..9e646e1`, the changes introduced for this task. All PRD
acceptance criteria are satisfied, CI and Windows packaging passed on the same
product SHA, and the task stayed within the approved search/catalog/navigation
scope.

## Delivered Behavior

- Sidebar Search and the compatibility `switchProject` action open one
  Orca-style Quick Open; `Ctrl+Shift+F` remains current-worktree file search.
- Quick Open searches in-memory Agent targets, exact terminal panes, related
  worktrees, settings, and allowlisted actions, with Unicode-safe 2 KiB query
  bounds, filters, stable empty-query ordering, direct number keys, and stale
  target failure.
- `Workspace` owns one shared `WorktreeCatalog` used by the sidebar and Quick
  Open. It scans exactly each configured top-level project folder and never
  recursively or host-wide searches for repositories.
- Native Local, WSL, and SSH discovery share the strict `mt-project` porcelain
  parser. Remote scans use NUL output first, exit-129-only text fallback,
  bounded output/time, single-flight scheduling, global concurrency limits,
  last-known projection, and source/generation/SSH-epoch fencing.
- Selecting a discovered row revalidates catalog ownership, registers an exact
  Local/WSL/SSH child through centralized placement, preserves top-level
  aliases, and activates the returned `ProjectId + WorktreeId`.
- Terminal results carry project, execution host, worktree, tab, pane, logical
  session, and optional incarnation IDs. Exact dormant panes may hydrate; a
  missing or reused route never creates a replacement pane.

## Product Commits

- `600a5e1d67d387a02bc3e32c3bfe1c6df9ecd848` - add Quick Open and shared
  worktree discovery.
- `427f6bfbc0091528dc4cbf0cce8098a03edd754d` - apply generated i18n and
  formatting diagnostics.
- `532dceccc23879a65d200bb19ef4bea246ba8fe9` - apply Linux/Windows build
  diagnostics.
- `9e646e195baeef6e8f80b67bb7fe416df81a9e30` - satisfy final changed-line
  Clippy gates.

## GitHub Actions CI

- Workflow run: `33938763291`
- URL: https://github.com/vihor3/mini-term/actions/runs/33938763291
- Product SHA: `9e646e195baeef6e8f80b67bb7fe416df81a9e30`
- Result: success
- Rust workspace job: `101231864990`
- Windows MSVC job: `101231864887`
- Root workspace tests: 1,988 passed, 1 ignored, 0 failed
- Sidecar tests: 94 passed, 0 failed
- Node staging tests: 10 passed, 0 failed
- Windows focused tests: 66 passed, 0 failed
- Changed-line rustfmt diagnostics: 0 hunks
- Generated i18n dictionary: synchronized
- Changed-line Clippy diagnostics: 0 warnings
- Linux/Windows annotations: 0
- Locked metadata, Linux checks, Windows MSVC checks, sidecar checks, and
  whitespace checks: success

## Windows Package

- Workflow run: `33938763502`
- URL: https://github.com/vihor3/mini-term/actions/runs/33938763502
- Job: `101231852511`, success, 0 annotations
- Product SHA: `9e646e195baeef6e8f80b67bb7fe416df81a9e30`
- Target: `x86_64-pc-windows-msvc`
- Package version: `1.2.2-ci.27`; application version: `1.2.2`
- Artifact ID: `9961440350`
- Artifact: `Mini-Term_1.2.2-ci.27_windows-x64`
- Artifact size: 18,297,454 bytes
- Artifact digest:
  `sha256:d8dc52b85e7bb55ec9fbf5c8fa08bdb557f26c175d0aa9c2ca11df2d7c96b3d6`
- Artifact expiry: `2026-09-19T02:43:42Z`
- Installer: `Mini-Term_1.2.2-ci.27_x64-setup.exe`
- Installer size: 18,306,839 bytes
- Installer SHA-256:
  `49f0da633d575bf0dbd6ebfe77d1f384559741c2be1aadfd66724c568a8c9c70`
- Installer machine: `0x014c`
- Verification: 8 payloads, 7 feature markers, and 13 extracted files passed
- Downloaded deliverable:
  `/home/leo/Downloads/Mini-Term_1.2.2-ci.27_windows-x64/`

## Focused Test Evidence

- Public worktree parser tests cover NUL/text parity, malformed UTF-8 and
  quoting, conflicting fields, unknown fields, duplicate paths, and explicit
  Native/POSIX path semantics.
- Catalog tests cover output authority, timeout/truncation rejection,
  unsupported-`-z` versus non-Git classification, POSIX case, top-level-only
  scheduling, single-flight dirty reruns, global capacity, source generation,
  SSH epoch/fingerprint fences, canonical aliases, fallback rows, and
  last-known preservation.
- Registration tests cover Local, WSL, and SSH child placement, root/host/
  connection/distribution validation, canonical dedupe, repeated selection,
  exact returned identity, and top-level alias preservation.
- Navigation tests cover Unicode bounds, ranking, filter reconciliation,
  terminal/Agent dedupe, stable recency, direct-number selection, row geometry,
  keymap precedence, and exact terminal identity mismatch.
- Pointer selection, backdrop/Esc close, single-overlay ownership, focus
  restoration, and both open entrypoints were additionally checked through the
  concrete GPUI wiring and the existing overlay lifecycle tests. The repository
  does not currently provide an end-to-end GPUI window interaction harness.

## Static Review And Trellis Synchronization

- `python3 .trellis/scripts/task.py validate` passed with 11 implementation and
  14 check context entries.
- `git diff --check 5ce905f..9e646e1` passed.
- Remote branch `fork/feat/remote-file-management` matched the full product SHA.
- The old `project_switcher.rs` implementation is removed; only the compatible
  action/hotkey/overlay identity remains.
- Sidebar and Quick Open consume the same catalog entity; no second scanner or
  recursive repository discovery path remains.
- Relevant `mt-app` and `mt-project` contracts now record public porcelain path
  semantics, catalog authority, exact terminal routing, and child placement.
- No repository `target/`, `dist/`, or `release/` directory was created.
- Unrelated dirty Trellis/bootstrap/archive files were not staged, rewritten,
  reviewed, or included in this task's commits.

## Execution Policy And Residual Risk

No local Cargo build, rustfmt, Clippy, test, generated-i18n command, Node test,
Docker command, installer build, or installer extraction was run. Executable
validation and packaging ran only in GitHub Actions. Local work was limited to
source/document inspection, Trellis validation, Git metadata, artifact
download, and checksum inspection.

Residual risk:

- A physical Windows GPU/UI smoke test was not performed.
- The repository has no GPUI end-to-end window harness. Pointer, backdrop, and
  focus behavior is covered by unit contracts, existing overlay tests, source
  inspection, Windows compilation, and verified packaging rather than a real
  window automation run.
