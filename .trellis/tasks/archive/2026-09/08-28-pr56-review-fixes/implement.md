# PR #56 审核意见整改 — Implementation Plan

## Preconditions

- [ ] 用户批准本 PRD、design 和 implement 最终摘要。
- [ ] 运行 `task.py start`，任务进入 `in_progress`。
- [ ] 使用 `trellis-before-dev` 重新加载 mt-app、mt-i18n 与共享规范。
- [ ] 确认产品 worktree 干净且 HEAD 与 fork 分支一致；主工作区现有脏文件全部保留。

## Phase A — Markdown/HTML sanitization

1. [ ] 把 raw HTML link/resource 权限拆成明确策略，不再用一个 `allow_external` 同时控制两者。
2. [ ] 在 Markdown AST replacement collector 中处理 `MarkdownNode::Html`，只替换真实 raw HTML 节点。
3. [ ] 删除 remote/session Markdown 的全文 HTML 属性盲扫；保留原始 `.html` 文件入口的 scanner。
4. [ ] 简化 `markdown_safe_plain_label`：空值 fallback，非空值完整保留并转义 ASCII 标点。
5. [ ] 更新/新增纯测试：fenced/inline code 原样、raw HTML 清洗、标点标签保留、二次 AST 无不安全活动节点。

Review gate：任何 HTML 属性修改都必须能追溯到 `mdast::Html.position()`；不得扫描 Markdown 全文。

## Phase B — Remote image consent and layout

1. [ ] 远程 Text/Table sanitizer 禁用全部 Markdown image/image-reference 节点。
2. [ ] 给 `FileViewer` 增加每文档 URL 批准集合和占位点击动作；未批准时不得调用 URI asset loader。
3. [ ] 点击占位停止冒泡、写入批准集合并重绘；批准后复用现有加载/失败处理。
4. [ ] 远程 raw HTML 禁止自动 external `src` / `poster`，链接策略保持显式可点。
5. [ ] 删除 viewport 宽度推导；图片/占位/行/wrapper 增加父宽度 clamp。
6. [ ] 增加纯决策测试并静态审查所有远程图片入口，确认没有第二条自动请求路径。

Rollback point：若点击加载接线不稳定，远程图片全部 fail closed 为 alt 文本，不恢复自动加载。

## Phase C — Search behavior

1. [ ] 建立懒加载全局 `SearchModal` 实体，overlay 关闭后保留 query/results/search task。
2. [ ] 保持 project ID + root 绑定；项目变化取消并清空旧搜索。
3. [ ] 恢复 `ResultAction` 与单/双/三击纯函数测试。
4. [ ] 单击立即打开工作区页签并延迟关闭 overlay；双击取消延迟、打开外部编辑器并关闭 overlay。
5. [ ] SSH 项目快捷键入口推送不支持搜索的本地化 info toast，且不创建搜索任务。

Review gate：单击与双击都不能把旧搜索结果绑定到新项目；关闭/重开 overlay 不得丢 query/results。

## Phase D — Failure feedback and close-list documentation

1. [ ] 下载 helper 的所有身份/全局上下文 early return 改为 fail closed + 本地化 error toast。
2. [ ] `AppStore::remove_project` 脏文档闸命中时推送阻止原因；显式 alert 维持不变。
3. [ ] 为五项关窗预览 + 剩余数量补纯函数测试或确认现有覆盖；代码行为不改。
4. [ ] 更新 search/fileTree/fileViewer 所需双语源、`USED_KEYS`、生成字典和一致性计数。

## Phase E — Static quality review

1. [ ] 使用 `trellis-check` 子代理做 spec、数据流、复用、竞态和测试覆盖审查。
2. [ ] 运行 i18n 生成器并再次运行确认幂等。
3. [ ] 运行 `git diff --check`。
4. [ ] 对 `origin/main...HEAD` 审计文件清单、二进制、未跟踪/忽略产物和 `.trellis`。
5. [ ] 不运行任何本地 Rust 格式化、编译、Clippy 或测试命令。

## Phase F — Commit, push, Actions, and PR reply

1. [ ] 以相关逻辑边界提交，作者保持 `vihor3 <vihor3@gmail.com>`；不包含 `.trellis`。
2. [ ] 推送 `feat/file-workbench-remote-editor` 到 `vihor3/mini-term`。
3. [ ] 监控 fork 与 PR #56 GitHub Actions；只根据远端日志做针对性修复，直到全绿。
4. [ ] 再次 fetch `origin/main`，确认 0 behind、PR mergeable/CLEAN、工作区无杂项。
5. [ ] 在维护者审核评论下用中文逐项回复 1–8；第 8 项说明保留前五条 + 剩余数量是有意的可读性策略。
6. [ ] 核对 PR 正文仍只描述功能，没有塞入冗长审核过程。

## Phase G — Third review security and recovery fixes

1. [ ] 远程/会话 Markdown 的 `MarkdownNode::Html` 全部降级为可见但惰性的源码文本；不得调用 URL 扫描器后继续作为 raw HTML 渲染。
2. [ ] 远程 `.html` 默认走源码编辑/查看分支，本地 `.html` 预览保持不变；删除或断开远程 HTML 富文本渲染入口。
3. [ ] 用审核方三个 html5ever 插入模式载荷补回归测试，验证 raw HTML 不形成活动节点且 fenced/inline code 原样。
4. [ ] 远程替换入口联合判断 target/backup：目标存在时清理陈旧确定性备份后继续，目标缺失或状态不明时继续 fail closed。
5. [ ] 为陈旧备份可恢复保存、目标缺失保留恢复数据、清理失败拒绝继续补纯逻辑/协议测试。

Security gate：不可信 HTML 的安全性不得依赖手写 tokenizer 与 html5ever 解析结果一致。

## Phase H — Refresh and focus fixes

1. [ ] `refresh_remote` 失败且已有 editor/result 时只设置非阻断警告，保留内容、草稿、焦点和 `error=None`；首次加载失败保留错误页。
2. [ ] 成功刷新/重载时清除刷新警告；保存警告与刷新警告不得互相误清除或掩盖关键状态。
3. [ ] 搜索单击延迟关闭成功后重新激活当前工作区文档，复用现有项目/文档身份检查；双击取消路径不补焦点。
4. [ ] 补状态决策与点击/关闭焦点纯测试，静态审计异步回调不会聚焦新项目或新页签。
5. [ ] 重跑 Phase E、F；本地仍禁止 Rust/Cargo/rustfmt，最终以 GitHub Actions 和审核方 Windows 复验为准。

## Phase I — Fourth review fixed-point follow-up

1. [x] 从 `sanitize_untrusted_markdown` 抽出纯 `apply_markdown_replacements`，保留既有排序、去重与逆序替换语义。
2. [x] 清洗函数最多循环四轮“GFM 解析 → 收集替换 → 应用替换”，无替换即返回不动点结果；解析失败或超限时把原始输入整体降级为缩进代码块。
3. [x] 补 HTML block type 1–5 + 四空格缩进图片/链接/`file://`/raw HTML 载荷，使用生产清洗函数并再次解析，断言没有活动节点。
4. [x] 将 `sanitize_untrusted_html_urls` / `sanitize_remote_html_urls` 删除或限制为 `#[cfg(test)]`，确认生产本地 HTML 预览仍只复用可信改写底层。
5. [x] 在远程保存成功分支清除 `refresh_warning`，补成功清除与失败保留的状态决策测试。
6. [x] 由 `trellis-check` 独立复核安全不动点、fallback 可见性、生产命名空间和 warning 生命周期；不得运行本地 Rust/Cargo/rustfmt。
7. [x] 运行 i18n 生成幂等检查、`git diff --check` 和文件范围审计；提交推送后仅由 GitHub Actions 执行 Rust 门禁。
8. [x] Actions 全绿后用中文回复最新审核，明确远程 HTML 源码模式属于安全取舍，并如实说明搜索焦点路径仍需真机交互确认。
