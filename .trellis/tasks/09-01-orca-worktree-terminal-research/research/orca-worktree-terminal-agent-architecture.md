# Orca Worktree、终端恢复与远程智能体架构研究

## 研究范围

- 研究日期：2026-09-01。
- Orca 本地源码：`/home/leo/orca`，分支 `main`，commit `5aa02ead59a4f34a186c3e8814558b5795260ee9`。
- Orca 源码快照版本：`1.4.178-rc.2`。
- 目标不是逐行移植 Orca，而是提取能落地到 mini-term 的身份、所有权、持久化和恢复原则。
- 本文中的 Orca 路径均相对其源码仓库；mini-term 路径均相对当前仓库。
- UI/UX 的逐组件源码对照与迁移方案见 `research/orca-ui-ux-mapping.md`。

## 结论摘要

1. Worktree 列表不能只靠 libgit2 枚举和路径存在性。Orca 把 Git porcelain 输出作为事实层，明确区分权威扫描与 fallback，避免一次失败把 UI 中的全部 worktree 清空。
2. “关闭后精准恢复终端”有两个不同等级：
   - warm reattach：PTY 和子进程仍由独立 daemon 持有，GUI 重开后重新附着，是真正的同一进程继续运行。
   - cold restore：原进程已不存在，只能用持久化终端模型重绘历史画面，再启动新 shell，并可选续接 AI provider session。
3. 远程智能体识别也有两个不同问题：
   - capability probe 回答“这台机器装了哪些 CLI”。
   - launch attestation/provider Hook 回答“这个 pane 当前跑的是谁、哪个 provider session、处于什么状态”。
4. 稳定状态跟踪的关键不是增加更多进程名正则，而是建立 host、pane、terminal incarnation、agent run、provider session 和事件代际之间的可信关联。
5. mini-term 已有不少可复用基础，但当前的 `u32 pty_id`、本地 loopback Hook、GUI 内 PTY 所有权和 `ai-working/ai-idle/idle` 三态不足以支撑跨重启、跨主机的精确语义。
6. Orca 的 UI 优势来自三段式信息架构而不是配色；mini-term 借鉴其结构，但用户侧明确采用 `Project -> Worktree`：左侧 project/worktree 导航，中央每个 worktree 独立 workbench，右侧顶部 `Files / Git / Tasks / Sessions`。

## 1. Orca 的 Worktree 发现与解析

### 1.1 Git 事实层

`src/main/git/worktree-list-reader.ts` 首选执行：

```text
git worktree list --porcelain -z
```

`-z` 让字段和记录以 NUL 分隔，路径中即使包含换行也不会破坏解析。旧 Git 不支持 `-z` 时，Orca 回退到 `git worktree list --porcelain`，并缓存能力判断，避免每次扫描都重新触发失败。

`src/main/git/worktree-list-parser.ts` 解析的字段包括：

- `worktree`
- `HEAD`
- `branch`
- `bare`
- `sparse`
- `locked [reason]`
- `prunable [reason]`

首条记录被标记为 main worktree。文本 fallback 还处理 Git 的 C-style quoted 字符串，避免 lock/prunable reason 被错误展示。

主仓路径不是简单取当前 checkout。reader 使用 `git rev-parse --path-format=absolute --show-toplevel --git-common-dir`，并对旧 Git、separate-git-dir、submodule 和 WSL 路径做兼容。

### 1.2 扫描补充与失败语义

`src/main/git/worktree-listing.ts` 在基础列表上补充 sparse checkout 等信息，并限制并发，防止大量 worktree 同时启动 Git 子进程。

它区分两种调用：

- lenient：目录缺失、不是 Git 仓库或扫描暂时失败时返回可降级结果。
- strict：创建/删除等需要确定事实的操作将错误上抛。

新建 worktree 后如果列表暂时看不到新 checkout，`describeCreatedWorktree` 会直接验证新目录的 common-dir、branch 和 HEAD，构造刚创建的权威行，避免 UI 与实际创建结果短暂分裂。

`src/main/git/worktree-scan-cache.ts` 以 repo、distro、timeout、generation、scan kind 为 key 共享同一次 in-flight 扫描。Git mutation 会提升 generation，使旧扫描结果不能覆盖 mutation 后的新状态。

### 1.3 三层模型

`src/shared/worktree/types.ts` 体现了三层数据：

1. `GitWorktreeInfo`：Git 直接提供的事实。
2. `Worktree`：加上显示名、评论、PR/issue、pin/archive 等用户元数据。
3. `DetectedWorktree`：再加 ownership、visibility、host 等运行时投影。

列表结果还带 `authoritative` 和 source。这个字段是安全边界：

- 权威 Git 扫描可以证明一条 worktree 已消失。
- metadata/session fallback 只能补齐或保留可见性，不能据此清理已有行。

`src/shared/worktree/id.ts` 使用 repo identity 与 worktree path 组合身份；folder workspace 的实例后缀保留在 workspace identity 中，只在真正访问文件系统时剥离。

### 1.4 元数据合并与显示

`src/main/ipc/worktree-metadata-merge.ts` 将 Git 行和用户元数据合并：

- 自动显示名可来自短 branch、repo 名或目录 basename。
- 用户固定的名称优先。
- comment、issue/PR、pin、archive、unread、order、activity、lineage 和 sparse 信息不改变 Git 身份。

UI 先用 `visible-worktrees.ts` 做纯过滤，再由 `worktree-list/grouping/build-rows.ts` 生成扁平行模型，最后使用 TanStack virtualizer 渲染。host-qualified row key 防止不同设备上的相同 repo/path 冲突。

### 1.5 对 mini-term 的直接启示

mini-term 当前 `crates/mt-project/src/git.rs` 的 `WorktreeInfo` 只保留 `name/path/branch/is_main/is_valid/is_locked`，`list_worktrees` 主要依赖 libgit2 `Repository::worktrees`。缺失项包括：

- HEAD OID、bare、sparse、lock/prunable reason。
- `--porcelain -z` 对换行路径的可靠解析。
- Git 版本能力 fallback 和 WSL 路径兼容。
- authoritative/fallback 标记。
- mutation generation 和共享 in-flight scan。
- host-qualified worktree identity。

`crates/mt-app/src/git_worktree.rs` 已有管理弹窗、后台操作和安全删除顺序，可保留为操作层；事实发现和身份应下沉到 `mt-project`。

## 2. Orca 的独立终端与精准恢复

### 2.1 稳定身份

相关文件：

- `src/shared/pty-session-id-format.ts`
- `src/main/daemon/pty-session-id.ts`
- `src/shared/stable-pane-id.ts`

Orca 不把 renderer 的临时组件实例或数组下标当作终端身份。核心身份至少包含：

- worktree identity
- stable pane key
- terminal session id
- session incarnation

PTY session id 由 worktree identity 与随机短 UUID 组成。incarnation 用于区分“同一个持久 session key 背后已经换过新进程”的情况。

### 2.2 PTY 所有权离开 GUI

`src/main/daemon/daemon-entry.ts` 是独立 Node 进程，使用 `node-pty` 持有终端。`daemon-spawner.ts` 使用版本化 Windows named pipe 或 POSIX Unix socket，并维护 token、PID、nonce 和端点所有权。

`src/main/startup/main-process-quit.ts` 在正常退出时只 disconnect daemon，不 shutdown。结果是：

- renderer/main process 可以退出。
- daemon、PTY 和前台/后台子进程继续运行。
- 下次 GUI 打开时可以 warm reattach。

`src/main/daemon/terminal-host.ts` 以 `sessionId -> Session` 管理会话。`createOrAttach` 按 session id 串行化：

- 已有 live session：返回权威 snapshot，标记 `isNew: false`，增加 client attachment。
- `attachOnly` 且会话不存在：明确失败，不静默创建新 shell。
- client detach 不等于 kill；没有 client 时仍允许 PTY 继续运行。

### 2.3 Headless terminal model

`src/main/daemon/headless-emulator.ts` 在 daemon 内维护 `@xterm/headless` 状态，而不是让 renderer 成为唯一终端模型。

快照不仅是 scrollback 字符串，还包含：

- 主屏/备用屏可重放 ANSI。
- cursor 和 wrap-pending 状态。
- terminal modes 与 rehydrate sequences。
- cwd、title、cols、rows、scrollback depth。
- OSC links。
- partial escape tail。
- output sequence 和 owner 信息。

partial escape tail 必须最后重放，否则一个被拆在两个输出 chunk 中的 CSI/OSC 序列会在恢复时损坏。

### 2.4 输出顺序、checkpoint 与增量日志

`src/main/daemon/session-output-plane.ts` 把 output、resize、clear 放进同一顺序平面。pending output 有界；超限时不继续积累不完整增量，而是要求完整 snapshot。

`src/main/daemon/history-manager.ts` 和相关 history 文件为每个 session 保存：

```text
meta.json
checkpoint.json
output.log
```

实现要点：

- 正常阶段追加增量 output log。
- 周期性或断开/溢出/日志达到上限时生成 full checkpoint。
- checkpoint 使用临时文件加 rename 原子替换。
- output frame 带 generation、batch sequence 和长度。
- sequence/generation 缺口 fail closed。
- 文件尾 torn frame 只截断到最后一个完整 frame。
- 大快照优先裁掉最老 scrollback，保留可见 frame、模式和元数据。

`src/main/daemon/daemon-pty-runtime-state.ts` 对 checkpoint 周期、并发和 full checkpoint 冷却做全局限流，避免大量 pane 同时序列化卡住 daemon。

### 2.5 Warm reattach 与 cold restore

Warm reattach：

1. GUI 用持久 terminal session id 请求 attach。
2. daemon 返回当前权威 snapshot、当前尺寸和 incarnation。
3. renderer 先恢复结构状态，再接实时输出。
4. 用户继续操作的是原 PTY 中的同一进程。

Cold restore：

1. history reader 读取 checkpoint 和增量日志。
2. 在 scratch emulator 中验证并重放。
3. 启动一个新 shell/PTY。
4. 把恢复画面绘制到新终端模型。
5. 若保存了 provider session，按 provider 的 resume 协议启动 AI。

`src/renderer/src/components/terminal-pane/pty-connection/apply-reattach-payload.ts` 的优先级是 snapshot > replay > coldRestore。结构重放期间抑制真实 PTY resize，按源尺寸重放后再 fit，partial escape tail 最后应用。

因此 cold restore 只能恢复可见终端历史和可续接的 AI 对话，不能恢复 shell job table、管道、任意调试器或已经死亡的子进程。

### 2.6 Workspace UI 状态

`src/shared/workspace-session-state-types.ts` 与 schema 保存的不是单一 active tab，而是完整的 host/worktree 会话切片，包括：

- active repo/worktree/tab。
- tabsByWorktree。
- 递归 split layout。
- leaf -> terminal session id。
- buffer/ref/title。
- per-worktree active tab/type。
- remote sessions、sleeping agents。
- per-pane incarnation、topology revision、tombstone。

schema salvage 对独立坏项做丢弃，而不是一处脏数据导致整份 workspace state 作废。renderer 退出前同步 capture/stage，main 在 deadline 内异步 flush。

### 2.7 mini-term 当前差距

`crates/mt-app/src/persist.rs` 已保存 split/pane、shell、cwd 和 AI session；`crates/mt-app/src/store/panes.rs` 重启后重新创建 PTY，并在存在 AI session 时准备 resume。

但：

- `crates/mt-pty/src/lib.rs` 中 `PtySession::drop` 会结束子进程。
- `crates/mt-app/src/pane.rs` 的 `TerminalPane::drop` 会 shutdown PTY。
- PTY 由 GUI 内 `TerminalPane` 直接持有。
- pane/PTY id 是运行时数字，重启后重分配。
- `mt-terminal` 的 alacritty 状态机没有持久 snapshot codec。

所以现状属于“重建布局 + 新 shell + 可选 AI resume”，不是 warm reattach。

## 3. Orca 的远程智能体识别与状态跟踪

### 3.1 Capability probe 只判断可用性

`src/shared/tui-agent-detection-commands.ts` 从统一 TUI agent config 生成检测命令、required commands 和 runtime exclusion。

`src/main/preflight/agent-detection.ts` 的 `detectRemoteAgents` 通过现有 SSH relay 请求 `preflight.detectAgents`。断线时返回空列表而不是抛出 UI 错误。

这只回答“远端 PATH 中哪些命令可用”，不能证明某个 pane 正在运行哪个 agent。

### 3.2 精确身份来自 provider 专用 Hook

`src/shared/agent-status-types.ts` 明确写明状态来自 native agent hooks，不从终端标题推断；标题、OSC 和 process evidence 只是不同来源的较弱观察。

Orca 为不同 provider 安装专用 Hook/插件。`src/main/agent-hooks/managed-agent-hook-registry.ts` 和 `remote-managed-hook-installers.ts` 管理 Claude、Codex、Gemini、OpenCode、Cursor、Grok 等适配器。

远端安装前先正向检测并应用 allowlist。空 allowlist fail closed，不能解释成“给所有 provider 写配置”，避免为未安装的 CLI 创建配置目录或破坏用户配置。

Hook source 由 provider 专用 URL/插件路径确定，而不是由一个通用脚本根据 payload 形状猜测。每个事件还携带：

- pane key、tab id、worktree id。
- launch token。
- provider session id。
- provider event name。
- agent type/model/tool/prompt/subagent 信息。

Provider session id 只用于精确 resume，不与 terminal session id 混用。

### 3.3 远程 relay 与 trust boundary

`src/shared/agent-hook-relay.ts` 定义 relay -> Orca 的 JSON-RPC envelope。remote relay 先做 provider normalization，main process 在 SSH 信任边界再次做 canonical validation。

关键规则：

- wire 上的 `connectionId` 恒为 `null`。
- Orca 在接收时根据当前 mux/SSH attachment 注入真实本地 connection id。
- 远端进程不能自行宣称属于哪个本地连接或主机。
- envelope 带协议 version/env，主进程可以诊断旧 Hook/旧 relay。
- 过大 frame 可以显式 shed 可选字段，但不能把“传输裁剪”误解为“provider 主动清空”。

`src/main/ssh/ssh-relay-session.ts` 先注册 `agent.hook` notification handler，再主动调用 `agent_hook.requestReplay`。这避免 relay 在订阅尚未建立时推 replay，导致事件静默丢失。

连接丢失时，Orca 清除属于该 connection 的 live status；重连后由 relay last-payload cache 重放。`isReplay` 事件受 launch token hash 和已恢复状态 watermark 限制，旧进程不能覆盖新会话。

### 3.4 状态归一化

Orca 的主状态是：

```text
working | blocked | waiting | done
```

并额外保存 model、tool、interactive prompt、assistant message、subagent roster、provider session、interrupted、session boundary 和 orchestration context。

`AgentStatusEntry` 以稳定 pane key 为主索引，同时记录 worktree、terminal handle 和 connection id。agent type 是可扩展字符串；内置名称只是便利 union。

Provider adapter 负责把原生事件转换为统一语义。例如权限请求、等待用户回答、工具执行、回合完成和 session end 不能只靠同一个 `idle` 状态表达，必须保留 reason/cause。

### 3.5 顺序、代际与恢复状态

`src/shared/agent-status-observation.ts` 为每条观察记录：

- `origin`
- `authorityId`
- `incarnation`
- `revision`
- `observedAt`
- `boundary/kind`

同一 authority 内，`(authorityId, incarnation, revision)` 可以排序；不同 authority 之间不可直接比较 revision。

文件还指出一个重要风险：远端主机时钟可能快或慢，客户端不能用自己的 `now` 减远端 `updatedAt` 判断新鲜度。正确做法是使用接收端 receipt time，或让 authority 直接给出 freshness verdict。

`last-status.json` 持久化最近状态。重启 hydrate 后，非终态行被标记 `restoredUnconfirmed`：它只提供 UI 连续性，不是当前仍在工作的证据。任何 accepted live event 会清除此标记。

PTY/agent 结束但 Stop/SessionEnd 丢失时，Orca 使用执行主机的 PTY/进程 liveness 做 reconciliation。它不会把一个无法证明的 stale `waiting/working` 行伪造为正常完成；若进程已确定消失，则清理 live claim 或标记退出。

### 3.6 对 mini-term 的直接启示

mini-term 已有本地 Hook 能力：

- `sidecars/src/bin/miniterm-hook.rs` 读取 provider payload，注入 `MINITERM_PTY_ID`，POST 到本机 loopback server。
- `crates/mt-ai/src/hook_server.rs` 记录 pty -> status/session，并用会话墓碑防止 SessionEnd 后迟到事件复活状态。
- `crates/mt-ai/src/monitor.rs` 让 Hook 状态优先于输出轮询，并保留停摆收敛逻辑。
- `crates/mt-ai/src/hook_registry.rs` 管理 Claude、Codex、Grok Hook。

但远程场景存在结构缺口：

- Hook server 仅监听本地 `127.0.0.1`，远端设备上没有 relay/endpoint/spool。
- Hook 身份只使用运行时 `u32 pty_id`，没有 host、stable pane、terminal session/incarnation 或 launch token。
- `miniterm-hook` 仍会根据 `turn_id`/`GROK_SESSION_ID` 猜 agent；扩展 provider 后容易误判。
- 状态只有 `ai-working/ai-idle/idle`，需要依赖 cause 才能区分权限、等待、完成和失败。
- 没有 remote sequence、event id、ack/replay、connection stamp、generation fence 或 persisted last status。
- `crates/mt-app/src/remote_ssh/sessions.rs` 能经 SFTP 扫描远端 Claude/Codex 历史，但历史扫描不是 live process identity，也不能替代 Hook。
- `crates/mt-ai/src/monitor.rs` 的现有注释已承认 SSH pane 没有 Hook 时依赖输入/输出降级，这只能作为低置信度 UI 提示。

## 4. Orca 的 GitHub CLI、认证与执行边界

Orca 的 GitHub 数据层不是 renderer 直接持有 token 调 API，而是统一经过 `ghExecFileAsync`：

- `src/main/git/command-runner/gh-exec-file.ts` 是 `gh` 单一 spawn 点，使用 argv 而不是 shell 字符串，默认禁用非预期 prompt，并集中处理 timeout、cancellation、幂等重试和 rate-limit breaker。
- runner 会把 runtime 与 GitHub host 纳入 scope；原生 Windows/macOS/Linux 与 WSL 使用各自可解析到的 GitHub CLI，GHES 请求会显式绑定 hostname，避免误发到 `github.com`。
- `src/main/github/work-item-details.ts` 和对应测试通过 `gh api`/GraphQL JSON 获取 work item 详情；WSL 测试确认命令携带目标 distro，而不是在错误 runtime 上猜同名路径。
- `src/main/github/client-ssh-provider-execution-boundary.test.ts` 对 SSH repo 的 Git 身份采取 fail-closed：remote provider 不可验证时不能退回本地同名路径。Orca 当前这条测试主要约束 Git discovery；mini-term 的产品要求进一步收紧为 SSH project 的 `gh` 查询本身也必须在远端 execution host 执行。

认证同样以 GitHub CLI 和 host 为中心：

- `src/main/preflight/agent-detection.ts` 用 `gh auth status` 探测 native/WSL 登录态，并认识到未登录时命令会非零退出。
- `src/main/github/auth-diagnose.ts` 同时解析 stdout/stderr，区分 `gh` 缺失、active account、host、scope，以及 `GH_TOKEN/GITHUB_TOKEN` 覆盖 keyring credential 的情况。
- `src/main/github/project-view/internals.ts` 和 `project-error-classification.ts` 对目标 GHES host 生成 `gh auth login --hostname <host>`，并把 auth、scope、rate limit、network、not found 分成不同错误类型。
- `src/renderer/src/components/github-project/GhAuthErrorHelp.tsx` 明确 GHES 登录按 host 隔离；环境变量 token 无法通过 `gh auth refresh` 修改时，需要给出不同修复路径，不能重复提示一个无效命令。

Orca 的 GitHub UI 当前主要提供复制登录命令/重载。其 `src/cli/handlers/account.ts` 另有面向 headless/SSH 的 device-auth 说明，但该段是 Codex account 登录，不是 GitHub 登录实现。mini-term 的产品决策更接近 Orca GitHub UI 的保守路径：只给出目标 host 的登录命令，由用户自行执行，不托管远端 GitHub auth flow。

对 mini-term 的结论：

1. Local 使用本机 `gh`/`gh.exe`，WSL 使用该 distro 的 `gh`，SSH 使用远端 host 的 `gh`；Git remote、GitHub account 和请求运行时必须一致。
2. Issue/PR 列表与详情都解析结构化 JSON并在 mini-term 内显示；浏览器不是数据 transport，Tasks 也不主动打开授权页。
3. 未认证时只显示目标 execution host 的 `gh auth login --hostname <host>`；用户自行执行，凭据由该 host 的 GitHub CLI credential store 持有，客户端不得复制 token 到 layout、session 或远端环境。
4. auth/cache key 至少包含 `ExecutionHostId + GitHub host + account/auth generation`；host、账户或环境 token 改变后 fence 旧请求。

## 5. 推荐给 mini-term 的证据优先级

从高到低：

1. `provider_hook_attested`：provider 专用 Hook，且 launch token、terminal incarnation、host attachment 全部匹配。
2. `launch_attested`：mini-term 自己启动 agent，命令与 adapter 已知，但尚未收到 provider session Hook。
3. `structured_terminal`：受协议约束的 OSC/side channel，带 stable pane/session 身份。
4. `process_verified`：terminal host 查询自己的 PTY 前台进程树，匹配规范化 executable signature。
5. `title_inferred`：终端标题或输出关键字，仅做临时展示，不持久化为权威身份。

规则：低优先级证据不能覆盖同一 incarnation 中更新的高优先级证据；新 incarnation 会使旧证据整体失效。

## 6. 推荐的远程状态规则

- 设备连接状态和 agent 活动状态分成两个轴。
- relay heartbeat 过期：`connectivity = stale/disconnected`，保留 last-known activity，不改成 `done`。
- provider Hook 报终态：可进入 `done/failed/interrupted`。
- terminal host 证明 PTY/agent process 已退出但无 provider 终态：进入 `exited` 或清理 live claim，不伪造 `done`。
- 重启加载的非终态：`restored_unconfirmed`，直到 live Hook、terminal inventory 或进程 liveness 重新确认。
- 所有 freshness 计时使用本地 receipt time；远端 wall clock 仅用于展示和诊断。
- replay 必须幂等，并按 relay instance、stream sequence、terminal incarnation 和 launch token fence。

## 7. 推荐落地顺序

1. Worktree porcelain catalog 与 host-qualified identity。
2. Stable pane/terminal identity 持久化。
3. 独立 terminal host 与 warm reattach。
4. Headless emulator snapshot、checkpoint 和 cold restore。
5. 远程 runtime/relay 的 host identity、heartbeat、spool 和 session inventory。
6. Provider Hook adapters、agent identity/status state machine 和 replay reconciliation。
7. Orca 式 project sidebar、per-worktree tabs/open files 和后台创建反馈。
8. 内联 Agent 行、右侧 `Files/Git/Sessions` contextual sidebar 和恢复诊断。
9. Execution-host-scoped `gh` Tasks provider、内部只读 Issue/PR 详情和 host-local auth flow。
10. 固定锚定在左侧入口右侧的全局 Agents 非模态浮窗，复用 live target 与精确 pane 路由，不引入新的中央页面。

## 8. 主要风险

- Rust alacritty terminal model 没有现成的完整 serde snapshot；需要设计 replayable ANSI + modes + metadata codec，并用宽字符、组合字符、alternate screen、resize、拆分 escape sequence 做 golden/fuzz 测试。
- SSH 断线不等于远端 PTY 已死；若先做 UI 状态而没有 remote session inventory，会制造大量假完成和假退出。
- 仅用 remote path 或本地 SSH connection id 作为 host/worktree identity，会在同机多连接、重命名连接或不同设备同路径时串台。
- Hook 配置是用户级变更，必须只修改已正向检测且用户允许的 provider，并保留非 mini-term 条目。
- launch token 是防旧代际串台的关联凭据，不应被误当成对同一用户账户内恶意进程的强安全边界。
- SSH project 若只在客户端运行 `gh`，会把本地账户、rate limit 和 repo identity 错配到远端项目；远端 `gh` 缺失或未登录必须显式报错，不能静默回退本地。
- `gh auth status` 的诊断文本不是稳定 JSON；只把它用于有限 auth 分类并保留兼容 parser，Issue/PR 正常数据必须使用 JSON 输出。

## 源码索引

### Orca

- Worktree：`src/main/git/worktree-list-parser.ts`、`worktree-list-reader.ts`、`worktree-listing.ts`、`worktree-scan-cache.ts`。
- Worktree 类型/UI：`src/shared/worktree/types.ts`、`src/shared/worktree/id.ts`、`src/main/ipc/worktree-metadata-merge.ts`、`src/renderer/src/components/sidebar/worktree-list/`。
- Workspace shell/UI：`src/renderer/src/components/sidebar/index.tsx`、`SidebarNav.tsx`、`SidebarHeader.tsx`、`WorktreeList.tsx`、`worktree-card-surface.tsx`、`WorktreeCardAgents.tsx`、`AgentStateDot.tsx`、`src/renderer/src/app-shell/AppWorkspaceShell.tsx`。
- Workbench/context sidebar：`src/renderer/src/components/terminal-titlebar/TerminalTitlebarTabs.tsx`、`TerminalTabLeadingIcon.tsx`、`src/renderer/src/components/right-sidebar/index.tsx`、`use-right-sidebar-activity-items.ts`、`right-sidebar-panel-content.tsx`。
- Background creation：`src/renderer/src/components/sidebar/worktree-list/PendingWorktreeRow.tsx`、`src/renderer/src/components/worktree-creation/WorktreeCreationPanel.tsx`。
- Terminal host：`src/main/daemon/terminal-host.ts`、`session.ts`、`daemon-entry.ts`、`daemon-spawner.ts`。
- Terminal snapshot/history：`src/main/daemon/headless-emulator.ts`、`terminal-snapshot.ts`、`history-manager.ts`、`history-reader.ts`、`session-output-plane.ts`。
- Renderer restore：`src/renderer/src/components/terminal-pane/pty-connection/apply-reattach-payload.ts`。
- Workspace state：`src/shared/workspace-session-state-types.ts`、`workspace-session-schema.ts`、`workspace-session-salvage.ts`。
- Agent types/listener：`src/shared/agent-status-types.ts`、`agent-status-observation.ts`、`agent-hook-listener/`。
- Remote agent relay：`src/shared/agent-hook-relay.ts`、`src/main/ssh/ssh-relay-session.ts`、`src/main/agent-hooks/server/`。
- Remote capability/hooks：`src/shared/tui-agent-detection-commands.ts`、`src/main/preflight/agent-detection.ts`、`src/main/agent-hooks/remote-managed-hook-installers.ts`。
- GitHub CLI/auth：`src/main/git/command-runner/gh-exec-file.ts`、`src/main/github/auth-diagnose.ts`、`src/main/github/project-view/internals.ts`、`project-error-classification.ts`、`src/main/github/work-item-details.ts`、`src/renderer/src/components/github-project/GhAuthErrorHelp.tsx`、`src/main/github/client-ssh-provider-execution-boundary.test.ts`。

### mini-term

- Worktree：`crates/mt-project/src/git.rs`、`crates/mt-app/src/git_worktree.rs`。
- Layout/restore：`crates/mt-app/src/persist.rs`、`crates/mt-app/src/store/panes.rs`、`crates/mt-layout/`。
- PTY/terminal：`crates/mt-pty/src/lib.rs`、`crates/mt-app/src/pane.rs`、`crates/mt-terminal/`。
- AI Hook/status：`sidecars/src/bin/miniterm-hook.rs`、`crates/mt-ai/src/hook_server.rs`、`monitor.rs`、`hook_registry.rs`。
- SSH/remote sessions：`sidecars/src/daemon.rs`、`sidecars/src/bin/mt-ssh-cli.rs`、`crates/mt-ssh/`、`crates/mt-app/src/remote_ssh/sessions.rs`。
- Current shell/UI：`crates/mt-app/src/main.rs`、`project_list.rs`、`project_tree.rs`、`terminal_area.rs`、`terminals_panel.rs`、`session_panel.rs`、`git_panel.rs`、`file_tree/`。
