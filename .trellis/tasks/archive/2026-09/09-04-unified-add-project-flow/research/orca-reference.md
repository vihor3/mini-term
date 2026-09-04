# Orca Add Project Reference

## Scope

This note records the concrete Orca implementation patterns used as design evidence for mini-term. Orca is reference behavior, not a source-code dependency.

## Evidence

- `AddRepoDialogChrome.tsx:23-34` keeps one stable dialog shell and switches only the inner step content.
- `AddRepoStartSteps.tsx:153-223` renders the host selector, one primary folder action, and always-visible alternate actions without a disclosure click.
- `AddRepoStartSteps.tsx:99-141` gives Browse initial focus and uses roving keyboard focus for the action list.
- `AddRepoHostSelector.tsx:53-81` keeps the selected host compact in the form header.
- `AddRepoHostSelector.tsx:84-163` places Add Remote Host inside the same selector menu.
- `AddRepoHostSelector.tsx:164-228` keeps disconnected hosts visible and exposes Connect without selecting an unusable host.
- `use-add-repo-host-change-reset.ts:16-31` invalidates host-scoped fields whenever the selected host changes.
- `AddRepoCloneStep.tsx:92-218` uses a focused URL/parent form, host-aware folder browsing, inline error, disabled submit, and progress below the stable action button.
- `AddRepoCreateStep.tsx:101-104` derives an explicit final target preview from parent and name.
- `AddRepoCreateStep.tsx:143-255` constrains long paths and groups location details without changing dialog width.
- `useCreateRepo.ts:36-51` and `useAddRepoCloneFlow.ts:56-96` allocate monotonic operation generations and reset them on navigation.
- `useCreateRepo.ts:92-140` and `useAddRepoCloneFlow.ts:123-184` bind each completion to both its generation and host token before state mutation.
- `repo-creation-handlers.ts:137-223` validates basename, absolute parent, and collision at the execution boundary.
- `repo-creation-handlers.ts:225-256` distinguishes initialization failures and only cleans state owned by the operation.
- `remote-repo-clone.ts:50-114` derives the target on the selected SSH host and preserves host ownership through completion.
- `repo-clone-lifecycle.ts:135-224` keeps local clone as structured argv and protects retries from stale abort cleanup.

## Mini-Term Adaptation

Mini-term should copy the information architecture and ownership rules, not Orca's React/Electron layering. GPUI state remains entity-owned; host operations stay behind Rust services; project registration continues through `AppStore` and workbench identity.

Differences required by the approved product behavior:

- Create New Project has New Folder and Initialize Existing Folder modes.
- Folder browsing is always a pure add operation.
- Initialization runs `git init` only; it does not create an initial commit or template files.
- Selecting a directory nested in another worktree blocks nested initialization and offers the detected root.
- Local and saved SSH hosts are the only visible host choices; WSL remains an automatic local-path routing detail.
