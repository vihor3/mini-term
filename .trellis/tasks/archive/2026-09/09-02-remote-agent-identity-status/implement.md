# Implementation Plan

1. Add `AgentRunId` and `AgentEventId` to `mt-identity` with canonical parsing,
   serde, and random-ID tests.
2. Add the pure `mt-ai::agent_runtime` types, evidence ordering, registry
   reconciliation, missing-process handling, connectivity updates, and legacy
   status projection tests.
3. Enrich `AiBridge` events with captured stable routes; validate them in
   `AppStore`, update the rich registry after the existing local status/session
   path, and expose read-only run snapshots for later UI children.
4. Preallocate SSH legacy terminal incarnations and add fixed, shell-safe remote
   route environment injection through `mt-pty::ssh`.
5. Add `mt-ssh` Linux exact-route process inventory, strict bounded parser, and
   provider/process tests.
6. Add the `remote_ssh` facade and AppStore generation/route/config/epoch-fenced
   polling loop, activity fallback, disconnect behavior, and feature gate.
7. Add integration tests for old-route event rejection, SSH incarnation/env
   equality, stale epoch/generation rejection, and compatibility projection.
8. Update the executable agent-runtime specs in `mt-ai`, `mt-ssh`, and `mt-app`.
9. Run Docker-only rustfmt, affected tests, workspace check, Clippy, Windows
   MSVC checks, diff inspection, then commit and archive the child task.

## Validation Commands

```text
./scripts/docker-ci.sh run cargo fmt --all -- --check
./scripts/docker-ci.sh run cargo test --locked -p mt-identity
./scripts/docker-ci.sh run cargo test --locked -p mt-ai
./scripts/docker-ci.sh run cargo test --locked -p mt-pty ssh
./scripts/docker-ci.sh run cargo test --locked -p mt-ssh agent
./scripts/docker-ci.sh run cargo test --locked -p mt-app agent
./scripts/docker-ci.sh run cargo check --locked --workspace --all-targets
./scripts/docker-ci.sh run cargo clippy --locked --workspace --all-targets --no-deps
cargo xwin check --locked --target x86_64-pc-windows-msvc -p mt-identity -p mt-ai -p mt-pty -p mt-ssh -p mt-app
```

## Risk And Rollback Points

- Preserve the existing Hook endpoint and notification code as the compatibility
  boundary; rich-state failures must not suppress local status events.
- Keep all remote output bounded and schema-only. Any parser ambiguity fails
  closed as unsupported/error rather than clearing a live run.
- The archived remote-runtime-foundation commits `40714bf` and `56f2eba` are
  the source rollback point. `MINI_TERM_REMOTE_AGENT_STATUS=0` restores the
  previous SSH behavior without removing stable identities or layouts.
