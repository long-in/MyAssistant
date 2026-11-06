//! ワークスペース(パーソナル / ワークのプロジェクト)ごとの実行コンテキストと、プロジェクトの管理。
//!
//! DB・APIキー・AWS の一時状態をワークスペースごとに分け、混線させない。
//! パーソナルは従来どおりアプリデータ直下、プロジェクトは`<アプリデータ>/workspaces/<id>/`に置く。
//! プロジェクト一覧(名称・イメージカラー)は`workspaces/registry.json`に保持する。
use std::{
    collections::HashMap,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

use serde::{Deserialize, Serialize};
use tauri::{
    Manager, Runtime,
    ipc::{CommandArg, CommandItem, InvokeError},
};

use super::{aws::AwsState, batch, batch::BatchGuard, debug_log, secrets::Secrets};
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
        })
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
            loaded: RwLock::new(HashMap::from([(id.clone(), Arc::new(personal))])),
            active: RwLock::new(id),
            projects: Mutex::new(projects),
        }
    }

    fn projects(&self) -> std::sync::MutexGuard<'_, Vec<Project>> {
        self.projects.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn project_dir(&self, id: &str) -> PathBuf {
        self.root.join(DIR_NAME).join(id)
    }

    /// 現在選択中のワークスペース。
    pub fn active(&self) -> Arc<WorkspaceCtx> {
        let id = self.active.read().unwrap_or_else(|p| p.into_inner()).clone();
        self.get(&id).expect("the active workspace is always loaded")
    }

    pub fn active_info(&self) -> ActiveWorkspace {
        let id = self.active.read().unwrap_or_else(|p| p.into_inner()).clone();
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
        let ids: Vec<String> = self.projects().iter().map(|p| p.id.clone()).collect();
        for id in ids {
            if let Err(e) = self.load(&id) {
                log::warn!("project {id} not loaded: {e}");
            }
        }
        self.all()
    }

    /// プロジェクトを(未読み込みなら)開く。一覧に無いIDは開かない。
    fn load(&self, id: &str) -> Result<Arc<WorkspaceCtx>, AppError> {
        if let Some(ctx) = self.get(id) {
            return Ok(ctx);
        }
        if !self.projects().iter().any(|p| p.id == id) {
            return Err(AppError::ProjectNotFound);
        }
        let ctx = Arc::new(WorkspaceCtx::open(id, &self.project_dir(id))?);
        // 別スレッドが先に開いていたらそちらを使う
        let mut loaded = self.loaded.write().unwrap_or_else(|p| p.into_inner());
        Ok(loaded.entry(id.to_owned()).or_insert(ctx).clone())
    }

    /// IDからワークスペースを得る(プロジェクトは未読み込みなら開く)。一覧に無いIDは`ProjectNotFound`。
    pub fn resolve(&self, id: &str) -> Result<Arc<WorkspaceCtx>, AppError> {
        if id == PERSONAL_ID { Ok(self.get(PERSONAL_ID).expect("personal is always loaded")) } else { self.load(id) }
    }

    /// `candidate`(ナレッジやデバッグログの保存先にしたい場所)が、`id`以外のワークスペースの保存先と重ならないことを確かめる。
    /// 相手の保存先は、現在の場所と既定の場所(「既定に戻す」で使われうる場所)の両方で、ナレッジとデバッグログが対象。
    /// 同じ場所、親子関係(含む・含まれる)のどちらも重なりとして拒否する。
    pub fn ensure_paths_free(&self, id: &str, candidate: &Path) -> Result<(), AppError> {
        let candidate = canonical_lenient(candidate);
        for other in self.load_all().into_iter().filter(|c| c.id != id) {
            for claimed in claimed_paths(&other) {
                if overlaps(&candidate, &canonical_lenient(&claimed)) {
                    return Err(AppError::PathInUse);
                }
            }
        }
        Ok(())
    }

    pub fn list(&self) -> Vec<Project> {
        self.projects().clone()
    }

    /// プロジェクトを作る。名称・色を検証し、上限(`MAX_PROJECTS`)を超える場合は拒否する。
    pub fn create(&self, name: &str, color: &str) -> Result<Project, AppError> {
        let (name, color) = (clean_name(name)?, clean_color(color)?);
        let mut projects = self.projects();
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
        // 先にフォルダとDBを作れることを確かめてから一覧へ載せる
        let ctx = Arc::new(WorkspaceCtx::open(&project.id, &self.project_dir(&project.id))?);
        let mut next = projects.clone();
        next.push(project.clone());
        save_registry(&self.root, &next)?;
        *projects = next;
        self.loaded.write().unwrap_or_else(|p| p.into_inner()).insert(project.id.clone(), ctx);
        Ok(project)
    }

    /// 名称とイメージカラーを変える。
    pub fn update(&self, id: &str, name: &str, color: &str) -> Result<Project, AppError> {
        let (name, color) = (clean_name(name)?, clean_color(color)?);
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
        if id != PERSONAL_ID {
            self.load(id)?;
        }
        *self.active.write().unwrap_or_else(|p| p.into_inner()) = id.to_owned();
        Ok(self.active_info())
    }

    /// プロジェクトを一覧とディスクから削除する。選択中ならパーソナルへ戻す。元に戻せない。
    pub fn delete(&self, id: &str) -> Result<(), AppError> {
        let mut projects = self.projects();
        if !projects.iter().any(|p| p.id == id) {
            return Err(AppError::ProjectNotFound);
        }
        let next: Vec<Project> = projects.iter().filter(|p| p.id != id).cloned().collect();
        save_registry(&self.root, &next)?;
        *projects = next;
        {
            let mut active = self.active.write().unwrap_or_else(|p| p.into_inner());
            if *active == id {
                *active = PERSONAL_ID.to_owned();
            }
        }
        self.loaded.write().unwrap_or_else(|p| p.into_inner()).remove(id);
        // `id`は一覧にあったもの(生成したUUID)だけなので、`workspaces/`配下の1フォルダに限られる
        if let Err(e) = std::fs::remove_dir_all(self.project_dir(id))
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("project dir not removed: {e}");
        }
        Ok(())
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
}
