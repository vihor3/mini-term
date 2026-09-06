# Tasks Account Execution Plan

Inherit the parent approval, Actions-only, and dispatch gates. Execute after
remote Git, serializing any shared execution-host changes. No live credential
command, account switch, or login is permitted during planning/research.

## Implementation Order

- [x] Read official-source research and the current Tasks/source/config contracts.
- [x] Add structured account enumeration and capability/error parsing in
  `mt-github`, retaining domain/transport separation and bounded field allowlists.
- [x] Add a default-compatible selected-account identity setting with exact
  project/host scope, without altering global `gh` configuration.
- [x] Implement the dedicated secret-safe Native/WSL/SSH request executor and
  sanitized failure handling; keep secrets outside ordinary plans/results/logs.
- [x] Replace active-account pipeline validation with selected-account validation
  and preserve source/epoch/cache/request generation ownership.
- [x] Add the Tasks selector and missing/multiple/invalid-account states; retain
  inert manual login Copy/Retry and exact-host labeling.
- [x] Add native, actual SSH and actual WSL Actions fixtures and obtain the
  combined Trellis source check, including foreground cache/activation fixes.
- [ ] Obtain exact-SHA Actions and matching native artifact acceptance; update
  evidence without treating source review as a passed execution gate.

Checked items are source implementation/review, not passing validation.
Noether's completed follow-up adds explicit foreground access ownership, narrow
WorkItem activation hooks and regressions without a passive-notification fetch
loop. Native/SSH assertions were strengthened; the actual disposable WSL test
and launcher are source-complete. McClintock independently reviewed/fixed main's
CI setup. All new execution/native gates remain UNRUN; see review.md and
ci-review.md for contracts, exact test names and limits.

## Actions-Only Cases

- Two configured accounts, one broken and one valid, sole/no account, unsupported
  auth flags/JSON, inaccessible secure store, and inherited auth environment.
- Project A selects account A and project B selects B concurrently. Switching
  either Tasks choice or global active `gh` does not change the other choices.
- Local/WSL/SSH command plans preserve execution ownership with hostile path,
  username/host quoting, timeout/cancellation, reconnect, and lookup failure.
- A disposable runner `gh` shim emits a sentinel credential; ensure it is used
  only inside the intended child environment and absent from args, config,
  diagnostics, error responses, normal stdout, and remote-client transport.
- Selection change/logout/revocation during list/detail, origin replacement,
  wrong account proof, cache reuse, and A-to-B-to-A stale completions.

Do not use the user's real accounts/tokens for automated verification. An
Actions-only fixture is not proof of native secure-store compatibility; final
artifact acceptance must report any device-specific capability/auth issue.
