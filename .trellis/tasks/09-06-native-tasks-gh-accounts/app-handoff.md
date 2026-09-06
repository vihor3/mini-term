# Tasks App Integration Handoff

App/config slice is SOURCE-COMPLETE, NOT VERIFIED. Ownership is released for
main's independent review and Actions integration. All compilation, formatting,
lint, tests, fixtures, whitespace checks and native acceptance are UNRUN here.

## Boundary

App ownership is `crates/mt-app/src/github_tasks.rs` and private modules beneath
`github_tasks/`, plus the focused selection DTO/field/default/tests in
`crates/mt-config/src/config.rs` and its export. No store, workbench/panes, main,
execution-host, remote-SSH, domain, generated dictionaries, specs, or workflow
edits. Existing dirty files belong to their current owners and are preserved.

## Dependencies / Resolved Questions

- Consuming the executor's finalized `AccountCancellation`,
  `AccountExecutionControl::new`, `AccountHostResult`, capability, enumeration,
  and selected-account APIs. Finer `AccountExecutionError` / `AccountError`
  categories will remain distinct, including missing Python 3.8+ on WSL/SSH.
- Re-read Copernicus's source-complete handoff: API signatures/error variants
  are unchanged. Its 10 authored native/POSIX/SSH cases and main's draft Windows
  and isolated loopback-SSH gates remain separate, UNRUN executor evidence.
- Main approved retaining origin discovery through the ordinary host command executor, for
  Git only. It is bounded, but that public API has no cancellation argument.
  App cancellation checks before/after origin prevent any subsequent account
  stage or publication. Physical cancellation of an already-dispatched origin
  read is intentionally not added; no generic executor extension is requested.
- The account executor must preserve observed SSH epochs on errors, and verify
  list/detail identity before/after using the same captured credential, as
  declared. App owns origin, selection, source, request, and generation fences.
- No existing Tasks locale namespace was found. Existing Tasks English UI is
  preserved with main's approval. New locale keys: none. Generated dictionaries,
  UsedKeys and counts are unchanged by this slice.

## Implementation Shape

Persist normalized account identity in a default-empty config list keyed by root
project, execution host, stable backend, and GitHub hostname. Never persist an
SSH epoch, token, or raw account status. Private pipeline and service modules
separate bounded host work from UI-thread selection/persistence/publication.
The toolbar selector is exact-source guarded; worktree presentation and tabs
remain independent, with old identity tabs inert after invalidation.

Independent check, exact-SHA Actions builds/tests/lint/formatting, and native
acceptance from Actions-produced artifacts remain main-owned gates.

## Files Written

- MODIFIED `crates/mt-app/src/github_tasks.rs`: account selector inside the Tasks
  toolbar, bounded account/host/repository text, missing/multiple/broken states,
  exact-source selection and row guards, manual login Copy/Retry, source-switch
  cancellation, and stale detail rendering. Existing worktree mode/filter/row
  selection/scroll and workbench tab API are retained. No global tool, document,
  terminal, Git credential, login, browser or account-switch action is added.
- NEW `crates/mt-app/src/github_tasks/model.rs`: private selection/config
  projection, durable account scope, fine-grained Tasks errors, cache/source
  records, and request/slot ownership predicates.
- NEW `crates/mt-app/src/github_tasks/pipeline.rs`: capability/enumeration
  preparation and selected-account list/detail orchestration over the finalized
  executor API. Origin is checked before/after reads and failures; account
  enumeration and selected identity are revalidated before publishing. Account
  activity flags do not choose an identity or participate in its ownership.
- NEW `crates/mt-app/src/github_tasks/service.rs`: exact-source preparation,
  config persistence through `AppStore::patch_config`, project/host/GitHub-host
  auth generations, bounded request cancellation, selected-account data caches,
  and UI-thread completion gates. Shared repository keys remain downstream of
  each worktree's own origin/account proof. Auth invalidation removes associated
  lists/details and readiness, but leaves different roots/hosts/GitHub hosts alone.
- NEW `crates/mt-app/src/github_tasks/tests.rs`: 17 synthetic tests against the
  production orchestration, selection, generation and ownership helpers.
- MODIFIED `crates/mt-config/src/config.rs`: default-empty
  `tasks_account_selections`, `TasksAccountScope`, `TasksAccountSelection`, and
  two focused serialization/config-store reload tests. The durable backend DTO
  reuses `WorktreeVisibilityBackend`; epochs/fingerprints/credentials are absent.
- MODIFIED `crates/mt-config/src/lib.rs`: exports only the two new config DTOs.
- This handoff document. No other files were written by this app slice.

The private modules are new/untracked files. Include their complete contents
with the matching parent imports when applying Actions formatting artifacts.

## Authored Fixtures (All UNRUN)

The 17 app tests cover multiple accounts with a broken peer, sole/no/missing
choices, canonical/exact lookup spelling, restart serialization, root/host/WSL/
SSH/GitHub-host independence, invalid/duplicate saved choices, actual production
list/detail orchestration with strict synthetic stage replies for all backends,
external active-account changes, capability/auth/store/access/offline/helper/
protocol/cleanup errors, cancellation before/after origin and selected data,
logout and wrong post-read identity, first-epoch adoption, later success/error
epoch rejection, origin A-to-B-to-A, source/account/request/auth-generation
ownership, scope cancellation/cache invalidation, sibling cache identity,
worktree tab identity, rollback parsing and bounded inert context text.

Two config tests cover missing-field compatibility and identity-only JSON, plus
the production `ConfigStore` save/drop/reopen path with independent A/B choices
inside a temporary runner fixture directory. These tests are authored only,
not executed locally. The synthetic app executor invokes no process or gh;
Copernicus owns actual native/POSIX/SSH sentinel process fixtures separately.

## Remaining Gates / Limits

1. Independent app/config/domain/executor review and exact-product-SHA Actions
   evidence are still required. No static inspection is claimed as a passing
   test, build, lint, formatting, whitespace, or UI gate.
2. Run the app's `github_tasks` tests and the config `tasks_account_choices`
   tests through existing Actions workflows, plus workspace and Windows gates.
   Full-file formatting must also run in Actions. No local commands are implied.
3. Exercise actual Native/WSL/SSH transport, secure-store accessibility, account
   switching, logout, reconnect, stale tabs and toolbar sizing using the executor
   fixtures and matching Actions-produced native artifact. In particular,
   Python-envelope tests alone are not proof of actual WSL transport behavior.
4. Ordinary already-dispatched origin reads remain bounded rather than physically
   cancellable, as approved. Their late result cannot advance an account stage
   or publish after cancellation. Account stages use explicit cancellation.
5. External origin or credentials can change between separate host reads. The
   implementation rejects observed changes, cancellation and obsolete ownership;
   it does not claim an atomic transaction with external Git/gh configuration.
   Same-credential pre/data/post identity proof remains the executor's contract.

Only source/documentation reads, manual apply_patch edits and read-only Git
status/diff inspection were performed. No real account/credential probe, build,
Cargo metadata, test, fixture, formatter, lint, whitespace check, application
launch, staging, commit, workflow edit or spec edit was performed by this slice.
