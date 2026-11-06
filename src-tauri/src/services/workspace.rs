//! ワークスペース(パーソナル / ワークのプロジェクト)ごとの実行コンテキスト。
//!
//! DB・APIキー・AWS の一時状態をワークスペースごとに分け、混線させない。
//! 現時点ではパーソナル(`PERSONAL_ID`)だけを持つ。
use std::{
    collections::HashMap,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use tauri::{
    Manager, Runtime,
    ipc::{CommandArg, CommandItem, InvokeError},
};

use super::{aws::AwsState, batch::BatchGuard, secrets::Secrets};
use crate::{db::Db, utils::error::AppError};

/// パーソナルワークスペースのID。保存先は従来どおりアプリデータ直下。
pub const PERSONAL_ID: &str = "personal";

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

/// 読み込み済みのワークスペースと、現在選択中のID。
pub struct Workspaces {
    loaded: RwLock<HashMap<String, Arc<WorkspaceCtx>>>,
    active: RwLock<String>,
}

impl Workspaces {
    /// パーソナルだけを持つ状態で始める。
    pub fn new(personal: WorkspaceCtx) -> Self {
        let id = personal.id.clone();
        Self {
            loaded: RwLock::new(HashMap::from([(id.clone(), Arc::new(personal))])),
            active: RwLock::new(id),
        }
    }

    /// 現在選択中のワークスペース。
    pub fn active(&self) -> Arc<WorkspaceCtx> {
        let id = self.active.read().unwrap_or_else(|p| p.into_inner()).clone();
        self.get(&id).expect("the active workspace is always loaded")
    }

    pub fn get(&self, id: &str) -> Option<Arc<WorkspaceCtx>> {
        self.loaded.read().unwrap_or_else(|p| p.into_inner()).get(id).cloned()
    }

    /// 読み込み済みのすべて(バックグラウンド処理の巡回用)。
    pub fn all(&self) -> Vec<Arc<WorkspaceCtx>> {
        self.loaded.read().unwrap_or_else(|p| p.into_inner()).values().cloned().collect()
    }
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

    #[test]
    fn contexts_do_not_share_data() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let ws = Workspaces::new(WorkspaceCtx::open(PERSONAL_ID, a.path()).unwrap());
        let other = WorkspaceCtx::open("p1", b.path()).unwrap();
        ws.active().db.set_setting("k", "personal").unwrap();
        other.db.set_setting("k", "project").unwrap();
        assert_eq!(ws.active().db.get_setting("k").unwrap().as_deref(), Some("personal"));
        assert_eq!(other.db.get_setting("k").unwrap().as_deref(), Some("project"));
        assert_eq!(ws.active().id, PERSONAL_ID);
        assert_eq!(ws.all().len(), 1);
    }
}
