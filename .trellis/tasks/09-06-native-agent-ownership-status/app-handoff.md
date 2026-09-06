# App Integration Handoff

Status: app source ready for combined Agent review; NOT a passed quality gate.
All new Agent changes remain unstaged. Main owns staging, Actions, specs, commits,
and exact-artifact acceptance. No local build, test, probe, formatting, generator,
whitespace check, native launch, credential probe, or automated verification ran.

## Changes And Files

- `worktree_catalog.rs`: manual queued/in-flight progress is independent of scan
  authority. Automatic healthy scans stay quiet; registration/revision fences,
  fixed sidebar progress lane, errors, offline state and last-known rows remain.
- `store/remote_agents.rs`: schedule all registered nonexited terminals, including
  background projects/worktrees. Process observations always carry Unknown;
  output recency is never assigned to processes. Existing route/incarnation,
  generation/source/epoch guards, two-empty retirement and observer teardown stay.
- `pane.rs`, `store/panes.rs`: original PTY-output timestamps accompany actual
  OSC 0/2 title changes through the captured exact route. The existing pinned VTE
  parser handles fragmentation; retained emulator/config/resize titles cannot
  renew activity. Repeated title text is deduplicated. Normal terminal responses,
  input, exit, close, owner hydration and flat navigation contracts are unchanged.
- `store/remote_agents.rs`, `store/context.rs`: titles require two accepted unique
  foreground samples bracketing capture, the exact process/provider/run/source,
  a preceding sample within 4 seconds, and original capture within the lower
  15-second freshness window. Semantic acceptance precedes projection effects;
  Hook remains stronger. Reconnect, PID replacement and ambiguity reject capture.
- `store/context.rs`, `session_panel.rs`, `remote_ssh/sessions.rs`: display names
  prefer explicit user title, exact owned provider-session metadata, valid owned
  live title, then bounded provider/terminal plus short identity. Session reads
  capture owners before I/O and revalidate source afterward; SSH cache keys add
  endpoint fingerprint/epoch. No newest-history/provider/cwd-only borrowing or
  history-created liveness. Sidebar, Runtime and top tabs share title facts;
  independent runs get separate Runtime rows, not split/group UI.
- `store/ai.rs`, `pane_actions.rs`, `title_bar.rs`: Unknown and retained Done/Failed
  preserve actual Agent liveness/provider/session metadata. Exact close warnings
  and live-session lookup no longer require legacy Working/Waiting status. Fork
  continues requiring an exact captured session and pending-source checks.
- `orca_sidebar.rs`, `agent_activity.rs`, `session_panel.rs`, `store/context.rs`:
  Unknown is steady; stale work is last-known/Recent, not live animation. Top-tab
  display also suppresses stale/disconnected work without changing process life.
- `store/mod.rs`, `store/projects.rs`: initialize the two owned title maps only,
  aside from the separately requested navigation Clippy repair below.

## Requested CI Repairs

- Both earlier `store/panes.rs` relocation repairs are present: the missing
  `super::{AppStore, ProjectState, TerminalJumpTarget}` import follows `super::pure`;
  lifecycle fixture declares `states` before assigning `request.aliases`.
  Main incorporated those separately; the remaining Agent hunk is title routing.
- Actions 33996331503 / navigation SHA 4947f55: replaced the sidebar redundant
  closure with `force_refresh_global`, moved the entire `flat_terminal_tests`
  module after all production items, and removed the `to_value` needless borrow.
  No assertions/navigation behavior changed, no blanket allow, no staging here.
  That navigation compile result is not validation of the new Agent source.

## Lower-Layer Contract And Open Gates

- Consumes stable `RemoteAgentProcess.foreground`, Unknown inventory inputs,
  `observe_semantic`, `activity_freshness`, and `AmbiguousRun`/`StrongerEvidence`.
  Lower review now preserves accepted presentation event ID/time on unchanged
  weak polls while sequence ordering advances. App renders/acknowledges those
  accepted fields, never the inventory event ID; no duplicate app watermark.
  Identical connectivity is filtered before app allocates or emits a new event.
- Lower same-provider unbound-Hook versus process correlation follow-up is still
  pending at this handoff. Do not claim complete independent same-provider runs
  until its review report and combined Actions regressions pass.
- `mt-ai/monitor.rs::settle_stalled_ai` still mutates HookState from silence.
  App treats synthetic Stall/StallExit as weak, not Hook, and rejects ambiguous
  generic routes before side effects. Lower owner/main must resolve the remaining
  monitor silence inference; this slice did not edit lower files.
- Initial titles before first process attestation are not promoted later. Local
  terminals without attested foreground use exact Hook/session facts or Unknown;
  no local transcript/output heuristic was added. Provider-session title metadata
  arrives through existing Sessions scans, not an independent history poller.

## Authored Tests, UNRUN

- Production-helper regressions cover quiet/manual refresh coalescing and
  authority, background registered selection, process-only Unknown, retirement
  and route/source/epoch rejection, fragmented real OSC capture and nonrenewal,
  foreground bracketing/PID reuse/expiry, exact title identity and bounded fallback,
  Unknown liveness, stale top-tab/feed/sidebar display, and weak-vs-Hook priority.
- Waiting/Blocked/Done/Failed acknowledgement regression checks repeated process
  inventory retains event ID/receipt, stays read, and remains alive. Cross-crate
  tracker fixtures use public `track_input_with_line_snapshot`, not private input.
- Main must run combined exact-SHA Linux tests/Clippy/format and Windows build/
  packaging in Actions, then inspect the matching native artifact for quiet
  cadence, background owned terminals, genuine work/permission/stop/exit,
  duplicate SSH labels, freshness expiry and flat exact-target close/fork/resume.
