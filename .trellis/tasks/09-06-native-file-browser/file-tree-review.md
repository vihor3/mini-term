# FileTree R12 / R13 Review

## Status

Direct trellis-check source review and bounded fixes complete, 2026-09-06.
The three P1 findings from the first review are fixed in source; acceptance
remains pending Actions. Reviewed the coordinated Files slice, not the current
`native-remote-git` task pointer. No child agents were spawned.
The subsequently approved row.rel Git caller followup is also source-complete
and released. The separate R11 pass is recorded in onboarding-review.md.

Loaded Files check.jsonl, PRD/design/implement, file-tree-handoff.md, before-dev
and check guidance, relevant indexes/contracts and shared guides. Preserved
implementer/concurrent-owner edits. No local automation, fixtures, syntax
checks, app launches, Git writes, or user-host probes ran.

## Findings (fixed)

1. **P1: listing results and cached rows lacked producing provenance.**
   Files: `remote_ssh/dirs.rs`, `file_tree/{mod,tests}.rs`.
   `list_directory_at_epoch` returns entries plus actual acquired epoch,
   connection ID/fingerprint, requested root and directory. It validates the
   acquired session before probes and after completion. Only read-only
   bootstrap accepts `None`; it adopts the producing epoch, never a later one.
   Scoped listings enforce existing canonical root containment. Ignore-cache
   keys include fingerprint/epoch and retain the connection-invalidation prefix.
   FileTree accepts exact request ID/source/worktree/generation and producing
   provenance. Per-worktree caches retain that owner. Menus never restamp stale
   rows with a current epoch. Stale rows are muted and inert with existing
   loading/failure status. The existing tick detects epoch changes and schedules
   fresh reads; stale descendants refresh parent-first without failure loops.
   Request IDs and source generations do not wrap.

2. **P1: create/rename/delete/copy/download acquired unpinned sessions.**
   Files: `remote_ssh/{dirs,delete,transfer}.rs`, `file_tree/{menu,ops}.rs`.
   Added opt-in pinned variants alongside the first-pass upload preflight/upload
   pins. FileTree retains the original connection and epoch through dispatch,
   prompts and conflict choice. `FileSessionPin` checks acquired epoch, observed
   epoch and exact pooled Arc before work, before write/commit steps and after
   completion. Recursion and cleanup retain the original SFTP handle. Pinned
   delete refuses fresh-session fallback, including proven pre-dispatch failure.
   No mutating replay was added. Legacy signatures, unpinned policy, browser
   APIs, transport formats and non-FileTree callers remain intact. Existing
   staging/conflict/cleanup paths are retained. Rejected pending directory
   commits preserve rollback-count snapshots. Source-change errors retain
   partial upload/download counts and operation/cleanup diagnostics.

3. **P1: busy ownership and presentation ownership were conflated.**
   Files: `file_tree/{mod,ops,tests}.rs`.
   A monotonic operation ID owns full source, row/blank identity and listing
   request. Preflight -> Choice -> Running retains one reservation without a
   release gap. Source switches never clear it. Only its exact owner and phase
   can release it; queued cancellation cannot clear Running or a successor.
   Download retains the full target through local preflight and choice.
   Completion applies expansion/collapse/reload/alerts only to exact current
   source/worktree/generation/epoch and operation ID. Stale completion may only
   invalidate the original stable source cache or start a fresh owned read on
   that stable source. Reconciliation invalidates pending listing IDs too, so
   late pre-completion child reads cannot repopulate actionable rows. No old
   presentation changes are replayed after ABA return.

4. **Clipboard and remote paths lost capture or exact text.**
   Files: `file_tree/{mod,menu,ops,tests}.rs`, `remote_ssh/transfer.rs`.
   The private clipboard retains its typed target, listing proof, worktree,
   generation and epoch. Paste validates both ends. Remote mutations use checked
   `Path::to_str`; remote source roots and operation keys compare exact text,
   not Windows path normalization. `download_conflicts_for_files` parses POSIX
   basenames on every platform while legacy preflight retains its policy.
   Invalid remote text fails before Files transfer I/O. No local fallback.

5. **Git caller followup: row-relative POSIX backslashes were rewritten.**
   Files: `file_tree/{mod,tests}.rs`. Read Git `backend-review.md` and fixed the
   authorized caller boundary. `row_relative_path` uses the cached listing's
   captured root/backend, checked text and existing POSIX relative helper.
   It bypasses both the unconditional replacement and `fs_ops::relative_path`,
   which already normalized backslashes internally. Only actual Local Windows
   drive/UNC representations convert native separators; SSH and POSIX-spelled
   Local/WSL paths retain literal backslashes. Invalid/out-of-root paths produce
   no guessed target. Added cross-platform POSIX and Windows drive/UNC/WSL
   helper regressions, UNRUN. Removed FileTree's redundant normalization of
   already source-relative Git status DTO paths. Git diff API/menu forwarding
   stays unchanged.

## Findings (not fixed)

- No further scoped P1 source correction is outstanding. Compilation, lint,
  formatting and authored regressions remain unverified; source review is not
  end-to-end or native acceptance evidence.
- Existing SFTP helpers and bounded shell deletion are opaque during an awaited
  call. Pin checks cannot atomically revoke an already dispatched request or
  interrupt its internal file commit. Partial effects remain on the captured
  session; cleanup uses that handle and no replacement replays the operation.
  No mt-ssh/generic executor changes were authorized or made. Deterministic
  mid-write interruption/cleanup-failure coverage remains coordinated transport
  work, not something the acquisition/replacement fixture below proves.
- Native sizing/input/presentation acceptance and main-owned contract-document
  synchronization remain open.
- Git-owner followup outside the authorized caller correction:
  `mt-project/src/git.rs::collect_repo_status` still unconditionally replaces
  backslashes in `display_path` (and rename `old_path`) before legacy
  `get_git_status` reaches FileTree. This can mislabel or hide the current
  status-gated View Diff menu for a literal POSIX backslash filename, even
  though `row.rel` now retains the exact target. Main: relay to the mt-project
  owner or finish migration of the legacy status provider. No backend guessing
  or mt-project edit was made here; no full legacy-status parity is claimed.

## Source Review

- Actual layout: `main.rs::render_context_sidebar` supplies a bounded flex
  column and `flex_1 + min_h(0) + overflow_hidden` content slot. FileTree fills it
  with a shrinkable flex column, one bounded list, nonshrinking rows/status
  strips and an overlay limited to the reserved right scrollbar gutter. This
  is source diagnosis, not reproduced native wheel behavior or interception.
- Header contains only Refresh with shared `IconTooltips::button/group`.
  Applicable row actions and both creation actions remain. Creation/drop uses
  file parent, directory self and captured blank root. Blank menus contain
  exactly New File and New Folder. There was no prior Cut action.
- No upload menu/header/chooser entry remains; external-path drop is the sole
  FileTree upload entry. Internal path-to-terminal drag stays separate.
- Rows, Git presentation, selection, warnings and ScrollHandle remain scoped by
  WorktreeId despite global ContextPanel. Busy ownership is independent.
- Single-click preview/toggle, double-click rename, deferred prompts and document
  promotion semantics were not redesigned. The public document-download entry
  keeps its signature and now requires a current root listing before capture.

## Changed Paths / APIs

Reviewer writes: `crates/mt-app/src/file_tree/{mod,menu,ops,tests}.rs`,
`crates/mt-app/src/remote_ssh/{dirs,delete,transfer}.rs`, and this report.
No R12/R13 reviewer edits to file_ops.rs, remote_ssh/mod.rs, project_ops/browser
behavior, main/store, other libraries, workflow/spec files or generated
dictionaries. Subsequent approved R11 browser/probe changes are documented
separately in onboarding-review.md, not changes to the Files pinned APIs.
Locale delta: zero beyond Parfit's original three keys. Main owns combined
968-entry registry/count integration and Actions i18n generation.

Existing module re-exports expose these opt-in APIs:

```rust
list_directory_at_epoch(conn, expected_epoch: Option<u64>, path, root, refresh)
    -> Result<RemoteFileListing, String>
// source: connection_id, connection_fingerprint, connection_epoch,
//         project_root, directory; entries: Vec<FileEntry>
create_entry_at_epoch(conn, expected_epoch: u64, root, parent, name, is_dir)
rename_entry_at_epoch(conn, expected_epoch: u64, root, path, new_name)
delete_entry_at_epoch(conn, expected_epoch: u64, root, path)
copy_entry_keep_both_at_epoch(conn, expected_epoch: u64, root, source, target_dir)
upload_conflicts_at_epoch(conn, expected_epoch: u64, root, target_dir, local_paths)
upload_paths_at_epoch(conn, expected_epoch: u64, root, target_dir, local_paths, strategy)
download_entries_at_epoch(conn, expected_epoch: u64, root, paths, download_dir, strategy)
download_conflicts_for_files(download_dir, remote_paths) // local POSIX preflight
```

`remote_ssh::transfer::FileSessionPin::{new,check,finish,is_pinned}` is visible to
sibling fixture modules for the real acquired/current/pool boundary while
retaining the original Arc. No extra module visibility or framework requested.
No credential/command architecture was introduced.

## Authored Regressions

- `file_tree::tests`: real request/result provenance, bootstrap epoch adoption,
  cache provenance across ABA/rebinding, stale descendant refresh planning,
  independent busy ownership, phase/operation-ID cancellation, exact preflight
  target and POSIX root identity. Original clipboard/upload capture, menu/layout
  builders, Windows drive/UNC, click and exclusive-create/symlink fixtures remain.
- `remote_ssh::transfer::epoch_tests`: acquired/current/pool agreement, composed
  operation/cleanup errors, partial counts, POSIX download preflight and invalid
  Unicode. Production helpers, not source-text assertions.
- `remote_ssh::delete::epoch_tests`: pinned deletion never reacquires, while the
  legacy policy stays allowed. Main: include this focused Actions filter.
- Ignored Unix authenticated SFTP filter:
  `remote_ssh::transfer::epoch_tests::actual_loopback_sftp_files_pins_mutations_and_containment`.
  Consumes `remote_ssh::loopback_ssh_fixture()` and owns only a `files-<uuid>`
  child. Covers listing metadata, real create/rename/delete, file+directory
  copy/upload/download, upload conflict policies, root/symlink containment,
  actual pool replacement and stale-epoch refusal for every pinned Files API
  with unchanged-destination assertions. Does not interrupt a dispatched
  transfer or exercise native dialogs. No server/environment/key setup added.

Additional command for main's shared **Actions-only** loopback step:

```sh
cargo test -p mt-app remote_ssh::transfer::epoch_tests::actual_loopback_sftp_files_pins_mutations_and_containment -- --ignored --exact --test-threads=1
```

The shared helper guards GITHUB_ACTIONS, a RUNNER_TEMP-contained root, isolated
HOME and loopback-only key/port configuration. Main owns sshd setup, outer
timeout, teardown and safe artifacts. Never upload fixture private keys.

## Verification / Coordination

- Lint: UNRUN, Actions-only.
- TypeCheck/compilation/Cargo metadata: UNRUN, Actions-only.
- Tests/fixtures: UNRUN, including the ignored authenticated SFTP fixture.
- Formatting/whitespace/i18n generation/app launch/automated UI: UNRUN.
- Main-reported run 33997336854 tested four formatter-artifact regressions, not
  this Files slice. No product commit/job/artifact currently validates it.
- Main already owns Windows FileTree/picker/transfer steps and locale integration.
  Add the delete pure filter and exact ignored Unix SFTP filter above. No shared
  workflow or registry edit was made here.
- Native Actions-artifact acceptance still needs final-row wheel/scrollbar
  access, narrow/high-DPI sizing, tooltip timing, row/blank/drop routing, restored
  scroll/cache positions, both double-click rename gestures, conflicts/cancel/
  errors/progress and cross-worktree tool switching. Mid-dispatch interruption
  requires separately coordinated fixture control; this review claims no such
  transport or native evidence.
