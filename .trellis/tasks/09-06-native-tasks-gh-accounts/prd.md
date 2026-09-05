# Select gh accounts independently inside Tasks

## Goal

Resolve feedback item 15: discover project-host gh accounts and select an account in Tasks without synchronizing the device default active gh account.

## Requirements

- Own parent requirement R15. Inherit all constraints and confirmed decisions in
  [the parent PRD](../09-06-native-ui-remote-feedback/prd.md).
- Discover the accounts already logged in through the project's execution-host
  `gh`, and expose the selector in Tasks for both Issues and PRs.
- Remember a per-project selection, qualified by execution host and GitHub host.
  Tasks selection must not change the device's default `gh` account; an external
  default-account switch must not overwrite Tasks selection.
- Remote authentication and requests stay on the remote execution host. Persist
  account identity only, not a separate credential copy. Never return a remote
  credential through the normal command-result, diagnostic, or UI path.
- A logged-out, revoked, inaccessible, or unsupported selected account produces
  an actionable state, not silent use of another account. Distinguish missing
  CLI, unsupported CLI capability, account authentication, and repository access.
- Every request, cached result, and identity validation uses the selected
  account and exact source; late results cannot cross account changes.

## Evidence

`crates/mt-github/src/commands.rs:40` probes only the active account, while
`crates/mt-app/src/github_tasks.rs` validates that active identity around requests.
[Official-source research](research/gh-account-isolation.md) confirms a feasible
way to select stored account credentials without a global account switch.

## Acceptance Criteria

- [ ] Two logged-in device accounts appear in Tasks and can each be selected;
  an account-specific problem does not hide all other known accounts (R15).
- [ ] Projects choosing A and B retain their choices concurrently and after
  restart, independently of the device's default account (R15).
- [ ] Switching the device default account does not retarget current Tasks
  requests or choices; switching Tasks does not change global `gh` state (R15).
- [ ] No remote token is returned to the client, persisted separately, placed
  into command arguments, or exposed by logs/errors (R15).
- [ ] Revocation, logout, missing capabilities, account changes during requests,
  and host reconnects preserve explicit errors and request ownership (R15).

## Out of Scope

Global `gh auth switch`, a separate OAuth/token store, Tasks write operations,
Git credential changes, or synchronization with other projects/devices.

## Risks

Installed `gh` capabilities and noninteractive credential-store access need
Actions fixtures plus target-artifact acceptance. Official-source feasibility
does not establish that either real device account is currently accessible.

## Execution Status

The user approved the independent account behavior and complete final parent
scope on 2026-09-06. The disjoint transport-free mt-github domain slice is
activated; app/host execution and configuration integration remain coordinated
with the preceding children. No real credential command or authentication
probe is authorized here. All compatibility, isolation, security, build and
test execution remains GitHub Actions-only.
