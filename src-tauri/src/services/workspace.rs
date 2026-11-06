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

use super::{aws::AwsState, batch::BatchGuard, secrets::Secrets};
use crate::{db::Db, utils::error::AppError};

/// パーソナルワークスペースのID。保存先は従来どおりアプリデータ直下。
pub const PERSONAL_ID: &str = "personal";
/// 作れるプロジェクトの上限。
pub const MAX_PROJECTS: usize = 30;
/// プロジェクト名の最大文字数。
pub const NAME_MAX_CHARS: usize = 40;
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

/// `#rrggbb`だけを受け付け、小文字へそろえる。
fn clean_color(color: &str) -> Result<String, AppError> {
    let c = color.trim();
    let ok = c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit());
    if !ok {
        return Err(AppError::InvalidProject("color".into()));
    }
    Ok(c.to_ascii_lowercase())
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
        let a = ws.create("A", "#112233").unwrap();
        let b = ws.create("B", "#445566").unwrap();
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
            assert!(matches!(ws.create(name, "#000000"), Err(AppError::InvalidProject(_))), "{name:?}");
        }
        for color in ["", "red", "#12345", "#1234567", "#12345g", "112233"] {
            assert!(matches!(ws.create("x", color), Err(AppError::InvalidProject(_))), "{color:?}");
        }
        let p = ws.create("  営業  ", "#ABCDEF").unwrap();
        assert_eq!((p.name.as_str(), p.color.as_str()), ("営業", "#abcdef"));
        assert!(ws.create(&"あ".repeat(NAME_MAX_CHARS), "#000000").is_ok());
    }

    #[test]
    fn enforces_project_limit() {
        let d = tempfile::tempdir().unwrap();
        let ws = workspaces(d.path());
        for i in 0..MAX_PROJECTS {
            ws.create(&format!("p{i}"), "#000000").unwrap();
        }
        assert!(matches!(ws.create("over", "#000000"), Err(AppError::ProjectLimit)));
        let first = ws.list()[0].id.clone();
        ws.delete(&first).unwrap();
        assert!(ws.create("again", "#000000").is_ok());
    }

    #[test]
    fn registry_persists_and_projects_reload_lazily() {
        let d = tempfile::tempdir().unwrap();
        let id = {
            let ws = workspaces(d.path());
            let p = ws.create("A", "#112233").unwrap();
            ws.get(&p.id).unwrap().db.set_setting("k", "v").unwrap();
            ws.update(&p.id, "A2", "#223344").unwrap();
            p.id
        };
        let ws = workspaces(d.path());
        assert!(ws.get(&id).is_none(), "not loaded until needed");
        assert_eq!(ws.list()[0].name, "A2");
        let info = ws.switch(&id).unwrap();
        assert_eq!(info.project.unwrap().color, "#223344");
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
        let p = ws.create("A", "#112233").unwrap();
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
}
