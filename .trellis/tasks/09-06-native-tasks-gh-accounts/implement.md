# Tasks Account Execution Plan

Inherit the parent approval, Actions-only, and dispatch gates. Execute after
remote Git, serializing any shared execution-host changes. No live credential
command, account switch, or login is permitted during planning/research.

## Implementation Order

- [ ] Read official-source research and the current Tasks/source/config contracts.
- [ ] Add structured account enumeration and capability/error parsing in
  `mt-github`, retaining domain/transport separation and bounded field allowlists.
- [ ] Add a default-compatible selected-account identity setting with exact
  project/host scope, without altering global `gh` configuration.
- [ ] Implement the dedicated secret-safe Native/WSL/SSH request executor and
  sanitized failure handling; keep secrets outside ordinary plans/results/logs.
- [ ] Replace active-account pipeline validation with selected-account validation
  and preserve source/epoch/cache/request generation ownership.
- [ ] Add the Tasks selector and missing/multiple/invalid-account states; retain
  inert manual login Copy/Retry and exact-host labeling.
- [ ] Add Actions fixtures, obtain Trellis check, and update the affected contract
  through main after behavior and validation evidence agree.

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
