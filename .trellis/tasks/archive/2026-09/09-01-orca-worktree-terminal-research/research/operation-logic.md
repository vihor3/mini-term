# mini-term Project / Worktree 操作逻辑草案

## 文档定位

- 日期：2026-09-02。
- `research/orca-ui-mockup-v2.html` 仅作为当前视觉基线；旧原型已经废弃，不再参与实现判断。
- 最新产品增量：左侧固定保留全局 `Agents`。v2 尚未画出这一入口，实现时必须补回，但不要求继续修改原型文件。
- 本文只定义操作、状态归属和失败语义，不修改产品代码。

## 核心原则

1. **切换只改变视图，不改变后台所有权。** 切换 Project、Worktree、右栏 tab 或全局 Agents，不得停止 PTY、丢弃草稿或取消无关任务。
2. **Worktree 是工作台隔离边界。** terminal、打开文件、preview、diff、split、active pane 和右栏浏览位置均不得跨 `WorktreeId` 串位。
3. **Project 只共享仓库级数据。** GitHub remote、Issues/PR 缓存和 worktree catalog 属于 Project；文件、Git working tree 和 Agent sessions 不共享。
4. **所有异步结果回到发起它的对象。** 请求必须捕获 `ProjectId / WorktreeId / TabId / PaneKey / generation`，完成时重新校验；不能使用“此刻 active worktree”。
5. **恢复类型必须诚实。** attach 原进程、重放历史画面、resume provider session 是三个不同动作和提示。
6. **危险动作显式化。** 关闭页面、关闭 terminal、移除项目、删除 worktree、取消创建分别使用不同命令与确认文案。

## 状态归属

| 作用域 | 持有状态 |
|---|---|
| App | active `WorktreeId`、active context tab、Quick Open、Agents 浮窗状态、打开浮窗前的 focus return target |
| Project | repo identity、execution host、worktree catalog、展开状态、GitHub identity/cache |
| Worktree | tab 顺序、active tab、split tree、active pane、打开文件/草稿、右栏 selection/scroll、session index |
| Terminal pane | `PaneKey`、`TerminalSessionId`、expected incarnation、attachment/recovery、Agent binding |
| Context request | source identity、request generation、取消句柄、最后成功数据与错误 |

主工作面与 Agents 浮窗使用相互独立的状态：

```text
PrimarySurface = Workbench(active_worktree_id)
AgentsOverlay = Closed | Open(filter, selection, focus_return_target)
```

打开 `Agents` 不替换中央主内容，也不改变右栏 mode。原 active worktree、tab、split、终端画面和草稿继续存在；关闭浮窗时焦点返回打开前的 terminal/file pane。浮窗本身不拥有任何 terminal 或 document lifecycle。

## 启动与恢复

1. 立即从持久化状态渲染 Project -> Worktree 列表、active worktree 和中央 tab 骨架，不等待 Git、SSH 或 PTY 全部完成；应用启动不自动恢复为打开的 Agents 浮窗。
2. 后台刷新 worktree catalog；非权威失败保留 last-known rows，只在对应 Project/Worktree 上显示 stale/reconnect。
3. 查询 terminal host inventory，active worktree 的可见 terminal 优先 `attach-only`；其他 worktree 只更新状态，不抢焦点、不主动重建 shell。
4. warm reattach 成功尽量无感；cold restore、provider resume 或失败在对应 tab 内持续显示明确结果。
5. 保存的 active target 已消失时，按“同 project main worktree -> 第一个可用 worktree -> 空工作台”降级，并解释原因；不得把旧 tab 绑定到另一个 worktree。

## 左侧 Project Sidebar

### 全局入口

- `Search`：打开 Quick Open，搜索 project、worktree、file、tab 和 live Agent target。
- `Agents`：打开锚定在入口右侧的全局实时 Agent 非模态浮窗，badge 只统计 needs-attention，不统计全部 working/history。
- `Usage`：打开现有用量统计。
- `Settings`：打开设置。

### Project 行

- 点击标题：只展开/收起，不改变 active worktree。
- 点击 `+`：以该 project 为默认值打开 New Worktree composer。
- 更多菜单：Refresh、New Worktree、Project Settings、Remove from mini-term。
- `Remove from mini-term` 只移除登记，不删除磁盘仓库或 worktree。若仍有 live terminal，确认框必须显示数量和“进程将继续 detached”或提供显式停止选项。

### 添加 Project

1. 选择 Local / WSL / SSH host 与仓库路径。
2. 验证路径、Git common-dir 和执行主机身份，再生成/查找稳定 `ProjectId`。
3. 已存在时不新增重复项，直接 reveal 原 project。
4. 新增成功后展开 project、选择 main worktree；若没有保存过 workbench，按需创建第一个 shell terminal。

### Worktree 行

- 点击整行：切到该 `WorktreeId` 的已保存 workbench；若 Agents 浮窗仍开着，列表内容随 live state 更新，但不能把浮窗 selection 误当作 worktree selection。
- 普通切换不会 kill、sleep 或重新创建其他 worktree 的 terminal。
- 点击内联 live Agent：直接激活 `WorktreeId + TabId + PaneKey`；目标成功获得焦点后才确认该条 unread。
- live target 已消失时不自动新建 terminal，转到对应 Sessions 记录或显示 `Session no longer live`。
- pending 行：打开创建进度页。
- disconnected 行：打开 last-known workbench；文件写入、Git mutation 和 resume 禁用，提供明确 Reconnect。

## 中央 Workbench

### Tab 与分屏

- 顶层 tab 类型：Terminal、File、Diff、Session Transcript；每个 tab identity 必须包含 `WorktreeId`。
- tab 内可有 split tree；`active pane` 只属于该 worktree/tab。
- tab 可在同一 worktree 内排序；MVP 不允许跨 worktree 拖动，避免隐式复制 terminal/document identity。
- 切换 tab 只更新 active target，不重启 terminal、不重复读取已缓存文件。

### 新建 Terminal

- tab strip 的 `+` 默认在 active worktree 根目录创建 shell terminal。
- `+` 的下拉菜单选择 Shell / Claude / Codex / Grok 等 launcher；可用项来自 execution host capability，不代表当前已有该 Agent 在运行。
- 创建请求携带 `WorktreeId + cwd + launcher + operation id`。用户切走后成功只写回原 worktree，不抢焦点；失败在原 worktree 产生可重试 tab/通知。

### 关闭语义

- 关闭 File/Diff/Transcript tab：只关闭视图；dirty file 必须 Save / Discard / Cancel。
- 关闭 Terminal tab：结束对应 PTY。若 Agent 为 working/needs-input，必须显示 provider、任务和 worktree，再确认。
- 关闭 mini-term 窗口：GUI detach，terminal host 内 PTY 继续运行。
- MVP 建议不提供含义模糊的 `Sleep worktree`；先用折叠/切换隐藏工作面。若后续增加 Sleep，必须明确它是否只隐藏、detach，还是批量停止进程。

## 右侧 Context Sidebar

顶部固定 `Files / Git / Tasks / Sessions`。右栏可以收起和调宽，但 panel slot 不因无数据而消失。

context tab 类型全局保持，例如用户在 Git 中切换 worktree 后仍查看 Git；具体 selection、展开节点、scroll、filter、last-known data 和 request generation 按 `WorktreeId` 保存。这样适合跨 worktree 对比，同时不会串数据。Tasks 的网络 cache 以 Project 共享，但 UI selection/filter 仍按 worktree 保存。

### Files

- root 永远是 active worktree canonical path，不能退回 project main path。
- Orca 的实际事件链：单击文件以 `preview: true` 打开；首次 preview 追加到统一 tab order 末尾，通常显示在 terminal 标签右侧，后续 preview 在原位置替换。文件名文本双击会 `stopPropagation` 并进入重命名；preview 标签双击转为 permanent。
- Orca 文件行容器还保留一个双击固定 handler，所以双击图标/行空白会固定 preview，而双击文件名会重命名。此前把这个行级 handler 概括成“文件行双击固定”是不准确的。
- mini-term 已确认保留用户可见的主路径并消除命中区域差异：文件树整行双击统一重命名；preview 只通过双击 preview 标签、显式 Pin 或开始编辑转为 permanent。
- preview identity 以 `WorktreeId + TabGroupId` 为 scope。首次 preview 追加到该 group 的统一 tab order 末尾，通常位于 terminal 标签右侧；后续 preview 在原位置替换，不能驱逐另一 worktree 或另一 split group 的 preview。
- 打开请求捕获 `WorktreeId + canonical path + document generation`。切换 worktree 后迟到结果只能进入原 worktree 缓存，不能替换当前 tab。
- 切换 worktree 时 watcher 先解绑旧 scope，再绑定新 scope；旧 watcher 事件按 generation 丢弃。

### Git

- 默认显示 branch、working tree changes 与 commit history/tree。
- 点击 changed file：在中央打开该 worktree 的 Diff tab；重复点击复用同一 diff identity。
- 点击 commit：显示 commit detail；再点文件打开 `commit vs parent` Diff tab。
- Commit / Pull / Push 等 mutation 保留为明确按钮或菜单，默认面板以查看为主。
- 所有 scan/diff/mutation 捕获 `WorktreeId + repo generation`；切换页面不改变命令目标。

### Tasks

- 数据作用域是 `ExecutionHostId + ProjectId + GitHubRepoIdentity + auth generation`；同一 project 的 worktree 共用请求、rate-limit 和 last-known cache。
- 每个 worktree 可以独立保存 Issues/PR filter 与 selection，但不得重复拉取同一仓库数据。
- Local project 使用本机对应平台的 `gh`/`gh.exe`，WSL/SSH project 使用对应 execution host 中的 `gh`。Git remote 解析、认证探测、列表和详情命令都在同一 host 执行，不能把本机账户或 token 借给远端。
- MVP 列表和详情只读；点击 Issue/PR 在中央 workbench 打开 worktree-scoped `WorkItemDetail` preview/permanent tab，普通查看不打开浏览器。评论写入、merge 和 Projects board 延后。
- 首次进入或手动刷新时依次探测 `gh --version`、`gh auth status --hostname <host>`，再使用结构化 `gh ... --json`/`gh api` argv 获取数据。命令输出必须带 request generation，切换 host/project/account 后迟到结果不得覆盖当前视图。
- 无 GitHub remote、`gh` 未安装、未认证、账户/host 不匹配、scope 不足、rate limit、网络失败和远端断线分别显示不同状态，不影响其他三个 tab。
- 目标 execution host 未认证时，Tasks 显示目标环境、`gh auth login --hostname <host>`、Copy 和 Retry。mini-term 不运行该命令、不创建或聚焦 terminal tab、也不打开浏览器；用户自行登录后点击 Retry，成功探测 ready 才递增 auth generation 并重新加载。
- 登录账户或环境变量 token 变化时失效旧 cache。诊断可以显示 host、账户、scope 和凭据来源类型，但不能显示、保存或跨 host 传输 token。

### Sessions

- 扫描 active worktree canonical path 可归属的全部受支持 provider 历史，结果不得混入同 project 的其他 worktree。
- live row：点击精确跳到已绑定 `TabId + PaneKey`。
- historical row：点击打开只读 Transcript tab；`Resume in new terminal` 是独立按钮，不能因浏览记录自动执行 provider resume。
- Resume 创建新的 `TerminalSessionId / incarnation`，但保留 provider session lineage；用户切走后不抢焦点。
- 远端断线时保留 last-known index，resume 禁用并引导 Reconnect。
- 文件扫描只能证明“有历史记录”，不能覆盖 Hook/runtime 提供的 live Agent status。

## 全局 Agents

### 职责

- 只聚合 mini-term 所拥有 terminal 中的 live/近期 Agent run、needs-attention 和 unread。
- 不扫描完整历史 transcript，不承担 resume；这些属于右侧 Sessions。
- 推荐分为 `Needs You / Working / Recent`，先按 attention 排序，再按本地 receipt time 排序。

### 已确认交互

1. 点击左侧 `Agents`，在当前 workbench 上方打开非模态浮窗；再次点击同一入口或点关闭按钮关闭。
2. 打开前记录当前 `PaneKey` 作为 focus return target。浮窗打开期间 terminal 继续渲染和接收输出，但键盘输入进入浮窗的筛选/列表。
3. 关闭浮窗后，若原 pane 仍存在则恢复其焦点；不存在则回到 active worktree 的当前 pane。
4. 点击 Agent item 后关闭浮窗，路由到 `WorktreeId + TabId + PaneKey`；确认目标真实存在并获得焦点后，只清该 run 的 unread。
5. 若 item 已变 stale，保留最后状态并提供 `Open session history`，不能假装仍可跳 live pane。
6. 仅打开浮窗不批量清 unread；badge 只随逐条处理或权威状态变化减少。

浮窗不创建新的 workbench route，也不持久化为应用重启后的打开状态。它固定锚定在左侧 Agents 入口右侧并自动贴边；点外、`Esc`、关闭按钮与再次点击入口共用同一个 close action。首版不允许自由拖动/缩放，也不持久化 geometry。

现有代码中的 `overlay.rs`、terminal marker popover 与 `DatePicker` 已实现 overlay stack、`anchored().snap_to_window_with_margin(...)`、点外/Esc 关闭和 previous-focus restore。Agents 复用该模式，目标宽度约 480px，并在窄窗口内 clamp 到可用 workbench 宽度；最大高度受 viewport 限制，内容区内部滚动。

## Quick Open

- 结果类型：Project、Worktree、File、Open Tab、Live Agent。
- 每条结果携带完整 target，而不是先改 active project 再异步猜 pane。
- Live Agent 与左侧内联 Agent、全局 Agents 复用同一个 `activate_agent_target` action。
- stale target 降级为 Sessions/history，不自动 resume。

## 创建 Worktree

1. 从 project `+` 打开 composer：Name、Start from、Run on、可选 Agent launcher。
2. Submit 后 composer 立即关闭，创建 `PendingWorktreeId`；sidebar 在最终 project 分组中插入 pending row。
3. 若用户仍停留在该 row，中央原位显示 `Fetching -> Creating -> Setup -> Starting agent`。
4. 用户离开后任务继续；成功只把 pending row 原位替换成真实 worktree，并显示轻量完成标记，不抢焦点。
5. 失败保留错误详情、Retry、Remove。运行中的页面使用 `Run in background`；只有底层支持安全取消时才显示独立 `Cancel creation`，不能把 Dismiss 当取消。
6. Retry 复用原意图但生成新 operation generation；旧完成回调不能覆盖 retry 结果。

## 远程重连

- disconnected 是 connectivity overlay，不改写 Agent 的最后 activity 为 done/failed。
- 点击 Reconnect 只针对对应 `ExecutionHostId`，显示 connecting/handshake/reconciling 三阶段。
- 重连后先校验 host identity 和 connection epoch，再对账 worktree catalog、terminal inventory、Agent snapshot。
- 对账完成前 terminal/Agent 显示 `Restoring / Last seen`，旧 relay event 不能越过新 epoch。
- 连接失败保留 last-known workbench；用户仍可阅读已缓存 transcript/diff，但所有会产生远端写入的动作保持禁用。

## 删除与移除

- main worktree 不提供 Git delete；只能 Remove Project from mini-term，磁盘数据不变。
- linked worktree 的 Delete 先检查 dirty files、Git changes、未推送提交和 live terminals。
- 确认后先停止/解绑该 worktree 的 terminal，再执行 `git worktree remove`，最后删除 UI metadata；任何一步失败都保留可诊断状态，不能先从 sidebar 消失。
- `Force delete` 不放在一级菜单，且不能绕过 host/worktree identity 校验。

## 异步所有权检查表

| 操作 | 必须捕获的身份 |
|---|---|
| file read/write/watch | `WorktreeId + DocumentKey + generation` |
| Git scan/diff/mutation | `WorktreeId + RepoId + generation` |
| GitHub Tasks | `ExecutionHostId + ProjectId + GitHubRepoIdentity + auth/fetch generation` |
| session scan/resume | `WorktreeId + canonical root + provider session id` |
| terminal create/attach | `PaneKey + TerminalSessionId + expected incarnation` |
| worktree create | `ProjectId + PendingWorktreeId + operation generation` |
| remote reconnect | `ExecutionHostId + connection epoch` |
| Agent event/jump | `AgentRunId + WorktreeId + PaneKey + terminal incarnation` |

## MVP 建议与已确认决策

MVP 纳入：Project -> Worktree、独立 workbench、固定全局 Agents、Files/Git/Tasks/Sessions、后台创建、精确 Agent 跳转、明确关闭/恢复、远程 reconnect。

延后：Kanban Agent Dashboard、GitHub 写操作、跨 worktree tab 拖动、含义未定的 Sleep、任意系统级进程扫描。

已确认：GitHub 未认证时只提示用户在正确 execution host 自行运行 `gh auth login --hostname <host>`；Tasks 提供 Copy 和 Retry，不托管授权、不创建终端、不主动打开浏览器。
