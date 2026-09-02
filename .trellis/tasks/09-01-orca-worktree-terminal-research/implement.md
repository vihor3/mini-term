# 实施计划

## 当前任务交付边界

当前 Trellis 任务只交付研究、技术设计和后续实施拆分，不修改产品代码。正式实现建议建立一个父任务，并按下列阶段创建可独立验收的 child tasks；依赖关系写在每个 child 的 PRD/implement 中。

## 推荐任务树

```text
parent: orca-project-worktree-runtime
  1. worktree-catalog-v2
  2. stable-worktree-workbench-identity
  3. terminal-host-warm-reattach
  4. terminal-snapshot-cold-restore
  5. remote-runtime-foundation
  6. remote-agent-identity-status
  7. orca-project-sidebar-workbench
  8. worktree-context-sidebar
  9. github-project-tasks
  10. global-agent-activity-feed
```

`orca-project-sidebar-workbench` 依赖 Phase 1/2，可与 terminal host/remote runtime 并行；`worktree-context-sidebar` 可先迁移 Files/Git/Sessions，live Agent state 再接 Phase 6；`github-project-tasks` 的 UI 可独立开发，但完整 Local/WSL/SSH 交付需要 Phase 5 的 execution-host command boundary，以保证 `gh` 在项目所属 host 执行；`global-agent-activity-feed` 复用 Phase 6 的 live state 和 Phase 8 的精确 target routing。这样首个可见 UI 结果不必等所有底层阶段结束，也不会把历史 Sessions 与实时 Agents 混成一个数据源。

## Phase 1: Worktree Catalog V2

### 目标

用 Git porcelain 建立可测试、带权威语义、支持 host identity 的 worktree catalog。

### 主要改动

- 在 `mt-project` 增加 NUL/text porcelain parser 和 command runner。
- 扩展 worktree fact 类型：HEAD、branch、bare、sparse、locked/prunable reason。
- 增加 strict/lenient、authoritative/source、scan generation 和 single-flight。
- 在执行主机上 canonicalize common-dir/worktree path，并生成 `RepoId/WorktreeId`。
- `mt-app` 改从 catalog row model 读取，但暂时保留现有管理弹窗和 mutation flow。

### 验收

- NUL fixture 覆盖换行、空格、非 UTF-8/损坏记录的 fail-closed 行为。
- 旧 Git fallback fixture 与 NUL 结果等价。
- 非权威空结果不会清除已有 worktree。
- 不同 host 相同路径生成不同 ID/row key。
- mutation 后旧 generation 结果不能覆盖新列表。

### 验证命令

```bash
cargo test -p mt-project
cargo test -p mt-app git_worktree
cargo fmt --all -- --check
cargo clippy -p mt-project -p mt-app --all-targets -- -D warnings
```

### 高风险文件

- `crates/mt-project/src/git.rs`
- `crates/mt-app/src/git_worktree.rs`
- project/worktree persistence schema

### 回滚点

保留旧 libgit2 listing behind feature gate；新 catalog 不权威或解析失败时只降级展示，不执行清理。

## Phase 2: Stable Worktree Workbench/Terminal Identity

### 依赖

Worktree identity contract 已确定；不要求 terminal host 已完成。

### 主要改动

- 在 `mt-layout` 增加 `ExecutionHostId/PaneKey/TerminalSessionId/TerminalIncarnationId`。
- 旧数字 pane/PTY ID 加载时迁移到稳定 UUID。
- layout leaf 与 terminal binding 分离；runtime `u32 pty_id` 只保留为本次进程句柄。
- schema salvage、版本升级和原子写回。

### 验收

- 重启前后 pane/session ID 不变。
- split/move/close/reopen 不误绑定其他 pane。
- 损坏单个 pane 记录不丢整份 workspace。
- 旧配置可读并只迁移一次。

### 验证命令

```bash
cargo test -p mt-layout
cargo test -p mt-app persist
cargo fmt --all -- --check
cargo clippy -p mt-layout -p mt-app --all-targets -- -D warnings
```

### 高风险文件

- `crates/mt-app/src/persist.rs`
- `crates/mt-app/src/tree.rs`
- `crates/mt-app/src/store/panes.rs`
- `crates/mt-layout/`

### 回滚点

迁移前保留旧字段读取；新字段写入失败时继续当前 cold-only 恢复，不创建半绑定 terminal row。

## Phase 3: Detached Terminal Host 与 Warm Reattach

### 依赖

Phase 2 的 stable terminal identity。

### 主要改动

- 新增 `mt-terminal-host` crate/binary 和版本化 RPC。
- 复用现有 sidecar daemon 的 endpoint ownership、权限和 detached spawn 模式。
- 将 PTY spawn/write/resize/kill/list/create-or-attach 移出 `TerminalPane`。
- GUI 退出只 detach；显式 terminal close 才 kill。
- host 生成/校验 incarnation，输出带 sequence。
- `mt-app` 启动时先 attach-only；失败才进入 fresh/cold policy。

### 验收

- 关闭并重开 GUI 后，同一 shell PID/前台程序仍存在。
- 后台命令在 GUI 关闭期间持续输出，重开后无重复、无缺口。
- attach-only 不存在时不静默创建新 shell。
- 旧 incarnation 的 write/resize/kill 被拒绝。
- host 崩溃只影响终端运行时，不损坏 layout DB。

### 验证命令

```bash
cargo test -p mt-pty
cargo test -p mt-terminal-host
cargo test -p mt-app terminal
cargo fmt --all -- --check
cargo clippy -p mt-pty -p mt-terminal-host -p mt-app --all-targets -- -D warnings
```

补充真实进程集成测试：spawn shell -> 输出 marker -> detach GUI client -> 输出第二 marker -> reattach -> 校验 PID 与两段输出。

### 高风险文件

- `crates/mt-pty/src/lib.rs`
- `crates/mt-app/src/pane.rs`
- `crates/mt-app/src/store/panes.rs`
- sidecar packaging/release files

### 回滚点

保留 `terminal_host_warm_reattach` gate；协议握手失败时回退旧进程内 PTY，但 UI 必须显示为 legacy/cold-only。

## Phase 4: Terminal Snapshot 与 Cold Restore

### 依赖

Phase 3 的 terminal host 和顺序输出协议。

### 主要改动

- 为 alacritty model 设计 `TerminalSnapshot` 和 replayable ANSI codec。
- 保存 modes、cursor、主/备用屏、cwd/title/size、partial escape tail。
- 实现 checkpoint + framed output log + checksum/generation/sequence。
- renderer 按源尺寸重放，结构恢复后再 fit。
- 保存 provider resume metadata；cold restore 生成新 incarnation。

### 验收

- 宽字符、组合字符、emoji、wrap-pending、alternate screen、resize、clear、OSC title/link 正确恢复。
- 拆分 escape sequence 在恢复后不损坏屏幕。
- torn final frame 可恢复到完整前缀；中间 sequence gap fail closed。
- cold restore 明确显示新 shell，不伪装同进程。
- 支持的 Claude/Codex/Grok session 可在正确 cwd 续接。

### 验证命令

```bash
cargo test -p mt-terminal
cargo test -p mt-terminal-host
cargo test -p mt-app persist
cargo fuzz run terminal_snapshot_roundtrip
cargo fmt --all -- --check
cargo clippy -p mt-terminal -p mt-terminal-host -p mt-app --all-targets -- -D warnings
```

若仓库未配置 cargo-fuzz，则在 child task 中先建立固定 seed 的 property/golden test，fuzz 作为后续检查项。

### 高风险文件

- `crates/mt-terminal/`
- terminal host history writer/reader
- `crates/mt-app/src/pane.rs`

### 回滚点

history codec 版本不匹配或校验失败时显示 `Recovery unavailable`，启动干净 shell；绝不重放部分不可信中间状态。

## Phase 5: Remote Runtime Foundation

### 依赖

Phase 2 identity contract。可与 Phase 4 的 snapshot codec 部分并行，但远端 terminal persistence 最终依赖 Phase 3/4 协议。

### 主要改动

- 定义 remote `HostInstallId`，与 SSH host key fingerprint 组合成 `ExecutionHostId`。
- 部署/启动版本化 mini-term remote runtime。
- 建立 authenticated mux、heartbeat、connection epoch、reconnect 和 cancellation。
- 远端 runtime 提供 worktree catalog、terminal inventory 和 capability probe。
- 远端 PTY 由 remote runtime 持有，不把本地 `ssh` 进程当长期权威。

### 验收

- 同一远端设备重连后 HostId 稳定；host key 改变触发重新信任。
- connection id 重建不改变 worktree/session identity。
- 网络断开只标 disconnected，不把 terminal/agent 判 done。
- 重连后 inventory 能区分原 session 存活、session 丢失和 incarnation 改变。
- 远端 runtime 旧版本与新客户端握手失败时安全降级。

### 验证命令

```bash
cargo test -p mt-ssh
cargo test -p mt-terminal-host
cargo test -p mt-app remote_ssh
cargo fmt --all -- --check
cargo clippy -p mt-ssh -p mt-terminal-host -p mt-app --all-targets -- -D warnings
```

增加 fake SSH/mux 集成测试：断线、重连、重复 notification、旧 connection epoch、host key mismatch。

### 高风险文件

- `crates/mt-ssh/`
- `crates/mt-app/src/remote_ssh/`
- remote runtime deployment/packaging
- SSH credential/host-key handling

### 回滚点

`remote_runtime_v1` gate 关闭时保留现有直接 SSH terminal/filesystem 路径；agent status 和 warm reattach 降级，但 SSH 基础功能保持可用。

## Phase 6: Remote Agent Identity 与 Status

### 依赖

Phase 2 stable identity + Phase 5 authenticated remote runtime。若要精确绑定长期 PTY，还依赖 Phase 3 terminal host contract。

### 主要改动

- 在 `mt-ai` 定义 provider adapter、Hook envelope 和统一状态机。
- remote runtime 提供 provider-specific Hook endpoint、token、body limits 和 spool。
- capability probe 只给正向检测且启用的 provider 安装 Hook。
- spawn env 注入 host/worktree/pane/terminal/incarnation/agent-run identity。
- 实现 relay instance/sequence/event-id、last snapshot、pull replay 和 ack/high watermark。
- 客户端在 trust boundary 盖 connection/host/receipt time，并执行 evidence ranking/fence。
- 现有 Hook 三态通过 compatibility projection 输出给旧 UI。

### 验收

- 同一远端 worktree 多 pane 同时运行不同 agent 不串台。
- 手动在 mini-term 远端 PTY 中启动受支持 agent，Hook 能绑定到正确 pane/provider session。
- 新 incarnation 拒绝旧 Hook replay。
- relay 断线期间事件在有界 spool 中保留，重连 pull replay 幂等。
- replay 不重复触发完成通知或 attention。
- heartbeat/stale/disconnected 不产生假 done。
- remote wall clock 快/慢均不影响 freshness。
- Hook 缺失时 process/title 只显示低置信度临时状态，不持久化为权威身份。

### 验证命令

```bash
cargo test -p mt-ai
cargo test -p mt-ssh
cargo test -p mt-app ai
cargo test -p mt-app remote_ssh
cargo fmt --all -- --check
cargo clippy -p mt-ai -p mt-ssh -p mt-app --all-targets -- -D warnings
```

必须增加 provider contract fixtures，至少覆盖 Claude、Codex、Grok：正常 turn、权限、等待、失败、interrupt、SessionEnd、重复/乱序/迟到事件。

### 高风险文件

- `sidecars/src/bin/miniterm-hook.rs`
- `crates/mt-ai/src/hook_server.rs`
- `crates/mt-ai/src/monitor.rs`
- `crates/mt-ai/src/hook_registry.rs`
- remote runtime protocol/persistence

### 回滚点

Hook adapter 可逐 provider gate；关闭时回退现有输入/输出检测，但 UI 标记为 inferred，不继续显示上次 Hook 的 live 状态。

## Phase 7: Orca Project Sidebar 与 Worktree Workbench

### 依赖

Phase 1 worktree catalog + Phase 2 stable identity。可以先消费现有 `PaneStatus` compatibility projection，不阻塞于 Phase 3-6。

### 主要改动

- 重构主壳为 `Project Sidebar + active-worktree Workbench + Context Sidebar`，feature gate 为 `orca_project_shell`。
- 左侧 sidebar 顶部固定 Search 与 Agents，header 固定为 `Projects`；不显示 Workspace 文案，也不提供 Project/Status 分组切换。Phase 7 建立非模态 Agents 浮窗容器、toggle action 与 focus-return 骨架，live feed 数据由 Phase 10 接入。
- 在 `overlay.rs` 增加 Agents overlay kind，复用现有防叠开、Esc/点外关闭、anchored snap 和 previous-focus restore；Agents toggle 在该 overlay 打开时仍需可用。
- 构建 project/repo -> worktree 扁平 row model 和虚拟化列表；host 作为 metadata，不增加用户侧层级。
- 左下角只保留 Usage 与 Settings；Usage 复用现有 `mt-usage`/UsagePanel，移除常驻 runtime connection 文案。
- Worktree card 使用固定 status lane、host/repo identity、display name、branch/path、primary/sparse metadata。
- main worktree 与 linked worktree 使用相同 card model；旧 `parentProjectId` 仅作为迁移输入。
- 卡片先复用当前 AI 四态聚合，并预留 inline Agent row slot；后续 Phase 8 接入新状态协议。
- 中央 workbench 以 `WorktreeId` 为 scope，独立持久化 terminal tabs、file tabs、open files、active tab、split tree 和 view state；同 project 的 worktree 不共享 workbench state。
- 统一 terminal/file/按需 diff tab identity 和 tab strip；保留 split 布局。
- 将 `TerminalsPanel` 包成兼容 tab-group，禁止新增第二套顶层导航依赖。
- 新建 worktree 改为后台创建：sidebar pending row + 中央 faux tab + 成功原位 handoff + 失败 Retry/Remove；`Run in background` 与可选 Cancel 分开。
- Quick Open 搜索 worktree、tab；只有携带完整 `WorktreeId + TabId + PaneKey` target 后才展示 live Agent 结果。Phase 8 接入精确 Agent 路由前不得把 Agent 结果降级为仅激活 worktree。
- 主工作面始终是 `Workbench(active_worktree_id)`；Agents 使用独立 overlay state，打开时不收起右侧 contextual sidebar、不卸载 workbench，并记录打开前的 focus return target。

### 验收

- 多 host 同路径不冲突。
- 非权威空 scan 不闪空。
- sidebar 只按 Project -> Worktree 展示；选择、展开和滚动位置稳定。
- active worktree 切换不 kill 其他 worktree terminal，也不重建无关 sidebar row。
- 同一 project 的两个 worktree 可同时保存不同 terminal tabs、打开文件、active tab 和 split；来回切换无串位。
- file/diff/terminal 使用同一顶层 tab strip；没有双重 tab 导航。
- 左下角只出现 Usage 和 Settings，Usage 打开现有统计面板。
- 左侧固定显示 Search 与 Agents；打开/关闭 Agents 浮窗不改变 active worktree 的 tab、split、terminal、右栏 mode 或草稿，关闭后焦点回到原 pane。
- 创建 worktree 的 modal 提交后立即关闭，进度与错误在 sidebar/中央持续可见。
- 文本在常见桌面/窄窗口不溢出或遮挡。
- 1000+ worktree rows 下滚动、筛选和状态更新保持稳定。

### 验证命令

```bash
cargo test -p mt-app
cargo fmt --all -- --check
cargo clippy -p mt-app --all-targets -- -D warnings
```

增加 GPUI snapshot/interaction tests，覆盖 project/worktree row model、pending create、active worktree handoff、per-worktree tabs/open files、统一 tab strip 和 footer；若已有可执行 UI 测试基建，再覆盖桌面和窄窗口截图。

### 高风险文件

- `crates/mt-app/src/main.rs`
- `crates/mt-app/src/project_list.rs`
- `crates/mt-app/src/project_tree.rs`
- `crates/mt-app/src/workbench_area.rs`
- `crates/mt-app/src/terminal_area.rs`
- `crates/mt-app/src/terminals_panel.rs`
- `crates/mt-layout/`

### 回滚点

保留旧 `ActivityBar + ProjectList/FileTree` shell behind gate；两套壳共用同一 store/canonical row model，回滚只切 presentation，不回滚新 identity/schema。

## Phase 8: Worktree Context Sidebar 与恢复诊断

### 依赖

Phase 7 shell slots。Files/Git 可先实现；Sessions 的 live/stale 合并依赖 Phase 6，恢复状态依赖 Phase 3/4。

### 主要改动

- Worktree card 接入 inline Agent rows，状态优先级为 needs-you > working > done > waiting > unknown；点击精确定位 worktree/tab/pane。
- 右侧使用顶部 tab bar，固定 slots 为 `Files / Git / Tasks / Sessions`；本阶段实现 Files/Git/Sessions，Tasks 由 Phase 9 接入。
- `active_context_tab` 作为 App 级 route 全局保持；每个 panel 的 selection/expanded/scroll/filter/cache/generation 以 `WorktreeId` 分桶，切换时先换 scope 再刷新。
- `Files` 迁移现有 FileTree，并按 active `WorktreeId` 切换 root、watcher、selection 与 generation。
- 为统一 tab 模型增加 `WorktreeId + TabGroupId` scoped preview slot：首次追加到 tab order 末尾，后续原位替换；编辑、显式 Pin 或 preview 标签双击 promote 为 permanent。
- 文件树整行双击统一进入 rename，移除“文件名与图标/空白区域行为不同”的命中差异。
- `Git` 合并现有 GitPanel/GitHistory，展示 active worktree changes、diff、branch 和 commit tree。
- `Sessions` 迁移 SessionPanel，扫描 active worktree canonical path 下全部受支持 provider session，并把历史记录与 Phase 6 live status 分源合并。
- 统一 terminal tab 与 worktree card 的状态 glyph 和 unread priority；provider identity 与 activity glyph 分开。
- 增加 `Reattaching/Reattached/Restored from history/Agent resumed/Restart required/Recovery unavailable` 局部恢复状态。
- disconnected/stale 保留 last-known Agent activity，显示 Connect/Reconnect/Last seen，不生成假 done。
- 诊断视图显示 evidence、incarnation、last receipt、protocol/host 版本，不在普通 UI 暴露敏感 token、完整 cmdline 或环境。
- 明确应用退出、关闭 terminal tab、Remove Project、Delete worktree 四条交互与确认文案；Sleep worktree 延后到语义明确后。

### 验收

- 同一 worktree 多 Agent 行不串 pane，子 Agent 层级可展开且循环/坏 lineage fail closed。
- worktree card 与 terminal tab 对同一事件显示同一状态语义。
- blocked/working/disconnected 三种视觉语义互不覆盖；断线不会触发完成通知。
- 点击 inline Agent row 能精确切换 worktree、tab、split pane 并确认 unread；Quick Open 命中 Agent 会话时复用同一路由。
- 正常 warm reattach 无阻塞弹窗；cold restore/失败才显示持续反馈。
- Files/Git/Sessions 在切换 worktree 时不残留上一个 worktree 的数据、watcher、请求或 selection。
- 切换 worktree 不改变当前 context tab 类型；返回原 worktree 时恢复该 panel 自己的 selection、展开、scroll 和 filter。
- 连续单击多个文件只保留一个 preview 且 tab 位置不跳；双击文件行不固定 preview，双击 preview 标签/Pin/编辑会固定，跨 worktree/split group 不互相驱逐。
- 右栏菜单始终位于顶部并保持 `Files / Git / Tasks / Sessions` 顺序；各 tab 使用独立 loading/empty/error state。
- 1000+ worktree/agent rows 下状态更新不触发整栏重建。

### 验证命令

```bash
cargo test -p mt-ai
cargo test -p mt-layout
cargo test -p mt-app ai
cargo test -p mt-app session_panel
cargo test -p mt-app git_panel
cargo test -p mt-app remote_ssh
cargo fmt --all -- --check
cargo clippy -p mt-ai -p mt-layout -p mt-app --all-targets -- -D warnings
```

增加 GPUI interaction/screenshot tests，至少覆盖：project/worktree sidebar、Needs You/Working/Done/Disconnected、四个顶部 context tabs、worktree 切换 fencing、后台创建、warm reattach、cold restore、窄窗口右栏收起。

### 高风险文件

- `crates/mt-app/src/session_panel.rs`
- `crates/mt-app/src/git_panel.rs`
- `crates/mt-app/src/file_tree/`
- `crates/mt-app/src/store/ai.rs`
- `crates/mt-app/src/terminal_area.rs`
- `crates/mt-app/src/main.rs`

### 回滚点

`orca_worktree_context` gate 关闭时，sidebar/workbench shell 继续可用；Agent 状态回退现有兼容投影，右栏暂时挂回旧 FileTree/Session/Git content，但不回退 identity 或 remote protocol。

## Phase 9: GitHub Project Tasks

### 依赖

Phase 1 的 host-qualified project/repo identity + Phase 7/8 的右栏 `Tasks` slot和统一详情 tab。Local/WSL slice 可先接现有命令执行器；完整 SSH 支持依赖 Phase 5 的 authenticated bounded remote exec。与 terminal host 和 remote Agent status 无直接依赖。

### 主要改动

- 从 project 所属 execution host 的 Git remote 解析规范化 `GitHubRepoIdentity(owner/repo/host)`，不从 worktree display name 或客户端同名路径猜测。
- 新增 `mt-github` 领域层与 `ExecutionHostCommandRunner` 注入边界。Local 使用本机 `gh`/`gh.exe`，WSL 使用目标 distro 的 `gh`，SSH 使用远端 host 的 `gh`；任何目标不可用时 fail closed，不退回本机账户。
- 以结构化 argv 执行 `gh --version`、`gh auth status --hostname <host>`、Issue/PR list/view 和必要的 `gh api`，优先解析 JSON；限制 timeout/output，支持 cancellation 和 request generation。
- 新增只读 project task row/detail model。点击 Issue/PR 在中央统一 tab strip 打开 worktree-scoped `WorkItemDetail` preview/permanent tab，普通查看流程不打开浏览器。
- 同一 project 下所有 worktree 共享 `ExecutionHostId + ProjectId + GitHubRepoIdentity + auth generation` 的 fetch/cache/rate-limit state；每个 worktree 保留自己的 Tasks selection/filter/scroll 和详情 tab state。
- 增加 host-scoped auth probe 与错误分类：`client_missing`、`auth_required`、`wrong_host_or_account`、`scope_required`、`rate_limited`、`offline_or_disconnected`、`ready`。环境变量 token 覆盖 keyring 时只显示来源类型与修复提示，不显示值。
- 未认证时显示目标 execution host、`gh auth login --hostname <host>`、Copy 和 Retry。mini-term 不运行登录命令、不创建 terminal tab、也不调用 URL opener；用户自行登录后显式 Retry。
- host/account/remote URL/reconnect 变化时递增 auth/fetch generation 并取消旧请求；迟到结果只能写原 cache generation，不能覆盖当前 project。
- 网络、CLI 缺失、未认证、非 GitHub remote、权限不足、rate limit 和远端断线使用不同空态；失败不得影响 Files/Git/Sessions。
- 首版不实现创建、编辑、评论、merge 或 project board。

### 验收

- 同一 project 的 main/linked worktree 显示同一 Issues/PR 数据且不会重复请求。
- 切换到另一个 project 后不会残留前一个 repo 的 task rows。
- Local/WSL/SSH project 均使用所属 execution host 的 Git remote、`gh` 可执行文件和登录账户；远端未安装/未认证时不静默使用本地 `gh`。
- SSH/WSL 认证凭据不写入 layout/session/cache/log，也不经 RPC 转发到客户端；用户自行登录后凭据仍由对应 execution host 持有。
- 非 GitHub remote 显示明确空态；离线时保留有时间戳的 last-known cache。
- Issue 与 PR 点击在中央打开正确的只读详情，不打开系统浏览器；auth_required 也只显示命令，不调用 URL opener。
- 目标 GitHub host、账户或 auth generation 改变后，旧请求、旧详情和旧 rate-limit 状态不会污染新身份。
- 恶意标题、body、remote slug 和 CLI 输出只作为不可信数据处理；argv 无 shell 注入，Markdown 不渲染原始 HTML。

### 验证命令

```bash
cargo test -p mt-project
cargo test -p mt-github
cargo test -p mt-ssh github
cargo test -p mt-app github_tasks
cargo fmt --all -- --check
cargo clippy -p mt-project -p mt-github -p mt-ssh -p mt-app --all-targets -- -D warnings
```

使用 fake execution-host command runner 覆盖 native/Windows `gh.exe`、WSL distro、SSH remote exec、CLI 缺失、未登录非零退出、GHES hostname、账户/scope、env-token shadow、分页、401/403、rate limit、空 repo、remote 变更、断线、cancellation 和 stale generation。增加交互测试验证 Issue/PR 详情和 auth_required 都不调用 URL opener，登录命令包含正确 execution host/hostname，Copy 与 Retry 路径可用。

### 高风险文件

- 新增 `crates/mt-github/`
- `crates/mt-project/src/git.rs` 与 Git remote identity
- `crates/mt-ssh/` bounded remote exec
- `crates/mt-app/src/main.rs`、统一 tab/workbench action、Tasks panel
- Tasks auth remediation、clipboard 与 Retry orchestration

### 回滚点

`github_project_tasks` gate 关闭时保留 `Tasks` tab 与不可用空态，其余三个 context tabs 不受影响。

## Phase 10: Global Agent Activity Feed

### 依赖

Phase 6 的统一 live Agent state、Phase 7 的 Agents overlay slot，以及 Phase 8 的 `activate_agent_target` 精确路由。它不依赖历史 session transcript 扫描。

### 主要改动

- 左侧固定 Agents entry 与 needs-attention badge；badge 不统计全部 working/history。
- 浮窗内实现 `Needs You / Working / Recent`；它覆盖当前 workbench 但不替换 route、不卸载中央内容，也不强制收起右侧 contextual sidebar。
- 打开浮窗时记录 `focus_return_target`；关闭按钮、Agents toggle 与 Escape 走同一 close action，关闭后恢复原 pane 焦点。
- 首版按固定锚定浮层实现：锚在左侧 Agents 入口右侧、自动贴边，点外/Esc/关闭按钮/toggle 共用关闭动作；不实现拖动、缩放或 geometry 持久化。
- feed row identity 使用 `ExecutionHostId + AgentRunId`，并携带 `WorktreeId + TabId + PaneKey + TerminalIncarnationId` target。
- 与 inline Agent row、Quick Open 共用 `activate_agent_target`；成功聚焦目标 pane 后才 ack 对应 unread。
- 仅打开 Agents 浮窗不批量清 unread；重复/replay event 按 event id/sequence 去重，旧 incarnation 事件不得重新产生 badge。
- stale item 保留 last-known 状态并提供 `Open session history`；不得自动创建 terminal 或触发 provider resume。
- feed 只消费 Hook/runtime live state 和近期终态，不把右侧 Sessions 的文件扫描结果升级为 live activity。

### 验收

- v2 视觉基线缺失的 Agents 入口已固定补回，点击后打开非模态浮窗而非中央页面或阻塞式 modal。
- 打开/关闭浮窗和切换多个 worktree 时，所有 terminal 继续运行，tab/split/右栏/草稿状态无变化。
- 浮窗获得键盘焦点但不吞掉 terminal 输出；关闭后焦点精准返回原 pane，原 pane 已删除时安全回退到当前 active pane。
- 浮窗在正常和窄窗口下均保持 viewport 内可见，内容溢出只滚动列表，不改变锚点或遮挡关闭入口。
- 多 host 相同路径、同 provider 多 run 不串 item 或 target。
- 点击 live item 精确激活正确 worktree/tab/pane；目标消失时安全降级到 history，不误开新 shell。
- 打开 feed 不清 badge；只有逐条成功处理或权威状态变化才减少 needs-attention。
- replay、迟到和重复事件不产生重复行、重复通知或 unread 回弹。
- 1000+ recent rows 下筛选、状态更新和滚动不触发整个 shell 重建。

### 验证命令

```bash
cargo test -p mt-ai
cargo test -p mt-app ai
cargo test -p mt-app agent_activity
cargo fmt --all -- --check
cargo clippy -p mt-ai -p mt-app --all-targets -- -D warnings
```

增加 GPUI interaction/snapshot tests，覆盖 Needs You/Working/Recent、toggle/Escape/close、focus return、badge ack、精确 pane 路由、stale fallback、窄窗口和与右栏并存。

### 高风险文件

- `crates/mt-app/src/main.rs`
- `crates/mt-app/src/activity_bar.rs`
- `crates/mt-app/src/project_list.rs`
- `crates/mt-app/src/store/ai.rs`
- `crates/mt-app/src/terminal_area.rs`
- `crates/mt-app/src/overlay.rs`
- `crates/mt-ai/`

### 回滚点

`global_agent_activity_feed` gate 关闭时隐藏 Agents entry，inline Agent rows、Quick Open、right-side Sessions 和底层 live state 均继续工作；不回退 Agent identity/status 协议。

## 跨阶段检查

- 每个 child 开始前运行 `trellis-before-dev`，读取对应 package spec。
- 每个 child 完成后由 `trellis-check` 验证跨层 identity、错误和持久化路径。
- 新协议/身份/恢复语义在稳定后写入 `.trellis/spec/`，避免后续功能重新引入 runtime ID 或跨设备时钟比较。
- Phase 3、4、5、6、8、9、10 都需要故障注入测试，不能只测试 happy path。

## 研究任务完成检查

```bash
python3 ./.trellis/scripts/task.py validate 09-01-orca-worktree-terminal-research
git diff --check -- .trellis/tasks/09-01-orca-worktree-terminal-research
```

在执行 `task.py start` 前确认：

- `prd.md`、`design.md`、`implement.md` 与 research 文档内容一致。
- `implement.jsonl` 和 `check.jsonl` 均有真实 spec/research context。
- 用户已在最终规划摘要之后明确批准。
