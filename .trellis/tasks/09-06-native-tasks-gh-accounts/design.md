# Independent Tasks Account Selection

## Account State

Maintain a selected account name per root project, execution host/backend, and
GitHub hostname. Sibling worktrees of that configured project use its Tasks
choice; different projects or execution hosts do not retarget one another.
Worktree-specific Issues/PRs mode, selection, scroll, and detail tabs stay separate.
Store only the normalized host/account identity in app config with a missing
field default; do not persist credentials or overwrite `gh` configuration.

Discover known accounts through that host's `gh` structured auth-status output,
without `--active` or `--show-token`, and allowlist nonsecret identity/status
fields. Treat per-account auth errors independently of enumeration success.
When no selection is stored, a sole usable account may initialize the choice;
multiple accounts require a Tasks selection before a data request. Once chosen,
the device's active-account changes never rewrite the choice. An unavailable
chosen account remains explicit until the user reselects.

## Secret-Safe Host Execution

The official-source research confirms explicit username token lookup and
per-process authentication overrides. Introduce an account-scoped executor
alongside the existing ordinary command executor. Public command plans carry
only account/hostname identity and structured data-command argv, never a token.

- Native local: a dedicated bounded credential stage captures the named token
  in a secret-only owner and creates the data process with its own environment.
  It must not pass through general `CommandOutput` logging/formatting.
- WSL/SSH: named token lookup and data execution occur in one execution-host
  envelope. The secret is captured inside that host, checked for failure/empty
  output, applied to the request's environment, and never returned over stdout
  or reintroduced through client argv. Use the existing single-quote encoder
  only for nonsecret parameters; disable tracing for this envelope.
- Override the applicable GitHub/GHES auth variable for the child and remove
  conflicting inherited auth/debug/host variables as appropriate. Do not mutate
  process-global environment, run `gh auth switch`, invoke login, or write a
  credential/temp script/config file containing the token.
- Keep bounded timeout/output/cancellation, process cleanup, and authenticated
  connection epoch ownership. On a lookup error, return an allowlisted error
  category with no raw credential-stage stdout/stderr or serialized secret.

This is a purpose-specific extension. Do not put arbitrary environment/secret
maps into clonable/debuggable domain plans as a general workaround.

## Pipeline and Cache

Probe required CLI capabilities and discover the exact worktree's origin as
today. Validate the selected account via the request's overridden `gh api user`
context, then fetch the Issues/PRs list or detail with explicit repository/JSON
fields. Revalidate origin and selected identity before publishing. The device
active account is informational, not a request fence.

Selection changes advance only that project's account/auth request generation
and invalidate its active requests and identity-dependent data. Preserve the
existing source, repository, normalized account, auth generation, request ID,
and SSH epoch fences. Share repository data across sibling worktrees only after
each source independently proves the same repository and selected identity.
Old detail previews cannot become actionable under a different account.

## Tasks UI and Errors

Place the account selector in Tasks alongside its existing view/filter controls,
with bounded account names and clear selected state. Keep manual host-specific
login Copy/Retry when no account is available; no separate login experience.
Do not conflate unsupported CLI flags, credential-store access, missing auth,
revocation, repository permission, rate limit, and offline errors. A failing
account must not hide other discovered accounts or trigger silent fallback.

## Compatibility and Contract Updates

Required auth JSON and named-account lookup capabilities are checked rather than
assuming every installed CLI matches current documentation. The existing spec's
active-account rechecks must be replaced with selected-account rechecks; its
no-token-in-general-output invariant remains. Global right-tool selection does
not synchronize Tasks account identity. Git panel credentials remain unchanged.
