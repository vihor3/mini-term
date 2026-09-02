# PR 审核整改与重新提交

## Goal

关闭由 `vihor6` 创建的 PR #53，在不提交任何 `.trellis/` 内容的前提下，按维护者审核意见修复安全与交互问题、清理无关格式化噪音，并由 `vihor3` 从干净分支重新提交可审阅、可合并的 PR。

## Background

- PR #53 已关闭；GitHub 不支持真正删除 PR，关闭记录会保留。
- 当前功能分支相对 `origin/main` 落后 10 个提交、领先 21 个提交，历史中混入了 Trellis 归档、journal 和多次纯 rustfmt 提交。
- `origin/main` 已通过 `f7a3a45` 移除 Trellis 集成；旧 PR 仍把约 1500 行 `.trellis/` 内容显示为新增文件。
- PR #53 的 GitHub Actions `Rust workspace` 已通过，但维护者提出合并前必须整改的安全、交互和仓库卫生问题。
- 用户明确要求 `.trellis/` 不得出现在后续提交或新 PR 中。
- 用户明确要求 Rust 编译、Clippy 和测试继续只由 GitHub Actions 执行，不在本地运行。

## Confirmed Findings

### Windows 下载路径安全

- `crates/mt-app/src/remote_ssh.rs:487` 的 `valid_remote_name` 拒绝 `/`、`\\`、NUL、`.` 和 `..`，但允许 `:`。
- 顶层下载目标在 `remote_ssh.rs:2627` 使用 `download_dir.join(&name)`；递归子项在约 `:2541` 使用父目标 `.join(&entry.name)`。
- Windows 的盘符相对名称如 `C:evil.exe` 可能令 `Path::join` 脱离下载根目录；当前仅检查下载目录自身为绝对路径，没有对每个最终目标执行下载根包含关系校验。

### 冲突提示

- `upload_conflicts` 与 `download_conflicts` 已返回具体冲突名称。
- `crates/mt-app/src/file_tree.rs:1554` 附近把上传冲突列表丢弃为 `_`；下载调用点同样只判断是否为空。
- `crates/mt-app/src/prompt.rs:364` 的 `show_file_conflict_choice` 只显示固定文案，覆盖按钮为主按钮，用户看不到将被覆盖的项目。

### PR 历史与格式化噪音

- 当前分支包含 `.trellis/` 新增文件以及菜单提示任务的归档记录，与远程文件管理产品改动无关。
- `ad09416`、`b7fa18f`、`60dcf6e` 是纯格式化提交；其中 `ad09416` 对 19 个文件产生约 2000 行重排。
- 仅在旧分支上追加删除/反向格式化提交仍会保留混杂历史。推荐从最新 `origin/main` 创建新的干净分支，只移植真实产品与 CI 改动，再实施审核修复。

### 审核中的非阻塞项

- `crates/mt-pty/src/ssh.rs:94` 生成 `cd '<path>' && exec $SHELL -l`；远程 cwd 失效会令 shell 不启动。改成容错 `cd ... 2>/dev/null; exec ...` 属于小范围修复。
- `settings/pages_system.rs:80-86` 在 render 中启动下载目录写探针验证；移动到页面切换事件需要调整设置页生命周期。
- 长传输目前没有实时字节进度和取消能力，属于新的交互能力。
- 本地 `CopyConflictPolicy::Overwrite` 当前没有 UI 调用点，删除会影响 `mt-project` 原语与现有测试，和本次合并阻塞无直接关系。

## Requirements

- R1：新 PR 的提交历史和最终 diff 均不得包含 `.trellis/`、workspace、task、journal 或个人开发过程文件。
- R2：从最新 `origin/main` 建立由 `vihor3` 推送的新分支，不复用旧 PR 的混杂提交历史。
- R3：拒绝可能被宿主路径规则解释为盘符或 ADS 的远程名称，并在下载顶层及递归落地前验证目标仍位于配置的下载根目录内。
- R4：为 Windows 路径逃逸增加不触网的针对性测试，覆盖冒号名称、顶层目标和递归目标边界。
- R5：上传/下载冲突弹窗展示具体名称，默认显示有限条目并在超出时显示剩余数量；原有跳过、覆盖、生成副本及取消语义不变。
- R6：保留必要的 changed-lines rustfmt/Clippy CI 门禁，但移除与真实逻辑无关的 rustfmt 重排。
- R7：所有代码修改保持项目既有手工格式，不在本地运行 `cargo fmt`、Cargo 编译、Clippy 或 Rust 测试。
- R8：本地仅执行静态搜索、i18n 生成/幂等、任务校验和 `git diff --check` 等非编译检查；Rust 质量门由新分支 GitHub Actions 完成。
- R9：旧 PR #53 保持关闭；整改完成并由 Actions 验证通过后，以 `vihor3` 创建新的 PR，并在描述中逐项回应原审核意见。
- R10：远程终端请求的 cwd 不存在时，忽略 `cd` 失败并继续启动登录 shell；路径引用与命令注入防护保持不变。

## Acceptance Criteria

- [ ] AC1：新分支基于提交时最新的 `origin/main`，新 PR 作者和 head 仓库均为 `vihor3`。
- [ ] AC2：`git diff origin/main...HEAD -- .trellis` 无输出，提交列表不包含 Trellis/task/journal 提交。
- [ ] AC3：远程条目名 `C:evil.exe`、含 `:`、反斜杠、NUL、`.`、`..` 均不能成为本地下载目标。
- [ ] AC4：所有下载目标在创建、冲突扫描和递归展开阶段均不能逃出下载根；违反边界时返回可见错误且不写入目标。
- [ ] AC5：上传和下载发生冲突时，弹窗列出冲突名称；长列表被确定性截断并显示剩余数量。
- [ ] AC6：新 PR 不包含 `main.rs`、`activity_bar.rs`、`file_viewer.rs` 等文件中的无关格式重排，真实逻辑改动仍保留。
- [ ] AC7：GitHub Actions 的格式、i18n、Cargo check、Clippy、测试和 whitespace 检查全部通过。
- [ ] AC8：新 PR 描述包含原 PR #53 链接、整改清单、验证记录，并明确 `.trellis/` 已剔除。
- [ ] AC9：远程 pane 的 cwd 有效时进入指定目录；cwd 失效时仍能启动登录 shell，并由 GitHub Actions 中的纯函数测试覆盖命令形态。

## Constraints

- 不重写或强推旧 PR #53 的分支作为最终交付；使用新的干净分支和新 PR。
- 不删除用户工作区现有的未跟踪 `.trellis/` 文件，只保证它们不被 Git 跟踪、不进入提交。
- 不把审核中明确标为后续增强的能力偷偷扩展进本次 PR。
- 不在本地执行 Rust 编译或测试。

## Key Decisions

- 使用基于最新 `origin/main` 的新分支和新提交，不复用旧 PR 的混杂历史。
- 仅移植真实产品与 CI 改动；丢弃 Trellis/task/journal、菜单提示归档和纯 rustfmt 提交。
- 本次处理全部合并阻塞项、冲突名称提示和小型远程 cwd 回退。
- 审核中明确标为后续增强的三个较大 minor 单独跟进，不扩大本次 PR。
- 新 PR 在 GitHub Actions 全绿后再创建，作者和 head 仓库均使用 `vihor3`。

## Out of Scope

- 设置页验证任务生命周期重构。
- 长传输实时进度与取消。
- 删除尚无 UI 调用点的本地 Overwrite 原语。
