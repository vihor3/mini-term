# Native Feedback Integration Design

## Status

Planning artifact for final review, not permission to implement. The parent PRD
owns behavior; each child owns its detailed design. No product source or installed
state is changed by this plan.

## Boundaries

1. Terminal display becomes a flat worktree-scoped projection: one visible
   terminal, individual top tabs, no split/group interactions. Existing runtime
   route and persisted terminal identities are not UI tab indexes.
2. `Workspace::context_panel` remains the shared right-tool selection. Files,
   Git, Tasks, and Sessions retain worktree/source-qualified data and requests.
   Selecting a different worktree retargets content, not the tool selection.
3. Agent ownership starts from Mini-Term's registered live terminals, including
   background worktrees. Process inventory attests liveness; semantic evidence
   attests activity. Projection never promotes generic output to task certainty.
4. Files/onboarding reuse existing source-qualified I/O and registration owners.
   The directory browser is navigation, not a second onboarding persistence path.
5. Git actions operate through a bounded execution-host backend. Tasks actions
   use selected-account `gh` on that host. Tasks account selection does not alter
   Git credentials or global `gh` state.

## Shared Integration Surfaces

| Owner | Primary surfaces | Required coordination |
| --- | --- | --- |
| Terminal navigation | main/titlebar/terminal area, pane actions, tooltips | Finish flat-tab route selection before Agent presentation integration |
| Agent status | mt-ai runtime/tracker, mt-ssh inventory, mt-app store/sidebar/Sessions | Consume stable terminal routes; do not reintroduce visible panel grouping |
| Files | picker/onboarding, file tree/menu/scroll | Preserve the global right-tool selection and workbench documents |
| Remote Git | Git panel and all children, transport-free Git plans/parsers | Serialize shared execution-host changes with Tasks |
| Tasks accounts | mt-github, task service/panel, selected-account config, execution host | Add a secret-safe execution owner, not credentials in generic plans |

Use existing crate ownership. Do not introduce a broad replacement runtime,
general-purpose transport framework, or unrelated tree/schema refactor. Shared
files are edited serially unless the implementation plan gives explicit disjoint
ownership. Children may share research, not unowned source edits.

## Compatibility

- Preserve all saved terminal records from legacy split/group layouts. Their
  internal owner IDs may remain to keep live and restored routes compatible;
  UI is flat regardless of the retained storage representation.
- Existing document and Tasks detail tabs retain worktree ownership and dirty
  close safeguards. Moving terminal navigation must not destroy these views or
  expose a second redundant terminal tab row.
- Worktree visibility policy, configured aliases, canonical source signatures,
  connection fingerprints/epochs, and accepted-event ordering remain intact.
- Unsupported process semantic evidence is shown conservatively; do not claim
  a universal exact task-state detector for every provider/version.
- Existing `gh` capabilities and secure-store availability vary by execution
  context. Unsupported CLI, unavailable credential, revoked account, network,
  and repository-access failures remain separate outcomes.

## Planned Contract Updates

Implementation must update only the affected executable specs after source and
Actions evidence agree. In particular, the current generic PTY-recency exception
in Agent specs cannot override the stricter approved activity design; the current
Tasks active-account pipeline must become selected-account validation. Preserve
the prohibition on returning tokens through ordinary command outputs. The
worktree-context contract already specifies global right-tool selection and does
not need a competing per-project preference model.

## Risk and Recovery

- Route identity reassignment can invalidate live SSH Agent ownership. Prefer
  presentation changes over reparenting existing terminals.
- Delayed Git mutations may have completed even after disconnect. Reconcile
  read-only on the same source; do not silently retry an uncertain mutation.
- Credentials must never enter args, saved app state, diagnostic output, or SSH
  responses. Account selection is identity state, not a credential store.
- Retain recoverable legacy layout payloads; no destructive database rebuild or
  mass metadata normalization. Scope rollback to the affected behavior and keep
  user terminal/document data intact.
- Exact-native flashing and hit routing are not proven by source review. The
  parent cannot close on compilation alone; acceptance uses an Actions artifact.
