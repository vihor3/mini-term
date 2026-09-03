# Technical Design

## Source And Projection

`AppStore::agent_target_views()` remains the only feed source. The projection is
extended with feed unread state derived from an acknowledgement watermark:

```text
AgentRunId -> acknowledged AgentEventId
```

A target is unread when its current `last_event_id` differs from the stored
watermark and its activity represents user attention or newly completed work.
Because acknowledgement stores the event ID, any later accepted update to the
same run becomes unread without an explicit invalidation callback.

The legacy `DoneTracker` continues to drive tray/title compatibility. Feed
watermarks are independent so `set_window_focused()` cannot bulk-ack the feed.
Route removal prunes orphaned watermarks.

## Grouping

A pure projection groups and orders targets:

1. Needs You: pane attention, Blocked, Failed, or unread Done/Waiting.
2. Working: Starting or Working.
3. Recent: acknowledged Done/Waiting, Interrupted, Exited, Unknown, and remaining
   stale/disconnected entries.

Within a group, newest receipt comes first, then project/worktree/pane/provider
and `AgentRunId` provide deterministic ties. Recent rows are bounded while all
active Needs You and Working rows remain visible.

## Activation And Ack

The row submits only `AgentRunId`. The store re-resolves and revalidates host,
worktree, tab, pane, terminal session, incarnation, and PTY route, then switches
and focuses. Only after that succeeds does it store the current event watermark
and clear exact pane attention where compatible. It never creates a terminal or
uses session-history resume.

If revalidation fails, the action returns false. The overlay remains open and
no acknowledgement changes. The next render may drop the row if authoritative
state removed its route.

## Overlay Lifecycle

The existing `toggle_agents`, `close_agents`, overlay stack entry, viewport
geometry, and focus-return model remain authoritative. Rendering changes only
the feed body and badge count. Opening captures focus and pushes the overlay;
it performs no project/workbench mutation.

Row activation closes with `restore=false` after exact focus succeeded, so focus
stays in the destination pane. Escape/outside/close/toggle use `restore=true` and
return focus to the captured workbench target.

## Rollback

`MINI_TERM_GLOBAL_AGENT_ACTIVITY=0` hides the global Agents entry and prevents
the overlay from opening. Runtime state, inline worktree rows, Sessions badges,
and exact activation APIs remain active; no persisted schema is removed.
