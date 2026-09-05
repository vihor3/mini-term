//! Confirmed single-terminal close and source-fenced fork-to-new-terminal actions.
//!
//! Tab X, menu close and Ctrl+Shift+W share one captured route and confirmation.
//! Window-close accounting still includes every owned terminal in every project.

use gpui::{App, Entity, Window};
use mt_config::{AiLauncher, ShellConfig};

use crate::i18n::{t, tr};
use crate::menu;
use crate::prompt::Confirm;
use crate::session_branch::{BranchMenuSegment, branch_menu_segment};
use crate::store::{AppStore, TerminalJumpTarget, resolve_fork_cwd};
use crate::tree::{PaneState, PaneStatus};

/// 这个状态算「AI 会话还活着」吗。
pub fn is_ai_alive(status: PaneStatus) -> bool {
    matches!(status, PaneStatus::AiWorking | PaneStatus::AiIdle)
}

// === 新建终端菜单 ===

/// 这个项目能不能在「新建终端」菜单里出 AI 启动器段。
///
/// 远程项目**一律不出**:SSH 项目的 PTY 是 ssh 启动器,启动初期可能停在口令或
/// host key 确认交互上,**预写的命令会被当口令消费** —— 命令丢失之外,登录本身
/// 还可能因此失败一次;存了密码的连接则会与 PTY 的密码 autofill 状态机抢同一次
/// 输入。判据与 `AppStore::hydrate_project` 的自动续接守卫同源(`store/panes.rs`,
/// 那里对远程项目跳过 resume 预写),也与移动端 `mt_relay::can_start_session`
/// 把远程项目挡在发起会话之外的口径一致。
///
/// WSL 项目**不挡**:本地 PTY 直接起 `wsl.exe`,没有口令交互那一段。移动端连
/// WSL 根项目一起挡是对话镜像盲发的问题,与桌面端这条路径无关。
pub fn project_allows_launchers(ssh_connection_id: Option<&str>) -> bool {
    ssh_connection_id.is_none()
}

/// 「新建终端」菜单的数据源:shell 列表 + 该项目**可用**的启动器。
///
/// 与 [`new_terminal_menu_entries`] 一样收成一处 —— 三处入口各判一次远程守卫
/// 迟早漏掉一处,而漏掉的那处正好是会把用户 ssh 口令吃掉的那条路。
/// 项目不存在时按「不给启动器」处置(保守侧)。
pub fn new_terminal_menu_data(
    store: &AppStore,
    project_id: &str,
) -> (Vec<ShellConfig>, Vec<AiLauncher>) {
    let shells = store.config().available_shells.clone();
    let allows = store
        .project(project_id)
        .is_some_and(|p| project_allows_launchers(p.ssh_connection_id.as_deref()));
    let launchers = if allows {
        store.mobile_relay().launchers
    } else {
        Vec::new()
    };
    (shells, launchers)
}

/// 「新建终端」菜单该不该弹出来。
///
/// 原判据是 `shells.len() <= 1` —— 只有一个 shell 时直接开,别让单 shell 用户
/// 每次多点一下(见 `terminal_area::render_leaf_tab_bar` 那处注释)。接入 AI
/// 启动器后可选项变成两段,判据必须**连启动器一起算**,否则单 shell 用户永远
/// 看不到启动器段。
///
/// ⚠️ **影响面比字面大**:启动器的读口径(`AppStore::mobile_relay`)在配置整块
/// 缺失时回落 `Default`,含预置的 Claude / Codex 两条,`launcher_count` 因此
/// 恒 ≥ 2 —— 除非用户把启动器删光。也就是说**所有**单 shell 用户点「+」从此
/// 都会弹菜单,而不只是自己精简过 shell 列表的那批。这是有意为之:本次的目的
/// 就是把启动器这个入口曝光出来,多的那一次点击换来的是「一键开 AI 会话」。
///
/// 例外是远程项目 —— [`new_terminal_menu_data`] 对它返回空启动器,
/// 于是单 shell + 远程项目仍然是点一下直接开。
pub fn should_show_new_terminal_menu(shell_count: usize, launcher_count: usize) -> bool {
    shell_count + launcher_count > 1
}

/// 「新建终端」菜单的条目:shell 段 +(有启动器时)AI 启动器段 +「管理启动器…」。
///
/// # 为什么收成一个入口
///
/// 新建终端有三条路:tab 栏的 `+`、空态的「+ 新建终端」按钮、终端面板的 `+`。
/// 三处此前是同一份代码复制三遍,菜单内容一致纯靠人工对齐;加了启动器段之后
/// 三份各改一遍必然漂移(与本模块开头「关终端必须收成一个入口」同一个理由)。
///
/// 落点差异由两个回调表达:前两处开 tab(带/不带锚点),第三处开面板。
pub fn new_terminal_menu_entries(
    shells: Vec<ShellConfig>,
    launchers: Vec<AiLauncher>,
    on_shell: impl Fn(ShellConfig, &mut Window, &mut App) + Clone + 'static,
    on_launcher: impl Fn(AiLauncher, &mut Window, &mut App) + Clone + 'static,
) -> Vec<menu::MenuEntry> {
    let mut entries: Vec<menu::MenuEntry> = shells
        .into_iter()
        .map(|shell| {
            let on_shell = on_shell.clone();
            let name = shell.name.clone();
            menu::item(name, move |window, cx| on_shell(shell.clone(), window, cx))
        })
        .collect();

    // 启动器一条都没有时不出这一段(含标题与分隔线)——空标题比没有更难看
    if !launchers.is_empty() {
        entries.push(menu::separator());
        entries.push(menu::MenuEntry::Header(
            t("terminalArea", "aiLaunchers").into(),
        ));
        for launcher in launchers {
            let on_launcher = on_launcher.clone();
            let name = launcher.name.clone();
            entries.push(menu::item(name, move |window, cx| {
                on_launcher(launcher.clone(), window, cx)
            }));
        }
    }

    // 启动器配置住在「移动端」面板里(它同时是移动端发起会话的名单),
    // 桌面端用户找不到那个入口 —— 这一条是唯一的指路牌,故**总是**出现。
    entries.push(menu::separator());
    entries.push(menu::item(
        t("terminalArea", "manageLaunchers"),
        crate::mobile_panel::open,
    ));
    entries
}

/// 盘点一组 pane 里活着的 AI 会话,返回它们的显示名(顺序同 tab 顺序)。
pub fn ai_session_labels(panes: &[PaneState]) -> Vec<String> {
    panes
        .iter()
        .filter(|p| is_ai_alive(p.status))
        .map(|p| p.label().to_string())
        .collect()
}

/// 这个 pane 要计入**关窗**确认吗(`App.tsx:57-60` 的 `collectLiveAiPanes`)。
///
/// 比关 tab / 关整组多一条 `ptyId !== undefined`:布局是从 `config.json` 恢复的,
/// 落盘时带着 `ai-idle` 的 pane 在 PTY 起来之前**什么都不会被杀掉**,
/// 拿它去拦关窗纯属噪音。状态判据本身与关 tab 完全相同。
pub fn counts_for_window_close(pane: &PaneState) -> bool {
    pane.pty_id.is_some() && is_ai_alive(pane.status)
}

/// 关窗确认正文里的一行:`· {项目名} / {标签}`;项目名为空时退成 `· {标签}`
/// (`App.tsx:62-63` 一字不差)。
pub fn window_close_line(project_name: &str, pane: &PaneState) -> String {
    if project_name.is_empty() {
        format!("· {}", pane.label())
    } else {
        format!("· {project_name} / {}", pane.label())
    }
}

/// 关窗前跨**全部项目**盘点活着的 AI 会话,返回正文用的名字列表。
///
/// 与 TS 的一处偏差(与 `collect_ai_projects` 同源):那边遍历 `projectStates`
/// (插入序),Rust 侧那是 `HashMap`、遍历序不定,于是改按**配置里的项目次序**走
/// —— 既确定,又与项目列表的上下顺序一致。
pub fn collect_live_ai_panes(store: &AppStore) -> Vec<String> {
    let mut names = Vec::new();
    for project in store.projects() {
        let Some(state) = store.project_state(&project.id) else {
            continue;
        };
        for pane in state.all_panes() {
            if counts_for_window_close(pane) {
                names.push(window_close_line(&project.name, pane));
            }
        }
    }
    names
}

/// 关闭一个终端 tab。**总是**先确认(与原版一致:没有 AI 也要问一句)。
pub fn close_pane(
    store: Entity<AppStore>,
    project_id: String,
    pane_id: String,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(target) = store.read(cx).terminal_jump_target_for_pane(&project_id, &pane_id) else {
        return;
    };
    close_terminal_target(store, target, window, cx);
}

pub fn close_terminal_target(
    store: Entity<AppStore>,
    target: TerminalJumpTarget,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(request) = store.read(cx).terminal_close_request(&target) else {
        return;
    };
    let Some(pane) = store
        .read(cx)
        .project_state(&target.project_id)
        .and_then(|s| s.pane(target.pane_key.as_str()))
        .cloned()
    else {
        return;
    };
    let label = pane.label().to_string();
    let has_ai = is_ai_alive(pane.status);
    let (title, message) = if has_ai {
        (
            t("paneGroup", "closeAiTitle"),
            tr!("paneGroup", "closeTabAiMessage", label = label),
        )
    } else {
        (
            t("paneGroup", "closeTerminalTitle"),
            tr!("paneGroup", "closeTabMessage", label = label),
        )
    };

    Confirm::new(title, message).open(
        move |window, cx| {
            let close = store.update(cx, |store, cx| store.close_terminal_target(request.clone(), cx));
            let target = target.clone();
            window.spawn(cx, async move |cx| {
                if close.await {
                    let _ = cx.update(|window, cx| {
                        crate::workbench_area::reactivate_active_page(
                            &target.project_id, &target.worktree_id, window, cx,
                        );
                    });
                }
            }).detach();
        },
        window,
        cx,
    );
}

fn fork_source_unchanged(expected: &PaneState, current: &PaneState) -> bool {
    expected.pane_key == current.pane_key
        && expected.terminal_session_id == current.terminal_session_id
        && expected.terminal_incarnation_id == current.terminal_incarnation_id
        && expected.shell_name == current.shell_name
        && expected.cwd == current.cwd
        && expected.ai_session == current.ai_session
}

/// Resolve CWD off-thread, then create a new terminal only while the captured
/// source route and focus still match. Register lineage before writing the command.
pub fn fork_pane_session(
    store: Entity<AppStore>,
    project_id: String,
    pane_id: String,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(target) = store.read(cx).terminal_jump_target_for_pane(&project_id, &pane_id) else {
        return;
    };
    if store.read(cx).active_project_id.as_deref() != Some(project_id.as_str())
        || store.read(cx).resolve_terminal_jump_target(&target).is_none()
    {
        return;
    }
    let selected_before = store.read(cx).active_pane_id(&project_id);
    let focus_before = window.focused(cx);
    let Some(source_snapshot) = store
        .read(cx)
        .project_state(&project_id)
        .and_then(|s| s.pane(&pane_id))
        .cloned()
    else {
        return;
    };
    let Some(session) = source_snapshot.ai_session.clone() else {
        return;
    };
    let Some(shell) = store.read(cx).resolve_shell(Some(&source_snapshot.shell_name)) else {
        return;
    };
    // 命令与归一化 agent 都由菜单那份判据产出 —— 菜单出得来的项,动作就跑得通
    let BranchMenuSegment::Fork { command, agent, .. } = branch_menu_segment(Some(&session), None)
    else {
        return;
    };
    let parent_session_id = session.session_id.clone();

    window
        .spawn(cx, async move |cx| {
            // 反查是**同步磁盘遍历**(`~/.claude/projects` 逐桶找文件),
            // 落在 GPUI 主线程上就是整个窗口卡住
            let cwd = cx
                .background_executor()
                .spawn(async move { resolve_fork_cwd(&session) })
                .await;
            let _ = cx.update(|window, cx| {
                let focus_unchanged = focus_before.as_ref().map_or_else(
                    || window.focused(cx).is_none(),
                    |focus| focus.is_focused(window),
                );
                if !focus_unchanged {
                    return;
                }
                let created = store.update(cx, |store, cx| {
                    if store.active_project_id.as_deref() != Some(project_id.as_str())
                        || store.active_worktree_id() != Some(&target.worktree_id)
                        || store.resolve_terminal_jump_target(&target).is_none()
                        || store.active_pane_id(&project_id) != selected_before
                    {
                        return false;
                    }
                    let Some(source) = store.project_state(&project_id).and_then(|state| state.pane(&pane_id)) else {
                        return false;
                    };
                    if !fork_source_unchanged(&source_snapshot, source) {
                        return false;
                    }
                    let cwd = cwd.or_else(|| source_snapshot.cwd.clone());
                    let Some(new_pane) = store.new_terminal_with_cwd(
                        &project_id,
                        Some(shell),
                        Some(pane_id.clone()),
                        cwd,
                        window,
                        cx,
                    ) else {
                        return false;
                    };
                    let pty_id = store
                        .project_state(&project_id)
                        .and_then(|s| s.pane(&new_pane))
                        .and_then(|p| p.pty_id);
                    if let Some(pty_id) = pty_id {
                        store.register_pending_fork(pty_id, &agent, &parent_session_id);
                    }
                    store.write_to_pane(&project_id, &new_pane, &format!("{command}\r"), cx);
                    true
                });
                if created {
                    crate::workbench_area::activate_terminal_page(window, cx);
                }
            });
        })
        .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_fork_rejects_source_replacement_but_not_activity_updates() {
        use crate::tree::AiSessionRef;
        use mt_identity::{PaneKey, TerminalIncarnationId, TerminalSessionId};
        let mut source = PaneState::new("source-shell");
        source.terminal_incarnation_id = Some(TerminalIncarnationId::new());
        source.cwd = Some("/source/cwd".into());
        source.ai_session = Some(AiSessionRef {
            agent: Some("codex".into()), session_id: "parent".into(), cwd: Some("/session/cwd".into()),
        });
        let mut current = source.clone();
        current.status = PaneStatus::AiWorking;
        current.attention = true;
        assert!(fork_source_unchanged(&source, &current));
        let mut changed = source.clone();
        changed.pane_key = PaneKey::new();
        assert!(!fork_source_unchanged(&source, &changed));
        let mut changed = source.clone();
        changed.terminal_session_id = TerminalSessionId::new();
        assert!(!fork_source_unchanged(&source, &changed));
        let mut changed = source.clone();
        changed.terminal_incarnation_id = Some(TerminalIncarnationId::new());
        assert!(!fork_source_unchanged(&source, &changed));
        let mut changed = source.clone();
        changed.cwd = Some("/different".into());
        assert!(!fork_source_unchanged(&source, &changed));
        let mut changed = source.clone();
        changed.shell_name = "different-shell".into();
        assert!(!fork_source_unchanged(&source, &changed));
        let mut changed = source.clone();
        changed.ai_session.as_mut().unwrap().session_id = "other-parent".into();
        assert!(!fork_source_unchanged(&source, &changed));
        let mut changed = source.clone();
        changed.ai_session.as_mut().unwrap().agent = Some("claude-code".into());
        assert!(!fork_source_unchanged(&source, &changed));
    }

    /// 「只有一个可选项就别弹菜单」那道闸:接入 AI 启动器后必须**连启动器一起算**。
    /// 只按 shell 数判的话,单 shell 用户永远看不到启动器段 —— 而那正是这次要给
    /// 他们的入口。因为启动器读口径缺省含预置两条,实际效果是所有单 shell 用户
    /// 点「+」都会弹菜单(远程项目除外,那边启动器为空)。
    #[test]
    fn single_option_gate_counts_launchers_too() {
        // 一个 shell、零启动器:只有一条路,直接开(原行为;也是远程项目的情形)
        assert!(!should_show_new_terminal_menu(1, 0));
        // 一个 shell、一条启动器:有得选了,必须弹
        assert!(should_show_new_terminal_menu(1, 1));
        // 预置两条启动器是缺省状态 —— 单 shell 用户实际落在这一档
        assert!(should_show_new_terminal_menu(1, 2));
        // 多 shell 照旧弹
        assert!(should_show_new_terminal_menu(2, 0));
        // 一条 shell 都没有(配置损坏)也不弹:调用方那条分支会回落默认 shell
        assert!(!should_show_new_terminal_menu(0, 0));
    }

    /// 远程项目不出启动器段:ssh 启动初期停在口令/host key 交互上时,
    /// 预写的 `{命令}\r` 会被当口令消费 —— 判据与 `hydrate_project` 的自动续接
    /// 守卫同源。WSL 不挡(本地直接起 wsl.exe,没有那段交互)。
    #[test]
    fn launcher_section_hidden_for_remote_projects_only() {
        assert!(project_allows_launchers(None));
        assert!(!project_allows_launchers(Some("conn-1")));
    }

    fn pane(label: &str, status: PaneStatus) -> PaneState {
        let mut p = PaneState::new(label);
        p.status = status;
        p
    }

    /// 「活着的 AI」只认两个状态 —— idle 与 error 都不算。
    #[test]
    fn ai_存活判据只认两态() {
        assert!(is_ai_alive(PaneStatus::AiWorking));
        assert!(is_ai_alive(PaneStatus::AiIdle));
        assert!(!is_ai_alive(PaneStatus::Idle));
        // shell 退出(error)不该让关闭确认框变成「有 AI 在跑」
        assert!(!is_ai_alive(PaneStatus::Error));
    }

    /// 盘点取显示名、保持 tab 顺序、非 AI 的不进表。
    #[test]
    fn 盘点按_tab_顺序列出活着的会话() {
        let mut renamed = pane("pwsh", PaneStatus::AiWorking);
        renamed.custom_title = Some("codex 跑测试".into());
        let panes = vec![
            pane("bash", PaneStatus::Idle),
            renamed,
            pane("cmd", PaneStatus::Error),
            pane("pwsh", PaneStatus::AiIdle),
        ];
        assert_eq!(
            ai_session_labels(&panes),
            vec!["codex 跑测试".to_string(), "pwsh".to_string()]
        );
    }

    /// 一个 AI 都没有时盘点为空 —— 调用方据此走「不带名字」的那套文案。
    #[test]
    fn 没有_ai_时盘点为空() {
        let panes = vec![pane("bash", PaneStatus::Idle), pane("cmd", PaneStatus::Error)];
        assert!(ai_session_labels(&panes).is_empty());
    }

    /// 关窗口径**比关 tab 多一条 pty_id**:恢复出来还没起进程的 pane 关掉不损失
    /// 任何东西,拿它拦关窗是纯噪音。
    #[test]
    fn 关窗盘点要求_pty_已起() {
        let mut restored = pane("pwsh", PaneStatus::AiIdle);
        restored.pty_id = None;
        assert!(!counts_for_window_close(&restored), "没起过 PTY 的不算");

        let mut live = pane("pwsh", PaneStatus::AiIdle);
        live.pty_id = Some(7);
        assert!(counts_for_window_close(&live));

        // 关 tab 那条口径**不看** pty_id —— 两者有意不同,别互相同化
        assert!(is_ai_alive(restored.status));
    }

    /// 状态判据与关 tab 完全一致:只有两个 AI 态算,idle / error 都不算。
    #[test]
    fn 关窗盘点的状态判据与关_tab_同() {
        for (status, expect) in [
            (PaneStatus::AiWorking, true),
            (PaneStatus::AiIdle, true),
            (PaneStatus::Idle, false),
            (PaneStatus::Error, false),
        ] {
            let mut p = pane("pwsh", status);
            p.pty_id = Some(1);
            assert_eq!(counts_for_window_close(&p), expect, "{status:?}");
        }
    }

    /// 正文一行的拼串:`· 项目名 / 标签`,项目名为空时退成 `· 标签`;
    /// 标签取 `customTitle || shellName`。
    #[test]
    fn 关窗清单每行的拼串() {
        let mut p = pane("pwsh", PaneStatus::AiWorking);
        p.pty_id = Some(1);
        assert_eq!(window_close_line("mini-term", &p), "· mini-term / pwsh");
        assert_eq!(window_close_line("", &p), "· pwsh");

        p.custom_title = Some("codex 跑测试".into());
        assert_eq!(
            window_close_line("mini-term", &p),
            "· mini-term / codex 跑测试"
        );
    }
}
