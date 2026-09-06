# Native Shell Layout Contract

## 1. Scope

Applies to the approved Orca-style native caption, project sidebar, terminal
navigation and right context sidebar. The approved task HTML is a visual
reference, not an alternate application or native rendering test.

## 2. Signatures

```rust
ShellGeometry::resolve(
    viewport_width: f32,
    is_mac: bool,
    projects_expanded: bool,
    projects_overlay_open: bool,
    context: ContextVisibility,
    preferred_context_width: Option<f64>,
) -> ShellGeometry;

AppStore::terminal_tab_title(&self, target: &TerminalJumpTarget) -> Option<String>;
```

`Workspace` owns visibility preferences and resolves one `ShellGeometry` for
the caption and body in the same frame. The titlebar emits visibility commands;
it does not own a second copy of project/context preferences.

## 3. Contracts

- Caption and body share the left boundary. With the right sidebar docked,
  their right boundary is also identical: terminal tabs, Add and overflow may
  not extend over the right context sidebar. Its caption region contains the
  right-tool toggle and native window controls, not terminal tabs.
- Use the same live drag width or existing persisted preference for both
  regions. The 284px preview default applies only without a usable preference;
  do not replace a user's saved width on startup, collapse or viewport resize.
- Caption height is 42 logical pixels and individual terminal tabs are 200
  logical pixels. Retain theme/type scaling, stable tool hit areas, horizontal
  overflow, active-tab reveal and a reachable overflow menu.
- The expanded left sidebar is 300px and its collapsed rail is 48px on
  Windows/Linux. macOS must reserve its traffic lights and sidebar toggle
  inside the shared left width, enlarging the rail where necessary. Do not add
  traffic-light space before all caption columns.
- Narrow-window overlays do not change saved expanded/collapsed preferences.
  With the right sidebar hidden/overlaid, reserve only the actual caption
  controls. Native Windows buttons are 46px each; calculate their space from
  the same constants used by rendering, not from the HTML's mock buttons.
- Reuse `middle_column_visible` for the normal project-sidebar preference.
  Context visibility and `ContextPanel` selection remain window-owned.
  Hiding a panel gates its existing visibility API without changing the chosen
  tool, worktree caches, document drafts, terminal entities or Agent routes.
- Caption controls, tab tools and sidebar toggles are not inside window-drag
  hit regions. Keep native Windows control actions in `WindowControlArea` and
  retain only the existing Linux click fallback. Never cache the titlebar:
  native hitboxes must be registered on every painted frame.
- Tab titles use the same exact-owned custom/session/live title precedence as
  runtime diagnostics, but their fallback omits the generated identity suffix.
  Do not parse an already formatted diagnostic label to remove brackets or hex
  text. User titles containing such text are legitimate and remain intact.
- Display titles are not route identities. Retain diagnostic identity suffixes,
  `TerminalJumpTarget` validation, keyboard indices, reorder and per-terminal
  close confirmation even though visible ordinal badges disappear.

## 4. Validation Matrix

| Condition | Required behavior |
| --- | --- |
| Docked right sidebar is dragged | Caption/body boundaries move together |
| Sidebar hides and restores | Same selected tool and worktree-owned content |
| Narrow window becomes wide | Restore preferences; no persisted width rewrite |
| macOS project rail is collapsed | Traffic lights and toggle fit inside left width |
| Many or long terminal titles | Scroll/reveal inside center; tools stay reachable |
| Title is `Review [abcd1234]` | Preserve the full human title |
| Title evidence is absent | Human fallback in tab, identity fallback in diagnostics |
| Captured target becomes stale | No fallback navigation or new terminal creation |

## 5. Cases

- Good: Workspace passes one resolved geometry to both native consumers.
- Base: An existing 400px context width stays 400px when the viewport permits.
- Bad: A titlebar independently subtracts a fixed right width while the body
  uses a live resize value, or allows tabs across the sidebar's header.

## 6. Tests

Run `shell_geometry::tests::`, `title_bar::tests::`, `orca_sidebar::navigation_tests::` and
`store::context::tests::` in GitHub Actions. Windows CI must first discover a
nonempty suite and then execute it; compilation alone is not a geometry gate.
Cover docked/hidden/overlaid columns, resize bounds, saved preferences, native
control reserves, title precedence and unchanged diagnostic fallback. Review
the actual render consumers as well as the pure geometry helper.
Use inline const assertions for fixed caption/rail/control-width invariants;
keep viewport-derived geometry and command behavior in runtime regression tests.

All compilation, formatting, lint, tests and UI harnesses are Actions-only.
Native hit testing, visual alignment and real pointer/focus behavior still need
acceptance using the matching Actions-produced application artifact.

## 7. Wrong Vs Correct

Wrong: remove the last bracketed suffix from `view.pane_label` and use it as a
new terminal name, or let caption navigation consume all width after the logo.

Correct: request the exact-target readable projection and render navigation
inside the center width supplied by the same geometry used for the body.
