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
    pub foreground: bool,
}
```

The probe may return `LinuxProc` or `Unsupported`. Supported providers are
Claude, Codex, OpenCode, Pi, and Grok. The result carries the immutable
`CachedSession` connection epoch.

## 3. Probe Protocol

- Run one fixed POSIX shell command on the already authenticated pooled
  session. Match every route environment field and protocol version exactly.
- Require one live managed login root with matching PID/start ticks/TTY, then
  prove each candidate's ancestry, session and controlling TTY against it.
  Remote login captures `MINITERM_MANAGED_ROOT_PID`,
  `MINITERM_MANAGED_ROOT_START_TICKS` and `MINITERM_MANAGED_ROOT_TTY` before exec
  preserves that process identity. These are public facts, not secrets or a
  substitute for positive process lineage. Copied route/root environment alone
  does not admit a process from an unrelated terminal.
- On Linux, inspect readable `/proc/<pid>/environ`, `/proc/<pid>/exe`,
  `/proc/<pid>/cmdline`, and `/proc/<pid>/stat` remotely. Provider
  classification happens in the script.
- Permit pathname expansion only for the fixed trusted `/proc/[0-9]*`
  enumeration. Keep `set -f` protection while splitting remote argv/stat text.
  Disabling globbing before that enumeration without a scoped re-enable
  produces a literal path and falsely reports an empty supported inventory.
- Return `mini-term-agent-inventory-v2`, one capability row, at most 64
  `agent/provider/PID/start_ticks/foreground-or-background` tab-separated rows,
  and `end`. Transport output is capped at 16 KiB and the request is time
  bounded. v1 captures lack the required ownership facts and are rejected.
- Interpreter entrypoints/native executable identity identify providers;
  arbitrary prompt or option text does not. Known help/version/noninteractive
  helper modes are excluded. A single positively proven launcher/native-child
  pair in the same process group projects one logical launcher run; independent
  children remain separate. Provider/cwd equality alone never merges runs.
- `foreground` states whether that logical CLI process group owns the managed
  TTY foreground group. It is not Working/Waiting evidence. Owned background
  processes remain in inventory, independent of the active GUI worktree.
- Bound root enumeration, environment/argv/stat capture, ancestry depth and
  candidate count. Recheck process/root identity and foreground group before
  publishing. Raced or incomplete ownership cannot become supported-empty.
- NUL-delimited argv/environment capture must preserve read status, its size
  bound and final NUL. A successful pipeline's last command is not proof that
  its `/proc` read succeeded. Keep empty argv fields and wildcard text literal;
  otherwise an empty option value can consume a following helper-mode flag.
  Compare executable/argv samples and recheck every buffered process identity
  plus the managed root before publishing a successful frame.
- Reject missing/duplicate framing, invalid providers, zero or malformed
  numbers, duplicate process identities, truncation, extra fields, non-UTF-8,
  uncertain channel state, and missing exit status.
- `/proc` or required-tool absence returns `Unsupported`; it is not an empty
  supported inventory. Missing/ambiguous managed-root proof, including legacy
  terminals opened before root capture existed, also returns Unsupported.
  Those terminals need relaunch for this stronger probe; absence/retirement
  cannot be inferred from the missing capability.
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
| Exact public route, managed root, ancestry/TTY and provider PID/start ticks | Return normalized logical process facts |
| Any route field differs | Exclude that process |
| Copied route/root environment in an unrelated terminal | Exclude without managed ancestry |
| One known launcher/native-child pair with same provider/group | One logical run, not two rows |
| Independent CLI processes on an owned terminal | Keep distinct runs; foreground is a separate fact |
| Legacy terminal has no managed-root facts | Unsupported; never confirm absence |
| Required Linux capability is absent | Return `Unsupported`, not supported-empty |
| Framing is ambiguous, truncated, or lacks exit status | Reject the capture |
| Argv contains wildcard text | Keep it literal; never classify expanded filenames |
| Empty argv option value precedes a helper flag | Preserve the empty field and exclude the helper |
| Failed, oversized or unterminated process capture | Reject/unsupported as appropriate; never confirmed absence |

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
  controlled disposable PTY/process tree. Assert positive exact-route/root
  discovery, every mismatched route field, copied-route unrelated-terminal
  rejection, foreground/background preservation, launcher normalization,
  independent children, helper exclusion, PID/start ticks, bounded framing,
  races and wildcard-looking argv safety. String-presence tests alone cannot
  detect a valid-looking command that enumerates no processes.
- Fault injection addresses only fixture PIDs and covers failed/partial/NUL-less
  captures, empty argv options and pre-publication identity changes. A simulated
  stat change is a race fixture, not evidence that the kernel reused a PID.
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
