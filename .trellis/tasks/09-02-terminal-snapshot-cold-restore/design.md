# Technical Design

## Boundary

The change spans `mt-terminal`, `mt-terminal-host`, and `mt-app`:

```text
PTY bytes -> host stream sequence -> disk frame + headless emulator
                                      |
                                      v
                         checkpoint snapshot + bounded log
                                      |
GUI attach miss -> explicit restore -+-> new PTY/new incarnation
                                      `-> snapshot applied before live attach
```

`attach` remains attach-only. `restore` is the only operation allowed to turn
durable history into a new live incarnation. SSH terminals remain unchanged.

## Terminal Snapshot Codec

`mt-terminal` owns a versioned compressed snapshot codec shared by host and GUI.
The snapshot contains the active Alacritty grid, source rows/columns, configured
scrollback, active/saved cursor point and template, wrapping flags, and the
bounded bytes of an incomplete ANSI control/UTF-8 sequence. It intentionally
does not restore live-process modes into the new shell.

Decode applies compressed and decompressed byte limits before deserialization,
validates dimensions against the grid, installs the grid/cursors at source
size, primes a fresh parser with the incomplete tail, and leaves view fitting to
the caller.

## Durable History

The terminal host stores each canonical `TerminalSessionId` beneath a safe
UUID-derived directory. `meta.json` binds session, worktree, and current
incarnation. `checkpoint.json` contains a checksummed compressed snapshot and
the last incorporated stream sequence. Both JSON files use
`mt_core::atomic_write`.

`output.log` uses this binary frame order:

```text
magic | version | kind | generation length | generation | sequence |
payload length | payload | crc32(header-after-magic + payload)
```

Output and resize frames share the live incarnation generation. The host
headless emulator consumes the same ordered events before checkpoint rotation.
When the log reaches its byte budget, the host writes a checkpoint through the
current sequence and truncates the log. Exit and orderly teardown also flush a
checkpoint. No spawn arguments, environment values, or autofill password are
written.

Recovery accepts the longest valid prefix only when the remaining bytes are an
incomplete final frame. A fully present frame with a bad checksum, invalid
version/kind, wrong generation, oversized payload, or non-contiguous sequence
is corruption and fails closed.

## Restore Protocol

Add protocol v2 `restore`:

```text
session_id
worktree_id
expected_previous_incarnation_id
spawn (used only to create the new process; never persisted)
```

The host validates durable metadata and history, reconstructs a headless
emulator, then atomically reserves the logical session and spawns a new PTY.
The new session keeps `TerminalSessionId`, rotates `TerminalIncarnationId`,
starts stream sequence at one, and seeds its new history generation from the
recovered snapshot. The response includes the new descriptor and snapshot.

The client applies the snapshot synchronously before opening the attachment;
the normal replay-to-live handoff then covers every new-incarnation output byte.
Old-incarnation mutations continue to fail through existing fences.

## Application State

Replace the warm boolean with a recovery enum:

```text
Fresh | Reattached | RestoredHistory | Compatibility | Unavailable
```

Hydration clears `resume_pending` only for `Reattached`. After
`RestoredHistory`, the existing provider resume command may run and upgrades the
pane-local notice to `Agent resumed`; cold history remains distinct from warm
reattach.

## Rollback

`MINI_TERM_TERMINAL_HOST=0` continues to select the legacy in-process PTY. The
new disk files are additive and ignored by older builds. Reverting this child
does not require deleting stable IDs or layout state.
