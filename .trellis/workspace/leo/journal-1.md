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


## Session 3: 文件工作区与远程文件编辑

**Date**: 2026-08-28
**Task**: 文件工作区与远程文件编辑
**Package**: mt-app
**Branch**: `feat/file-workbench-remote-editor`

### Summary

完成文件树快捷操作、本地/远程文件工作区页签、代码编辑与 Markdown 默认预览，并补齐远程保存冲突和 SFTP staged replacement 安全边界；PR #55 已创建且上游 CI 全绿。

### Main Changes

- 文件树工具栏新增上传、粘贴和新建快捷按钮，文件在主内容区以项目级页签打开。
- 本地/远程文本支持编辑、高亮、缩进、Markdown 默认渲染、冲突处理和安全保存。
- 修复延迟焦点/关闭串页、远程富文本本地 URL、SSH 项目本地搜索及远程图片无效读取。

### Git Commits

| Hash | Message |
|------|---------|
| `d51599bdaf142932c7b7c68eeb62d74f16008c4e` | (see git log) |
| `125797e7c2c8bfebcd05a8d54a9e0fbcee9e63ef` | (see git log) |
| `78dda1add65502f5bda25ac0b6e6c5e66c02ad0f` | (see git log) |
| `b7dbb0210aa4adf990f0b331a1357c1f428a11b5` | (see git log) |
| `c239a47b71b56abd884b50d04210d5db5562e239` | (see git log) |
| `77598a8644266681c766a44340801429a5b13297` | (see git log) |
| `425f6de2b2dfb5aba40ca0a1f7b50f1937252561` | (see git log) |
| `b9548c885ab344a8f857dc068077d7a59c6d0012` | (see git log) |
| `2fa55c47b114f9a585f65fa02f674ebe1403033e` | (see git log) |
| `5f1742399bde64b5936f485590d3c55ab3335c02` | (see git log) |
| `adc9f19d874de01522479f0464ad8783809643ce` | (see git log) |

### Testing

- [OK] GitHub Actions 33096203975 全部通过。
- [OK] 上游 PR Actions 33098199008 全部通过。

### Status

[OK] **Completed**

### Next Steps

- 等待上游审核并合并 PR #55。

## Session 4: Orca 目标壳原型 UI 评审

**Date**: 2026-09-02
**Task**: 09-01-orca-worktree-terminal-research（planning）
**Branch**: `feat/remote-file-management`

### Summary

评审 `research/orca-ui-mockup.html`（v1），按 P0-P3 出修订版 `orca-ui-mockup-v2.html`：侧栏宽度对齐文档基线（280-340 / 300-380）、断线态改降对比度覆盖层（弃红点）、移除全局 Agents feed（产品确认 MVP 不做，attention 由状态列 + 内联 Agent 行 + Quick Open 承担）、context 条与状态栏合并省 31px、补分屏/pending 创建/三类空态演示与右栏点击穿透闭环、lucide 换 jsdelivr（unpkg 不在 artifact 白名单、cdnjs 未收录）。结论已回写 prd.md / design.md / implement.md / orca-ui-ux-mapping.md（含 v2 变更清单）。

### Status

[OK] **Completed**（规划工件更新；任务仍处于 planning，task.py start 待确认）

### Next Steps

- 用户 review mockup v2 与文档回写后，决定是否 `task.py start` 进入执行/归档流程。


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
