# Remote Agent Status Implementation Plan

## Gate and Dependencies

- The user approved the final complete parent summary on 2026-09-05. The
  visibility implementation has handed off to static review; begin this
  child's disjoint process/state files and wait for the explicit sidebar
  handoff before changing `orca_sidebar.rs`.
- Use Trellis implement/check agents with native context injection and curated
  manifests; fallback loading follows each role's protocol.
- Process/event tests are independent of the worktree child. Execute shared
  sidebar changes after that child's integration to avoid overlapping edits.
  Parent startup acceptance explicitly waits for both children.
- Preserve existing user changes, guard tests, epoch fencing, and the remote
  feature rollback. Compiler/test/generated-code gates are CI-only.

## Ordered Checklist

- [x] Read final PRD/design/research and capture the current scoped baseline.
  Keep confirmed defects separate from runtime hypotheses.
- [x] Add the generated-probe execution fixture and negative route/shell-safety
  tests; correct only trusted enumeration glob scope.
- [x] Add deterministic stale PTY versus inventory ordering coverage. Move
  routed Agent acceptance before all legacy/attention/completion side effects,
  preserving ordinary shell and no-route behavior.
- [x] Project accepted registry state after both event and inventory paths;
  cover Hook priority, ignored individual observations, and multiple processes.
- [x] Add confirmed-exit then shell-output regression. Clear contradicted weak
  tracking only after last attested-run retirement with no stronger live owner;
  preserve two-empty hysteresis and unverified fallback behavior.
- [x] Add natural-PTY-exit plus delayed-monitor/poll regression. Make local
  observer/poll teardown idempotent, while retaining disconnected remote
  semantic state and existing explicit detach/warm-attach contracts.
- [x] Add pure sidebar activity/connectivity/attention presentation and tests
  for live work, steady waiting/error/attention, stale work, and no evidence.
  Coordinate the status lane with the already integrated worktree changes.

The implementation agent handed off on 2026-09-05. Checked items denote
implementation and regression source only, not executed validation. Independent
static review is complete. Actions verification passed for `1ee49b8`; native
evidence remains pending. See the parent validation record.
- [x] Add bounded diagnostics only where existing views cannot prove the
  startup acceptance. Never log prompts, argv, environments, or credentials.
- [x] Run the Trellis reviewer and update relevant accepted-projection,
  confirmed-retirement, and observer-lifecycle contracts from verified changes.
- [x] Obtain exact-product CI and an Actions-produced native acceptance build.
- [ ] Obtain native startup/quiet/exit/reconnect evidence; report residual
  unsupported/heuristic limitations honestly.

The existing runtime/context diagnostic views retain the necessary ownership,
capability, activity, and connectivity facts; no new broad logging was added.
Static review and the Hook-owner correction are complete. The production
`retire_terminal_polling` helper is now covered by regression source, but full
Pane/AppStore event-loop and native delivery remain unverified. No check was
executed locally. Complete exact-product CI and Windows packaging passed for
`1ee49b8`; native delivery remains pending.

## Validation

Local source/diff review only, not execution of CI checks:

```sh
git diff --stat
```

Focused commands in GitHub Actions after approval:

```sh
cargo test --locked -p mt-ssh agent::tests
cargo test --locked -p mt-ai agent_runtime::tests
cargo test --locked -p mt-ai tracker::tests
cargo test --locked -p mt-ai monitor::tests
cargo test --locked -p mt-pty ssh::tests
cargo test --locked -p mt-app --bin mini-term store::remote_agents::tests
cargo test --locked -p mt-app --bin mini-term store::ai::route_tests
cargo test --locked -p mt-app --bin mini-term ai::tests
cargo test --locked -p mt-app --bin mini-term orca_sidebar::status_tests
cargo test --locked -p mt-app --bin mini-term store::context::tests
cargo test --locked -p mt-ui icons::status::tests
```

Require the complete
existing CI workflow: locked main/sidecar checks and tests, affected Clippy,
changed-line formatting, i18n, and Windows MSVC. A source-only mt-ssh change
still requires both workspaces because sidecars share it.

This is a hard user constraint for every agent: all CI/build/test/fixture,
lint/format/whitespace, generation, packaging, and automated native execution
is Actions-only. A one-off local/SSH/container reproduction is not an exception.

Automated native evidence and fixture processes run in Actions. Any manual
acceptance uses its produced artifact. Cases: no-input startup on a known remote project; controlled Agent
launch, quiet interval, process exit, later shell output, and reconnect; no
stale Working resurrection or repeated catalog-warning churn. Use only
explicitly started disposable fixtures for process termination. Do not report
the user's original symptom reproduced without an actual trace.

## Risk and Rollback

Highest-risk boundaries are stale acceptance before legacy projection,
multi-process Hook precedence, observer teardown versus retained remote
semantics, and shell glob scope. Do not add universal heuristic expiry,
arbitrary provider scanning, or unread-feed rewrites under this task without
returning to planning. Existing feature-disable value `0` must still work.
Parent integration combines this evidence with the worktree child's result.
