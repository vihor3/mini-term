# Owned Agent Activity and Titles

## Ownership and Evidence

Start from registered non-exited Mini-Term terminals, across all worktrees,
not the currently visible worktree alone. Preserve exact host/worktree/internal
tab/pane/session/incarnation and SSH connection fingerprint/epoch fences. Flat
visual terminal tabs must not become a new runtime route authority.

Keep three independent facts: terminal ownership, Agent process liveness, and
task activity. A host process enumeration may inspect a bounded `/proc` inventory,
but only processes positively tied to a managed terminal's exact route and
process lineage are candidates. Inspect parent/start/terminal/foreground facts
inside `mt-ssh`; export only bounded identity/classification facts, not raw
environment/argv. A copied route or same cwd/provider is insufficient ownership.

Normalize a launcher/native-child chain to one logical CLI run only with positive
lineage evidence. Do not collapse independent runs merely because they have the
same provider or terminal. Background *terminals* remain eligible regardless of
which project is visible; helper processes do not each become conversation rows.

## Activity Reconciliation

- Keep Hook/session-owned semantic evidence strongest, with accepted-event
  ordering before all legacy status, attention, and notification effects.
- Reuse provider-aware semantic/title/permission and foreground evidence paths,
  studying the recorded Orca implementations rather than copying its broader
  orchestration scope or introducing broad transcript regex guesses.
- Process-only evidence establishes detected/running liveness with unknown task
  activity; it does not mean Working or Waiting. Generic terminal-output recency
  cannot set every matching process to Working or replace stronger Waiting.
- A semantic observation must identify its owning run/pane and freshness. Stale
  or ambiguous observations preserve last-known semantics with the appropriate
  stale/unknown presentation, not manufactured Done/Working.
- Preserve two-successful-empty disappearance confirmation, stale-event fences,
  accepted weak retirement, and idempotent terminal-exit observer teardown.
  Unsupported probes/disconnection are not proof that a task finished.

The current PTY-recency clauses in `mt-ai` and `mt-app` specs require a narrow
successor contract after implementation. Do not leave competing rules in the
specs or patch only a polling timeout while retaining the false inference.

## Codex Live Terminal Evidence Follow-up

The 2026-09-07 screenshot exposes a missing semantic producer, not an incorrect
sidebar label: Codex conversation titles carry no state and current remote
integration only decodes title markers. Add bounded provider-specific evidence
from the live terminal's active Codex status/composer region, parsed from the
existing VT emulator rather than scanning raw escape bytes or scrollback.

Expected product boundary: `mt-ai` owns the pure provider decoder; `mt-terminal`
may expose a bounded current-screen read if existing APIs cannot capture it
atomically; `mt-app` pane/output and remote polling own original capture time,
route, foreground PID/start ticks, connection epoch and accepted projection.
Retain Hook priority and confirm ownership with a sample scheduled after capture.
Scrolling, restored snapshots, unrelated redraws and cached old text are not new
semantic events. Continuous Working updates must not starve confirmation; a
newer contradictory state must not publish an obsolete pending Working sample.

Do not modify layout, process discovery scope, Hook credentials, legacy weak
lifecycle reconciliation or runtime identities. Add focused production-path
regressions for the screenshot, idle composer, old/quoted status text, foreground
replacement, delayed inventory, reconnect and strong Hook precedence. Execute
all checks and package generation only in existing GitHub Actions workflows.

## Catalog Presentation

Track automatic inventory refresh separately from user-requested refresh.
Successful automatic scans do not insert/remove the header progress glyph on
every poll. Preserve stable icon geometry, meaningful manual feedback, last-known
data, and real error/connectivity indicators. Catalog scans never create Agent
activity or unread events.

## Runtime Titles

Build a source-qualified display title for the exact terminal/run: explicit
user name first, exact provider-session conversation metadata or valid pane-owned
live title next, then a bounded provider/terminal fallback that distinguishes
duplicate SSH labels. Do not generate titles through a new paid model call or
borrow the newest history item. An unavailable title must not alter liveness.
Sidebar, Sessions Runtime, and top tabs use shared ownership facts while keeping
their appropriate compact layouts.

## Risks

Providers without semantic hooks/titles may remain unknown. This is intentional
precision, not a reason to synthesize Working. Remote process ancestry and CLI
launcher shapes vary; fixtures must cover unknown and unsupported cases. Native
symptom cadence and the three screenshot rows remain unverified until artifact
acceptance; old passing CI is not evidence that this task is fixed.
