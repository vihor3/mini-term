# Orca UI/UX 到 mini-term 的映射方案

## 基线

- 研究日期：2026-09-01。
- Orca 本地源码：`/home/leo/orca`。
- 分支：`main`。
- commit：`5aa02ead59a4f34a186c3e8814558b5795260ee9`。
- package version：`1.4.178-rc.2`。
- 当前视觉基线：`research/orca-ui-mockup-v2.html`。旧 v1 已废弃并删除，不再用于对照或实现验收。
- 2026-09-02 最新产品确认：固定保留全局 Agents feed，并采用覆盖当前 workbench 的非模态浮窗。v2 文件尚未绘制该入口，实现时把它视为已确认的产品增量，不继续修改原型文件。
- mini-term 产品取舍：用户侧使用 `Project -> Worktree`，右侧顶部固定 `Files / Git / Tasks / Sessions`，左下角只保留 Usage 与 Settings。
- 本文讨论信息架构和交互语义，不要求逐像素复制 Electron/Tailwind 实现。

## 核心结论

Orca 的主要优势不是配色，而是把 worktree 设为整个产品的一等导航对象：

1. 左侧栏回答“有哪些 project，每个 project 有哪些 worktree、哪些需要我处理”。
2. 中央 workbench 回答“当前 worktree 独立打开了哪些终端和文件”。
3. 右侧上下文栏回答“当前 worktree 的文件、Git、Agent sessions，以及所属 project 的 GitHub tasks 是什么”。
4. Agent 状态同时出现在 worktree 卡、agent 子行和 terminal tab，但三处使用同一状态词汇与优先级。
5. 颜色只表达状态，Agent 身份图标与状态图标分开，避免把“是谁”和“正在做什么”混成一个符号。

mini-term 若只在现有项目行尾追加更多徽章，会继续受到“项目、worktree、终端、Agent 都挤在同一行”的限制。推荐调整整体信息架构，而不是只换视觉皮肤。

## Orca 源码依据

### 左侧 Workspace 导航

- `src/renderer/src/components/sidebar/index.tsx`
  - 220-500px 可调宽度。
  - 固定顶部导航、workspace header、虚拟化 worktree 列表和底部工具栏。
- `SidebarNav.tsx`
  - Search、Tasks/Agents/Automations/Mobile 等全局入口。
- `SidebarHeader.tsx`
  - Projects/Workspaces 标题、显示选项、添加项目、新建 workspace。
- `WorktreeList.tsx`
  - host/repo/status 分组、筛选、排序、虚拟化、selection 与 reveal。
- `WorktreeCardStatusSlot.tsx`
  - 卡片左侧稳定状态列，unread 叠在同一列，不挤动标题。
- `WorktreeCardAgents.tsx`
  - worktree 内联 Agent 行、子 Agent 层级、精确跳转 pane。

### 中央 Workbench

- `src/renderer/src/app-shell/AppWorkspaceShell.tsx`
  - 左 sidebar、中央 workbench、右 contextual sidebar 三段式布局。
- `TerminalTitlebarTabs.tsx`
  - terminal/editor/browser 使用统一 tab strip。
- `TerminalTabLeadingIcon.tsx`
  - 状态 glyph 与 Agent/provider identity 并列显示。
- `WorktreeCreationPanel.tsx`
  - 创建 worktree 后立即关闭 composer，sidebar 出现 pending row；中央用 faux tab 显示后台创建进度，成功后原位切换成真实终端。

### 右侧上下文栏

- `src/renderer/src/components/right-sidebar/index.tsx`
  - panel 与 icon activity bar，可顶置或侧置。
- `use-right-sidebar-activity-items.ts`
  - Explorer、Agents history、Source Control、Checks、Ports 等按 workspace 能力动态显隐。
- `right-sidebar-panel-content.tsx`
  - 只展示当前 workspace 的上下文，不承担全局任务导航。

### 状态词汇

- `AgentStateDot.tsx`
  - working：spinner。
  - waiting/permission：橙色问号。
  - done：绿色完成标识。
  - blocked/interrupted/failed：红色。
  - idle：灰色。
- `StatusIndicator.tsx`
  - worktree 聚合态使用同一词汇，但在高密度 sidebar 中将 quiet active/done 收敛为绿色圆点。

## mini-term 目标壳

```text
┌──────────────────────────┬────────────────────────────────────┬───────────────────────┐
│ Project Sidebar          │ Active Worktree Workbench          │ Context Sidebar       │
│ 280-360px                │                                    │ 300-420px             │
│ Search                   │ unified tabs                       │ Files Git Tasks Sess. │
│ Agents                   │ ┌ terminal ┬ file ┬ file ┐          │ --------------------- │
│                          │ └─────────────────────────┘        │ worktree files        │
│ Projects        [⋯][+ ]  │ worktree-specific tabs/splits      │ changes + commits     │
│ mini-term                │                                    │ GitHub Issues / PRs   │
│   main                   │                                    │ scanned Agent sessions│
│   orca-ui                │                                    │                       │
│   pty-daemon             │                                    │                       │
│ Usage             [gear] │                                    │                       │
└──────────────────────────┴────────────────────────────────────┴───────────────────────┘
```

MVP 不要求立刻具备 Orca 的浏览器和完整编辑器。中央先统一 terminal、现有 file viewer、diff/session preview 的 tab identity 和 tab strip，后续内容类型再扩展。

## 左侧 Sidebar UX

### 1. 全局导航

首版只保留高价值入口：

- Search：打开全局 Quick Open，可直接命中 project、worktree、file、tab 与 live Agent target。
- Agents：打开锚定在入口右侧的跨 worktree live attention/unread 非模态浮窗，点击 item 精确跳转到 worktree + tab + pane。
- Projects：当前默认页，不单独做按钮。
- Settings 固定在左下角；Mobile/SSH 管理等低频入口进入设置或 project menu，不继续保留一条独立的 44px ActivityBar。
- Agents 与右侧 Sessions 不共用状态源：Agents 只回答“哪个 live Agent 需要我”，Sessions 只回答“当前 worktree 有哪些历史记录可查看或 resume”。

现有 `ActivityBar` 的状态灯、Sessions/Git/SSH 等入口不应逐个平移到左侧顶部，否则会把 Orca 的低噪声导航重新变成图标墙。

### 2. Projects header

固定一行：

- 标题固定为 `Projects`，不出现 Workspace 文案。
- project options：sort、host/project filters、隐藏 default branch 等。
- 添加项目。
- 在 project 行或 `+` composer 中新建 worktree。

常用创建必须 1 次点击进入 composer；复杂筛选统一收进一个 options menu，并在按钮上显示 active filter count。

### 3. Project -> Worktree 层级

- 只提供 project/repo -> worktree 层级，不提供 Project/Status 切换。
- 同一 project 的 main worktree 与 linked worktree 并列使用同一 row model。
- project 共享 repo identity、GitHub Issues/PR cache；worktree 独立持有 terminal、open files、tabs、split 和右栏 selection。
- 不同 host 的同路径 worktree 必须使用 host-qualified row key。远端连接状态属于 worktree 覆盖信息，不参与 Agent activity 排序。

### 4. Worktree 卡片

建议固定解剖：

```text
[status lane] [host/repo icon] display name       [hover actions]
              branch/path · primary/sparse
              [agent state] [provider] session title · age
              [child agent disclosure...]
```

- 左侧状态列恒定 20px，避免状态切换时标题左右跳。
- 标题 13px；未读用字重，不新增常驻 `DONE` 药丸。
- `primary`、`sparse`、远端 host 等只在确实有辨识价值时显示。
- branch/path、PR/issue、端口等低频信息默认收进次行或 hover details。
- hover 只出现删除、更多等危险/低频动作；常用“打开”就是点整张卡。
- 卡片展开 Agent 行后，点击 Agent 必须定位到精确 worktree + tab + pane，而不是只切项目。

### 5. Agent 行

Agent 行显示：

- 状态 glyph。
- provider icon/名称。
- provider session title 或 terminal title。
- 相对时间。
- 远端时仅在需要区分设备时显示 host badge。

状态与身份分开显示：spinner 表示 working，Claude/Codex 图标表示 provider。不能用一个彩色 provider 图标同时表达两件事。

### 6. 左侧 Footer

- 移除 `Local runtime connected` 常驻文案；连接异常直接投影到受影响的 remote project/worktree 行。
- Usage 入口复用现有 `mt-usage` 与 UsagePanel，展示 tokens/cost/calls 等既有口径。
- Settings 使用独立齿轮按钮；footer 不再放 Hosts、账户或其他状态项。

## 中央 Workbench UX

### 1. Worktree 作用域

切换 worktree 时只切换 workbench session；其他 worktree 的 terminal host session 在后台继续运行。terminal tabs、file tabs、active tab、split tree、打开文件和 view state 全部按 `WorktreeId` 持久化，不能按 project 共享。

### 2. 统一 tab strip

目标 tab 模型：

- `Terminal`
- `File/Preview`
- 按需打开的 `Diff/History`
- 后续可扩展 `Browser`

tab 前导区固定为：`activity glyph + provider/shell identity`。working/permission/unread/done 使用统一优先级，quiet 状态退回 provider/shell identity。

### 3. 恢复反馈

启动恢复不弹全局阻塞对话框。每个 tab 使用局部状态：

- `Reattaching...`：正在连接同一 PTY。
- `Reattached`：同一进程成功续接；短暂 toast/chip 后消失。
- `Restored from history`：只恢复画面并启动了新 shell。
- `Agent resumed`：新 shell 已续接 provider session。
- `Restart required`：历史可见但没有可自动续接的进程/会话。
- `Recovery unavailable`：校验失败，允许打开干净 shell并查看诊断。

正常 warm reattach 成功应尽量无感；只有降级恢复或失败才持续占用可见空间。

### 4. 关闭语义

- 退出 mini-term：detach，不 kill terminal host session。
- 关闭 terminal tab：kill 对应 PTY；若 Agent 正在 working/needs input，弹明确确认。
- MVP 不提供 Sleep worktree；切换/折叠已能隐藏工作面。后续若加入，必须先明确是 hide、detach 还是批量 stop。
- Delete worktree：先处理 live terminal，再执行 Git 删除；危险动作只在 context menu/确认对话框出现。

## 右侧 Context Sidebar UX

右侧菜单像 Orca 一样固定在面板顶部，顺序为 `Files / Git / Tasks / Sessions`：

- `Files`：active worktree 根目录内的文件树。切换 worktree 后 watcher、selection 和打开动作一起换 scope。
- Orca 中单击文件以 preview 打开，首次追加到统一 tab 条末尾、后续在原位替换；文件名双击重命名，preview 标签双击固定。文件行非文字区域另有双击固定 handler；mini-term 已确认不复制该命中差异，整行双击统一重命名，固定只由 preview 标签双击、显式 Pin 或编辑触发。
- `Git`：active worktree 的 branch、working tree changes、diff 和 commit history/tree；点击行在中央打开该 worktree 的 Diff/History tab。
- `Tasks`：active worktree 所属 project 的 GitHub Issues 与 Pull Requests。数据由 project 所属 execution host 的 `gh` 提供，按 host/project/repo 共享；点击后在中央打开只读详情 tab，普通查看不跳浏览器。
- `Sessions`：扫描 active worktree canonical path 匹配到的 Claude、Codex、Grok 等全部智能体 session，显示 provider、标题、时间、live/stale 和 resume。

Files/Git/Sessions 是 `WorktreeId` scope；Tasks cache 是 `ExecutionHostId + ProjectId + GitHubRepoIdentity + auth generation` scope。当前 tab 类型全局保持，切换 worktree 后仍停留在相同 tab；selection、展开、scroll、filter 和 request generation 按 worktree 隔离。面板可关闭、可调宽并保持 docked；无 remote、`gh` 缺失、未认证、scope 不足、断线和空结果各自显示独立空态。未认证时只显示对应 Local/WSL/SSH host、可复制的 `gh auth login --hostname <host>` 和 Retry；mini-term 不执行命令或打开浏览器。

## 全局 Agents Feed

MVP 保留全局 Agent feed，并与另外三处复用同一 live target：

1. worktree 卡状态列：聚合 working / needs-input / failed，disconnected 作为覆盖态。
2. 卡片内联 Agent 行：provider + 标题 + 相对时间，点击精确跳 pane。
3. Quick Open：按关键词直接命中同一个 live target。
4. 全局 Agents：按 `Needs You / Working / Recent` 聚合跨 worktree 事件与 unread。

点击左侧 Agents 后，在当前 workbench 上方打开固定锚定浮窗：锚在入口右侧并自动贴边，不切换中央页面、不卸载 active worktree，也不强制收起右侧 contextual sidebar。点外、`Esc`、关闭按钮或再次点击入口关闭，首版不支持拖动/缩放。浮窗获得临时键盘焦点；关闭后回到打开前的 pane。点击 item 时关闭浮窗并精确跳转 `WorktreeId + TabId + PaneKey`，成功聚焦后才确认该 run 的 unread；仅打开浮窗不批量清 badge。

历史会话浏览、transcript preview 与 provider resume 仍只属于 active worktree 的右侧 `Sessions`。feed 的权威来源是 Hook/terminal runtime live state，Sessions 的来源是 worktree-scoped 历史扫描，两者不得相互覆盖。第一版不做 Kanban Agent Dashboard。

## 远程设备 UX

远程信息分三层：

1. Host：连接状态 `connected/reconnecting/disconnected`。
2. Terminal：`attached/detached/missing/restoring`。
3. Agent：最后确认的 activity + `live/stale/disconnected` connectivity。

显示规则：

- 断线时保留 worktree 卡与最后 Agent activity，但整体降低对比度并显示 Connect/Reconnect。
- 断线不能把 working 自动改成 done。
- reconnect 后等待 terminal inventory 与 agent snapshot 对账；对账前标记 `Restoring` 或 `Last seen`。
- capability probe 只影响 Agent launcher 是否可选，不在 worktree 卡上宣称某个 Agent 正在运行。

## 创建 Worktree UX

采用 Orca 的后台创建模型：

1. Composer 填 `Project / Run on / Name / Agent / Start from`。
2. 提交后立即关闭 composer。
3. sidebar 在最终位置插入 pending worktree row。
4. 中央显示同名 faux tab 和阶段文案：`Fetching -> Creating -> Running setup -> Starting agent`。
5. 成功后原位替换为真实 worktree + terminal，不跳屏。
6. 失败保留行与错误，提供 Retry/Remove；运行中的页面用 `Run in background`，只有底层支持安全取消时才出现独立 Cancel。

## 当前 mini-term 到目标的映射

| 当前 mini-term | 目标 |
|---|---|
| `ActivityBar` 图标列 | Sidebar 顶部少量文字导航 + 左下 Usage/Settings |
| `ProjectList` 项目行 | project -> worktree 分层卡片；无 Workspace/Status mode |
| worktree 作为 `parentProjectId` 子项目 | `WorktreeId` 一等行；main worktree 与 linked worktree 同模型 |
| `FileTree` 固定在项目列表下方 | 右侧顶部 `Files` tab，active-worktree scoped |
| Sessions/Git 悬浮抽屉 | 右侧顶部 `Sessions/Git` tabs |
| 无 GitHub task panel | 新增右侧 `Tasks` + 中央只读详情 tab，通过 project 所属 execution host 的 `gh` 获取数据 |
| `TerminalsPanel` 右缘面板竖条 | 逐步并入统一 worktree tab strip；保留面板概念仅作高级分组 |
| 四态 `PaneStatus` | activity/connectivity/confirmation 分轴，兼容投影到旧四态 |
| 行尾 DONE 药丸 | 未读字重/小标记 + Agents feed，不占用主行宽度 |
| SSH 项目单一徽章 | host 状态 + terminal attachment + agent activity 分层 |

## 分阶段落地

### UI Slice A：Project Sidebar

- 基于新 worktree catalog 构建 host/repo/worktree row model。
- 新 sidebar 壳、固定 Search/Agents、Projects header、Usage/Settings footer。
- Worktree card、status lane、host/repo identity、virtualization。
- 先复用当前 `PaneStatus` 聚合，后续切换新 Agent 状态协议。

### UI Slice B：Workbench Tabs

- worktree-scoped session 与稳定 tab/pane ID。
- 每个 worktree 独立 open files、tab/split/view state。
- 统一 terminal/file/按需 diff tab strip。
- terminal panel 迁移兼容层。

### UI Slice C：Context Sidebar

- 顶部 `Files / Git / Tasks / Sessions` tabs。
- 将现有 FileTree、SessionPanel、GitPanel/GitHistory 逐个迁移，并新增 execution-host-scoped 只读 GitHub task provider。
- 对 Files/Git/Sessions 实施 worktree generation fence；Tasks 使用 host/project/repo/auth-generation cache，并在中央打开 worktree-scoped 详情 tab。

### UI Slice D：Agent 状态与远端状态

- inline Agent rows、全局 Agents feed、精确 pane 跳转和统一 unread/attention。
- remote connectivity/freshness 与恢复状态。

## 不应复制的部分

- 不复制 Orca 的品牌、logo、营销页面和供应商账户 UI。
- 不因对齐 UI 而引入 mini-term 暂无的 Browser、Automations、GitHub 写操作/Projects 全套能力、Linear/Jira。
- 不逐像素复制 Electron/Tailwind 实现；GPUI 保持自身组件与性能模型。
- 不先做独立 Agent Dashboard；先完成 sidebar + 全局 Agents feed + Quick Open + contextual panel 的核心闭环。

## 原型 v2 基线与最新增量（2026-09-02）

v2 文件：`research/orca-ui-mockup-v2.html`。旧 v1 已废弃并删除；实现以 v2 的布局和交互演示为视觉基线，同时应用下述最新产品增量。

### P0 与文档对齐

1. **侧栏宽度对齐基线**：左侧 280-340px、右侧 300-380px（v1 为 218-252 / 200-242，导致卡片与右栏大面积截断）。
2. **断线态不再用红点**：disconnected 降为覆盖层——整卡降低对比度、状态列放灰色 `unplug` 标记、行内提供 Reconnect；红色只留给 failed/interrupted。与「远程设备 UX」一节和 design 的三轴模型一致。
3. **补回固定全局 Agents 浮窗**：v2 文件当前只画了 Search，这是已知缺口。实现必须增加 Agents 入口、needs-attention badge 和非模态浮窗；浮窗覆盖 workbench，但不能替换 workbench route 或阻塞 terminal 生命周期。

### P1 补演示态

4. **分屏演示**：`main` worktree 的 terminal surface 演示左右两个 pane（各持独立终端与 agent 状态，活动 pane 有强调边框）。tab 条是 workbench 级、split tree 在每个 tab 内容内，与现有 `SavedTab.split_layout`（`mt-config/src/config.rs:375`）同构；tab 前导 glyph 取该 tab 内 pane 的最高优先级。
5. **pending worktree 创建态**：sidebar pending row（spinner + 骨架文案）+ 点击后中央显示阶段进度 `Fetching → Creating → Running setup → Starting agent`。v2 当前写着 Dismiss；实现文案改为 `Run in background`，失败态改为 Retry/Remove。
6. **空态演示**：`pty-daemon` 的 Tasks 展示「No GitHub remote」独立空态。
7. **点击穿透闭环**：右栏 Files/Git/Sessions 行点击驱动中央区（开文件 tab / diff tab / 精确跳转 worktree+tab）。

### P2 降噪

8. **context 条与状态栏合并**为一条 28px 的 workbench 底条：左 path / branch，右 agent 状态 + attachment；删除 `UTF-8` 常驻项与「main / Files」式重复标题。垂直 chrome 从 130px 降到 103px。
9. **agent 行补相对时间**（`· 2m`）；未读态演示为状态列叠加蓝点 + 字重（不引入常驻 DONE 药丸）。
10. **桌面式紧凑模式**替代 Web 断点：窗口低于约 880px 时右栏隐藏（由标题栏按钮控制），侧栏收窄；不提供 430px 手机断点。

### P3 工程

11. **lucide 改用 jsdelivr**（unpkg/cdnjs 均不可用：unpkg 不在 artifact CSP 白名单、cdnjs 未收录 lucide；jsdelivr 在白名单内）；补全 `<!doctype>/<title>` 文档骨架，可作为独立文件分发。
12. 右栏 tab 标签从 8px 提到 9.5px 等效，保证可读性下限。
