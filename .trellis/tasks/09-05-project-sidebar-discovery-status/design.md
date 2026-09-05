# Project Sidebar Integration Design

## Final Product Policy

Newly discovered valid worktrees appear automatically. User-hidden rows stay
hidden across restart/refresh; invalid rows are excluded by default. Project
Settings provides per-row control. This explicitly differs from Orca's
ownership-based default-hide policy and must override earlier recommendations.

Hiding is presentation only: no Git pruning/deletion, no project removal,
no terminal/Agent shutdown, and no global search restriction. A disconnected
remote is not an invalid worktree. Same-upstream independent local checkouts
remain separate inventories.

## Deliverable Map

| Child | Product responsibility | Primary boundaries |
| --- | --- | --- |
| `09-05-sidebar-worktree-discovery` | Defaults, invalid exclusion, project settings/persistence, routine refresh warning stability | mt-config, AppStore projects, WorktreeCatalog, sidebar header/list |
| `09-05-sidebar-agent-status` | Remote process discovery, accepted-state projection, lifecycle, activity/connectivity/attention | mt-ssh probe, mt-ai registry helpers, AppStore Agent ownership, sidebar status lane |

Each child's `design.md` is authoritative for its technical contracts. The
parent owns integration, scope, artifact review, final native validation, and
preservation of unrelated user work; it is not a third product implementer.

There is no logical data dependency between process recognition and visibility.
Execute the visibility child first, then the Agent child, to serialize their
shared sidebar edits. Both must pass independent checks. Parent startup/
refresh acceptance depends explicitly on both, because the reported marker
may be catalog freshness while process recognition has a separate real defect.

## Shared Invariants

- All CI, compilation, tests/fixtures, lint/format/whitespace checks, code
  generation, packaging, and automated verification run exclusively in GitHub
  Actions. No agent may substitute local/SSH/container execution. Manual native
  acceptance uses only an Actions-produced artifact.
- One host-aware raw catalog feeds settings, navigation, and sidebar projection.
- Preference keys use stable source/path identity, never branch names or
  transient connection epochs. Unknown/offline state does not erase choices.
- Ordinary refresh progress is not a failure or Agent activity. Preserve
  registration authority and owner/epoch fencing independently of warning UI.
- Registry event acceptance precedes routed Agent legacy/attention effects.
- Process liveness, activity, connectivity, and attention remain distinct.
  Preserve Hook authority and existing legacy compatibility for other views.
- Terminal transport exit does not prove a remote Agent exited. Hiding a row
  does not affect either lifecycle.
- Reuse existing menu/dialog/icon/config writer APIs and i18n ownership. No
  new runtime, persistence database, dependency, or lockfile change is planned.

## Integration Acceptance

With the verified sample inventories, v26.019 shows its three valid worktrees;
AICOS shows its nine non-prunable worktrees initially. Manually hiding one
removes only that sidebar row; a later valid discovery still appears. Restart,
offline retry, branch rename, and project switching preserve the choices.

While a remote Agent remains idle, repeated successful worktree scans do not
toggle an Agent/warning state. Genuine probe errors remain visible. A
controlled exact-route process is discovered; Working/Waiting/attention/error
presentation follows accepted evidence, and late events or terminal exit do
not resurrect an old Working indication. Hiding/unhiding that worktree never
changes the underlying Agent's route or activity.

## Risks and Deferred Work

The original flashing cadence has not been reproduced in a running binary.
Source defects and screenshot marker paths are proven, but completion requires
native traces tied to the tested build. Linux process presence plus PTY recency
does not provide universal Hook-quality provider semantics; unsupported hosts
and environment-stripping multiplexers retain explicit fallback limitations.

No wholesale Orca menu copy, global settings/search redesign, destructive Git
cleanup, new remote Hook protocol, universal heuristic timeout, or general
unread/provider rewrite is included. Material expansion returns to planning.

Rollback preserves user data and preferences. The existing remote-status
feature gate remains available; visibility can revert independently without
removing raw catalog entries. Use scoped changes, not worktree resets.
