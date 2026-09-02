# 技术设计

## 设计目标

- Worktree、pane、terminal、agent 在本地、WSL 和 SSH host 上使用同一套稳定身份。
- GUI 只是状态投影和交互客户端，不拥有长生命周期 PTY。
- 同一终端进程的 warm reattach 与新进程上的 cold restore 有不同协议和 UI 状态。
- 远程 agent identity/status 由执行主机提供权威证据，客户端负责验证、排序和展示。
- mini-term 的主壳向 Orca 靠近：worktree 是左侧导航的一等对象，中央 workbench 只承载 active worktree，右侧栏只展示当前 worktree 的上下文。
- 现有本地 Hook、SSH daemon、layout DB 和 terminal emulator 尽量复用，但不维持不正确的所有权边界。

## 非目标

- 不在 MVP 中扫描或接管 mini-term 之外的任意系统级 agent 进程。
- 不保证机器重启后恢复任意原进程。
- 不把历史会话文件扫描当作 live status 来源。
- 不逐像素复制 Orca 的品牌和 Electron/Tailwind 实现，也不因 UI 对齐引入 Browser、Automations、GitHub/Linear/Jira 全套功能。
- 不在 MVP 中实现独立 Kanban Agent Dashboard；全局 Agents feed、sidebar 内联 Agent 行与 Quick Open 只复用同一实时 target/routing 模型，不引入三套状态源。

## 总体架构

```text
mt-app (GUI projection)
  | workspace/worktree/terminal/agent RPC
  v
execution-host client
  | local socket / authenticated SSH mux
  v
mini-term runtime host
  |- worktree catalog
  |- terminal host (PTY + headless emulator + history)
  |- agent hook endpoint + adapter registry
  |- capability probe + process/session inventory
  `- durable replay/spool
```

本地主机、WSL 和 SSH 远端都抽象为 execution host。MVP 可以先交付本地 host，再让同一协议运行在远端 runtime；不要在 UI 层分别维护三套 identity 和状态逻辑。

## Orca 对齐的 UI/UX

### 目标信息架构

```text
Project Sidebar                Active Worktree Workbench          Context Sidebar
----------------------------   --------------------------------   ----------------------
Search / Agents                independent unified tab strip      Files
Projects header                terminal / open files              Git history + changes
project -> worktree cards      split layout                       GitHub Tasks
Usage + Settings footer        restore/reconnect local states     Agent Sessions
```

这不是三块都显示相同状态：

- 左侧 sidebar 只表达 Project -> Worktree 层级与跨 worktree attention，不在用户侧引入 Workspace 概念。
- 中央 workbench 负责 active worktree 的执行与编辑上下文；同一 project 下每个 worktree 都有独立 tabs/splits/open files。
- 右侧 contextual sidebar 负责 active worktree 的文件、Git、Agent sessions，以及所属 project 的 GitHub Issues/PR。
- 全局 Agents 作为覆盖 workbench 的非模态浮窗存在，不成为新的中央页面，也不改变上述三段的状态归属。

### 左侧 Project Sidebar

建议替换当前独立 44px `ActivityBar` 与 `ProjectList/FileTree` 上下堆叠：

1. 顶部全局导航固定为 Search 与 Agents；Search 提供跨 worktree Quick Open，Agents 提供 live attention/unread 聚合，两者与右侧 session history 分源。
2. `Projects` header 只提供 project options 与 Add Project；不显示 Workspaces，不提供 Project/Status segmented control。
3. 唯一层级为 project/repo -> worktree；host 作为 project/worktree metadata 展示，不成为用户必须操作的额外层级。
4. row model 必须扁平化后虚拟化；project header 与 worktree card 都使用稳定、host-qualified key。
5. 非权威 scan 失败时保留 last-known cards 并显示刷新/连接状态，不闪成空列表。
6. 左下角只显示 Usage 与 Settings：Usage 复用现有 `mt-usage`/`UsagePanel`，Settings 打开现有设置面板；不显示 `Local runtime connected` 常驻文案。

### Worktree 卡片

卡片使用固定状态列，避免状态变化挤动标题：

```text
[20px status] [host/repo identity] display name              [hover actions]
              branch/path · primary/sparse/review
              [activity] [provider] session title · age
              child agent rows...
```

规则：

- main worktree 与 linked worktree 使用相同 card model；`primary` 只是 metadata。
- provider identity 与 activity glyph 分开；例如 spinner + Claude icon。
- 未读优先用标题字重和状态列小标记，不使用占宽的永久 `DONE` 药丸。
- branch/path、PR/issue、ports 等低频信息进入次行或 hover details。
- Agent 行点击必须定位 `WorktreeId + tab + PaneKey`；不能只激活项目。
- remote disconnected 是卡片覆盖信息，不能改写最后 Agent activity。

### 状态视觉词汇

```text
working                      spinner
blocked / waiting / needs you  amber question
done / quiet active         emerald dot/check
failed / interrupted        red dot
idle / unknown              gray dot
plain shell                 no agent state glyph
```

主 UI 不显示 evidence/confidence；诊断面板再显示 attested/hook/process/title、incarnation 和 receipt time。

### 中央 Workbench

- active worktree 决定 tabs/splits/session/open files；切换 worktree 不停止其他 worktree 的后台 PTY，也不复用另一 worktree 的 file tabs。
- `WorktreeWorkbenchState` 按 `WorktreeId` 持久化 `tabs + split tree + active tab + open file descriptors + view state`；project 级状态只记录 active worktree 与共享 remote/task metadata。
- 统一 tab strip 首版承载 Terminal 与 File/Preview；Git diff 从右侧 Git tab 打开后可以生成 worktree-scoped Diff tab，但不作为所有 worktree 的固定 tab。
- terminal tab 前导区按 `activity glyph + provider/shell identity` 展示，同一优先级也用于 Quick Open。
- `TerminalsPanel` 的“项目级面板”可先作为兼容 tab group 存在，逐步并入统一 worktree session；不要同时长期保留两套顶层 tab 导航。

### 右侧 Contextual Sidebar

右侧使用 Orca 式顶部 tab bar，固定顺序为：

- `Files`：只读取 active worktree 根目录，复用现有 `FileTree` watcher 和文件打开 action；切换 worktree 必须先切 scope 再刷新 watcher。
- `Git`：读取 active worktree 的 branch、working tree changes、diff 和 commit history/tree；点击文件或 commit 打开该 worktree 内的 Diff/History tab。
- `Tasks`：在 project 所属 execution host 上解析 Git remote，并通过该 host 的 GitHub CLI (`gh`/`gh.exe`) 只读加载 Issues、Pull Requests 和详情；同一 project 的 worktree 共享缓存，但 selection/filter 可以按 worktree 保存。点击 row 在中央统一 tab strip 打开 worktree-scoped 只读 `WorkItemDetail` tab，普通查看不打开浏览器。
- `Sessions`：扫描 active worktree canonical path 匹配到的 Claude/Codex/Grok 等全部可解析 session，展示 provider、标题、更新时间、live/stale 与 resume；历史扫描不能覆盖 Hook/terminal runtime 的 live status。

顶部 tab 固定存在；具体面板在无数据、无 GitHub remote、`gh` 缺失、未认证、scope 不足或远端断开时显示独立空态，不把整个右栏隐藏。面板内部优先复用现有 `FileTree`、`GitPanel/GitHistory`、`SessionPanel`，新增 execution-host-scoped GitHub task provider。

`active_context_tab` 是 App 级 route，切换 worktree 时保持当前类型，便于连续查看 Git 或 Files。panel 内部的 selection、expanded nodes、scroll、filter、last-known data 与 request generation 以 `WorktreeId` 为 key；Tasks 的数据 cache 以 `ExecutionHostId + ProjectId + GitHubRepoIdentity + auth generation` 共享，但其 UI selection/filter 按 worktree 保存。mini-term 当前 `DrawerPanel` 已是应用级运行时 route，Orca 也使用统一 `rightSidebarTab`，因此不为每个 worktree 再复制 tab route。

Tasks 不直接从 GUI 进程携带 token 请求 GitHub，也不把本地登录态复用到 WSL/SSH。Local project 调用本机平台的 `gh`，WSL project 调用对应发行版里的 `gh`，SSH project 通过已认证 remote exec channel 调用远端 `gh`；命令使用结构化 argv 和 JSON 输出。若目标 host 未登录，Tasks 只显示 host label、`gh auth login --hostname <host>`、Copy 和 Retry。mini-term 不启动登录进程、不创建 terminal tab、也不打开浏览器；用户自行在对应环境完成登录。

GitHub auth 状态至少区分 `client_missing / auth_required / wrong_host_or_account / scope_required / rate_limited / offline_or_disconnected / ready`。认证状态以 `ExecutionHostId + GitHub host` 为 scope，并带 generation；账户切换、remote URL 变化或重连后，旧请求不得覆盖新身份下的数据。环境变量提供的 token 若覆盖 keyring 登录，需要在诊断中明确指出来源，但不能显示 token 值。

Files 使用单个 preview slot per `WorktreeId + TabGroupId`：首次打开追加到当前统一 tab order 末尾，后续 preview 在原位置替换；任何编辑先将 preview promote 为 permanent。文件树整行双击统一路由到 rename，避免 Orca 当前“文件名重命名、图标/空白固定 preview”的命中差异。preview promotion 只由 preview 标签双击、显式 Pin 或编辑触发。

### 全局 Agents Feed

现有 Sessions 入口同时承担历史和 live attention，语义不够清晰。MVP 明确拆分：

- 左侧固定 `Agents`：聚合跨 worktree 的 live/近期 Agent run、needs-attention 与 unread；点击必须精确定位 `WorktreeId + TabId + PaneKey`。
- 右侧 `Sessions`：active worktree 文件夹内扫描到的全部历史会话、正文 preview 和 provider resume。
- Search / Quick Open：按关键词直接命中 live Agent target，但复用与 Agents feed、inline Agent row 相同的激活 action。

Agents 采用覆盖当前 workbench 的非模态浮窗。打开时 active `WorktreeId`、tab、split、终端和右侧 contextual sidebar 均不切换或卸载；浮窗获得临时键盘焦点，关闭后焦点返回打开前的 `PaneKey`。点击 live item 后关闭浮窗并激活 `WorktreeId + TabId + PaneKey`，目标真实获得焦点后才确认对应 run 的 unread。仅打开浮窗不批量清 unread；独立 Kanban Agent Dashboard 继续延后。

现有 `overlay.rs`、`TerminalArea::render_marker_popover` 和 `DatePicker` 已提供防叠开、锚定贴边、点外/Esc 关闭及 previous-focus restore。首版确认复用这条固定锚定浮层路径：新增 `overlay::kind::AGENT_ACTIVITY`，从左侧 Agents 按钮取得窗口锚点，`anchored().snap_to_window_with_margin(...)` 渲染；浮窗有筛选输入时应让 terminal 全局快捷键让路，但 Agents toggle 与 Escape 必须仍可关闭。首版不实现拖动、缩放或 geometry 持久化。

### Worktree 创建

采用后台创建与原位 handoff：

1. composer 收集 Project / Run on / Name / Agent / Start from。
2. submit 后立即关闭，不让 Git/SSH 长操作占住 modal。
3. sidebar 在最终分组位置插入 pending row。
4. 中央用同名 faux tab 显示 `Fetching -> Creating -> Setup -> Starting agent`。
5. 成功后原位替换为真实 worktree/session；若用户已离开则只更新原 pending row，不抢焦点。失败保留错误、Retry 和 Remove；运行中使用 `Run in background`，Cancel 仅在底层支持安全取消时出现。

### 恢复与连接反馈

恢复反馈属于 pane/tab 局部状态，不用启动时全局阻塞弹窗：

- `Reattaching...`：attach-only 进行中。
- `Reattached`：同一 PTY；短暂反馈后消失。
- `Restored from history`：新 PTY + cold visual restore。
- `Agent resumed`：新 PTY + provider resume。
- `Restart required`：历史可见但不能自动继续。
- `Recovery unavailable`：校验失败，可打开干净 shell并查看诊断。

正常 warm reattach 应尽量无感；只有降级与失败持续可见。

### 关闭、移除与 Delete

- 应用退出：detach GUI client，不 kill PTY。
- 关闭 terminal tab：kill PTY；working/needs-you Agent 必须确认。
- Remove Project：只取消 mini-term 登记，不删除磁盘仓库/worktree；若仍有 live PTY，明确显示其将继续 detached 或提供单独停止选项。
- Delete worktree：先完成 terminal teardown/preflight，再执行 Git 删除，使用独立确认流。
- MVP 不提供含义模糊的 Sleep worktree；后续若增加，必须先定义它是 hide、detach 还是批量 stop。

### 当前组件映射

| 当前 mini-term | 目标职责 |
|---|---|
| `activity_bar.rs` | 少量 sidebar nav；Usage 与 Settings 移到底部固定 footer |
| `project_list.rs` | Orca 式 project -> worktree virtualized cards；不再显示 Workspace/Status mode |
| `project_tree.rs` 的 worktree child | 一等 `WorktreeId`；repo 只分组 |
| `file_tree/` | right sidebar `Files`，严格绑定 active `WorktreeId` |
| `session_panel.rs` | right sidebar `Sessions`，按 worktree canonical path 扫描全部 provider history |
| `git_panel.rs`/`git_history.rs` | right sidebar `Git` 的 changes/diff/commit tree |
| 新增 execution-host GitHub task provider | right sidebar `Tasks` 与中央只读 `WorkItemDetail`，通过项目所属 host 的 `gh` 获取 Issues/PR |
| `terminals_panel.rs` | 兼容 tab-group，最终并入 unified workbench tabs |
| `terminal_area.rs` | worktree-scoped workbench + stable tab/pane identity |
| `store/ai.rs` 四态聚合 | 新 activity/connectivity/confirmation 的兼容投影 |
| `overlay.rs` + marker/date popover | Agents 浮窗防叠开、锚定贴边、Esc/点外关闭和焦点归还基线 |

完整视觉对照见 `research/orca-ui-ux-mapping.md`，逐动作与异步所有权规则见 `research/operation-logic.md`。

## 模块边界

### `mt-project::worktree`

新增或重构为以下组件：

- `WorktreeCommandRunner`：执行 Git 命令，处理 timeout、WSL/remote host 和 cancellation。
- `WorktreePorcelainParser`：解析 NUL 和文本 fallback。
- `WorktreeCatalog`：single-flight scan、generation invalidation、strict/lenient policy。
- `WorktreeIdentity`：host-qualified repo/worktree identity。
- `WorktreeMetadata`：名称、pin、archive、order、comment 等用户数据。

libgit2 可继续用于 mutation 或补充校验，但列表事实以 Git porcelain 为主，避免两个来源在 locked/prunable/sparse 等语义上不一致。

### 新增 `mt-github`

负责 GitHub CLI 的领域模型与 host-neutral 编排：

- `GitHubRepoIdentity`：规范化 `host/owner/repo`，来源必须是 execution host 上的 Git remote。
- `GitHubCliPlan`：以结构化 argv 描述 `gh --version`、`gh auth status`、Issue/PR list/view 和 auth 命令，不拼 shell 字符串。
- `GitHubCliParser`：只解析受版本控制的 JSON 字段和有限诊断输出，不依赖本地化的人类可读列表文本。
- `GitHubAuthState`/`GitHubTaskError`：区分 CLI 缺失、未登录、账户/host、scope、rate limit、网络和连接错误。
- `GitHubTaskCache`：按 `ExecutionHostId + ProjectId + GitHubRepoIdentity + auth generation` single-flight、取消和 generation fence。

实际命令执行由注入的 `ExecutionHostCommandRunner` 完成：Local/WSL adapter 使用对应环境的进程执行器，SSH adapter 复用 `mt-ssh` 的 bounded remote exec。`mt-github` 不拥有 SSH 连接、浏览器或 GPUI；`mt-app` 只把 auth remediation 投影为 host-aware 文案、可复制命令和显式 Retry。

### `mt-layout`

负责版本化的 workspace session：

- worktree -> tabs/split tree。
- leaf -> stable pane key。
- pane -> terminal session id + expected incarnation。
- active repo/worktree/tab/pane。
- terminal display metadata。
- agent binding、last-known status 和恢复结果。

schema 采用逐项 salvage。旧布局缺少稳定 ID 时在加载阶段一次性补发 UUID 并写回。

### 新增 `mt-terminal-host`

独立进程，负责：

- PTY spawn/create-or-attach/detach/kill/list。
- session/connection ownership。
- headless terminal model。
- snapshot、checkpoint、incremental output log。
- per-session output sequence 和 incarnation。
- 本地 socket/named pipe 服务与协议握手。

它不与 `mt-ssh-cli daemon` 合并。两者可复用端点所有权、当前用户权限、版本握手、stale socket recovery 和 detached spawn 模式，但故障域与升级节奏保持独立。

### `mt-terminal`

扩展为终端模型和持久化 codec 的所有者：

- `TerminalSnapshot`
- `TerminalReplaySegment`
- mode/cursor/title/cwd/size metadata
- partial escape tail
- checkpoint serializer/deserializer
- snapshot/replay golden tests

GUI 和 terminal host 使用同一语义模型，避免两边对 ANSI/mode 的解释漂移。

### `mt-ai`

新增 execution-host agent runtime 协议：

- provider adapter registry。
- hook envelope normalization。
- agent identity/status state machine。
- evidence ranking、generation fence 和 reconciliation。
- capability probe schema。
- legacy `ai-working/ai-idle/idle` 投影。

现有 `hook_server`/`hook_registry` 可作为第一批 Claude/Codex/Grok adapter 的行为基线，但 endpoint、identity 和状态类型需要升级。

### `mt-ssh`

负责远端 runtime 的安全传输和部署：

- SSH host key fingerprint 获取与校验。
- runtime bootstrap/version handshake。
- RPC/mux、heartbeat、reconnect 和 cancellation。
- remote file transfer 继续复用现有 SFTP 原语。

它不解释 agent provider 事件，也不拥有 worktree/UI policy。

### `mt-app`

只负责：

- 把 catalog/session/status 投影为 Orca 式 project sidebar、worktree workbench tab 和 contextual sidebar。
- 发出用户命令。
- 显示恢复类型、连接状态和错误。
- 保留当前兼容 UI 所需的三态映射。

不直接持有 `PtySession`，不根据终端标题决定权威 agent identity。

## 稳定身份模型

### Execution host

```text
HostInstallId     = runtime 首次启动时生成并持久化的 UUID
HostKeyFingerprint = SSH 握手验证的服务端 key fingerprint；本地主机为 local marker
ExecutionHostId   = hash(host_key_fingerprint, host_install_id)
ConnectionId      = 本次客户端 attachment 的临时路由 ID
ConnectionEpoch   = 每次重连递增
```

`ConnectionId` 不能作为设备身份。远端 runtime 在 wire 中也不能自行声明本地 connection id；客户端根据当前已认证 transport 注入。

### Repo/worktree

```text
RepoId     = hash(execution_host_id, canonical_git_common_dir)
WorktreeId = hash(repo_id, canonical_worktree_path, optional_workspace_instance)
```

路径必须在执行主机上 canonicalize。显示名、branch rename 和本地 SSH 连接名称不参与 identity。

### Pane/terminal

```text
PaneKey                = 持久 UUID，跟随 layout leaf
TerminalSessionId      = 持久 UUID，标识可 reattach 的逻辑终端
TerminalIncarnationId  = host 每次真正 spawn 新 PTY 时生成的 UUID
```

同一个 `TerminalSessionId` cold restore 后会产生新的 `TerminalIncarnationId`。任何旧 incarnation 的输出、Hook 或 kill 命令都必须被拒绝。

### Agent

```text
AgentRunId          = mini-term 启动/首次绑定 agent 时生成的 UUID
AgentType           = 可扩展规范化字符串
ProviderSessionId   = provider 自己的 conversation/thread/session id
AgentEventId        = 幂等 UUID
```

Terminal session 与 provider session 永远是两种 ID：前者定位 PTY，后者用于 provider resume。

## Worktree 数据流

1. Catalog 在 execution host 执行 `git worktree list --porcelain -z`。
2. parser 生成 Git fact rows；旧 Git 才使用文本 fallback。
3. enrichment 以受限并发补 sparse、main/common-dir 等信息。
4. scan result 带 `generation/source/authoritative`。
5. client 只允许 authoritative generation 清理已消失行。
6. metadata store 按 `WorktreeId` 合并名称、pin、archive、order、comment。
7. UI 生成 project -> pinned/normal worktree 的扁平 row model，再虚拟化渲染；host 只作为 project/worktree metadata 和稳定 key 的组成部分，不形成用户侧层级。

建议首版行字段：

```text
row_key, host_id, repo_id, worktree_id, path, branch, head,
is_main, is_bare, is_sparse, locked_reason, prunable_reason,
display_name, pinned, archived, terminal_summary, agent_summary,
scan_source, authoritative
```

## GitHub Tasks 数据流

1. active `WorktreeId` 解析到稳定 `ProjectId + ExecutionHostId`，在 execution host 上读取 Git remote。
2. `mt-github` 规范化 `GitHubRepoIdentity(host, owner, repo)`，并用相同 host 执行 `gh --version` 与 `gh auth status --hostname <host>`。
3. ready 时通过结构化 `gh issue/pr ... --json` 或 `gh api` argv 拉取列表/详情；stdout 只按 JSON 解析，stderr 只用于有限错误分类。
4. list result 写入 project-scoped cache；每个 worktree 只持久化自己的 filter、selection、scroll 和已打开的 `WorkItemDetail` tab identity。
5. 用户点击 Issue/PR 后在中央 workbench 打开只读详情 tab；Markdown 作为受限富文本渲染，外部链接需要显式动作，主详情流程不依赖浏览器。
6. auth_required 时只显示“在 `<execution host>` 运行 `gh auth login --hostname <host>`”、Copy 和 Retry。用户完成登录后点击 Retry，mini-term 重新执行 auth probe；ready 后递增 auth generation 并重新拉取。
7. worktree/project/host 切换、remote URL 变化、账户变化或 remote reconnect 都会取消或 fence 旧请求；迟到结果只能落入其原 cache generation，不能覆盖当前 Tasks。

浏览器不是 GitHub task transport，Tasks 也不主动打开授权页。任何本地、WSL、SSH 凭据都不进入 layout、session、command history、日志或跨 host RPC；用户执行登录后，远端凭据只在远端 `gh` 的 credential store 中落盘。

## Terminal 数据流

### Create

1. UI 创建持久 pane/terminal ID 并先写 layout draft。
2. terminal host 原子执行 `create(session_id, expected_absent, spawn_spec)`。
3. host 生成 incarnation，启动 PTY 和 headless emulator。
4. 成功后返回 session descriptor；UI 提交 layout draft。
5. 失败则回滚 draft，不留下指向不存在 session 的稳定绑定。

### Detach

1. GUI 关闭/退出时发送 detach client。
2. terminal host 保持 PTY、emulator 和 history writer。
3. 只有明确“关闭终端”命令才 kill session。

### Warm reattach

1. GUI 用 `TerminalSessionId + expected incarnation` 请求 `attach_only`。
2. host 返回当前 snapshot、output sequence、size、incarnation。
3. GUI 先应用 snapshot，再订阅大于 snapshot sequence 的实时输出。
4. sequence gap 时停止增量应用并重新请求 full snapshot。

### Cold restore

1. attach_only 返回 session missing。
2. client/host 读取最后完整 checkpoint 和合法增量前缀。
3. host 启动新 PTY，生成新 incarnation。
4. renderer 以历史源尺寸恢复画面，再 fit 新视口。
5. 若保存了 provider session，按 adapter 生成 resume argv。
6. UI 标记 `Restored` 或 `Resumed`，不能显示为 `Reattached`。

## Terminal 持久化格式

每个 terminal session：

```text
meta.json
checkpoint.json
output.log
```

`meta.json` 至少包含：session/worktree/host identity、incarnation、spawn spec 摘要、started/ended time、last cwd/title/size、history generation。

`checkpoint.json` 保存可重放 snapshot；`output.log` 使用 framed binary records：

```text
magic | version | generation | sequence | kind | length | payload | checksum
```

record kind 至少包括 output、resize、clear。写入顺序必须等于 emulator 应用顺序。

恢复规则：

- checksum/length/generation/sequence 不合法立即停止。
- 最后一帧撕裂可截断；中间缺口不可跳过。
- checkpoint 原子 replace。
- 达到大小上限时裁旧 scrollback 或滚动 checkpoint，不静默丢可见 frame。

## 远程 Agent 识别协议

### 1. Capability inventory

execution host 周期或按需执行受控 probe：

```text
agent_type -> executable candidates + required commands + runtime exclusions
```

结果用于设置 UI、launcher availability 和 Hook 安装 allowlist，不绑定到具体 pane。

### 2. Launch attestation

mini-term 启动 agent 时向该 PTY 注入保留环境变量：

```text
MINITERM_HOST_ID
MINITERM_WORKTREE_ID
MINITERM_PANE_KEY
MINITERM_TERMINAL_SESSION_ID
MINITERM_TERMINAL_INCARNATION_ID
MINITERM_AGENT_RUN_ID
MINITERM_AGENT_HOOK_ENDPOINT
MINITERM_AGENT_HOOK_TOKEN
MINITERM_AGENT_PROTOCOL_VERSION
```

用户/project env 不能覆盖这些变量。WSL/SSH 场景由 execution host 直接注入，不依赖本地 `ssh` 客户端的普通环境继承。

### 3. Provider Hook

- 每个 adapter 使用独立 source 路由，例如 `/hook/claude`、`/hook/codex`。
- receiver 从路由和已安装 adapter 确定 source，不让 payload 自报 provider。
- Hook token 用于 endpoint admission；launch token/run id 用于代际关联。
- body 有严格大小、字段长度和 JSON 深度限制。
- provider 原始事件在 host 上归一化为统一状态，同时保留 event name/reason。

### 4. 低置信度 fallback

进程树和 title 只在 Hook 尚未到达时提供临时识别：

- terminal host 查询自己拥有的 PTY 前台 process group。
- executable signature 只返回规范化名称，不上传完整敏感 cmdline。
- process/title evidence 不覆盖同 incarnation 内的 attested Hook identity。

## Agent 事件契约

```text
AgentStatusEvent {
  protocol_version,
  event_id,
  relay_instance_id,
  relay_sequence,
  execution_host_id,
  worktree_id,
  pane_key,
  terminal_session_id,
  terminal_incarnation_id,
  agent_run_id,
  agent_type,
  provider_session_id?,
  activity,
  reason?,
  evidence,
  provider_event?,
  is_replay,
  occurred_at_remote?,
  received_at_local
}
```

`execution_host_id` 和 `received_at_local` 由可信接收层覆盖/生成，不接受远端 payload 的同名声明。

## Agent 状态模型

活动状态和连接状态分轴：

```text
ActivityState:
  starting | working | blocked | waiting | done | failed | interrupted | exited | unknown

ConnectivityState:
  live | stale | disconnected

ConfirmationState:
  live_confirmed | restored_unconfirmed
```

语义：

- `blocked`：需要权限、表单或明确用户动作。
- `waiting`：agent TUI 存活，等待下一条用户输入。
- `done`：一个 turn 已正常完成，可触发完成通知；不等于进程退出。
- `exited`：provider/terminal session 已退出。
- `unknown`：证据冲突或无法判断，不能假装完成。

兼容投影：

```text
starting/working                  -> ai-working
blocked/waiting/done/failed       -> ai-idle + cause
interrupted                       -> ai-idle + Interrupt
exited/unknown without live claim -> idle/error presentation
```

## 顺序、重放与 Fence

- remote relay 每次进程启动生成 `relay_instance_id`，事件使用单调 `relay_sequence`。
- 每条事件有稳定 `event_id`，client 维护有界 dedupe cache。
- 事件只在 host/worktree/pane/terminal session/incarnation/agent run 全部匹配时可更新当前行。
- provider session 更新可以是 `identity-only`，不能伪造活动状态转换。
- client 先注册 notification handler，再主动请求 replay/snapshot。
- replay 只恢复 last-known state；标记 `is_replay`，不能重触发完成提示、通知或自动化副作用。
- 新 incarnation 会 tombstone 旧 incarnation；旧事件、旧 kill、旧 resize 一律拒绝。
- 同一 relay instance 内按 sequence 排序；跨 relay instance 只通过 snapshot/fence 重新建立基线。

## Heartbeat 与断线恢复

- host runtime heartbeat 与 agent Hook 是两种信号。Hook 可能长时间安静，不能用 Hook 静默判断设备掉线。
- heartbeat interval/lease 作为可配置运行参数；初版建议 5 秒心跳、15 秒 stale、连接明确关闭时立即 disconnected，最终值以压测调整。
- stale/disconnected 只改变 connectivity，不改变最后 activity。
- 重连后依次请求：runtime identity、terminal inventory、agent last snapshot、spool replay。
- terminal inventory 是 PTY 存活的权威来源；process probe 只验证 agent foreground liveness。
- last status 从磁盘加载后，非终态一律 `restored_unconfirmed`，直到当前 runtime 重新确认。
- 新鲜度使用 client receipt monotonic time；remote wall clock 仅用于展示。

## 持久化与恢复

Client worktree workbench state 持久化：

- stable identities/bindings。
- per-`WorktreeId` terminal tabs、file tabs、active tab、split tree、open files 和 view state。
- last-known status projection。
- last receipt time、confirmation/connectivity state。
- provider session resume metadata。

Execution host 持久化：

- terminal history/checkpoint。
- per-pane last normalized agent status。
- launch token hash、incarnation 和 agent run fence。
- bounded replay spool/high watermark。

状态文件/DB 写入必须原子。交互式 prompt 内容不应在无明确需要时长期持久化；恢复 stale 可回答卡片比不恢复更危险。

## UI 投影

Project sidebar 的 worktree 卡显示：

- 固定 status lane、host/repo identity、display name、branch/path。
- agent 汇总：blocked/needs-you > working > done > waiting > unknown。
- 必要时内联 Agent 行，点击精确定位到 tab/pane。
- terminal restore 状态只在异常/降级时持续显示，正常 reattach 不占用常驻卡片空间。
- disconnected/stale 为覆盖层，不伪装成 activity，也不隐藏 last-known agent state。

Agent/pane 行显示：

- agent type/model。
- activity + reason。
- evidence/confidence（只在诊断面板显示，主 UI 用稳定图标/文案）。
- `Reattached`、`Restored`、`Resumed`、`Disconnected`。

相同路径在不同 host 下必须是不同 row key。非权威 worktree 空列表显示 stale/refresh 状态，不删除已有卡片。

全局 Agents 浮窗只展示跨 worktree 的 live attention/unread；右侧 `Sessions` panel 只显示当前 worktree 路径匹配到的全部历史与 resume。两者不能共用状态源或模糊 selection scope。浮窗是临时 UI state，不替换 `active_worktree_id`，也不拥有 PTY 或文档生命周期。

右侧顶部 tabs 的 scope：

- `Files/Git/Sessions` key 为 `WorktreeId`，切换 worktree 后旧结果必须被 generation/cancellation fence。
- `Tasks` key 为 `ExecutionHostId + ProjectId + GitHubRepoIdentity + auth generation`，同 project worktree 共享 fetch/cache，但 UI selection 可按 `WorktreeId` 保存；详情使用 worktree-scoped tab identity，数据仍复用 project cache。
- `Sessions` 的 cwd/path 匹配使用执行主机 canonical path；本地、WSL、SSH 不能直接比较客户端路径字符串。
- `Usage` footer 只打开现有 usage surface，使用 `mt-usage` 的 tokens/cost/calls 口径，不以 sidebar 自己的百分比建立新账本。

## 兼容与迁移

- 旧布局中的数字 `pty_id` 只用于本次读取映射；加载时生成 `PaneKey` 和 `TerminalSessionId`。
- 旧 session 没有 terminal host history，只能标记 cold-only，并按现有 cwd/AI session resume。
- 现有 `StatusChange` 三态保留为 UI compatibility projection，内部先切换到新状态模型。
- Hook protocol 版本化；旧 `miniterm-hook` 可继续 POST 旧 `/hook`，receiver 将其标记为 legacy/low-confidence，不赋予 remote authority。
- provider Hook 安装更新必须保留用户自定义条目，并可完整卸载 mini-term 自己的 marker。

## 安全边界

- 本地 endpoint 使用当前用户可访问的 Unix socket/named pipe，或 loopback + bearer token。
- SSH host identity 以已验证 host key 和 remote install id 组合；host key 改变需要重新信任。
- connection id、host id、local receipt time 由接收端盖章。
- Hook body、字段长度、缓存数量、spool 和 snapshot 均设上限。
- 不传输完整远程 cmdline、环境或 prompt，除非对应 UI 功能明确需要并经过裁剪。
- launch token 主要防 stale generation 串台，不宣称抵御同一账户下的恶意进程。
- GitHub 命令必须使用结构化 argv；repo 名称、Issue/PR 标题和远端输出都按不可信数据处理，不能进入 shell 拼接或富文本原始 HTML。
- 不读取、复制、显示或持久化 `gh` 原始 token。Local、WSL、SSH 各自使用所在 execution host 的 credential store；Tasks 只展示非敏感登录/修复命令和重新探测结果。
- 诊断可显示 GitHub host、登录账户、scope 名称和 token 来源类型（环境变量/keyring），不得显示 token 值；环境变量覆盖 keyring 时应给出可操作提示。

## 回滚与发布

建议按 feature gate 独立发布：

1. `worktree_catalog_v2`
2. `stable_terminal_identity`
3. `orca_project_sidebar_shell`
4. `terminal_host_warm_reattach`
5. `terminal_cold_restore`
6. `remote_runtime_agent_status`
7. `orca_worktree_context_sidebar`
8. `github_project_tasks`
9. `global_agent_activity_feed`

每层可回退到旧 UI/旧 spawn，但新持久化 schema 必须保持向后可读；关闭 remote agent status 不应影响 SSH terminal/filesystem/git 基础功能。

## 关键决策

- 先 warm reattach，后 cold restore。
- worktree 删除只信 authoritative scan。
- remote device、connection、terminal、provider session 使用不同身份。
- remote agent MVP 只覆盖 mini-term-owned PTY。
- provider Hook/launch attestation 是身份权威；process/title 只是 fallback。
- activity 与 connectivity 分轴。
- freshness 使用本地 receipt time，不直接比较跨设备 wall clock。
- UI 目标借鉴 Orca 的三段式信息架构，但用户侧采用 `Project -> Worktree`，不显示 Workspace 层级或 Status 分组。
- 每个 worktree 独立持有 terminal、open files、tabs 和 split state；project 只共享 repo/GitHub task identity。
- Sidebar 内联 Agent 行与全局 Agents feed 复用同一 live target/routing model；独立 Kanban Dashboard 后续再评估。
- 全局 Agents 使用非模态浮窗，不切换中央主页面；打开/关闭只改变 overlay 与焦点状态。
- Agents 浮窗固定锚定到左侧入口右侧并自动贴边，点外/Esc/关闭按钮/toggle 共用关闭动作；首版不支持拖动和缩放。
- 右侧顶部固定为 `Files / Git / Tasks / Sessions`：Files/Git/Sessions 是 worktree-scoped，Tasks 是 project-scoped。
- 右侧当前 tab 类型全局保持；panel 内 selection/展开/scroll/filter/request generation 按 `WorktreeId` 隔离。
- Files 每个 worktree/tab group 只有一个可替换 preview slot；文件树双击统一重命名，preview 标签双击/Pin/编辑才固定。
- FileTree、Git、历史会话迁到 active-worktree contextual sidebar，中央保持执行面干净；GitHub Tasks 首版只读。
- GitHub Tasks 的列表与详情均通过 project 所属 execution host 的 `gh` 获取并在 mini-term 内展示；未认证时只提示用户在该 host 自行运行 `gh auth login --hostname <host>`，mini-term 不执行命令或打开浏览器，凭据不跨 host。
- 左侧 footer 只保留现有 Usage 与 Settings，不常驻显示 runtime connectivity。
