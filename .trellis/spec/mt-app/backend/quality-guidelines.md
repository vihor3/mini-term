# Quality Guidelines

> Code quality standards for backend development.

---

## Overview

<!--
Document your project's quality standards here.

Questions to answer:
- What patterns are forbidden?
- What linting rules do you enforce?
- What are your testing requirements?
- What code review standards apply?
-->

The GPUI application starts filesystem and SSH work on background executors.
Correctness therefore depends on preserving operation ownership across entity
updates, project switches, overlays, and late async completions.

---

## Forbidden Patterns

<!-- Patterns that should never be used and why -->

- Inferring a context-menu target from hover or selection after the click.
- Letting a stale async completion mutate the current project or picker state.
- Storing a global file-operation lock only inside the currently rendered file
  tree entity.
- Showing local OS actions such as reveal-in-folder for a remote path.
- Treating UI conflict preflight as proof that the destination is unchanged.

---

## Required Patterns

<!-- Patterns that must always be used -->

- Snapshot local/remote source identity and allocate a request or operation token
  before spawning background work.
- Validate the token and source identity before every UI mutation. Clear shared
  busy state only from the completion that owns it.
- Represent row and blank-area context-menu targets explicitly.
- Revalidate download destinations and transfer conflicts at execution time.
- Keep destructive and transfer operations staged where a partial result would
  otherwise replace a valid destination.

---

## Testing Requirements

<!-- What level of testing is expected -->

- Add pure tests for operation-token ownership, source-identity comparisons,
  blank-area targeting, conflict planning, and stale request rejection.
- Cover project switching and directory refresh during active operations.

### Hard Constraint: GitHub Actions Only

The user explicitly requires all CI and build-related execution to happen in
GitHub Actions. This applies to the main agent, every sub-agent, and every
package/sidecar, including one-off reproduction or diagnostic commands.

- Run compilation, `cargo check`/metadata, tests/fixtures, Clippy/lint,
  formatting/checks, generated-code/i18n commands, packaging, installer checks,
  and CI scripts only in the existing GitHub Actions workflows. Do not run
  them locally, in a local container, through `act`, or manually over SSH.
- Local work is source/configuration editing, code reading, and read-only Git
  status/diff review. Static review is not passing CI evidence. Automated
  whitespace checks such as `git diff --check` belong to the Actions gate.
- On failure, inspect Actions logs and artifacts, apply scoped source or
  generated diagnostic patches, and rerun Actions. A missing runner, slow
  workflow, or convenient local toolchain is not an exception to this rule.
- Require workflow evidence for the exact product commit: run URL/ID,
  `headSha`, job conclusions, and relevant artifact identity. Use
  `.github/workflows/ci.yml`, `windows-package.yml`, or `release.yml` as
  appropriate; an unrelated green run does not validate current changes.
- Any manual native acceptance uses an Actions-produced artifact. Automated
  UI/regression harnesses and their fixture processes remain Actions-only.

This constraint overrides older troubleshooting examples that suggest a
local Cargo/test run. Do not silently skip a gate or claim an unrun check.

---

## Code Review Checklist

<!-- What reviewers should check -->

- [ ] Can a late task clear or overwrite state owned by a newer task?
- [ ] Can switching projects bypass an operation lock?
- [ ] Are remote and local menu capabilities separated?
- [ ] Does execution re-check assumptions made by a dialog or preflight?
- [ ] Are cleanup and rollback failures preserved in the reported error?
