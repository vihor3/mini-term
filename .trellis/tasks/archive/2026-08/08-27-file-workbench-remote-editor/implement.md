# 文件工作区与远程文件编辑 — Implementation Plan

## Preconditions

- [ ] 用户审核并明确批准本 PRD/design/implementation plan。
- [ ] 运行 `task.py start`，任务进入 `in_progress`。
- [ ] 通过实现 sub-agent 注入 `implement.jsonl` 上下文。
- [ ] 从 PR #54 最新 head 建立独立干净 worktree/功能分支；原工作区未跟踪 `.trellis` 不动。

## Phase A — Branch and Baseline

1. [ ] 获取 `origin/main`、fork 和 PR #54 最新 head。
2. [ ] 创建本任务独立 worktree/分支，确认产品工作树干净。
3. [ ] 记录 PR #54 依赖点；禁止 `.trellis/` 进入 staged paths。
4. [ ] 静态确认现有 `FileViewer`、文件树动作和 remote SFTP API 与设计研究一致。

Rollback point：删除本任务临时 worktree/未推送分支，PR #54 分支不动。

## Phase B — Workbench Runtime Model

1. [ ] 新增 `WorkbenchArea`/document controller 与 `DocumentKey`、项目级 open tabs、active page 纯状态。
2. [ ] `Workspace` 改为持有 WorkbenchArea；现有 TerminalArea 作为常驻 Terminal page。
3. [ ] 实现文件页去重、激活、关闭后相邻选择、项目切换和项目删除清理。
4. [ ] 保证文件页状态不进入 `SplitNode`、SavedPane 或布局数据库。
5. [ ] 增加状态机纯测试。

Review gate：打开/关闭文件不创建、销毁或持久化任何终端 pane/PTY。

## Phase C — Embed Existing File Viewer

1. [ ] 把 `FileViewer` 的 singleton/overlay host 与文档状态/渲染拆开。
2. [ ] 为每个文件页创建独立文档实体，保留编辑器、dirty、undo、line-ending、preview 和 watcher。
3. [ ] tab 显示文件图标、文件名、dirty 点和关闭按钮；文档 toolbar 移除重复 modal close。
4. [ ] 实现宿主驱动的未保存关闭确认、保存、焦点恢复。
5. [ ] 文件树本地入口与全局搜索入口改走统一 document controller；保留搜索行定位。
6. [ ] 验证 Markdown 默认预览、源码切换和草稿预览逻辑不退化。

## Phase D — Remote Document I/O

1. [ ] 在 `mt-ssh::SftpHandle` 增加 bounded in-memory read/replace primitive，复用 staged replacement。
2. [ ] 在 `remote_ssh.rs` 增加根内普通文件读取服务，返回统一 `FileContentResult` 和远端基线。
3. [ ] 增加远程安全保存服务：连接身份、根/叶校验、大小限制、staging、rollback。
4. [ ] 增加保存前基线重读与结构化外部变化结果；实现 reload/force-save UI。
5. [ ] 文档层接入 `DocumentSource::Remote`，移除“不支持远程预览” alert。
6. [ ] 断链、连接配置变化、文件删除、symlink、特殊条目和权限错误显示明确状态。
7. [ ] 增加远程边界/错误/回滚/冲突纯测试。

Review gate：任何远程写入都必须能追溯到“当前连接身份 + canonical root + 普通叶子 + staging replace”。

## Phase E — Workbench Tabs and Hotkeys

1. [ ] 绘制 Terminal + documents 工作区页签条，支持点击激活和关闭。
2. [ ] Ctrl/Cmd+S、Ctrl/Cmd+W、Ctrl/Cmd+F 按当前文档路由。
3. [ ] 给终端专属 workspace actions 增加 Terminal page guard。
4. [ ] 切换终端/文档/项目时恢复正确焦点。
5. [ ] 检查窄窗口、长文件名、多个脏页签和主题配色。

## Phase F — File-tree Header Actions

1. [ ] 增加上传文件、上传文件夹、粘贴、新建文件、新建文件夹矢量图标。
2. [ ] refresh 后按要求顺序插入按钮。
3. [ ] 用 backend identity、busy 和 clipboard context 计算可见/禁用状态。
4. [ ] 每次 click 重新解析 operation context/connection，调用已有操作入口。
5. [ ] 复用现有 tooltip i18n；如增加 disabled/remote 文案则同步中英文和生成字典。
6. [ ] 增加 capability/order 纯测试。

## Phase G — Markdown and Unsupported Files

1. [ ] 本地 Markdown 相对资源回归；远程相对资源不得误走本地路径。
2. [ ] 远程 Markdown 保证标题、列表、表格和代码块渲染；相对图片显示明确占位或保持不加载。
3. [ ] 远程 binary/oversize/image 分支提供下载入口，不允许进入文本保存链路。
4. [ ] 核对 unknown extension/plain-text 和 LF/CRLF 行为。

## Phase H — Local Static Checks

1. [ ] 如有 i18n 变化，运行生成器并再次运行确认幂等。
2. [ ] 运行静态搜索，确认旧 remote-preview unsupported 入口已移除、没有第二套文档状态。
3. [ ] `git diff --check`。
4. [ ] 检查 staged/branch diff 不含 `.trellis/`、task、journal 或无关格式化。
5. [ ] 不运行任何 Cargo/rustfmt/Clippy/Rust test 命令。

## Phase I — Trellis Check and GitHub Actions

1. [ ] dispatch `trellis-check` 做全范围静态审查，允许其针对性修复但禁止本地 Rust 工具链。
2. [ ] 按逻辑边界提交，作者使用 `vihor3 <vihor3@gmail.com>`。
3. [ ] 推送本任务分支并触发 GitHub Actions。
4. [ ] 只依据 Actions 日志做精确修复，重复到 formatting/i18n/check/Clippy/test/whitespace 全绿。
5. [ ] 审核长文档/多页签/远程错误的手工交互风险，并记录未自动覆盖项。

## Phase J — Rebase and New PR

1. [ ] PR #54 合并后获取最新 `origin/main`。
2. [ ] rebase 或把本任务逻辑提交移植到基于 main 的干净分支。
3. [ ] 再跑 GitHub Actions 并确认 diff 不重复 #54、不含 `.trellis/`。
4. [ ] 使用 `vihor3` 提交中文 PR，说明本地/远程文件页签、编辑器复用、远程保存安全和已知限制。
5. [ ] 附成功 Actions 链接并核对作者、head owner、mergeability 和文件清单。

## Deferred Follow-ups

- 远程 Markdown 相对图片的 SSH asset cache/custom scheme。
- 文档页签与未保存草稿跨重启恢复。
- 远程持续 watcher/stat polling。
- LSP、补全、诊断、格式化、多光标等完整 IDE 能力。
