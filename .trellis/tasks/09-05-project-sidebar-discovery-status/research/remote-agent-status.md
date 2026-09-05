# Research: Remote Agent Status And Sidebar Indicators

- Query: Why do SSH projects appear to flash continuously immediately after mini-term starts, and why are Agent states recognized incorrectly? Trace detection, ownership, reconciliation, projection, rendering, and focused coverage before planning a correction.
- Scope: mixed, predominantly internal read-only source research; one primary external shell reference.
- Date: 2026-09-05
- Active task: `.trellis/tasks/09-05-project-sidebar-discovery-status`.
- Delivery owner: child `.trellis/tasks/09-05-sidebar-agent-status`; this file is the only research output written.
- Context loading: no saved-hook-output notice or `trellis-hook-injected` marker was supplied. Used the dispatch's explicit active-parent path, read `.trellis/workflow.md`, `trellis-brainstorm`, applicable specs, and the child PRD. Did not query another session's active task or load either implementation/check JSONL manifest.

## Findings

### Executive Assessment

1. **Confirmed transport-probe defect:** `mt-ssh` disables globbing with `set -f`, then enumerates `/proc/[0-9]*` using a glob. On a normal Linux POSIX shell the loop never examines a PID, yet returns a valid `LinuxProc` empty inventory. This directly breaks authenticated Agent recognition. Source: `crates/mt-ssh/src/agent.rs:359`, `:367`, `:368`, `:425`.
2. **The screenshot is not proof of an Agent spinner.** It decoded successfully in this research session, contrary to the child PRD's earlier limitation. It shows `Refreshing`, `last known`, and small yellow worktree dots, with no inline Agent rows. Those dots are consistent with the static catalog-warning branch at `crates/mt-app/src/orca_sidebar.rs:541`, not necessarily the animated Agent-working branch at `:529`. A single frame cannot establish the animation or its cadence.
3. **There are additional code-level reconciliation gaps:** a stale queued PTY event can change legacy UI before the rich registry rejects it; successful empty inventory does not reconcile a heuristic-only Agent; natural PTY exit leaves observation sources registered. These have concrete source evidence, but their occurrence in the user's startup report has not been reproduced.
4. **Do not call the probe defect the complete flashing root cause.** Cold restore initializes pane status to Idle, the rich registry starts empty, and SSH auto-resume is explicitly excluded. The broken probe produces missing evidence, not Working by itself. First correlate the actual moving marker with catalog refresh, PTY fallback, or a genuine working run.

**Planning close-out boundary:** D1 is the highest-confidence correction candidate; D2 is a concrete stale-projection defect with a deterministic regression sequence. D3/D4 are bounded lifecycle regression risks, not evidence that every such path needs changing. H1/H2 remain runtime/presentation hypotheses. Provider expansion, classifier redesign, multiple-provider behavior and new feed-acknowledgement policy are deferred. Research is complete for this planning turn; no additional product question or implementation authorization is implied.

### Screenshot And Narrow Catalog/Status Overlap

Reviewed `/home/leo/.cache/tmp/orca-paste-1788582261130-8a62636b-8a65-498a-9726-4e96fb932681.png` using `file` and `view_image`: PNG, 444 x 192. Visible content includes the `mini-term` project, `Refreshing`, `feat/remote-file-management`, `feat/remote-file-management-v2`, and `last known`. No provider-labelled Agent row is visible. The second row is dimmed; a still image does not identify the cause of that dimming or blinking.

This older image does not supersede the latest textual remote-startup/connection report, which remains the primary reproduction scope in the updated child PRD. Its marker ambiguity is a caveat, not grounds to dismiss that report or move all Agent work to the discovery child.

Three distinct visual sources must not be conflated:

| Source | Input and rendering | Meaning |
| --- | --- | --- |
| Worktree Agent aggregate | `orca_sidebar.rs:486` reads configured `ProjectState.status`; `:528` calls `ui::status_dot` for non-Idle | Working can rotate; AiIdle/Error are static |
| Idle worktree catalog warning | `orca_sidebar.rs:541` checks Idle, no `needs_attention`, and `worktree.last_known`; draws a 6 px warning-colored `div` | Static discovery/freshness indicator, no Agent evidence required |
| Inline Agent row | `orca_sidebar.rs:643`, `:657`, `:684`, `:721`, `:744` | Static activity and connectivity dots plus activity text; no animation call |

Only the catalog-to-status boundary was inspected beyond Agent paths. `worktree_catalog.rs:418` marks a scan in-flight and `:420` changes the existing snapshot to last-known even for an ordinary refresh. `:1314` sets `scan.authoritative=false`; `:1243` projects that to `last_known=true`. An accepted fresh result replaces the snapshot at `:489`. Thus repeated refresh/fresh-completion cycles can toggle an Idle row's warning dot without any Agent event. The refresh text comes from `orca_sidebar.rs:428`. **The code path is proven; repeated cadence as the user's visual cause is a runtime hypothesis.** Broader refresh scheduling, visibility, menus, and configuration remain with the main session/worktree child.

### Files Found

Paths below are repository-relative unless absolute.

| File | Responsibility |
| --- | --- |
| `.trellis/tasks/09-05-sidebar-agent-status/prd.md` | A1 trace, A2 preserve real work/attention, A3 exact ownership, planning-only acceptance |
| `crates/mt-app/src/store/remote_runtime.rs` | Project-owned runtime deferral, generations, epoch checks, Ready/fallback/rebind phases |
| `crates/mt-ssh/src/runtime.rs` | Authenticated host/install identity and canonical repository/worktree facts |
| `crates/mt-ssh/src/pool.rs` | Authenticated pooled sessions, immutable epochs, exact cached-session ownership |
| `crates/mt-app/src/store/identity.rs` | Install authoritative project binding and refuse unsafe live rebind |
| `crates/mt-app/src/store/panes.rs` | SSH launch route, incarnation allocation, hydration and terminal subscriptions |
| `crates/mt-pty/src/ssh.rs` | Exact public route environment and SSH login command |
| `crates/mt-app/src/pane.rs` | PTY byte observation, terminal exit, detach/shutdown |
| `crates/mt-ai/src/detect.rs` | CLI command, visible-line and output-echo recognition |
| `crates/mt-ai/src/tracker.rs` | Heuristic session latch, recent-output timestamps, focus/resize suppression |
| `crates/mt-ai/src/perception.rs` | Input/output observation assembly and monitor wiring |
| `crates/mt-ai/src/monitor.rs` | 500 ms fallback, Hook authority, deduplication, latched stall settling |
| `crates/mt-app/src/ai.rs` | Route-capturing event channel, shared monotonic sequence, remote gate |
| `crates/mt-app/src/main.rs` | UI Agent-event receiver and two-second remote inventory pump |
| `crates/mt-app/src/remote_ssh/mod.rs` | Blocking background facade, bounded inventory retry and current-session validation |
| `crates/mt-ssh/src/agent.rs` | Fixed remote `/proc` probe, provider classification and strict frame parser |
| `crates/mt-app/src/store/remote_agents.rs` | Poll eligibility/fences, empty hysteresis, connectivity and legacy projection |
| `crates/mt-ai/src/agent_runtime.rs` | Run identity, evidence precedence, lifecycle, epoch/sequence rejection |
| `crates/mt-app/src/store/ai.rs` | PTY/Hook event application, legacy pane update and notifications |
| `crates/mt-app/src/store/context.rs` | Exact Agent targets, alias selection, acknowledgement and diagnostics |
| `crates/mt-app/src/agent_activity.rs` | Global feed grouping with activity/connectivity separated |
| `crates/mt-app/src/tree.rs`, `store/mod.rs`, `persist.rs` | Pane status, project aggregation and cold-restore defaults |
| `crates/mt-app/src/orca_sidebar.rs`, `ui.rs` | Status-marker selection and legacy-to-icon adapter |
| `crates/mt-ui/src/icons/status.rs`, `motion.rs` | Working-only spin geometry and shared low-frequency animation pump |
| `crates/mt-app/src/worktree_catalog.rs` | Narrow last-known/refresh marker provenance only |
| `.github/workflows/ci.yml`, `Cargo.toml`, `Cargo.lock` | Existing validation gates and checked source/dependency versions |

### End-To-End Ownership And Data Flow

1. **Cold start and restore.** `store/mod.rs:633` creates project states from saved layouts; `persist.rs:127` uses `PaneState::from_identity`, which initializes Idle/no PTY/no attention (`tree.rs:135`). Saved provider/session identity becomes `resume_pending`, not working evidence (`persist.rs:134`). The Agent registry starts empty (`store/mod.rs:719`). `store/panes.rs:750` and `:776` exclude SSH from automatic Agent resume; SSH terminals do not use the local terminal-host warm-attach path (`:1018`). Do not blame persisted Agent status or generic auto-resume without additional evidence.
2. **Host identity before hydration.** `store/panes.rs:680` enters `defer_remote_hydration`; `store/remote_runtime.rs:117` owns that gate. A runtime request records generation/path/connection/fingerprint, publishes Connecting (`:259`), and validates ownership plus current epoch at completion (`:295`, `:325`). `mt-ssh/src/runtime.rs:189` obtains the remote install ID; `:233` derives execution-host identity from that ID plus authenticated server-key fingerprint, then repository/worktree IDs from canonical paths. Pool epochs are allocated after authentication (`pool.rs:530`). `store/identity.rs:562`, `:620` refuse an identity-changing rebind with live PTYs/documents. Binding installation precedes Ready and deferred hydration (`remote_runtime.rs:348`, `:381`). Research did not execute this bootstrap, which can write remote identity state.
3. **Pane and incarnation.** `store/panes.rs:906` allocates a fresh SSH incarnation, combines execution host/worktree/tab/pane/session/incarnation into `AgentRoute`, and passes those same public values to the launcher (`:920`). `mt-pty/src/ssh.rs:102`, `:146` exports only seven route/protocol fields with individually quoted values. It does not export local PTY IDs, Hook secrets or endpoints remotely. The actual terminal's incarnation is then registered in `terminal_routes` and `AiBridge` (`store/panes.rs:1048`). Subscription callbacks reject incarnation/route reuse (`:1065`).
4. **PTY heuristic path.** Every PTY output batch goes to the emulator and perception (`pane.rs:291`). `perception.rs:95` forwards to `SessionTracker::note_output`. The tracker recognizes input commands and bounded output echoes; it also timestamps every output batch outside focus/resize cooldown, without checking whether bytes represent meaningful work (`tracker.rs:349`, `:373`). The shared three-second recent-output idea is used both by `monitor.rs:206` and `store/remote_agents.rs:547`. The monitor runs every 500 ms with panes present (`monitor.rs:358`), respects per-pane Hook authority, otherwise emits Working/Waiting from a heuristic session latch plus output recency. `StatusEmitter` suppresses identical status/no-cause repeats (`monitor.rs:134`).
5. **PTY event transport and application.** `ai.rs:62` allocates a shared sequence and captures the route before queueing. Remote polls use the same counter (`ai.rs:169`). `main.rs:647` drains events into `store/ai.rs:416`. Exact route equality is checked first, but legacy pane state is updated before registry application (`store/ai.rs:489`, `:505`); see defect D2 below. Provider comes from event, active route, or tracker. Cause/per-pane Hook state selects evidence; the ordinary SSH fallback is `PtyActivity` (`store/ai.rs:334`, `:351`).
6. **Remote poll scheduling.** `main.rs:668` immediately starts the pump and repeats every two seconds (`store/remote_agents.rs:22`). Eligible candidates must be SSH projects with a current terminal route, Ready runtime, matching execution host/worktree and latest authenticated connection epoch (`:247` through `:331`). Each route has one matching in-flight poll (`:369`); request facts include PTY, project/path, generation, connection/fingerprint, route and epoch. Poll-map recreation already seeds prior process evidence from the exact route (`:405`). Non-ready runtime changes connectivity only; repeated identical connectivity/epoch observations are already suppressed (`:153`, `:683`).
7. **Bounded remote inventory.** `remote_ssh/mod.rs:527` runs the blocking facade off the UI thread, uses a five-second probe timeout (`:88`), retries a retryable probe once (`:569`, `:586`), and accepts only the current pool winner/epoch (`:556`). `mt-ssh/src/agent.rs:155` runs a fixed bounded command. Intended discovery matches every route field, then classifies provider and emits PID/start ticks only (`:325`, `:380`, `:423`). Parser rejects malformed/framing/duplicate/oversized rows (`:208`), caps 64 processes and 16 KiB output (`:19`), and sorts by start ticks/PID (`:298`). D1 currently defeats the enumeration before any provider matching.
8. **Inventory reconciliation.** Completion rechecks request/runtime facts and the observed epoch (`remote_agents.rs:438`, `:480`). A nonempty result applies ProcessAttested evidence, using pane-wide recent output for every returned process (`:547`, `:568`). Following prior process evidence, two successful empty inventories are required before retirement (`:197`); a first empty is a race window, not done. Unsupported/failure updates capability/diagnostics/connectivity without semantic completion (`:517`, `:658`). `update_remote_agent_projection` bypasses Hook-enabled panes, updates the legacy tree, and recomputes project status (`:713`).
9. **Rich run ownership.** `AgentRoute` includes all six identities (`agent_runtime.rs:109`); `AgentRunId` is independently allocated (`:303`). Exact process `(pid,start_ticks)` and provider/session are matching evidence, not run IDs (`:533`). Registry rejects duplicate event IDs, invalid sequences/epochs, old epochs and out-of-order matched-run events (`:270`, `:497`, `:612`). Evidence is Hook > ProcessAttested > PTY > history (`:164`). Weak PTY Working/Waiting may refresh process activity, but cannot end it (`:579`); successful absence retires ProcessAttested runs (`:408`). Done/Failed are semantic states, not process-exit tombstones: `is_ended` only includes Interrupted/Exited (`:133`).
10. **Target resolution and UI.** `store/context.rs:310` re-resolves execution host/worktree/tab/pane/session/incarnation, current PTY route and terminal entity before exposing a row; same-route project aliases choose active then stable project ID (`:389`). `agent_activity.rs:54` places only Live Starting/Working in Working; stale/offline activity remains in Recent unless attention takes precedence. In contrast, project summary is the maximum four-state pane value across all panels of that project (`store/mod.rs:213`, `tree.rs:49`), with no connectivity dimension. `orca_sidebar.rs:486` reads that legacy aggregate. Starting/Working/Blocked project to `ai-working`, Waiting/Done to `ai-idle`, Failed to error, Exited/Interrupted/Unknown to idle (`agent_runtime.rs:137`).
11. **Animation is not a status detector.** `ui.rs:1063` maps PaneStatus to StatusKind. Only AiWorking spins (`mt-ui/src/icons/status.rs:122`); rendering consults that boolean at `:276`. Period is 900 ms (`:219`), reduced motion slows rather than disables it. `motion.rs:143`, `:215`, `:262` supplies a shared 100 ms foreground/500 ms background pump and drops consumers no longer rendering animation. Identical store notifications alone do not reset a per-element animation phase. No evidence here establishes an animation-engine defect.

### Concrete Defects And Candidate Causes

#### D1. Proven: Global No-Glob Disables All Linux Agent Enumeration

`build_probe_command` begins with `set -f` (`mt-ssh/src/agent.rs:359`). Its next process iteration is `for proc in /proc/[0-9]*` (`:367`), with no intervening `set +f`. The loop receives the literal pattern, fails `[ -r "$proc/environ" ]`, and continues. The already printed Linux capability and final footer form a valid empty success.

Read-only local checks executed:

```sh
sh -f -c 'printf "%s\n" /proc/[0-9]*'
# Actual stdout: /proc/[0-9]*
sh -f -c 'test -r /proc/[0-9]*/environ'
# Actual exit status: 1
```

The GNU Bash manual documents that `-f` disables filename expansion: [The Set Builtin](https://www.gnu.org/s/bash/manual/html_node/The-Set-Builtin.html). This supports the shell behavior; the source plus local check establishes the defect, not a remote-host reproduction.

Impact: no authenticated process discovery even for correct route environment; diagnostics can misleadingly show LinuxProc/Live/zero processes/no error. A fresh poll does not clear heuristic activity because `had_processes=false` (`remote_agents.rs:207`). Existing attested state, if seeded before this probe or in a test, can be incorrectly retired after two false empty results.

Minimal boundary: correct glob scope in `mt-ssh/src/agent.rs` and add a test that executes the generated command against a controlled process. **Keep glob suppression while splitting untrusted argv/stat text** at `:396` and `:412`; merely deleting all protection can introduce wildcard expansion there. No protocol, dependency, identity or sidebar redesign is needed to correct D1.

#### D2. Proven: Rejected Rich Events Can Still Mutate Legacy Sidebar State

`store/ai.rs:489` writes the pane and `:499` updates project status before `:505` calls `observe_agent_status`; that helper discards the registry outcome at `:359`. Exact route validation alone does not fence event sequence. Registry rejection at `agent_runtime.rs:286` cannot undo the preceding UI/attention/notification mutation.

Deterministic ordering example: queue PTY Working at sequence 10; accept a same-route/epoch inventory Waiting at sequence 11; then drain sequence 10. The pane becomes Working, while the registry keeps Waiting after rejecting OutOfOrder. The next successful inventory may correct it, creating an observable oscillation window. The shared counter exists, but the two source queues have different delivery paths (`ai.rs:62`, `:169`; `main.rs:647`; `remote_agents.rs:430`). **Ordering defect is proven; this interleaving during the report is unverified.**

Minimal boundary: decide acceptance before any legacy/attention/DoneTracker mutation for routed events, then project the accepted authoritative state. Preserve the legacy no-route path and ordinary shell status behavior; do not turn lack of Agent provider into suppression of all terminal events. Inventory projection also should derive from accepted current state, not assume `Ok(Vec<RunId>)` means every observation applied: registry can skip individual ended/out-of-order process observations (`agent_runtime.rs:369`) while app projection uses raw inventory activity (`remote_agents.rs:635`).

#### D3. Proven Reconciliation Gap: Empty Inventory Does Not Clear A Heuristic Latch

With no prior process evidence, every empty inventory is suppressed (`remote_agents.rs:207`), and the suppressed path only marks connectivity (`:647`). Even an applied inventory only retires runs that already carry ProcessAttested process identity (`agent_runtime.rs:410`). Neither remote completion path clears `SessionTracker.ai_sessions`.

Consequences reproducible from the code:

- Typing a recognized but unavailable Agent command latches a session at `tracker.rs:505`; the shell's command-not-found output can be treated as Working, then Waiting forever, despite successive real empty process inventories.
- After a formerly attested process disappears, two empties project Idle, but a still-latched tracker can emit Working on later shell output; with no live run to preserve, a new heuristic run can be created. Later empty polls now have `had_processes=false` and cannot correct it.

This is not permission to clear all heuristic state on a single empty poll. A narrow correction needs an explicit route-owned supported-negative-evidence policy, launch grace/confirmation, and expiry of only the contradicted heuristic session. Unsupported probes, failed reads, reconnects, tmux environment discontinuities and Hook semantics must not be treated as authoritative exits. This policy belongs to the status child before implementation, because current specs explicitly preserve fallback behavior and only define retirement for attested processes.

#### D4. Proven Lifecycle Gap: Natural PTY Exit Does Not Unregister Observers

`pane.rs:662` sets `exited` and emits `PaneEvent::Exited`. `store/ai.rs:274` sets Error and `exited_ptys`, but does not remove the route, remote poll or AiBridge live-pane/tracker state. `AiBridge::remove_pane` is only called by explicit shutdown/detach (`pane.rs:1255`, `:1263`), not the natural-exit branch. Remote eligibility checks the route and project, not `exited_ptys` (`remote_agents.rs:247`); Agent target resolution checks terminal existence, not terminal exit (`store/context.rs:346`).

An SSH client exit can therefore leave the fallback monitor active and a prior rich run visible; delayed monitor/poll projection can overwrite the legacy Error. This is a concrete lifecycle omission, not proof of the startup symptom. Separate **local terminal transport exit** from **remote Agent process exit**: disconnecting SSH must preserve last-known semantic activity with offline/stale connectivity, not manufacture remote Done. Keep GUI-only detach/warm-attach preservation intact.

#### H1. Runtime Hypothesis: Idle TUI Redraws Sustain Working

Outside the existing 800 ms focus/resize cooldown, every output batch refreshes `last_output` (`tracker.rs:373`). Both fallback and remote inventory call recent-output within three seconds. Periodic control-only redraws faster than that can sustain Working; slower redraws can alternate Working/Waiting. Input echo, shell output after an unobserved Agent exit, and delayed SSH resize response can have the same effect. Quiet real work can conversely look Waiting. Those limitations follow from the heuristic, but actual bytes/timing/provider behavior on the user's host are not known. The previous Hook-only stall fix is already latched and explicitly exempts attention (`monitor.rs:251`); extending or bypassing it blindly is not a remote-status correction.

#### H2. Proven Presentation Loss, Runtime Relevance Unverified

The legacy worktree aggregate ignores Agent connectivity and attention detail. Connectivity-only updates intentionally preserve Working (`remote_agents.rs:679`, `agent_runtime.rs:456`), so `orca_sidebar.rs:529` can keep spinning when a rich row is Stale/Offline. Blocked also maps to AiWorking (`agent_runtime.rs:139`), while `needs_attention` at the project level denotes legacy completion and is only used for an Idle fallback marker, not exact pane approval (`store/mod.rs:97`; `orca_sidebar.rs:531`).

Minimal candidate: derive an explicit sidebar presentation from exact-route activity plus connectivity/attention, without rewriting semantic activity to Idle/Done. Preserve the genuine-working animation and distinct waiting/error/attention. Only coordinate the status-marker code with the sibling sidebar task. The existing global feed already demonstrates state/connectivity separation (`agent_activity.rs:54`), so external Orca source comparison was unnecessary.

#### Remote Capability Limit

Default SSH launch does not forward Hook endpoint/secret, and inventory returns process presence/provider only. It cannot distinguish true task completion, a permission dialog, quiet reasoning or an idle prompt with Hook precision. Non-Linux hosts, unreadable `/proc`, or missing inherited route environment can leave fallback necessary. This is a caveat for validation, not a proposal to expand this task into additional provider support.

### Existing Focused Coverage And Current Worktree Safeguards

Reviewed current working-file contents, not a clean checkout. No Git operation was permitted, so **which lines are staged/unstaged/untracked, authorship, and baseline diff attribution are not verified**. The main session must review its dirty diff before dispatching implementation. All current edits were preserved.

| Current coverage | What it establishes and what is missing |
| --- | --- |
| `mt-ssh/src/agent.rs:454`, `:472`, `:489`, `:509` | Parser/framing, route-string presence and bounded exec classification. No generated-script execution, so none catches D1. |
| `store/remote_agents.rs:785` | Independent generation/path/connection/fingerprint/incarnation/epoch request-fence cases. Does not assert stale-event immunity of the actual legacy pane update. |
| `store/remote_agents.rs:851`, `:865` | Two-empty hysteresis and exact-route process evidence on poll-map recreation already exist. Preserve them; do not propose their absence as a new diagnosis. |
| `store/remote_agents.rs:933`, `:995` | No-active-run/unchanged-connectivity suppression and every non-ready phase projection already exist. They do not deduplicate unchanged nonempty inventory. |
| `mt-ai/src/agent_runtime.rs:698`, `:733`, `:779`, `:826`, `:894`, `:921` | Evidence upgrade, PTY cannot end attested process, event/route/epoch fences, process lifecycle, connectivity preservation, legacy vocabulary. No multi-source store/legacy interleaving test or same-provider ambiguity test. |
| `mt-ai/src/tracker.rs:1133`, `:1140`, `:1147` | Current empty-Enter/autosuggestion and empty history-snapshot guards. Preserve these existing changes. These cases require input and do not establish a pure cold-start Agent. |
| `mt-ai/src/tracker.rs:1125`, `:1155`, `:1164`, `:1176`, `:1189` | Preserve history, completion, noninteractive-flag and bounded echo behavior. Missing a non-Agent command producing an Agent-looking output line and supported-empty reconciliation. |
| `mt-ai/src/perception.rs:222`; `tracker.rs:1197` | Existing resize/focus cooldown suppression; no arbitrary periodic idle-redraw discrimination. |
| `mt-ai/src/monitor.rs:395`, `:412`, `:491`, `:593`, `:637`, `:697` | Hook stability, latched stall, attention exemption and deduplicated emissions. Avoid reintroducing memoryless Hook timeout oscillation. |
| `store/ai.rs:859`; `store/context.rs:826`, `:949`, `:978`, `:991`, `:1030` | Captured-route rejection, exact routing/diagnostics, deterministic alias selection and acknowledgement. No natural-exit-to-poll/monitor integration test. |
| `tree.rs:912`, `:1128`; `mt-ui/src/icons/status.rs:312` | Status aggregation, attention separate from status, and only AiWorking spins. Not proof that the right semantic status reaches the icon. |
| `worktree_catalog.rs:1765` | Degraded snapshots retain targets and mark last-known. Routine-refresh visual-marker churn belongs to the main session's catalog investigation. |

### Minimal Correction Boundaries And Order

1. **Resolve which marker is flashing.** Main session should correlate the decoded screenshot with the last-known marker and catalog refresh timeline. This is a coordination dependency, not approval to change discovery/menu/config behavior in the Agent child.
2. **Correct D1 and prove nonempty exact-route discovery.** Keep the patch within `mt-ssh/src/agent.rs` plus focused tests unless a fixture helper is necessary. Preserve fixed bounded framing, quoting, authentication, provider normalization, and PID/start identity. No dependency changes expected.
3. **Add deterministic cross-source tests before expanding correction.** Reproduce D2, D3 and D4 in focused store/perception lifecycle tests; fix only the owner modules demonstrated necessary. `store/ai.rs`, `store/remote_agents.rs` and `pane.rs` are the relevant correction boundaries, not broad project identity/layout rewrites.
4. **Keep any negative-evidence correction bounded to D3.** Preserve two-empty confirmation, launch grace and failed/unsupported-probe behavior. Do not introduce feed-acknowledgement changes or provider-wide redesign in this close-out. Keep acceptance and projection consistent; do not share a mere legacy status string as stronger evidence.
5. **Change sidebar presentation only for a confirmed presentation defect.** A separate animation-eligibility/status projection can preserve last-known semantic Working while displaying offline/stale or attention without a misleading busy spinner. Do not disable `StatusKind::spins`, global motion or all Agent animation to conceal upstream state errors.

The child remains in planning. Further negative-evidence/presentation policy is deferred until the relevant regression and runtime evidence justify it; it is not another product question for this turn. Shared `orca_sidebar.rs` edits must be coordinated with the worktree child, without imposing unrelated tree-order dependencies.

### Deterministic Regression Matrix

All are proposals, not tests added or executed in this session. Use controlled timestamps/fake clock and explicit completion ordering rather than sleeping 3-10 seconds. Current `SessionTracker` stores `Instant` and only has a current-time test hook (`tracker.rs:259`); use module-private timestamp seeding or a narrow testable clock input. Existing monitor tests pass a timeout parameter for deterministic settling (`monitor.rs:251`).

| Test | Stimulus | Required assertion |
| --- | --- | --- |
| Execute actual generated probe | Linux test helper process with exact route env, controlled provider argv0, pipe-based readiness and shutdown | Generated command returns that PID/start identity; fails on current `set -f` defect. Do not merely test a glob substring. |
| Probe isolation/quoting | Change one of seven route fields at a time; mismatching sibling; wildcard-looking non-provider argv/stat data | Only exact route matches; no leaked environment/argv; inner splitting does not glob. |
| Startup without Agent | Restored session identity, fresh SSH shell, banner/control output, no user launch | Idle legacy aggregate; no live Agent manufactured; catalog warning is independently testable. |
| Working to quiet | One exact process; output timestamp recent then older than three seconds | Same run/provider/route remains; Working then Waiting; quiet never means process Exited. |
| Process disappearance | Nonempty, empty, empty at same current route/epoch | First empty retains live activity; second ends the process; same sequence after poll-map eviction still needs confirmation. |
| Failed launch and leftover tracker | Heuristic Agent plus repeated supported empties; then unrelated shell output | Approved grace policy eventually clears only contradicted evidence; no new working Agent from leftover latch. |
| Out-of-order PTY vs inventory | Queue Working seq 10; apply Waiting inventory seq 11; then deliver seq 10 | No legacy/attention/DoneTracker mutation from rejected seq 10; pane, registry and sidebar agree. |
| Natural terminal exit | Keep terminal entity visible after Exited; deliver queued monitor event and poll completion | Exited terminal is not republished live; no overwrite of terminal-exit UI; remote semantic result is not invented. |
| Connectivity gap and recovery | Ready -> Connecting/fallback/rebind-deferred -> Ready; old/new epochs and superseded completions | Last-known semantic activity preserved, connectivity correct, no stale completion mutation, no synthetic Done. |
| Exact ownership | Same path on two hosts; sibling worktrees; recycled incarnation; stale request generation | Distinct targets; old observations cannot change the current route or unrelated sidebar entries. |
| Hook preservation | Hook Blocked/Done/Failed then weak PTY/inventory and a transport gap | Hook semantic authority and attention retained; no timeout or projection silently clears approval. |
| Marker presentation | Idle, Waiting, Working, Blocked/attention, Failed, Working+Stale/Offline, Idle+last-known | Only the approved live-work state animates; catalog marker cannot claim Agent activity. Keep geometry stable. |
| Catalog refresh marker | Known authoritative snapshot -> ordinary refresh -> accepted fresh result, with Agent state fixed Idle | Document/verify intended warning-marker behavior; if it must stay stable, fix catalog/presentation ownership in the sibling scope. |

### Validation Commands And Evidence

Performed: read-only source/spec/test inspection, `file`/`view_image`, two non-writing local shell checks for D1, and primary shell-doc lookup. Created the requested research directory and wrote this file. No Cargo/Rust compilation, tests, app launch, SSH probe, remote process operation, Git command or workflow dispatch was executed.

Proposed focused commands for the implementation/check agent **in GitHub Actions**, after implementation approval, consistent with `.trellis/spec/mt-app/backend/quality-guidelines.md`:

```sh
cargo test --locked -p mt-ssh agent::tests -- --nocapture
cargo test --locked -p mt-ai agent_runtime::tests
cargo test --locked -p mt-ai tracker::tests
cargo test --locked -p mt-ai monitor::tests
cargo test --locked -p mt-ai perception::tests
cargo test --locked -p mt-pty ssh::tests
cargo test --locked -p mt-app --bin mini-term store::remote_agents::tests
cargo test --locked -p mt-app --bin mini-term store::ai::route_tests
cargo test --locked -p mt-app --bin mini-term store::context::tests
cargo test --locked -p mt-app --bin mini-term agent_activity::tests
cargo test --locked -p mt-ui icons::status::tests
```

Add the eventual new store lifecycle/interleaving module filters to that list; existing filters cannot validate nonexistent new tests. This is a native GPUI application, not a browser app, so use native screenshots/video and store diagnostics rather than claiming Playwright exercises it.

Existing broader CI gates are `cargo check --locked --workspace --all-targets` (`.github/workflows/ci.yml:151`), affected-package Clippy (`:161`), `cargo test --locked --workspace --all-targets --no-fail-fast` (`:176`), sidecar locked check/tests (`:154`, `:179`), and Windows MSVC affected-package check (`:214`). Keep both workspace lockfiles unchanged for source-only corrections; any actual mt-ssh dependency change must validate both workspaces. Do not run repository-wide formatting: `CLAUDE.md:37` warns that the baseline is not rustfmt-clean.

For a later authorized native runtime validation, record bounded, non-secret diagnostics per exact route: runtime phase, inventory capability/count/error class, request and current epochs, event sequence/source, provider/process identity, activity/connectivity, legacy pane/project status, output age and cooldown, plus catalog refreshing/last-known flag. `store/context.rs:759` already exposes much of the remote poll diagnostic view. Capture a short startup trace/video without input, then a controlled Agent launch/quiet/exit sequence. Never log raw command lines, prompts, credentials or remote environments. Do not use a real user's remote process as a kill fixture.

### Related Specs

- `.trellis/workflow.md` and `.agents/skills/trellis-brainstorm/SKILL.md`: remain in planning; research evidence and a later review/approval gate, not `task.py start`.
- `.trellis/spec/mt-ai/backend/agent-runtime-contract.md`: exact route/event identity, evidence precedence, process retirement, one-way legacy projection and connectivity separation.
- `.trellis/spec/mt-ssh/backend/remote-agent-inventory-contract.md`: fixed exact-route bounded probe and no raw process data; D1 contradicts intended Linux discovery.
- `.trellis/spec/mt-ssh/backend/remote-runtime-contract.md`: authenticated host identity, immutable epochs and current session ownership.
- `.trellis/spec/mt-app/backend/remote-agent-reconciliation-contract.md`: polling fences, durable empty hysteresis, identical-connectivity suppression, Hook precedence, rollback exact-zero behavior.
- `.trellis/spec/mt-app/backend/remote-runtime-reconciliation-contract.md`: identity before hydration, fallback/rebind and stale completion rules.
- `.trellis/spec/mt-app/backend/workbench-identity-contract.md`: stable pane/session and changing incarnation, live-rebind constraints.
- `.trellis/spec/mt-app/backend/global-agent-activity-contract.md`: exact-run feed, watermark and activity/connectivity contracts.
- `.trellis/spec/mt-app/backend/quality-guidelines.md` and `.trellis/spec/guides/cross-layer-thinking-guide.md`: async-owner checks, pure regression tests, source-to-presentation contracts, CI execution.
- Read package indexes for mt-app/mt-ai/mt-ssh/mt-ui and their quality guidance. Several generic mt-ai/mt-ssh/mt-ui guideline pages remain placeholders; concrete contracts and current code supplied the operative rules.

### External References And Versions

- Primary reference: [GNU Bash Reference Manual, The Set Builtin](https://www.gnu.org/s/bash/manual/html_node/The-Set-Builtin.html), retrieved through search on 2026-09-05; documents `-f` disabling globbing. Direct Open Group POSIX.1-2024 page fetch returned HTTP 403 and the first GNU URL fetch timed out; the official GNU search result supplied the supported reference. No reliance on third-party technical commentary.
- Examined workspace declares mini-term `1.2.2`, Rust edition 2024 / minimum `1.95` (`Cargo.toml:21`). This identifies source metadata, **not the user's running binary**.
- Current lockfile: GPUI `0.2.2` (`Cargo.lock:2660`), gpui-component `0.5.1` (`:2756`), russh `0.61.2` (`:6111`). No library upgrade is proposed.
- No external Orca checkout/source examined. mini-term's existing rich rows/global feed already show the necessary semantic-state/connectivity split.

## Caveats / Not Found

- The exact startup report remains unreproduced; no access to current remote runtime telemetry or running-binary/source correspondence was established. D1 is proven source behavior, not proof that it causes all observed flashing.
- The supplied Agent screenshot is now decoded and suggests a catalog freshness marker. A still frame cannot distinguish repeated appearance/disappearance from a working rotation. This discrepancy is a critical handoff to the main session and may change which child owns the visible symptom.
- Cold restore does not persist Working and default SSH hydration does not auto-resume an Agent. A no-input startup flashing diagnosis must explain where any real Agent evidence was introduced, or identify the marker as catalog state instead.
- Exact local Git dirty-change attribution was intentionally not obtained because this researcher is forbidden from any Git operation. Current guards/tests were reviewed and preserved; do not overwrite them based on older research or assume they were created in this task.
- Focused tests and expensive builds were not run. Existing tests are source evidence of intended coverage, not a current passing result. Proposed commands belong to a later approved implementation/check stage in CI.
- All writes stayed within the requested parent research file/directory. No code, tests, PRDs, specs, manifests, config, task state or sibling research were changed. No historical session logs, private SSH configuration or raw remote environment were read.
