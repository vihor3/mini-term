# Remote Agent Inventory Contract

## 1. Scope / Trigger

Use this contract to identify agent processes on an authenticated SSH host for
one exact mini-term terminal route. `mt-ssh` returns normalized facts only; raw
remote environment and command lines never cross the transport boundary.

## 2. Request And Result

```rust
pub struct RemoteAgentRoute {
    pub protocol_version: u32,
    pub execution_host_id: ExecutionHostId,
    pub worktree_id: WorktreeId,
    pub tab_id: TabId,
    pub pane_key: PaneKey,
    pub terminal_session_id: TerminalSessionId,
    pub terminal_incarnation_id: TerminalIncarnationId,
}

pub struct RemoteAgentProcess {
    pub provider: RemoteAgentProvider,
    pub pid: u32,
    pub start_ticks: u64,
}
```

The probe may return `LinuxProc` or `Unsupported`. Supported providers are
Claude, Codex, OpenCode, Pi, and Grok. The result carries the immutable
`CachedSession` connection epoch.

## 3. Probe Protocol

- Run one fixed POSIX shell command on the already authenticated pooled
  session. Match every route environment field and protocol version exactly.
- On Linux, inspect readable `/proc/<pid>/environ`, `/proc/<pid>/exe`,
  `/proc/<pid>/cmdline`, and `/proc/<pid>/stat` remotely. Provider
  classification happens in the script.
- Permit pathname expansion only for the fixed trusted `/proc/[0-9]*`
  enumeration. Keep `set -f` protection while splitting remote argv/stat text.
  Disabling globbing before that enumeration without a scoped re-enable
  produces a literal path and falsely reports an empty supported inventory.
- Return only a fixed UTF-8 header, one capability row, at most 64
  `provider/PID/start_ticks` rows, and a footer. Transport output is capped at
  16 KiB and the request is time bounded.
- Reject missing/duplicate framing, invalid providers, zero or malformed
  numbers, duplicate process identities, truncation, extra fields, non-UTF-8,
  uncertain channel state, and missing exit status.
- `/proc` or required-tool absence returns `Unsupported`; it is not an empty
  supported inventory.
- Transport failures may retire only the exact failed pooled session and may be
  retried once. Protocol and remote-state errors are not reconnect loops.

### Security

Route values use POSIX single-quote escaping. The response must never include
raw environment values, argv, credentials, private-key material, Hook tokens,
or arbitrary command output. PID plus Linux start ticks is required so PID
reuse cannot impersonate an existing run.

## 4. Validation Matrix

| Condition | Result |
| --- | --- |
| Exact public route and known provider with PID/start ticks | Return normalized process facts |
| Any route field differs | Exclude that process |
| Required Linux capability is absent | Return `Unsupported`, not supported-empty |
| Framing is ambiguous, truncated, or lacks exit status | Reject the capture |
| Argv contains wildcard text | Keep it literal; never classify expanded filenames |

## 5. Good / Base / Bad

- Good: a generated-command test finds only its explicitly owned fixture and
  rejects each mismatched route field.
- Base: a successful exact-route scan with no matched process returns an empty
  supported inventory, subject to application-owned absence hysteresis.
- Bad: a command-string test passes while `set -f` prevents all enumeration.

## 6. Required Tests

- Command includes all route fields and contains no raw process-data output.
- Supported and unsupported frames parse successfully.
- Ambiguous, duplicate, malformed, truncated, and over-cap responses fail
  closed.
- Timeout and uncertain channel states retain transport/retirement
  classification.
- On Linux in GitHub Actions, execute the actual generated command against a
  controlled disposable process. Assert positive exact-route discovery,
  mismatched route-field rejection, provider normalization, PID/start ticks,
  bounded framing, and wildcard-looking argv safety. String-presence tests
  alone cannot detect a valid-looking command that enumerates no processes.
- Fixtures use only public route values and deterministic readiness/cleanup;
  never inspect or terminate the user's Agent as a test fixture. All probe and
  fixture execution is Actions-only, not a local or manually SSH-run check.

## 7. Wrong vs Correct

Wrong:

```sh
set -f
for proc in /proc/[0-9]*; do
  # The loop receives the literal pattern.
  :
done
```

Correct:

```sh
set +f
for proc in /proc/[0-9]*; do
  set -f
  # Only the fixed list expanded; subsequent argv/stat splitting stays literal.
  :
done
```
