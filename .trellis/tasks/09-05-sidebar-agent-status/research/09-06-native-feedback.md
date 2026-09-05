# Research: Native Feedback, Items 1/2/3/16

- Query: Explain refresh flashing, false Working reports, discovery scope, and missing runtime titles; compare local Orca.
- Scope: Internal, source-only planning checkpoint. No implementation approval or requirements changes.
- Date: 2026-09-06

## Findings

### Evidence Matrix: Files Found and Code Patterns

Mini-term paths are relative to `/home/leo/mini-term`.

| Item | Confirmed facts and source anchors | Remaining unknown |
| --- | --- | --- |
| 1. Header refresh | `crates/mt-app/src/orca_sidebar.rs:564` conditionally inserts a **static catalog-refresh glyph**, not an Agent spinner. `crates/mt-app/src/worktree_catalog.rs:28` sets 10s remote polling; `:262` polls while focused; `:1209` projects in-flight state and `:470` clears it. Startup/focus regain also refresh (`:253`, `:286`). Appearance/disappearance can therefore recur with zero Agents. | Actual flashing cadence and whether the reported glyph follows these scans, retries, or another indicator. |
| 2. Three Working rows | `crates/mt-app/src/store/remote_agents.rs:23` sets 2s polling and a 3s activity window. At `:592`, recent **terminal-wide** output means Working, otherwise Waiting; `:613` assigns that same activity to every matched process. `crates/mt-ai/src/tracker.rs:369` timestamps output outside resize/focus cooldown, without per-process semantic attribution. Distinct processes can create distinct runs (`crates/mt-ai/src/agent_runtime.rs:368`). Sidebar prints accepted activity (`crates/mt-app/src/orca_sidebar.rs:879`). Presence alone does not imply Working, but unrelated same-terminal output can make a quiet process Working. | The three rows' evidence, process identities/lineage, output timing, and connectivity. They are not proven independent conversations or genuinely working turns. |
| 3. Device-wide appearance | `crates/mt-ssh/src/agent.rs:325` requires protocol plus exact host/worktree/tab/pane/session/incarnation env matches; `:369` enumerates `/proc`, then `:383` filters before provider classification. Thus enumeration is host-wide, **acceptance is route-filtered**. Scheduling starts from registered non-exited terminals across open projects (`crates/mt-app/src/store/remote_agents.rs:286`) with host/worktree/epoch gates (`:348`). `crates/mt-pty/src/ssh.rs:146` injects the route into the login shell: descendants retaining it qualify. The probe checks neither PPID nor TTY/foreground/cwd; stat supplies start ticks (`crates/mt-ssh/src/agent.rs:412`). | Whether the reported rows are descendants, launcher/native-child duplicates, copied-env processes, or unrelated. No live process tree was inspected. |
| 16. Runtime titles | `crates/mt-app/src/session_panel.rs:1720` builds Runtime rows from terminal diagnostics, not history. It renders `diagnostic.pane_label` (`:1210`, `:1273`): pane custom name, else SSH connection label, else shell (`crates/mt-app/src/store/ssh.rs:186`), not conversation/OSC title. Diagnostics select one newest active run per pane (`crates/mt-app/src/store/context.rs:755`; `crates/mt-ai/src/agent_runtime.rs:502`), whereas sidebar can show multiple runs. | Literal blank versus generic/repeated/clipped labels; availability of exact session identity and usable title metadata in the tested artifact. |

Two important existing boundaries:

- **Accepted state:** `crates/mt-app/src/store/ai.rs:572` accepts before legacy/attention effects; `:126` rejects ignored outcomes and `:73` aggregates accepted runs. `crates/mt-ai/src/agent_runtime.rs:655` preserves stronger evidence, allowing weak PTY Working/Waiting to refresh process-attested activity. This prevents stale-event projection but does not make recency semantic truth. Inventory uses separate SSH exec, not PTY output (`crates/mt-ssh/src/agent.rs:155`).
- **Display scope:** `crates/mt-app/src/store/context.rs:310` requires an exact current terminal route/entity to resolve a run; sidebar groups those targets by configured project (`crates/mt-app/src/orca_sidebar.rs:1018`, `:1063`). History matches exact provider/session ID only (`crates/mt-app/src/session_panel.rs:326`); process-created runs lack that ID (`crates/mt-ai/src/agent_runtime.rs:408`). History does not supply the Runtime label.

### Orca Reference

External comparison uses only local `/home/leo/orca`; `package.json:3` reports `1.4.178-rc.2`. Paths below are relative to that checkout.

- **State:** `src/main/runtime/runtime-terminal-agent-status-query.ts:64` combines explicit state, title/permission evidence, and foreground checks; process-only evidence returns `isRunningAgent` with `status: null` (`:127`), not Working. Sidebar uses pane-scoped explicit state with freshness decay and title fallback (`src/renderer/src/components/sidebar/worktree-agent-rows.ts:167`, `:201`); fallback requires live PTY membership (`worktree-title-derived-agent-rows.ts:69`).
- **Scope:** SSH inspection addresses a managed PTY (`src/main/providers/ssh-pty-provider.ts:263`). Relay POSIX inventory roots at managed PTY PIDs (`src/relay/pty-handler.ts:2388`); `src/main/providers/agent-foreground-process-batch.ts:70` traverses children and `:117` filters foreground process group. Unlike mini-term, this supplies lineage/foreground correlation. Orca also permits explicit child rows and worktree-attributed orchestration workers without a visible tab (`src/renderer/src/components/sidebar/worktree-agent-rows.ts:187`, `:203`), so its UI is not strictly visible-terminals-only.
- **Names:** `src/renderer/src/components/sidebar/worktree-card-compact-agent-row.tsx:32` prefers conversation name, then task/prompt text, then state. `src/shared/agent-row-conversation-name.ts:113` resolves manual/semantic/generated/live names; the split-pane caller reads the owning pane and excludes child borrowing (`src/renderer/src/components/dashboard/use-agent-row-conversation-name.ts:34`, `:45`). Mini-term's Runtime projection lacks this richer title selection.

### Recommended Boundaries and Actions-Only Cases

These are planning suggestions, not approved changes or executed checks.

| Item | Boundary | Focused regression in GitHub Actions |
| --- | --- | --- |
| 1 | Keep routine catalog progress separate from Agent state; consider quiet automatic refresh without weakening errors/registration fences. | Zero-Agent SSH terminal across repeated catalog scans, manual refresh, and failure/retry; inspect header presentation. |
| 2 | Preserve evidence precedence; distinguish process liveness, logical run identity, and activity. Do not fix by changing only a timeout. | Multiple matched processes with output from only one, quiet prompt, shell output/redraw, genuine work, and stronger Waiting/Blocked/Done. |
| 3 | Preserve exact routes; explicitly decide descendant/background inclusion and launcher deduplication from positive lineage evidence, not cwd/provider guesses. | Sibling panes/worktrees, old incarnation, unrelated provider, inherited descendant, wrapper/native child; separate enumeration from acceptance. |
| 16 | Titles must belong to the exact pane/run; retain a distinguishable fallback and never infer liveness/title ownership from latest history. | Duplicate SSH labels, renamed/unnamed panes, multiple same-pane runs, exact/missing/mismatched session identity, split-pane titles. |

Related specs: `.trellis/spec/mt-app/backend/navigation-catalog-contract.md`, `.trellis/spec/mt-app/backend/remote-agent-reconciliation-contract.md`, `.trellis/spec/mt-app/backend/global-agent-activity-contract.md`, `.trellis/spec/mt-ai/backend/agent-runtime-contract.md`, `.trellis/spec/mt-ssh/backend/remote-agent-inventory-contract.md`.

## Caveats / Not Found

- **No runtime reproduction, automated verification, probes, Git operations, or Actions dispatch.** Source and existing regression code were read only; all future execution is Actions-only.
- Baseline `2e6660e` and tested product `1ee49b8` are dispatch-provided labels. Anchors describe the current shared worktree, not a verified commit diff or artifact correspondence. No screenshot-derived scope/cadence inference.
- Only this research note was written. Product/spec/requirements and existing edits remain untouched; main owns expanded-task consent.
