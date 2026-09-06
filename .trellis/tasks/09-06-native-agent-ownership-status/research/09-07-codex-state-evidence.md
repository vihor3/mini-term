# Research: Codex Current-Screen Task-State Evidence

- Query: How does local Orca detect managed-terminal Codex task state, and what bounded parser boundary can recognize the reported native `Working (2s [U+2022] esc to interrupt)` row without transcript or liveness inference?
- Scope: internal source inspection of external reference checkout `/home/leo/orca`; bounded discovery of locally available upstream Codex source; Mini-Term semantic contract only. Main owns the Mini-Term observation integration trace.
- Date: 2026-09-07

## Findings

### Decision Summary

1. Orca's inspected Codex activity paths are Hook-event normalization and OSC-title interpretation. No production Codex parser for `Working (... esc to interrupt)` was found. Do not describe the proposed screen parser as an existing Orca state algorithm.
2. Orca has a useful, independent composer-structure parser: cursor context, bold Codex prompt glyph, and footer below the composer. Reuse its evidence shape, not its draft-extraction purpose or viewport-relative extraction.
3. For the reported screenshot, recommend a narrow **Working-only** screen decoder: explicit status row beside a current Codex composer/footer, with owner/freshness proven separately. Composer presence, model/cwd, output, and absence of Working do not establish Waiting or Done.
4. No upstream Codex Rust checkout was found in the bounded source-location search. The exact reported Working row is user-supplied screenshot evidence, not a source-verified guarantee about the installed remote Codex version.

### Files Found

Paths below beginning `src/` are relative to `/home/leo/orca`.

| File | Purpose |
| --- | --- |
| `src/shared/agent-hook-listener/providers/codex-events.ts` | Actual Codex Hook-to-task-state mapping and lead/child aggregation. |
| `src/shared/agent-hook-listener/providers/codex-state.ts` | Retained Codex lead state and remote Hook reconciliation. |
| `src/main/codex/codex-hook-definition.ts` | Orca's installed Codex Hook event set. |
| `src/main/codex/codex-hook-script.ts` | Generated Hook transport, not a native TUI parser. |
| `src/shared/synthetic-agent-title.ts` | Provider-specific synthetic title policy; Codex Working synthesis disabled. |
| `src/main/startup/synthetic-title-runtime.ts` | Per-pane Hook-driven title dispatch, kept separate from real PTY output. |
| `src/shared/agent-title-status.ts` | Shared OSC-title status classifier and transition tracker. |
| `src/shared/terminal-output-side-effects.ts` | Per-PTY OSC/title facts, synthetic-frame path, and stale-title provenance. |
| `src/shared/terminal-cursor-line-context.ts` | Cursor-near cells, bold/dim/color/wrap facts, and viewport limits. |
| `src/shared/terminal-composer-draft.ts` | Pure structured composer/footer recognition and draft extraction. |
| `src/main/daemon/headless-emulator.ts` | Existing xterm emulator, parsed-write completion, visible/cursor/tail readers. |
| `src/main/runtime/terminal-wait-detection.ts` | Startup readiness and blocked-prompt checks, not a native Codex Working-row parser. |
| `src/main/runtime/terminal-wait-tail-state.ts` | Retained-text-tail source for startup/wait checks. |
| `src/shared/terminal-composer-draft.test.ts` | Existing source examples for Codex composer, footer variants, and false positives. |
| `src/shared/agent-hook-listener-hermes-codex-droid.test.ts` | Existing source examples for Codex question/ordinary-tool state mapping. |
| `/home/leo/mini-term/crates/mt-ai/src/agent_semantics.rs` | Pure provider-title decoder; Codex titles intentionally supply no semantics. |

### Actual Orca State Algorithm

**Hooks:** `src/main/codex/codex-hook-definition.ts:16` registers SessionStart, UserPromptSubmit, PreToolUse, PermissionRequest, PostToolUse, SubagentStart, SubagentStop, and Stop. These are configured hooks transported by the generated script, not inferred from terminal words (`src/main/codex/codex-hook-script.ts:60`). No installer or transport was executed or configured during this research.

`normalizeCodexEvent`, `src/shared/agent-hook-listener/providers/codex-events.ts:102`, maps:

| Event | Orca state |
| --- | --- |
| SessionStart, UserPromptSubmit, ordinary PreToolUse, PostToolUse | `working` |
| PermissionRequest, PreToolUse for an ask-user-input tool | `waiting` |
| Stop | `done` |
| Unknown event | no status payload |

The ask-user-input exception is explicit at line 113: an auto-allowed question can still be blocked on a human answer. Lead state and child rosters are reconciled at lines 132-177; the file can consult an exact hook-supplied transcript path for child lifecycle bookkeeping. That broader child-orchestration/transcript machinery is not required for this Mini-Term screenshot correction. Likewise, Orca's SessionStart-to-working policy is not a reason to classify process liveness as Working in Mini-Term.

**Titles:** `src/shared/synthetic-agent-title.ts:25` gives Codex `Codex - action required` and `Codex ready` labels but sets `synthesizeWorkingTitle: false`. Its comment says Codex supplies native working OSC frames while final frames can be missed; this is Orca's source assumption, not verification of the screenshot's installed Codex. `shouldDriveSyntheticAgentTitleFromHook` at line 101 permits non-working synthetic states. `src/main/startup/synthetic-title-runtime.ts:122` resolves the exact pane to PTY, stops synthetic working animation before a final/attention label, and at line 48 sends fabricated title frames directly to the title tracker rather than the emulator, transcript, or output counters.

`src/shared/terminal-output-side-effects.ts:184` extracts observed OSC titles in byte order and passes them to `createAgentStatusTracker`. The shared classifier (`src/shared/agent-title-status.ts:181`) recognizes provider markers, then spinner glyphs, agent-name-qualified waiting/permission/idle/working words, and finally name-only idle. Its title tracker at line 85 detects transitions. This is title-specific, not a transcript parser; its broader defaults do not satisfy Mini-Term's stricter unknown-by-default semantic contract.

The same output-side-effect file has a 3-second stale-title clearing timer at line 200, with explicit `staleWorkingTitleClear` provenance. Do not copy that as a task completion/Waiting detector. Silence or unrelated output does not establish a new semantic state.

### Reusable Composer and Current-Screen Boundary

`src/shared/terminal-composer-draft.ts:1` defines an emulator-independent input record containing row text, undimmed text, prompt boldness, wrapping, below-cursor text/color facts, before/after-cursor text, and cursor visibility/position.

The concrete recognition algorithm is:

1. Reject missing context, hidden cursor, or no rows (`:103`). This is a draft/composer gate, not proof that every working Codex version always exposes a cursor.
2. Find the last nonempty below-cursor row (`:69`). It is a footer if either a blank gap precedes a nonwrapped dim row, a blank gap precedes a nonwrapped custom-foreground row, or it matches `^\s*(?:gpt-\S+|o\d\S*)\s+[U+00B7 or U+2022]\s+\S.*$` case-insensitively (`:28`). The notation here expands the two literal separator characters in the source regex.
3. Walk upward from the cursor through wrapped/indented composer continuation rows to a prompt (`:119`). Codex prompt glyphs U+203A and U+00BB require first-visible-cell boldness **and** a detected footer. A different provider's U+276F prompt requires a preceding frame rule.
4. Treat `Ask Codex to do anything`, `Ask a follow-up question`, and `Try` followed by a quote as placeholders, not typed drafts (`:88`). This never maps the placeholder to idle or Working. Typed draft content is allowed; a placeholder-only status implementation would miss work while users compose the next prompt.
5. Stop continuation extraction at composer rules, the footer, or suitable blank boundaries; preserve hard/soft line-wrap differences (`:30`, `:132`).

The extraction helper is **not scroll-independent**: `src/shared/terminal-cursor-line-context.ts:71` correctly locates the cursor at `baseY + cursorY`, but clamps surrounding rows using `viewportY` at lines 81 and 94 and reports viewport-relative coordinates at line 123. `src/main/daemon/headless-emulator.ts:307` also uses `viewportY` for visible lines. `getBufferTailLines` at line 330 walks the entire buffer tail and is not a current-screen-only contract either.

The useful ownership split is therefore emulator-owned cells -> bounded plain context -> pure provider decoder, not a new ANSI stripper or a new terminal model. `src/main/daemon/headless-emulator.ts:170` also illustrates waiting until the emulator has actually parsed the output before combining cell state with side facts. The existing Mini-Term Alacritty emulator should remain the terminal parser; main has already traced its producer and view-offset caveat.

### Recommended Bounded Mini-Term Decoder

This is a proposed adaptation, **not found Orca code**. Keep provider logic in `mt-ai`, adjacent to `activity_from_owned_title`; add a distinct screen-context entry point so an arbitrary title string cannot become screen evidence. The extractor should remain provider-neutral and return a small immutable context, not an emulator/UI handle. No Mini-Term integration code beyond the title decoder was inspected here.

- Extract from the **active live grid**, excluding scrollback and display offset, immediately after accepted real output has been applied. Keep actual grid cursor coordinates, dimensions, buffer kind, relevant wrap/style facts, and original capture identity/time. Do not derive capture time from rendering, polling, scrolling, attachment replay, or re-reading a cached grid.
- Bound the row window around the actual cursor/composer, not necessarily the physical bottom of a large terminal: inline Codex can occupy only a short area near the top. A proposed initial cap is 16 rows before and 8 after the cursor, 32 KiB total decoded context, with fail-closed handling when the complete candidate region is unavailable. These are design limits, not Orca/upstream constants.
- Recognize one current Codex composer and a structurally valid footer below it. Retain bold prompt and gap/wrap metadata so plain shell prompts and quoted examples do not qualify. For the initial reported shape, require the supported model/cwd footer or equally strong explicit Codex chrome; arbitrary colored/dim text is weaker evidence than Orca's draft parser needs and should not independently authorize task semantics.
- Restrict the status search to the small status area immediately above that composer, across only allowed blank/rule separators and supported soft wraps. Require the explicit `Working (` header, a bounded supported elapsed-time field, U+2022 separator, and `esc to interrupt)` suffix. The screenshot supplies this specific vocabulary. Do not broaden to any line containing `working`, arbitrary tool prose, arbitrary spinner glyphs, or any duration-like sentence. Prefix glyphs and additional duration/header formats need explicit source/screenshot evidence before support is claimed.
- Start with `Some(Working)` for a complete supported pattern; otherwise no semantic observation. The composer remains available during work, so a placeholder, ready banner, or missing Working row cannot prove Waiting, Done, or process exit. New approval/question/idle grammars require their own stronger shapes and evidence.
- Complete-screen parsing must respect normal ANSI/UTF-8 chunking and synchronized redraws. Do not publish a half-erased or half-repainted combination of an old status row and a new composer. Reuse existing emulator/parser completion state; unavailable or transient context remains inconclusive.
- An old positive row must not gain a new semantic timestamp merely because unrelated output arrived. Changes to the semantic row, such as its elapsed counter, can establish a new capture; full-screen churn cannot. Preserve the original capture through any deferred owner confirmation, and do not normalize away the counter before freshness/change detection.
- Reuse the exact run/route/provider/process-or-session/epoch acceptance boundary. Screens prove no ownership by themselves and cannot override Hook semantics. Preserve stale last-known state and the existing 15-second semantic-age contract instead of manufacturing Working from continued process samples.

### Recommended Bounded Tests

Author through the implement agent; **run all cases only in GitHub Actions**. No fixtures were created and no tests were run by this research.

| Case | Required result |
| --- | --- |
| Reported Working row, active Codex prompt, model/cwd footer | Working from screen evidence without OSC title or Hook. |
| Same frame with typed composer draft; supported prompt glyph variants | Same Working result; placeholder text is not required as the sole anchor. |
| Live grid remains Working while user scrolls into history | Same detection independent of display offset. |
| Historical Working row but live composer is idle; Working only in scrollback | No Working observation. |
| Quoted status in transcript, shell output, typed draft, or unrelated title | No semantic observation, including the whole literal Working phrase. |
| Banner/model/directory or placeholder alone; quiet output/process samples | No Working/Waiting/Done inference. |
| Candidate far above composer, wrong/missing footer, shell prompt, overlay, conflicting composers, oversized/partial context | Fail closed. |
| Compact inline TUI near grid top with blank rows below | Recognized when all supported anchors are in the bounded cursor context. |
| CR/erase/cursor-motion repaint removes Working, then unrelated output | Old row cannot persist through a transcript buffer or gain fresh semantics. |
| UTF-8/ANSI sequences split at chunk boundaries, supported soft wrap, synchronized redraw boundary | Decode only a complete coherent frame; no cross-frame false positive. |
| Counter advances versus identical row plus unrelated output | Genuine semantic-row update may be fresh; unchanged old row retains capture time. |
| Deferred delivery, old epoch/incarnation/PID-start pair, independent sibling run, Hook Waiting | Existing semantic owner/freshness fences reject stale/wrong-owner evidence and preserve Hook authority. |

Existing source-only precedents: `src/shared/terminal-composer-draft.test.ts:149` and `:166` cover both Codex prompt glyphs; `:183` and `:201` show configurable footer variants; `:219` rejects a shell prompt without footer. `src/shared/agent-hook-listener-hermes-codex-droid.test.ts:161` and `:219` distinguish ask-user-input waiting from ordinary tool working. These examples have not been executed here.

### External References and Versions

- Local Orca checkout: `/home/leo/orca`, package version `1.4.178-rc.2` (`package.json:3`). Source contents were read without git operations; no immutable revision is asserted.
- Orca's checked-in Codex Hook contract Actions job pins `@openai/codex` `0.150.1` (`.github/workflows/pr.yml:313` and `:327`). This does not establish the user's remote binary version or a passing test result.
- Orca pins `@xterm/headless` `6.1.0-beta.302` and `@xterm/xterm` `6.1.0-beta.303` (`package.json:158`, `:223`). These identify the reference API shape; no dependency change is proposed.
- Upstream Codex is linked as `https://github.com/openai/codex` in Orca's `README.md:177`. No upstream page or source was fetched; no claims here depend on current online documentation.

### Related Specs and Task Documents

- `.trellis/workflow.md`: source research persisted in the active task; implementation/check dispatch remains main-owned.
- `.trellis/spec/mt-ai/backend/agent-runtime-contract.md`, Owned Semantic Activity: exact ownership, Hook priority, original capture freshness, no generic PTY recency, and bounded provider-specific decoding. A screen decoder is a separate new semantic source, not permission to weaken the existing title rejection.
- `.trellis/spec/mt-ai/backend/index.md` and `quality-guidelines.md`: mostly placeholders; the executable runtime contract is the useful package-specific guidance.
- `.trellis/spec/guides/code-reuse-thinking-guide.md`: shared decoder and normalization at the data owner; no duplicate per-consumer parsing.
- This task's `prd.md`, `design.md`, and `implement.md`, plus parent `09-06-native-ui-remote-feedback/prd.md`: owned native terminals only, Unknown without evidence, Actions-only verification, no new remote Hook-secret transport.

## Caveats / Not Found

### Main Follow-up: Official Codex Source

Main subsequently inspected official OpenAI source through GitHub, pinned to
`4aec23384e85734bbae3a3eed06a9218babc4e51` (the queried main revision).
This is reference-source evidence, not attestation of the user's remote binary.

- [Status indicator](https://github.com/openai/codex/blob/4aec23384e85734bbae3a3eed06a9218babc4e51/codex-rs/tui/src/status_indicator_widget.rs):
  `fmt_elapsed_compact` uses seconds, minutes plus padded seconds, or hours plus
  padded minutes/seconds. Its native test includes the reported elapsed/Esc row.
- [Bottom pane](https://github.com/openai/codex/blob/4aec23384e85734bbae3a3eed06a9218babc4e51/codex-rs/tui/src/bottom_pane/mod.rs):
  `set_task_running` and `hide_status_indicator` deliberately have different
  semantics; streaming can hide the row while the task remains active.
- [Composer](https://github.com/openai/codex/blob/4aec23384e85734bbae3a3eed06a9218babc4e51/codex-rs/tui/src/bottom_pane/chat_composer.rs):
  the empty-composer shortcuts hint is not gated on idle state. A model/cwd
  footer or an editable input area is not positive Waiting evidence.

The implementation therefore must not replace missing Working with an invented
Waiting or Done. The supplied screenshot's model footer also contains a reasoning
token between model name and separator; fixtures must preserve that shape.

- Searched Orca source for the exact interrupt/placeholder strings and related Codex state, title, readiness, and composer paths. The interrupt string was present in a synthetic ANSI fuzz-stream word list, not a production Codex Working detector (`src/shared/agent-tui-ansi-fuzz-stream.ts:53`). This is a bounded not-found result, not proof about every Orca revision.
- No upstream `codex`/`codex-rs` checkout was found under the inspected home-level source layouts or bounded `tmp`/Downloads source-name locations. No installed executable, user Codex configuration, credentials, session history, process table, SSH host, or device was queried. Hidden user configuration was deliberately not searched.
- The screenshot image itself was not supplied to this sidecar; its visible row/composer/footer were supplied by main. Cursor position, cell styles, exact spacing, native elapsed-time variants, and remote Codex version remain unverified.
- The role-isolated research protocol excludes `implement.jsonl` and `check.jsonl`; task prose and applicable specs were loaded directly instead. No other task output was modified.
- All work was local source/document reads and this task research file. No tests, builds, format/lint/syntax checks, probes, automated verification, local fixtures, app/SSH/device processes, git operations, staging, commits, or pushes were performed. Main's Mini-Term integration trace was not duplicated. Existing dirty product paths were untouched.
