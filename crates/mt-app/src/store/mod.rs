//! 全局状态。对应 `src/store.ts` 的那一份 zustand store。
//!
//! # 形状
//!
//! ```text
//! AppStore
//!  ├─ config: AppConfig            ← mt-config 加载/保存(带写盘令牌)
//!  ├─ active_project_id
//!  ├─ project_states: {projectId → ProjectState{ layout: Option<SplitNode>, status }}
//!  ├─ terminals:      {ptyId → Entity<TerminalPane>}   ← 旧版的 terminalCache
//!  ├─ focused_pane_id                                   ← 旧版靠 DOM 焦点推,这里显式记
//!  └─ ai: AiBridge                                      ← hook / monitor / 输入输出旁路
//! ```
//!
//! store 本身是一个 gpui `Entity`,放在 `Global` 里给所有视图取用;视图通过
//! `cx.observe(&store)` 订阅变化 —— 等价于 zustand 的 `useAppStore(selector)`,
//! 只是粒度粗一档(整棵重画,终端内容不受影响:那一层在 `TerminalPane` 自己的
//! entity 上,不随 store 的 notify 重跑)。
//!
//! # 文件布局(纯拆分,逻辑一行未改)
//!
//! ```text
//! store/
//!  ├─ mod.rs      结构体定义 / 字段 / 装配(`new`)/ 全局取用 / 只读访问
//!  ├─ projects.rs 目录技术栈探测、项目 CRUD、项目分组
//!  ├─ panes.rs    终端与分屏、pane 拖拽移动、双击最大化、PTY 起停
//!  ├─ ai.rs       AI 任务标记(⚑)、AI 事件、通知/待办、会话分支自记账
//!  ├─ prefs.rs    面板视图 / 用量 / 主题 / 各类配置 / 感知 / 语言 / 重命名 / 中转
//!  ├─ ssh.rs      SSH 连接表、远程项目、「关联 SSH」、断线重连
//!  ├─ layout.rs   项目级终端面板、三栏与抽屉、文件树展开、布局与配置落盘
//!  └─ pure.rs     无 `self` 的纯函数与它们的类型,连同全部单测
//! ```
//!
//! `AppStore` 的字段一律私有,子模块靠「私有项对后代模块可见」直接读写 ——
//! 拆分没有放宽任何字段的可见性。

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use gpui::{App, Context, Entity, Global, Subscription, Task};
use mt_ai::AgentRuntimeRegistry;
use mt_config::{AppConfig, ConfigStore, ProjectConfig};
use mt_identity::{AgentEventId, AgentRunId, HostInstallId, PaneKey, WorktreeId};
use mt_layout::ProjectWorktreeBinding;
use mt_relay::MobileRelayStatusPayload;
use mt_terminal_host::{TerminalHostClient, terminal_host_enabled};
use mt_ui::TerminalTheme;
use mt_ui::icons::ProjectKind;
use mt_ui::theme_bridge::BackgroundArt;

use crate::ai::AiBridge;
use crate::markers::AiMarker;
use crate::notify::{AlertPlan, DoneTracker};
use crate::pane::TerminalPane;
use crate::persist;
use crate::tree::{PaneState, PaneStatus, ProjectPanel, SplitNode};

mod ai;
mod config_writer;
mod context;
mod identity;
mod layout;
mod panes;
mod prefs;
mod projects;
mod pure;
mod remote_agents;
mod remote_runtime;
mod ssh;

use config_writer::ConfigWriter;

pub use context::{
    AgentTargetView, TerminalDiagnosticView, TerminalJumpTarget, TerminalJumpView,
    orca_worktree_context_enabled,
};
pub use projects::{ProjectLocationKey, ProjectPlacement, ProjectRegistrationOutcome};

// 纯函数与它们的类型原本就住在 store.rs 顶层;拆进 `pure` 后原样再导出,
// `crate::store::Xxx` 这条对外路径一字不变(全仓其它文件零改动的前提)。
pub use pure::*;
pub use remote_agents::{
    REMOTE_AGENT_POLL_INTERVAL, RemoteAgentPollState, RemoteAgentProbeCapability,
};
pub use remote_runtime::RemoteRuntimeProjectState;

/// 单个项目的运行时状态(对应 `types.ts` 的 `ProjectState`)。
pub struct ProjectState {
    /// Legacy route owners and complete saved trees. Presentation is flat;
    /// background terminal entities and PTYs remain independently owned.
    pub panels: Vec<ProjectPanel>,
    /// 活动面板 id。列表非空时恒有效([`Self::active_panel`] 兜底取第一个)。
    pub active_panel_id: Option<String>,
    /// One visible terminal; legacy panels remain its immutable routing owners.
    pub selected_terminal_pane_key: Option<PaneKey>,
    pub terminal_order: Vec<PaneKey>,
    /// 由**全部面板**聚合出的项目级状态(error > ai-working > ai-idle > idle)。
    pub status: PaneStatus,
    /// 非激活项目里有 AI 任务完成 —— 项目行上的提示点。
    pub needs_attention: bool,
    /// 双击最大化的 pane:终端区只渲染它所在的那个叶子。
    ///
    /// **纯运行时,不落盘**(`types.ts::ProjectState.maximizedPaneId` 同样不进
    /// `savedLayout`,`persist.rs` 里一个字都不该出现它)。语义是「哪个 pane 被
    /// 铺满了」而不是「哪个叶子」—— 同组内切 tab 仍然保持最大化,与原版一致。
    /// 只对活动面板有意义,切面板时清掉。
    pub maximized_pane_id: Option<String>,
}

impl ProjectState {
    fn new() -> Self {
        Self {
            panels: Vec::new(),
            active_panel_id: None,
            selected_terminal_pane_key: None,
            terminal_order: Vec::new(),
            status: PaneStatus::Idle,
            needs_attention: false,
            maximized_pane_id: None,
        }
    }

    // ── 面板访问 ──────────────────────────────────────────────
    //
    // 调用侧的两类语义在这里分流:围绕**看得见的那棵树**的操作
    // (渲染/切 tab/分屏/最大化)走 `active_*`;按 id / pty 找 pane 的操作
    // (状态回报/改名/移动端写入/关闭)跨全部面板 —— pane id 与 pty id
    // 全局唯一,漏了后台面板就是「后台 AI 的状态灯永远不亮」这类静默 bug。

    /// 活动面板。`active_panel_id` 失配/缺失时兜底取第一个 —— 恢复期/关面板的
    /// 中间态不该让整个终端区渲染成空白。
    pub fn active_panel(&self) -> Option<&ProjectPanel> {
        self.active_panel_id
            .as_deref()
            .and_then(|id| self.panels.iter().find(|p| p.id == id))
            .or_else(|| self.panels.first())
    }

    pub fn active_layout(&self) -> Option<&SplitNode> {
        self.active_panel().map(|p| &p.layout)
    }

    pub fn active_layout_mut(&mut self) -> Option<&mut SplitNode> {
        let id = self.active_panel()?.id.clone();
        self.panels
            .iter_mut()
            .find(|p| p.id == id)
            .map(|p| &mut p.layout)
    }

    /// 活动面板在列表里的下标(落盘的 `activeTabIndex`)。
    pub fn active_panel_index(&self) -> usize {
        self.active_panel()
            .and_then(|active| self.panels.iter().position(|p| p.id == active.id))
            .unwrap_or(0)
    }

    pub fn panel_mut(&mut self, panel_id: &str) -> Option<&mut ProjectPanel> {
        self.panels.iter_mut().find(|p| p.id == panel_id)
    }

    /// 全部面板的树,面板序。
    pub fn layouts(&self) -> impl Iterator<Item = &SplitNode> {
        self.panels.iter().map(|p| &p.layout)
    }

    pub fn layouts_mut(&mut self) -> impl Iterator<Item = &mut SplitNode> {
        self.panels.iter_mut().map(|p| &mut p.layout)
    }

    /// 持有该 pane 的面板 id(跨全部面板)。
    pub fn panel_id_of_pane(&self, pane_id: &str) -> Option<&str> {
        self.panels
            .iter()
            .find(|p| p.layout.pane(pane_id).is_some())
            .map(|p| p.id.as_str())
    }

    pub fn layout_of_pane(&self, pane_id: &str) -> Option<&SplitNode> {
        self.layouts().find(|l| l.pane(pane_id).is_some())
    }

    pub fn layout_of_pane_mut(&mut self, pane_id: &str) -> Option<&mut SplitNode> {
        self.panels
            .iter_mut()
            .map(|p| &mut p.layout)
            .find(|l| l.pane(pane_id).is_some())
    }

    /// 按 pane id 找(跨全部面板)。
    pub fn pane(&self, pane_id: &str) -> Option<&PaneState> {
        self.layouts().find_map(|l| l.pane(pane_id))
    }

    pub fn pane_mut(&mut self, pane_id: &str) -> Option<&mut PaneState> {
        self.panels
            .iter_mut()
            .find_map(|p| p.layout.pane_mut(pane_id))
    }

    pub fn pane_by_pty_mut(&mut self, pty_id: u32) -> Option<&mut PaneState> {
        self.panels
            .iter_mut()
            .find_map(|p| p.layout.pane_by_pty_mut(pty_id))
    }

    /// 全部面板的全部 pane,面板序 × 树内 DFS 序。
    pub fn all_panes(&self) -> Vec<&PaneState> {
        self.layouts().flat_map(|l| l.panes()).collect()
    }

    pub fn ordered_terminal_panes(&self) -> Vec<&PaneState> {
        let mut panes = self.all_panes();
        let mut order = HashMap::new();
        for (index, key) in self.terminal_order.iter().enumerate() {
            order.entry(key).or_insert(index);
        }
        panes.sort_by_key(|pane| order.get(&pane.pane_key).copied().unwrap_or(usize::MAX));
        panes
    }

    pub fn selected_terminal(&self) -> Option<&PaneState> {
        self.selected_terminal_pane_key
            .as_ref()
            .and_then(|key| self.pane(key.as_str()))
            .or_else(|| self.active_layout().and_then(SplitNode::first_active_pane))
            .or_else(|| self.all_panes().first().copied())
    }

    pub fn select_terminal(&mut self, pane_id: &str) -> bool {
        let Some(panel) = self
            .panels
            .iter_mut()
            .find(|panel| panel.layout.pane(pane_id).is_some())
        else {
            return false;
        };
        self.selected_terminal_pane_key =
            panel.layout.pane(pane_id).map(|pane| pane.pane_key.clone());
        panel.layout.activate_pane(pane_id);
        self.active_panel_id = Some(panel.id.clone());
        true
    }

    pub fn normalize_terminal_navigation(&mut self, focused: Option<&str>) {
        let keys = self
            .all_panes()
            .iter()
            .map(|pane| pane.pane_key.clone())
            .collect::<Vec<_>>();
        let inventory = keys.iter().cloned().collect::<HashSet<_>>();
        let mut seen = HashSet::new();
        self.terminal_order
            .retain(|key| inventory.contains(key) && seen.insert(key.clone()));
        self.terminal_order
            .extend(keys.into_iter().filter(|key| seen.insert(key.clone())));
        let selected = self
            .selected_terminal_pane_key
            .as_ref()
            .and_then(|key| self.pane(key.as_str()))
            .or_else(|| focused.and_then(|id| self.pane(id)))
            .or_else(|| self.selected_terminal())
            .map(|pane| pane.id.clone());
        if let Some(selected) = selected {
            self.select_terminal(&selected);
        } else {
            self.selected_terminal_pane_key = None;
        }
    }

    pub fn restore_terminal_navigation(&mut self, saved: &mt_config::SavedProjectLayout) {
        self.selected_terminal_pane_key = saved.selected_terminal_pane_key.clone();
        self.terminal_order = saved.terminal_order.clone().unwrap_or_default();
        self.normalize_terminal_navigation(None);
    }

    pub(super) fn cycle_terminal_target(&self, from: &str, delta: i32) -> Option<String> {
        let panes = self.ordered_terminal_panes();
        let index = panes.iter().position(|pane| pane.id == from)?;
        let index = (index as i64 + i64::from(delta)).rem_euclid(panes.len() as i64) as usize;
        Some(panes[index].id.clone())
    }

    pub(super) fn terminal_at_index(&self, index: usize) -> Option<&PaneState> {
        self.ordered_terminal_panes()
            .get(index.checked_sub(1)?)
            .copied()
    }

    pub(super) fn reorder_terminal(
        &mut self,
        source: &PaneKey,
        target: &PaneKey,
        after: bool,
    ) -> bool {
        if source == target
            || self.pane(source.as_str()).is_none()
            || self.pane(target.as_str()).is_none()
        {
            return false;
        }
        self.normalize_terminal_navigation(None);
        let previous = self.terminal_order.clone();
        self.terminal_order.retain(|key| key != source);
        let index = self
            .terminal_order
            .iter()
            .position(|key| key == target)
            .expect("target was validated");
        self.terminal_order
            .insert(index + usize::from(after), source.clone());
        self.terminal_order != previous
    }

    pub(super) fn saved_layout(&self) -> mt_config::SavedProjectLayout {
        let mut saved = persist::serialize_layout(&self.panels, self.active_panel_index());
        saved.selected_terminal_pane_key =
            self.selected_terminal().map(|pane| pane.pane_key.clone());
        saved.terminal_order = Some(
            self.ordered_terminal_panes()
                .iter()
                .map(|pane| pane.pane_key.clone())
                .collect(),
        );
        saved
    }

    pub(super) fn append_background_terminal(
        &mut self,
        tab_id: mt_identity::TabId,
        pane: PaneState,
    ) -> bool {
        if self.panels.is_empty() {
            let panel = ProjectPanel::with_tab_id(tab_id, SplitNode::leaf(pane));
            self.active_panel_id = Some(panel.id.clone());
            self.panels.push(panel);
        } else {
            let Some(panel) = self.panels.iter_mut().find(|panel| panel.tab_id == tab_id) else {
                return false;
            };
            let previous = panel.layout.first_active_pane().map(|pane| pane.id.clone());
            if !panel.layout.append_pane(None, pane) {
                return false;
            }
            if let Some(previous) = previous {
                panel.layout.activate_pane(&previous);
            }
        }
        self.normalize_terminal_navigation(None);
        true
    }

    pub fn pty_ids(&self) -> Vec<u32> {
        self.layouts().flat_map(|l| l.pty_ids()).collect()
    }

    /// 跨全部面板的聚合状态。
    pub fn highest_status(&self) -> PaneStatus {
        self.layouts().fold(PaneStatus::Idle, |acc, l| {
            let s = l.highest_status();
            if s.priority() > acc.priority() {
                s
            } else {
                acc
            }
        })
    }

    /// 按节点 id 找叶子/split(跨全部面板;节点 id 全局唯一)。
    pub fn node(&self, node_id: &str) -> Option<&SplitNode> {
        self.layouts().find_map(|l| l.node(node_id))
    }

    pub fn node_mut(&mut self, node_id: &str) -> Option<&mut SplitNode> {
        self.panels
            .iter_mut()
            .find_map(|p| p.layout.node_mut(node_id))
    }

    /// 持有该 pane 的叶子(跨全部面板)。
    pub fn leaf_of_pane(&self, pane_id: &str) -> Option<&SplitNode> {
        self.layouts().find_map(|l| l.leaf_of_pane(pane_id))
    }

    /// 把 pane 从它所在的面板里摘掉;面板随最后一个 pane 一起消失,
    /// 活动指针挪到邻位(原下标处的右邻,没有则末位)。
    pub fn remove_pane(&mut self, pane_id: &str) {
        let order = self
            .ordered_terminal_panes()
            .iter()
            .map(|pane| pane.id.clone())
            .collect::<Vec<_>>();
        let selected = self.selected_terminal().map(|pane| pane.id.clone());
        let neighbor = order.iter().position(|id| id == pane_id).and_then(|index| {
            order
                .get(index + 1)
                .or_else(|| index.checked_sub(1).and_then(|index| order.get(index)))
                .cloned()
        });
        let Some(idx) = self
            .panels
            .iter()
            .position(|p| p.layout.pane(pane_id).is_some())
        else {
            return;
        };
        let ProjectPanel {
            id,
            tab_id,
            custom_title,
            layout,
        } = self.panels.remove(idx);
        match layout.remove_pane(pane_id) {
            Some(layout) => self.panels.insert(
                idx,
                ProjectPanel {
                    id,
                    tab_id,
                    custom_title,
                    layout,
                },
            ),
            None => {
                if self.active_panel_id.as_deref() == Some(id.as_str()) {
                    self.active_panel_id = self
                        .panels
                        .get(idx)
                        .or_else(|| self.panels.last())
                        .map(|p| p.id.clone());
                }
            }
        }
        if selected.as_deref() == Some(pane_id) {
            self.selected_terminal_pane_key = None;
            if let Some(neighbor) = neighbor {
                self.select_terminal(&neighbor);
            }
        }
        self.normalize_terminal_navigation(None);
    }
}

#[cfg(test)]
mod flat_terminal_tests {
    use super::*;
    use crate::tree::{AiSessionRef, SplitDirection, gen_id};
    use mt_identity::{TabId, TerminalIncarnationId};

    fn legacy_state() -> ProjectState {
        let mut panes = (0..6)
            .map(|index| {
                let mut pane = PaneState::new(format!("shell-{index}"));
                pane.cwd = Some(format!("/worktree/{index}"));
                if index % 2 == 0 {
                    pane.pty_id = Some(index + 10);
                    pane.terminal_incarnation_id = Some(TerminalIncarnationId::new());
                }
                pane
            })
            .collect::<Vec<_>>();
        panes[1].ai_session = Some(AiSessionRef {
            agent: Some("codex".into()),
            session_id: "saved-provider-session".into(),
            cwd: Some("/worktree/1".into()),
        });
        panes[2].status = PaneStatus::Error;
        let mut first_leaf = SplitNode::leaf(panes[0].clone());
        first_leaf.append_pane(None, panes[1].clone());
        first_leaf.activate_pane(&panes[0].id);
        let split = |direction, children| SplitNode::Split {
            id: gen_id("split"),
            direction,
            children,
            sizes: vec![37.0, 63.0],
        };
        let mut first = ProjectPanel::new(split(
            SplitDirection::Horizontal,
            vec![first_leaf, SplitNode::leaf(panes[2].clone())],
        ));
        first.custom_title = Some("legacy owner title".into());
        let second = ProjectPanel::new(split(
            SplitDirection::Vertical,
            vec![
                SplitNode::leaf(panes[3].clone()),
                split(
                    SplitDirection::Horizontal,
                    vec![
                        SplitNode::leaf(panes[4].clone()),
                        SplitNode::leaf(panes[5].clone()),
                    ],
                ),
            ],
        ));
        let mut state = ProjectState::new();
        state.active_panel_id = Some(first.id.clone());
        state.panels = vec![first, second];
        state
    }

    fn keys(state: &ProjectState) -> Vec<PaneKey> {
        state
            .ordered_terminal_panes()
            .iter()
            .map(|pane| pane.pane_key.clone())
            .collect()
    }

    fn owned_panes(state: &ProjectState) -> Vec<(TabId, PaneState)> {
        state
            .panels
            .iter()
            .flat_map(|panel| {
                panel
                    .layout
                    .panes()
                    .into_iter()
                    .map(move |pane| (panel.tab_id.clone(), pane.clone()))
            })
            .collect()
    }

    #[test]
    fn inventory_includes_all_legacy_owners_leaves_dormant_and_exited_records() {
        let state = legacy_state();
        let original = state.panels.clone();
        let panes = state.ordered_terminal_panes();
        assert_eq!(panes.len(), 6);
        assert_eq!(panes.iter().filter(|pane| pane.pty_id.is_some()).count(), 3);
        assert_eq!(
            panes
                .iter()
                .filter(|pane| pane.status == PaneStatus::Error)
                .count(),
            1
        );
        assert_eq!(
            panes[1].ai_session.as_ref().unwrap().session_id,
            "saved-provider-session"
        );
        assert_eq!(
            state.panels, original,
            "inventory reads cannot mutate route owners"
        );
    }

    #[test]
    fn nonfirst_owner_and_leaf_selection_survives_save_restore_without_shells() {
        let mut state = legacy_state();
        let target = keys(&state)[5].clone();
        state.normalize_terminal_navigation(Some(target.as_str()));
        assert_eq!(state.selected_terminal().unwrap().pane_key, target);
        let saved = state.saved_layout();
        assert_eq!(saved.selected_terminal_pane_key.as_ref(), Some(&target));
        assert_eq!(saved.active_tab_id.as_ref(), Some(&state.panels[1].tab_id));
        assert_eq!(saved.active_tab_index, 1);
        let json = serde_json::to_string(&saved).unwrap();
        let saved: mt_config::SavedProjectLayout = serde_json::from_str(&json).unwrap();
        let mut config = AppConfig::default();
        config.available_shells.clear();
        let (panels, active) = persist::restore_layout(&saved, &config);
        let mut restored = ProjectState::new();
        restored.panels = panels;
        restored.active_panel_id = active;
        restored.restore_terminal_navigation(&saved);
        assert_eq!(restored.selected_terminal().unwrap().pane_key, target);
        assert_eq!(restored.active_panel_id, state.active_panel_id);
        assert_eq!(keys(&restored), keys(&state));
        assert_eq!(
            restored.panels[0].custom_title,
            state.panels[0].custom_title
        );
        for ((old_owner, old), (owner, pane)) in
            owned_panes(&state).into_iter().zip(owned_panes(&restored))
        {
            assert_eq!(owner, old_owner);
            assert_eq!(pane.pane_key, old.pane_key);
            assert_eq!(pane.terminal_session_id, old.terminal_session_id);
            assert_eq!(pane.terminal_incarnation_id, old.terminal_incarnation_id);
            assert_eq!(pane.shell_name, old.shell_name);
            assert_eq!(pane.cwd, old.cwd);
            assert_eq!(pane.ai_session, old.ai_session);
            assert!(pane.pty_id.is_none());
        }
        assert_eq!(
            serde_json::to_string(&restored.saved_layout()).unwrap(),
            json,
            "a second snapshot must preserve selection, order, geometry and saved records"
        );
    }

    #[test]
    fn selecting_a_leaf_sibling_updates_compatibility_before_save_and_background_append() {
        let mut state = legacy_state();
        state.normalize_terminal_navigation(None);
        let original = keys(&state);
        let selected = &original[1];
        let processes = owned_panes(&state);
        assert!(state.select_terminal(selected.as_str()));
        let SplitNode::Leaf { active_pane_id, .. } = state.leaf_of_pane(selected.as_str()).unwrap()
        else {
            panic!("selected pane must remain in its original leaf");
        };
        assert_eq!(active_pane_id, selected.as_str());
        assert_eq!(state.selected_terminal_pane_key.as_ref(), Some(selected));
        assert_eq!(owned_panes(&state), processes);
        let snapshot = state.saved_layout();
        assert_eq!(snapshot.selected_terminal_pane_key.as_ref(), Some(selected));
        assert_eq!(
            snapshot.active_tab_id.as_ref(),
            Some(&state.panels[0].tab_id)
        );
        assert_eq!(snapshot.terminal_order.as_ref(), Some(&original));
        assert!(!state.select_terminal("missing-pane"));
        assert_eq!(
            serde_json::to_value(&state.saved_layout()).unwrap(),
            serde_json::to_value(&snapshot).unwrap()
        );

        let owner = state.panels[0].tab_id.clone();
        assert!(state.append_background_terminal(owner, PaneState::new("background")));
        let SplitNode::Leaf { active_pane_id, .. } = state.leaf_of_pane(selected.as_str()).unwrap()
        else {
            panic!("background append must preserve the leaf");
        };
        assert_eq!(active_pane_id, selected.as_str());
        assert_eq!(state.selected_terminal_pane_key.as_ref(), Some(selected));
        assert_eq!(state.all_panes().len(), original.len() + 1);
    }

    #[test]
    fn stale_duplicate_and_missing_preferences_are_lossless_and_idempotent() {
        let mut state = legacy_state();
        let original = keys(&state);
        state.selected_terminal_pane_key = Some(PaneKey::new());
        state.terminal_order = vec![original[4].clone(), PaneKey::new(), original[4].clone()];
        state.normalize_terminal_navigation(None);
        assert_eq!(state.selected_terminal().unwrap().pane_key, original[0]);
        let expected = [
            vec![original[4].clone()],
            original
                .iter()
                .filter(|key| **key != original[4])
                .cloned()
                .collect(),
        ]
        .concat();
        assert_eq!(keys(&state), expected);
        let before = serde_json::to_value(state.saved_layout()).unwrap();
        state.normalize_terminal_navigation(Some(original[5].as_str()));
        assert_eq!(
            serde_json::to_value(state.saved_layout()).unwrap(),
            before,
            "valid selection wins over window-global focus"
        );
    }

    #[test]
    fn reorder_cycle_and_index_share_flat_order_without_changing_routes() {
        let mut state = legacy_state();
        state.normalize_terminal_navigation(None);
        let original = keys(&state);
        state.select_terminal(original[4].as_str());
        let owners = state.panels.clone();
        assert!(state.reorder_terminal(&original[5], &original[0], false));
        assert_eq!(keys(&state)[0], original[5]);
        assert_eq!(
            state.panels, owners,
            "cross-owner drag changes no legacy tree or process identity"
        );
        assert_eq!(state.selected_terminal().unwrap().pane_key, original[4]);
        assert_eq!(
            state
                .cycle_terminal_target(original[5].as_str(), 1)
                .as_deref(),
            Some(original[0].as_str())
        );
        assert_eq!(
            state
                .cycle_terminal_target(original[5].as_str(), -1)
                .as_deref(),
            Some(original[4].as_str())
        );
        assert_eq!(state.terminal_at_index(1).unwrap().pane_key, original[5]);
        assert!(state.terminal_at_index(0).is_none());
        assert!(state.terminal_at_index(7).is_none());
        assert!(state.cycle_terminal_target("missing", 1).is_none());
        assert!(!state.reorder_terminal(&PaneKey::new(), &original[0], true));
        assert!(!state.reorder_terminal(&original[5], &original[0], false));
        assert_eq!(state.panels, owners);
        let saved = state.saved_layout();
        assert_eq!(saved.terminal_order, Some(keys(&state)));
    }

    #[test]
    fn closing_one_terminal_preserves_hidden_siblings_and_uses_flat_neighbors() {
        let mut state = legacy_state();
        state.normalize_terminal_navigation(None);
        let original = keys(&state);
        state.select_terminal(original[0].as_str());
        state.remove_pane(original[1].as_str());
        assert_eq!(state.selected_terminal().unwrap().pane_key, original[0]);
        assert_eq!(state.all_panes().len(), 5);
        state.reorder_terminal(&original[5], &original[0], true);
        state.remove_pane(original[0].as_str());
        assert_eq!(state.selected_terminal().unwrap().pane_key, original[5]);
        assert!(state.pane(original[2].as_str()).is_some());
        state.select_terminal(original[4].as_str());
        state.remove_pane(original[4].as_str());
        assert_eq!(state.selected_terminal().unwrap().pane_key, original[3]);
        for key in keys(&state) {
            state.remove_pane(key.as_str());
        }
        assert!(state.panels.is_empty());
        assert!(state.selected_terminal_pane_key.is_none());
        assert!(state.terminal_order.is_empty());
        assert!(state.active_panel_id.is_none());
    }

    #[test]
    fn background_append_preserves_selection_order_and_existing_processes() {
        let mut state = legacy_state();
        let original = keys(&state);
        state.select_terminal(original[5].as_str());
        state.normalize_terminal_navigation(None);
        let processes = owned_panes(&state);
        let owner = state.active_panel().unwrap().tab_id.clone();
        let mut added = PaneState::new("mobile-shell");
        added.pty_id = Some(99);
        let added_key = added.pane_key.clone();
        assert!(state.append_background_terminal(owner.clone(), added));
        assert_eq!(state.selected_terminal().unwrap().pane_key, original[5]);
        assert_eq!(keys(&state).last(), Some(&added_key));
        assert_eq!(state.all_panes().len(), 7);
        for (owner, pane) in processes {
            assert_eq!(state.pane(&pane.id), Some(&pane));
            assert_eq!(state.panel_id_of_pane(&pane.id), Some(owner.as_str()));
        }
        let before = state.panels.clone();
        assert!(!state.append_background_terminal(TabId::new(), PaneState::new("lost-owner")));
        assert_eq!(state.panels, before);
        let mut empty = ProjectState::new();
        assert!(empty.append_background_terminal(owner, PaneState::new("mobile-shell")));
        assert_eq!(empty.all_panes().len(), 1);
        assert!(empty.selected_terminal().is_some());
    }
}

struct GlobalStore(Entity<AppStore>);
impl Global for GlobalStore {}

/// 用量面板的六个偏好(对应旧版那六个 localStorage 键)。
///
/// 一把传是为了只触发一次 500ms 去抖写盘 —— 连点分段控件不该连写六次。
/// 取值合法性由面板侧的白名单/正则保证,store 只负责搬运。
pub struct UsagePrefs {
    pub scope: String,
    pub range: String,
    /// 项目**原始路径**;`None` = 整机。
    pub project: Option<String>,
    pub auto_refresh: u32,
    pub custom_from: String,
    pub custom_to: String,
}

/// 一次「关联 SSH」保存的结果(`SshAssocModal.tsx::handleSave` 收尾那一段
/// 要的全部素材)。由 [`AppStore::apply_ssh_assoc`] 返回。
pub struct SshAssocOutcome {
    /// 保存后该项目是否处于「已启用 SSH 工具」状态。
    pub enabled: bool,
    /// 保存**之前**是否已启用 —— 三条提示文案(启用/更新/停用)靠它分档。
    pub was_enabled: bool,
    /// 有效配置没变(幂等 reconcile / 存量迁移):落盘即可,**不弹提示**。
    pub silent: bool,
    /// 本次范围里的连接数与连接总数 —— 提示文案里的 `scopeAll` / `scopeSubset`。
    pub scope_len: usize,
    pub total_len: usize,
    /// 启用时的项目能力令牌(已由 [`AppStore::set_project_ssh_assoc`] 落盘,
    /// 这里带回只为调用方需要时展示/排查)。
    pub project_token: Option<String>,
    /// 注册器返回的中文提示(与装机版一字不差,不走 mt-i18n)。
    /// ⚠️ **当前没有读者**,与原版一致:`EnableSshToolsResult` 也带 `message`,
    /// 而 `SshAssocModal.tsx` 只取 `projectToken` —— 提示文案是弹窗自己按
    /// 启用/更新/停用三档拼的,注册器那句只进日志面。字段留着是为了不丢
    /// 服务层的返回信息(要排查「注册器到底做了什么」时它是唯一线索)。
    #[allow(dead_code)]
    pub message: String,
}

/// 一次 AI 事件算出来的提醒动作 + 播报所需的上下文。
///
/// 提示音与任务栏闪烁要碰 `Window`(拿 HWND),而 AI 事件是从后台 channel 泵进来的
/// —— store 只算「该做什么」,真正执行留给持有 window 的 [`crate::Workspace`]。
pub struct PendingAlert {
    pub plan: AlertPlan,
    pub project_id: String,
    pub project_name: String,
    /// 自定义提示音路径(`config.aiCompletionSoundPath`)。
    pub sound_path: Option<String>,
}

pub struct AppStore {
    config: AppConfig,
    /// 写盘令牌(乐观并发);0 = 还没成功 load 过,此时一律不写盘。
    token: u64,
    config_store: Arc<ConfigStore>,
    /// 配置落盘的单写者后台线程。主线程只把**完整快照**入队(见
    /// [`crate::store::config_writer`]):那条链末端是 `synchronous=FULL` 的
    /// SQLite 事务加一次投影文件 fsync,慢盘上几百毫秒,不能留在 UI 线程上。
    config_writer: ConfigWriter,
    /// 界面布局的落盘口(`layout.db`)。`None` = 库开不起来(盘满 / 权限),
    /// 此时布局**只在内存里活着**:界面照常用,退出即忘 —— 与配置加载失败时
    /// 「只读模式」同一条红线,绝不因为存不下就不让用。
    layout_store: Option<Arc<mt_layout::LayoutStore>>,
    /// 安装级身份来自 layout.db；库不可用时仅在本进程生成临时值。
    host_install_id: HostInstallId,
    /// 兼容 project ID 到稳定 worktree 身份的唯一注册表。
    project_worktree_bindings: HashMap<String, ProjectWorktreeBinding>,
    /// 与 `active_project_id` 同步的稳定路由身份。
    active_worktree_id: Option<WorktreeId>,
    /// Authenticated SSH runtime state stays project-scoped and process-local.
    remote_runtime_projects: HashMap<String, RemoteRuntimeProjectState>,
    /// Unique generations fence project removal and connection/path edits.
    next_remote_runtime_generation: u64,
    /// Per-terminal scheduling diagnostics for authenticated remote probes.
    remote_agent_polls: HashMap<u32, RemoteAgentPollState>,
    /// Unique generations fence terminal reuse and stale probe completions.
    next_remote_agent_generation: u64,
    /// 窗口几何(退出时的大小/位置/最大化态)。config 里没有对应字段 ——
    /// 这是 GPUI 版新补的能力,只住在 `layout.db` 与这里。
    window_geometry: Option<mt_layout::WindowGeometry>,
    /// 终端区右缘「终端列表」竖条的显隐。与 [`Self::window_geometry`] 同类:
    /// config 里没有对应字段,只住在 `layout.db` 与这里。
    terminals_panel_visible: bool,
    /// 攒着待写的项目 id 与「全局项脏了」标记。防抖窗口内拖十次分隔条只落一次盘。
    layout_dirty_projects: HashSet<String>,
    /// 每个 worktree 最后一次排队保存来自哪个兼容项目别名。共享 worktree 只允许
    /// 这个显式 owner 落盘,不能让 HashSet 遍历顺序决定最终内容。
    layout_dirty_worktree_owners: HashMap<WorktreeId, String>,
    layout_globals_dirty: bool,
    /// 布局防抖的代号,与 [`Self::save_generation`] 同一套路。
    /// **单独一份**:布局与配置现在写去两个地方,共用代号会让其中一路饿死。
    layout_save_generation: u64,
    _layout_save_task: Option<Task<()>>,

    pub active_project_id: Option<String>,
    project_states: HashMap<String, ProjectState>,
    /// ptyId → 终端视图。pane 只在树里存 id,视图挂这里(旧版 terminalCache)。
    terminals: HashMap<u32, Entity<TerminalPane>>,
    /// Process-local PTY attachments fenced by their stable route.
    terminal_routes: HashMap<u32, identity::TerminalRoute>,
    /// Rich agent state consumed by worktree cards and the global Agents feed.
    agent_runtime: AgentRuntimeRegistry,
    /// Process-local acknowledgement watermark for the global exact-run feed.
    /// This is deliberately separate from the legacy pane-level `DoneTracker`.
    agent_feed_acknowledged: HashMap<AgentRunId, AgentEventId>,
    /// 每个 pane 的退出订阅,与 terminals 同生命周期。
    pane_subs: HashMap<u32, Subscription>,
    /// 当前拿着键盘焦点的 pane(旧版靠 DOM `activeElement` 推,这里显式维护)。
    pub focused_pane_id: Option<String>,

    /// 移动端中转的连接状态(`src/store.ts:702` 的 `mobileRelayStatus`)。
    /// **纯运行时,不落盘** —— 与 [`Self::focused_pane_id`] 同类。
    mobile_relay_status: Option<MobileRelayStatusPayload>,

    /// AI 任务标记(`src/store.ts:666-671` 的 `markersByPty`)。
    /// **纯运行时,不落盘**;pane 一没,这一份跟着没(见 [`Self::dispose_terminal`])。
    markers_by_pty: HashMap<u32, Vec<AiMarker>>,
    /// 「这个 pane 上次跳到哪条标记」的游标(`useMarkerHotkeys.ts:19` 的 `lastJumpRef`)。
    ///
    /// 原版那份是模块级 ref、**从不清理**(pane 关了条目还在,微量泄漏 +
    /// 「pty id 复用后游标是旧的」的边界)。这里与标记表同生共死,顺手修掉。
    marker_cursor: HashMap<u32, String>,

    /// 会话分支的**自记账登记**(`src/store.ts:173` 的 `pendingForks`)。
    /// mini-term 自己发起的 fork 在新 pane 的 PTY 上登记「等新会话身份」,
    /// hook 上报新 id 时落成 child→parent 边写进 `config.session_lineage`。
    /// **纯运行时,不落盘**;见 [`AppStore::register_pending_fork`]。
    pending_forks: HashMap<u32, PendingFork>,

    next_pty_id: u32,
    ai: AiBridge,

    /// 当前生效的终端配色(主题装配的产物,见 [`crate::theme`])。
    /// 新建终端拿它,已存在的终端由 [`AppStore::apply_theme_from_config`] 热更新。
    terminal_theme: TerminalTheme,
    /// 当前主题的背景图氛围层参数。**渲染归 mt-ui,这里只是数据落点**。
    background_art: Option<BackgroundArt>,

    /// 展开的目录(按项目)。运行时态,落盘走 `ProjectConfig::expanded_dirs`。
    expanded_dirs: HashMap<String, HashSet<String>>,

    /// 目录技术栈探测缓存(`src/store.ts:708` 的 `dirKinds`)。
    /// key = 目录路径**原样**;`None` = 已探测但识别不出(**不再重探**)。
    /// 项目根与文件树里的子工程目录共用这一份。
    dir_kinds: HashMap<String, Option<ProjectKind>>,
    /// 在途探测(`useProjectKinds.ts` 那个模块级 `pending`)。
    /// **不是可订阅状态**,只为去重 —— 变化不 notify。
    dir_kinds_pending: HashSet<String>,

    /// 已退出的 PTY(`src/store.ts:660` 的 `exitedPtyIds`,`pty-exit` 登记)。
    /// 悬停缩略图据此画「已断开」遮罩;远程 pane 的重连覆盖层随 #28。
    /// Per-user PTY host client. `None` keeps the compatibility in-process backend.
    terminal_host: Option<TerminalHostClient>,
    /// Explicit dormant closes survive project switches and exclude alias recovery.
    pending_terminal_closes: panes::PendingTerminalCloses,

    /// **纯运行时,不落盘**;pane 一没跟着没(见 [`Self::dispose_terminal`])。
    exited_ptys: HashSet<u32>,

    /// 完成队列(未读集合 + 完成序号),对应旧版的 unreadDonePaneIds / aiDoneOrder。
    done: DoneTracker,
    /// 主窗口是否聚焦。聚焦时完成的任务用户正看着,不计入「未读完成」。
    window_focused: bool,

    /// 防抖保存的代号:只有最后一次排上的任务才真写盘。
    save_generation: u64,
    _save_task: Option<Task<()>>,
}

/// 开布局库,顺带跑一次「从 config.json 迁入」。
///
/// 返回 `None` 的三种情形都按同一档降级处理:**布局本次不落盘**,界面照常用。
/// 其中迁移失败也返回 `None` 是刻意的 —— 让本次继续走内存里那份、下次启动重试,
/// 比拿一份半截数据把用户的布局盖掉强。
fn open_layout_store(config: &AppConfig, may_migrate: bool) -> Option<Arc<mt_layout::LayoutStore>> {
    let dir = match mt_config::active_data_dir() {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("[layout] 定位数据目录失败({err:#}),本次布局不落盘");
            return None;
        }
    };
    let store = match mt_layout::LayoutStore::open_at(&dir) {
        Ok(store) => store,
        Err(err) => {
            eprintln!("[layout] 布局库打不开({err:#}),本次布局不落盘");
            return None;
        }
    };
    if may_migrate && store.needs_config_migration() {
        let fallback = layout_migration_fallback(config, &dir);
        let source = fallback.as_ref().unwrap_or(config);
        match store.migrate_from_config(source) {
            Ok(n) => eprintln!(
                "[layout] 已迁入 {n} 个项目的布局 → {}",
                store.path().display()
            ),
            Err(err) => {
                eprintln!("[layout] 布局迁移失败({err:#}),本次布局不落盘");
                return None;
            }
        }
    }
    Some(Arc::new(store))
}

/// 布局迁移的**兜底数据源**:`{dir}/config.json.pre-sqlite`(配置搬进 config.db
/// 时留下的完整旧配置存档)。
///
/// 为什么需要它:配置迁移一完成,`config.json` 就被覆盖成只剩 SSH 的投影,而
/// `savedLayout` 此后只活在「内存里这一份 AppConfig」上。要是布局迁移偏偏在那一次
/// 失败(盘满 / 库被占用),下次启动的 config 是从 config.db 读的、`savedLayout`
/// 全是 `None` —— 重试也只会迁出一片空白,旧布局**永久丢失**。
///
/// 只在「传进来的 config 一个布局都没有」时才回退去读存档:正常首启走内存那份
/// (更新、且不必读盘);第二次以后 `needs_config_migration()` 已是 false,根本
/// 到不了这里。
fn layout_migration_fallback(config: &AppConfig, dir: &Path) -> Option<AppConfig> {
    if config.projects.iter().any(|p| p.saved_layout.is_some()) {
        return None;
    }
    let archive = dir.join("config.json.pre-sqlite");
    let archived = mt_config::read_config_from(&archive).ok().flatten()?;
    if !archived.projects.iter().any(|p| p.saved_layout.is_some()) {
        return None;
    }
    eprintln!(
        "[layout] 内存里的配置已无 savedLayout,改从存档迁移: {}",
        archive.display()
    );
    Some(archived)
}

/// 把库里的布局覆盖进 `config` 的对应字段(内存缓存),并清掉已删项目的残行。
///
/// 库里**没有**的全局项保持 config 里的值不动:`None` 的语义是「这个键没存过」,
/// 不是「用户把它设成了默认值」。项目级则相反 —— 逐个赋值(含赋 `None`),
/// 库才是唯一真相:用户把某项目的终端关光了,config.json 里的残留不该复活。
///
/// 返回窗口几何与终端列表竖条显隐(config 里没有它们的位置 —— 都是 GPUI 版
/// 新加的能力,只住在 `layout.db` 与 `AppStore` 的字段上,由调用方单独接住)。
fn apply_layout_db(
    store: &mt_layout::LayoutStore,
    config: &mut AppConfig,
    may_reconcile: bool,
) -> (
    Option<mt_layout::WindowGeometry>,
    Option<bool>,
    HostInstallId,
    HashMap<String, ProjectWorktreeBinding>,
) {
    let globals = store.load_globals();
    if globals.layout_sizes.is_some() {
        config.layout_sizes = globals.layout_sizes;
    }
    if globals.middle_column_sizes.is_some() {
        config.middle_column_sizes = globals.middle_column_sizes;
    }
    if let Some(visible) = globals.middle_column_visible {
        config.middle_column_visible = visible;
    }
    if globals.right_drawer_width.is_some() {
        config.right_drawer_width = globals.right_drawer_width;
    }

    let host_install_id = store.local_host_install_id().unwrap_or_else(|error| {
        eprintln!("[identity] 读取安装身份失败({error:#}),本次使用临时身份");
        HostInstallId::new()
    });
    let existing_bindings = store.load_project_bindings().unwrap_or_else(|error| {
        eprintln!("[identity] 读取持久化项目绑定失败: {error:#}");
        HashMap::new()
    });
    let desired_bindings = identity::resolve_project_bindings(
        &config.projects,
        &config.ssh_connections,
        &host_install_id,
        &existing_bindings,
    );

    let (mut layouts, bindings) = if may_reconcile {
        let now_ms = layout::unix_time_ms();
        match store.reconcile_worktree_layouts(&desired_bindings, now_ms) {
            Ok(reconciled) => (reconciled.layouts, reconciled.bindings),
            Err(error) => {
                eprintln!("[identity] worktree 布局协调失败({error:#}),本次退回兼容读取");
                (
                    store.load_project_layouts(),
                    desired_bindings
                        .into_iter()
                        .map(|binding| (binding.project_id.clone(), binding))
                        .collect(),
                )
            }
        }
    } else {
        (store.load_project_layouts(), existing_bindings)
    };

    for project in config.projects.iter_mut() {
        project.saved_layout = layouts.remove(&project.id);
    }
    if may_reconcile {
        let live: HashSet<String> = config.projects.iter().map(|p| p.id.clone()).collect();
        if let Err(error) = store.retain_project_bindings(&live) {
            eprintln!("[layout] 清理无主项目绑定失败: {error:#}");
        }
    }

    (
        globals.window.filter(|geo| geo.is_sane()),
        globals.terminals_panel_visible,
        host_install_id,
        bindings,
    )
}

impl AppStore {
    /// 装配 store:加载配置 → 恢复各项目布局(不起 PTY,PTY 在首次显示时懒起)。
    pub fn new(config_store: Arc<ConfigStore>, ai: AiBridge, cx: &mut Context<Self>) -> Self {
        let (mut config, token) = match config_store.load() {
            Ok(loaded) => (loaded.config, loaded.token),
            Err(err) => {
                // 加载失败**绝不**伪装成空配置:令牌留 0,后续所有保存都会被自己挡下,
                // 免得一次读盘故障把用户的项目列表清空(旧版同一条红线)。
                eprintln!("[store] 配置加载失败({err:#}),本次以只读模式运行");
                (AppConfig::default(), 0)
            }
        };

        // 布局库:开库 →(首次)从 config.json 灌一次 → 把库里的值覆盖回
        // `config` 的对应字段。**覆盖这一步是整个改造的支点** —— 各处 getter
        // 照旧读 `self.config.*`(它现在是内存缓存),只有落盘那一步改了道。
        // 配置加载失败(token=0)时不迁移:那份 config 是空默认值,灌进去等于
        // 拿一份伪造的空布局把用户真实的布局盖掉。
        let layout_store = open_layout_store(&config, token != 0);
        let (window_geometry, terminals_panel_visible, host_install_id, project_worktree_bindings) =
            if let Some(store) = layout_store.as_ref() {
                apply_layout_db(store, &mut config, token != 0)
            } else {
                let host_install_id = HostInstallId::new();
                let bindings = identity::resolve_project_bindings(
                    &config.projects,
                    &config.ssh_connections,
                    &host_install_id,
                    &HashMap::new(),
                )
                .into_iter()
                .map(|binding| (binding.project_id.clone(), binding))
                .collect();
                (None, None, host_install_id, bindings)
            };

        let mut project_states = HashMap::new();
        let mut expanded_dirs = HashMap::new();
        for project in &config.projects {
            let mut state = ProjectState::new();
            if let Some(saved) = &project.saved_layout {
                let (panels, active) = persist::restore_layout(saved, &config);
                state.panels = panels;
                state.active_panel_id = active;
                state.restore_terminal_navigation(saved);
                state.status = state.highest_status();
            }
            project_states.insert(project.id.clone(), state);
            expanded_dirs.insert(
                project.id.clone(),
                project.expanded_dirs.iter().cloned().collect(),
            );
        }

        let active_project_id = config
            .last_active_project_id
            .clone()
            .filter(|id| project_states.contains_key(id))
            .or_else(|| config.projects.first().map(|p| p.id.clone()));
        let active_worktree_id = active_project_id
            .as_deref()
            .and_then(|project_id| project_worktree_bindings.get(project_id))
            .map(|binding| binding.worktree_id.clone());

        // 配置落盘搬去后台线程之后,退出前必须有人把队列排干。挂在这里而不是
        // `main.rs`:`AppStore` 是配置写入的唯一入口,排干义务跟着它走才不会
        // 因为壳那边改动漏掉。
        //
        // 时序靠 gpui 的 `App::shutdown` 兜住 —— 它**先把所有退出观察者的函数体
        // 跑完**(`main.rs` 那个在函数体里补的最后一次 `save_config_now()` 于是
        // 已经入队),**再**统一 await 收上来的 future。所以本观察者虽然注册得更
        // 早,轮到它的 future 被 poll 时看到的已是最终队列。
        let config_writer = ConfigWriter::spawn(config_store.clone());
        let drain = config_writer.drain_handle();
        let terminal_host = if terminal_host_enabled() {
            match TerminalHostClient::production() {
                Ok(client) => Some(client),
                Err(error) => {
                    eprintln!("[terminal-host] unavailable, using compatibility backend: {error}");
                    None
                }
            }
        } else {
            None
        };

        // 显式走 `App::on_app_quit` 而不是 `Context::on_app_quit`:排干不需要
        // `&mut AppStore`,而后者会在退出那一刻回头 `update` 本实体 —— 平白多一
        // 条「实体还在不在」的依赖。
        App::on_app_quit(cx, move |_cx| {
            let drain = drain.clone();
            async move {
                drain.drain();
            }
        })
        .detach();

        Self {
            config,
            token,
            config_store,
            config_writer,
            layout_store,
            host_install_id,
            project_worktree_bindings,
            active_worktree_id,
            remote_runtime_projects: HashMap::new(),
            next_remote_runtime_generation: 0,
            remote_agent_polls: HashMap::new(),
            next_remote_agent_generation: 0,
            window_geometry,
            // 缺省展开:面板是发现型入口,收着的话没人知道它存在
            terminals_panel_visible: terminals_panel_visible.unwrap_or(true),
            layout_dirty_projects: HashSet::new(),
            layout_dirty_worktree_owners: HashMap::new(),
            layout_globals_dirty: false,
            layout_save_generation: 0,
            _layout_save_task: None,
            active_project_id,
            project_states,
            terminals: HashMap::new(),
            terminal_host,
            pending_terminal_closes: Default::default(),
            terminal_routes: HashMap::new(),
            agent_runtime: AgentRuntimeRegistry::default(),
            agent_feed_acknowledged: HashMap::new(),
            pane_subs: HashMap::new(),
            focused_pane_id: None,
            mobile_relay_status: None,
            markers_by_pty: HashMap::new(),
            marker_cursor: HashMap::new(),
            pending_forks: HashMap::new(),
            next_pty_id: 1,
            ai,
            // 真正的配色在 `apply_theme_from_config` 里装配(要 `&mut App` 取系统
            // 外观 / 装 gpui-component 主题层),这里先给个能跑的初值
            terminal_theme: TerminalTheme::default(),
            background_art: None,
            expanded_dirs,
            dir_kinds: HashMap::new(),
            dir_kinds_pending: HashSet::new(),
            exited_ptys: HashSet::new(),
            done: DoneTracker::default(),
            window_focused: true,
            save_generation: 0,
            _save_task: None,
        }
    }

    // === 全局取用 ===

    pub fn set_global(store: Entity<AppStore>, cx: &mut App) {
        cx.set_global(GlobalStore(store));
    }

    pub fn global(cx: &App) -> Entity<AppStore> {
        cx.global::<GlobalStore>().0.clone()
    }

    // === 只读访问 ===

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn projects(&self) -> &[ProjectConfig] {
        &self.config.projects
    }

    pub fn project(&self, id: &str) -> Option<&ProjectConfig> {
        self.config.projects.iter().find(|p| p.id == id)
    }

    pub fn project_state(&self, id: &str) -> Option<&ProjectState> {
        self.project_states.get(id)
    }

    pub fn active_project(&self) -> Option<&ProjectConfig> {
        self.active_project_id
            .as_deref()
            .and_then(|id| self.project(id))
    }

    pub fn active_layout(&self) -> Option<&SplitNode> {
        self.active_project_id
            .as_deref()
            .and_then(|id| self.project_states.get(id))
            .and_then(|s| s.active_layout())
    }

    pub fn terminal(&self, pty_id: u32) -> Option<&Entity<TerminalPane>> {
        self.terminals.get(&pty_id)
    }

    /// 这个 PTY 已经退出了吗(`exitedPtyIds.has`)。
    pub fn is_pty_exited(&self, pty_id: u32) -> bool {
        self.exited_ptys.contains(&pty_id)
    }
}
