# PR 审核整改与重新提交 — Technical Design

## 1. Delivery Boundary

本任务不在当前脏工作区直接重写旧分支。实现阶段在 `/tmp` 下创建独立 Git worktree，并从当时最新的 `origin/main` 建立新分支 `feat/remote-file-management-v2`。

这样同时满足三项约束：

1. 用户现有 `.claude/`、`.agents/`、`.trellis/` 等未提交文件保持原样。
2. 新分支天然不继承旧 PR 的 Trellis、journal、菜单提示归档和纯格式化提交。
3. 若移植失败，可删除临时 worktree/分支并重新开始，旧功能分支和测试 Release 不受影响。

## 2. Clean Change Transplant

从旧分支按 allowlist 以 `git cherry-pick --no-commit` 顺序移植真实产品与 CI 修复：

- `b3a9e85` — 远程文件管理主体
- `7aa43a3` — 文件操作生命周期加固
- `4aca5d4`、`820c62e`、`2ed5409`、`830717d` — changed-file/changed-line CI 门禁
- `28f2d0b`、`2d2c393`、`747dcbb`、`5ffc7d0`、`4d9fc4c` — 编译、Clippy 和边界修复
- `0e0db07` — 可移植工作区检查

明确丢弃：

- `2c209ca`、`5f80cda` — 无关菜单提示任务归档与 journal
- `ad09416`、`b7fa18f`、`60dcf6e` — 纯 rustfmt 重排
- `21c0eaa`、`43d1516`、`7ad5961`、`b760801` — `.trellis/` spec/task/journal/归档整理

冲突解决以 `origin/main` 的结构和手工格式为基线，只保留真实逻辑。移植完成后先检查 staged path，任何 `.trellis/` 路径立即阻断提交；再对 reviewer 点名文件做普通 diff 与 `-w` diff 对照，清除只改变空白或换行的 hunks。

最终以 `vihor3` 的本地 Git 身份生成新的干净提交，不保留旧提交作者或混杂历史。

## 3. Windows Download Boundary

### 3.1 Component validation

`valid_remote_name` 增加冒号拒绝规则。由于该函数位于所有远程叶子名进入本地/远程操作的共同边界，`C:evil.exe`、NTFS ADS 风格名称及其它含冒号名称会在路径拼接之前失败。

现有 `/`、反斜杠、NUL、`.`、`..` 规则继续保留。

### 3.2 Containment validation

新增纯路径 helper，输入：

- canonical/normalized 下载根目录；
- 当前父目录；
- 一个已验证的单路径组件名称。

helper 负责：

1. 再次调用 `valid_remote_name`；
2. 验证父目录位于下载根内；
3. 执行 `parent.join(name)`；
4. 验证结果仍以下载根为前缀，否则返回错误。

下载开始时在创建并验证下载目录后 canonicalize 一次根目录。顶层冲突扫描、顶层下载目标、递归子项以及 KeepBoth 后的实际目标都经过同一边界检查。暂存容器由安全目标的父目录创建，因此仍在 canonical 下载根内；提交前继续保持现有 staging/rollback 校验。

`download_conflicts` 改为返回 `Result<Vec<String>, String>`，使无效名称或边界错误在弹出冲突策略前成为可见错误，而不是静默当成“无冲突”。

## 4. Conflict Dialog Content

`show_file_conflict_choice` 接收冲突名称列表。新增纯格式化 helper：

- 保留扫描顺序；
- 最多显示前 5 个名称；
- 每个名称单独一行，作为纯文本渲染；
- 超过 5 项时通过 i18n 文案显示“另 N 项 / N more items”。

上传和下载调用点不再丢弃 conflicts。三种策略、批量应用语义、遮罩/Esc 取消和 busy ownership 均保持不变。

需要更新 `fileTree.ts` 中英文词条、生成 `dict.rs`，并补纯 helper 测试覆盖短列表与截断列表。

## 5. Remote Terminal cwd Fallback

`mt-pty::ssh::build_remote_login_command` 从：

```text
cd '<path>' && exec $SHELL -l
```

改为：

```text
cd '<path>' 2>/dev/null; exec $SHELL -l
```

路径仍由 `shell_single_quote` 包裹，命令注入边界不变。cwd 有效时行为不变；cwd 无效时 SSH 登录 shell 在默认登录目录启动。同步更新 argv 纯函数测试和注释。

## 6. Validation and Release Flow

本地只执行：

- i18n 生成并再次运行确认幂等；
- 静态搜索和路径/提交清单审计；
- `git diff --check`；
- Trellis task/context 校验（仅原工作区，本地文件不提交）。

明确不执行 `cargo fmt`、`cargo check`、`cargo build`、`cargo clippy` 或 `cargo test`。

推送新分支后由 GitHub Actions 执行 formatting、i18n、Cargo check、sidecar check、Clippy、Cargo test 和 whitespace。只在 Actions 全绿且 PR diff 无 `.trellis/`/格式噪音后，由 `vihor3` 创建新 PR。

## 7. Rollback

- 移植或冲突处理不可控：放弃临时 worktree 和新分支，旧分支保持不变。
- GitHub Actions 失败：仅在新分支追加针对性修复，不回退到旧混杂分支。
- 新 PR 创建前发现 diff 污染：停止推送/提 PR，重新从 `origin/main` 构建干净分支。
