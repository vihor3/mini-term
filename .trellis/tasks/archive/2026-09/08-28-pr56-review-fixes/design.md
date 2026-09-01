# PR #56 审核意见整改 — Technical Design

## 1. Delivery boundary

产品代码继续在独立 worktree `/tmp/mini-term-file-workbench` 的 `feat/file-workbench-remote-editor` 分支修改。主工作区仅保存 Trellis 规划、研究和本地规范，不把 `.trellis`、`.agents`、journal 或构建产物带入 PR。

本任务不拆子任务：Markdown 清洗、远程图片与图片布局都集中在 `file_viewer.rs`，其余改动共享搜索状态、toast、i18n 生成和同一轮 GitHub Actions 门禁；拆分会造成文件所有权重叠而没有独立交付价值。

## 2. AST-scoped Markdown and HTML sanitization

### 2.1 Unified policy

把现有“Markdown AST 清洗 + 整串 HTML 属性盲扫”收口为一次 AST 遍历。清洗策略明确区分：

- Markdown links：远程/会话均只允许 HTTP(S)、mailto、tel 和 fragment；
- Markdown images：进入 `TextView` 的不可信文本全部禁用；
- raw HTML links：远程文档允许显式网络/mail/tel/fragment 链接，会话正文禁用；
- raw HTML resources (`src` / `poster`)：远程文档和会话正文都禁用自动外部加载。

`collect_untrusted_markdown_replacements` 新增 `MarkdownNode::Html` 分支，只对 `html.value` 调用 HTML URL 清洗器，再通过节点 position byte range 替换。`Code`、`InlineCode`、普通 `Text` 不扫描，因此示例代码保持原字节。

原始 `.html` 文件不是 Markdown，仍由独立 HTML scanner 处理；远程 HTML 的链接可保留，但资源属性按不自动加载策略置为安全占位值。

### 2.2 Plain-label downgrade

`markdown_safe_plain_label` 只在空标签时使用 `link` / `image` fallback。所有非空标签逐字符保留，并对 ASCII 标点加反斜杠，确保降级文本重新解析后不会重建链接、图片或其它 Markdown 结构。

## 3. Deliberate remote image loading

### 3.1 TextView path

远程 Markdown 的 Text/Table cell 在进入 `TextView::markdown` 前禁用全部 image/image-reference 节点。列表、引用和其它复杂容器中的图片显示转义后的 alt 文本，不能触发 `PreviewHttpClient`。

### 3.2 Custom pure-image path

纯顶层图片段落继续由 `MdBlock::Images` 自绘。`FileViewer` 新增每文档、按 URL 的已批准集合：

1. 远程 URL 未批准：不调用 `window.use_asset`，显示带“点击加载”提示的占位；
2. 点击：停止事件冒泡，把 URL 加入批准集合并重绘；
3. 已批准：复用现有 `PreviewHttpClient`、超时、32 MiB 上限和失败后浏览器打开逻辑；
4. 本地文档不经过批准闸，行为不变。

外层图片链接不能抢占“点击加载”的第一次点击；占位处理器必须停止冒泡。

## 4. Search modal lifecycle and result actions

### 4.1 Persistent state

新增懒加载的 `GlobalSearchModal(Entity<SearchModal>)`。覆盖物关闭只隐藏 dialog，不销毁实体，query、结果、总数和进行中的搜索均保留。重新打开复用同一输入实体并重新聚焦。

项目观察逻辑继续以 producing project ID + root 为边界：切换项目时清空旧结果，不能把本地结果解释为另一个项目或 SSH 路径。

### 4.2 Single/double click

恢复纯函数 `result_action(click_count)` 和 pinned tests：

- click 1：立即在工作区打开/激活文件页签，同时启动一个短暂的延迟关闭任务；
- click >=2：取消延迟任务，调用配置的外部编辑器，然后关闭搜索覆盖物；
- 只有单击时，延迟窗口结束后关闭覆盖物并显示已经打开的工作区文件。

延迟是“模态预览迁移到后台工作区”后的必要适配：若 click 1 当场关闭 dialog，click 2 不可能再到达同一行。任务被替换/实体销毁时依靠 GPUI `Task` drop 取消旧延迟。

### 4.3 Remote search feedback

SSH 项目仍不调用本地搜索后端。快捷键入口读取当前项目快照并推送本地化 info toast，说明远程项目暂不支持全文搜索。

## 5. Visible failure feedback

### 5.1 Download identity mismatch

保留项目 ID、根路径、连接 ID 和连接指纹的全部安全校验。任一条件失败或全局文件树不可用时，不启动传输，并通过全局 store 获取项目名称后推送本地化 error toast，提示上下文已变化、需要重新打开或刷新文件。

### 5.2 Dirty worktree cleanup

`AppStore::remove_project` 继续作为最后一道脏文档闸。闸命中时先保留项目，再推送已有 `projectRemovalBlocked` 文案。显式删除路径仍显示 alert；store toast 负责自动 reconcile/prune 和显式检查后的竞态回退。

## 6. Image layout boundary

删除基于 `window.viewport_size()` 的 `preview_avail_width`。860px 只保留为设计最大宽度/图片固有宽度上限；实际布局由 Markdown 内容列的父宽度决定。

图片、占位、图片行及外层链接 wrapper 按需要增加 `max_w_full()` 和 `min_w_0()`。GPUI 的 100% max-width 以真实父元素宽度解析，因此左右面板挤压中栏时图片缩小或 flex-wrap，不再按整窗宽度溢出。

## 7. i18n and close-list decision

新增中英文文案：

- 远程图片点击加载；
- 远程搜索不支持；
- 下载上下文失效。

复用 `fileViewer.projectRemovalBlocked`。通过现有生成器更新 `dict.rs`、`USED_KEYS` 和一致性计数。

`CLOSE_RISK_PREVIEW_LIMIT = 5` 保持不变；这是为了限制 360px 确认框高度，并与冲突列表“前五项 + 剩余数量”一致。可补纯函数测试，并在 PR 审核回复中说明。

## 8. Validation and rollout

本地只执行：

- 静态搜索与人工 diff 审查；
- i18n 生成及幂等复核；
- `git diff --check`；
- `.trellis`、未跟踪产物和无关文件审计。

不运行 `cargo fmt`、Cargo build/check、Clippy 或 Rust tests。提交推送后由 GitHub Actions 完成 changed-rustfmt、generated i18n、Cargo check、sidecar、Clippy、workspace tests 和 whitespace。

CI 全绿后，在 PR #56 审核评论下逐项回复 1–8 的处理结果；PR 正文继续保持简短功能说明。

## 9. Rollback

- Markdown 清洗出现 parser/position 问题：回退 AST HTML 分支，保留旧安全策略并停止交付，不用全文正则补洞。
- 搜索持久化引入生命周期问题：回退为每次新建实体，但不得恢复静默丢失；先以可验证的状态快照方案替代。
- 远程图片批准态异常：fail closed 为纯 alt 文本，不恢复自动网络加载。
- Actions 失败：只按远端日志追加针对性修复，不在本地运行 Rust 编译测试。

## 10. Third review round: remove parser-boundary ambiguity

### 10.1 Untrusted raw HTML

The hand-written URL scanner cannot safely reproduce html5ever tree-builder insertion
modes. Raw-text behavior depends on whether an element was actually inserted, not only
on the token name. Therefore remote Markdown and session Markdown must replace every
`mdast::Html` node with escaped visible source (or an equivalent plain-text form) before
`TextView::markdown`; they must never pass those nodes to `TextView::html`.

Standalone remote `.html` uses the source editor/view instead of rich preview. Trusted
local `.html` keeps `rewrite_html_urls` and preview behavior. The untrusted scanner may
remain only if needed by historical tests/helpers, but it is not a security boundary and
must not be reachable from remote/session rendering.

This preserves Markdown headings, lists, code fences and ordinary links while making the
three reviewed html5ever insertion-mode payloads inert by construction.

### 10.2 Deterministic remote backup lifecycle

At replacement entry, classify deterministic backup state together with target state:

- target regular + backup present: the target is the committed/new file and backup is
  stale cleanup residue; remove the backup, verify it is gone, then continue;
- target missing + backup present: ambiguous crash/recovery state; preserve and refuse;
- uncertain probe/type: preserve and refuse;
- no backup: normal replacement flow.

Never delete recovery data when the target is missing or commit status is uncertain.
Cleanup failure remains actionable and must not silently overwrite the backup.

### 10.3 Remote refresh presentation

Separate fatal initial-load errors from refresh warnings. If an editor/result already
exists, refresh failure updates a visible warning channel and leaves `error`, editor,
draft, selection and focus unchanged. A successful reload clears the refresh warning.
Only a document with no usable loaded content enters the full-page error branch.

### 10.4 Search close focus handoff

The delayed single-click close task captures the producing project identity. After
`close_guarded` succeeds, defer one activation/focus handoff to the current active
workbench document. The workbench/file-viewer identity checks remain authoritative, so a
quick project or tab switch cannot focus a stale file. Double-click continues to cancel
the pending close and opens the external editor.

## 11. Fourth review round: sanitizer fixed point and warning lifecycle

### 11.1 Fixed-point untrusted Markdown sanitization

Escaping one AST generation is insufficient because replacements may change CommonMark
block structure. Split replacement application from collection, then repeatedly parse,
collect, and apply until a parse produces no replacements. Use a small hard limit (four
passes) to bound work. Parser failure or failure to converge must fail closed by rendering
the original document as an indented code block, preserving visibility without producing
active Markdown/HTML nodes.

Regression tests must exercise HTML block types 1–5 followed by four-space-indented image,
link, `file://`, and raw-HTML payloads. The production result is reparsed with the same GFM
options and must yield zero replacements. Existing ordinary-document output remains stable.

### 11.2 Remove misleading production entry points

The hand-written HTML scanner remains usable only underneath trusted local HTML URL
rewriting. Wrapper functions named as untrusted/remote sanitizers are test-only or removed,
so future production code cannot mistake them for a complete security boundary.

### 11.3 Refresh-warning lifecycle

A successful remote save proves the connection and write path recovered, so `finish_save`
clears `refresh_warning` only in its success branch. Error paths preserve the warning and
continue to surface the save error independently.
