# Tasks Account Domain Handoff

Domain slice is SOURCE-COMPLETE, NOT VERIFIED. Resumed static review and test
gap work is complete; the independent domain review and all Actions gates remain
outstanding. `mt-github` ownership is released for main's dependent review while
this implementer works on the separately dispatched app executor. No rollback,
staging, commit, or local checks were performed. The separately dispatched
executor is now also source-complete and released; see `executor-handoff.md`
for its stable API, executable UNRUN fixtures and remaining transport gates.

## Files Written

- NEW `crates/mt-github/src/accounts.rs`: validated canonical account identity,
  exact discovered login spelling, bounded host-map parser, per-account state
  and static problem categories, initial selection/find helpers, identity proof.
- NEW `crates/mt-github/src/account_error.rs`: separate `AccountError` enum,
  `AccountCapability`, `AccountCommandStage`, `require_account_success`, and
  `classify_account_execution_error`. No raw-error payload is retained.
- NEW `crates/mt-github/src/account_tests.rs`: 20 deterministic tests authored
  against production parser/plan/error functions. All are UNRUN.
- MODIFIED `crates/mt-github/src/commands.rs`: structured account enumeration,
  help-only capability checks, private selected-account identity/list/detail
  request plans, and legacy migration comments. Existing commands preserved.
- MODIFIED `crates/mt-github/src/lib.rs`: exports both new production modules
  and registers the new test module. All referenced files exist in source.
- MODIFIED `crates/mt-github/src/model.rs`: legacy `parse_account` documentation
  only; existing list/detail/account DTO behavior unchanged.
- MODIFIED `crates/mt-github/src/remote.rs`: `normalize_host` becomes
  `pub(crate)` for reuse; existing repository parsing behavior unchanged.
- This handoff document. No app/executor/config/manifest/lock/spec/Git changes.

New module files were untracked at pause; ordinary `git diff --stat` does not
include them. Any later Actions formatting artifact must preserve complete new
files together with their matching module/import edits, not isolated hunks.

## Remaining Gates

1. Main obtains independent domain review. Static review added object-only
   decoding to reject Serde positional-array representations, plus focused
   capability/identity completeness and size tests and host/login boundaries.
   Syntax and compilation have NOT been verified by a tool.
2. Main arranges complete-file formatting, focused `mt-github` tests, compilation,
   lint, and workspace/Windows checks ONLY through GitHub Actions, then obtains
   Trellis check. No formatter, whitespace check, Cargo metadata, test, fixture,
   probe, launch, or automation was run locally.
3. Integrate the app in its separately owned slice using the requirements below.
   Credential isolation, actual executor compatibility, UI, config persistence,
   cache generations, cancellation, and SSH epochs are not implemented here.

## Authored Tests (UNRUN)

Coverage currently includes mixed valid/broken accounts, sole/none/multiple,
device-active changes not retargeting identity/plans, exact lookup spelling,
managed names, hostile hosts/logins, duplicate accounts/JSON keys, wrong host,
invalid/contradictory schema, 64-account and 64-KiB limits, invalid UTF-8 including
ignored fields, missing status, both truncation flags, timeouts, inherited auth
markers, static permission/revocation/scope/rate/offline/store/lookup categories,
fatal enumeration errors, help-only capability detection, explicit read plans,
preserved list/detail fields and limits, wrong identity proof, and sentinel
absence from returned account/plan/error formatting. All inputs are synthetic.

Only source reads, official web-source inspection, manual apply_patch edits,
and read-only Git status/diff inspection were performed. No live gh account or
credential command was executed. No verification workflow evidence exists for
this slice yet.

## Boundary

Only `crates/mt-github/src/` account domain files, exports, legacy API comments,
and internal host-normalization visibility are in scope. No process execution,
credential handling, config/filesystem access, manifests, or app edits.

## API For Integration

- `GitHubAccountIdentity::new(host, login)` validates and canonicalizes identity.
  `host()` / `login()` are lowercase and suitable for persisted selection/cache
  comparisons. A discovered `KnownGitHubAccount::login()` retains exact login
  spelling for named lookup: gh's user config/keyring lookup uses that spelling.
  Persist canonical identity, then re-enumerate to resolve the lookup spelling.
- `known_accounts_plan(host)` plans `gh auth status --hostname HOST --json hosts`
  without `--active` or `--show-token`. `parse_known_accounts(host, output)`
  yields `KnownGitHubAccounts`, retaining all validly identified rows even when
  one has a per-account error. Only login/host, informational active state,
  structured status, and static error category survive parsing.
- `KnownGitHubAccounts::accounts()`, `find(identity)`, `initial_selection()`:
  the last returns a sole successful account only; multiple choices never use
  device-active state as a selection default. Missing selected identity never
  falls back. The app still owns selection state and all generation fences.
- `AccountCapability::{AuthStatusJson, NamedAccountLookup}`,
  `account_capability_plan(capability)` / `verify_account_capability(...)`
  use help-only probes. Enumeration itself verifies required JSON schema.
- `SelectedAccountRequestPlan::{identity, list, detail}` accept a discovered
  account (and repository/kind/number as applicable). Private fields allow
  only the three data commands, not arbitrary programs, secret lookups, or
  environment maps. `account()`, `lookup_login()`, `data_plan()`, `stage()`,
  `output_limit()` give the dedicated executor its nonsecret inputs.
- `verify_selected_account(identity, output)` validates bounded `gh api user`
  JSON under the executor's selected-account context; active gh state is not
  proof. The executor must enforce the plan's host; the API response only
  proves login, not transport/source/host ownership.
- `AccountError` is a new static enum, not an expansion of `GitHubErrorKind`.
  This avoids breaking the reserved app's exhaustive matches in this slice.
  New consumers must explicitly retain the finer error categories instead of
  collapsing them into legacy `AuthRequired` or raw diagnostic strings.

## Required App Migration

`auth_status_plan`, `account_plan`, `parse_account`, and existing list/detail
DTOs remain behavior-compatible for the current app. Do not change just the
auth command in the old active-account pipeline. Integrate enumeration,
project/host selection, a dedicated secret-safe executor, selected identity
pre/post probes, cache/generation invalidation, and UI/error states together.

Enumeration must remove inherited GitHub auth/debug overrides on its execution
host. gh can otherwise add an environment-only active row or duplicate a
stored account. The parser rejects explicit environment-backed status data;
it cannot sanitize the execution environment. Never return a credential-stage
`CommandOutput`: the executor returns only an `AccountError` category on lookup
failure, and holds the secret exclusively on the selected execution host.

## Official-Source Notes

- [Status source](https://github.com/cli/cli/blob/trunk/pkg/cmd/auth/status/status.go)
  defines a `hosts` object keyed by hostname, with account entries containing
  `state`, `error`, `active`, `host`, and `login`. States are `success`,
  `timeout`, or `error`. JSON mode removes token fields unless explicitly
  requested and does not fail merely for a broken account.
- [Named lookup source](https://github.com/cli/cli/blob/trunk/pkg/cmd/auth/token/token.go)
  and [config source](https://github.com/cli/cli/blob/trunk/internal/config/config.go)
  show that named lookup can collapse a missing token and inaccessible keyring
  into the same failure. Report `CredentialLookupFailed` when cause is unknown;
  `CredentialStoreUnavailable` requires specific evidence from the executor.
- [Environment documentation](https://cli.github.com/manual/gh_help_environment)
  includes both github.com and subdomains of ghe.com in GH_TOKEN routing;
  arbitrary GHES hosts use GH_ENTERPRISE_TOKEN. The app executor owns this.
- [Managed-user names](https://docs.github.com/en/enterprise-cloud%40latest/admin/managing-iam/iam-configuration-reference/username-considerations-for-external-authentication)
  can contain the underscore/enterprise suffix and still have a 39-byte cap.

All tests/checks are UNRUN and Actions-only. No local fixture, probe, account,
credential, build, formatting, or automation command has been executed.
