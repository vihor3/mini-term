# Implementation Plan

## 1. Freeze Scope And Evidence

- Persist the exact `0bc6f28..c644ae9` commit and file inventory.
- Separate 93 non-Trellis implementation/release files from task bookkeeping.
- Map every file to one primary review domain and mark cross-layer second-pass paths.
- Read the archived parent/child acceptance criteria and applicable code specs.

## 2. Parallel Read-Only Review

Dispatch five Trellis reviewers in parallel for:

1. catalog/identity/persistence/workbench;
2. terminal host/history/PTY;
3. remote runtime/Agent/GitHub;
4. Orca UI/context/global activity;
5. GitHub Actions/Windows packaging.

Reviewers report evidence-backed candidates only and do not edit. Each must distinguish
introduced defects from baseline observations.

## 3. Central Triage And Reproduction

- Merge duplicate candidates into a task-local findings ledger.
- Inspect every P0-P2 candidate against current source and the introducing diff.
- Add focused failing tests or deterministic reproduction fixtures for confirmed issues.
- Record baseline/not-a-bug rejections so they are not rediscovered during later passes.

## 4. Scoped Fix Batches

- Assign disjoint file ownership for each confirmed fix batch.
- Implement the smallest correction that restores the documented invariant.
- Add regression tests for behavior changes and failure paths.
- Push coherent fix batches and run affected GitHub Actions jobs after each batch;
  do not execute package tests locally or defer all feedback to the final workflow.

## 5. Cross-Layer Robustness Pass

Recheck:

- identity and generation propagation from storage/transport to UI;
- detach/reattach/cold-restore and shutdown cleanup;
- reconnect, retry, timeout, cancellation, and session retirement;
- remote quoting, auth/account isolation, secret redaction, and bounded output;
- rapid worktree switching, tab preview/promotion, sidebar scope, and Agent routing;
- rollback gates and compatibility projections.

## 6. GitHub Actions Quality Gates

Commit and push the task diff, then require GitHub Actions to execute:

```text
changed-task Rust rustfmt --check
cargo metadata/check/test/clippy --locked for the root workspace
cargo metadata/check/test --manifest-path sidecars/Cargo.toml --locked
Windows MSVC checks for mt-pty, mt-terminal-host, mt-app, and sidecars
node staging/ConPTY tests and workflow/script static checks
git diff --check
```

Do not run local `cargo`, `rustc`, `rustfmt`, Clippy, tests, staging scripts,
`makensis`, Docker build/test jobs, or package validation. Local work is limited to
editing, read-only inspection, Git operations, and artifact cleanup.

## 7. Conditional Windows Release

If any application, terminal host, sidecar, ConPTY staging, or installer payload changes,
GitHub Actions must:

- rebuild all Windows release payloads on a Windows runner;
- inject icon/version/manifest resources through the release path;
- stage and verify portable ConPTY;
- build and extract the NSIS installer;
- verify PE machines, resources, required feature markers, and exact payload hashes;
- upload the verified installer and machine-readable validation evidence.

## 8. Finish

- Update relevant specs for newly discovered executable contracts or gotchas.
- Write `validation.md` with findings, fixes, rejected baseline observations, tests, and
  residual platform risks.
- Commit task work in coherent batches without staging unrelated dirty files.
- Archive the task and record the session journal through an isolated commit if existing
  journal changes remain dirty.

## Risk Controls

- Do not mass-format or dependency-upgrade the repository.
- Do not let parallel reviewers edit overlapping files.
- Do not weaken identity/generation checks to make tests pass.
- Do not claim a physical Windows visual smoke test from compile/package checks alone.
- Stop and record evidence rather than broadening into a baseline defect.
