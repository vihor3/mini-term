# 研究 Orca 的 Worktree、终端会话与远程智能体架构

## Goal

基于 Orca 源码和 mini-term 现状，形成一份可直接指导后续实现的架构方案，覆盖：

1. Git worktree 的可靠发现、解析、身份建模与列表展示。
2. 每个 worktree 的终端隔离，以及关闭界面后再次打开时的精确恢复。
3. 远程设备上智能体类型、会话身份和运行状态的稳定识别、跟踪与断线恢复。
4. 将 mini-term 的信息架构和核心交互向 Orca 靠近，让 worktree 成为左侧导航的一等对象，并统一中央 workbench、右侧上下文栏和 Agent 状态入口。

用户价值是避免 worktree 串位、终端串会话和远程智能体状态串台，让用户能从一个低噪声界面快速判断“在哪个 worktree、哪个 Agent 需要我、哪个终端能继续”，并明确“同一进程继续运行”和“仅恢复历史画面/AI 对话”的产品边界。

## Background

- 研究基于 2026-09-01 克隆到 `/home/leo/orca` 的 Orca `main` 源码，commit 为 `5aa02ead59a4f34a186c3e8814558b5795260ee9`，包版本为 `1.4.178-rc.2`。
- mini-term 已有本地/WSL/SSH 终端、Claude/Codex/Grok Hook、AI 会话记录扫描和 SSH sidecar，但这些能力目前没有统一的稳定身份与远程状态协议。
- mini-term 当前重启恢复的是布局、cwd 和可续接的 AI session；GUI 退出时 PTY 会被销毁，不能把现状描述为“终端进程精确恢复”。
- 当前远程项目通过本地 `ssh` 进程打开 shell；本地 Hook 端点和 `MINITERM_PTY_ID` 不会自然变成远程设备上的可信身份通道。
- mini-term 当前使用 `ActivityBar + ProjectList/FileTree + TerminalArea + 右抽屉/TerminalsPanel`；目标壳借鉴 Orca，但用户侧信息模型明确为 `Project -> Worktree`，不引入额外的 Workspace 层级或命名。

## Requirements

### R1. Worktree 发现与解析

- 说明 Orca 如何使用 `git worktree list --porcelain -z` 解析 worktree，并兼容旧 Git。
- 覆盖主 worktree、HEAD、branch、bare、sparse、locked reason、prunable reason 和包含换行的路径。
- 定义 authoritative 与 fallback 结果的区别：只有权威扫描能证明 worktree 已消失并触发清理。
- 给出适合 mini-term 的稳定 `WorktreeId`，身份必须包含执行主机，不能只依赖路径或显示名。

### R2. Worktree 列表展示

- 把 Git 事实、用户元数据和运行态分层，不让显示名、排序或临时连接状态污染 Git 身份。
- 定义主机、仓库、worktree、智能体状态和终端状态的行模型及分组规则。
- 非权威空结果不得清空上次已知列表；同一路径在不同主机上不得发生 UI key 冲突。
- 左侧采用 Orca 式 project sidebar：固定全局导航、`Projects` header、按 project 分组的虚拟化 worktree 卡片和内联 Agent 行，不提供 Workspace 或 Status 分组模式。
- main worktree 与 linked worktree 使用同一行模型；project/repo 负责分组和共享 GitHub task scope，不再把 worktree 降格为普通项目的特殊子项。

### R3. 每个 Worktree 的独立终端

- 每个 pane 使用持久 `PaneKey`，每个终端使用持久 `TerminalSessionId`，每次新进程使用新的 `TerminalIncarnationId`。
- 每个 `WorktreeId` 独立持有 terminal tabs、file tabs、active tab、split layout 和最近打开文件；切换同一 project 下的 worktree 不得复用或覆盖另一 worktree 的 workbench state。
- PTY 所有权从 GUI 进程移到独立 terminal host；关闭 GUI 只 detach，不 kill PTY。
- 终端输入、输出、resize、clear 和 checkpoint 必须按同一顺序流处理。

### R4. 终端恢复语义

- 明确区分 warm reattach、cold visual restore 和 provider resume。
- warm reattach 必须连接到同一个仍存活的 PTY/子进程，并恢复当前权威终端快照后继续接收实时输出。
- cold restore 只允许启动新 shell、重放历史终端画面，并在支持时续接 AI provider session；不得宣称复活任意已死亡 OS 进程。
- UI 必须能区分 `Reattached`、`Restored`、`Resumed` 和 `Recovery unavailable`。

### R5. 远程设备智能体识别

- 将“远程安装了哪些智能体”与“某个 pane 当前实际运行哪个智能体”分开建模。
- capability probe 只能报告可用 CLI；运行中身份必须优先来自 mini-term 启动凭据或 provider 专用 Hook。
- MVP 只保证跟踪 mini-term 所拥有的远程 PTY 中的智能体；任意系统级、非 mini-term 进程扫描不在本任务实现范围。
- agent type 必须可扩展，未知 provider 保留原始标识，不以固定 enum 拒绝新智能体。

### R6. 远程状态跟踪

- 定义 provider 事件到统一状态机的映射，至少覆盖 `working`、`blocked`、`waiting`、`done`、`failed`、`exited` 和 `unknown`。
- 状态记录必须包含 host、worktree、pane、terminal session/incarnation、agent run、provider session、证据来源和本地接收时间。
- 远端 relay 必须使用有界 spool/last snapshot、幂等 event id、单调序号和重连后 pull replay。
- 旧 launch token、旧 terminal incarnation 或旧 relay generation 的事件必须被 fence，不能覆盖新会话。
- 心跳超时只改变 connectivity/freshness，不得自行把智能体判定为 `done`。
- 远端时钟只用于展示；新鲜度以接收端本地时间或主机给出的权威 verdict 计算，避免时钟偏差。

### R7. Orca 对齐 UI/UX

- 左侧统一为 project sidebar：Search、固定的全局 Agents、`Projects` header、project 分组与 worktree 卡片；左下角只保留现有用量统计入口和设置。
- 中央 workbench 以 active `WorktreeId` 为作用域，使用统一 tab strip 承载 terminal 和打开文件，并保留 split 布局；同一 project 的不同 worktree 拥有完全独立的 tab/file state。
- 右侧 contextual sidebar 使用 Orca 式顶部 tab bar，固定为 `Files / Git / Tasks / Sessions`，不再使用右缘竖向菜单。
- 右侧当前 tab 类型属于应用级状态，切换 worktree 时全局保持；每个 panel 的 selection、展开节点、scroll、filter、last-known data 和 request generation 按 `WorktreeId` 独立保存。
- `Files` 只展示 active worktree 根目录内的文件树。
- `Files` 单击或键盘激活文件时，在统一 tab 条末尾（通常为 terminal 标签右侧）创建/复用一个 worktree-scoped preview；后续文件在该位置替换。文件树任意位置双击统一进入重命名，不复制 Orca 文件名与行空白的命中差异；preview 仅在双击 preview 标签、显式 Pin 或开始编辑时转为永久 tab。
- `Git` 展示 active worktree 的当前差异、branch 状态和提交记录树。
- `Tasks` 以 project 的 GitHub remote 为作用域，只读展示 Issues 与 Pull Requests；同一 project 下的 worktree 共享该数据源。列表与详情都在 mini-term 内展示，普通查看流程不得跳转浏览器。
- GitHub 数据必须由 project 所属 execution host 上的 GitHub CLI 提供：本地 project 使用本机对应平台的 `gh`/`gh.exe`，WSL/SSH project 使用对应环境中的 `gh`。不得把本机账户或 token 偷渡给远端，也不得在客户端直接代远端请求 GitHub。
- Tasks 必须区分 `gh` 未安装、目标 GitHub host 未登录、账户/host 不匹配、scope 不足、rate limit、网络失败和远端断线。未认证时只显示目标 execution host、准确的 `gh auth login --hostname <host>` 命令、Copy 和 Retry；mini-term 不执行登录命令、不创建 terminal tab、也不主动打开浏览器。用户自行在对应 Local/WSL/SSH 环境登录，凭据由该环境的 `gh` 持有。
- `Sessions` 扫描 active worktree 目录匹配到的全部受支持智能体会话记录，统一展示 provider、标题、更新时间、live/stale 状态和 resume 入口；历史文件不能被当作 live status 权威来源。
- Agent 状态 glyph 必须统一：working spinner、needs-input 问号、done/quiet 绿色、失败/中断红色、idle 灰色；provider identity 与 activity glyph 分开显示。
- Worktree 创建提交后在后台执行，sidebar 出现 pending row，中央以同名 faux tab 展示阶段进度；成功后原位切换为真实终端，失败保留 Retry/Remove。隐藏进度与取消创建必须是两个不同动作。
- 全局 Agents feed 保留并进入 MVP（2026-09-02 产品确认），采用固定锚定在左侧 Agents 入口右侧、自动贴边的非模态浮窗，不切换中央页面、不卸载 active worktree；点外、`Esc`、关闭按钮或再次点击入口关闭，首版不支持拖动/缩放。它只负责跨 worktree 的 live attention、unread 与精确 pane 跳转。历史会话扫描、正文预览与 resume 只属于 active worktree 的右侧 `Sessions` tab。
- 退出应用、关闭 terminal tab、移除 project、delete worktree 必须使用不同文案和行为，不让 detach、kill、unregister、delete 混为一谈；含义未定的 sleep worktree 延后。

### R8. mini-term 落地方案

- 指出可复用模块、缺失能力、建议新增 crate/sidecar 和跨层边界。
- 给出按依赖排序、可独立验收的实施阶段和验证命令。
- 研究任务本身只产出文档，不修改产品代码。

## Acceptance Criteria

- [ ] 研究文档能从 Orca 源码解释 worktree 解析、缓存、元数据合并和列表投影。
- [ ] 研究文档能解释 Orca terminal daemon、会话身份、终端快照、增量日志、warm/cold restore 的完整数据流。
- [ ] 研究文档能解释 Orca 远端 capability probe、managed Hook、relay、connection stamping、replay 和状态持久化机制。
- [ ] 设计文档定义不依赖路径显示名或运行时数字 PTY ID 的稳定身份模型。
- [ ] 设计文档定义多证据优先级、状态机、心跳、租约、断线、重放、去重、fence 和时钟偏差处理。
- [ ] 设计文档明确 warm reattach 与 cold restore 的可验证边界。
- [ ] UI/UX 研究文档能从 Orca 当前源码解释 sidebar、worktree card、内联 Agent 行、统一 tab strip、后台创建和 contextual right sidebar，并记录 mini-term 的 project-first 调整。
- [ ] 同一 project 的不同 worktree 在原型和设计中拥有不同 terminal tabs、打开文件、文件树、Git 数据和 Agent session 列表。
- [ ] 右侧顶部 `Files / Git / Tasks / Sessions` 的作用域、数据来源、空态和刷新边界均有定义。
- [ ] 本地、WSL、SSH project 的 Tasks 均通过该 project 所属 execution host 上的 `gh` 获取列表和详情；Issue/PR 正常查看不打开浏览器，未认证时只提示用户在正确 host 自行运行 `gh auth login --hostname <host>`，mini-term 不代为执行或打开授权页。
- [ ] GitHub auth、账户和 scope 状态按 `ExecutionHostId + GitHub host` 隔离；布局、session、缓存和诊断信息不持久化或转发原始 token，远端凭据不会被本地账户覆盖。
- [ ] 在 `Files / Git / Tasks / Sessions` 任一 tab 中切换 worktree 后，tab 类型保持不变，但 selection、展开、scroll、filter 和异步结果切换到目标 `WorktreeId`，无跨 worktree 残留。
- [ ] Files 单击连续浏览只复用一个 preview 且保持其 tab 位置；文件树任意位置双击都进入重命名，preview 标签双击、显式 Pin 或编辑才转为永久 tab，两个 worktree 的 preview 不互相替换。
- [ ] 设计文档给出 mini-term 当前 `ActivityBar/ProjectList/FileTree/TerminalArea/抽屉` 到 Orca 式目标壳的逐组件迁移方案。
- [ ] 全局 Agents 以非模态浮窗打开；打开/关闭不改变 active worktree、tab、split、草稿或 PTY，点击 live item 可精确聚焦目标 pane。
- [ ] 实施计划将 worktree、Orca 式 sidebar/workbench、terminal host、snapshot/cold restore、remote relay、agent adapters/context sidebar 拆成有依赖说明的阶段。
- [ ] mini-term 当前能力与目标架构之间的差距有具体文件依据。

## Out Of Scope

- 在本研究任务中直接修改 mini-term 产品代码。
- 逐像素复制 Orca 的品牌、Electron/Tailwind 实现、移动端、自动化、GitHub 写操作/项目管理全套能力、Linear/Jira 或供应商账户系统。
- MVP 不实现独立 Kanban Agent Dashboard；先交付全局 Agents feed、sidebar 内联 Agent 行与 worktree-scoped Sessions。
- 在设备重启、内核杀进程或 PTY host 数据损坏后复活任意原始进程。
- 把窗口标题、shell prompt、输出关键词或单次 `ps` 结果作为高置信度智能体身份。
- MVP 跟踪 mini-term 之外启动的任意系统级智能体进程。

## Technical Notes

- 推荐先交付 warm reattach，再增加 cold visual restore；两者风险和验收口径不同。
- UI 以 Orca 的信息架构为目标，但通过兼容投影分阶段迁移；先重组壳与 worktree 卡片，再替换底层状态与 terminal ownership，避免一次性重写全部面板。
- Sidebar 只使用 Project -> Worktree 分组；attention 通过 worktree 状态列和固定全局 Agents 入口表达，不再引入 Status 分组。
- Agents 浮窗是 workbench 上方的临时交互层，不是新的 `PrimarySurface`；关闭后焦点返回打开前的 pane，点击目标成功后焦点转交目标 pane。
- 左下角 Usage 直接复用现有 `mt-usage` 账本与 `UsagePanel`，不新增第二套统计口径。
- GitHub Tasks 首版只读，数据与详情由 execution-host-scoped `gh` 提供；未登录时展示 `gh auth login --hostname <host>`、Copy 与 Retry，由用户自行完成登录。网络、认证、scope、CLI 缺失或 remote 解析失败时保留 Files/Git/Sessions 能力并显示独立空态。
- 推荐复用现有 SSH daemon 的 IPC、端点所有权和 stale socket 处理模式，但 terminal host 与 SSH 工具 daemon 应保持独立进程和协议。
- 远端智能体识别采用“能力探测 + 启动证明 + provider Hook + 低置信度兜底”的分层模型。
- 远端连接断开时保留 last-known 状态并标记 `disconnected/stale`；只有 provider 终态事件或执行主机的 PTY/进程权威结果才能产生终态。
