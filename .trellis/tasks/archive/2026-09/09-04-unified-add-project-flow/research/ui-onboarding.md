# Research: Current project-onboarding UI

- Query: Inspect the current local and SSH project-onboarding UI, host selection and management, directory pickers, modal/navigation patterns, and project activation callbacks for the unified add-project design.
- Scope: internal
- Date: 2026-09-04

## Findings

### Files Found

| File | Current responsibility |
|---|---|
| `crates/mt-app/src/orca_sidebar.rs` | Default Orca project navigation and the visible add-project entry points. |
| `crates/mt-app/src/first_run.rs` | Empty-project screen with separate local and SSH onboarding buttons. |
| `crates/mt-app/src/project_list.rs` | Legacy-shell and group-targeted local/remote add entry points. |
| `crates/mt-app/src/modal.rs` | Current local add-project dialog and native directory chooser. |
| `crates/mt-app/src/remote_project.rs` | Current SSH connection selector, path/name form, validation, persistence, and activation callback. |
| `crates/mt-app/src/remote_directory_picker.rs` | Nested SSH directory browser with request fencing. |
| `crates/mt-app/src/ssh_panel.rs` | SSH connection CRUD plus shared connection-list visual primitives. |
| `crates/mt-app/src/ssh_conn.rs` | Pure SSH grouping, ordering, summaries, and remote-name fallback. |
| `crates/mt-app/src/prompt.rs` | Guarded GPUI dialog open/close and custom title/close patterns. |
| `crates/mt-app/src/overlay.rs` | Global overlay stack and unique modal-kind registry. |
| `crates/mt-app/src/ui.rs` | Shared buttons, choice controls, and responsive dialog sizing. |
| `crates/mt-app/src/store/projects.rs` | Local project deduplication, persistence, identity registration, hydration, and activation. |
| `crates/mt-app/src/store/ssh.rs` | Remote project persistence and identity registration. |
| `crates/mt-app/src/store/identity.rs` | Project-to-worktree identity and execution-host snapshots. |
| `crates/mt-app/src/workbench_area.rs` | Exact project/worktree focus restoration after overlays and switches. |

### Current Entry Points

- The Orca shell is the default; only a truthy `MINI_TERM_LEGACY_SHELL` selects the rollback shell (`crates/mt-app/src/main.rs:324-327`, `crates/mt-app/src/main.rs:945-948`, `crates/mt-app/src/main.rs:2720-2739`).
- In the current Orca header, the visible plus button opens the local dialog, while SSH onboarding is hidden in the adjacent options menu (`crates/mt-app/src/orca_sidebar.rs:756-833`).
- The zero-project screen exposes two separate buttons and routes them to `modal::open_add_project` and `remote_project::open` (`crates/mt-app/src/first_run.rs:77-85`, `crates/mt-app/src/first_run.rs:91-128`).
- The legacy shell still exposes separate root and group-targeted local/remote routes (`crates/mt-app/src/project_list.rs:979-998`, `crates/mt-app/src/project_list.rs:2344-2394`).

Recommendation: make every entry point delegate to one API such as `project_onboarding::open(store, target_group, initial_host, window, cx)`. Keep `target_group` for rollback-shell compatibility, but remove direct calls to the old local and remote dialogs so entry-point behavior cannot drift.

### Local Add-Project Dialog

- The current local dialog is a 460 px confirm dialog containing a manual path input and a native single-directory chooser (`crates/mt-app/src/modal.rs:194-225`, `crates/mt-app/src/modal.rs:225-275`). Manual input is intentionally retained for UNC/WSL paths (`crates/mt-app/src/modal.rs:182-185`).
- The native chooser returns asynchronously and writes directly into the captured input entity; there is no host, page, or request-generation check (`crates/mt-app/src/modal.rs:236-264`). This is harmless in the one-page dialog, but is insufficient once the same entity can switch hosts/forms while a chooser is open.
- Submit only checks `raw.is_empty()` and `path.is_dir()` and silently leaves the dialog open on failure; there is no field error, validating phase, or running phase (`crates/mt-app/src/modal.rs:276-282`).
- Top-level local add calls `add_project`, which activates the result; group-targeted add calls `add_project_at` and moves the result but intentionally does not activate it (`crates/mt-app/src/modal.rs:283-303`). The unified acceptance criteria require both routes to open the resulting project.
- Local duplicate handling is currently inconsistent: `add_project_at` uses normalized path comparison (`crates/mt-app/src/store/projects.rs:105-135`), while `add_project` compares the stored path string exactly (`crates/mt-app/src/store/projects.rs:188-200`).

### Remote Add-Project Dialog

- `AddRemotePanel` owns one selected connection ID, path and name inputs, `busy`, `error`, group target, and list presentation state (`crates/mt-app/src/remote_project.rs:47-65`). It selects the first saved connection and defaults the path to `~` (`crates/mt-app/src/remote_project.rs:79-105`).
- Its 720 px full-height two-pane connection browser reuses SSH management list primitives (`crates/mt-app/src/remote_project.rs:337-372`, `crates/mt-app/src/remote_project.rs:436-509`). This is reusable behavior, but the complete two-pane surface is larger than the requested compact top-level host selector.
- Changing connections is blocked while busy, clears the current error, and resets the path to `~` (`crates/mt-app/src/remote_project.rs:486-504`). There is no corresponding invalidation object because the current form disables all host/path mutation during validation.
- Browse opens the nested remote directory picker and copies the selected canonical path back into the parent input (`crates/mt-app/src/remote_project.rs:543-603`).
- Save snapshots connection, path, name, and target group, runs `validate_dir` off the UI thread, and applies the result only if all live values still match (`crates/mt-app/src/remote_project.rs:172-239`). This is a useful precedent, but equality of form values is weaker than an operation owner token: a later form instance can recreate the same values, and a stale completion currently clears shared `busy` state on mismatch.
- Success persists the remote project, expands the target group, activates it, and closes the modal (`crates/mt-app/src/remote_project.rs:242-288`). Failure stays in the form and presents the returned error (`crates/mt-app/src/remote_project.rs:289-296`).
- `add_remote_project` always creates a new ID; its own documentation explicitly says remote projects do not use local path deduplication (`crates/mt-app/src/store/ssh.rs:201-218`, `crates/mt-app/src/store/ssh.rs:218-247`). Host/path deduplication therefore needs a new shared completion boundary rather than a UI-only pre-check.
- With no saved connections, the current modal shows passive instructional text and no direct add-host action (`crates/mt-app/src/remote_project.rs:346-358`).

### SSH Host Selector and Management

- Reuse `ssh_conn::build_group_buckets` and `connection_summary`; they are the single source of truth for group order, empty groups, and `user@host[:port]` display (`crates/mt-app/src/ssh_conn.rs:15-21`, `crates/mt-app/src/ssh_conn.rs:29-109`).
- Reusable view pieces already exist for group rows, connection cards/text, headers, footers, and bucket rendering (`crates/mt-app/src/ssh_panel.rs:9-19`, `crates/mt-app/src/ssh_panel.rs:69-120`, `crates/mt-app/src/ssh_panel.rs:209-305`, `crates/mt-app/src/ssh_panel.rs:307-403`). A compact host dropdown should reuse the summary/card language, not copy the 720 px management layout.
- `ssh_panel::open` is a separate guarded modal over the global `AppStore`; saving a new connection calls the existing `upsert_ssh_connection` path (`crates/mt-app/src/ssh_panel.rs:620-654`, `crates/mt-app/src/ssh_panel.rs:761-789`). The onboarding "Add remote host" row should open this existing flow. If immediate auto-selection is required, add a narrow open-in-add/callback API instead of duplicating the private credential form.
- The current connection row renders saved configuration only: name plus `user@host[:port]` (`crates/mt-app/src/ssh_panel.rs:307-374`). No connection-scoped health state is exposed to onboarding. Existing runtime state is keyed by an already-registered `project_id`, not by arbitrary saved connection (`crates/mt-app/src/store/remote_runtime.rs:21-34`, `crates/mt-app/src/store/remote_runtime.rs:108-112`).
- `validate_dir` doubles as the real connection test and canonical directory validation (`crates/mt-app/src/remote_ssh/dirs.rs:214-250`); an unused `probe_connection` wrapper exists, but the SSH management UI intentionally has no test button (`crates/mt-app/src/remote_ssh/dirs.rs:375-386`). The unified modal must own any pre-project host status it wants to display.

### Remote Directory Picker

- The picker owns an immutable `SshConnection`, canonical/current/requested paths, list state, `request_id`, and a selection callback (`crates/mt-app/src/remote_directory_picker.rs:17-30`).
- Every load increments `request_id` and rejects a completion whose request or connection no longer matches (`crates/mt-app/src/remote_directory_picker.rs:32-69`). This is the closest current UI precedent for host-scoped stale-result fencing.
- The picker is a non-overlay-closable nested dialog, 560 px wide with a stable 300 px list, Home/Root/Up actions, retry, cancel, and Choose Current Folder (`crates/mt-app/src/remote_directory_picker.rs:74-120`, `crates/mt-app/src/remote_directory_picker.rs:136-177`, `crates/mt-app/src/remote_directory_picker.rs:208-299`).
- The service canonicalizes the requested path, verifies it is a directory, follows directory symlinks for browsing, and returns only child directories (`crates/mt-app/src/remote_ssh/dirs.rs:252-310`). Keep this picker boundary for every remote parent/existing-directory field.
- The row affordance currently uses text glyphs rather than the vector icon system (`crates/mt-app/src/remote_directory_picker.rs:179-204`); the unified visual pass should replace these while retaining the service/state behavior.

### Modal and Navigation Patterns

- Dialog builders run every frame, so editable/modal state belongs in one `Entity`, not captured mutable closure data (`crates/mt-app/src/modal.rs:15-19`). The unified flow should follow this model.
- `open_guarded` registers one overlay kind, prevents duplicate opens, and `close_guarded` only closes the expected topmost dialog (`crates/mt-app/src/prompt.rs:48-99`). Different overlay kinds can stack, which already supports onboarding -> remote picker or onboarding -> SSH management (`crates/mt-app/src/overlay.rs:46-105`, `crates/mt-app/src/overlay.rs:159-188`).
- Settings is the best multi-page modal precedent: an enum owns the active page, a fixed outer shell remains mounted, and one match dispatches page rendering (`crates/mt-app/src/settings/mod.rs:83-143`, `crates/mt-app/src/settings/mod.rs:441-488`, `crates/mt-app/src/settings/mod.rs:561-564`, `crates/mt-app/src/settings/mod.rs:877-890`).
- Existing Back behavior clears detail state and invalidates pending work; the session preview also increments its request token before returning (`crates/mt-app/src/session_panel.rs:1595-1620`).
- Shared control candidates are `ui::ghost_button`, `ui::primary_button`, responsive dialog sizing, and `ui::choice_button` (`crates/mt-app/src/ui.rs:580-620`, `crates/mt-app/src/ui.rs:667-685`, `crates/mt-app/src/ui.rs:849-880`). A tighter segmented shell exists only as private usage-panel helpers (`crates/mt-app/src/usage_panel/mod.rs:2324-2349`); extract it to `ui.rs` if the create-mode control needs joined segments.
- Do not use `gpui_component::IconName`: the application has no asset source and those icons render blank. Use `mt_ui::VectorIcon`/`FileIcon`; public Git and SSH shapes already exist (`crates/mt-app/src/activity_bar.rs:1-13`, `crates/mt-app/src/activity_bar.rs:234-260`, `crates/mt-app/src/activity_bar.rs:420-444`, `crates/mt-ui/src/icons/file.rs:125-188`). Back, Close, Clone, and any host chevrons should be added or exported through the same vector system instead of text glyphs.

### Activation Callback Contract

- `set_active_project` synchronizes the active worktree, clears attention, hydrates, persists the last active project, and notifies observers (`crates/mt-app/src/store/projects.rs:73-103`). Both Orca and legacy project lists observe store notifications, so successful persistence already refreshes visible project data (`crates/mt-app/src/orca_sidebar.rs:513-535`, `crates/mt-app/src/project_list.rs:1155-1173`).
- The best current exact activation handoff is Orca worktree activation: register/find the project, call `set_active_project`, capture the resulting `WorktreeId`, then call `reactivate_active_page` (`crates/mt-app/src/orca_sidebar.rs:632-674`).
- `reactivate_active_page` revalidates the captured project/worktree pair and restores that worktree's terminal/document route (`crates/mt-app/src/workbench_area.rs:563-578`, `crates/mt-app/src/workbench_area.rs:1046-1072`). This matches the workbench contract that deferred focus callbacks capture identity before yielding (`.trellis/spec/mt-app/backend/workbench-identity-contract.md:99-103`, `.trellis/spec/mt-app/backend/workbench-identity-contract.md:205-216`).

Recommendation: the UI should receive one result such as `ProjectRegistrationOutcome { project_id, worktree_id, disposition }`, where `disposition` is `Created` or `Existing`. On success: install/locate project -> `set_active_project` -> close the onboarding overlay -> `reactivate_active_page(project_id, worktree_id, ...)`. Do not let each subpage implement its own persistence, deduplication, activation, or focus sequence.

### Recommended UI State Model

Use one modal entity and explicit enums rather than independent booleans:

```rust
enum OnboardingPage {
    Home,
    Clone,
    Create { mode: CreateMode },
}

enum CreateMode {
    NewFolder,
    InitializeExisting,
}

enum HostChoice {
    Local,
    Ssh { connection_id: String, config_fingerprint: u64 },
}

enum OperationPhase {
    Idle,
    Validating,
    Running,
    Success,
    Failure(String),
}
```

- Treat Add Existing Folder as a Home-page action with a transient picker/operation state; Clone and Create are focused subpages. Keep a compact, read-only host identity row on subpages.
- Store independent drafts for clone and both create modes, but qualify every path draft and validation result with the current `HostChoice`/host generation. Switching hosts increments the generation and clears host-dependent paths, inferred names, repository facts, errors, and readiness.
- Allocate separate monotonic IDs for directory-pick/validation requests and submitted operations. Every completion must match modal instance, page, host key/fingerprint, generation, and request/operation ID before any UI or store mutation. Only the owning completion may clear its phase. This is required by the app quality contract (`.trellis/spec/mt-app/backend/quality-guidelines.md:19-21`, `.trellis/spec/mt-app/backend/quality-guidelines.md:29-45`).
- Snapshot all submitted fields into an immutable operation payload. Do not reread current input entities after background work completes.
- Disable duplicate submit while `Validating` or `Running`. Back/Close may be disabled during irreversible `Running`, but generation checks must still make late completions inert if the entity is closed or replaced.
- Model host status separately from operation status. Local is immediately ready; SSH starts unknown and may become connecting/ready/error from the selected-host probe, browse, or validation. Do not present a saved connection ID/name as authenticated execution-host authority; canonical host identity belongs to the remote runtime (`.trellis/spec/mt-project/backend/worktree-identity-contract.md:56-75`, `.trellis/spec/mt-ssh/backend/remote-runtime-contract.md:64-88`).
- Keep the modal dimensions fixed across `Home`, `Clone`, and `Create`; allow only the inner body to switch/scroll. This follows the Settings modal shell and avoids layout jumps.

### Likely Affected Files

- `crates/mt-app/src/project_onboarding.rs` (new): unified modal entity, pages, host selector, form state, request ownership, render dispatch, and completion handoff.
- `crates/mt-app/src/main.rs`: module wiring for the new surface.
- `crates/mt-app/src/orca_sidebar.rs`: route both project-header actions to the unified modal; likely remove the separate remote option.
- `crates/mt-app/src/first_run.rs`: replace two buttons with the unified entry point.
- `crates/mt-app/src/project_list.rs`: keep legacy/root/group entry points behaviorally aligned through the same API.
- `crates/mt-app/src/modal.rs` and `crates/mt-app/src/remote_project.rs`: retire or reduce old public add-project APIs to compatibility delegates; do not leave independent implementations.
- `crates/mt-app/src/remote_directory_picker.rs`: preserve browser behavior but pass/validate host and owner generation, or return through a typed picker result consumed by the unified entity.
- `crates/mt-app/src/ssh_panel.rs`: reuse existing management flow; optional narrow open-in-add/on-saved API if the host row must auto-select a newly saved connection.
- `crates/mt-app/src/overlay.rs`: use one onboarding kind; remove or alias the separate remote-project kind after all callers migrate.
- `crates/mt-app/src/ui.rs` and possibly shared icon modules: joined segmented control plus icon-only Back/Close/host affordances.
- `crates/mt-app/src/store/projects.rs`, `crates/mt-app/src/store/ssh.rs`, and `crates/mt-app/src/store/identity.rs`: one host-qualified add-or-activate result used by every subflow.
- `crates/mt-app/src/i18n.rs`, `crates/mt-i18n/locales/index.ts`, onboarding/project locale files, and `crates/mt-i18n/src/dict.rs`: unified labels, field errors, phases, operation names, and host status text.

### Related Specs

- `.trellis/tasks/09-04-unified-add-project-flow/prd.md`: source requirements and acceptance criteria.
- `.trellis/spec/mt-app/backend/quality-guidelines.md`: async operation ownership and stale-result rejection.
- `.trellis/spec/mt-app/backend/workbench-identity-contract.md`: exact project/worktree focus handoff.
- `.trellis/spec/mt-app/backend/remote-runtime-reconciliation-contract.md`: generation, path, connection, fingerprint, and epoch fencing (`:43-69`).
- `.trellis/spec/mt-project/backend/worktree-identity-contract.md`: host-owned canonical identity and local/SSH path rules.
- `.trellis/spec/mt-ssh/backend/remote-runtime-contract.md`: authenticated SSH host identity and current-session fencing.
- `.trellis/spec/guides/code-reuse-thinking-guide.md`: extend/extract shared selector and state primitives rather than copying them (`:30-59`, `:108-125`).

### External References

None. This report is based only on the current repository and Trellis contracts.

## Caveats / Not Found

- No current onboarding UI for Clone From URL, Create New Project, `git clone`, or `git init` was found under `crates/mt-app/src` or the onboarding locale sources. Those pages need new UI rather than adaptation of an existing form.
- No connection-scoped status registry exists for saved SSH hosts before a project is registered. The design must decide whether status is lazy/selected-host only or whether a new bounded host probe model is required; probing every saved host from the selector would be new behavior.
- The current `execution_host` abstraction is project-bound (`ProjectExecutionSnapshot`), so it cannot directly represent an arbitrary onboarding host before registration (`crates/mt-app/src/execution_host.rs:76-111`, `crates/mt-app/src/store/identity.rs:277-359`). This UI report does not define the new host-scoped filesystem/Git service boundary.
- No product code, specs, task manifests, or git state were modified by this research.
