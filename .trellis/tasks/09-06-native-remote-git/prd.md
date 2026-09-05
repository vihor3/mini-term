# Support the Git panel on remote project hosts

## Goal

Make the existing Git panel's repository, changes, history, and worktree workflows
usable on remote project hosts without local fallback or cross-worktree effects.

## Requirements

- Own parent requirement R14. Inherit all constraints and scope decisions in
  [the parent PRD](../09-06-native-ui-remote-feedback/prd.md).
- Make the Git panel usable for remote project worktrees on their own execution
  hosts, retaining the panel's existing workflow and applicable safeguards.
- Cover status, staging/unstaging, confirmed discard, commit, pull/push, branch
  history filtering, diffs, and the existing reachable Worktree Management actions.
  Do not expose a remote button that still invokes a local child/dialog backend.
- Never fall back to local Git execution or local repository paths for a remote
  source. Delayed results and mutations remain bound to the chosen host/worktree.
- Keep Git operations and their existing credential behavior separate from
  Tasks' selected `gh` account. Changing a Tasks account does not reconfigure Git.

## Evidence

`crates/mt-app/src/git_panel.rs:452` detects remote projects; `:531` suppresses
loading and `:822` displays remote-not-supported. Enabling the panel requires
execution-host backend support, not merely removing an error view.
[The action/API matrix](../09-05-sidebar-agent-status/research/09-06-remote-git-action-surface.md)
records the complete reachable surface, including local-only worktree registration
and cleanup paths that also need host ownership.

## Acceptance Criteria

- [ ] Remote worktree status and supported panel actions operate against the
  selected remote repository instead of showing remote-not-supported (R14).
- [ ] Changes/history/diff and Worktree Management actions retain their existing
  workflow semantics and operate on their captured host/repository (R14).
- [ ] Project switches, disconnect/reconnect, cancellation, and late responses
  cannot display or modify another worktree's Git state (R14).
- [ ] Existing local Git behavior and applicable mutation confirmations remain
  intact; Tasks account selection does not change Git credentials (R14/R15).
- [ ] A lost/cancelled reply after possible mutation does not claim rollback or
  replay the write; only the source-owned data is reconciled (R14).

## Out of Scope

New Git actions absent from the panel (such as checkout/stash/cherry-pick UI),
Tasks-driven Git credentials, local fallback for remote tools, and broad unrelated
local Git refactoring.

## Risks

Remote effects can be uncertain after dispatch; cancellation is not rollback.
Worktree cleanup needs authoritative remote evidence and exact terminal/document
ownership. New machine-format parsers need fixture coverage, not text guesses.

## Planning Status

PRD, source action inventory, design, execution plan, and curated context are
prepared for parent final review. No product edits, real remote Git commands, or
implementation approval; all automated checks and fixtures are Actions-only.
