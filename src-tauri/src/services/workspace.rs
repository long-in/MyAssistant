//! ワークスペース(パーソナル / ワークのプロジェクト)ごとの実行コンテキストと、プロジェクトの管理。
//!
//! DB・APIキー・AWS の一時状態をワークスペースごとに分け、混線させない。
//! パーソナルは従来どおりアプリデータ直下、プロジェクトは`<アプリデータ>/workspaces/<id>/`に置く。
//! プロジェクト一覧(名称・イメージカラー)は`workspaces/registry.json`に保持する。
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

use super::{aws::AwsState, batch, batch::BatchGuard, debug_log, debug_log::DebugLog, secrets::Secrets};
use crate::{db::Db, utils::error::AppError};

/// パーソナルワークスペースのID。保存先は従来どおりアプリデータ直下。
pub const PERSONAL_ID: &str = "personal";
/// 作れるプロジェクトの上限。
pub const MAX_PROJECTS: usize = 30;
/// プロジェクト名の最大文字数。
pub const NAME_MAX_CHARS: usize = 40;
/// プロジェクトに選べるイメージカラー(`#rrggbb`)。上限(`MAX_PROJECTS`)と同数で、プロジェクト同士で重複させない。
/// 色相をずらした明るい15色と暗い15色(明るさの違いと色相差で見分けやすくする)。
pub const PALETTE: [&str; MAX_PROJECTS] = [
    "#e61919", "#e66b19", "#e6bd19", "#bde619", "#6be619", "#19e619", "#19e66b", "#19e6bd", "#19bde6", "#196be6",
    "#1919e6", "#6b19e6", "#bd19e6", "#e619bd", "#e6196b", "#a5381d", "#a56e1d", "#a5a51d", "#6ea51d", "#38a51d",
    "#1da538", "#1da56e", "#1da5a5", "#1d6ea5", "#1d38a5", "#381da5", "#6e1da5", "#a51da5", "#a51d6e", "#a51d38",
];
/// ワークスペースが切り替わったことを画面へ知らせるイベント名。
pub const WORKSPACE_CHANGED_EVENT: &str = "workspace-changed";

const DIR_NAME: &str = "workspaces";
const REGISTRY_FILE: &str = "registry.json";

/// 1ワークスペース分の状態。`dir`配下に`timeline.db`と`secrets.json`を置く。
pub struct WorkspaceCtx {
    pub id: String,
    pub dir: PathBuf,
    pub db: Db,
    pub secrets: Secrets,
    pub aws: AwsState,
    pub batch: BatchGuard,
    pub debug: DebugLog,
    /// 削除された(または削除中の)プロジェクトの印。裏で動く処理は、書き込む前にこれを確かめて止まる。
    deleted: AtomicBool,
}

impl WorkspaceCtx {
    /// `dir`を開く(無ければ作る)。
    pub fn open(id: &str, dir: &Path) -> Result<Self, AppError> {
        std::fs::create_dir_all(dir).map_err(|e| AppError::Io(e.to_string()))?;
        Ok(Self {
            id: id.to_owned(),
            dir: dir.to_owned(),
            db: Db::open(&dir.join("timeline.db"))?,
            secrets: Secrets::open(dir)?,
            aws: AwsState::default(),
            batch: BatchGuard::default(),
            debug: DebugLog::default(),
            deleted: AtomicBool::new(false),
        })
    }

    /// 削除されていないことを確かめる。バッチや会話が、削除済みのプロジェクトへ書き込まないようにする。
    pub fn ensure_alive(&self) -> Result<(), AppError> {
        if self.deleted.load(Ordering::SeqCst) { Err(AppError::ProjectNotFound) } else { Ok(()) }
    }
}

/// ワークスペース内の1プロジェクト(独立したエージェント)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Project {
    pub id: String,
    pub name: String,
    /// イメージカラー(`#rrggbb`)。
    pub color: String,
    pub created_at: String,
}

/// 現在選択中のワークスペース。`project`は個人のとき`None`。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ActiveWorkspace {
    pub id: String,
    pub project: Option<Project>,
}

/// 名称を整える。前後の空白を除き、1〜`NAME_MAX_CHARS`文字で、制御文字を含まないこと。
fn clean_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    let chars = name.chars().count();
    if chars == 0 || chars > NAME_MAX_CHARS || name.chars().any(char::is_control) {
        return Err(AppError::InvalidProject("name".into()));
    }
    Ok(name.to_owned())
}

/// `PALETTE`の色だけを受け付け、小文字へそろえる。
fn clean_color(color: &str) -> Result<String, AppError> {
    let c = color.trim().to_ascii_lowercase();
    if !PALETTE.contains(&c.as_str()) {
        return Err(AppError::InvalidProject("color".into()));
    }
    Ok(c)
}

/// `name`と`color`を、`except`以外のプロジェクトがすでに使っていれば拒否する。
/// 名称は大文字小文字を区別せず比較する(`name`は整形済みであること)。
fn ensure_unique(projects: &[Project], name: &str, color: &str, except: Option<&str>) -> Result<(), AppError> {
    let others = || projects.iter().filter(|p| Some(p.id.as_str()) != except);
    if others().any(|p| p.name.to_lowercase() == name.to_lowercase()) {
        return Err(AppError::NameInUse);
    }
    if others().any(|p| p.color == color) {
        return Err(AppError::ColorInUse);
    }
    Ok(())
}

/// 読み込み済みのワークスペースと、現在選択中のID、プロジェクト一覧。
pub struct Workspaces {
    /// アプリデータディレクトリ(パーソナルの保存先でもある)。
    root: PathBuf,
    /// 一覧・読み込み・選択・保存先の変更を1つずつ行うための外側のロック。
    /// 削除と切り替え・読み込みが交差して、存在しないワークスペースを選んだり、削除済みを開き直したりしないようにする。
    op: Mutex<()>,
    loaded: RwLock<HashMap<String, Arc<WorkspaceCtx>>>,
    active: RwLock<String>,
    projects: Mutex<Vec<Project>>,
}

impl Workspaces {
    /// パーソナルを選択した状態で始め、保存済みのプロジェクト一覧を読む。
    /// 一覧が壊れていたら`.corrupt`へ退避して空から始める(プロジェクトのフォルダは残す)。
    pub fn new(root: &Path, personal: WorkspaceCtx) -> Self {
        let projects = load_registry(root);
        let id = personal.id.clone();
        Self {
            root: root.to_owned(),
            op: Mutex::new(()),
            loaded: RwLock::new(HashMap::from([(id.clone(), Arc::new(personal))])),
            active: RwLock::new(id),
            projects: Mutex::new(projects),
        }
    }

    fn op(&self) -> std::sync::MutexGuard<'_, ()> {
        self.op.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn projects(&self) -> std::sync::MutexGuard<'_, Vec<Project>> {
        self.projects.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn project_dir(&self, id: &str) -> PathBuf {
        self.root.join(DIR_NAME).join(id)
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
        let project = self.projects().iter().find(|p| p.id == id).cloned();
        ActiveWorkspace { id, project }
    }

    pub fn get(&self, id: &str) -> Option<Arc<WorkspaceCtx>> {
        self.loaded.read().unwrap_or_else(|p| p.into_inner()).get(id).cloned()
    }

    /// 読み込み済みのすべて。
    pub fn all(&self) -> Vec<Arc<WorkspaceCtx>> {
        self.loaded.read().unwrap_or_else(|p| p.into_inner()).values().cloned().collect()
    }

    /// すべてのプロジェクトを読み込んで返す(バックグラウンド処理の巡回用)。開けないものは警告して飛ばす。
    pub fn load_all(&self) -> Vec<Arc<WorkspaceCtx>> {
        let _op = self.op();
        self.load_all_locked()
    }

    fn load_all_locked(&self) -> Vec<Arc<WorkspaceCtx>> {
        let ids: Vec<String> = self.projects().iter().map(|p| p.id.clone()).collect();
        for id in ids {
            if let Err(e) = self.load_locked(&id) {
                log::warn!("project {id} not loaded: {e}");
            }
        }
        self.all()
    }

    /// プロジェクトを(未読み込みなら)開く。一覧に無いIDは開かない。`op`を持った状態で呼ぶ。
    fn load_locked(&self, id: &str) -> Result<Arc<WorkspaceCtx>, AppError> {
        if let Some(ctx) = self.get(id) {
            return Ok(ctx);
        }
        if !self.projects().iter().any(|p| p.id == id) {
            return Err(AppError::ProjectNotFound);
        }
        let ctx = Arc::new(WorkspaceCtx::open(id, &self.project_dir(id))?);
        // デバッグログの出力先は、開いたときに決める(裏で動く処理も自分のワークスペースのログへ書く)
        if let Err(e) = debug_log::refresh(&ctx) {
            log::warn!("debug log init failed for project {id}: {e}");
        }
        self.loaded.write().unwrap_or_else(|p| p.into_inner()).insert(id.to_owned(), ctx.clone());
        Ok(ctx)
    }

    /// IDからワークスペースを得る(プロジェクトは未読み込みなら開く)。一覧に無いIDは`ProjectNotFound`。
    pub fn resolve(&self, id: &str) -> Result<Arc<WorkspaceCtx>, AppError> {
        let _op = self.op();
        if id == PERSONAL_ID { Ok(self.personal()) } else { self.load_locked(id) }
    }

    /// `candidate`(ナレッジやデバッグログの保存先にしたい場所)が使えることを確かめる。`op`を持った状態で呼ぶ。
    /// - アプリのデータ(アプリデータ全体・`workspaces/`・ほかのワークスペースのDBやキー)に触れる場所は、自分の既定のナレッジの中を除いて拒否する。
    /// - `id`以外のワークスペースの保存先(現在の場所と既定の場所の両方で、ナレッジとデバッグログ)と、同じ場所・親子関係で重なる場所は拒否する。
    fn check_paths_free(&self, id: &str, candidate: &Path) -> Result<(), AppError> {
        let candidate = canonical_lenient(candidate);
        let own = if id == PERSONAL_ID { Some(self.personal()) } else { self.get(id) };
        let own_default = own.map(|c| canonical_lenient(&batch::default_knowledge_root(&c)));
        if overlaps(&candidate, &canonical_lenient(&self.root)) && !own_default.is_some_and(|d| candidate.starts_with(d)) {
            return Err(AppError::PathInUse);
        }
        for other in self.load_all_locked().into_iter().filter(|c| c.id != id) {
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

    pub fn list(&self) -> Vec<Project> {
        self.projects().clone()
    }

    /// プロジェクトを作る。名称・色を検証し、上限(`MAX_PROJECTS`)を超える場合は拒否する。
    pub fn create(&self, name: &str, color: &str) -> Result<Project, AppError> {
        let (name, color) = (clean_name(name)?, clean_color(color)?);
        let _op = self.op();
        let projects = self.projects();
        if projects.len() >= MAX_PROJECTS {
            return Err(AppError::ProjectLimit);
        }
        ensure_unique(&projects, &name, &color, None)?;
        let project = Project {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            color,
            created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        };
        // 既存のワークスペースが、新しいプロジェクトのフォルダを含む場所を保存先にしていたら作らない
        // (古い設定でアプリデータ全体を保存先にしている場合など。そのままではキーやDBがナレッジから見えてしまう)
        let dir = canonical_lenient(&self.project_dir(&project.id));
        drop(projects);
        for other in self.load_all_locked() {
            if claimed_paths(&other).iter().any(|c| overlaps(&dir, &canonical_lenient(c))) {
                return Err(AppError::PathInUse);
            }
        }
        let mut projects = self.projects();
        // 先にフォルダとDBを作れることを確かめてから一覧へ載せる
        let ctx = Arc::new(WorkspaceCtx::open(&project.id, &self.project_dir(&project.id))?);
        let mut next = projects.clone();
        next.push(project.clone());
        save_registry(&self.root, &next)?;
        *projects = next;
        if let Err(e) = debug_log::refresh(&ctx) {
            log::warn!("debug log init failed for project {}: {e}", project.id);
        }
        self.loaded.write().unwrap_or_else(|p| p.into_inner()).insert(project.id.clone(), ctx);
        Ok(project)
    }

    /// 名称とイメージカラーを変える。
    pub fn update(&self, id: &str, name: &str, color: &str) -> Result<Project, AppError> {
        let (name, color) = (clean_name(name)?, clean_color(color)?);
        let _op = self.op();
        let mut projects = self.projects();
        ensure_unique(&projects, &name, &color, Some(id))?;
        let mut next = projects.clone();
        let target = next.iter_mut().find(|p| p.id == id).ok_or(AppError::ProjectNotFound)?;
        target.name = name;
        target.color = color;
        let updated = target.clone();
        save_registry(&self.root, &next)?;
        *projects = next;
        Ok(updated)
    }

    /// プロジェクトを選択する。`PERSONAL_ID`でパーソナルへ戻る。
    pub fn switch(&self, id: &str) -> Result<ActiveWorkspace, AppError> {
        let _op = self.op();
        if id != PERSONAL_ID {
            self.load_locked(id)?;
        }
        *self.active.write().unwrap_or_else(|p| p.into_inner()) = id.to_owned();
        drop(_op);
        Ok(self.active_info())
    }

    /// プロジェクトを一覧とディスクから削除する。選択中ならパーソナルへ戻す。元に戻せない。
    /// 選択中を切り戻したかどうかを返す。裏で動いている処理には削除の印(`ensure_alive`)が付き、書き込む前に止まる。
    /// アプリが管理するフォルダ(`workspaces/<id>/`)だけを消す。利用者が指定したナレッジの保存先は消さない。
    pub fn delete(&self, id: &str) -> Result<bool, AppError> {
        let _op = self.op();
        let mut projects = self.projects();
        if !projects.iter().any(|p| p.id == id) {
            return Err(AppError::ProjectNotFound);
        }
        let next: Vec<Project> = projects.iter().filter(|p| p.id != id).cloned().collect();
        save_registry(&self.root, &next)?;
        *projects = next;
        drop(projects);
        let was_active = {
            let mut active = self.active.write().unwrap_or_else(|p| p.into_inner());
            let was = *active == id;
            if was {
                *active = PERSONAL_ID.to_owned();
            }
            was
        };
        if let Some(ctx) = self.loaded.write().unwrap_or_else(|p| p.into_inner()).remove(id) {
            ctx.deleted.store(true, Ordering::SeqCst);
        }
        // `id`は一覧にあったもの(生成したUUID)だけなので、`workspaces/`配下の1フォルダに限られる
        if let Err(e) = std::fs::remove_dir_all(self.project_dir(id))
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("project dir not removed: {e}");
        }
        Ok(was_active)
    }
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

fn load_registry(root: &Path) -> Vec<Project> {
    let path = registry_path(root);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            log::warn!("registry unreadable: {e}");
            return Vec::new();
        }
    };
    match serde_json::from_str::<Vec<Project>>(&text) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("registry corrupt, moved aside: {e}");
            let _ = std::fs::rename(&path, path.with_extension("json.corrupt"));
            Vec::new()
        }
    }
}

/// 一時ファイルへ書いてからリネームし、書き込み途中の破損を避ける。
fn save_registry(root: &Path, projects: &[Project]) -> Result<(), AppError> {
    let io = |e: std::io::Error| AppError::Io(e.to_string());
    let path = registry_path(root);
    std::fs::create_dir_all(root.join(DIR_NAME)).map_err(io)?;
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(projects).map_err(|e| AppError::Io(e.to_string()))?;
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
            assert!(matches!(ws.create(name, PALETTE[0]), Err(AppError::InvalidProject(_))), "{name:?}");
        }
        for color in ["", "red", "#12345", "#1234567", "#12345g", "112233", "#000000", "#ffffff"] {
            assert!(matches!(ws.create("x", color), Err(AppError::InvalidProject(_))), "{color:?}");
        }
        let p = ws.create("  営業  ", &PALETTE[0].to_uppercase()).unwrap();
        assert_eq!((p.name.as_str(), p.color.as_str()), ("営業", PALETTE[0]));
        assert!(ws.create(&"あ".repeat(NAME_MAX_CHARS), PALETTE[1]).is_ok());
    }

    #[test]
    fn enforces_project_limit() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        for i in 0..MAX_PROJECTS {
            ws.create(&format!("p{i}"), PALETTE[i]).unwrap();
        }
        assert!(matches!(ws.create("over", PALETTE[0]), Err(AppError::ProjectLimit)));
        let (first, first_color) = (ws.list()[0].id.clone(), ws.list()[0].color.clone());
        ws.delete(&first).unwrap();
        assert!(ws.create("again", &first_color).is_ok());
    }

    #[test]
    fn registry_persists_and_projects_reload_lazily() {
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
        assert_eq!(info.project.unwrap().color, PALETTE[2]);
        assert_eq!(ws.active().db.get_setting("k").unwrap().as_deref(), Some("v"));
    }

    #[test]
    fn switch_to_unknown_project_fails_and_keeps_active() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        assert!(matches!(ws.switch("../etc"), Err(AppError::ProjectNotFound)));
        assert_eq!(ws.active_info().id, PERSONAL_ID);
    }

    #[test]
    fn delete_removes_data_and_falls_back_to_personal() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let p = ws.create("A", PALETTE[0]).unwrap();
        ws.switch(&p.id).unwrap();
        assert!(d.path().join(DIR_NAME).join(&p.id).exists());
        ws.delete(&p.id).unwrap();
        assert_eq!(ws.active_info().id, PERSONAL_ID);
        assert!(!d.path().join(DIR_NAME).join(&p.id).exists());
        assert!(ws.list().is_empty());
        assert!(matches!(ws.delete(&p.id), Err(AppError::ProjectNotFound)));
        // 一覧に無いIDでは何も消えない(パスの注入対策)
        assert!(matches!(ws.delete(".."), Err(AppError::ProjectNotFound)));
        assert!(d.path().join("timeline.db").exists());
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
        assert_eq!((PALETTE.len(), set.len()), (MAX_PROJECTS, MAX_PROJECTS));
        for c in PALETTE {
            assert!(c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{c}");
        }
    }

    #[test]
    fn colors_cannot_be_shared_between_projects() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let a = ws.create("A", PALETTE[0]).unwrap();
        let b = ws.create("B", PALETTE[1]).unwrap();
        // 作成・更新とも、他のプロジェクトの色は選べない(大文字でも同じ色として扱う)
        assert!(matches!(ws.create("C", PALETTE[0]), Err(AppError::ColorInUse)));
        assert!(matches!(ws.create("C", &PALETTE[0].to_uppercase()), Err(AppError::ColorInUse)));
        assert!(matches!(ws.update(&b.id, "B", PALETTE[0]), Err(AppError::ColorInUse)));
        // 自分の色のまま名称だけ変えるのは許可される
        assert!(ws.update(&a.id, "A2", PALETTE[0]).is_ok());
        // 削除すると、その色は空く
        ws.delete(&a.id).unwrap();
        assert!(ws.create("C", PALETTE[0]).is_ok());
        assert_eq!(ws.list().len(), 2);
    }

    #[test]
    fn names_cannot_be_shared_between_projects() {
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
        ws.delete(&a.id).unwrap();
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
        // パーソナルから見ても、プロジェクトの場所は使えない
        assert!(ws.ensure_paths_free(PERSONAL_ID, &a_default).is_err());
        assert!(ws.ensure_paths_free(PERSONAL_ID, free.path()).is_ok());
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
        let project = ws.get(&p.id).unwrap();
        // パーソナルだけデバッグモードを有効にする
        personal.db.set_setting(debug_log::ENABLED_KEY, "1").unwrap();
        debug_log::refresh(&personal).unwrap();
        debug_log::refresh(&project).unwrap();
        // プロジェクトを選択している間も、パーソナルの裏の処理はパーソナルのログへ書く
        ws.switch(&p.id).unwrap();
        debug_log::record(&personal, "api", serde_json::json!({ "from": "personal" }));
        debug_log::record(&project, "api", serde_json::json!({ "from": "project" }));
        let files = |c: &WorkspaceCtx| std::fs::read_dir(debug_log::log_dir(c).unwrap()).map(|r| r.count()).unwrap_or(0);
        assert_eq!(files(&personal), 1);
        assert_eq!(files(&project), 0, "無効なワークスペースには書かない");
        assert!(debug_log::enabled(&personal) && !debug_log::enabled(&project));
    }

    #[test]
    fn app_data_is_never_a_storage_location() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path()); // プロジェクトが1件も無くても拒否する
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
    fn creating_a_project_is_refused_when_an_existing_location_would_contain_it() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        // 古い設定で、パーソナルがアプリデータ全体をナレッジの保存先にしている
        ws.get(PERSONAL_ID).unwrap().db.set_setting(batch::KNOWLEDGE_DIR_KEY, &d.path().to_string_lossy()).unwrap();
        assert!(matches!(ws.create("A", PALETTE[0]), Err(AppError::PathInUse)));
        assert!(ws.list().is_empty(), "一覧もフォルダも作らない");
        assert!(!d.path().join(DIR_NAME).exists());
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
    fn deleted_projects_are_marked_and_the_active_one_falls_back() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        let a = ws.create("A", PALETTE[0]).unwrap();
        let b = ws.create("B", PALETTE[1]).unwrap();
        let held = ws.get(&a.id).unwrap(); // 裏の処理が持っている
        ws.switch(&a.id).unwrap();
        assert!(held.ensure_alive().is_ok());
        assert!(ws.delete(&a.id).unwrap(), "選択中だった");
        assert!(matches!(held.ensure_alive(), Err(AppError::ProjectNotFound)), "裏の処理は削除に気づいて止まる");
        assert_eq!(ws.active().id, PERSONAL_ID);
        assert!(!ws.delete(&b.id).unwrap(), "選択中ではなかった");
        // 削除済みのIDは、選択も読み込みもできず、一覧から開き直されない
        assert!(matches!(ws.switch(&a.id), Err(AppError::ProjectNotFound)));
        assert!(ws.load_all().iter().all(|c| c.id == PERSONAL_ID));
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
                let _ = w2.delete(&id2);
            });
            t1.join().unwrap();
            t2.join().unwrap();
            // どの順序でも、削除済みのプロジェクトが選択・読み込み・フォルダとして残らない
            assert!(ws.list().is_empty());
            assert_eq!(ws.active().id, PERSONAL_ID);
            assert!(ws.get(&p.id).is_none());
            assert!(!d.path().join(DIR_NAME).join(&p.id).exists());
        }
    }
}
