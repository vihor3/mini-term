# Remote Agent Inventory Contract

## Scope

Use this contract to identify agent processes on an authenticated SSH host for
one exact mini-term terminal route. `mt-ssh` returns normalized facts only; raw
remote environment and command lines never cross the transport boundary.

## Request And Result

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

## Probe Protocol

- Run one fixed POSIX shell command on the already authenticated pooled
  session. Match every route environment field and protocol version exactly.
- On Linux, inspect readable `/proc/<pid>/environ`, `/proc/<pid>/exe`,
  `/proc/<pid>/cmdline`, and `/proc/<pid>/stat` remotely. Provider
  classification happens in the script.
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

## Security

Route values use POSIX single-quote escaping. The response must never include
raw environment values, argv, credentials, private-key material, Hook tokens,
or arbitrary command output. PID plus Linux start ticks is required so PID
reuse cannot impersonate an existing run.

## Required Tests

- Command includes all route fields and contains no raw process-data output.
- Supported and unsupported frames parse successfully.
- Ambiguous, duplicate, malformed, truncated, and over-cap responses fail
  closed.
- Timeout and uncertain channel states retain transport/retirement
  classification.
