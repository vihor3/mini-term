//! 界面布局持久化(rusqlite,存 `{active_data_dir}/layout.db`)。
//!
//! # 为什么从 config.json 里搬出来
//!
//! 布局是**交互频次**的数据(拖分隔条 / 开关终端 / 分屏 / 拖窗口),配置是月级的
//! (SSH 连接、shell 列表、各种开关)。此前两者共用 `config.json` 一个信封:改一次
//! 布局要把整份配置 `to_string_pretty` 重写一遍,还要先 `copy` 一份同样大小的
//! `.bak` —— 实测本机 config.json 62.6 KB,即拖一次分隔条约 125 KB 落盘,而真正
//! 变的只有几个 f64。搬进 SQLite 后一次布局变更是**一行 upsert**。
//!
//! 顺带把损坏半径切开:config.json 写坏会连项目列表与 SSH 连接一起赔进去,
//! 布局库炸了只丢布局(下次启动回到默认分屏,项目一个不少)。
//!
//! # 为什么树仍存 JSON,不拆关系表
//!
//! `SavedProjectLayout` 那棵分屏树**永远整读整写**,没有任何按节点查询的需求。
//! 拆成 `nodes(id, parent_id, ...)` 递归表只会把一次 upsert 变成 N 行事务,外加
//! 自己维护递归完整性与孤儿清理。SQLite 在这里的价值是「更好的写入信封」,
//! 不是「把树拆成关系模型」—— 磁盘上仍是同一个 serde 树定义；新增稳定身份字段
//! 都是可选字段，因此旧 config.json 里的 `savedLayout` 仍可迁移读取。
//!
//! # 与 usage.db 的两处刻意不同
//!
//! - **不复用同一个库**:`usage.db` 二十多 MB,且 [`mt_usage`] 的策略是 schema
//!   版本不匹配即**删表重建**(账本可从 JSONL 再生,所以无所谓)。布局**不可再生**,
//!   混进去等于给它装了个自毁开关。
//! - **版本不匹配不重建**:见 [`SCHEMA_VERSION`] 的注释。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context as _, Result};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use mt_config::{AppConfig, SavedPane, SavedProjectLayout, SavedSplitNode, SavedTab};
use mt_identity::{
    ExecutionHostId, HostInstallId, PaneKey, RepoId, TabId, TerminalIncarnationId,
    TerminalSessionId, WorktreeId,
};

/// 布局库 schema 版本。
///
/// ⚠️ 与 `usage.db` 相反:**版本不匹配绝不删表重建**。账本是 JSONL 的派生缓存,
/// 重建只是多跑一次 backfill;布局是第一手数据,重建即用户资产蒸发。
/// 加字段一律走 `CREATE TABLE IF NOT EXISTS` + `ALTER TABLE ADD COLUMN` 的加法路线,
/// 读到**更高**的版本号(用户装过新版又降级回来)也照常读写 —— kv 表天生向前兼容,
/// 不认识的 key 原样留着,新版装回去还在。
const SCHEMA_VERSION: i64 = 3;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS app_layout (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS project_layout (
  project_id    TEXT PRIMARY KEY,
  layout_json   TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS project_worktree_binding (
  project_id              TEXT PRIMARY KEY,
  execution_host_id       TEXT NOT NULL,
  repo_id                 TEXT NOT NULL,
  worktree_id             TEXT NOT NULL,
  identity_source         TEXT NOT NULL,
  canonical_worktree_path TEXT,
  identity_context        TEXT,
  updated_at_ms           INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_project_worktree_binding_worktree
  ON project_worktree_binding(worktree_id);
CREATE TABLE IF NOT EXISTS worktree_layout (
  worktree_id    TEXT PRIMARY KEY,
  layout_json   TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL
);
";

/// `meta` 表里记「已从 config.json 灌过一次」的键。存在即不再迁移 ——
/// 否则用户清空布局后重启,又会被旧 config.json 里的残留复活。
const META_MIGRATED: &str = "config_migrated";
const META_SCHEMA_VERSION: &str = "schema_version";
const META_LOCAL_HOST_INSTALL_ID: &str = "local_host_install_id";

const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";
const MAX_SALVAGE_JSON_BYTES: usize = 8 * 1024 * 1024;
const MAX_SALVAGE_TABS: usize = 512;
const MAX_SALVAGE_DEPTH: usize = 64;
const MAX_SALVAGE_CHILDREN: usize = 256;
const MAX_SALVAGE_PANES_PER_LEAF: usize = 256;

// `app_layout` 的键名。与 config.json 里的 camelCase 键同名,便于对着旧文件排查。
const KEY_LAYOUT_SIZES: &str = "layoutSizes";
const KEY_MIDDLE_COLUMN_SIZES: &str = "middleColumnSizes";
const KEY_MIDDLE_COLUMN_VISIBLE: &str = "middleColumnVisible";
const KEY_RIGHT_DRAWER_WIDTH: &str = "rightDrawerWidth";
const KEY_WINDOW: &str = "window";
// GPUI 版新增的键(旧 config.json 里没有对应物),沿用 camelCase 命名口径。
const KEY_TERMINALS_PANEL_VISIBLE: &str = "terminalsPanelVisible";

/// 窗口的开合状态。gpui 的 `WindowBounds` 三个变体的镜像 —— 那个类型不实现
/// serde,而本 crate 刻意**不依赖 gpui**(布局存储不该把整个 GPU 栈拖进来,
/// 也才好脱离窗口跑单测),故在这里复刻一份最小形状,转换住在 `mt-app` 侧。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowMode {
    Windowed,
    Maximized,
    Fullscreen,
}

/// 窗口几何。`x/y/width/height` 恒为**还原尺寸**(最大化/全屏时也存还原后的框),
/// 与 gpui `WindowBounds` 各变体内附的 bounds 同一语义:退出时最大化,下次启动
/// 直接最大化打开,而用户按「还原」时拿到的仍是最后一次窗口态的大小。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowGeometry {
    pub mode: WindowMode,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl WindowGeometry {
    /// 明显不可用的几何(尺寸为 0/负数/NaN,或小得放不下任何内容)直接判废,
    /// 由调用方回落默认居中窗口。**不校验是否落在某块屏幕内** —— 那要问平台
    /// 拿显示器列表,是 `mt-app` 的活(见 `main.rs` 的 `restore_window_bounds`)。
    pub fn is_sane(&self) -> bool {
        const MIN_SIDE: f64 = 200.0;
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite())
            && self.width >= MIN_SIDE
            && self.height >= MIN_SIDE
    }
}

/// 全局(非项目级)布局项。每一项都是 `Option`:`None` = 库里没有这个键,
/// 由调用方沿用自己的默认值,**不是**「用户显式设成了默认值」。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GlobalLayout {
    /// 三栏比例(左 / 中 / 右)。
    pub layout_sizes: Option<Vec<f64>>,
    /// 中栏内部(文件树 / 会话列表)的比例。
    pub middle_column_sizes: Option<Vec<f64>>,
    pub middle_column_visible: Option<bool>,
    pub right_drawer_width: Option<f64>,
    /// 终端区右缘「终端列表」竖条面板的显隐(GPUI 版新增,无旧 config 对应物)。
    pub terminals_panel_visible: Option<bool>,
    pub window: Option<WindowGeometry>,
}

/// Compatibility project registration projected onto stable worktree identity.
///
/// `identity_source` is deliberately persisted as an opaque string. Resolution
/// authority belongs to `mt-project`; this crate only stores and returns the
/// source marker without depending on the resolver layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectWorktreeBinding {
    pub project_id: String,
    pub execution_host_id: ExecutionHostId,
    pub repo_id: RepoId,
    pub worktree_id: WorktreeId,
    pub identity_source: String,
    pub canonical_worktree_path: Option<String>,
    /// Opaque resolver-owned provenance. `mt-layout` persists it but never
    /// interprets it; missing values remain backward compatible.
    pub identity_context: Option<String>,
}

/// Startup projection returned after bindings and layouts have been reconciled
/// in one transaction. Layouts remain keyed by compatibility project ID while
/// callers migrate their in-memory ownership to `WorktreeId`.
#[derive(Debug, Clone, Default)]
pub struct ReconciledProjectLayouts {
    pub layouts: HashMap<String, SavedProjectLayout>,
    pub bindings: HashMap<String, ProjectWorktreeBinding>,
}

impl GlobalLayout {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// `layout.db` 的读写口。
///
/// 持有一条常开连接(WAL + 5s busy_timeout)。写者只有 UI 线程一个,读也只在启动
/// 时发生一次,`Mutex<Connection>` 足够;没有 `mt-usage` 那套同步合并的必要。
pub struct LayoutStore {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl LayoutStore {
    /// `{dir}/layout.db`。目录不存在就建出来。
    ///
    /// 明确不是 SQLite 的文件会挪成 `layout.db.corrupt` 留证并重建空库。
    ///
    /// 一个可读 SQLite 库若只是带有本程序不认识或不兼容的更新 schema,错误会
    /// 原样上抛且文件保持不动。schema mismatch 不能被误判成 corruption,否则
    /// 降级启动一次就会把新版仍可恢复的数据整体隔离掉。
    pub fn open_at(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir)
            .with_context(|| format!("创建应用数据目录失败: {}", dir.display()))?;
        let path = dir.join("layout.db");
        if definitely_not_sqlite(&path)? {
            let corrupt = path.with_extension("db.corrupt");
            let _ = fs::remove_file(&corrupt);
            fs::rename(&path, &corrupt).with_context(|| {
                format!(
                    "布局库不是 SQLite,但无法挪至留证文件: {} -> {}",
                    path.display(),
                    corrupt.display()
                )
            })?;
            eprintln!(
                "[layout] {} 不是有效 SQLite 文件,已挪至 {} 并重建空库",
                path.display(),
                corrupt.display()
            );
        }
        Self::try_open(&path)
    }

    fn try_open(path: &Path) -> Result<Self> {
        let mut conn = Connection::open(path)
            .with_context(|| format!("打开布局库失败: {}", path.display()))?;
        conn.busy_timeout(Duration::from_millis(5000))?;
        // journal_mode 是有返回行的语句,得走 query_row。转不过去(比如另一实例
        // 正握着这个库)不算失败:退回默认的 delete 模式照样能读写,只是少了
        // 读写不互阻的好处。
        let _ = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0));
        // 默认阈值是 1000 页(约 4 MB)——对一个稳态只有几 KB 的库来说,意味着
        // WAL 能长到主库的上千倍才回收一次。实测首启迁移 44 个项目后 WAL 就有
        // 450 KB 而主库 4 KB;进程被强杀时这个 WAL 会一直躺在数据目录里。
        // 32 页(约 128 KB)对布局这种小步快写的负载足够摊薄 fsync。
        let _ = conn.execute_batch("PRAGMA wal_autocheckpoint=32");
        let tx = conn.transaction()?;
        tx.execute_batch(SCHEMA)
            .with_context(|| format!("增量升级布局库 schema 失败: {}", path.display()))?;
        ensure_project_binding_identity_context_column(&tx)
            .with_context(|| format!("升级项目绑定 provenance 失败: {}", path.display()))?;
        tx.commit()?;

        let store = Self {
            conn: Mutex::new(conn),
            path: path.to_path_buf(),
        };
        store.check_schema_version();
        Ok(store)
    }

    /// 版本只记录、不裁决(见 [`SCHEMA_VERSION`])。读到更高的版本只打一行日志,
    /// 并且**不把它降回去** —— 用户切回新版时不该发现自己的库被降级标记过。
    fn check_schema_version(&self) {
        let found = self
            .meta_get(META_SCHEMA_VERSION)
            .and_then(|v| v.parse::<i64>().ok());
        match found {
            Some(v) if v > SCHEMA_VERSION => {
                eprintln!("[layout] 库版本 {v} 高于本程序的 {SCHEMA_VERSION},按兼容模式读写");
            }
            Some(v) if v == SCHEMA_VERSION => {}
            _ => {
                self.meta_set(META_SCHEMA_VERSION, &SCHEMA_VERSION.to_string());
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    // ─── meta ────────────────────────────────────────────────────────────

    fn meta_get(&self, key: &str) -> Option<String> {
        let conn = self.conn.lock().ok()?;
        conn.query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
            r.get::<_, String>(0)
        })
        .optional()
        .ok()
        .flatten()
    }

    fn meta_set(&self, key: &str, value: &str) {
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "INSERT INTO meta(key, value) VALUES(?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            );
        }
    }

    /// 是否还没从 config.json 灌过。首启(空库)返回 true。
    pub fn needs_config_migration(&self) -> bool {
        self.meta_get(META_MIGRATED).is_none()
    }

    /// Return the installation-scoped host ID, creating it exactly once.
    ///
    /// An immediate transaction serializes the read/create sequence across two
    /// application processes that happen to start against the same data dir.
    pub fn local_host_install_id(&self) -> Result<HostInstallId> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("布局库锁中毒"))?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stored = tx
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![META_LOCAL_HOST_INSTALL_ID],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        match stored.as_deref().map(HostInstallId::from_str) {
            Some(Ok(id)) => {
                tx.commit()?;
                return Ok(id);
            }
            Some(Err(_)) => eprintln!("[layout] 本地安装 ID 无效,已重新生成"),
            None => {}
        }

        let id = HostInstallId::new();
        tx.execute(
            "INSERT INTO meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![META_LOCAL_HOST_INSTALL_ID, id.as_str()],
        )?;
        tx.commit()?;
        Ok(id)
    }

    // ─── 全局布局项 ──────────────────────────────────────────────────────

    /// 读全部全局项。库里没有 / 值解析不出来的键一律当 `None`
    /// ——手改坏一个键不该让整份布局读不出来。
    pub fn load_globals(&self) -> GlobalLayout {
        let Ok(conn) = self.conn.lock() else {
            return GlobalLayout::default();
        };
        let mut map: HashMap<String, String> = HashMap::new();
        if let Ok(mut stmt) = conn.prepare("SELECT key, value FROM app_layout") {
            if let Ok(rows) =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            {
                for (key, value) in rows.flatten() {
                    map.insert(key, value);
                }
            }
        }
        GlobalLayout {
            layout_sizes: from_kv(&map, KEY_LAYOUT_SIZES),
            middle_column_sizes: from_kv(&map, KEY_MIDDLE_COLUMN_SIZES),
            middle_column_visible: from_kv(&map, KEY_MIDDLE_COLUMN_VISIBLE),
            right_drawer_width: from_kv(&map, KEY_RIGHT_DRAWER_WIDTH),
            terminals_panel_visible: from_kv(&map, KEY_TERMINALS_PANEL_VISIBLE),
            window: from_kv(&map, KEY_WINDOW),
        }
    }

    /// 整体写回全局项。`None` 的字段**保持库里原样**(不删除)——
    /// 调用方通常只改了其中一项,不该因为没填其余字段就把它们抹掉。
    pub fn save_globals(&self, globals: &GlobalLayout) -> Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("布局库锁中毒"))?;
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO app_layout(key, value) VALUES(?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            )?;
            let mut put = |key: &str, value: Option<String>| -> Result<()> {
                if let Some(v) = value {
                    stmt.execute(params![key, v])?;
                }
                Ok(())
            };
            put(KEY_LAYOUT_SIZES, to_json(&globals.layout_sizes))?;
            put(
                KEY_MIDDLE_COLUMN_SIZES,
                to_json(&globals.middle_column_sizes),
            )?;
            put(
                KEY_MIDDLE_COLUMN_VISIBLE,
                to_json(&globals.middle_column_visible),
            )?;
            put(KEY_RIGHT_DRAWER_WIDTH, to_json(&globals.right_drawer_width))?;
            put(
                KEY_TERMINALS_PANEL_VISIBLE,
                to_json(&globals.terminals_panel_visible),
            )?;
            put(KEY_WINDOW, to_json(&globals.window))?;
        }
        tx.commit()?;
        Ok(())
    }

    // ─── Worktree identity and project layout compatibility ──────────────

    /// Read all valid persisted project bindings. A malformed row is isolated
    /// to that project so startup can still reuse every other known binding.
    pub fn load_project_bindings(&self) -> Result<HashMap<String, ProjectWorktreeBinding>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("布局库锁中毒"))?;
        load_project_bindings_from(&conn)
    }

    /// Reconcile the caller's desired project bindings and migrate layouts in
    /// one transaction.
    ///
    /// An existing destination worktree row always wins. If it is absent, a
    /// prior bound worktree row is copied before falling back to the project's
    /// legacy mirror. Alias groups select one newest candidate per source tier,
    /// with project ID as the stable tie-breaker, then project that layout to
    /// every alias. Source rows are never deleted by reconciliation.
    pub fn reconcile_worktree_layouts(
        &self,
        desired_bindings: &[ProjectWorktreeBinding],
        now_ms: i64,
    ) -> Result<ReconciledProjectLayouts> {
        let mut seen_projects = HashSet::new();
        let mut bindings_by_worktree = BTreeMap::<WorktreeId, Vec<&ProjectWorktreeBinding>>::new();
        for binding in desired_bindings {
            if !seen_projects.insert(binding.project_id.as_str()) {
                anyhow::bail!("重复的项目 worktree 绑定: {}", binding.project_id);
            }
            bindings_by_worktree
                .entry(binding.worktree_id.clone())
                .or_default()
                .push(binding);
        }
        for bindings in bindings_by_worktree.values_mut() {
            bindings.sort_by(|left, right| left.project_id.cmp(&right.project_id));
        }

        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("布局库锁中毒"))?;
        let tx = conn.transaction()?;
        let mut reconciled = ReconciledProjectLayouts::default();

        let mut reconciliation_groups = Vec::with_capacity(bindings_by_worktree.len());
        for (worktree_id, bindings) in bindings_by_worktree {
            let candidate = select_reconciliation_candidate(&tx, &worktree_id, &bindings)?;
            reconciliation_groups.push((worktree_id, bindings, candidate));
        }

        for (worktree_id, bindings, candidate) in reconciliation_groups {
            if let Some(candidate) = candidate {
                match decode_saved_layout(&candidate.row.layout_json, Some(&worktree_id)) {
                    Ok(decoded) => {
                        if decoded.repaired {
                            eprintln!(
                                "[layout] worktree {} 从 {} 修复布局: {}",
                                worktree_id,
                                candidate.label(),
                                decoded.stats.summary()
                            );
                        }
                        upsert_worktree_layout_if_changed(
                            &tx,
                            &worktree_id,
                            &decoded.normalized_json,
                            now_ms,
                        )?;
                        for binding in &bindings {
                            let selected_legacy_owner = candidate.legacy_owner();
                            let preserve_conflicting_legacy = bindings.len() > 1
                                && selected_legacy_owner != Some(binding.project_id.as_str())
                                && load_legacy_layout_row(&tx, &binding.project_id)?.is_some_and(
                                    |legacy| legacy.layout_json != decoded.normalized_json,
                                );
                            if preserve_conflicting_legacy {
                                eprintln!(
                                    "[layout] 项目 {} 与共享 worktree {} 的 legacy 布局冲突; 保留 legacy 行并使用已选 worktree 布局",
                                    binding.project_id, worktree_id
                                );
                            } else {
                                upsert_legacy_layout_if_changed(
                                    &tx,
                                    &binding.project_id,
                                    &decoded.normalized_json,
                                    now_ms,
                                )?;
                            }
                            reconciled
                                .layouts
                                .insert(binding.project_id.clone(), decoded.layout.clone());
                        }
                    }
                    Err(error) => eprintln!(
                        "[layout] worktree {} 的 {} 布局无法恢复,该行保持原样: {}",
                        worktree_id,
                        candidate.label(),
                        error
                    ),
                }
            }

            for binding in bindings {
                upsert_project_binding(&tx, binding, now_ms)?;
                reconciled
                    .bindings
                    .insert(binding.project_id.clone(), binding.clone());
            }
        }

        tx.commit()?;
        Ok(reconciled)
    }

    /// Atomically persist the stable worktree layout and the rollback mirror.
    /// An empty layout explicitly removes both rows but keeps the binding.
    pub fn save_worktree_layout(
        &self,
        binding: &ProjectWorktreeBinding,
        layout: &SavedProjectLayout,
        now_ms: i64,
    ) -> Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("布局库锁中毒"))?;
        let tx = conn.transaction()?;
        save_bound_layout_in(&tx, binding, layout, now_ms)?;
        tx.commit()?;
        Ok(())
    }

    /// Compatibility reader. Bound projects prefer `worktree_layout`; unbound
    /// projects continue to load their legacy rows. A malformed row remains
    /// isolated and does not force fallback from an existing destination row.
    pub fn load_project_layouts(&self) -> HashMap<String, SavedProjectLayout> {
        let mut out = HashMap::new();
        let Ok(conn) = self.conn.lock() else {
            return out;
        };
        let bindings = match load_project_bindings_from(&conn) {
            Ok(bindings) => bindings,
            Err(error) => {
                eprintln!("[layout] 读取项目 worktree 绑定失败: {error:#}");
                HashMap::new()
            }
        };
        let bound_projects: HashSet<String> = bindings.keys().cloned().collect();

        for (project_id, binding) in bindings {
            let row = match load_worktree_layout_row(&conn, &binding.worktree_id) {
                Ok(Some(row)) => Some(row),
                Ok(None) => match load_legacy_layout_row(&conn, &project_id) {
                    Ok(row) => row,
                    Err(error) => {
                        eprintln!("[layout] 项目 {project_id} 的 legacy 布局读取失败: {error:#}");
                        None
                    }
                },
                Err(error) => {
                    eprintln!("[layout] 项目 {project_id} 的 worktree 布局读取失败: {error:#}");
                    None
                }
            };
            let Some(row) = row else {
                continue;
            };
            match decode_saved_layout(&row.layout_json, Some(&binding.worktree_id)) {
                Ok(decoded) => {
                    out.insert(project_id, decoded.layout);
                }
                Err(error) => {
                    eprintln!("[layout] 项目 {project_id} 的布局解析失败,已跳过: {error}")
                }
            }
        }

        match load_all_legacy_layout_rows(&conn) {
            Ok(rows) => {
                for (project_id, row) in rows {
                    if bound_projects.contains(&project_id) {
                        continue;
                    }
                    match decode_saved_layout(&row.layout_json, None) {
                        Ok(decoded) => {
                            out.insert(project_id, decoded.layout);
                        }
                        Err(error) => eprintln!(
                            "[layout] 未绑定项目 {project_id} 的布局解析失败,已跳过: {error}"
                        ),
                    }
                }
            }
            Err(error) => eprintln!("[layout] 读取 legacy 项目布局失败: {error:#}"),
        }
        out
    }

    /// Compatibility writer. Once a binding exists it uses the stable
    /// dual-write path; otherwise it retains the legacy-only behavior.
    pub fn save_project_layout(
        &self,
        project_id: &str,
        layout: &SavedProjectLayout,
        now_ms: i64,
    ) -> Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("布局库锁中毒"))?;
        let tx = conn.transaction()?;
        if let Some(binding) = load_project_binding_from(&tx, project_id)? {
            save_bound_layout_in(&tx, &binding, layout, now_ms)?;
        } else if layout.tabs.is_empty() {
            delete_legacy_layout(&tx, project_id)?;
        } else {
            let mut normalized = layout.clone();
            let mut stats = SalvageStats::default();
            normalize_saved_layout_stable_ids(&mut normalized, None, &mut stats);
            if normalized.tabs.is_empty() {
                anyhow::bail!("项目 {project_id} 的非空布局没有可持久化 pane");
            }
            let json = serde_json::to_string(&normalized)?;
            upsert_legacy_layout(&tx, project_id, &json, now_ms)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Delete layout contents while retaining any project binding. If a valid
    /// binding exists, both the worktree row and this project's mirror go away.
    pub fn delete_project_layout(&self, project_id: &str) -> Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("布局库锁中毒"))?;
        let tx = conn.transaction()?;
        if let Some(binding) = load_project_binding_from(&tx, project_id)? {
            delete_worktree_layout(&tx, &binding.worktree_id)?;
        }
        delete_legacy_layout(&tx, project_id)?;
        tx.commit()?;
        Ok(())
    }

    /// Remove a compatibility project registration and its rollback mirror.
    /// The worktree row is intentionally retained for later re-registration.
    pub fn delete_project_binding(&self, project_id: &str) -> Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("布局库锁中毒"))?;
        let tx = conn.transaction()?;
        delete_project_binding_in(&tx, project_id)?;
        tx.commit()?;
        Ok(())
    }

    /// Retain only live project registrations and mirrors. Orphan worktree
    /// layouts are recoverable data and are never collected here.
    pub fn retain_project_bindings(&self, live_project_ids: &HashSet<String>) -> Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("布局库锁中毒"))?;
        let tx = conn.transaction()?;
        let stale = load_registered_project_ids(&tx)?
            .into_iter()
            .filter(|project_id| !live_project_ids.contains(project_id))
            .collect::<Vec<_>>();
        for project_id in stale {
            delete_project_binding_in(&tx, &project_id)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Backward-compatible name retained for current callers.
    pub fn retain_projects(&self, live: &HashSet<String>) -> Result<()> {
        self.retain_project_bindings(live)
    }

    // ─── 从 config.json 一次性迁移 ───────────────────────────────────────

    /// 把存量 `config.json` 里的布局灌进本库,并打上「已迁移」标记。
    ///
    /// 只在 [`needs_config_migration`](Self::needs_config_migration) 为真时调用。
    /// 幂等靠 meta 标记而不是「库里有没有数据」:用户把所有终端关光后重启,
    /// 库是空的但迁移确实做过,按后者判会把旧布局又灌回来。
    ///
    /// 返回灌进去的项目数。
    pub fn migrate_from_config(&self, config: &AppConfig) -> Result<usize> {
        let globals = GlobalLayout {
            layout_sizes: config.layout_sizes.clone(),
            middle_column_sizes: config.middle_column_sizes.clone(),
            // 这个字段在 config 里是裸 bool(默认 true),分不出「用户设过 true」
            // 与「从来没设过」。一律搬:值本身就是当前生效的那个,搬过来语义不变。
            middle_column_visible: Some(config.middle_column_visible),
            right_drawer_width: config.right_drawer_width,
            // 终端列表竖条与窗口几何都是 GPUI 版新加的能力,旧 config.json 里
            // 没有对应物(窗口几何在 Tauri 版存在另一个文件 `.window-state.json`,
            // 格式不兼容,不迁)
            terminals_panel_visible: None,
            window: None,
        };
        if !globals.is_empty() {
            self.save_globals(&globals)?;
        }

        let mut count = 0usize;
        for project in &config.projects {
            let Some(layout) = project.saved_layout.as_ref() else {
                continue;
            };
            if layout.tabs.is_empty() {
                continue;
            }
            self.save_project_layout(&project.id, layout, 0)?;
            count += 1;
        }
        self.meta_set(META_MIGRATED, "1");
        Ok(count)
    }
}

#[derive(Debug)]
struct RawProjectWorktreeBinding {
    project_id: String,
    execution_host_id: String,
    repo_id: String,
    worktree_id: String,
    identity_source: String,
    canonical_worktree_path: Option<String>,
    identity_context: Option<String>,
}

impl RawProjectWorktreeBinding {
    fn parse(self) -> std::result::Result<ProjectWorktreeBinding, &'static str> {
        if self.project_id.is_empty() {
            return Err("project_id");
        }
        if self.identity_source.trim().is_empty() {
            return Err("identity_source");
        }
        let execution_host_id =
            ExecutionHostId::from_str(&self.execution_host_id).map_err(|_| "execution_host_id")?;
        let repo_id = RepoId::from_str(&self.repo_id).map_err(|_| "repo_id")?;
        let worktree_id = WorktreeId::from_str(&self.worktree_id).map_err(|_| "worktree_id")?;
        Ok(ProjectWorktreeBinding {
            project_id: self.project_id,
            execution_host_id,
            repo_id,
            worktree_id,
            identity_source: self.identity_source,
            canonical_worktree_path: self.canonical_worktree_path,
            identity_context: self.identity_context,
        })
    }
}

#[derive(Debug, Clone)]
struct StoredLayoutRow {
    layout_json: String,
    updated_at_ms: i64,
}

#[derive(Debug, Clone, Copy)]
enum LayoutRowSource {
    Destination,
    PreviousBinding,
    LegacyProject,
}

impl LayoutRowSource {
    fn label(self) -> &'static str {
        match self {
            Self::Destination => "目标 worktree",
            Self::PreviousBinding => "旧绑定 worktree",
            Self::LegacyProject => "legacy project",
        }
    }
}

#[derive(Debug)]
struct ReconciliationCandidate {
    source: LayoutRowSource,
    owner_project_id: Option<String>,
    source_key: String,
    row: StoredLayoutRow,
}

impl ReconciliationCandidate {
    fn label(&self) -> String {
        match self.owner_project_id.as_deref() {
            Some(project_id) => format!("{} ({project_id})", self.source.label()),
            None => self.source.label().to_string(),
        }
    }

    fn legacy_owner(&self) -> Option<&str> {
        if matches!(self.source, LayoutRowSource::LegacyProject) {
            self.owner_project_id.as_deref()
        } else {
            None
        }
    }
}

fn select_reconciliation_candidate(
    conn: &Connection,
    worktree_id: &WorktreeId,
    bindings: &[&ProjectWorktreeBinding],
) -> Result<Option<ReconciliationCandidate>> {
    if let Some(row) = load_worktree_layout_row(conn, worktree_id)? {
        return Ok(Some(ReconciliationCandidate {
            source: LayoutRowSource::Destination,
            owner_project_id: None,
            source_key: worktree_id.as_str().to_string(),
            row,
        }));
    }

    let mut previous_candidates = Vec::new();
    for binding in bindings {
        let Some(previous) = load_project_binding_from(conn, &binding.project_id)? else {
            continue;
        };
        if previous.worktree_id.as_str() == worktree_id.as_str() {
            continue;
        }
        let Some(row) = load_worktree_layout_row(conn, &previous.worktree_id)? else {
            continue;
        };
        previous_candidates.push(ReconciliationCandidate {
            source: LayoutRowSource::PreviousBinding,
            owner_project_id: Some(binding.project_id.clone()),
            source_key: previous.worktree_id.as_str().to_string(),
            row,
        });
    }
    if let Some(candidate) = newest_reconciliation_candidate(previous_candidates) {
        return Ok(Some(candidate));
    }

    let mut legacy_candidates = Vec::new();
    for binding in bindings {
        let Some(row) = load_legacy_layout_row(conn, &binding.project_id)? else {
            continue;
        };
        legacy_candidates.push(ReconciliationCandidate {
            source: LayoutRowSource::LegacyProject,
            owner_project_id: Some(binding.project_id.clone()),
            source_key: binding.project_id.clone(),
            row,
        });
    }
    Ok(newest_reconciliation_candidate(legacy_candidates))
}

fn newest_reconciliation_candidate(
    mut candidates: Vec<ReconciliationCandidate>,
) -> Option<ReconciliationCandidate> {
    candidates.sort_by(|left, right| {
        right
            .row
            .updated_at_ms
            .cmp(&left.row.updated_at_ms)
            .then_with(|| left.owner_project_id.cmp(&right.owner_project_id))
            .then_with(|| left.source_key.cmp(&right.source_key))
    });
    candidates.into_iter().next()
}

fn ensure_project_binding_identity_context_column(conn: &Connection) -> Result<()> {
    let exists = conn
        .query_row(
            "SELECT 1
             FROM pragma_table_info('project_worktree_binding')
             WHERE name = 'identity_context'
             LIMIT 1",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        conn.execute_batch(
            "ALTER TABLE project_worktree_binding
             ADD COLUMN identity_context TEXT",
        )?;
    }
    Ok(())
}

fn definitely_not_sqlite(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let mut file = fs::File::open(path)
        .with_context(|| format!("读取布局库文件头失败: {}", path.display()))?;
    if file.metadata()?.len() == 0 {
        return Ok(false);
    }
    let mut header = [0u8; SQLITE_HEADER.len()];
    let read = file.read(&mut header)?;
    Ok(read != SQLITE_HEADER.len() || header.as_slice() != SQLITE_HEADER)
}

fn raw_binding_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawProjectWorktreeBinding> {
    Ok(RawProjectWorktreeBinding {
        project_id: row.get(0)?,
        execution_host_id: row.get(1)?,
        repo_id: row.get(2)?,
        worktree_id: row.get(3)?,
        identity_source: row.get(4)?,
        canonical_worktree_path: row.get(5)?,
        identity_context: row.get(6)?,
    })
}

fn load_project_bindings_from(
    conn: &Connection,
) -> Result<HashMap<String, ProjectWorktreeBinding>> {
    let mut stmt = conn.prepare(
        "SELECT project_id, execution_host_id, repo_id, worktree_id,
                identity_source, canonical_worktree_path, identity_context
         FROM project_worktree_binding",
    )?;
    let rows = stmt.query_map([], raw_binding_from_row)?;
    let mut bindings = HashMap::new();
    for raw in rows {
        let raw = raw?;
        let project_id = raw.project_id.clone();
        match raw.parse() {
            Ok(binding) => {
                bindings.insert(project_id, binding);
            }
            Err(field) => {
                eprintln!("[layout] 项目 {project_id} 的持久化绑定字段 {field} 无效,已跳过该绑定")
            }
        }
    }
    Ok(bindings)
}

fn load_project_binding_from(
    conn: &Connection,
    project_id: &str,
) -> Result<Option<ProjectWorktreeBinding>> {
    let raw = conn
        .query_row(
            "SELECT project_id, execution_host_id, repo_id, worktree_id,
                    identity_source, canonical_worktree_path, identity_context
             FROM project_worktree_binding WHERE project_id = ?1",
            params![project_id],
            raw_binding_from_row,
        )
        .optional()?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    match raw.parse() {
        Ok(binding) => Ok(Some(binding)),
        Err(field) => {
            eprintln!("[layout] 项目 {project_id} 的持久化绑定字段 {field} 无效,按未绑定处理");
            Ok(None)
        }
    }
}

fn upsert_project_binding(
    conn: &Connection,
    binding: &ProjectWorktreeBinding,
    now_ms: i64,
) -> Result<()> {
    if binding.project_id.is_empty() {
        anyhow::bail!("project_worktree_binding.project_id 不能为空");
    }
    if binding.identity_source.trim().is_empty() {
        anyhow::bail!(
            "项目 {} 的 project_worktree_binding.identity_source 不能为空",
            binding.project_id
        );
    }
    conn.execute(
        "INSERT INTO project_worktree_binding(
           project_id, execution_host_id, repo_id, worktree_id, identity_source,
           canonical_worktree_path, identity_context, updated_at_ms
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(project_id) DO UPDATE SET
           execution_host_id = excluded.execution_host_id,
           repo_id = excluded.repo_id,
           worktree_id = excluded.worktree_id,
           identity_source = excluded.identity_source,
           canonical_worktree_path = excluded.canonical_worktree_path,
           identity_context = excluded.identity_context,
           updated_at_ms = excluded.updated_at_ms
         WHERE project_worktree_binding.execution_host_id IS NOT excluded.execution_host_id
            OR project_worktree_binding.repo_id IS NOT excluded.repo_id
            OR project_worktree_binding.worktree_id IS NOT excluded.worktree_id
            OR project_worktree_binding.identity_source IS NOT excluded.identity_source
            OR project_worktree_binding.canonical_worktree_path
               IS NOT excluded.canonical_worktree_path
            OR project_worktree_binding.identity_context
               IS NOT excluded.identity_context",
        params![
            binding.project_id.as_str(),
            binding.execution_host_id.as_str(),
            binding.repo_id.as_str(),
            binding.worktree_id.as_str(),
            binding.identity_source.as_str(),
            binding.canonical_worktree_path.as_deref(),
            binding.identity_context.as_deref(),
            now_ms,
        ],
    )?;
    Ok(())
}

fn load_worktree_layout_row(
    conn: &Connection,
    worktree_id: &WorktreeId,
) -> Result<Option<StoredLayoutRow>> {
    conn.query_row(
        "SELECT layout_json, updated_at_ms FROM worktree_layout WHERE worktree_id = ?1",
        params![worktree_id.as_str()],
        |row| {
            Ok(StoredLayoutRow {
                layout_json: row.get(0)?,
                updated_at_ms: row.get(1)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn load_legacy_layout_row(conn: &Connection, project_id: &str) -> Result<Option<StoredLayoutRow>> {
    conn.query_row(
        "SELECT layout_json, updated_at_ms FROM project_layout WHERE project_id = ?1",
        params![project_id],
        |row| {
            Ok(StoredLayoutRow {
                layout_json: row.get(0)?,
                updated_at_ms: row.get(1)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn load_all_legacy_layout_rows(conn: &Connection) -> Result<Vec<(String, StoredLayoutRow)>> {
    let mut stmt =
        conn.prepare("SELECT project_id, layout_json, updated_at_ms FROM project_layout")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            StoredLayoutRow {
                layout_json: row.get(1)?,
                updated_at_ms: row.get(2)?,
            },
        ))
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn upsert_worktree_layout(
    conn: &Connection,
    worktree_id: &WorktreeId,
    layout_json: &str,
    now_ms: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO worktree_layout(worktree_id, layout_json, updated_at_ms)
         VALUES(?1, ?2, ?3)
         ON CONFLICT(worktree_id) DO UPDATE SET
           layout_json = excluded.layout_json,
           updated_at_ms = excluded.updated_at_ms",
        params![worktree_id.as_str(), layout_json, now_ms],
    )?;
    Ok(())
}

fn upsert_worktree_layout_if_changed(
    conn: &Connection,
    worktree_id: &WorktreeId,
    layout_json: &str,
    now_ms: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO worktree_layout(worktree_id, layout_json, updated_at_ms)
         VALUES(?1, ?2, ?3)
         ON CONFLICT(worktree_id) DO UPDATE SET
           layout_json = excluded.layout_json,
           updated_at_ms = excluded.updated_at_ms
         WHERE worktree_layout.layout_json IS NOT excluded.layout_json",
        params![worktree_id.as_str(), layout_json, now_ms],
    )?;
    Ok(())
}

fn upsert_legacy_layout(
    conn: &Connection,
    project_id: &str,
    layout_json: &str,
    now_ms: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO project_layout(project_id, layout_json, updated_at_ms)
         VALUES(?1, ?2, ?3)
         ON CONFLICT(project_id) DO UPDATE SET
           layout_json = excluded.layout_json,
           updated_at_ms = excluded.updated_at_ms",
        params![project_id, layout_json, now_ms],
    )?;
    Ok(())
}

fn upsert_legacy_layout_if_changed(
    conn: &Connection,
    project_id: &str,
    layout_json: &str,
    now_ms: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO project_layout(project_id, layout_json, updated_at_ms)
         VALUES(?1, ?2, ?3)
         ON CONFLICT(project_id) DO UPDATE SET
           layout_json = excluded.layout_json,
           updated_at_ms = excluded.updated_at_ms
         WHERE project_layout.layout_json IS NOT excluded.layout_json",
        params![project_id, layout_json, now_ms],
    )?;
    Ok(())
}

fn delete_worktree_layout(conn: &Connection, worktree_id: &WorktreeId) -> Result<()> {
    conn.execute(
        "DELETE FROM worktree_layout WHERE worktree_id = ?1",
        params![worktree_id.as_str()],
    )?;
    Ok(())
}

fn delete_legacy_layout(conn: &Connection, project_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM project_layout WHERE project_id = ?1",
        params![project_id],
    )?;
    Ok(())
}

fn delete_project_binding_in(conn: &Connection, project_id: &str) -> Result<()> {
    delete_legacy_layout(conn, project_id)?;
    conn.execute(
        "DELETE FROM project_worktree_binding WHERE project_id = ?1",
        params![project_id],
    )?;
    Ok(())
}

fn load_registered_project_ids(conn: &Connection) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT project_id FROM project_worktree_binding
         UNION
         SELECT project_id FROM project_layout",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect::<rusqlite::Result<HashSet<_>>>()
        .map_err(Into::into)
}

fn save_bound_layout_in(
    conn: &Connection,
    binding: &ProjectWorktreeBinding,
    layout: &SavedProjectLayout,
    now_ms: i64,
) -> Result<()> {
    let explicitly_empty = layout.tabs.is_empty();
    let mut normalized = layout.clone();
    let mut stats = SalvageStats::default();
    normalize_saved_layout_stable_ids(&mut normalized, Some(&binding.worktree_id), &mut stats);

    if normalized.tabs.is_empty() {
        if !explicitly_empty {
            anyhow::bail!("项目 {} 的非空布局没有可持久化 pane", binding.project_id);
        }
        delete_worktree_layout(conn, &binding.worktree_id)?;
        delete_legacy_layout(conn, &binding.project_id)?;
        upsert_project_binding(conn, binding, now_ms)?;
        return Ok(());
    }

    let json = serde_json::to_string(&normalized)?;
    upsert_worktree_layout(conn, &binding.worktree_id, &json, now_ms)?;
    upsert_legacy_layout(conn, &binding.project_id, &json, now_ms)?;
    upsert_project_binding(conn, binding, now_ms)?;
    Ok(())
}

#[derive(Debug)]
struct DecodedLayout {
    layout: SavedProjectLayout,
    normalized_json: String,
    repaired: bool,
    stats: SalvageStats,
}

#[derive(Debug, Default)]
struct SalvageStats {
    skipped_tabs: usize,
    skipped_panes: usize,
    dropped_nodes: usize,
    collapsed_splits: usize,
    rebuilt_sizes: usize,
    normalized_ids: usize,
}

impl SalvageStats {
    fn summary(&self) -> String {
        format!(
            "跳过 tab {}, pane {}, node {}; 折叠 split {}, 重建 sizes {}, 归一化 ID {}",
            self.skipped_tabs,
            self.skipped_panes,
            self.dropped_nodes,
            self.collapsed_splits,
            self.rebuilt_sizes,
            self.normalized_ids
        )
    }
}

fn decode_saved_layout(
    raw: &str,
    expected_worktree_id: Option<&WorktreeId>,
) -> std::result::Result<DecodedLayout, String> {
    let (mut layout, salvaged, mut stats) = match serde_json::from_str::<SavedProjectLayout>(raw) {
        Ok(layout) => (layout, false, SalvageStats::default()),
        Err(fast_error) => {
            if raw.len() > MAX_SALVAGE_JSON_BYTES {
                return Err(format!(
                    "typed parse 失败且 JSON 超过 salvage 上限({MAX_SALVAGE_JSON_BYTES} bytes)"
                ));
            }
            let value = serde_json::from_str::<Value>(raw).map_err(|syntax_error| {
                format!(
                    "JSON 语法无效(line {}, column {}); typed parse: {}",
                    syntax_error.line(),
                    syntax_error.column(),
                    fast_error
                )
            })?;
            validate_salvage_bounds(&value).map_err(str::to_string)?;
            let mut salvage_stats = SalvageStats::default();
            let layout = salvage_project_layout(&value, &mut salvage_stats)
                .ok_or_else(|| "有效 JSON 中没有可恢复的 layout 结构".to_string())?;
            (layout, true, salvage_stats)
        }
    };

    let had_tabs = !layout.tabs.is_empty();
    let before = serde_json::to_string(&layout).ok();
    normalize_saved_layout_stable_ids(&mut layout, expected_worktree_id, &mut stats);
    if had_tabs && layout.tabs.is_empty() {
        return Err("布局包含 tab,但没有任何可恢复 pane".to_string());
    }
    let normalized_json =
        serde_json::to_string(&layout).map_err(|error| format!("归一化布局无法序列化: {error}"))?;
    let repaired = salvaged || before.as_deref() != Some(normalized_json.as_str());
    Ok(DecodedLayout {
        layout,
        normalized_json,
        repaired,
        stats,
    })
}

fn salvage_project_layout(value: &Value, stats: &mut SalvageStats) -> Option<SavedProjectLayout> {
    let object = value.as_object()?;
    let tabs_value = object.get("tabs")?.as_array()?;

    let mut tabs = Vec::new();
    for value in tabs_value {
        match salvage_tab(value, stats) {
            Some(tab) => tabs.push(tab),
            None => stats.skipped_tabs += 1,
        }
    }
    if !tabs_value.is_empty() && tabs.is_empty() {
        return None;
    }

    let active_tab_index = object
        .get("activeTabIndex")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or_default();
    Some(SavedProjectLayout {
        worktree_id: salvage_optional_id(object.get("worktreeId"), stats),
        active_tab_id: salvage_optional_id(object.get("activeTabId"), stats),
        selected_terminal_pane_key: salvage_optional_id(object.get("selectedTerminalPaneKey"), stats),
        terminal_order: object.get("terminalOrder").and_then(Value::as_array).map(|values| {
            values.iter().filter_map(|value| salvage_optional_id(Some(value), stats)).collect()
        }),
        tabs,
        active_tab_index,
    })
}

fn salvage_tab(value: &Value, stats: &mut SalvageStats) -> Option<SavedTab> {
    let object = value.as_object()?;
    let split_layout = salvage_split_node(object.get("splitLayout")?, 0, stats)?;
    let custom_title = match object.get("customTitle") {
        None | Some(Value::Null) => None,
        Some(Value::String(title)) => Some(title.clone()),
        Some(_) => None,
    };
    Some(SavedTab {
        tab_id: salvage_optional_id(object.get("tabId"), stats),
        custom_title,
        split_layout,
    })
}

fn salvage_split_node(
    value: &Value,
    depth: usize,
    stats: &mut SalvageStats,
) -> Option<SavedSplitNode> {
    if depth >= MAX_SALVAGE_DEPTH {
        stats.dropped_nodes += 1;
        return None;
    }
    let object = value.as_object()?;
    match object.get("type").and_then(Value::as_str)? {
        "leaf" => salvage_leaf(object, stats),
        "split" => salvage_split(object, depth, stats),
        _ => {
            stats.dropped_nodes += 1;
            None
        }
    }
}

fn salvage_leaf(object: &Map<String, Value>, stats: &mut SalvageStats) -> Option<SavedSplitNode> {
    let mut panes = Vec::new();
    if let Some(values) = object.get("panes").and_then(Value::as_array) {
        for value in values {
            match salvage_pane(value, stats) {
                Some(pane) => panes.push(pane),
                None => stats.skipped_panes += 1,
            }
        }
    }
    if panes.is_empty()
        && let Some(value) = object.get("pane")
    {
        match salvage_pane(value, stats) {
            Some(pane) => panes.push(pane),
            None => stats.skipped_panes += 1,
        }
    }
    if panes.is_empty() {
        stats.dropped_nodes += 1;
        return None;
    }
    Some(SavedSplitNode::Leaf {
        pane: None,
        panes,
        active_pane_key: salvage_optional_id(object.get("activePaneKey"), stats),
    })
}

fn salvage_pane(value: &Value, stats: &mut SalvageStats) -> Option<SavedPane> {
    let object = value.as_object()?;
    let mut compatible = object.clone();
    compatible.remove("paneKey");
    compatible.remove("terminalSessionId");
    compatible.remove("terminalIncarnationId");
    let mut pane = serde_json::from_value::<SavedPane>(Value::Object(compatible)).ok()?;
    pane.pane_key = salvage_optional_id(object.get("paneKey"), stats);
    pane.terminal_session_id = salvage_optional_id(object.get("terminalSessionId"), stats);
    pane.terminal_incarnation_id = salvage_optional_id(object.get("terminalIncarnationId"), stats);
    Some(pane)
}

fn salvage_split(
    object: &Map<String, Value>,
    depth: usize,
    stats: &mut SalvageStats,
) -> Option<SavedSplitNode> {
    let child_values = object.get("children")?.as_array()?;
    let considered = child_values.len();
    let mut children = Vec::new();
    for child in child_values {
        if let Some(child) = salvage_split_node(child, depth + 1, stats) {
            children.push(child);
        }
    }
    match children.len() {
        0 => {
            stats.dropped_nodes += 1;
            None
        }
        1 => {
            stats.collapsed_splits += 1;
            children.pop()
        }
        count => {
            let sizes = if children.len() == considered {
                salvage_sizes(object.get("sizes"), count)
            } else {
                None
            }
            .unwrap_or_else(|| {
                stats.rebuilt_sizes += 1;
                equal_sizes(count)
            });
            Some(SavedSplitNode::Split {
                direction: object
                    .get("direction")
                    .and_then(Value::as_str)
                    .filter(|direction| !direction.is_empty())
                    .unwrap_or("horizontal")
                    .to_string(),
                children,
                sizes,
            })
        }
    }
}

fn validate_salvage_bounds(value: &Value) -> std::result::Result<(), &'static str> {
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    let Some(tabs) = object.get("tabs").and_then(Value::as_array) else {
        return Ok(());
    };
    if tabs.len() > MAX_SALVAGE_TABS {
        return Err("layout 超过 salvage tab 数量上限");
    }
    for tab in tabs {
        if let Some(split_layout) = tab.as_object().and_then(|object| object.get("splitLayout")) {
            validate_salvage_node_bounds(split_layout, 0)?;
        }
    }
    Ok(())
}

fn validate_salvage_node_bounds(
    value: &Value,
    depth: usize,
) -> std::result::Result<(), &'static str> {
    if depth >= MAX_SALVAGE_DEPTH {
        return Err("layout 超过 salvage split 深度上限");
    }
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    match object.get("type").and_then(Value::as_str) {
        Some("leaf")
            if object
                .get("panes")
                .and_then(Value::as_array)
                .is_some_and(|panes| panes.len() > MAX_SALVAGE_PANES_PER_LEAF) =>
        {
            return Err("layout 超过 salvage leaf pane 数量上限");
        }
        Some("leaf") => {}
        Some("split") => {
            if let Some(children) = object.get("children").and_then(Value::as_array) {
                if children.len() > MAX_SALVAGE_CHILDREN {
                    return Err("layout 超过 salvage split child 数量上限");
                }
                for child in children {
                    validate_salvage_node_bounds(child, depth + 1)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn salvage_sizes(value: Option<&Value>, expected_len: usize) -> Option<Vec<f64>> {
    let values = value?.as_array()?;
    if values.len() != expected_len {
        return None;
    }
    values
        .iter()
        .map(Value::as_f64)
        .collect::<Option<Vec<_>>>()
        .filter(|sizes| sizes.iter().all(|size| size.is_finite() && *size > 0.0))
}

fn salvage_optional_id<T>(value: Option<&Value>, stats: &mut SalvageStats) -> Option<T>
where
    T: FromStr,
{
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(raw)) => match T::from_str(raw) {
            Ok(id) => Some(id),
            Err(_) => {
                stats.normalized_ids += 1;
                None
            }
        },
        Some(_) => {
            stats.normalized_ids += 1;
            None
        }
    }
}

fn normalize_saved_layout_stable_ids(
    layout: &mut SavedProjectLayout,
    expected_worktree_id: Option<&WorktreeId>,
    stats: &mut SalvageStats,
) {
    mt_config::normalize_saved_layout(layout);

    let mut normalized_tabs = Vec::with_capacity(layout.tabs.len());
    for mut tab in std::mem::take(&mut layout.tabs) {
        match normalize_split_structure(tab.split_layout.clone(), stats) {
            Some(split_layout) => {
                tab.split_layout = split_layout;
                normalized_tabs.push(tab);
            }
            None => stats.skipped_tabs += 1,
        }
    }
    layout.tabs = normalized_tabs;

    match expected_worktree_id {
        Some(expected)
            if layout.worktree_id.as_ref().map(|id| id.as_str()) != Some(expected.as_str()) =>
        {
            layout.worktree_id = Some(expected.clone());
            stats.normalized_ids += 1;
        }
        None if layout
            .worktree_id
            .as_ref()
            .is_some_and(|id| WorktreeId::from_str(id.as_str()).is_err()) =>
        {
            layout.worktree_id = None;
            stats.normalized_ids += 1;
        }
        _ => {}
    }

    let mut seen_tabs = HashSet::new();
    let mut seen_panes = HashSet::new();
    let mut seen_sessions = HashSet::new();
    let mut seen_incarnations = HashSet::new();
    for tab in &mut layout.tabs {
        let keep_tab_id = tab.tab_id.as_ref().is_some_and(|id| {
            TabId::from_str(id.as_str()).is_ok() && seen_tabs.insert(id.as_str().to_string())
        });
        if !keep_tab_id {
            let tab_id = TabId::new();
            seen_tabs.insert(tab_id.as_str().to_string());
            tab.tab_id = Some(tab_id);
            stats.normalized_ids += 1;
        }
        normalize_pane_ids(
            &mut tab.split_layout,
            &mut seen_panes,
            &mut seen_sessions,
            &mut seen_incarnations,
            stats,
        );
    }

    if layout.tabs.is_empty() {
        layout.normalize_terminal_navigation();
        if layout.active_tab_index != 0 {
            layout.active_tab_index = 0;
        }
        if layout.active_tab_id.take().is_some() {
            stats.normalized_ids += 1;
        }
        return;
    }

    let fallback_index = layout.active_tab_index.min(layout.tabs.len() - 1);
    let selected_index = layout
        .active_tab_id
        .as_ref()
        .and_then(|active| {
            layout
                .tabs
                .iter()
                .position(|tab| tab.tab_id.as_ref().map(|id| id.as_str()) == Some(active.as_str()))
        })
        .unwrap_or(fallback_index);
    let selected_id = layout.tabs[selected_index].tab_id.clone();
    if layout.active_tab_index != selected_index {
        layout.active_tab_index = selected_index;
    }
    if layout.active_tab_id != selected_id {
        layout.active_tab_id = selected_id;
        stats.normalized_ids += 1;
    }
    layout.normalize_terminal_navigation();
}

fn normalize_split_structure(
    node: SavedSplitNode,
    stats: &mut SalvageStats,
) -> Option<SavedSplitNode> {
    match node {
        SavedSplitNode::Leaf {
            pane,
            mut panes,
            active_pane_key,
        } => {
            if let Some(pane) = pane
                && panes.is_empty()
            {
                panes.push(pane);
            }
            if panes.is_empty() {
                stats.dropped_nodes += 1;
                None
            } else {
                Some(SavedSplitNode::Leaf {
                    pane: None,
                    panes,
                    active_pane_key,
                })
            }
        }
        SavedSplitNode::Split {
            direction,
            children,
            sizes,
        } => {
            let original_len = children.len();
            let children = children
                .into_iter()
                .filter_map(|child| normalize_split_structure(child, stats))
                .collect::<Vec<_>>();
            match children.len() {
                0 => {
                    stats.dropped_nodes += 1;
                    None
                }
                1 => {
                    stats.collapsed_splits += 1;
                    children.into_iter().next()
                }
                count => {
                    let sizes_are_valid = original_len == count
                        && sizes.len() == count
                        && sizes.iter().all(|size| size.is_finite() && *size > 0.0);
                    let sizes = if sizes_are_valid {
                        sizes
                    } else {
                        stats.rebuilt_sizes += 1;
                        equal_sizes(count)
                    };
                    Some(SavedSplitNode::Split {
                        direction,
                        children,
                        sizes,
                    })
                }
            }
        }
    }
}

fn normalize_pane_ids(
    node: &mut SavedSplitNode,
    seen_panes: &mut HashSet<String>,
    seen_sessions: &mut HashSet<String>,
    seen_incarnations: &mut HashSet<String>,
    stats: &mut SalvageStats,
) {
    match node {
        SavedSplitNode::Leaf {
            panes,
            active_pane_key,
            ..
        } => {
            for pane in panes.iter_mut() {
                let keep_pane_key = pane.pane_key.as_ref().is_some_and(|id| {
                    PaneKey::from_str(id.as_str()).is_ok()
                        && seen_panes.insert(id.as_str().to_string())
                });
                if !keep_pane_key {
                    let pane_key = PaneKey::new();
                    seen_panes.insert(pane_key.as_str().to_string());
                    pane.pane_key = Some(pane_key);
                    stats.normalized_ids += 1;
                }

                let keep_session_id = pane.terminal_session_id.as_ref().is_some_and(|id| {
                    TerminalSessionId::from_str(id.as_str()).is_ok()
                        && seen_sessions.insert(id.as_str().to_string())
                });
                if !keep_session_id {
                    let terminal_session_id = TerminalSessionId::new();
                    seen_sessions.insert(terminal_session_id.as_str().to_string());
                    pane.terminal_session_id = Some(terminal_session_id);
                    stats.normalized_ids += 1;
                }

                let keep_incarnation = pane.terminal_incarnation_id.as_ref().is_none_or(|id| {
                    TerminalIncarnationId::from_str(id.as_str()).is_ok()
                        && seen_incarnations.insert(id.as_str().to_string())
                });
                if !keep_incarnation {
                    pane.terminal_incarnation_id = None;
                    stats.normalized_ids += 1;
                }
            }

            let selected = active_pane_key.as_ref().and_then(|active| {
                panes.iter().find_map(|pane| {
                    pane.pane_key
                        .as_ref()
                        .filter(|pane_key| pane_key.as_str() == active.as_str())
                        .cloned()
                })
            });
            let fallback = panes.first().and_then(|pane| pane.pane_key.clone());
            let selected = selected.or(fallback);
            if *active_pane_key != selected {
                *active_pane_key = selected;
                stats.normalized_ids += 1;
            }
        }
        SavedSplitNode::Split { children, .. } => {
            for child in children {
                normalize_pane_ids(child, seen_panes, seen_sessions, seen_incarnations, stats);
            }
        }
    }
}

fn equal_sizes(count: usize) -> Vec<f64> {
    vec![100.0 / count as f64; count]
}

fn to_json<T: Serialize>(value: &Option<T>) -> Option<String> {
    value.as_ref().and_then(|v| serde_json::to_string(v).ok())
}

/// kv 表里取一个键并解析。缺键 / 解析失败一律 `None` —— 手改坏一个键不该让
/// 整份布局读不出来(闭包做不到泛型,所以是个自由函数)。
fn from_kv<T: for<'de> Deserialize<'de>>(map: &HashMap<String, String>, key: &str) -> Option<T> {
    map.get(key).and_then(|v| serde_json::from_str(v).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt_config::{ProjectConfig, SavedPane, SavedSplitNode, SavedTab};

    fn temp_dir(label: &str) -> PathBuf {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("mt-layout-test-{label}-{ts}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn saved_pane(shell: &str, cwd: Option<&str>) -> SavedPane {
        SavedPane {
            pane_key: None,
            terminal_session_id: None,
            terminal_incarnation_id: None,
            shell_name: shell.into(),
            cwd: cwd.map(str::to_string),
            ai_session: None,
        }
    }

    fn layout(shell: &str) -> SavedProjectLayout {
        SavedProjectLayout {
            selected_terminal_pane_key: None,
            terminal_order: None,
            worktree_id: None,
            tabs: vec![SavedTab {
                tab_id: None,
                custom_title: None,
                split_layout: SavedSplitNode::Split {
                    direction: "vertical".into(),
                    sizes: vec![30.0, 70.0],
                    children: vec![
                        SavedSplitNode::Leaf {
                            active_pane_key: None,
                            pane: None,
                            panes: vec![saved_pane(shell, None)],
                        },
                        SavedSplitNode::Leaf {
                            active_pane_key: None,
                            pane: None,
                            panes: vec![saved_pane(shell, Some("D:/x"))],
                        },
                    ],
                },
            }],
            active_tab_index: 0,
            active_tab_id: None,
        }
    }

    fn empty_layout() -> SavedProjectLayout {
        SavedProjectLayout {
            selected_terminal_pane_key: None,
            terminal_order: None,
            worktree_id: None,
            tabs: vec![],
            active_tab_index: 0,
            active_tab_id: None,
        }
    }

    fn binding(project_id: &str, worktree_path: &str) -> ProjectWorktreeBinding {
        let install: HostInstallId = "install-v1:123e4567-e89b-42d3-a456-426614174000"
            .parse()
            .unwrap();
        let execution_host_id = ExecutionHostId::derive("local", &install);
        let repo_id = RepoId::derive(&execution_host_id, "/repo/.git");
        let worktree_id = WorktreeId::derive(&repo_id, worktree_path, None);
        ProjectWorktreeBinding {
            project_id: project_id.into(),
            execution_host_id,
            repo_id,
            worktree_id,
            identity_source: "authoritative-local-git".into(),
            canonical_worktree_path: Some(worktree_path.into()),
            identity_context: None,
        }
    }

    fn worktree_json(store: &LayoutStore, worktree_id: &WorktreeId) -> Option<String> {
        let conn = store.conn.lock().unwrap();
        conn.query_row(
            "SELECT layout_json FROM worktree_layout WHERE worktree_id = ?1",
            params![worktree_id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
    }

    fn worktree_updated_at(store: &LayoutStore, worktree_id: &WorktreeId) -> Option<i64> {
        let conn = store.conn.lock().unwrap();
        conn.query_row(
            "SELECT updated_at_ms FROM worktree_layout WHERE worktree_id = ?1",
            params![worktree_id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
    }

    fn legacy_json(store: &LayoutStore, project_id: &str) -> Option<String> {
        let conn = store.conn.lock().unwrap();
        conn.query_row(
            "SELECT layout_json FROM project_layout WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
    }

    fn first_shell(layout: &SavedProjectLayout) -> &str {
        fn from_node(node: &SavedSplitNode) -> Option<&str> {
            match node {
                SavedSplitNode::Leaf { panes, .. } => {
                    panes.first().map(|pane| pane.shell_name.as_str())
                }
                SavedSplitNode::Split { children, .. } => children.iter().find_map(from_node),
            }
        }
        from_node(&layout.tabs[0].split_layout).unwrap()
    }

    #[test]
    fn 分屏树往返() {
        let dir = temp_dir("roundtrip");
        let store = LayoutStore::open_at(&dir).unwrap();
        store.save_project_layout("p1", &layout("cmd"), 42).unwrap();

        let back = store.load_project_layouts();
        let got = back.get("p1").unwrap();
        assert_eq!(got.tabs.len(), 1);
        let SavedSplitNode::Split {
            sizes, children, ..
        } = &got.tabs[0].split_layout
        else {
            panic!("应还原成 split");
        };
        assert_eq!(sizes.as_slice(), &[30.0, 70.0]);
        assert_eq!(children.len(), 2);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 重开库仍读得到() {
        let dir = temp_dir("reopen");
        {
            let store = LayoutStore::open_at(&dir).unwrap();
            store.save_project_layout("p1", &layout("cmd"), 1).unwrap();
        }
        let store = LayoutStore::open_at(&dir).unwrap();
        assert!(store.load_project_layouts().contains_key("p1"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 全局项部分更新不抹掉其它键() {
        let dir = temp_dir("globals-partial");
        let store = LayoutStore::open_at(&dir).unwrap();
        store
            .save_globals(&GlobalLayout {
                layout_sizes: Some(vec![20.0, 60.0, 20.0]),
                right_drawer_width: Some(360.0),
                ..Default::default()
            })
            .unwrap();
        // 只改三栏比例,其余字段留 None
        store
            .save_globals(&GlobalLayout {
                layout_sizes: Some(vec![25.0, 55.0, 20.0]),
                ..Default::default()
            })
            .unwrap();

        let got = store.load_globals();
        assert_eq!(got.layout_sizes, Some(vec![25.0, 55.0, 20.0]));
        assert_eq!(got.right_drawer_width, Some(360.0), "没填的字段不该被抹掉");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 窗口几何往返() {
        let dir = temp_dir("window");
        let store = LayoutStore::open_at(&dir).unwrap();
        let geo = WindowGeometry {
            mode: WindowMode::Maximized,
            x: 100.0,
            y: 50.0,
            width: 1440.0,
            height: 900.0,
        };
        store
            .save_globals(&GlobalLayout {
                window: Some(geo),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(store.load_globals().window, Some(geo));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 离谱的窗口几何判废() {
        let ok = WindowGeometry {
            mode: WindowMode::Windowed,
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 800.0,
        };
        assert!(ok.is_sane());
        assert!(!WindowGeometry { width: 0.0, ..ok }.is_sane());
        assert!(!WindowGeometry { height: -5.0, ..ok }.is_sane());
        assert!(!WindowGeometry { x: f64::NAN, ..ok }.is_sane());
        assert!(
            !WindowGeometry { width: 10.0, ..ok }.is_sane(),
            "小得放不下内容"
        );
    }

    #[test]
    fn 空布局按删行处理() {
        let dir = temp_dir("empty");
        let store = LayoutStore::open_at(&dir).unwrap();
        store.save_project_layout("p1", &layout("cmd"), 1).unwrap();
        store.save_project_layout("p1", &empty_layout(), 2).unwrap();
        assert!(!store.load_project_layouts().contains_key("p1"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 清理无主项目行() {
        let dir = temp_dir("retain");
        let store = LayoutStore::open_at(&dir).unwrap();
        store.save_project_layout("p1", &layout("cmd"), 1).unwrap();
        store.save_project_layout("p2", &layout("cmd"), 1).unwrap();

        let live: HashSet<String> = ["p1".to_string()].into_iter().collect();
        store.retain_projects(&live).unwrap();

        let back = store.load_project_layouts();
        assert!(back.contains_key("p1"));
        assert!(!back.contains_key("p2"));
        fs::remove_dir_all(&dir).ok();
    }

    /// 迁移:config.json 的布局灌进库,且**只灌一次** —— 用户关光终端后重启,
    /// 不该被旧 config 里的残留复活。
    #[test]
    fn 从配置迁移一次且只迁一次() {
        let dir = temp_dir("migrate");
        let store = LayoutStore::open_at(&dir).unwrap();
        assert!(store.needs_config_migration(), "空库该要迁移");

        let mut config = AppConfig::default();
        config.layout_sizes = Some(vec![20.0, 60.0, 20.0]);
        config.right_drawer_width = Some(400.0);
        config.projects.push(ProjectConfig {
            id: "p1".into(),
            name: "proj".into(),
            path: "D:/proj".into(),
            saved_layout: Some(layout("cmd")),
            ..project_stub()
        });

        let n = store.migrate_from_config(&config).unwrap();
        assert_eq!(n, 1);
        assert!(!store.needs_config_migration(), "迁移后不该再迁");
        assert_eq!(
            store.load_globals().layout_sizes,
            Some(vec![20.0, 60.0, 20.0])
        );
        assert_eq!(store.load_globals().right_drawer_width, Some(400.0));
        assert!(store.load_project_layouts().contains_key("p1"));

        // 用户把终端关光 → 库里删了行,但标记还在,重启不该被 config 复活
        store.delete_project_layout("p1").unwrap();
        let store = LayoutStore::open_at(&dir).unwrap();
        assert!(!store.needs_config_migration());
        assert!(!store.load_project_layouts().contains_key("p1"));

        fs::remove_dir_all(&dir).ok();
    }

    /// 旧格式的 `pane`(单数)在迁移后仍读得出来 —— 迁移是逐字节搬 JSON,
    /// 归一化在读出来那一刻做(与 `migrate_config` 同一口径)。
    #[test]
    fn 旧格式单_pane_读出时归一化() {
        let dir = temp_dir("legacy-pane");
        let store = LayoutStore::open_at(&dir).unwrap();
        // 直接写一段旧格式 JSON:`pane`(单数)是 `skip_serializing` 的,走
        // `save_project_layout` 反而写不出这种形状 —— 这里模拟的是存量库/迁移
        // 时从 config.json 原样搬过来的那份数据。
        {
            let conn = store.conn.lock().unwrap();
            let json = r#"{"tabs":[{"splitLayout":{"type":"leaf","pane":{"shellName":"cmd","cwd":"D:/x"}}}],"activeTabIndex":0}"#;
            conn.execute(
                "INSERT INTO project_layout(project_id, layout_json, updated_at_ms) VALUES('p1', ?1, 0)",
                params![json],
            )
            .unwrap();
        }

        let back = store.load_project_layouts();
        let got = back.get("p1").unwrap();
        let SavedSplitNode::Leaf { pane, panes, .. } = &got.tabs[0].split_layout else {
            panic!("应是 leaf");
        };
        assert!(pane.is_none(), "旧字段读完即清");
        assert_eq!(panes.len(), 1, "应归一化进 panes");
        assert_eq!(panes[0].shell_name, "cmd");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 本地安装_id_跨重开稳定() {
        let dir = temp_dir("install-id");
        let first = {
            let store = LayoutStore::open_at(&dir).unwrap();
            let first = store.local_host_install_id().unwrap();
            assert_eq!(store.local_host_install_id().unwrap(), first);
            first
        };
        let store = LayoutStore::open_at(&dir).unwrap();
        assert_eq!(store.local_host_install_id().unwrap(), first);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn worktree_保存会归一化并双写且绑定可只读加载() {
        let dir = temp_dir("dual-write");
        let store = LayoutStore::open_at(&dir).unwrap();
        let mut binding = binding("p1", "/repo/main");
        binding.identity_context = Some("test-authority-v1".into());
        store
            .save_worktree_layout(&binding, &layout("cmd"), 10)
            .unwrap();

        let worktree_json = worktree_json(&store, &binding.worktree_id).unwrap();
        assert_eq!(
            legacy_json(&store, "p1").as_deref(),
            Some(worktree_json.as_str())
        );
        let persisted: SavedProjectLayout = serde_json::from_str(&worktree_json).unwrap();
        assert_eq!(persisted.worktree_id.as_ref(), Some(&binding.worktree_id));
        assert!(persisted.active_tab_id.is_some());
        assert!(persisted.tabs[0].tab_id.is_some());
        let SavedSplitNode::Split { children, .. } = &persisted.tabs[0].split_layout else {
            panic!("应保留 split");
        };
        let SavedSplitNode::Leaf {
            active_pane_key,
            panes,
            ..
        } = &children[0]
        else {
            panic!("应保留 leaf");
        };
        assert_eq!(active_pane_key.as_ref(), panes[0].pane_key.as_ref());
        assert!(panes[0].terminal_session_id.is_some());
        assert!(panes[0].terminal_incarnation_id.is_none());

        let bindings = store.load_project_bindings().unwrap();
        assert_eq!(bindings.get("p1"), Some(&binding));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn schema_v2_adds_identity_context_without_rewriting_existing_data() {
        let dir = temp_dir("schema-v2-identity-context");
        let path = dir.join("layout.db");
        let expected = binding("p1", "/repo/main");
        let layout_json = serde_json::to_string(&layout("v2-shell")).unwrap();
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE meta (
                   key   TEXT PRIMARY KEY,
                   value TEXT NOT NULL
                 );
                 INSERT INTO meta(key, value) VALUES('schema_version', '2');
                 CREATE TABLE project_worktree_binding (
                   project_id              TEXT PRIMARY KEY,
                   execution_host_id       TEXT NOT NULL,
                   repo_id                 TEXT NOT NULL,
                   worktree_id             TEXT NOT NULL,
                   identity_source         TEXT NOT NULL,
                   canonical_worktree_path TEXT,
                   updated_at_ms           INTEGER NOT NULL
                 );
                 CREATE TABLE worktree_layout (
                   worktree_id    TEXT PRIMARY KEY,
                   layout_json   TEXT NOT NULL,
                   updated_at_ms INTEGER NOT NULL
                 );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO project_worktree_binding(
                   project_id, execution_host_id, repo_id, worktree_id,
                   identity_source, canonical_worktree_path, updated_at_ms
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    expected.project_id.as_str(),
                    expected.execution_host_id.as_str(),
                    expected.repo_id.as_str(),
                    expected.worktree_id.as_str(),
                    expected.identity_source.as_str(),
                    expected.canonical_worktree_path.as_deref(),
                    17_i64,
                ],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO worktree_layout(worktree_id, layout_json, updated_at_ms)
                 VALUES(?1, ?2, ?3)",
                params![expected.worktree_id.as_str(), layout_json.as_str(), 19_i64],
            )
            .unwrap();
        }

        let store = LayoutStore::open_at(&dir).unwrap();
        assert_eq!(
            store.load_project_bindings().unwrap().get("p1"),
            Some(&expected)
        );
        let conn = store.conn.lock().unwrap();
        let migrated_version: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(migrated_version, SCHEMA_VERSION.to_string());
        let identity_context_columns: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('project_worktree_binding')
                 WHERE name = 'identity_context'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(identity_context_columns, 1);
        let persisted_binding: (Option<String>, i64) = conn
            .query_row(
                "SELECT identity_context, updated_at_ms
                 FROM project_worktree_binding WHERE project_id = 'p1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(persisted_binding, (None, 17));
        let persisted_layout: (String, i64) = conn
            .query_row(
                "SELECT layout_json, updated_at_ms FROM worktree_layout
                 WHERE worktree_id = ?1",
                params![expected.worktree_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(persisted_layout, (layout_json, 19));
        drop(conn);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn legacy_迁移归一化只发生一次() {
        let dir = temp_dir("identity-migrate-idempotent");
        let store = LayoutStore::open_at(&dir).unwrap();
        let binding = binding("p1", "/repo/main");
        let old_json = serde_json::to_string(&layout("cmd")).unwrap();
        {
            let conn = store.conn.lock().unwrap();
            upsert_legacy_layout(&conn, "p1", &old_json, 1).unwrap();
        }

        let first = store
            .reconcile_worktree_layouts(std::slice::from_ref(&binding), 10)
            .unwrap();
        let first_layout = first.layouts.get("p1").unwrap();
        assert_eq!(
            first_layout.worktree_id.as_ref(),
            Some(&binding.worktree_id)
        );
        assert!(first_layout.active_tab_id.is_some());
        let first_json = worktree_json(&store, &binding.worktree_id).unwrap();
        let first_updated_at = worktree_updated_at(&store, &binding.worktree_id).unwrap();

        let second = store
            .reconcile_worktree_layouts(std::slice::from_ref(&binding), 20)
            .unwrap();
        assert_eq!(
            serde_json::to_string(second.layouts.get("p1").unwrap()).unwrap(),
            first_json
        );
        assert_eq!(
            worktree_json(&store, &binding.worktree_id).unwrap(),
            first_json
        );
        assert_eq!(
            worktree_updated_at(&store, &binding.worktree_id),
            Some(first_updated_at),
            "内容未变时不该重复写回"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 多项目共享_worktree_时目标优先且冲突_legacy_原样保留() {
        let dir = temp_dir("shared-worktree-legacy-conflict");
        let store = LayoutStore::open_at(&dir).unwrap();
        let first = binding("p1", "/repo/shared");
        let second = binding("p2", "/repo/shared");
        assert_eq!(first.worktree_id, second.worktree_id);

        let first_legacy = serde_json::to_string(&layout("first-shell")).unwrap();
        let second_legacy = serde_json::to_string(&layout("second-shell")).unwrap();
        {
            let conn = store.conn.lock().unwrap();
            upsert_legacy_layout(&conn, "p1", &first_legacy, 1).unwrap();
            upsert_legacy_layout(&conn, "p2", &second_legacy, 1).unwrap();
        }

        let reconciled = store
            .reconcile_worktree_layouts(&[first.clone(), second.clone()], 2)
            .unwrap();
        assert_eq!(
            first_shell(reconciled.layouts.get("p1").unwrap()),
            "first-shell"
        );
        assert_eq!(
            first_shell(reconciled.layouts.get("p2").unwrap()),
            "first-shell"
        );

        let destination: SavedProjectLayout =
            serde_json::from_str(&worktree_json(&store, &first.worktree_id).unwrap()).unwrap();
        assert_eq!(first_shell(&destination), "first-shell");
        let first_mirror: SavedProjectLayout =
            serde_json::from_str(&legacy_json(&store, "p1").unwrap()).unwrap();
        assert_eq!(first_shell(&first_mirror), "first-shell");
        let second_mirror: SavedProjectLayout =
            serde_json::from_str(&legacy_json(&store, "p2").unwrap()).unwrap();
        assert_eq!(first_shell(&second_mirror), "second-shell");
        assert_eq!(
            store.load_project_bindings().unwrap(),
            HashMap::from([("p1".to_string(), first), ("p2".to_string(), second)])
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 共享_worktree_协调与输入顺序无关() {
        fn reconcile(order: [&str; 2], label: &str, older_json: &str, newer_json: &str) -> String {
            let dir = temp_dir(label);
            let store = LayoutStore::open_at(&dir).unwrap();
            let first = binding("p1", "/repo/shared");
            let second = binding("p2", "/repo/shared");
            let bindings = HashMap::from([
                (first.project_id.clone(), first.clone()),
                (second.project_id.clone(), second.clone()),
            ]);
            {
                let conn = store.conn.lock().unwrap();
                upsert_legacy_layout(&conn, "p1", older_json, 10).unwrap();
                upsert_legacy_layout(&conn, "p2", newer_json, 20).unwrap();
            }
            let desired = order
                .iter()
                .map(|project_id| bindings.get(*project_id).unwrap().clone())
                .collect::<Vec<_>>();

            let reconciled = store.reconcile_worktree_layouts(&desired, 30).unwrap();
            let first_layout_json =
                serde_json::to_string(reconciled.layouts.get("p1").unwrap()).unwrap();
            let second_layout_json =
                serde_json::to_string(reconciled.layouts.get("p2").unwrap()).unwrap();
            let destination_json = worktree_json(&store, &first.worktree_id).unwrap();
            assert_eq!(first_layout_json, second_layout_json);
            assert_eq!(first_layout_json, destination_json);
            assert_eq!(
                first_shell(
                    &serde_json::from_str::<SavedProjectLayout>(
                        &legacy_json(&store, "p1").unwrap(),
                    )
                    .unwrap(),
                ),
                "older-shell",
                "未选中的冲突 legacy 镜像必须保留"
            );
            fs::remove_dir_all(&dir).ok();
            destination_json
        }

        let target = binding("seed", "/repo/shared");
        let mut older = layout("older-shell");
        let mut newer = layout("newer-shell");
        normalize_saved_layout_stable_ids(
            &mut older,
            Some(&target.worktree_id),
            &mut SalvageStats::default(),
        );
        normalize_saved_layout_stable_ids(
            &mut newer,
            Some(&target.worktree_id),
            &mut SalvageStats::default(),
        );
        let older_json = serde_json::to_string(&older).unwrap();
        let newer_json = serde_json::to_string(&newer).unwrap();

        let forward = reconcile(
            ["p1", "p2"],
            "shared-worktree-forward",
            &older_json,
            &newer_json,
        );
        let reverse = reconcile(
            ["p2", "p1"],
            "shared-worktree-reverse",
            &older_json,
            &newer_json,
        );
        assert_eq!(forward, reverse);
        assert_eq!(
            first_shell(&serde_json::from_str::<SavedProjectLayout>(&forward).unwrap()),
            "newer-shell"
        );
    }

    #[test]
    fn 跨_worktree_协调在写入前冻结全部候选() {
        let dir = temp_dir("cross-worktree-candidate-snapshot");
        let store = LayoutStore::open_at(&dir).unwrap();
        let old_first = binding("p1", "/repo/first");
        let old_second = binding("p2", "/repo/second");
        let new_first = binding("p1", "/repo/second");
        let new_second = binding("p2", "/repo/first");
        assert_eq!(old_first.worktree_id, new_second.worktree_id);
        assert_eq!(old_second.worktree_id, new_first.worktree_id);

        let first_legacy = serde_json::to_string(&layout("first-shell")).unwrap();
        let second_legacy = serde_json::to_string(&layout("second-shell")).unwrap();
        {
            let conn = store.conn.lock().unwrap();
            upsert_project_binding(&conn, &old_first, 1).unwrap();
            upsert_project_binding(&conn, &old_second, 1).unwrap();
            upsert_legacy_layout(&conn, "p1", &first_legacy, 10).unwrap();
            upsert_legacy_layout(&conn, "p2", &second_legacy, 20).unwrap();
        }

        let reconciled = store
            .reconcile_worktree_layouts(&[new_first.clone(), new_second.clone()], 30)
            .unwrap();

        assert_eq!(
            first_shell(reconciled.layouts.get("p1").unwrap()),
            "first-shell"
        );
        assert_eq!(
            first_shell(reconciled.layouts.get("p2").unwrap()),
            "second-shell"
        );
        let first_destination: SavedProjectLayout =
            serde_json::from_str(&worktree_json(&store, &new_first.worktree_id).unwrap()).unwrap();
        let second_destination: SavedProjectLayout =
            serde_json::from_str(&worktree_json(&store, &new_second.worktree_id).unwrap()).unwrap();
        assert_eq!(first_shell(&first_destination), "first-shell");
        assert_eq!(first_shell(&second_destination), "second-shell");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rebind_时目标_worktree_布局优先且旧源保留() {
        let dir = temp_dir("rebind-destination-wins");
        let store = LayoutStore::open_at(&dir).unwrap();
        let old_binding = binding("p1", "/repo/old");
        store
            .save_worktree_layout(&old_binding, &layout("old-shell"), 1)
            .unwrap();

        let new_binding = binding("p1", "/repo/new");
        let mut destination = layout("destination-shell");
        let mut stats = SalvageStats::default();
        normalize_saved_layout_stable_ids(
            &mut destination,
            Some(&new_binding.worktree_id),
            &mut stats,
        );
        let destination_json = serde_json::to_string(&destination).unwrap();
        {
            let conn = store.conn.lock().unwrap();
            upsert_worktree_layout(&conn, &new_binding.worktree_id, &destination_json, 2).unwrap();
        }

        let reconciled = store
            .reconcile_worktree_layouts(std::slice::from_ref(&new_binding), 3)
            .unwrap();
        assert_eq!(
            first_shell(reconciled.layouts.get("p1").unwrap()),
            "destination-shell"
        );
        let old: SavedProjectLayout =
            serde_json::from_str(&worktree_json(&store, &old_binding.worktree_id).unwrap())
                .unwrap();
        assert_eq!(
            first_shell(&old),
            "old-shell",
            "旧 worktree 行不得删除或覆盖"
        );
        let mirror: SavedProjectLayout =
            serde_json::from_str(&legacy_json(&store, "p1").unwrap()).unwrap();
        assert_eq!(first_shell(&mirror), "destination-shell");
        assert_eq!(
            store.load_project_bindings().unwrap().get("p1"),
            Some(&new_binding)
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn flat_preferences_normalize_without_changing_legacy_records() {
        let binding = binding("flat", "/repo/flat");
        let mut saved = layout("first-shell");
        saved.tabs.extend(layout("second-shell").tabs);
        normalize_saved_layout_stable_ids(&mut saved, Some(&binding.worktree_id), &mut SalvageStats::default());
        let keys = saved.terminal_order.clone().unwrap();
        assert_eq!(keys.len(), 4);
        let records = serde_json::to_value(&saved.tabs).unwrap();
        saved.selected_terminal_pane_key = Some(keys[3].clone());
        saved.terminal_order = Some(vec![keys[3].clone(), PaneKey::new(), keys[3].clone(), keys[1].clone()]);
        normalize_saved_layout_stable_ids(&mut saved, Some(&binding.worktree_id), &mut SalvageStats::default());
        assert_eq!(saved.terminal_order, Some(vec![keys[3].clone(), keys[1].clone(), keys[0].clone(), keys[2].clone()]));
        assert_eq!(saved.selected_terminal_pane_key.as_ref(), Some(&keys[3]));
        assert_eq!(saved.active_tab_index, 1);
        assert_eq!(saved.active_tab_id, saved.tabs[1].tab_id);
        assert_eq!(serde_json::to_value(&saved.tabs).unwrap(), records);
        let json = serde_json::to_string(&saved).unwrap();
        let again = decode_saved_layout(&json, Some(&binding.worktree_id)).unwrap();
        assert!(!again.repaired, "normalized metadata must not churn on reopen");
        assert_eq!(again.normalized_json, json);
    }

    #[test]
    fn flat_selection_updates_only_the_selected_legacy_leaf_and_owner() {
        let mut saved = layout("first-shell");
        let mut second = layout("second-shell").tabs.remove(0);
        let SavedSplitNode::Split { children, .. } = &mut second.split_layout else {
            panic!("fixture must have split children");
        };
        let SavedSplitNode::Leaf { panes, .. } = &mut children[1] else {
            panic!("fixture must have a leaf");
        };
        let mut selected_pane = saved_pane("selected-shell", Some("/saved/selected"));
        selected_pane.ai_session = Some(mt_config::SavedAiSession {
            agent: Some("codex".into()),
            session_id: "saved-provider-session".into(),
            cwd: Some("/saved/session".into()),
        });
        panes.push(selected_pane);
        saved.tabs.push(second);
        normalize_saved_layout_stable_ids(&mut saved, None, &mut SalvageStats::default());
        let keys = saved.terminal_order.clone().unwrap();
        assert_eq!(keys.len(), 5);

        let mut expected_tabs = saved.tabs.clone();
        let SavedSplitNode::Split { children, .. } = &mut expected_tabs[1].split_layout else {
            panic!("second owner must keep its split");
        };
        let SavedSplitNode::Leaf { active_pane_key, .. } = &mut children[1] else {
            panic!("selected owner must keep its leaf");
        };
        *active_pane_key = Some(keys[4].clone());
        let order = keys.iter().rev().cloned().collect::<Vec<_>>();
        saved.selected_terminal_pane_key = Some(keys[4].clone());
        saved.terminal_order = Some(order.clone());
        normalize_saved_layout_stable_ids(&mut saved, None, &mut SalvageStats::default());

        assert_eq!(saved.selected_terminal_pane_key.as_ref(), Some(&keys[4]));
        assert_eq!(saved.active_tab_id, saved.tabs[1].tab_id);
        assert_eq!(saved.active_tab_index, 1);
        assert_eq!(saved.terminal_order.as_ref(), Some(&order));
        assert_eq!(
            serde_json::to_value(&saved.tabs).unwrap(),
            serde_json::to_value(&expected_tabs).unwrap(),
            "only the selected leaf pointer may change; all records and geometry survive"
        );
        let json = serde_json::to_string(&saved).unwrap();
        let decoded = decode_saved_layout(&json, None).unwrap();
        assert!(!decoded.repaired);
        assert_eq!(decoded.normalized_json, json);
    }

    #[test]
    fn absent_or_stale_flat_selection_prefers_the_legacy_owner_not_presentation_order() {
        let mut saved = layout("first-shell");
        saved.tabs.extend(layout("second-shell").tabs);
        normalize_saved_layout_stable_ids(&mut saved, None, &mut SalvageStats::default());
        let keys = saved.terminal_order.clone().unwrap();
        for selected in [None, Some(PaneKey::new())] {
            let mut candidate = saved.clone();
            candidate.selected_terminal_pane_key = selected;
            candidate.active_tab_id = candidate.tabs[1].tab_id.clone();
            candidate.active_tab_index = 0;
            candidate.terminal_order = Some(keys.iter().rev().cloned().collect());
            normalize_saved_layout_stable_ids(&mut candidate, None, &mut SalvageStats::default());

            assert_eq!(candidate.selected_terminal_pane_key.as_ref(), Some(&keys[2]));
            assert_eq!(candidate.active_tab_index, 1);
            assert_eq!(candidate.active_tab_id, candidate.tabs[1].tab_id);
            assert_eq!(candidate.terminal_order.as_ref().unwrap()[0], keys[3]);
            assert_eq!(
                serde_json::to_value(&candidate.tabs).unwrap(),
                serde_json::to_value(&saved.tabs).unwrap()
            );
        }
    }

    #[test]
    fn malformed_flat_preferences_survive_typed_read_and_per_record_salvage() {
        let mut saved = layout("sh");
        normalize_saved_layout_stable_ids(&mut saved, None, &mut SalvageStats::default());
        let keys = saved.terminal_order.clone().unwrap();
        let records = serde_json::to_value(&saved.tabs).unwrap();
        let mut value = serde_json::to_value(&saved).unwrap();
        value["selectedTerminalPaneKey"] = serde_json::json!({"invalid": true});
        value["terminalOrder"] = serde_json::json!([false, keys[1], "bad-pane-key", keys[1], null]);
        let decoded = decode_saved_layout(&value.to_string(), None).unwrap();
        assert_eq!(decoded.layout.terminal_order, Some(vec![keys[1].clone(), keys[0].clone()]));
        assert_eq!(decoded.layout.selected_terminal_pane_key.as_ref(), Some(&keys[0]));
        assert_eq!(serde_json::to_value(&decoded.layout.tabs).unwrap(), records);
        let mut stats = SalvageStats::default();
        let mut salvaged = salvage_project_layout(&value, &mut stats).unwrap();
        normalize_saved_layout_stable_ids(&mut salvaged, None, &mut stats);
        assert_eq!(serde_json::to_value(&salvaged).unwrap(), serde_json::to_value(&decoded.layout).unwrap());
        value["terminalOrder"] = serde_json::json!({"also": "invalid"});
        let decoded = decode_saved_layout(&value.to_string(), None).unwrap();
        assert_eq!(decoded.layout.terminal_order.as_ref(), Some(&keys));
        assert_eq!(serde_json::to_value(&decoded.layout.tabs).unwrap(), records);
        value.as_object_mut().unwrap().remove("terminalOrder");
        value.as_object_mut().unwrap().remove("selectedTerminalPaneKey");
        let legacy = decode_saved_layout(&value.to_string(), None).unwrap();
        assert_eq!(legacy.layout.terminal_order.as_ref(), Some(&keys));
        assert_eq!(legacy.layout.selected_terminal_pane_key.as_ref(), Some(&keys[0]));
    }

    #[test]
    fn flat_preferences_dual_write_and_follow_the_latest_alias_snapshot() {
        let dir = temp_dir("flat-selection-aliases");
        let store = LayoutStore::open_at(&dir).unwrap();
        let first = binding("first", "/repo/shared");
        let latest = binding("latest", "/repo/shared");
        let mut saved = layout("sh");
        normalize_saved_layout_stable_ids(&mut saved, Some(&first.worktree_id), &mut SalvageStats::default());
        let keys = saved.terminal_order.clone().unwrap();
        store.save_worktree_layout(&first, &saved, 1).unwrap();
        saved.selected_terminal_pane_key = Some(keys[1].clone());
        saved.terminal_order = Some(vec![keys[1].clone(), keys[0].clone()]);
        store.save_worktree_layout(&latest, &saved, 2).unwrap();
        let destination = worktree_json(&store, &first.worktree_id).unwrap();
        assert_eq!(legacy_json(&store, "latest").as_deref(), Some(destination.as_str()));
        assert_ne!(legacy_json(&store, "first").as_deref(), Some(destination.as_str()));
        let reconciled = store.reconcile_worktree_layouts(&[first.clone(), latest.clone()], 3).unwrap();
        for alias in ["first", "latest"] {
            let layout = &reconciled.layouts[alias];
            assert_eq!(layout.selected_terminal_pane_key.as_ref(), Some(&keys[1]));
            assert_eq!(layout.terminal_order, saved.terminal_order);
        }
        assert_eq!(worktree_updated_at(&store, &first.worktree_id), Some(2));
        store.delete_project_binding("first").unwrap();
        assert_eq!(worktree_json(&store, &latest.worktree_id).as_deref(), Some(destination.as_str()));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_flat_preferences_do_not_create_terminal_records() {
        let mut saved = empty_layout();
        saved.selected_terminal_pane_key = Some(PaneKey::new());
        saved.terminal_order = Some(vec![PaneKey::new()]);
        normalize_saved_layout_stable_ids(&mut saved, None, &mut SalvageStats::default());
        assert!(saved.tabs.is_empty());
        assert!(saved.selected_terminal_pane_key.is_none());
        assert_eq!(saved.terminal_order, Some(Vec::new()));
    }

    #[test]
    fn valid_json_salvage_只丢坏_pane_并修复指针与_sizes() {
        let dir = temp_dir("salvage");
        let store = LayoutStore::open_at(&dir).unwrap();
        let binding = binding("p1", "/repo/main");
        let malformed = r#"{
          "worktreeId":"invalid",
          "tabs":[
            {
              "tabId":"invalid",
              "splitLayout":{
                "type":"split",
                "direction":"vertical",
                "sizes":[100],
                "children":[
                  {"type":"leaf","activePaneKey":"invalid","panes":[
                    {"shellName":"cmd"},
                    {"shellName":42},
                    {"shellName":"powershell","paneKey":"invalid","terminalSessionId":"invalid"}
                  ]},
                  {"type":"leaf","panes":[{"shellName":"bash"}]}
                ]
              }
            },
            {"splitLayout":{"type":"leaf","panes":[{"shellName":false}]}}
          ],
          "activeTabIndex":9,
          "activeTabId":"invalid"
        }"#;
        {
            let conn = store.conn.lock().unwrap();
            upsert_legacy_layout(&conn, "p1", malformed, 1).unwrap();
        }

        let reconciled = store
            .reconcile_worktree_layouts(std::slice::from_ref(&binding), 2)
            .unwrap();
        let got = reconciled.layouts.get("p1").unwrap();
        assert_eq!(got.worktree_id.as_ref(), Some(&binding.worktree_id));
        assert_eq!(got.tabs.len(), 1);
        assert_eq!(got.active_tab_index, 0);
        assert_eq!(got.active_tab_id.as_ref(), got.tabs[0].tab_id.as_ref());
        let SavedSplitNode::Split {
            sizes, children, ..
        } = &got.tabs[0].split_layout
        else {
            panic!("两个有效 child 应保留 split");
        };
        assert_eq!(sizes.as_slice(), &[50.0, 50.0]);
        let SavedSplitNode::Leaf {
            active_pane_key,
            panes,
            ..
        } = &children[0]
        else {
            panic!("第一个 child 应为 leaf");
        };
        assert_eq!(panes.len(), 2, "坏 pane 不应带走两个有效 sibling");
        assert_eq!(panes[0].shell_name, "cmd");
        assert_eq!(panes[1].shell_name, "powershell");
        assert_eq!(active_pane_key.as_ref(), panes[0].pane_key.as_ref());
        assert!(panes.iter().all(|pane| pane.pane_key.is_some()));
        assert!(panes.iter().all(|pane| pane.terminal_session_id.is_some()));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn syntactically_invalid_json_只隔离本项目() {
        let dir = temp_dir("invalid-row-isolated");
        let store = LayoutStore::open_at(&dir).unwrap();
        let broken = binding("broken", "/repo/broken");
        let healthy = binding("healthy", "/repo/healthy");
        let healthy_json = serde_json::to_string(&layout("bash")).unwrap();
        let broken_legacy_json = serde_json::to_string(&layout("legacy-shell")).unwrap();
        {
            let conn = store.conn.lock().unwrap();
            upsert_worktree_layout(&conn, &broken.worktree_id, "{not-json", 1).unwrap();
            upsert_legacy_layout(&conn, "broken", &broken_legacy_json, 1).unwrap();
            upsert_legacy_layout(&conn, "healthy", &healthy_json, 1).unwrap();
        }

        let reconciled = store
            .reconcile_worktree_layouts(&[broken.clone(), healthy.clone()], 2)
            .unwrap();
        assert!(!reconciled.layouts.contains_key("broken"));
        assert!(reconciled.layouts.contains_key("healthy"));
        assert_eq!(
            worktree_json(&store, &broken.worktree_id).as_deref(),
            Some("{not-json")
        );
        assert_eq!(
            legacy_json(&store, "broken").as_deref(),
            Some(broken_legacy_json.as_str()),
            "已有但损坏的目标行不能被 legacy 静默覆盖"
        );
        assert_eq!(reconciled.bindings.len(), 2);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 删除绑定保留_worktree_且空保存删除两份布局() {
        let dir = temp_dir("binding-delete-and-empty-save");
        let store = LayoutStore::open_at(&dir).unwrap();
        let binding = binding("p1", "/repo/main");
        store
            .save_worktree_layout(&binding, &layout("cmd"), 1)
            .unwrap();

        store.delete_project_binding("p1").unwrap();
        assert!(!store.load_project_bindings().unwrap().contains_key("p1"));
        assert!(legacy_json(&store, "p1").is_none());
        assert!(worktree_json(&store, &binding.worktree_id).is_some());

        let restored = store
            .reconcile_worktree_layouts(std::slice::from_ref(&binding), 2)
            .unwrap();
        assert!(restored.layouts.contains_key("p1"));
        assert!(legacy_json(&store, "p1").is_some());

        store
            .save_worktree_layout(&binding, &empty_layout(), 3)
            .unwrap();
        assert!(worktree_json(&store, &binding.worktree_id).is_none());
        assert!(legacy_json(&store, "p1").is_none());
        assert_eq!(
            store.load_project_bindings().unwrap().get("p1"),
            Some(&binding),
            "空布局只清内容,不清绑定"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn retain_只删项目绑定与镜像并保留孤儿_worktree() {
        let dir = temp_dir("retain-bindings");
        let store = LayoutStore::open_at(&dir).unwrap();
        let keep = binding("keep", "/repo/keep");
        let stale = binding("stale", "/repo/stale");
        store
            .save_worktree_layout(&keep, &layout("keep-shell"), 1)
            .unwrap();
        store
            .save_worktree_layout(&stale, &layout("stale-shell"), 1)
            .unwrap();

        let live = HashSet::from(["keep".to_string()]);
        store.retain_project_bindings(&live).unwrap();
        let bindings = store.load_project_bindings().unwrap();
        assert!(bindings.contains_key("keep"));
        assert!(!bindings.contains_key("stale"));
        assert!(legacy_json(&store, "stale").is_none());
        assert!(worktree_json(&store, &stale.worktree_id).is_some());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 不兼容的新_schema_不会被当损坏库搬走() {
        let dir = temp_dir("future-schema");
        let path = dir.join("layout.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 INSERT INTO meta(key, value) VALUES('schema_version', '99');
                 CREATE TABLE project_worktree_binding (
                   project_id TEXT PRIMARY KEY,
                   future_payload TEXT NOT NULL
                 );
                 INSERT INTO project_worktree_binding(project_id, future_payload)
                 VALUES('future-project', 'keep-me');",
            )
            .unwrap();
        }

        assert!(LayoutStore::open_at(&dir).is_err());
        assert!(!dir.join("layout.db.corrupt").exists());
        let conn = Connection::open(&path).unwrap();
        let payload: String = conn
            .query_row(
                "SELECT future_payload FROM project_worktree_binding
                 WHERE project_id = 'future-project'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(payload, "keep-me");
        let version: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, "99");
        fs::remove_dir_all(&dir).ok();
    }

    /// 损坏的库不该让持久化整个停摆:挪走留证 + 重建空库,程序照常起来。
    #[test]
    fn 损坏的库挪走并重建() {
        let dir = temp_dir("corrupt");
        fs::write(
            dir.join("layout.db"),
            b"this is definitely not a sqlite file",
        )
        .unwrap();
        let store = LayoutStore::open_at(&dir).unwrap();
        store.save_project_layout("p1", &layout("cmd"), 1).unwrap();
        assert!(store.load_project_layouts().contains_key("p1"));
        assert!(dir.join("layout.db.corrupt").exists(), "旧文件留证");
        fs::remove_dir_all(&dir).ok();
    }

    fn project_stub() -> ProjectConfig {
        ProjectConfig {
            id: String::new(),
            name: String::new(),
            path: String::new(),
            description: None,
            saved_layout: None,
            expanded_dirs: vec![],
            ssh_mcp_enabled: false,
            ssh_cli_token: None,
            ssh_connection_ids: None,
            env_vars: vec![],
            hidden_worktrees: Vec::new(),
            wsl_sessions_distro: None,
            ssh_connection_id: None,
            parent_project_id: None,
            kind_override: None,
        }
    }
}
