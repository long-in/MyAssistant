//! ワークスペース(パーソナル / ワークのエージェント)ごとの実行コンテキストと、エージェントの管理。
//!
//! DB・APIキー・AWS の一時状態をワークスペースごとに分け、混線させない。
//! パーソナルは従来どおりアプリデータ直下、エージェントは`<アプリデータ>/work/<id>/`に置く。
//! エージェント一覧(名称・イメージカラー)は`work/registry.json`に保持する。
use std::{
    collections::HashMap,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use serde::{Deserialize, Serialize};
use tauri::{
    Manager, Runtime,
    ipc::{CommandArg, CommandItem, InvokeError},
};

use super::{
    task_run::{TaskState, RunStatus}, aws::AwsState, batch, batch::BatchGuard, debug_log, debug_log::DebugLog, global_settings::{GlobalStore, SettingsStore}, secrets::Secrets,
};
use crate::{
    db::{Db, Role},
    utils::error::AppError,
};

/// パーソナルワークスペースのID。保存先は従来どおりアプリデータ直下。
pub const PERSONAL_ID: &str = "personal";
/// リーダー(各エージェントのタスク進捗を管理する特別なエージェント)のID。エージェントの一覧・上限・色には含めない。
/// ワークの有効/無効(`1` / `0`)を全体設定に保存するキー。全体設定のバックアップや設定のコピーの対象には入れない。
pub const WORK_ENABLED_KEY: &str = "work_enabled";
pub const LEADER_ID: &str = "leader";
/// リーダーの表示名とイメージカラー(固定。エージェントの色パレットとは別)。
pub const LEADER_NAME: &str = "Leader";
pub const LEADER_COLOR: &str = "#f59e0b";
/// 作れるエージェントの上限。
pub const MAX_AGENTS: usize = 30;
/// エージェント名の最大文字数。
pub const NAME_MAX_CHARS: usize = 40;
/// エージェントに選べるイメージカラー(`#rrggbb`)。上限(`MAX_AGENTS`)と同数で、エージェント同士で重複させない。
/// 色相をずらした明るい15色と暗い15色(明るさの違いと色相差で見分けやすくする)。
pub const PALETTE: [&str; MAX_AGENTS] = [
    "#e61919", "#e66b19", "#e6bd19", "#bde619", "#6be619", "#19e619", "#19e66b", "#19e6bd", "#19bde6", "#196be6",
    "#1919e6", "#6b19e6", "#bd19e6", "#e619bd", "#e6196b", "#a5381d", "#a56e1d", "#a5a51d", "#6ea51d", "#38a51d",
    "#1da538", "#1da56e", "#1da5a5", "#1d6ea5", "#1d38a5", "#381da5", "#6e1da5", "#a51da5", "#a51d6e", "#a51d38",
];
/// ワークスペースが切り替わったことを画面へ知らせるイベント名。
pub const WORKSPACE_CHANGED_EVENT: &str = "workspace-changed";

/// ワークのフォルダ(アプリデータ直下)。リーダー・エージェント・全体設定・共通データを置く。
pub(crate) const DIR_NAME: &str = "work";
/// 削除時に残したナレッジを置くフォルダ(アプリデータ直下)。
pub(crate) const KEPT_DIR_NAME: &str = "kept-knowledge";
pub(crate) const REGISTRY_FILE: &str = "registry.json";
/// ワークスペースごとのDBのファイル名。
pub(crate) const DB_FILE: &str = "timeline.db";

/// 1ワークスペース分の状態。`dir`配下に`timeline.db`と`secrets.json`を置く。
pub struct WorkspaceCtx {
    pub id: String,
    pub dir: PathBuf,
    pub db: Db,
    pub secrets: Secrets,
    pub debug: DebugLog,
    /// 全エージェント共通の全体設定。どのワークスペースも同じファイルを読み、エージェントからは上書きできない。
    pub global: Arc<GlobalStore>,
    /// 同じワークスペースを別々に開いた複数の`WorkspaceCtx`の間で共有する状態。
    pub shared: Arc<Shared>,
}

/// 同じワークスペースの間で共有する状態。バッチのために一時的に開いたものと、画面用に読み込んだものが、
/// 同じ整理バッチを二重に動かしたり、削除に気づかなかったりしないようにする。
#[derive(Default)]
pub struct Shared {
    /// 整理バッチの多重実行を防ぐ。
    pub batch: BatchGuard,
    /// 削除された(または削除中の)エージェントの印。裏で動く処理は、書き込む前にこれを確かめて止まる。
    deleted: AtomicBool,
    /// 作業の依頼の実行状態(承認待ち・中断・進捗の記録)。切り替えても止めず、戻ったときに続きから再開する。
    pub task: TaskState,
    /// AWSの一時クレデンシャルとMFA入力待ち。バッチのために一時的に開いたものが待っているMFAにも、IDから答えられるようにする。
    pub aws: AwsState,
}

impl WorkspaceCtx {
    /// `dir`を開く(無ければ作る)。
    /// 全体設定は`dir`を保存先の基準にする(パーソナルと、単体で開くテスト用)。
    pub fn open(id: &str, dir: &Path) -> Result<Self, AppError> {
        Self::open_shared(id, dir, Arc::new(Shared::default()), Arc::new(GlobalStore::open(dir)?))
    }

    /// 共有状態と全体設定を指定して開く。同じワークスペースを開き直すときに、前の状態を引き継ぐ。
    /// リーダーのDBだけが台帳を持つ。
    pub fn open_shared(id: &str, dir: &Path, shared: Arc<Shared>, global: Arc<GlobalStore>) -> Result<Self, AppError> {
        std::fs::create_dir_all(dir).map_err(|e| AppError::Io(e.to_string()))?;
        let role = if id == LEADER_ID { Role::Leader } else { Role::Agent };
        let db = Db::open_as(&dir.join(DB_FILE), role)?;
        let ctx = Self {
            id: id.to_owned(),
            dir: dir.to_owned(),
            db,
            secrets: Secrets::open(dir)?,
            debug: DebugLog::default(),
            global,
            shared,
        };
        ctx.prepare_knowledge();
        Ok(ctx)
    }

    /// ナレッジのフォルダ構成を整える(失敗しても開くのは止めず、ログだけ残す)。
    /// フォルダ名を小文字にそろえ、アプリが使うフォルダ(`data`・`inbox`・`memory`・`skills`・`captures`・`preferences`)を(無ければ)作る。
    fn prepare_knowledge(&self) {
        let Ok(root) = super::batch::knowledge_root(self) else { return };
        // 以前のナレッジのコマンド権限のファイルは、DBへ取り込む
        super::command_policy::import_file(&self.db, &root);
        if root.is_dir() {
            let n = super::knowledge::lowercase_dirs(&root);
            if n > 0 {
                log::info!("lowercased {n} knowledge folder(s) of {}", self.id);
            }
        }
        // 画面から見えるよう、アプリが使うフォルダを(無ければ)作る。記憶はリーダーには無い
        if let Err(e) = super::knowledge::ensure_system_dirs(&root, self.id != LEADER_ID) {
            log::warn!("system folders not prepared for {}: {e}", self.id);
        }
    }

    /// 削除されていないことを確かめる。バッチや会話が、削除済みのエージェントへ書き込まないようにする。
    pub fn ensure_alive(&self) -> Result<(), AppError> {
        if self.shared.deleted.load(Ordering::SeqCst) { Err(AppError::AgentNotFound) } else { Ok(()) }
    }
}

/// ワークスペース内の1エージェント(ワーキングエージェント)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Agent {
    pub id: String,
    pub name: String,
    /// イメージカラー(`#rrggbb`)。
    pub color: String,
    pub created_at: String,
}

/// 現在選択中のワークスペース。`agent`は個人のとき`None`。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ActiveWorkspace {
    pub id: String,
    pub agent: Option<Agent>,
}

/// エージェント名にできない名称(切り替えやバッジで、パーソナル・ワークと紛らわしくなるもの)。大文字小文字・前後の空白は無視して比べる。
const RESERVED_NAMES: [&str; 6] = ["パーソナル", "ワーク", "リーダー", "personal", "work", "leader"];

/// 名称を整える。前後の空白を除き、1〜`NAME_MAX_CHARS`文字で、制御文字を含まず、予約名でないこと。
fn clean_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    let chars = name.chars().count();
    if chars == 0 || chars > NAME_MAX_CHARS || name.chars().any(char::is_control) {
        return Err(AppError::InvalidAgent("name".into()));
    }
    if RESERVED_NAMES.contains(&name.to_lowercase().as_str()) {
        return Err(AppError::ReservedName);
    }
    Ok(name.to_owned())
}

/// `PALETTE`の色だけを受け付け、小文字へそろえる。
fn clean_color(color: &str) -> Result<String, AppError> {
    let c = color.trim().to_ascii_lowercase();
    if !PALETTE.contains(&c.as_str()) {
        return Err(AppError::InvalidAgent("color".into()));
    }
    Ok(c)
}

/// `name`と`color`を、`except`以外のエージェントがすでに使っていれば拒否する。
/// 名称は大文字小文字を区別せず比較する(`name`は整形済みであること)。
fn ensure_unique(agents: &[Agent], name: &str, color: &str, except: Option<&str>) -> Result<(), AppError> {
    let others = || agents.iter().filter(|p| Some(p.id.as_str()) != except);
    if others().any(|p| p.name.to_lowercase() == name.to_lowercase()) {
        return Err(AppError::NameInUse);
    }
    if others().any(|p| p.color == color) {
        return Err(AppError::ColorInUse);
    }
    Ok(())
}

/// 読み込み済みのワークスペースと、現在選択中のID、エージェント一覧。
pub struct Workspaces {
    /// アプリデータディレクトリ(パーソナルの保存先でもある)。
    root: PathBuf,
    /// 全体設定。パーソナルが開いたものを、すべてのエージェントで共有する。
    global: Arc<GlobalStore>,
    /// 一覧・読み込み・選択・保存先の変更を1つずつ行うための外側のロック。
    /// 削除と切り替え・読み込みが交差して、存在しないワークスペースを選んだり、削除済みを開き直したりしないようにする。
    op: Mutex<()>,
    loaded: RwLock<HashMap<String, Arc<WorkspaceCtx>>>,
    /// ワークスペースIDごとの共有状態(読み込んでいないエージェントの分も、一時的に開くときのために持つ)。
    shared: Mutex<HashMap<String, Arc<Shared>>>,
    active: RwLock<String>,
    agents: Mutex<Vec<Agent>>,
    /// リーダー。作るまでは`None`。選択(`active`)の対象にはならず、`agents`にも入らない。
    leader: RwLock<Option<Arc<WorkspaceCtx>>>,
}

impl Workspaces {
    /// アプリデータディレクトリ。全エージェント共通のファイルの置き場所。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// パーソナルを選択した状態で始め、保存済みのエージェント一覧を読む。
    /// 一覧が壊れていたら`.corrupt`へ退避して空から始める(エージェントのフォルダは残す)。
    pub fn new(root: &Path, personal: WorkspaceCtx) -> Self {
        let agents = load_registry(root);
        let id = personal.id.clone();
        let global = personal.global.clone();
        // リーダーは、作成済み(フォルダがある)なら開く。開けなければ警告して、未作成として扱う
        let leader_dir = root.join(DIR_NAME).join(LEADER_ID);
        let leader = leader_dir
            .join(DB_FILE)
            .exists()
            .then(|| WorkspaceCtx::open_shared(LEADER_ID, &leader_dir, Arc::new(Shared::default()), global.clone()))
            .and_then(|r| r.map_err(|e| log::warn!("leader not opened: {e}")).ok())
            .map(Arc::new);
        // リーダーは、選択できるよう、読み込み済み・共有状態の一覧にも入れる(`agents`には入れない)
        let mut shared = HashMap::from([(id.clone(), personal.shared.clone())]);
        let mut loaded = HashMap::from([(id.clone(), Arc::new(personal))]);
        if let Some(l) = &leader {
            shared.insert(LEADER_ID.to_owned(), l.shared.clone());
            loaded.insert(LEADER_ID.to_owned(), l.clone());
            if let Err(e) = debug_log::refresh(l) {
                log::warn!("debug log init failed for leader: {e}");
            }
        }
        // 切り替えるまで開かれないエージェントも、ナレッジのフォルダ構成(小文字化・`data/`)を起動時に整える。開くだけで、読み込んだままにしない
        for agent in &agents {
            let dir = root.join(DIR_NAME).join(&agent.id);
            if let Err(e) = WorkspaceCtx::open_shared(&agent.id, &dir, Arc::new(Shared::default()), global.clone()) {
                log::warn!("agent {} not prepared at startup: {e}", agent.id);
            }
        }
        Self {
            leader: RwLock::new(leader),
            root: root.to_owned(),
            global,
            op: Mutex::new(()),
            shared: Mutex::new(shared),
            loaded: RwLock::new(loaded),
            active: RwLock::new(id),
            agents: Mutex::new(agents),
        }
    }

    fn op(&self) -> std::sync::MutexGuard<'_, ()> {
        self.op.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn agents(&self) -> std::sync::MutexGuard<'_, Vec<Agent>> {
        self.agents.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn agent_dir(&self, id: &str) -> PathBuf {
        self.root.join(DIR_NAME).join(id)
    }

    /// エージェントまたはリーダーの表示用の名称とイメージカラー。パーソナルや一覧に無いIDは`None`。
    pub fn label(&self, id: &str) -> Option<(String, String)> {
        if id == LEADER_ID {
            return self.leader().map(|_| (LEADER_NAME.to_owned(), LEADER_COLOR.to_owned()));
        }
        self.agents().iter().find(|s| s.id == id).map(|s| (s.name.clone(), s.color.clone()))
    }

    /// リーダー。作成前は`None`。
    pub fn leader(&self) -> Option<Arc<WorkspaceCtx>> {
        self.leader.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// ワークが有効か。全体設定に`0`と保存されているときだけ無効(未保存は有効)。
    /// 無効のあいだも、リーダー・エージェントのデータはディスクに残る。選択・配布・バックグラウンド処理の入口だけを閉じる。
    pub fn work_enabled(&self) -> bool {
        self.global.get_setting(WORK_ENABLED_KEY).ok().flatten().as_deref() != Some("0")
    }

    /// ワークの有効/無効を切り替える。無効にするときは、リーダーが作成済みで、ワークの中に実行中・承認待ちが無いこと。
    /// ワークを開いていたらパーソナルへ戻す。戻したかを返す。
    pub fn set_work_enabled(&self, enabled: bool) -> Result<bool, AppError> {
        let _op = self.op();
        if self.leader().is_none() {
            return Err(AppError::LeaderRequired);
        }
        let mut moved = false;
        if !enabled {
            let busy = |id: &str| matches!(self.run_status(id), RunStatus::Running | RunStatus::AwaitingApproval);
            if busy(LEADER_ID) || self.agents().iter().any(|p| busy(&p.id)) {
                return Err(AppError::WorkBusy);
            }
            let mut active = self.active.write().unwrap_or_else(|p| p.into_inner());
            if *active != PERSONAL_ID {
                *active = PERSONAL_ID.to_owned();
                moved = true;
            }
        }
        self.global.set_setting(WORK_ENABLED_KEY, if enabled { "1" } else { "0" })?;
        Ok(moved)
    }

    /// リーダーがあり、ワークが有効であることを確かめる(エージェントを作る前の条件)。
    pub fn ensure_leader(&self) -> Result<Arc<WorkspaceCtx>, AppError> {
        let leader = self.leader().ok_or(AppError::LeaderRequired)?;
        if !self.work_enabled() {
            return Err(AppError::WorkDisabled);
        }
        Ok(leader)
    }

    /// リーダーを作る(1つだけ)。専用の`timeline.db`と`secrets.json`を`work/leader/`に持つ。
    pub fn create_leader(&self) -> Result<Arc<WorkspaceCtx>, AppError> {
        let _op = self.op();
        let mut slot = self.leader.write().unwrap_or_else(|p| p.into_inner());
        if slot.is_some() {
            return Err(AppError::LeaderExists);
        }
        let dir = self.root.join(DIR_NAME).join(LEADER_ID);
        let ctx = Arc::new(WorkspaceCtx::open_shared(LEADER_ID, &dir, Arc::new(Shared::default()), self.global.clone())?);
        if let Err(e) = debug_log::refresh(&ctx) {
            log::warn!("debug log init failed for leader: {e}");
        }
        // 選択・整理バッチ・保存先の確認の対象にするため、読み込み済み・共有状態にも登録する
        self.shared.lock().unwrap_or_else(|p| p.into_inner()).insert(LEADER_ID.to_owned(), ctx.shared.clone());
        self.loaded.write().unwrap_or_else(|p| p.into_inner()).insert(LEADER_ID.to_owned(), ctx.clone());
        *slot = Some(ctx.clone());
        Ok(ctx)
    }

    /// 開いている(読み込み済みの)ワークスペースの依頼の状態。一度も開いていなければ`Idle`。開かず、中身も読まない。
    pub fn run_status(&self, id: &str) -> RunStatus {
        let shared = self.shared.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned();
        shared.map_or(RunStatus::Idle, |s| s.task.status())
    }

    /// ワークスペースの直近の依頼のセッションID(無ければ空)。
    pub fn run_session(&self, id: &str) -> String {
        let shared = self.shared.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned();
        shared.map(|s| s.task.session_id()).unwrap_or_default()
    }

    fn personal(&self) -> Arc<WorkspaceCtx> {
        self.get(PERSONAL_ID).expect("personal is always loaded")
    }

    /// 現在選択中のワークスペース。見つからなければ(削除と交差したときなど)パーソナルを返す。
    pub fn active(&self) -> Arc<WorkspaceCtx> {
        let id = self.active.read().unwrap_or_else(|p| p.into_inner()).clone();
        self.get(&id).unwrap_or_else(|| self.personal())
    }

    pub fn active_info(&self) -> ActiveWorkspace {
        let id = self.active().id.clone();
        let agent = self.agents().iter().find(|p| p.id == id).cloned();
        ActiveWorkspace { id, agent }
    }

    pub fn get(&self, id: &str) -> Option<Arc<WorkspaceCtx>> {
        self.loaded.read().unwrap_or_else(|p| p.into_inner()).get(id).cloned()
    }

    fn shared_for(&self, id: &str) -> Arc<Shared> {
        self.shared.lock().unwrap_or_else(|p| p.into_inner()).entry(id.to_owned()).or_default().clone()
    }

    /// バックグラウンド処理の対象になるワークスペースのID(パーソナル・リーダー・すべてのエージェント)。開かずに返す。
    pub fn batch_ids(&self) -> Vec<String> {
        let _op = self.op();
        self.ids_locked()
    }

    /// パーソナル・(あれば)リーダー・すべてのエージェントのID。
    fn ids_locked(&self) -> Vec<String> {
        if !self.work_enabled() {
            return vec![PERSONAL_ID.to_owned()];
        }
        std::iter::once(PERSONAL_ID.to_owned())
            .chain(self.leader().map(|_| LEADER_ID.to_owned()))
            .chain(self.agents().iter().map(|p| p.id.clone()))
            .collect()
    }

    /// 読み込み済みならそれを、未読み込みなら一時的に開いて返す(読み込み済みの一覧には加えない)。`op`を持った状態で呼ぶ。
    /// 一時的に開いたものは、使い終わったら捨てられる。共有状態は読み込み済みのものと同じ。
    fn open_transient_locked(&self, id: &str) -> Result<Arc<WorkspaceCtx>, AppError> {
        if let Some(ctx) = self.get(id) {
            return Ok(ctx);
        }
        if !self.agents().iter().any(|p| p.id == id) {
            return Err(AppError::AgentNotFound);
        }
        let ctx = Arc::new(WorkspaceCtx::open_shared(id, &self.agent_dir(id), self.shared_for(id), self.global.clone())?);
        if let Err(e) = debug_log::refresh(&ctx) {
            log::warn!("debug log init failed for agent {id}: {e}");
        }
        Ok(ctx)
    }

    /// 切り替えずにナレッジを操作するために開く。パーソナルは常に、ほかはワークが有効なときだけ。
    /// 未読み込みのエージェントは一時的に開くだけで、読み込んだままにしない。
    pub fn open_for_knowledge(&self, id: &str) -> Result<Arc<WorkspaceCtx>, AppError> {
        let _op = self.op();
        if id == PERSONAL_ID {
            return Ok(self.personal());
        }
        if !self.work_enabled() {
            return Err(AppError::WorkDisabled);
        }
        self.open_transient_locked(id)
    }

    /// すべてのワークスペースを、保存先の確認のために開く(未読み込みは一時的に開くだけ)。開けないものは警告して飛ばす。
    fn all_for_check_locked(&self) -> Vec<Arc<WorkspaceCtx>> {
        let ids = self.ids_locked();
        ids.iter()
            .filter_map(|id| {
                self.open_transient_locked(id).map_err(|e| log::warn!("workspace {id} not opened: {e}")).ok()
            })
            .collect()
    }

    /// エージェントを(未読み込みなら)開く。一覧に無いIDは開かない。`op`を持った状態で呼ぶ。
    fn load_locked(&self, id: &str) -> Result<Arc<WorkspaceCtx>, AppError> {
        if let Some(ctx) = self.get(id) {
            return Ok(ctx);
        }
        if !self.agents().iter().any(|p| p.id == id) {
            return Err(AppError::AgentNotFound);
        }
        let ctx = Arc::new(WorkspaceCtx::open_shared(id, &self.agent_dir(id), self.shared_for(id), self.global.clone())?);
        // デバッグログの出力先は、開いたときに決める(裏で動く処理も自分のワークスペースのログへ書く)
        if let Err(e) = debug_log::refresh(&ctx) {
            log::warn!("debug log init failed for agent {id}: {e}");
        }
        self.loaded.write().unwrap_or_else(|p| p.into_inner()).insert(id.to_owned(), ctx.clone());
        Ok(ctx)
    }

    /// IDからワークスペースを得る(エージェントは未読み込みなら開く)。一覧に無いIDは`AgentNotFound`。
    pub fn resolve(&self, id: &str) -> Result<Arc<WorkspaceCtx>, AppError> {
        let _op = self.op();
        if id == PERSONAL_ID { Ok(self.personal()) } else { self.load_locked(id) }
    }

    /// `candidate`(ナレッジやデバッグログの保存先にしたい場所)が使えることを確かめる。`op`を持った状態で呼ぶ。
    /// - アプリのデータ(アプリデータ全体・`work/`・ほかのワークスペースのDBやキー)に触れる場所は、自分の既定のナレッジの中を除いて拒否する。
    /// - `id`以外のワークスペースの保存先(現在の場所と既定の場所の両方で、ナレッジとデバッグログ)と、同じ場所・親子関係で重なる場所は拒否する。
    fn check_paths_free(&self, id: &str, candidate: &Path) -> Result<(), AppError> {
        let candidate = canonical_lenient(candidate);
        let own = if id == PERSONAL_ID { Some(self.personal()) } else { self.get(id) };
        let own_default = own.map(|c| canonical_lenient(&batch::default_knowledge_root(&c)));
        if overlaps(&candidate, &canonical_lenient(&self.root)) && !own_default.is_some_and(|d| candidate.starts_with(d)) {
            return Err(AppError::PathInUse);
        }
        for other in self.all_for_check_locked().into_iter().filter(|c| c.id != id) {
            for claimed in claimed_paths(&other) {
                if overlaps(&candidate, &canonical_lenient(&claimed)) {
                    return Err(AppError::PathInUse);
                }
            }
        }
        Ok(())
    }

    /// 保存先を使えることを確かめる(確認のみ。テスト用)。実際に保存するときは、確認と保存を分けない`set_location`を使う。
    #[cfg(test)]
    pub fn ensure_paths_free(&self, id: &str, candidate: &Path) -> Result<(), AppError> {
        let _op = self.op();
        self.check_paths_free(id, candidate)
    }

    /// 保存先を確かめ、使えるときだけ`save`を実行する。確認から保存までを他の操作(別の保存先の変更・削除など)と交差させない。
    /// `save`の中でこの`Workspaces`のメソッドを呼ばないこと。
    pub fn set_location<T>(&self, id: &str, candidate: &Path, save: impl FnOnce() -> Result<T, AppError>) -> Result<T, AppError> {
        let _op = self.op();
        self.check_paths_free(id, candidate)?;
        save()
    }

    pub fn list(&self) -> Vec<Agent> {
        self.agents().clone()
    }

    /// エージェントを作る。名称・色を検証し、上限(`MAX_AGENTS`)を超える場合は拒否する。
    pub fn create(&self, name: &str, color: &str) -> Result<Agent, AppError> {
        let (name, color) = (clean_name(name)?, clean_color(color)?);
        let _op = self.op();
        let agents = self.agents();
        if agents.len() >= MAX_AGENTS {
            return Err(AppError::AgentLimit);
        }
        ensure_unique(&agents, &name, &color, None)?;
        let agent = Agent {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            color,
            created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        };
        // 既存のワークスペースが、新しいエージェントのフォルダを含む場所を保存先にしていたら作らない
        // (古い設定でアプリデータ全体を保存先にしている場合など。そのままではキーやDBがナレッジから見えてしまう)
        let dir = canonical_lenient(&self.agent_dir(&agent.id));
        drop(agents);
        for other in self.all_for_check_locked() {
            if claimed_paths(&other).iter().any(|c| overlaps(&dir, &canonical_lenient(c))) {
                return Err(AppError::PathInUse);
            }
        }
        let mut agents = self.agents();
        // 先にフォルダとDBを作れることを確かめてから一覧へ載せる
        let ctx = Arc::new(WorkspaceCtx::open_shared(&agent.id, &self.agent_dir(&agent.id), self.shared_for(&agent.id), self.global.clone())?);
        let mut next = agents.clone();
        next.push(agent.clone());
        save_registry(&self.root, &next)?;
        *agents = next;
        if let Err(e) = debug_log::refresh(&ctx) {
            log::warn!("debug log init failed for agent {}: {e}", agent.id);
        }
        self.loaded.write().unwrap_or_else(|p| p.into_inner()).insert(agent.id.clone(), ctx);
        Ok(agent)
    }

    /// 名称とイメージカラーを変える。
    pub fn update(&self, id: &str, name: &str, color: &str) -> Result<Agent, AppError> {
        let (name, color) = (clean_name(name)?, clean_color(color)?);
        let _op = self.op();
        let mut agents = self.agents();
        ensure_unique(&agents, &name, &color, Some(id))?;
        let mut next = agents.clone();
        let target = next.iter_mut().find(|p| p.id == id).ok_or(AppError::AgentNotFound)?;
        target.name = name;
        target.color = color;
        let updated = target.clone();
        save_registry(&self.root, &next)?;
        *agents = next;
        Ok(updated)
    }

    /// エージェントを選択する。`PERSONAL_ID`でパーソナルへ戻る。
    pub fn switch(&self, id: &str) -> Result<ActiveWorkspace, AppError> {
        let _op = self.op();
        if id != PERSONAL_ID {
            if !self.work_enabled() {
                return Err(AppError::WorkDisabled);
            }
            self.load_locked(id)?;
        }
        *self.active.write().unwrap_or_else(|p| p.into_inner()) = id.to_owned();
        drop(_op);
        Ok(self.active_info())
    }

    /// エージェントを一覧とディスクから削除する。選択中ならパーソナルへ戻す。元に戻せない。
    /// `delete_knowledge`が偽なら、既定の場所のナレッジ(とデバッグログ)を、削除前に`kept-knowledge/`へ移して残す。
    /// 選択中を切り戻したかと、残した場所を返す。裏で動いている処理には削除の印(`ensure_alive`)が付き、書き込む前に止まる。
    /// アプリが管理するフォルダ(`work/<id>/`)だけを消す。利用者が指定したナレッジの保存先は消さない。
    pub fn delete(&self, id: &str, delete_knowledge: bool) -> Result<Deleted, AppError> {
        let _op = self.op();
        let mut agents = self.agents();
        let Some(agent) = agents.iter().find(|p| p.id == id).cloned() else {
            return Err(AppError::AgentNotFound);
        };
        // ナレッジを残すときは、消す前に移す。移せなければ何も消さずに失敗する
        let kept = if delete_knowledge { None } else { self.keep_knowledge(&agent)? };
        let next: Vec<Agent> = agents.iter().filter(|p| p.id != id).cloned().collect();
        if let Err(e) = save_registry(&self.root, &next) {
            if let Some((from, to)) = &kept {
                let _ = std::fs::rename(to, from); // 一覧が変わらないので、ナレッジも元の場所へ戻す
            }
            return Err(e);
        }
        *agents = next;
        drop(agents);
        let was_active = {
            let mut active = self.active.write().unwrap_or_else(|p| p.into_inner());
            let was = *active == id;
            if was {
                *active = PERSONAL_ID.to_owned();
            }
            was
        };
        // 読み込み済みでも、バッチのために一時的に開かれたものでも、同じ共有状態の印で止まる
        if let Some(shared) = self.shared.lock().unwrap_or_else(|p| p.into_inner()).remove(id) {
            shared.deleted.store(true, Ordering::SeqCst);
            // 実行中の依頼は中断する(動いているコマンドも止まり、承認待ちは拒否になる)
            shared.task.abort();
        }
        self.loaded.write().unwrap_or_else(|p| p.into_inner()).remove(id);
        // サンドボックスのコンテナ・ボリューム・ネットワーク・イメージを消す(失敗しても削除は続ける)
        super::sandbox::remove_agent(id);
        // `id`は一覧にあったもの(生成したUUID)だけなので、`work/`配下の1フォルダに限られる
        if let Err(e) = std::fs::remove_dir_all(self.agent_dir(id))
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("agent dir not removed: {e}");
        }
        Ok(Deleted { was_active, kept_knowledge: kept.map(|(_, to)| to.to_string_lossy().into_owned()) })
    }

    /// 既定の場所のナレッジを`kept-knowledge/<名称>-<ID先頭>/`へ移す。無ければ`None`。移す前と後の場所を返す。
    fn keep_knowledge(&self, agent: &Agent) -> Result<Option<(PathBuf, PathBuf)>, AppError> {
        let from = self.agent_dir(&agent.id).join("knowledge");
        if !from.is_dir() || is_empty_scaffold(&from) {
            return Ok(None);
        }
        let parent = self.root.join(KEPT_DIR_NAME);
        std::fs::create_dir_all(&parent).map_err(|e| AppError::Io(e.to_string()))?;
        let id8: String = agent.id.chars().take(8).collect();
        let to = parent.join(format!("{}-{id8}", folder_name(&agent.name)));
        if to.exists() {
            return Err(AppError::PathInUse);
        }
        std::fs::rename(&from, &to).map_err(|e| AppError::Io(e.to_string()))?;
        Ok(Some((from, to)))
    }
}

/// 空のフォルダだけのナレッジか(自動で作った`data/`・`inbox/`などの枠だけで、残すものが無い)。
fn is_empty_scaffold(knowledge: &Path) -> bool {
    fn only_empty_dirs(dir: &Path) -> bool {
        std::fs::read_dir(dir).is_ok_and(|d| d.flatten().all(|e| e.file_type().is_ok_and(|t| t.is_dir()) && only_empty_dirs(&e.path())))
    }
    only_empty_dirs(knowledge)
}

/// エージェントの削除の結果。
#[derive(Debug)]
pub struct Deleted {
    /// 選択中のエージェントを削除して、パーソナルへ戻ったか。
    pub was_active: bool,
    /// 残したナレッジの場所(残さなかった、または、既定の場所にナレッジが無いときは`None`)。
    pub kept_knowledge: Option<String>,
}

/// 名称を、フォルダ名に使える形にする(区切り文字や制御文字を`_`に、前後の空白とドットを除く)。
fn folder_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let s = s.trim_matches(|c: char| c.is_whitespace() || c == '.');
    if s.is_empty() { "agent".to_owned() } else { s.to_owned() }
}

/// そのワークスペースが使う(または、既定に戻すと使う)ナレッジとデバッグログの場所。
fn claimed_paths(ctx: &WorkspaceCtx) -> Vec<PathBuf> {
    let default_root = batch::default_knowledge_root(ctx);
    let mut paths = vec![default_root.join(debug_log::DIR_NAME), default_root];
    if let Ok(root) = batch::knowledge_root(ctx) {
        paths.push(root);
    }
    if let Ok(dir) = debug_log::log_dir(ctx) {
        paths.push(dir);
    }
    paths
}

/// 2つの場所が、同じか、一方が他方の中にあるか。
fn overlaps(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// 存在しない場所でも比べられるよう、存在する最も深い親を正規化(シンボリックリンクを解決)して、残りをつなぐ。
fn canonical_lenient(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut cur = path;
    loop {
        if let Ok(base) = cur.canonicalize() {
            return rest.iter().rev().fold(base, |acc, part| acc.join(part));
        }
        match (cur.parent(), cur.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                cur = parent;
            }
            _ => return path.to_owned(),
        }
    }
}

fn registry_path(root: &Path) -> PathBuf {
    root.join(DIR_NAME).join(REGISTRY_FILE)
}

fn load_registry(root: &Path) -> Vec<Agent> {
    let path = registry_path(root);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            log::warn!("registry unreadable: {e}");
            return Vec::new();
        }
    };
    match serde_json::from_str::<Vec<Agent>>(&text) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("registry corrupt, moved aside: {e}");
            let _ = std::fs::rename(&path, path.with_extension("json.corrupt"));
            Vec::new()
        }
    }
}

/// 一時ファイルへ書いてからリネームし、書き込み途中の破損を避ける。
fn save_registry(root: &Path, agents: &[Agent]) -> Result<(), AppError> {
    let io = |e: std::io::Error| AppError::Io(e.to_string());
    let path = registry_path(root);
    std::fs::create_dir_all(root.join(DIR_NAME)).map_err(io)?;
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(agents).map_err(|e| AppError::Io(e.to_string()))?;
    std::fs::write(&tmp, json).map_err(io)?;
    std::fs::rename(&tmp, &path).map_err(io)
}

/// コマンド引数で現在のワークスペースを受け取る。`ctx: Ctx`と書く。
pub struct Ctx(pub Arc<WorkspaceCtx>);

impl Deref for Ctx {
    type Target = WorkspaceCtx;
    fn deref(&self) -> &WorkspaceCtx {
        &self.0
    }
}

impl<'de, R: Runtime> CommandArg<'de, R> for Ctx {
    fn from_command(command: CommandItem<'de, R>) -> Result<Self, InvokeError> {
        let webview = command.message.webview_ref();
        let ws = webview
            .try_state::<Workspaces>()
            .ok_or_else(|| InvokeError::from("workspaces not initialized"))?;
        Ok(Ctx(ws.active()))
    }
}

/// `AppHandle`などから現在のワークスペースを得る。
pub fn active<R: Runtime>(app: &impl Manager<R>) -> Arc<WorkspaceCtx> {
    app.state::<Workspaces>().active()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspaces(root: &Path) -> Workspaces {
        Workspaces::new(root, WorkspaceCtx::open(PERSONAL_ID, root).unwrap())
    }

    #[test]
    fn disabling_work_closes_the_entrances_but_keeps_the_data() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        assert!(ws.work_enabled(), "未保存は有効");
        assert!(matches!(ws.set_work_enabled(false), Err(AppError::LeaderRequired)));
        ws.create_leader().unwrap();
        let agent = ws.create("A", PALETTE[0]).unwrap();
        ws.switch(&agent.id).unwrap();
        // 開いていたら、パーソナルへ戻る
        assert!(ws.set_work_enabled(false).unwrap());
        assert!(!ws.work_enabled());
        assert_eq!(ws.active().id, PERSONAL_ID);
        assert!(matches!(ws.switch(&agent.id), Err(AppError::WorkDisabled)));
        assert!(matches!(ws.switch(LEADER_ID), Err(AppError::WorkDisabled)));
        assert!(matches!(ws.ensure_leader(), Err(AppError::WorkDisabled)));
        assert_eq!(ws.batch_ids(), vec![PERSONAL_ID.to_owned()]);
        // データは残り、再起動後も無効のまま。有効に戻せば元どおり使える
        drop(ws);
        let again = workspaces(d.path());
        assert!(!again.work_enabled());
        assert_eq!(again.list().len(), 1);
        assert!(again.leader().is_some());
        assert!(!again.set_work_enabled(true).unwrap());
        assert_eq!(again.switch(&agent.id).unwrap().id, agent.id);
    }

    #[test]
    fn leader_is_created_once_outside_the_agent_list() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        assert!(matches!(ws.ensure_leader(), Err(AppError::LeaderRequired)));
        let leader = ws.create_leader().unwrap();
        assert_eq!(leader.id, LEADER_ID);
        assert!(matches!(ws.create_leader(), Err(AppError::LeaderExists)));
        // エージェントの一覧・上限・色には入らない
        assert!(ws.list().is_empty());
        assert_eq!(ws.active().id, PERSONAL_ID);
        // 選択はできる。Ctxの元になる`active()`も、パーソナルに落ちずリーダーを返す
        assert_eq!(ws.switch(LEADER_ID).unwrap().id, LEADER_ID);
        assert_eq!(ws.active().id, LEADER_ID);
        assert!(ws.active_info().agent.is_none());
        assert!(Arc::ptr_eq(&ws.resolve(LEADER_ID).unwrap(), &leader));
        assert_eq!(ws.run_status(LEADER_ID), RunStatus::Idle);
        assert_eq!(ws.label(LEADER_ID), Some((LEADER_NAME.into(), LEADER_COLOR.into())));
        assert_eq!(ws.label(PERSONAL_ID), None);
        assert!(ws.batch_ids().contains(&LEADER_ID.to_owned()), "整理バッチの対象に入る");
        ws.switch(PERSONAL_ID).unwrap();
        // 専用のDBとキーを持ち、パーソナルと混ざらない
        leader.secrets.set_tts_key("leader-only").unwrap();
        assert!(!ws.resolve(PERSONAL_ID).unwrap().secrets.has_tts_key().unwrap());
        // 再起動後も開き直される
        drop(ws);
        let again = workspaces(d.path());
        assert!(again.leader().unwrap().secrets.has_tts_key().unwrap());
    }

    #[test]
    fn leader_names_are_reserved() {
        assert!(matches!(clean_name("リーダー"), Err(AppError::ReservedName)));
        assert!(matches!(clean_name(" Leader "), Err(AppError::ReservedName)));
        assert!(clean_name("リーダー補佐").is_ok());
    }

    #[test]
    fn run_status_reads_loaded_agent_without_opening() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let a = ws.create("A", PALETTE[0]).unwrap();
        assert_eq!(ws.run_status(&a.id), RunStatus::Idle);
        let ctx = ws.resolve(&a.id).unwrap();
        assert!(ctx.shared.task.try_begin_detached("s1", "x"));
        assert_eq!((ws.run_status(&a.id), ws.run_session(&a.id).as_str()), (RunStatus::Running, "s1"));
        assert_eq!(ws.run_status("unknown"), RunStatus::Idle);
    }

    #[test]
    fn contexts_do_not_share_data() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let a = ws.create("A", PALETTE[0]).unwrap();
        let b = ws.create("B", PALETTE[1]).unwrap();
        ws.switch(&a.id).unwrap();
        ws.active().db.set_setting("k", "a").unwrap();
        ws.active().secrets.set_tts_key("key-a").unwrap();
        ws.switch(&b.id).unwrap();
        assert_eq!(ws.active().db.get_setting("k").unwrap(), None);
        assert!(!ws.active().secrets.has_tts_key().unwrap());
        ws.switch(PERSONAL_ID).unwrap();
        assert_eq!(ws.active().db.get_setting("k").unwrap(), None);
        assert_eq!(ws.get(&a.id).unwrap().db.get_setting("k").unwrap().as_deref(), Some("a"));
    }

    #[test]
    fn validates_name_and_color() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        for name in ["", "   ", "a\nb", &"あ".repeat(NAME_MAX_CHARS + 1)] {
            assert!(matches!(ws.create(name, PALETTE[0]), Err(AppError::InvalidAgent(_))), "{name:?}");
        }
        for color in ["", "red", "#12345", "#1234567", "#12345g", "112233", "#000000", "#ffffff"] {
            assert!(matches!(ws.create("x", color), Err(AppError::InvalidAgent(_))), "{color:?}");
        }
        let p = ws.create("  営業  ", &PALETTE[0].to_uppercase()).unwrap();
        assert_eq!((p.name.as_str(), p.color.as_str()), ("営業", PALETTE[0]));
        assert!(ws.create(&"あ".repeat(NAME_MAX_CHARS), PALETTE[1]).is_ok());
    }

    #[test]
    fn enforces_agent_limit() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        for (i, color) in PALETTE.iter().enumerate().take(MAX_AGENTS) {
            ws.create(&format!("p{i}"), color).unwrap();
        }
        assert!(matches!(ws.create("over", PALETTE[0]), Err(AppError::AgentLimit)));
        let (first, first_color) = (ws.list()[0].id.clone(), ws.list()[0].color.clone());
        ws.delete(&first, true).unwrap();
        assert!(ws.create("again", &first_color).is_ok());
    }

    #[test]
    fn registry_persists_and_agents_reload_lazily() {
        let d = tempfile::tempdir().unwrap();
        let id = {
            let ws = workspaces(d.path());
            let p = ws.create("A", PALETTE[0]).unwrap();
            ws.get(&p.id).unwrap().db.set_setting("k", "v").unwrap();
            ws.update(&p.id, "A2", PALETTE[2]).unwrap();
            p.id
        };
        let ws = workspaces(d.path());
        assert!(ws.get(&id).is_none(), "not loaded until needed");
        assert_eq!(ws.list()[0].name, "A2");
        let info = ws.switch(&id).unwrap();
        assert_eq!(info.agent.unwrap().color, PALETTE[2]);
        assert_eq!(ws.active().db.get_setting("k").unwrap().as_deref(), Some("v"));
    }

    #[test]
    fn switch_to_unknown_agent_fails_and_keeps_active() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        assert!(matches!(ws.switch("../etc"), Err(AppError::AgentNotFound)));
        assert_eq!(ws.active_info().id, PERSONAL_ID);
    }

    #[test]
    fn delete_removes_data_and_falls_back_to_personal() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let p = ws.create("A", PALETTE[0]).unwrap();
        ws.switch(&p.id).unwrap();
        assert!(d.path().join(DIR_NAME).join(&p.id).exists());
        ws.delete(&p.id, true).unwrap();
        assert_eq!(ws.active_info().id, PERSONAL_ID);
        assert!(!d.path().join(DIR_NAME).join(&p.id).exists());
        assert!(ws.list().is_empty());
        assert!(matches!(ws.delete(&p.id, true), Err(AppError::AgentNotFound)));
        // 一覧に無いIDでは何も消えない(パスの注入対策)
        assert!(matches!(ws.delete("..", true), Err(AppError::AgentNotFound)));
        assert!(d.path().join("timeline.db").exists());
    }

    #[test]
    fn delete_can_keep_the_default_knowledge() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let a = ws.create("A/B: x", PALETTE[0]).unwrap();
        let note = d.path().join(DIR_NAME).join(&a.id).join("knowledge").join("note.md");
        std::fs::create_dir_all(note.parent().unwrap()).unwrap();
        std::fs::write(&note, "keep me").unwrap();
        let r = ws.delete(&a.id, false).unwrap();
        // エージェントは消え、ナレッジはアプリデータ直下の別の場所に残る(名称の区切り文字は無害化される)
        assert!(!d.path().join(DIR_NAME).join(&a.id).exists());
        let kept = std::path::PathBuf::from(r.kept_knowledge.unwrap());
        assert_eq!(kept.parent().unwrap(), d.path().join(KEPT_DIR_NAME));
        assert_eq!(std::fs::read_to_string(kept.join("note.md")).unwrap(), "keep me");
        assert!(ws.list().is_empty());
    }

    #[test]
    fn opening_an_agent_creates_data_and_lowercases_the_knowledge_folders() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let a = ws.create("A", PALETTE[0]).unwrap();
        let knowledge = d.path().join(DIR_NAME).join(&a.id).join("knowledge");
        assert!(knowledge.join("data").is_dir(), "作成時に data/ を作る");
        // 大文字のフォルダは、開き直したときに小文字へ改名する。パーソナルには data/ を作らない
        std::fs::create_dir_all(knowledge.join("Dev/Rust")).unwrap();
        std::fs::remove_dir(knowledge.join("data")).unwrap();
        // 起動時(`Workspaces::new`)に、切り替えていないエージェントも整える
        drop(ws);
        let ws2 = workspaces(d.path());
        assert!(knowledge.join("data").is_dir() && knowledge.join("dev/rust").is_dir());
        for name in ["inbox", "memory", "skills", "captures", "preferences"] {
            assert!(knowledge.join(name).is_dir(), "{name}");
        }
        // パーソナルにも作る。リーダーには記憶を作らない
        assert!(d.path().join("knowledge/data").is_dir() && d.path().join("knowledge/inbox").is_dir());
        let leader = ws2.create_leader().unwrap();
        let lk = d.path().join(DIR_NAME).join(LEADER_ID).join("knowledge");
        assert!(lk.join("inbox").is_dir() && lk.join("data").is_dir() && !lk.join("memory").exists());
        drop(leader);
    }

    #[test]
    fn delete_removes_the_knowledge_when_asked_and_keeps_nothing() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let a = ws.create("A", PALETTE[0]).unwrap();
        let knowledge = d.path().join(DIR_NAME).join(&a.id).join("knowledge");
        std::fs::create_dir_all(&knowledge).unwrap();
        std::fs::write(knowledge.join("note.md"), "x").unwrap();
        let r = ws.delete(&a.id, true).unwrap();
        assert!(r.kept_knowledge.is_none());
        assert!(!d.path().join(KEPT_DIR_NAME).exists());
        // ナレッジが無ければ、残すを選んでも何も作らない
        let b = ws.create("B", PALETTE[1]).unwrap();
        assert!(ws.delete(&b.id, false).unwrap().kept_knowledge.is_none());
    }

    #[test]
    fn delete_fails_without_losing_data_if_the_kept_place_is_taken() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let a = ws.create("A", PALETTE[0]).unwrap();
        let knowledge = d.path().join(DIR_NAME).join(&a.id).join("knowledge");
        std::fs::create_dir_all(knowledge.join("notes")).unwrap();
        std::fs::write(knowledge.join("notes/a.md"), "x").unwrap();
        let id8: String = a.id.chars().take(8).collect();
        std::fs::create_dir_all(d.path().join(KEPT_DIR_NAME).join(format!("A-{id8}"))).unwrap();
        assert!(matches!(ws.delete(&a.id, false), Err(AppError::PathInUse)));
        assert!(knowledge.exists());
        assert_eq!(ws.list().len(), 1);
    }

    #[test]
    fn corrupt_registry_is_moved_aside() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(DIR_NAME)).unwrap();
        std::fs::write(registry_path(d.path()), "not json").unwrap();
        let ws = workspaces(d.path());
        assert!(ws.list().is_empty());
        assert!(registry_path(d.path()).with_extension("json.corrupt").exists());
    }

    #[test]
    fn palette_has_30_distinct_valid_colors() {
        let set: std::collections::HashSet<_> = PALETTE.iter().collect();
        assert_eq!((PALETTE.len(), set.len()), (MAX_AGENTS, MAX_AGENTS));
        for c in PALETTE {
            assert!(c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{c}");
        }
    }

    #[test]
    fn colors_cannot_be_shared_between_agents() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let a = ws.create("A", PALETTE[0]).unwrap();
        let b = ws.create("B", PALETTE[1]).unwrap();
        // 作成・更新とも、他のエージェントの色は選べない(大文字でも同じ色として扱う)
        assert!(matches!(ws.create("C", PALETTE[0]), Err(AppError::ColorInUse)));
        assert!(matches!(ws.create("C", &PALETTE[0].to_uppercase()), Err(AppError::ColorInUse)));
        assert!(matches!(ws.update(&b.id, "B", PALETTE[0]), Err(AppError::ColorInUse)));
        // 自分の色のまま名称だけ変えるのは許可される
        assert!(ws.update(&a.id, "A2", PALETTE[0]).is_ok());
        // 削除すると、その色は空く
        ws.delete(&a.id, true).unwrap();
        assert!(ws.create("C", PALETTE[0]).is_ok());
        assert_eq!(ws.list().len(), 2);
    }

    #[test]
    fn names_cannot_be_shared_between_agents() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let a = ws.create("Sales", PALETTE[0]).unwrap();
        let b = ws.create("Dev", PALETTE[1]).unwrap();
        // 前後の空白・大文字小文字の違いは同じ名称として扱う
        for name in ["Sales", "  Sales ", "sales", "SALES"] {
            assert!(matches!(ws.create(name, PALETTE[2]), Err(AppError::NameInUse)), "{name:?}");
        }
        assert!(matches!(ws.update(&b.id, "sales", PALETTE[1]), Err(AppError::NameInUse)));
        // 自分の名称のまま色だけ変える、大文字小文字だけ変えるのは許可される
        assert!(ws.update(&a.id, "Sales", PALETTE[3]).is_ok());
        assert!(ws.update(&a.id, "SALES", PALETTE[3]).is_ok());
        // 削除すると、その名称は空く
        ws.delete(&a.id, true).unwrap();
        assert!(ws.create("Sales", PALETTE[0]).is_ok());
        // 拒否された操作では、一覧は変わらない
        assert_eq!(ws.list().len(), 2);
    }

    #[test]
    fn storage_locations_cannot_overlap_between_workspaces() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let a = ws.create("A", PALETTE[0]).unwrap();
        let b = ws.create("B", PALETTE[1]).unwrap();
        let a_default = d.path().join(DIR_NAME).join(&a.id).join("knowledge");
        let free = tempfile::tempdir().unwrap();

        // 他のワークスペースの既定の場所・その中・その親・データのフォルダは使えない
        for bad in [
            a_default.clone(),
            a_default.join("sub"),
            a_default.join("debug-logs"),
            d.path().join(DIR_NAME).join(&a.id),
            d.path().join(DIR_NAME),
            d.path().to_owned(), // 全体を含む
            d.path().join("knowledge"),         // パーソナルの既定
            d.path().join("knowledge").join("x"),
        ] {
            assert!(matches!(ws.ensure_paths_free(&b.id, &bad), Err(AppError::PathInUse)), "{bad:?}");
        }
        // まだ存在しない場所でも、重なりは見抜く
        assert!(ws.ensure_paths_free(&b.id, &a_default.join("not/yet/created")).is_err());
        // 関係のない場所と、自分自身の既定の場所は使える
        assert!(ws.ensure_paths_free(&b.id, free.path()).is_ok());
        assert!(ws.ensure_paths_free(&b.id, &d.path().join(DIR_NAME).join(&b.id).join("knowledge")).is_ok());
        // パーソナルから見ても、エージェントの場所は使えない
        assert!(ws.ensure_paths_free(PERSONAL_ID, &a_default).is_err());
        assert!(ws.ensure_paths_free(PERSONAL_ID, free.path()).is_ok());
    }

    #[test]
    fn leader_storage_is_protected_and_can_use_its_own_default() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        ws.create_leader().unwrap();
        let a = ws.create("A", PALETTE[0]).unwrap();
        let leader_default = d.path().join(DIR_NAME).join(LEADER_ID).join("knowledge");
        let free = tempfile::tempdir().unwrap();
        // エージェントやパーソナルは、リーダーのナレッジの場所(その中・その親)を使えない
        for bad in [leader_default.clone(), leader_default.join("sub"), d.path().join(DIR_NAME).join(LEADER_ID)] {
            assert!(matches!(ws.ensure_paths_free(&a.id, &bad), Err(AppError::PathInUse)), "{bad:?}");
            assert!(matches!(ws.ensure_paths_free(PERSONAL_ID, &bad), Err(AppError::PathInUse)), "{bad:?}");
        }
        // リーダー自身は、自分の既定の場所へ戻せる。別の空いた場所も使える。エージェントの場所は使えない
        assert!(ws.ensure_paths_free(LEADER_ID, &leader_default).is_ok());
        assert!(ws.ensure_paths_free(LEADER_ID, free.path()).is_ok());
        assert!(ws.ensure_paths_free(LEADER_ID, &d.path().join(DIR_NAME).join(&a.id).join("knowledge")).is_err());
        assert!(ws.ensure_paths_free(LEADER_ID, d.path()).is_err(), "アプリのデータ全体は保存先にできない");
    }

    #[test]
    fn custom_locations_are_claimed_too() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let a = ws.create("A", PALETTE[0]).unwrap();
        let b = ws.create("B", PALETTE[1]).unwrap();
        let (kx, dx) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        // Aがナレッジとデバッグログの保存先を変えている
        let ctx_a = ws.get(&a.id).unwrap();
        ctx_a.db.set_setting(batch::KNOWLEDGE_DIR_KEY, &kx.path().to_string_lossy()).unwrap();
        ctx_a.db.set_setting(debug_log::DIR_KEY, &dx.path().to_string_lossy()).unwrap();
        for bad in [kx.path().to_owned(), kx.path().join("sub"), dx.path().to_owned(), kx.path().parent().unwrap().to_owned()] {
            assert!(ws.ensure_paths_free(&b.id, &bad).is_err(), "{bad:?}");
        }
        // Aが自分の既定の場所へ戻しても、Bの場所とは重ならない(Bが先に使っていれば拒否される)
        let a_default = d.path().join(DIR_NAME).join(&a.id).join("knowledge");
        assert!(ws.ensure_paths_free(&a.id, &a_default).is_ok());
        // Bが、Aの「既定に戻す」場所を先に取ることはできない
        assert!(ws.ensure_paths_free(&b.id, &a_default).is_err());
        // 自分の保存先は、自分からは使える
        assert!(ws.ensure_paths_free(&a.id, kx.path()).is_ok());
    }

    #[test]
    fn debug_logs_stay_in_their_own_workspace() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let p = ws.create("A", PALETTE[0]).unwrap();
        let personal = ws.get(PERSONAL_ID).unwrap();
        let agent = ws.get(&p.id).unwrap();
        // パーソナルだけデバッグモードを有効にする
        personal.db.set_setting(debug_log::ENABLED_KEY, "1").unwrap();
        debug_log::refresh(&personal).unwrap();
        debug_log::refresh(&agent).unwrap();
        // エージェントを選択している間も、パーソナルの裏の処理はパーソナルのログへ書く
        ws.switch(&p.id).unwrap();
        debug_log::record(&personal, "api", serde_json::json!({ "from": "personal" }));
        debug_log::record(&agent, "api", serde_json::json!({ "from": "agent" }));
        let files = |c: &WorkspaceCtx| std::fs::read_dir(debug_log::log_dir(c).unwrap()).map(|r| r.count()).unwrap_or(0);
        assert_eq!(files(&personal), 1);
        assert_eq!(files(&agent), 0, "無効なワークスペースには書かない");
        assert!(debug_log::enabled(&personal) && !debug_log::enabled(&agent));
    }

    #[test]
    fn app_data_is_never_a_storage_location() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path()); // エージェントが1件も無くても拒否する
        for bad in [d.path().to_owned(), d.path().join(DIR_NAME), d.path().join(DIR_NAME).join("anything"), d.path().join("models")] {
            assert!(matches!(ws.ensure_paths_free(PERSONAL_ID, &bad), Err(AppError::PathInUse)), "{bad:?}");
        }
        assert!(ws.ensure_paths_free(PERSONAL_ID, d.path().parent().unwrap()).is_err(), "アプリデータを含む親も拒否");
        // 自分の既定のナレッジ(とその中)は使える
        assert!(ws.ensure_paths_free(PERSONAL_ID, &d.path().join("knowledge")).is_ok());
        assert!(ws.ensure_paths_free(PERSONAL_ID, &d.path().join("knowledge").join("sub")).is_ok());
        let p = ws.create("A", PALETTE[0]).unwrap();
        let own = d.path().join(DIR_NAME).join(&p.id).join("knowledge");
        assert!(ws.ensure_paths_free(&p.id, &own).is_ok());
        assert!(ws.ensure_paths_free(&p.id, &d.path().join(DIR_NAME).join(&p.id)).is_err(), "自分のDB・キーのフォルダは不可");
    }

    #[test]
    fn creating_a_agent_is_refused_when_an_existing_location_would_contain_it() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        // 古い設定で、パーソナルがアプリデータ全体をナレッジの保存先にしている
        ws.get(PERSONAL_ID).unwrap().db.set_setting(batch::KNOWLEDGE_DIR_KEY, &d.path().to_string_lossy()).unwrap();
        assert!(matches!(ws.create("A", PALETTE[0]), Err(AppError::PathInUse)));
        assert!(ws.list().is_empty(), "一覧もフォルダも作らない");
        // 全体設定のDBのほかには、何も作らない
        let names: Vec<_> = std::fs::read_dir(d.path().join(DIR_NAME)).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, [crate::services::global_settings::DB_FILE]);
    }

    #[test]
    fn set_location_saves_only_when_the_check_passes() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let p = ws.create("A", PALETTE[0]).unwrap();
        let other = d.path().join(DIR_NAME).join(&p.id).join("knowledge");
        let mut ran = false;
        let r: Result<(), _> = ws.set_location(PERSONAL_ID, &other, || {
            ran = true;
            Ok(())
        });
        assert!(r.is_err() && !ran, "重なるなら保存の処理を実行しない");
        let free = tempfile::tempdir().unwrap();
        assert_eq!(ws.set_location(PERSONAL_ID, free.path(), || Ok(7)).unwrap(), 7);
    }

    #[test]
    fn deleted_agents_are_marked_and_the_active_one_falls_back() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let a = ws.create("A", PALETTE[0]).unwrap();
        let b = ws.create("B", PALETTE[1]).unwrap();
        let held = ws.get(&a.id).unwrap(); // 裏の処理が持っている
        ws.switch(&a.id).unwrap();
        assert!(held.ensure_alive().is_ok());
        assert!(ws.delete(&a.id, true).unwrap().was_active, "選択中だった");
        assert!(matches!(held.ensure_alive(), Err(AppError::AgentNotFound)), "裏の処理は削除に気づいて止まる");
        assert_eq!(ws.active().id, PERSONAL_ID);
        assert!(!ws.delete(&b.id, true).unwrap().was_active, "選択中ではなかった");
        // 削除済みのIDは、選択も読み込みもできず、一覧から開き直されない
        assert!(matches!(ws.switch(&a.id), Err(AppError::AgentNotFound)));
        assert_eq!(ws.batch_ids(), vec![PERSONAL_ID.to_owned()]);
        assert!(!d.path().join(DIR_NAME).join(&a.id).exists());
    }

    #[test]
    fn active_never_panics_even_if_its_context_is_gone() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let a = ws.create("A", PALETTE[0]).unwrap();
        ws.switch(&a.id).unwrap();
        // 読み込み済みの一覧から消えた状態(削除と交差した場合)でも、パーソナルを返す
        ws.loaded.write().unwrap().remove(&a.id);
        assert_eq!(ws.active().id, PERSONAL_ID);
        assert_eq!(ws.active_info().id, PERSONAL_ID);
    }

    #[test]
    fn concurrent_switch_and_delete_never_leave_a_ghost() {
        let d = tempfile::tempdir().unwrap();
        let ws = Arc::new(workspaces(d.path()));
        for round in 0..20 {
            let p = ws.create(&format!("p{round}"), PALETTE[round % 2]).unwrap();
            let (w1, w2, id1, id2) = (ws.clone(), ws.clone(), p.id.clone(), p.id.clone());
            let t1 = std::thread::spawn(move || {
                let _ = w1.switch(&id1);
                let _ = w1.active();
            });
            let t2 = std::thread::spawn(move || {
                let _ = w2.delete(&id2, true);
            });
            t1.join().unwrap();
            t2.join().unwrap();
            // どの順序でも、削除済みのエージェントが選択・読み込み・フォルダとして残らない
            assert!(ws.list().is_empty());
            assert_eq!(ws.active().id, PERSONAL_ID);
            assert!(ws.get(&p.id).is_none());
            assert!(!d.path().join(DIR_NAME).join(&p.id).exists());
        }
    }

    #[test]
    fn background_work_lists_agents_without_opening_them() {
        let d = tempfile::tempdir().unwrap();
        let id = workspaces(d.path()).create("A", PALETTE[0]).unwrap().id;
        let ws = workspaces(d.path()); // 再起動後: エージェントは未読み込み
        assert_eq!(ws.batch_ids(), vec![PERSONAL_ID.to_owned(), id.clone()], "開かずに一覧を返す");
        assert!(ws.get(&id).is_none());

        // 保存先の確認も、他のエージェントを読み込んだままにしない
        let free = tempfile::tempdir().unwrap();
        assert!(ws.ensure_paths_free(PERSONAL_ID, free.path()).is_ok());
        assert!(ws.get(&id).is_none());

        // 削除は、読み込み済みのものにも伝わる
        ws.switch(&id).unwrap();
        let loaded = ws.get(&id).unwrap();
        ws.delete(&id, true).unwrap();
        assert!(matches!(loaded.ensure_alive(), Err(AppError::AgentNotFound)));
    }

    #[test]
    fn reserved_names_are_refused() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        for name in ["パーソナル", "ワーク", "Personal", "personal", "  WORK  ", "wOrK"] {
            assert!(matches!(ws.create(name, PALETTE[0]), Err(AppError::ReservedName)), "{name:?}");
        }
        // 予約名を含むだけの名称は使える。更新で予約名へ変えることもできない
        assert!(ws.create("Work Tools", PALETTE[0]).is_ok());
        let p = ws.create("A", PALETTE[1]).unwrap();
        assert!(matches!(ws.update(&p.id, "Personal", PALETTE[1]), Err(AppError::ReservedName)));
    }

    #[tokio::test]
    async fn deleting_a_agent_aborts_its_running_request() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let p = ws.create("A", PALETTE[0]).unwrap();
        let shared = ws.get(&p.id).unwrap().shared.clone();
        let token = crate::services::cancel::CancelToken::default();
        shared.task.set_cancel(token.clone());
        let rx = shared.task.register_approval("t1");
        ws.delete(&p.id, true).unwrap();
        token.cancelled().await; // 中断されている
        assert!(rx.await.is_err(), "承認待ちは拒否として終わる");
        // 別のエージェントの依頼には影響しない
        let q = ws.create("B", PALETTE[1]).unwrap();
        let other = ws.get(&q.id).unwrap().shared.clone();
        let rx2 = other.task.register_approval("t2");
        ws.delete(&p.id, true).unwrap_err();
        other.task.answer("t2", true);
        assert!(rx2.await.unwrap());
    }
}
