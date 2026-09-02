# 文件工作区与远程文件编辑 — Technical Design

## 1. Delivery and Branch Boundary

本任务独立于已归档的 PR 整改任务。实现使用新的干净 worktree/分支：

1. PR #54 未合并期间，以 `feat/remote-file-management-v2` 的最新提交作为临时开发基线，因为工具栏、上传/下载和远程文件操作服务都在该分支。
2. PR #54 合并后获取最新 `origin/main`，把本任务的新增提交 rebase 或按逻辑边界 cherry-pick 到新分支。
3. 新 PR 只包含本任务的产品代码、i18n 和测试，不重复包含 #54 的提交，不包含 `.trellis/`。

若 #54 在开发期间发生接口变化，优先适配合并后的 `main`，不在新任务里复制旧实现。

## 2. Workbench Host Instead of PaneState Union

### 2.1 Boundary

在 `Workspace` 主内容槽和现有 `TerminalArea` 之间新增工作区宿主：

```text
Workspace
  └─ WorkbenchArea
       ├─ workbench tab strip
       ├─ Terminal page -> existing Entity<TerminalArea>
       └─ Document pages -> Entity<FileDocument> per open file
```

`TerminalArea` 实体始终由宿主持有；切到文件页时只是停止渲染终端页，不销毁 PTY、分屏树或 UI 状态。

不修改 `PaneState`、`SplitNode`、`SavedPane` 或 `ProjectPanel` 的终端语义。这样避免把文件页传播到 PTY hydration、AI 状态、终端关闭确认、移动端快照、终端列表和布局数据库。

### 2.2 Runtime model

建议新增：

```text
DocumentKey = project_id + backend identity + normalized path
WorkbenchPage = Terminal | Document(DocumentKey)
ProjectDocuments = ordered tabs + active page
```

- 每个项目维护自己的页签顺序和当前页。
- 同一 `DocumentKey` 去重并激活已有实体。
- 每个文档页持有独立编辑器、撤销栈、dirty、preview/source 和加载任务。
- 文件页签不落盘；项目删除时丢弃该项目全部文档实体。
- SSH 连接指纹/代次属于 key 和读写凭据。连接变化后旧页签进入失效态，不能借新连接静默保存。

### 2.3 Shared open entry point

文件树与全局搜索都需要打开文档。使用一个由 `Workspace` 安装并持有的文档控制器/工作区实体作为统一入口；不要让 `FileTree` 持有 `TerminalArea`，也不要在两个入口各建一份页签状态。

入口参数包括项目 ID、后端快照、项目根、目标路径和可选 1-based 高亮行。控制器负责规范化 key、去重、激活和焦点转交。

## 3. Refactor Existing FileViewer into FileDocument

### 3.1 Reuse boundary

保留现有 `file_viewer.rs` 中以下能力：

- 扩展名到 tree-sitter 语言映射；
- `InputState::code_editor`、行号、自动缩进、缩进参考线和搜索；
- LF/CRLF 探测、编辑归一化和保存还原；
- dirty/saving/error 状态；
- 本地 watcher 与外部修改 banner；
- Markdown/HTML 渲染、表格/图片分段和预览缓存；
- 1 MiB、二进制和不可编辑分支；
- 搜索命中行定位。

把 singleton `CURRENT`、overlay `open_guarded/close_guarded`、弹窗尺寸和 Esc 关闭从文档实体中拆出。文档实体向宿主暴露/发送：标题、路径、dirty 变化、保存状态、关闭请求结果和焦点方法。

### 3.2 Embedded behavior

- tab 自身提供关闭按钮和 dirty 标记，文档内工具栏不再重复显示 modal 的关闭按钮。
- Ctrl/Cmd+S 仍由文档容器处理。
- Esc 不关闭文件页；编辑器搜索面板可自行消费 Esc。
- Ctrl/Cmd+W 由 `WorkbenchArea` 关闭当前文件页并调用文档的未保存确认。
- Markdown/HTML 初始 `preview = true`；普通文本初始显示编辑器。
- 切页只隐藏实体，不调用 `navigate` 覆盖另一份草稿。

如果仍需兼容旧 modal API，保留极薄的 adapter，但文件树和全局搜索都改走工作区入口，避免双状态源。

## 4. Document Backend Contract

新增本地/远程统一文档来源：

```text
DocumentSource::Local { project_root, path }
DocumentSource::Remote {
  project_id,
  connection_id,
  connection_fingerprint,
  project_root,
  path,
}
```

统一读结果沿用 `FileContentResult` 的 `content / is_binary / too_large` 语义。文档层不直接操作 SFTP；它只调用本地 `mt_project::fs` 或 `remote_ssh` 服务接口。

## 5. Remote Read and Save

### 5.1 Bounded read

在 `remote_ssh.rs` 增加同步服务函数，由 GPUI background executor 调用。服务函数在内部 Tokio runtime 中：

1. 根据当前连接快照获取/重连 session；
2. canonicalize 项目根；
3. 校验目标是根内叶子，且 `lstat` 类型为普通文件；
4. 拒绝符号链接和特殊节点；
5. 读取最多 `MAX_FILE_VIEW_SIZE + 1` 字节；
6. 超限返回 `too_large`；UTF-8 解码失败返回 `is_binary`；正常文本返回内容；
7. 返回规范化远端路径和加载基线供保存校验。

不能把远程 POSIX path 交给本地 `Path::canonicalize` 或本地默认应用。

### 5.2 In-memory staged write

在 `mt-ssh::SftpHandle` 增加受限内存内容写入原语：

1. 要求目标已验证为普通文件且内容不超过 1 MiB；
2. 在目标同目录用排他 CREATE 创建唯一 `.partial` sibling；
3. 分块 `write_all`、flush、shutdown；
4. 调用现有 staged replacement/backup-swap 逻辑提交；
5. 任一步失败清理 staging；替换失败按现有机制恢复 backup，并保留完整错误链。

不创建本地临时文件再调用 upload，避免权限、清理、磁盘空间和路径身份的新风险。

### 5.3 Optimistic conflict protection

远程文档保存时携带“上次加载/保存成功的原始磁盘字节基线”。普通保存流程：

1. 再次确认项目、连接 ID、连接指纹/代次与打开时一致；
2. 重新规范化根和目标；
3. 受限重读当前远端内容；
4. 当前内容与基线不同则返回结构化 `ExternalChange`，不写入；
5. UI 显示“重新加载 / 仍然覆盖”；
6. 只有用户明确选择覆盖后才跳过内容相等检查，但仍重复身份、边界和类型校验；
7. 保存成功后更新基线、disk/saved 和 dirty 状态。

该机制是 SFTP 能力范围内的乐观保护；不宣称提供跨客户端事务锁。连续远程 polling 不进入第一版。

## 6. File-tree Header Actions

在 `file_tree.rs` 的 refresh 后插入五个固定尺寸按钮，并复用已有入口：

- upload file/folder -> `choose_upload_paths`
- paste -> `paste_file_clipboard`
- new file/folder -> `new_entry_prompt`

每次点击时重新调用 `operation_context` / `remote_conn`，不捕获 render 时的路径或连接。可见/可用性通过 `FileBackendIdentity` 判断：

- `Remote`：显示上传，允许粘贴/新建；
- `Local`：不显示上传，允许粘贴/新建；
- `BrokenRemote`：不允许变更操作。

按钮 disabled 状态包含 `operation_busy`；paste 还包含 `file_clipboard.can_paste_into(context)`。tooltip 复用 `fileTree.menu.*` 词条。

## 7. Keyboard and Focus Routing

- `WorkbenchArea` 知道当前是 Terminal 还是 Document。
- 文档页激活时：Ctrl/Cmd+S 保存、Ctrl/Cmd+W 关闭、Ctrl/Cmd+F 由编辑器搜索接管。
- 终端的新建、关闭 pane、分屏、写入路径和重连等 action 增加“当前工作区页是终端”的守卫。
- 切回终端页时把焦点交还之前活动的终端 pane；切回文件页时聚焦编辑器或预览容器。
- 工作区页签的鼠标点击、关闭和项目切换不得触发后台终端 tab 的动画/关闭逻辑。

## 8. Markdown and Unsupported Content

- 本地 Markdown 保持现有相对图片、表格和代码块行为。
- 远程 Markdown 文本、表格、代码块和网络绝对 URL 可按现有富文本路径渲染。
- 远程相对图片不伪装成本地 `file://`；第一版显示占位/无法加载，由后续 SSH asset cache 实现。
- 远程图片文件、其它二进制和超过 1 MiB 内容显示不可编辑提示，并提供下载入口；本地既有图片预览保持不变。

## 9. Tests and Validation

新增或更新的纯 Rust 测试由 GitHub Actions 执行：

- document key 去重、活动页/关闭后选择、项目隔离；
- 打开文件不改变终端 `SplitNode` / persistence 投影；
- 本地/远程 source 路由、连接指纹失效；
- 远程大小边界、UTF-8/二进制、symlink/越界拒绝；
- staging 成功、写失败清理、替换失败回滚；
- 远端基线冲突与显式 force-save；
- Markdown 默认 preview 和搜索命中行兼容；
- 工具栏 capability/order helper。

本地只运行 i18n 生成/幂等、静态搜索和 `git diff --check`。Rust formatting/check/Clippy/test 全部由 GitHub Actions 执行。

## 10. Rollback

- 工作区宿主集成失败：恢复 `Workspace -> TerminalArea` 直接挂载，保留远程 I/O 原语但不接 UI。
- 远程写入审核不通过：先交付只读远程预览，远程编辑入口保持只读，不能降级为非原子直接覆盖。
- #54 合并产生大冲突：从最新 main 新建分支，只移植本任务逻辑提交，不把旧 PR 历史带入新 PR。
