# PR 审核整改与重新提交 — Implementation Plan

## Preconditions

- [ ] PRD 与 design 获得用户最终批准。
- [ ] 运行 `task.py start`，任务状态进入 `in_progress`。
- [ ] 执行 `trellis-before-dev`，加载 mt-app / mt-pty / mt-i18n 相关约定。
- [ ] 记录当前工作区 dirty 状态；后续产品改动只在独立 `/tmp` worktree 完成。

## Phase A — Clean Branch Reconstruction

1. [ ] 获取最新 `origin/main`，创建独立临时 worktree 和 `feat/remote-file-management-v2`。
2. [ ] 按 design allowlist 使用 `git cherry-pick --no-commit` 移植真实产品/CI改动。
3. [ ] 解决与最新 main 的冲突，始终保留 main 的手工格式和最新行为。
4. [ ] 确认 staged/working diff 中没有 `.trellis/`、task、journal、菜单提示归档。
5. [ ] 对纯 rustfmt 提交涉及的文件逐个比较普通 diff 与 `git diff -w`，还原无逻辑意义的重排。
6. [ ] 形成新的干净基础提交，作者使用 `vihor3 <vihor3@gmail.com>`。

Rollback point：移除临时 worktree 和未推送的新分支，旧功能分支不动。

## Phase B — Download Path Safety

1. [ ] `valid_remote_name` 拒绝 `:`。
2. [ ] 新增统一的本地下载目标 containment helper。
3. [ ] 下载开始时 canonicalize 已创建并验证的下载根。
4. [ ] 顶层冲突扫描改为 fallible API，并使用统一 helper。
5. [ ] 顶层下载、递归子项、KeepBoth 实际路径和 staging 提交前复用边界检查。
6. [ ] 增加纯测试：冒号名称、父目录越界、目标越界、安全子项。

Review gate：任何本地写入路径都必须能追溯到同一 canonical 下载根和单组件校验。

## Phase C — Conflict UX

1. [ ] 修改 `show_file_conflict_choice`，接收冲突名称。
2. [ ] 新增最多 5 项的确定性格式化 helper 和剩余数量文案。
3. [ ] 上传/下载调用点透传 conflicts；扫描错误显示现有操作失败弹窗。
4. [ ] 更新中英文 `fileTree` 词条、生成字典和 USED_KEYS/一致性计数（如生成器要求）。
5. [ ] 增加短列表、截断列表和剩余计数纯测试。

## Phase D — Remote cwd Fallback

1. [ ] 把远程登录命令改为容错 `cd ... 2>/dev/null; exec $SHELL -l`。
2. [ ] 更新注释和所有期望命令字符串的测试。
3. [ ] 保留并复核 hostile path 单引号转义断言。

## Phase E — Local Non-Compilation Checks

1. [ ] 运行 i18n 生成器。
2. [ ] 再次运行生成器，确认生成文件幂等。
3. [ ] 运行 `git diff --check`。
4. [ ] 静态确认新分支无 `.trellis/` 路径和 dropped commits。
5. [ ] 审阅 reviewer 点名文件，确认无纯格式化噪音。
6. [ ] 不运行任何 Rust 编译、格式化、Clippy 或测试命令。

## Phase F — Commit, Push, and GitHub Actions

1. [ ] 以逻辑边界拆分提交，提交作者为 `vihor3`。
2. [ ] 推送 `feat/remote-file-management-v2` 到 `vihor3/mini-term`。
3. [ ] 监控 GitHub Actions；根据远端日志做针对性修复并重复推送，直到全绿。
4. [ ] 再次获取最新 `origin/main`，确认新分支可合并且 PR diff 无 `.trellis/`。

## Phase G — New PR

1. [ ] 用 `vihor3` 创建新 PR，base 为 `dreamlonglll/mini-term:main`。
2. [ ] PR 描述链接已关闭的 #53，并逐项说明路径逃逸、冲突提示、`.trellis`、格式噪音和 cwd 回退整改。
3. [ ] 附 GitHub Actions 成功链接，明确本地未运行 Rust 编译测试。
4. [ ] 核对 PR 作者、head owner、mergeability、checks 和最终文件清单。

## Deferred Follow-ups

- 设置页 render 副作用生命周期重构。
- 长传输实时字节进度与取消。
- 未接入 UI 的本地 Overwrite 原语清理。
