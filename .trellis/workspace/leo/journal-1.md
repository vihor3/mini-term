# Journal - leo (Part 1)

> AI development session journal
> Started: 2026-08-26

---



## Session 1: Activity Bar 悬浮提示改进

**Date**: 2026-08-26
**Task**: Activity Bar 悬浮提示改进
**Package**: mt-ai
**Branch**: `fix/activity-bar-hover-labels`

### Summary

实现 VS Code 式 Activity Bar 悬浮标签：首次延迟 500ms、侧边栏内切换立即显示、右侧垂直居中；GitHub Actions 三平台发布构建成功并生成 Windows 测试安装包。

### Git Commits

| Hash | Message |
|------|---------|
| `31ad0cb` | (see git log) |

### Status

[OK] **Completed**


## Session 2: Remote file management overhaul

**Date**: 2026-08-27
**Task**: Remote file management overhaul
**Package**: mt-ai
**Branch**: `feat/remote-file-management`

### Summary

Implemented VS Code-like local/remote file operations, remote path browsing, transfer conflict handling, configurable downloads, terminal cwd support, guarded deletion/transfers, and GitHub-only Rust quality gates; final Actions run 33052419858 passed.

### Git Commits

| Hash | Message |
|------|---------|
| `b3a9e85` | (see git log) |
| `7aa43a3` | (see git log) |
| `ad09416` | (see git log) |
| `4aca5d4` | (see git log) |
| `28f2d0b` | (see git log) |
| `820c62e` | (see git log) |
| `2d2c393` | (see git log) |
| `b7fa18f` | (see git log) |
| `21c0eaa` | (see git log) |
| `0e0db07` | (see git log) |
| `2ed5409` | (see git log) |
| `830717d` | (see git log) |
| `747dcbb` | (see git log) |
| `5ffc7d0` | (see git log) |
| `60dcf6e` | (see git log) |
| `4d9fc4c` | (see git log) |

### Status

[OK] **Completed**


## Session 5: Orca project workbench shell
<!-- trellis-session: v=2 fp=39103d55e835164c -->

**Date**: 2026-09-02
**Task**: Orca project workbench shell
**Branch**: `feat/remote-file-management`

### Summary

Delivered the Orca-aligned Project to Worktree shell, worktree-scoped workbench preview behavior, docked context tabs, Agents overlay, and a verified Windows installer built entirely in Docker.

### Main Changes

- Made the Orca shell the default with Project to Worktree navigation and Usage/Settings footer.
- Added worktree-scoped active-page restoration, replaceable preview tabs, and file-row double-click rename.
- Built and unpacked Mini-Term_1.2.2-orca-20260902_x64-setup.exe with validated PE resources and ConPTY hashes.

### Git Commits

| Hash | Message |
|------|---------|
| `5188188` | feat: add Orca project workbench shell |
| `9234947` | docs: record Orca workbench validation |

### Testing

- [OK] Docker check/clippy and 34 focused tests passed; full workspace tests had one pre-existing out-of-scope DnD failure.
- [OK] Windows x64 main/sidecars built with cargo-xwin; NSIS payload hashes and Orca binary markers passed.

### Status

[OK] **Completed**

### Next Steps

- Run the installer on a physical Windows host and confirm the visible Orca shell; Wine lacks bcryptprimitives.dll and icuuc.dll for this smoke test.


## Session 6: Stable worktree workbench identity
<!-- trellis-session: v=2 fp=c11768e54fedb76a -->

**Date**: 2026-09-02
**Task**: Stable worktree workbench identity
**Branch**: `feat/remote-file-management`

### Summary

Added stable host, repository, worktree, tab, pane, terminal-session, and incarnation identities; migrated layouts additively; fenced workbench callbacks by WorktreeId; and produced the Docker-built Windows installer.

### Main Changes

- Added the mt-identity crate and authoritative/provisional worktree identity resolution.
- Persisted and migrated worktree-scoped layouts, terminal bindings, documents, previews, searches, and focus restoration.

### Git Commits

| Hash | Message |
|------|---------|
| `0386e6b` | feat: add stable worktree workbench identity |

### Testing

- [OK] Docker focused checks, workspace checks, changed-line rustfmt, and changed-line Clippy passed except the documented pre-existing DnD test.
- [OK] Built and structurally verified Mini-Term_1.2.2-stable-identity-20260902_x64-setup.exe (SHA-256 a1a482d6fb51499cc212656230c9ee8a8c05894057c0fc5ea998f034ddc246c0).

### Status

[OK] **Completed**

### Next Steps

- Continue the parent runtime task with detached terminal ownership and warm reattachment.


## Session 7: Orca Project Worktree Runtime 完整交付
<!-- trellis-session: v=2 fp=224415168689428f -->

**Date**: 2026-09-03
**Task**: Orca Project Worktree Runtime 完整交付
**Branch**: `feat/remote-file-management`

### Summary

完成十阶段 Orca Project -> Worktree 架构的父任务集成验收，修复 Docker 集成门禁和 sidecar 锁漂移，验证真实 OpenSSH 远程运行时，并生成最终 Windows 安装包。

### Main Changes

- 归档全部十个子任务与父任务，补齐可独立校验的 Trellis 上下文和验证记录。
- 同步 sidecars/Cargo.lock，并把独立 sidecar 工作区的 locked release 约束写入 SSH runtime spec。
- 生成并结构化验证 Mini-Term 1.2.2-orca-final-20260903 Windows x64 安装包。

### Git Commits

| Hash | Message |
|------|---------|
| `25430a0` | test: stabilize Docker integration gates |
| `c644ae9` | fix: synchronize sidecar runtime lockfile |
| `75ac3d1` | chore(task): complete Orca runtime integration |

### Testing

- [OK] Docker workspace tests: 1840 passed, 1 ignored；check、clippy 与 Windows xwin checks passed。
- [OK] Docker sidecar locked metadata/check/test: 94 tests passed；Windows release sidecars passed。
- [OK] 真实 OpenSSH 9.2 冒烟两次通过，覆盖认证、session 复用、exec、SFTP 与稳定远程身份。

### Status

[OK] **Completed**

### Next Steps

- 在真实 Windows 桌面安装并做最终 GPU/UI 视觉冒烟；当前 Linux/Wine 环境缺少 Windows 系统 DLL，未声明截图通过。
