# Validation Report

Date: 2026-09-03

## Outcome

The differential audit of `0bc6f28..c644ae9` is complete. The immutable scope
contains 193 changed files, including 93 non-Trellis implementation/release files
and 73 Rust files. Every non-Trellis file was assigned to one of five review domains,
and 29 high-risk cross-layer files received a second pass.

The audit confirmed 29 findings: 13 P1 and 16 P2. Twenty-eight findings were fixed
with focused regression coverage. `TERM-08` remains an explicitly accepted residual:
GPUI-side terminal-host RPC is now bounded, but making pane construction fully async
requires a broader cancellation and orphan-ownership redesign outside this audit.

## Audit Commits

- `514f47e` - harden Orca worktree runtime invariants.
- `54a9620` - move validation and Windows packaging to GitHub Actions.
- `ad4edd3` - record the audit contracts in Trellis specs.
- `4474c21` - publish changed-line rustfmt diagnostics.
- `4b9fb19` - publish full rustfmt diagnostics.
- `fa06581` - apply Actions rustfmt diagnostics.
- `7fa2f35` - synchronize staging workflow assertions.
- `6db04a6` - resolve changed-line Clippy findings.
- `2858d91` - apply the final Actions rustfmt diagnostic.
- `c3194a6` - upgrade task-owned uploads to native Node 24 `upload-artifact@v7`.

## GitHub Actions CI

Final head: `c3194a683264dbc5e448c6945c71097f2b4f2e22`

- Run: `33799663655`
- URL: https://github.com/vihor3/mini-term/actions/runs/33799663655
- Started: `2026-09-03T19:59:54Z`
- Completed: `2026-09-03T20:15:03Z`
- Rust workspace job `100795747761`: success, zero annotations.
- Windows MSVC job `100795748072`: success, zero annotations.
- Changed-line rustfmt: success; zero changed formatting hunks.
- Generated i18n dictionary: success.
- Sidecar and portable-ConPTY Node tests: 10 passed.
- Root and sidecar locked Cargo metadata: success.
- Root workspace `cargo check --locked --workspace --all-targets`: success.
- Sidecar `cargo check --locked --all-targets`: success.
- Changed-line Clippy gate for all affected packages: success. Existing diagnostics
  outside the changed lines remain baseline warnings and did not weaken the gate.
- Root workspace tests: 1,916 passed, 1 ignored, 0 failed.
- Sidecar tests: 94 passed, 0 failed.
- Windows MSVC checks for affected root packages and sidecars: success.
- `git diff --check`: success.

## Windows Package

- Run: `33799663753`
- URL: https://github.com/vihor3/mini-term/actions/runs/33799663753
- Job: `100795747471`, success, zero annotations.
- Started: `2026-09-03T19:59:54Z`
- Completed: `2026-09-03T20:25:16Z`
- Target: `x86_64-pc-windows-msvc`
- Package version: `1.2.2-ci.7`; application version: `1.2.2`.
- Artifact: `Mini-Term_1.2.2-ci.7_windows-x64` (`9911452864`).
- Artifact size: 18,084,660 bytes.
- Artifact digest: `sha256:47f03d33a3519d5353bc35c7369cdfc8a330be27b7126d5f76c49a510193a98a`.
- Artifact expiry: `2026-09-17T20:25:08Z`.
- Download: https://github.com/vihor3/mini-term/actions/runs/33799663753/artifacts/9911452864
- Installer: `Mini-Term_1.2.2-ci.7_x64-setup.exe`, 18,094,115 bytes.
- Installer SHA-256: `1521f8d234e1c1d161cc3889cb1047454cf6df2137e323d74497c193af9e8dd8`.
- Installer PE machine: `0x014c`; product version `1.2.2-ci.7`; numeric file
  and product versions `1.2.2.0`; resource types `3`, `5`, `14`, `16`, and `24`.
- Main executable resources: product/file version `1.2.2`, numeric versions
  `1.2.2.0`, and resource types `3`, `14`, `16`, and `24`.
- NSIS extraction used 7-Zip and discovered 13 files.

All seven required feature markers were present:

```text
MINI_TERM_LEGACY_SHELL
MINI_TERM_TERMINAL_HOST
MINI_TERM_REMOTE_RUNTIME
MINI_TERM_REMOTE_AGENT_STATUS
MINI_TERM_ORCA_WORKTREE_CONTEXT
MINI_TERM_GITHUB_PROJECT_TASKS
MINI_TERM_GLOBAL_AGENT_ACTIVITY
```

All eight packaged payloads matched the staged size, SHA-256, and PE architecture:

| Payload | Size | SHA-256 | PE machine |
|---|---:|---|---|
| `mini-term.exe` | 80,116,736 | `7181162f1ed30dedaf2e05245791af6ef31b160821108a59684db2acbbd99d46` | `0x8664` |
| `miniterm-hook.exe` | 297,984 | `01860c64d95863e1608dfae9f93330e88558dbc7fc21f3d83462664141c36a17` | `0x8664` |
| `mt-ssh-cli.exe` | 4,806,144 | `57577ea0da6bebe63240c81fd47f4708fa052191285810623150a7d9f34fc2a9` | `0x8664` |
| `mt-ssh-mcp.exe` | 5,617,152 | `f4f2bedd5f551fc1416c59abf1c09e59e6dfdd47451af3a5fd8f14540411329c` | `0x8664` |
| `mt-terminal-host.exe` | 2,360,320 | `cfb4322e5230d8fc9c7a50db8c6ca58eec2027e4f86ee058eeb216b9cfd3e66f` | `0x8664` |
| `portable-conpty/conpty.dll` | 109,920 | `39fba2713e2495117b1591ae8c32a3b904bea7aa66069cf7815e2844c76d75d8` | `0x8664` |
| `portable-conpty/x64/OpenConsole.exe` | 1,066,296 | `b7fd936c2668b87b9ecf7b3366dc6568afc1c6f981874cba3e955a1c35cf8160` | `0x8664` |
| `portable-conpty/arm64/OpenConsole.exe` | 1,120,056 | `ed7622fd0d3bedc9ab9f122f5e58edf0def9e7999224f52dd395ba9f54edbe09` | `0xaa64` |

The upload completed through `actions/upload-artifact@v7`, whose action runtime is
Node 24. The final package log contains no Node.js 20 deprecation or runtime-forcing
warning. The matching CI workflow pin is covered by the staging workflow assertions;
its diagnostic upload step was correctly skipped because rustfmt passed.

## Execution Policy And Residual Risk

No local compile, rustfmt, Clippy, test, staging, installer build, installer extraction,
or Docker validation command was run. The final artifact was downloaded only to `/tmp`
to read the workflow-produced `windows-package-validation.json`; the installer was not
executed or independently validated locally.

Task-owned local build artifacts, temporary audit worktrees, and Docker resources were
removed. A physical Windows GPU/UI smoke test was not performed and is not claimed by
this audit. That visual runtime check and the bounded synchronous `TERM-08` path are the
only declared residual platform risks.
