# Lower-Layer API Handoff

Status: lower-layer source implementation complete, pending independent review
and Actions. No builds, probes, tests, formatting, or automated checks have run
locally. Main owns Actions and acceptance.

## Stable App Integration

- `RemoteAgentRoute`, `inspect_remote_agents`, and `RemoteAgentInventory` keep
  their existing signatures. `RemoteAgentProcess` adds `foreground: bool`;
  update struct literals in application fixtures. It is a logical owned CLI,
  not every provider-looking descendant. The private wire frame is v2; v1
  captures lack ownership proof and are rejected, not treated as empty.
- Root PID/start ticks/TTY are captured remotely by `mt-pty` before login-shell
  exec. No new `RemoteTerminalEnv` fields or app-side launch values are needed.
  Legacy terminals without these facts yield Unsupported until relaunched.
- `AgentProcessObservation.activity` remains for source compatibility but is
  ignored. Pass `AgentActivity::Unknown` from inventory. Direct
  `AgentObservation` with ProcessAttested evidence also proves only liveness;
  generic PtyActivity non-ended observations normalize to Unknown. Neither
  can replace accepted Hook or explicitly owned semantic activity.
- `AgentRuntimeRegistry::observe_semantic(AgentSemanticObservation)` is the
  non-Hook semantic boundary. Fields: `event_id`, `run_id`, `route`, `provider`,
  `owner`, `sequence`, `connection_epoch: Option<u64>`, `activity`,
  `observed_at_unix_ms`, `received_at_unix_ms`.
- `AgentSemanticOwner::ForegroundProcess(AgentProcessIdentity)` requires the
  exact current attested process. Only call it for a fresh pane title captured
  with a unique foreground process from accepted inventory. Never pick the
  newest run or apply one pane title to multiple matching processes.
- `AgentSemanticOwner::ProviderSession(String)` requires the exact already
  bound normalized provider/session, not a history-derived guess. This API
  never creates a run or supplies a previously unknown session identity.
- Semantic acceptance requires the exact route/provider/run/identity/epoch,
  LiveConfirmed process evidence and Live connectivity. Hook remains stronger.
  Unknown, process exit, stale/future timestamps, and repeated original capture
  timestamps are rejected. `AGENT_SEMANTIC_MAX_AGE_MS` is 15,000; use the original
  observation time, never the latest poll time.
- The timestamp must also be at or after `process_owner_since`: first process
  attestation, a changed exact process/provider, or an accepted new connection
  epoch. Old-epoch title state remains last-known/stale even if its wall-clock
  age is less than 15 seconds. Repeated original title timestamps are rejected;
  same text alone is not a newly captured state, and polling must not re-stamp it.
- `activity_freshness(&AgentRunId, now_unix_ms)` returns
  `AgentActivityFreshness::{Unknown,Fresh,Stale}` independently of process
  liveness/connectivity. Expiry preserves the last-known activity; presentation
  must suppress live-work animation for Stale semantics. Hook semantics retain
  their existing authority and do not decay on this title-observation timer.
- A conservative `activity_from_owned_title(&AgentProvider, &str)` decoder is
  available for native provider title formats. Codex without explicit title
  semantics remains unknown and uses existing Hook/session observations.
- Keep acceptance before legacy/attention/unread effects. Inventory does not
  manufacture Waiting/Done, and generic PTY recency does not mean Working.
  Tracker input, usage submission, resize and IME APIs remain unchanged.
- Local no-Hook user-input detection still creates live-confirmed Unknown weak
  runs. A true later launch is admitted after retained retirement watermarks;
  queued older weak events and inventory cannot create replacement runs.
- Cross-crate fixtures that formerly seeded ProcessAttested Working/Waiting
  should first attest the process (expect Unknown), then submit a fresh exact
  `observe_semantic` event, or seed Hook evidence when testing Hook semantics.
  Retain tests proving inventory/activity compatibility fields are ignored.
  `AgentProcessObservation.activity` has not been removed.
- Semantic session ownership must be unique on the exact route/provider; IDs
  are nonempty, control-free and capped at 512 bytes. Ambiguous unbound Hook or
  PTY observations reject with `AmbiguousRun`. Zero PID/start-tick identities
  reject with `InvalidProcess`. Stale whole-route inventory is rejected before
  it can allocate an independent process after a later retirement watermark.

## Changed Files

- `crates/mt-ai/src/agent_runtime.rs`: weak/process normalization, exact semantic
  acceptance/freshness, identity/ordering gates, regression tests.
- `crates/mt-ai/src/agent_semantics.rs`: bounded native title decoder and tests.
- `crates/mt-ai/src/lib.rs`: re-export the semantic API and decoder.
- `crates/mt-pty/src/ssh.rs`: remote managed-root capture before login exec and
  launch-string regression assertions. Existing route-less launch is unchanged.
- `crates/mt-ssh/src/agent.rs`: ancestry/TTY/session/foreground attestation,
  bounded launcher normalization, v2 framing/parser and inventory regressions.
- `crates/mt-ssh/src/agent_probe_tests.py`: disposable Linux Actions fixture,
  included only by Rust tests. This is not a runtime dependency or installed
  remote script. Production probing uses POSIX sh plus head/tr/readlink/awk.
- This handoff, explicitly requested by main. No other task metadata or specs.

## Coordination

The implementer tool list contains no `send_input`/agent messaging tool. Main
should forward this file to Rawls (`01a07379-8615-7cd1-a4a2-68701d4cbb35`).
Only lower-layer source files and this explicitly requested handoff are owned
here; no mt-app, workflow, spec, task status, commit, or push changes.

## Authored, Not Run

- Native title markers for working/waiting/approval; arbitrary title, transcript,
  redraw, wrong provider and oversized/control-containing title rejection.
- Process-only/PTY Unknown; accepted Working/Waiting/Blocked/Done/Failed surviving
  weak output and inventories; semantic expiry without liveness loss; exact
  route/provider/process/session/epoch/freshness rejection; Hook priority.
- Independent runs, duplicate sessions, old inventory and weak-retirement
  watermarks, a genuine later launch, local no-Hook input liveness via the public
  tracker API, invalid process identities, ended semantic rejection, reconnect
  and pre-owner title rejection.
- Generated-command fixtures cover all five native providers, each mismatched
  route field, a copied full route/root environment on an external sibling with
  the same controlling TTY/session/cwd, launcher/native collapse versus native
  independent descendants and multiple launcher children, background process
  groups, helper/wildcard argv, repeated empty inventories after owned exit,
  missing root/TTY/tools, and old root start ticks.
- Parser tests cover v1 rejection, v2 supported/unsupported frames, duplicate
  identities, missing/extra/invalid foreground fields, malformed framing,
  process/output caps, and uncertain/truncated SSH captures.
- Performed only source inspection and read-only scoped Git diff/status review.
  No automatic verification, generated probe, test helper, toolchain, native app,
  commit, or push execution. Nothing is marked accepted or quality-gate passed.

## Remaining Gates and Caveats

- Main must run the existing Actions Linux tests/Clippy/format gates and Windows
  target compilation/package gate for the exact integrated product SHA. The new
  process fixtures assert `GITHUB_ACTIONS=true`, use a disposable session/PTY and
  only explicitly spawned processes, and need the runner's Python 3 for fixture
  orchestration. They have not executed yet.
- Legacy remote terminals without root facts remain Unsupported until a genuine
  relaunch. Failed/missing ownership probes never imply successful absence.
- Unknown/custom interpreter entrypoints and detached/new-session processes are
  not guessed into a conversation. Recognized launchers collapse only one direct
  same-provider native child in the same process group; multiple children remain
  distinct. PID/start, full route, TTY/session and final root/foreground checks
  remain required, and large/incomplete/raced captures fail closed.
- Initial titles emitted before the first exact process attestation are not
  retrospectively promoted. A later genuine title observation or bound session
  event is required. Providers with no reliable native semantics remain Unknown.
- No UI/catalog changes or background-project scheduler changes are owned here.
  Rawls owns integration, accepted projection, title-event capture and native UI
  acceptance. Main owns the successor specs. The current mt-ai spec was read;
  public semantic signatures still match it. Add the unique/512-byte session and
  InvalidProcess details there if useful; do not restore the old PTY exception.
