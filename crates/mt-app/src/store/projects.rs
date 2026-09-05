//! 项目相关的 `AppStore` 方法:目录技术栈探测、项目 CRUD、项目分组。
//!
//! 从 `store.rs` 原样搬来的三段(`// === 目录技术栈探测 ===` /
//! `// === 项目 ===` / `// === 项目分组 ===`),段注释随代码走,逻辑一行未改。

use std::collections::HashSet;
use std::path::Path;

use gpui::Context;
use mt_config::ProjectConfig;
use mt_identity::WorktreeId;
use mt_ui::icons::ProjectKind;

use crate::project_tree;
use crate::tree::gen_id;

use super::pure::remove_from_tree;
use super::{AppStore, ProjectState};

impl AppStore {
    // === 目录技术栈探测(`useProjectKinds.ts`) ===

    /// 读缓存。`None` = 还没探过;`Some(None)` = 探过但识别不出。
    pub fn dir_kind(&self, path: &str) -> Option<Option<ProjectKind>> {
        self.dir_kinds.get(path).copied()
    }

    /// 批量探测(去重 + 带缓存)。**只接本地路径**,远程由调用方跳过。
    ///
    /// 每条路径一个后台任务:`detect_local` 要读目录、可能还读 `package.json`,
    /// 在主线程上跑会把网络盘/WSL 上的一次悬停做成秒级卡顿。
    pub fn ensure_dir_kinds(&mut self, paths: Vec<String>, cx: &mut Context<Self>) {
        for path in paths {
            if self.dir_kinds.contains_key(&path) || !self.dir_kinds_pending.insert(path.clone()) {
                continue;
            }
            cx.spawn(async move |this, cx| {
                let probe = path.clone();
                let kind = cx
                    .background_executor()
                    .spawn(async move {
                        crate::project_kind::detect_local(std::path::Path::new(&probe))
                    })
                    .await;
                let _ = this.update(cx, |store: &mut AppStore, cx| {
                    store.dir_kinds_pending.remove(&path);
                    store.set_dir_kind(path, kind, cx);
                });
            })
            .detach();
        }
    }

    /// 写缓存并通知(`setDirKind`)。识别不出也要写 —— 否则每帧重探。
    pub fn set_dir_kind(
        &mut self,
        path: String,
        kind: Option<ProjectKind>,
        cx: &mut Context<Self>,
    ) {
        self.dir_kinds.insert(path, kind);
        cx.notify();
    }

    /// 失效(`removeDirKind`):项目根的标记文件变动时调。下一轮 `ensure` 会重探。
    pub fn remove_dir_kind(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.dir_kinds.remove(path).is_some() {
            cx.notify();
        }
    }

    // === 项目 ===

    pub fn set_active_project(&mut self, id: &str, cx: &mut Context<Self>) {
        self.set_active_project_inner(id, true, cx);
    }

    /// Exact live-runtime navigation must not hydrate unrelated dormant panes.
    pub(super) fn set_active_project_without_hydration(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
    ) {
        self.set_active_project_inner(id, false, cx);
    }

    fn set_active_project_state(&mut self, id: &str) -> bool {
        if self.active_project_id.as_deref() == Some(id) {
            return false;
        }
        self.active_project_id = Some(id.to_string());
        self.sync_active_worktree();
        if let Some(state) = self.project_states.get_mut(id) {
            state.needs_attention = false;
        }
        self.config.last_active_project_id = Some(id.to_string());
        true
    }

    fn set_active_project_inner(&mut self, id: &str, hydrate: bool, cx: &mut Context<Self>) {
        if !self.set_active_project_state(id) {
            return;
        }
        // Ordinary project navigation keeps lazy hydration. Exact live-runtime
        // navigation already owns a terminal entity and deliberately skips it.
        if hydrate {
            self.hydrate_project(id, cx);
        }
        self.save_config_soon(cx);
        cx.notify();
    }

    /// 按路径找项目(`store.ts::findProjectByPath`)。
    ///
    /// 比对走 [`normalize_path`](crate::git_worktree::normalize_path):Windows 统一
    /// 分隔符并忽略大小写,POSIX 保留原生分隔符与大小写;两边都去尾斜杠。
    /// SSH 远程项目排除在外 —— worktree 的路径是本机路径。
    pub fn find_project_by_path(&self, path: &str) -> Option<&ProjectConfig> {
        let target = crate::git_worktree::normalize_path(path);
        self.config.projects.iter().find(|p| {
            p.ssh_connection_id.is_none() && crate::git_worktree::normalize_path(&p.path) == target
        })
    }

    /// 添加项目并**返回它的 id**;`parent` 非空时挂成子项目。
    ///
    /// 对应 `store.ts:777-799` 的 `addProject(project, parentProjectId)`:
    /// - 父项目必须真实存在,否则回落为普通顶层项目(防止产生渲染不出来的孤儿);
    /// - **子项目不进 `projectTree`**(移动出去时才转成普通树节点);
    /// - 路径已经是项目 → 返回既有 id,不重复添加(`GitWorktreeModal.tsx:341-351`)。
    ///
    /// 这是 worktree「设为项目」的内部插入路径；统一项目引导使用下面的
    /// `register_or_activate_project` 边界来完成规范路径去重与激活。
    pub fn add_project_at(
        &mut self,
        path: &Path,
        parent: Option<&str>,
        cx: &mut Context<Self>,
    ) -> String {
        let path_str = path.to_string_lossy().to_string();
        if let Some(existing) = self.find_project_by_path(&path_str).map(|p| p.id.clone()) {
            return existing;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path_str.clone());
        let id = gen_id("proj");
        let parent_ok = parent.filter(|pid| self.config.projects.iter().any(|p| p.id == *pid));

        self.config.projects.push(ProjectConfig {
            id: id.clone(),
            name,
            path: path_str,
            description: None,
            saved_layout: None,
            expanded_dirs: Vec::new(),
            ssh_mcp_enabled: false,
            ssh_cli_token: None,
            ssh_connection_ids: None,
            env_vars: Vec::new(),
            wsl_sessions_distro: None,
            ssh_connection_id: None,
            parent_project_id: parent_ok.map(str::to_string),
            kind_override: None,
        });
        if parent_ok.is_none() {
            let tree = self.config.project_tree.get_or_insert_with(Vec::new);
            tree.push(mt_config::ProjectTreeItem::ProjectId(id.clone()));
        }
        self.project_states.insert(id.clone(), ProjectState::new());
        self.expanded_dirs.insert(id.clone(), HashSet::new());
        self.register_project_identity(&id);
        self.save_config_soon(cx);
        cx.notify();
        id
    }

    /// 只回收某个项目的终端,**不删项目**(`projectActions.ts:25-32` 的
    /// `disposeProjectTerminals`)。
    ///
    /// worktree 删除必须先走这一步:Windows 上 shell 占着目录会让
    /// `git worktree remove` 直接失败。
    pub fn dispose_project_terminals(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let pty_ids: Vec<u32> = self
            .project_states
            .get(project_id)
            .map(|s| s.pty_ids())
            .unwrap_or_default();
        for pty_id in pty_ids {
            self.dispose_terminal(pty_id, cx);
        }
        cx.notify();
    }

    /// 改项目显示名(`store.ts::renameProject`)。空名不接受 —— 列表上会变成
    /// 一行只有路径的空条目,而原版的内联重命名框同样在空串时直接放弃。
    pub fn rename_project(&mut self, id: &str, name: &str, cx: &mut Context<Self>) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let Some(project) = self.config.projects.iter_mut().find(|p| p.id == id) else {
            return;
        };
        if project.name == name {
            return;
        }
        project.name = name.to_string();
        self.save_config_soon(cx);
        cx.notify();
    }

    /// 设置项目需求描述;空串 = 清除(`store.ts::setProjectDescription` 的
    /// `description || undefined` 同语义 —— 存空串会让 `skip_serializing_if`
    /// 失效,配置文件里留一堆 `"description": ""`)。
    pub fn set_project_description(&mut self, id: &str, description: &str, cx: &mut Context<Self>) {
        let next = match description.trim() {
            "" => None,
            text => Some(text.to_string()),
        };
        let Some(project) = self.config.projects.iter_mut().find(|p| p.id == id) else {
            return;
        };
        if project.description == next {
            return;
        }
        project.description = next;
        self.save_config_soon(cx);
        cx.notify();
    }

    /// 项目级环境变量(`ProjectEnvVarsModal` 的落盘那一半)。
    ///
    /// **立即落盘**而不是 500ms 防抖:整屏手填的键值对不该在防抖窗口里被一次
    /// 崩溃吃掉(与 SSH 连接同一条理由)。入参已由弹窗清洗过 —— 这里不做校验,
    /// 校验的唯一实现在 `env_vars::compute_errors`(单测钉死)。
    ///
    /// 生效面:只影响**之后新建**的终端(`start_pty` 里读 `env_vars`),
    /// 已有终端不受影响 —— 弹窗底栏那句脚注说的就是这件事。
    pub fn set_project_env_vars(
        &mut self,
        project_id: &str,
        vars: Vec<mt_config::ProjectEnvVar>,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.config.projects.iter_mut().find(|p| p.id == project_id) else {
            return;
        };
        project.env_vars = vars;
        self.save_config_now();
        cx.notify();
    }

    /// 项目类型徽标覆盖:`None` = 自动探测,`Some("none")` = 不显示,
    /// 其余是技术栈 key(直接喂 `TechIcon`)。对应 `ProjectList.tsx` 的
    /// `setProjectKindOverride`(它是「改 config + 立刻落盘」两步)。
    pub fn set_project_kind_override(
        &mut self,
        id: &str,
        kind: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let next = kind.map(|k| k.to_string());
        let Some(project) = self.config.projects.iter_mut().find(|p| p.id == id) else {
            return;
        };
        if project.kind_override == next {
            return;
        }
        project.kind_override = next;
        self.save_config_soon(cx);
        cx.notify();
    }

    /// 移除项目注册: hosted pane 只 detach；legacy pane 按旧语义随实体回收。
    pub fn remove_project(&mut self, id: &str, cx: &mut Context<Self>) {
        if crate::workbench_area::project_has_dirty_documents(id, cx) {
            let project_name = self
                .project(id)
                .map(|project| project.name.clone())
                .unwrap_or_else(|| id.to_string());
            crate::toast::push_message_deduped(
                crate::notify::ToastKind::PasteError,
                id.to_string(),
                project_name,
                crate::i18n::t("fileViewer", "projectRemovalBlocked").to_string(),
                cx,
            );
            return;
        }
        let pty_ids: Vec<u32> = self
            .project_states
            .get(id)
            .map(|s| s.pty_ids())
            .unwrap_or_default();
        for pty_id in pty_ids {
            self.detach_terminal(pty_id, cx);
        }

        // Flush a departing alias only when it owns the latest pending shared
        // snapshot. Older or ownerless aliases must not overwrite that row.
        let worktree_is_shared = self.project_has_other_worktree_alias(id);
        self.flush_project_layout_before_removal(id, worktree_is_shared, cx);
        self.project_states.remove(id);
        self.expanded_dirs.remove(id);
        self.done.retain_panes(&self.live_pane_ids());
        // 它的 toast 一并撤掉(`store.ts:859`)—— 留着的话点下去会跳向一个
        // 已经不存在的项目
        crate::toast::remove_project(id, cx);
        self.config.projects.retain(|p| p.id != id);
        if let Some(tree) = self.config.project_tree.as_mut() {
            remove_from_tree(tree, id);
        }
        if self.active_project_id.as_deref() == Some(id) {
            self.active_project_id = self.config.projects.first().map(|p| p.id.clone());
            self.config.last_active_project_id = self.active_project_id.clone();
        }
        self.remove_remote_runtime_project(id);
        self.remove_project_identity(id);
        self.save_config_soon(cx);
        cx.notify();
    }

    // === 项目分组(`store.ts:1266-1313` 的五个 action) ===

    /// `ensureTree`(`store.ts:611-617`)的 Rust 版:第一次碰分组时把
    /// `projectTree` 补齐,免得后面的树操作全落进 `None` 里静默失效。
    ///
    /// **旧格式迁移不在这里**:`projectGroups`/`projectOrdering` → `projectTree`
    /// 已经由 `mt_config::migrate_config` 在读盘时做过一遍(config.rs:646-676),
    /// 这里只补「压根没有过分组」的那一档。
    ///
    /// ⚠️ 与 TS 的一处有意偏差:铺初值时**跳过 worktree 子项目**。那边是
    /// `projects.map(p => p.id)` 一个不落,但「子项目不进 projectTree」是两侧
    /// 共同的不变量(见 [`Self::add_project_at`]),把它们塞进去会让
    /// `get_ordered_tree` 同时按树序和父项目序各排一次。
    fn ensure_tree(&mut self) {
        if self
            .config
            .project_tree
            .as_ref()
            .is_some_and(|tree| !tree.is_empty())
        {
            return;
        }
        let ids: Vec<mt_config::ProjectTreeItem> = self
            .config
            .projects
            .iter()
            .filter(|p| p.parent_project_id.is_none())
            .map(|p| mt_config::ProjectTreeItem::ProjectId(p.id.clone()))
            .collect();
        self.config.project_tree = Some(ids);
    }

    /// 新建分组。`parent_group_id` 为 `None` = 建在顶层,**一律追加到末尾**。
    ///
    /// 父组找不到时 `insert_into_tree` 返回 false,原版就此**静默丢弃**
    /// (`store.ts:1266-1273` 不看返回值)—— 这里照抄:能走到这一步说明右键菜单
    /// 拿的是刚渲染过的组 id,丢弃比"悄悄建到顶层"更容易暴露真正的 bug。
    pub fn create_group(
        &mut self,
        name: &str,
        parent_group_id: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        self.ensure_tree();
        let group = mt_config::ProjectGroup {
            id: gen_id("group"),
            name: name.to_string(),
            collapsed: false,
            children: Vec::new(),
        };
        let tree = self.config.project_tree.get_or_insert_with(Vec::new);
        project_tree::insert_into_tree(
            tree,
            parent_group_id,
            mt_config::ProjectTreeItem::Group(group),
            None,
        );
        self.save_config_soon(cx);
        cx.notify();
    }

    /// 删分组。**组员(含子组)原位晋升到父级,一个都不删** —— 与原版
    /// `removeGroupAndPromoteChildren` 同语义,所以确认框那句"会移到上一级"是真的。
    pub fn remove_group(&mut self, group_id: &str, cx: &mut Context<Self>) {
        let Some(tree) = self.config.project_tree.as_mut() else {
            return;
        };
        if !project_tree::remove_group_and_promote_children(tree, group_id) {
            return;
        }
        self.save_config_soon(cx);
        cx.notify();
    }

    /// 改分组名。空名不接受(调用方那边也 `trim` 过一道,两处都拦)。
    pub fn rename_group(&mut self, group_id: &str, name: &str, cx: &mut Context<Self>) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let Some(tree) = self.config.project_tree.as_mut() else {
            return;
        };
        let Some(group) = project_tree::find_group_in_tree_mut(tree, group_id) else {
            return;
        };
        if group.name == name {
            return;
        }
        group.name = name.to_string();
        self.save_config_soon(cx);
        cx.notify();
    }

    /// 折叠 / 展开。**只影响侧栏渲染**:移动端快照那条路
    /// (`mobile_relay::ordered_projects`)刻意不跳过折叠组,折不折叠由手机自己决定。
    pub fn toggle_group_collapse(&mut self, group_id: &str, cx: &mut Context<Self>) {
        let Some(tree) = self.config.project_tree.as_mut() else {
            return;
        };
        let Some(group) = project_tree::find_group_in_tree_mut(tree, group_id) else {
            return;
        };
        group.collapsed = !group.collapsed;
        self.save_config_soon(cx);
        cx.notify();
    }

    /// 把节点(项目或分组)移到 `target_group_id` 里的 `index` 位置。
    /// `target_group_id = None` = 根层;`index = None` = 追加到末尾。
    ///
    /// 全部边界语义(自环先验 / worktree 子项目脱离父项目 / 树外孤儿收编 /
    /// 目标组被并发删掉的兜底)在 [`project_tree::move_item_in_tree`],
    /// 这里只是搬运 + save/notify。
    ///
    /// 返回值 = 这次有没有真的动过树。
    pub fn move_item(
        &mut self,
        item_id: &str,
        target_group_id: Option<&str>,
        index: Option<usize>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.ensure_tree();
        let tree = self.config.project_tree.get_or_insert_with(Vec::new);
        if !project_tree::move_item_in_tree(
            tree,
            &mut self.config.projects,
            item_id,
            target_group_id,
            index,
        ) {
            return false;
        }
        self.save_config_soon(cx);
        cx.notify();
        true
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ProjectLocationKey {
    Local {
        normalized_canonical_path: String,
    },
    Ssh {
        connection_id: String,
        normalized_posix_path: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectRegistrationDisposition {
    RegisteredNew,
    ActivatedExisting,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectRegistrationOutcome {
    pub project_id: String,
    pub worktree_id: WorktreeId,
    pub disposition: ProjectRegistrationDisposition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectPlacement<'a> {
    TopLevel { target_group: Option<&'a str> },
    ChildWorktree { root_project_id: &'a str },
}

impl AppStore {
    pub fn register_or_activate_project(
        &mut self,
        location: ProjectLocationKey,
        canonical_path: &str,
        suggested_name: Option<&str>,
        target_group: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Result<ProjectRegistrationOutcome, String> {
        self.register_or_activate_project_with_placement(
            location,
            canonical_path,
            suggested_name,
            ProjectPlacement::TopLevel { target_group },
            cx,
        )
    }

    pub fn register_or_activate_project_with_placement(
        &mut self,
        location: ProjectLocationKey,
        canonical_path: &str,
        suggested_name: Option<&str>,
        placement: ProjectPlacement<'_>,
        cx: &mut Context<Self>,
    ) -> Result<ProjectRegistrationOutcome, String> {
        self.register_or_activate_project_placed_inner(
            location,
            canonical_path,
            suggested_name,
            placement,
            Some(cx),
        )
    }

    /// `activation_cx` is always present in production. Tests may omit it to
    /// exercise the registration transaction without starting a GPUI runtime.
    fn register_or_activate_project_inner(
        &mut self,
        location: ProjectLocationKey,
        canonical_path: &str,
        suggested_name: Option<&str>,
        target_group: Option<&str>,
        activation_cx: Option<&mut Context<Self>>,
    ) -> Result<ProjectRegistrationOutcome, String> {
        self.register_or_activate_project_placed_inner(
            location,
            canonical_path,
            suggested_name,
            ProjectPlacement::TopLevel { target_group },
            activation_cx,
        )
    }

    fn register_or_activate_project_placed_inner(
        &mut self,
        location: ProjectLocationKey,
        canonical_path: &str,
        suggested_name: Option<&str>,
        placement: ProjectPlacement<'_>,
        activation_cx: Option<&mut Context<Self>>,
    ) -> Result<ProjectRegistrationOutcome, String> {
        validate_registration_location(&location, canonical_path)?;
        let (requested_group, parent_project_id) = match placement {
            ProjectPlacement::TopLevel { target_group } => {
                let requested_group = match target_group {
                    Some(group_id) => {
                        let group = self
                            .config
                            .project_tree
                            .as_deref()
                            .and_then(|tree| {
                                crate::project_tree::find_group_in_tree(tree, group_id)
                            })
                            .ok_or_else(|| {
                                format!("target project group no longer exists: {group_id}")
                            })?;
                        Some((group_id.to_string(), group.collapsed))
                    }
                    None => None,
                };
                (requested_group, None)
            }
            ProjectPlacement::ChildWorktree { root_project_id } => {
                validate_child_worktree_placement(
                    &self.config.projects,
                    root_project_id,
                    &location,
                    canonical_path,
                )?;
                (None, Some(root_project_id.to_string()))
            }
        };
        let existing_id = self
            .config
            .projects
            .iter()
            .find(|project| {
                project_matches_location(
                    project,
                    self.onboarding_canonical_path_for_project(project),
                    &location,
                )
            })
            .map(|project| project.id.clone());
        if let Some(project_id) = existing_id {
            if self.worktree_id_for_project(&project_id).is_none() {
                let project = self.project(&project_id).cloned().ok_or_else(|| {
                    "existing project disappeared during registration".to_string()
                })?;
                let prepared = self.prepare_project_identity(&project)?;
                self.install_prepared_project_identity(&project_id, prepared);
            }
            let worktree_id = self
                .worktree_id_for_project(&project_id)
                .cloned()
                .ok_or_else(|| "existing project has no worktree identity".to_string())?;
            if let Some(cx) = activation_cx {
                self.set_active_project(&project_id, cx);
            } else {
                self.set_active_project_state(&project_id);
            }
            return Ok(ProjectRegistrationOutcome {
                project_id,
                worktree_id,
                disposition: ProjectRegistrationDisposition::ActivatedExisting,
            });
        }

        let id = gen_id("proj");
        let ssh_connection_id = match &location {
            ProjectLocationKey::Local { .. } => None,
            ProjectLocationKey::Ssh { connection_id, .. } => {
                if !self
                    .config
                    .ssh_connections
                    .iter()
                    .any(|connection| connection.id == *connection_id)
                {
                    return Err(format!("SSH connection {connection_id} is unavailable"));
                }
                Some(connection_id.clone())
            }
        };
        let name = suggested_name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| project_name_from_path(canonical_path));
        let project = ProjectConfig {
            id: id.clone(),
            name,
            path: canonical_path.to_string(),
            description: None,
            saved_layout: None,
            expanded_dirs: Vec::new(),
            ssh_mcp_enabled: false,
            ssh_cli_token: None,
            ssh_connection_ids: None,
            env_vars: Vec::new(),
            wsl_sessions_distro: None,
            ssh_connection_id,
            parent_project_id: parent_project_id.clone(),
            kind_override: None,
        };
        let prepared = self.prepare_project_identity(&project)?;

        if parent_project_id.is_none() {
            self.ensure_tree();
            if let Some((group_id, true)) = requested_group.as_ref()
                && let Some(tree) = self.config.project_tree.as_mut()
                && let Some(group) = crate::project_tree::find_group_in_tree_mut(tree, group_id)
            {
                group.collapsed = false;
            }
        }
        self.config.projects.push(project);
        if parent_project_id.is_none() {
            let tree = self.config.project_tree.get_or_insert_with(Vec::new);
            crate::project_tree::insert_into_tree(
                tree,
                requested_group
                    .as_ref()
                    .map(|(group_id, _)| group_id.as_str()),
                mt_config::ProjectTreeItem::ProjectId(id.clone()),
                None,
            );
        }
        self.project_states.insert(id.clone(), ProjectState::new());
        self.expanded_dirs.insert(id.clone(), HashSet::new());
        let worktree_id = self.install_prepared_project_identity(&id, prepared);
        if let Some(cx) = activation_cx {
            self.set_active_project(&id, cx);
        } else {
            self.set_active_project_state(&id);
        }
        self.save_config_now();
        Ok(ProjectRegistrationOutcome {
            project_id: id,
            worktree_id,
            disposition: ProjectRegistrationDisposition::RegisteredNew,
        })
    }
}

fn validate_registration_location(
    location: &ProjectLocationKey,
    canonical_path: &str,
) -> Result<(), String> {
    if canonical_path.is_empty() || canonical_path.contains('\0') {
        return Err("canonical project path is invalid".into());
    }
    match location {
        ProjectLocationKey::Local {
            normalized_canonical_path,
        } => {
            if !Path::new(canonical_path).is_absolute()
                && mt_core::parse_wsl_unc(&canonical_path.replace('/', "\\")).is_none()
            {
                return Err("local canonical project path must be absolute".into());
            }
            let expected =
                crate::execution_host::normalize_host_visible_project_path(canonical_path)?;
            if expected != *normalized_canonical_path {
                return Err("local project location key does not match its canonical path".into());
            }
        }
        ProjectLocationKey::Ssh {
            connection_id,
            normalized_posix_path,
        } => {
            if connection_id.is_empty() || connection_id.contains('\0') {
                return Err("SSH project connection identity is invalid".into());
            }
            if normalize_registration_posix(canonical_path)? != *normalized_posix_path {
                return Err("SSH project location key does not match its canonical path".into());
            }
        }
    }
    Ok(())
}

fn project_matches_location(
    project: &ProjectConfig,
    canonical_binding_path: Option<&str>,
    location: &ProjectLocationKey,
) -> bool {
    match location {
        ProjectLocationKey::Local {
            normalized_canonical_path,
        } => {
            project.ssh_connection_id.is_none()
                && [Some(project.path.as_str()), canonical_binding_path]
                    .into_iter()
                    .flatten()
                    .any(|path| {
                        crate::execution_host::normalize_host_visible_project_path(path)
                            .ok()
                            .as_deref()
                            == Some(normalized_canonical_path.as_str())
                    })
        }
        ProjectLocationKey::Ssh {
            connection_id,
            normalized_posix_path,
        } => {
            project.ssh_connection_id.as_deref() == Some(connection_id.as_str())
                && [Some(project.path.as_str()), canonical_binding_path]
                    .into_iter()
                    .flatten()
                    .any(|path| {
                        normalize_registration_posix(path).ok().as_deref()
                            == Some(normalized_posix_path.as_str())
                    })
        }
    }
}

fn normalize_registration_posix(path: &str) -> Result<String, String> {
    crate::execution_host::normalize_absolute_posix_path(path)
}

fn validate_child_worktree_placement(
    projects: &[ProjectConfig],
    root_project_id: &str,
    location: &ProjectLocationKey,
    canonical_path: &str,
) -> Result<(), String> {
    let root = projects
        .iter()
        .find(|project| project.id == root_project_id)
        .ok_or_else(|| format!("root project no longer exists: {root_project_id}"))?;
    if root.parent_project_id.is_some() {
        return Err("discovered worktree parent must be a top-level project".into());
    }

    match location {
        ProjectLocationKey::Ssh { connection_id, .. } => {
            if root.ssh_connection_id.as_deref() != Some(connection_id.as_str()) {
                return Err(
                    "discovered SSH worktree does not belong to the root connection".into(),
                );
            }
            if mt_core::parse_wsl_unc(&canonical_path.replace('/', "\\")).is_some() {
                return Err("discovered SSH worktree path cannot be a WSL path".into());
            }
        }
        ProjectLocationKey::Local { .. } => {
            if root.ssh_connection_id.is_some() {
                return Err("discovered local worktree does not belong to the SSH root".into());
            }
            let root_wsl = mt_core::parse_wsl_unc(&root.path.replace('/', "\\"));
            let child_wsl = mt_core::parse_wsl_unc(&canonical_path.replace('/', "\\"));
            match (root_wsl, child_wsl) {
                (Some(root_wsl), Some(child_wsl))
                    if root_wsl.distro.eq_ignore_ascii_case(&child_wsl.distro) => {}
                (Some(_), Some(_)) => {
                    return Err("discovered WSL worktree belongs to another distribution".into());
                }
                (Some(_), None) | (None, Some(_)) => {
                    return Err("discovered worktree host does not match the root project".into());
                }
                (None, None) => {}
            }
        }
    }
    Ok(())
}

fn project_name_from_path(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_string()
}

#[cfg(test)]
mod project_onboarding_tests {
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use std::sync::Arc;

    use mt_config::{AppConfig, ProjectGroup, ProjectTreeItem, SshConnection};
    use mt_identity::{ExecutionHostId, HostInstallId, RepoId};
    use mt_layout::ProjectWorktreeBinding;
    use mt_project::worktree::WorktreeIdentitySource;

    use super::*;

    fn project(id: &str, path: &str, ssh_connection_id: Option<&str>) -> ProjectConfig {
        ProjectConfig {
            id: id.into(),
            name: id.into(),
            path: path.into(),
            description: None,
            saved_layout: None,
            expanded_dirs: Vec::new(),
            ssh_mcp_enabled: false,
            ssh_cli_token: None,
            ssh_connection_ids: None,
            env_vars: Vec::new(),
            wsl_sessions_distro: None,
            ssh_connection_id: ssh_connection_id.map(str::to_string),
            parent_project_id: None,
            kind_override: None,
        }
    }

    fn host_install_id() -> HostInstallId {
        "install-v1:123e4567-e89b-42d3-a456-426614174000"
            .parse()
            .unwrap()
    }

    fn ssh_connection(id: &str) -> SshConnection {
        SshConnection {
            id: id.into(),
            name: format!("display-{id}"),
            host: "host.example".into(),
            port: 22,
            user: "deploy".into(),
            password: None,
            identity_file: None,
            group: None,
        }
    }

    fn authoritative_remote_binding(
        project_id: &str,
        configured_path: &str,
        canonical_path: &str,
        connection: &SshConnection,
    ) -> ProjectWorktreeBinding {
        let remote_install: HostInstallId = "install-v1:123e4567-e89b-42d3-a456-426614174001"
            .parse()
            .unwrap();
        let execution_host_id = ExecutionHostId::derive("SHA256:verified-host", &remote_install);
        let repo_id = RepoId::derive(&execution_host_id, &format!("{canonical_path}/.git"));
        let identity_context = serde_json::to_string(&(
            "ssh-authority-v2",
            connection.id.as_str(),
            connection.host.as_str(),
            connection.port,
            connection.user.as_str(),
            configured_path,
        ))
        .unwrap();
        ProjectWorktreeBinding {
            project_id: project_id.into(),
            execution_host_id,
            repo_id: repo_id.clone(),
            worktree_id: WorktreeId::derive(&repo_id, canonical_path, None),
            identity_source: WorktreeIdentitySource::AuthoritativeRemoteGit
                .as_str()
                .into(),
            canonical_worktree_path: Some(canonical_path.into()),
            identity_context: Some(identity_context),
        }
    }

    fn test_store(
        config: AppConfig,
        project_worktree_bindings: HashMap<String, ProjectWorktreeBinding>,
        ai: crate::ai::AiBridge,
    ) -> AppStore {
        let active_project_id = config.last_active_project_id.clone();
        let active_worktree_id = active_project_id
            .as_deref()
            .and_then(|project_id| project_worktree_bindings.get(project_id))
            .map(|binding| binding.worktree_id.clone());
        let project_states = config
            .projects
            .iter()
            .map(|project| (project.id.clone(), ProjectState::new()))
            .collect();
        let expanded_dirs = config
            .projects
            .iter()
            .map(|project| (project.id.clone(), HashSet::new()))
            .collect();
        let config_store = Arc::new(mt_config::ConfigStore::at(
            std::env::temp_dir()
                .join("mt-app-project-registration-test")
                .join("config.json"),
        ));
        let config_writer = crate::store::config_writer::ConfigWriter::spawn(config_store.clone());

        AppStore {
            config,
            token: 0,
            config_store,
            config_writer,
            layout_store: None,
            host_install_id: host_install_id(),
            project_worktree_bindings,
            active_worktree_id,
            remote_runtime_projects: HashMap::new(),
            next_remote_runtime_generation: 0,
            remote_agent_polls: HashMap::new(),
            next_remote_agent_generation: 0,
            window_geometry: None,
            terminals_panel_visible: true,
            layout_dirty_projects: HashSet::new(),
            layout_dirty_worktree_owners: HashMap::new(),
            layout_globals_dirty: false,
            layout_save_generation: 0,
            _layout_save_task: None,
            active_project_id,
            project_states,
            terminals: HashMap::new(),
            terminal_host: None,
            terminal_routes: HashMap::new(),
            agent_runtime: Default::default(),
            agent_feed_acknowledged: HashMap::new(),
            pane_subs: HashMap::new(),
            focused_pane_id: None,
            mobile_relay_status: None,
            markers_by_pty: HashMap::new(),
            marker_cursor: HashMap::new(),
            pending_forks: HashMap::new(),
            next_pty_id: 1,
            ai,
            terminal_theme: Default::default(),
            background_art: None,
            expanded_dirs,
            dir_kinds: HashMap::new(),
            dir_kinds_pending: HashSet::new(),
            exited_ptys: HashSet::new(),
            done: Default::default(),
            window_focused: true,
            save_generation: 0,
            _save_task: None,
        }
    }

    fn local_location(path: &str) -> ProjectLocationKey {
        ProjectLocationKey::Local {
            normalized_canonical_path: crate::execution_host::normalize_host_visible_project_path(
                path,
            )
            .unwrap(),
        }
    }

    fn project_occurrences(items: &[ProjectTreeItem], project_id: &str) -> usize {
        items
            .iter()
            .map(|item| match item {
                ProjectTreeItem::ProjectId(id) => {
                    if id == project_id {
                        1
                    } else {
                        0
                    }
                }
                ProjectTreeItem::Group(group) => project_occurrences(&group.children, project_id),
            })
            .sum()
    }

    fn tree_project_count(items: &[ProjectTreeItem]) -> usize {
        items
            .iter()
            .map(|item| match item {
                ProjectTreeItem::ProjectId(_) => 1,
                ProjectTreeItem::Group(group) => tree_project_count(&group.children),
            })
            .sum()
    }

    #[test]
    fn project_onboarding_local_locator_matches_only_local_projects() {
        let canonical = if cfg!(windows) {
            r"C:\Users\leo\repo"
        } else {
            "/home/leo/repo"
        };
        let location =
            ProjectLocationKey::Local {
                normalized_canonical_path:
                    crate::execution_host::normalize_host_visible_project_path(canonical).unwrap(),
            };

        assert!(project_matches_location(
            &project("local", &format!("{canonical}/"), None),
            None,
            &location,
        ));
        assert!(!project_matches_location(
            &project("remote", canonical, Some("ssh-a")),
            None,
            &location,
        ));
        assert!(project_matches_location(
            &project("local-alias", "/configured/alias", None),
            Some(canonical),
            &location,
        ));
        assert!(validate_registration_location(&location, canonical).is_ok());

        let relative = "relative/repo";
        assert!(
            validate_registration_location(
                &ProjectLocationKey::Local {
                    normalized_canonical_path:
                        crate::execution_host::normalize_host_visible_project_path(relative)
                            .unwrap(),
                },
                relative,
            )
            .is_err()
        );
    }

    #[test]
    fn project_onboarding_ssh_locator_requires_connection_and_exact_posix_case() {
        let location = ProjectLocationKey::Ssh {
            connection_id: "ssh-a".into(),
            normalized_posix_path: "/home/leo/Repo".into(),
        };

        assert!(project_matches_location(
            &project("same", "/home/leo/./Repo/", Some("ssh-a")),
            None,
            &location,
        ));
        assert!(!project_matches_location(
            &project("other-host", "/home/leo/Repo", Some("ssh-b")),
            None,
            &location,
        ));
        assert!(!project_matches_location(
            &project("other-case", "/home/leo/repo", Some("ssh-a")),
            None,
            &location,
        ));
        assert!(validate_registration_location(&location, "/home/leo/Repo").is_ok());
    }

    #[test]
    fn project_onboarding_ssh_path_normalization_fails_closed() {
        assert_eq!(
            normalize_registration_posix("/home//leo/./repo/").unwrap(),
            "/home/leo/repo"
        );
        assert_eq!(
            normalize_registration_posix(r"/home/leo/repo\name").unwrap(),
            r"/home/leo/repo\name"
        );
        assert!(normalize_registration_posix("relative/repo").is_err());
        assert!(normalize_registration_posix("/home/leo/../secret").is_err());
        assert!(normalize_registration_posix("/home/leo/\0repo").is_err());
    }

    #[test]
    fn project_registration_dedupes_an_authoritative_local_canonical_binding() {
        let test_root = std::env::temp_dir().join(format!(
            "mt-app-project-registration-authoritative-alias-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&test_root);
        let repository_path = test_root.join("repo");
        fs::create_dir_all(&repository_path).unwrap();
        let init = std::process::Command::new("git")
            .arg("init")
            .arg("--quiet")
            .arg(&repository_path)
            .output()
            .unwrap();
        assert!(
            init.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );
        let configured_path = repository_path.join(".");
        let resolved =
            mt_project::worktree::resolve_local(&host_install_id(), &configured_path).unwrap();
        assert_eq!(
            resolved.source,
            WorktreeIdentitySource::AuthoritativeLocalGit
        );
        let canonical_path = resolved.canonical_worktree_path.clone();
        let configured_path = configured_path.to_string_lossy().to_string();
        assert_ne!(
            mt_project::worktree::normalize_path_for_comparison(&configured_path),
            mt_project::worktree::normalize_path_for_comparison(&canonical_path)
        );
        let alias = project("local-alias", &configured_path, None);
        let binding = super::super::identity::binding_from_resolved(alias.id.clone(), resolved);
        let config = AppConfig {
            projects: vec![alias.clone()],
            project_tree: Some(vec![ProjectTreeItem::ProjectId(alias.id.clone())]),
            ..AppConfig::default()
        };
        let mut bindings = HashMap::new();
        bindings.insert(alias.id.clone(), binding.clone());
        let (ai, _events) = crate::ai::AiBridge::new(false);
        let mut store = test_store(config, bindings, ai);

        assert_eq!(
            store.onboarding_canonical_path_for_project(&alias),
            binding.canonical_worktree_path.as_deref()
        );
        assert_eq!(
            store.trusted_canonical_worktree_path_for_project(&alias.id),
            binding.canonical_worktree_path.as_deref()
        );
        let outcome = store
            .register_or_activate_project_inner(
                local_location(&canonical_path),
                &canonical_path,
                Some("Ignored duplicate name"),
                None,
                None,
            )
            .unwrap();

        assert_eq!(
            outcome.disposition,
            ProjectRegistrationDisposition::ActivatedExisting
        );
        assert_eq!(outcome.project_id, alias.id);
        assert_eq!(outcome.worktree_id, binding.worktree_id);
        assert_eq!(store.config.projects.len(), 1);
        assert_eq!(
            tree_project_count(store.config.project_tree.as_deref().unwrap()),
            1
        );
        drop(store);
        fs::remove_dir_all(test_root).unwrap();
    }

    #[test]
    fn onboarding_dedupe_rejects_stale_provisional_local_and_wsl_bindings() {
        let (configured_local_path, stale_local_path) = if cfg!(windows) {
            (r"C:\configured\repo", r"C:\stale\repo")
        } else {
            ("/configured/repo", "/stale/repo")
        };
        let configured_wsl_path = r"\\wsl$\Ubuntu\home\leo\configured";
        let stale_wsl_path = r"\\wsl$\Ubuntu\home\leo\stale";
        let local_project = project("local-provisional", configured_local_path, None);
        let wsl_project = project("wsl-provisional", configured_wsl_path, None);
        let local_binding = super::super::identity::binding_from_resolved(
            local_project.id.clone(),
            mt_project::worktree::resolve_provisional_local(&host_install_id(), stale_local_path)
                .unwrap(),
        );
        let wsl_binding = super::super::identity::binding_from_resolved(
            wsl_project.id.clone(),
            mt_project::worktree::resolve_provisional_wsl(
                &host_install_id(),
                "Ubuntu",
                stale_wsl_path,
            )
            .unwrap(),
        );
        let config = AppConfig {
            projects: vec![local_project.clone(), wsl_project.clone()],
            ..AppConfig::default()
        };
        let bindings = HashMap::from([
            (local_project.id.clone(), local_binding),
            (wsl_project.id.clone(), wsl_binding),
        ]);
        let (ai, _events) = crate::ai::AiBridge::new(false);
        let store = test_store(config, bindings, ai);

        assert_eq!(
            store.onboarding_canonical_path_for_project(&local_project),
            None
        );
        assert_eq!(
            store.onboarding_canonical_path_for_project(&wsl_project),
            None
        );
        assert_eq!(
            store.trusted_canonical_worktree_path_for_project(&local_project.id),
            None
        );
        assert_eq!(
            store.trusted_canonical_worktree_path_for_project(&wsl_project.id),
            None
        );
    }

    #[test]
    fn project_registration_transaction_places_dedupes_and_returns_exact_identity() {
        let (ai, _events) = crate::ai::AiBridge::new(false);
        let test_root = std::env::temp_dir().join(format!(
            "mt-app-project-registration-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&test_root);
        let first_path = test_root.join("alpha");
        let second_path = test_root.join("beta");
        fs::create_dir_all(&first_path).unwrap();
        fs::create_dir_all(&second_path).unwrap();
        let first_path = first_path.to_string_lossy().to_string();
        let second_path = second_path.to_string_lossy().to_string();

        // Fixture SSH identities must remain independent of GPUI and remote runtime shutdown.
        {
            let config = AppConfig {
                project_tree: Some(vec![ProjectTreeItem::Group(ProjectGroup {
                    id: "target".into(),
                    name: "Target".into(),
                    collapsed: true,
                    children: Vec::new(),
                })]),
                ..AppConfig::default()
            };
            let mut store = test_store(config, HashMap::new(), ai);

            {
                let first_location = local_location(&first_path);
                let stale_group_error = store
                    .register_or_activate_project_inner(
                        first_location.clone(),
                        &first_path,
                        Some("Alpha"),
                        Some("missing"),
                        None,
                    )
                    .unwrap_err();
                assert!(stale_group_error.contains("target project group no longer exists"));
                assert!(store.config.projects.is_empty());
                assert!(store.project_worktree_bindings.is_empty());
                assert!(
                    crate::project_tree::find_group_in_tree(
                        store.config.project_tree.as_deref().unwrap(),
                        "target",
                    )
                    .unwrap()
                    .children
                    .is_empty()
                );

                let first = store
                    .register_or_activate_project_inner(
                        first_location.clone(),
                        &first_path,
                        Some("  Alpha  "),
                        Some("target"),
                        None,
                    )
                    .unwrap();
                assert_eq!(
                    first.disposition,
                    ProjectRegistrationDisposition::RegisteredNew
                );
                assert_eq!(
                    store.worktree_id_for_project(&first.project_id),
                    Some(&first.worktree_id)
                );
                assert_eq!(store.active_worktree_id(), Some(&first.worktree_id));
                assert_eq!(
                    store.active_project_id.as_deref(),
                    Some(first.project_id.as_str())
                );
                assert_eq!(
                    store.config.last_active_project_id.as_deref(),
                    Some(first.project_id.as_str())
                );
                assert_eq!(
                    store
                        .config
                        .projects
                        .iter()
                        .filter(|project| project_matches_location(project, None, &first_location))
                        .count(),
                    1
                );
                assert_eq!(store.project(&first.project_id).unwrap().name, "Alpha");
                assert!(store.project_states.contains_key(&first.project_id));
                assert!(store.expanded_dirs.contains_key(&first.project_id));
                let tree = store.config.project_tree.as_deref().unwrap();
                let target = crate::project_tree::find_group_in_tree(tree, "target").unwrap();
                assert!(!target.collapsed);
                assert_eq!(store.config.projects.len(), 1);
                assert_eq!(tree_project_count(tree), 1);
                assert_eq!(project_occurrences(tree, &first.project_id), 1);
                assert_eq!(project_occurrences(&target.children, &first.project_id), 1);

                let second = store
                    .register_or_activate_project_inner(
                        local_location(&second_path),
                        &second_path,
                        None,
                        None,
                        None,
                    )
                    .unwrap();
                assert_eq!(
                    store.active_project_id.as_deref(),
                    Some(second.project_id.as_str())
                );
                let projects_before_duplicate = store.config.projects.len();
                let tree_records_before_duplicate =
                    tree_project_count(store.config.project_tree.as_deref().unwrap());

                let duplicate = store
                    .register_or_activate_project_inner(
                        first_location.clone(),
                        &first_path,
                        Some("Ignored duplicate name"),
                        None,
                        None,
                    )
                    .unwrap();
                assert_eq!(
                    duplicate.disposition,
                    ProjectRegistrationDisposition::ActivatedExisting
                );
                assert_eq!(duplicate.project_id, first.project_id);
                assert_eq!(duplicate.worktree_id, first.worktree_id);
                assert_eq!(store.config.projects.len(), projects_before_duplicate);
                assert_eq!(
                    tree_project_count(store.config.project_tree.as_deref().unwrap()),
                    tree_records_before_duplicate
                );
                assert_eq!(
                    project_occurrences(
                        store.config.project_tree.as_deref().unwrap(),
                        &first.project_id,
                    ),
                    1
                );
                assert_eq!(
                    store
                        .config
                        .projects
                        .iter()
                        .filter(|project| project_matches_location(project, None, &first_location))
                        .count(),
                    1
                );
                assert_eq!(
                    store.active_project_id.as_deref(),
                    Some(first.project_id.as_str())
                );
                assert_eq!(store.active_worktree_id(), Some(&first.worktree_id));

                let projects_before_ssh = store.config.projects.len();
                let tree_records_before_ssh =
                    tree_project_count(store.config.project_tree.as_deref().unwrap());
                let bindings_before_ssh = store.project_worktree_bindings.len();
                let ssh_error = store
                    .register_or_activate_project_inner(
                        ProjectLocationKey::Ssh {
                            connection_id: "missing-ssh".into(),
                            normalized_posix_path: "/srv/repo".into(),
                        },
                        "/srv/repo",
                        None,
                        None,
                        None,
                    )
                    .unwrap_err();
                assert!(ssh_error.contains("SSH connection missing-ssh is unavailable"));
                assert_eq!(store.config.projects.len(), projects_before_ssh);
                assert_eq!(
                    tree_project_count(store.config.project_tree.as_deref().unwrap()),
                    tree_records_before_ssh
                );
                assert_eq!(store.project_worktree_bindings.len(), bindings_before_ssh);

                let connection = ssh_connection("ssh-a");
                store.config.ssh_connections.push(connection.clone());
                let remote_location = ProjectLocationKey::Ssh {
                    connection_id: connection.id.clone(),
                    normalized_posix_path: "/srv/new-repo".into(),
                };
                let remote = store
                    .register_or_activate_project_inner(
                        remote_location.clone(),
                        "/srv/new-repo",
                        Some("Remote"),
                        None,
                        None,
                    )
                    .unwrap();
                assert_eq!(
                    remote.disposition,
                    ProjectRegistrationDisposition::RegisteredNew
                );
                assert_eq!(
                    store
                        .project(&remote.project_id)
                        .and_then(|project| project.ssh_connection_id.as_deref()),
                    Some(connection.id.as_str())
                );
                let remote_project_count = store.config.projects.len();
                let remote_tree_count =
                    tree_project_count(store.config.project_tree.as_deref().unwrap());
                let remote_duplicate = store
                    .register_or_activate_project_inner(
                        remote_location,
                        "/srv/./new-repo/",
                        Some("Ignored"),
                        None,
                        None,
                    )
                    .unwrap();
                assert_eq!(
                    remote_duplicate.disposition,
                    ProjectRegistrationDisposition::ActivatedExisting
                );
                assert_eq!(remote_duplicate.project_id, remote.project_id);
                assert_eq!(remote_duplicate.worktree_id, remote.worktree_id);
                assert_eq!(store.config.projects.len(), remote_project_count);
                assert_eq!(
                    tree_project_count(store.config.project_tree.as_deref().unwrap()),
                    remote_tree_count
                );

                let configured_path = "/srv/repo-link";
                let canonical_path = "/srv/repo-real";
                let remote_project = project(
                    "ssh-authoritative",
                    configured_path,
                    Some(connection.id.as_str()),
                );
                store.config.projects.push(remote_project.clone());
                store
                    .config
                    .project_tree
                    .as_mut()
                    .unwrap()
                    .push(ProjectTreeItem::ProjectId(remote_project.id.clone()));
                store
                    .project_states
                    .insert(remote_project.id.clone(), ProjectState::new());
                store
                    .expanded_dirs
                    .insert(remote_project.id.clone(), HashSet::new());
                let authoritative = authoritative_remote_binding(
                    &remote_project.id,
                    configured_path,
                    canonical_path,
                    &connection,
                );
                let authoritative_worktree_id = authoritative.worktree_id.clone();
                let authoritative_context = authoritative.identity_context.clone();
                store
                    .project_worktree_bindings
                    .insert(remote_project.id.clone(), authoritative);

                let prepared = store.prepare_project_identity(&remote_project).unwrap();
                let installed =
                    store.install_prepared_project_identity(&remote_project.id, prepared);
                assert_eq!(installed, authoritative_worktree_id);
                let preserved = store
                    .project_worktree_bindings
                    .get(&remote_project.id)
                    .unwrap();
                assert_eq!(preserved.worktree_id, authoritative_worktree_id);
                assert_eq!(
                    preserved.identity_source,
                    WorktreeIdentitySource::AuthoritativeRemoteGit.as_str()
                );
                assert_eq!(
                    preserved.canonical_worktree_path.as_deref(),
                    Some(canonical_path)
                );
                assert_eq!(preserved.identity_context, authoritative_context);
                assert_eq!(
                    store.onboarding_canonical_path_for_project(&remote_project),
                    Some(canonical_path)
                );
                assert_eq!(
                    store.trusted_canonical_worktree_path_for_project(&remote_project.id),
                    Some(canonical_path)
                );

                let projects_before_alias_duplicate = store.config.projects.len();
                let tree_records_before_alias_duplicate =
                    tree_project_count(store.config.project_tree.as_deref().unwrap());
                let alias_duplicate = store
                    .register_or_activate_project_inner(
                        ProjectLocationKey::Ssh {
                            connection_id: connection.id.clone(),
                            normalized_posix_path: canonical_path.into(),
                        },
                        canonical_path,
                        Some("Ignored canonical alias"),
                        None,
                        None,
                    )
                    .unwrap();
                assert_eq!(
                    alias_duplicate.disposition,
                    ProjectRegistrationDisposition::ActivatedExisting
                );
                assert_eq!(alias_duplicate.project_id, remote_project.id);
                assert_eq!(alias_duplicate.worktree_id, authoritative_worktree_id);
                assert_eq!(store.config.projects.len(), projects_before_alias_duplicate);
                assert_eq!(
                    tree_project_count(store.config.project_tree.as_deref().unwrap()),
                    tree_records_before_alias_duplicate
                );
            }

            drop(store);
        }
        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn child_local_registration_dedupes_and_preserves_top_level_aliases() {
        let test_root = std::env::temp_dir().join(format!(
            "mt-app-child-local-registration-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&test_root);
        let root_path = test_root.join("root");
        let child_path = test_root.join("child");
        let alias_path = test_root.join("top-level-alias");
        fs::create_dir_all(&root_path).unwrap();
        fs::create_dir_all(&child_path).unwrap();
        fs::create_dir_all(&alias_path).unwrap();
        let root_path = root_path.to_string_lossy().to_string();
        let child_path = child_path.to_string_lossy().to_string();
        let alias_path = alias_path.to_string_lossy().to_string();
        let root = project("root", &root_path, None);
        let alias = project("alias", &alias_path, None);
        let config = AppConfig {
            projects: vec![root.clone(), alias.clone()],
            project_tree: Some(vec![
                ProjectTreeItem::ProjectId(root.id.clone()),
                ProjectTreeItem::ProjectId(alias.id.clone()),
            ]),
            ..AppConfig::default()
        };
        let (ai, _events) = crate::ai::AiBridge::new(false);
        let mut store = test_store(config, HashMap::new(), ai);

        let child = store
            .register_or_activate_project_placed_inner(
                local_location(&child_path),
                &child_path,
                Some("Feature"),
                ProjectPlacement::ChildWorktree {
                    root_project_id: &root.id,
                },
                None,
            )
            .unwrap();
        assert_eq!(
            child.disposition,
            ProjectRegistrationDisposition::RegisteredNew
        );
        assert_eq!(
            store
                .project(&child.project_id)
                .and_then(|project| project.parent_project_id.as_deref()),
            Some(root.id.as_str())
        );
        assert_eq!(
            tree_project_count(store.config.project_tree.as_deref().unwrap()),
            2
        );

        let repeated = store
            .register_or_activate_project_placed_inner(
                local_location(&child_path),
                &child_path,
                Some("Ignored"),
                ProjectPlacement::ChildWorktree {
                    root_project_id: &root.id,
                },
                None,
            )
            .unwrap();
        assert_eq!(
            repeated.disposition,
            ProjectRegistrationDisposition::ActivatedExisting
        );
        assert_eq!(repeated.project_id, child.project_id);
        assert_eq!(repeated.worktree_id, child.worktree_id);

        let top_level_alias = store
            .register_or_activate_project_placed_inner(
                local_location(&alias_path),
                &alias_path,
                Some("Ignored"),
                ProjectPlacement::ChildWorktree {
                    root_project_id: &root.id,
                },
                None,
            )
            .unwrap();
        assert_eq!(top_level_alias.project_id, alias.id);
        assert_eq!(
            store
                .project(&top_level_alias.project_id)
                .unwrap()
                .parent_project_id,
            None
        );
        assert_eq!(
            project_occurrences(
                store.config.project_tree.as_deref().unwrap(),
                &top_level_alias.project_id,
            ),
            1
        );

        let nested_path = test_root.join("nested").to_string_lossy().to_string();
        let nested_parent_error = store
            .register_or_activate_project_placed_inner(
                local_location(&nested_path),
                &nested_path,
                None,
                ProjectPlacement::ChildWorktree {
                    root_project_id: &child.project_id,
                },
                None,
            )
            .unwrap_err();
        assert!(nested_parent_error.contains("top-level project"));
        drop(store);
        fs::remove_dir_all(test_root).unwrap();
    }

    #[test]
    fn child_wsl_registration_dedupes_unc_aliases_and_fences_distro() {
        let root = project("wsl-root", r"\\wsl$\Ubuntu\home\leo\repo", None);
        let config = AppConfig {
            projects: vec![root.clone()],
            project_tree: Some(vec![ProjectTreeItem::ProjectId(root.id.clone())]),
            ..AppConfig::default()
        };
        let (ai, _events) = crate::ai::AiBridge::new(false);
        let mut store = test_store(config, HashMap::new(), ai);
        let child_path = r"\\wsl$\Ubuntu\home\leo\repo-feature";

        let child = store
            .register_or_activate_project_placed_inner(
                local_location(child_path),
                child_path,
                Some("Feature"),
                ProjectPlacement::ChildWorktree {
                    root_project_id: &root.id,
                },
                None,
            )
            .unwrap();
        assert_eq!(
            store
                .project(&child.project_id)
                .and_then(|project| project.parent_project_id.as_deref()),
            Some(root.id.as_str())
        );
        assert_eq!(
            tree_project_count(store.config.project_tree.as_deref().unwrap()),
            1
        );

        let alias_path = r"\\wsl.localhost\ubuntu\home\leo\repo-feature";
        let duplicate = store
            .register_or_activate_project_placed_inner(
                local_location(alias_path),
                alias_path,
                None,
                ProjectPlacement::ChildWorktree {
                    root_project_id: &root.id,
                },
                None,
            )
            .unwrap();
        assert_eq!(duplicate.project_id, child.project_id);
        assert_eq!(
            duplicate.disposition,
            ProjectRegistrationDisposition::ActivatedExisting
        );

        let other_distro = r"\\wsl$\Debian\home\leo\repo-feature";
        let error = store
            .register_or_activate_project_placed_inner(
                local_location(other_distro),
                other_distro,
                None,
                ProjectPlacement::ChildWorktree {
                    root_project_id: &root.id,
                },
                None,
            )
            .unwrap_err();
        assert!(error.contains("another distribution"));
    }

    #[test]
    fn child_ssh_registration_requires_the_root_connection() {
        let connection = ssh_connection("ssh-a");
        let other_connection = ssh_connection("ssh-b");
        let root = project("ssh-root", "/srv/repo", Some(connection.id.as_str()));
        let config = AppConfig {
            projects: vec![root.clone()],
            project_tree: Some(vec![ProjectTreeItem::ProjectId(root.id.clone())]),
            ssh_connections: vec![connection.clone(), other_connection.clone()],
            ..AppConfig::default()
        };
        let (ai, _events) = crate::ai::AiBridge::new(false);
        let mut store = test_store(config, HashMap::new(), ai);
        let location = ProjectLocationKey::Ssh {
            connection_id: connection.id.clone(),
            normalized_posix_path: "/srv/repo-feature".into(),
        };

        let child = store
            .register_or_activate_project_placed_inner(
                location.clone(),
                "/srv/repo-feature",
                Some("Feature"),
                ProjectPlacement::ChildWorktree {
                    root_project_id: &root.id,
                },
                None,
            )
            .unwrap();
        assert_eq!(
            store
                .project(&child.project_id)
                .and_then(|project| project.parent_project_id.as_deref()),
            Some(root.id.as_str())
        );
        assert_eq!(
            store
                .project(&child.project_id)
                .and_then(|project| project.ssh_connection_id.as_deref()),
            Some(connection.id.as_str())
        );
        assert_eq!(
            tree_project_count(store.config.project_tree.as_deref().unwrap()),
            1
        );

        let duplicate = store
            .register_or_activate_project_placed_inner(
                location,
                "/srv/./repo-feature/",
                None,
                ProjectPlacement::ChildWorktree {
                    root_project_id: &root.id,
                },
                None,
            )
            .unwrap();
        assert_eq!(duplicate.project_id, child.project_id);

        let wrong_connection_error = store
            .register_or_activate_project_placed_inner(
                ProjectLocationKey::Ssh {
                    connection_id: other_connection.id.clone(),
                    normalized_posix_path: "/srv/other".into(),
                },
                "/srv/other",
                None,
                ProjectPlacement::ChildWorktree {
                    root_project_id: &root.id,
                },
                None,
            )
            .unwrap_err();
        assert!(wrong_connection_error.contains("root connection"));

        let local_child_path = if cfg!(windows) {
            r"C:\repo-feature"
        } else {
            "/tmp/repo-feature"
        };
        let local_error = validate_child_worktree_placement(
            store.projects(),
            &root.id,
            &local_location(local_child_path),
            local_child_path,
        )
        .unwrap_err();
        assert!(local_error.contains("SSH root"));
    }
}
