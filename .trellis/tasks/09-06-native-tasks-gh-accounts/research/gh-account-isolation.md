# Request-Scoped gh Account Selection

Date: 2026-09-06. Source-only feasibility research; not an implementation or a
claim about the installed CLI version on any target device.

## Official Evidence

- [gh auth token](https://cli.github.com/manual/gh_auth_token) supports explicit
  hostname and username. The
  [command source](https://github.com/cli/cli/blob/trunk/pkg/cmd/auth/token/token.go)
  calls `TokenForUser` when a username is supplied, rather than `ActiveToken`.
- [AuthConfig source](https://github.com/cli/cli/blob/trunk/internal/config/config.go)
  resolves `TokenForUser` from that user's keyring/config entry. It does not
  activate another user or fall back to the active user's token on a miss.
- [gh environment](https://cli.github.com/manual/gh_help_environment) documents
  `GH_TOKEN`/`GITHUB_TOKEN` precedence over stored credentials for GitHub.com,
  and the corresponding enterprise variables. These can be applied to only
  the child request process, without mutating global account/configuration state.
- [gh auth status](https://cli.github.com/manual/gh_auth_status) can enumerate
  known accounts; `--active` restricts the result. JSON mode does not return a
  failing exit status merely because one account has authentication issues.
  Do not request `--show-token`; allowlist nonsecret account fields for UI data.

## Feasibility and Boundaries

Per-request selection is feasible without `gh auth switch`: resolve the named
account's credential and apply it only to the intended request process. The
Tasks selector stores account identity independently of the device active user.
On SSH/WSL, credential lookup and use must both remain inside that execution
host. Do not obtain a remote token through the existing captured-stdout command
API and then send it back as a parameter.

`crates/mt-app/src/execution_host.rs:325` currently plans local/WSL structured
argv and quoted SSH commands, but has no account-scoped secret execution owner.
`crates/mt-github/src/commands.rs` plans only program/args. The eventual design
must add a bounded secret-safe execution path and retain timeout, cancellation,
source, and epoch ownership. Credentials must not enter argv, serialized plans,
general command outputs, logging, tracing, or persistent app data.

Current Tasks pre/post identity validation follows the active account. It must
instead validate the explicitly selected identity under the same request-scoped
authentication context, without losing repository/account generation fences.
Inherited authentication/debug variables must not retarget the chosen account
or expose credentials. These are design obligations, not tested guarantees.

## Compatibility and Failure Policy

Check required CLI capabilities in the eventual implementation; do not label an
unsupported flag or malformed response as "not logged in." Account logout or
revocation affects availability even though active-account selection is isolated.
Do not silently substitute another account. Credential-store access from the
actual noninteractive execution context may fail and needs explicit handling.

All enumeration/isolation/failure/security regressions and compilation run in
GitHub Actions only. No token lookup, login, account switch, live authentication
probe, build, formatter, or test was executed during this research.
