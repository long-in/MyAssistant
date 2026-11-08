use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::{
    services::{
        backup::{self, Preview, Summary},
        workspace::Workspaces,
    },
    utils::error::AppError,
};

/// ダイアログで選ばれた、拡張子`.zip`の絶対パスのみ受け付ける。
fn zip_path(path: &str) -> Result<PathBuf, AppError> {
    let p = PathBuf::from(path.trim());
    if !p.is_absolute() || p.extension().and_then(|e| e.to_str()) != Some("zip") {
        return Err(AppError::PathNotAllowed);
    }
    Ok(p)
}

fn root(app: &AppHandle) -> PathBuf {
    app.state::<Workspaces>().root().to_owned()
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, AppError> + Send + 'static) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| AppError::Io(e.to_string()))?
}

/// アプリ全体(全スペースのDB・ナレッジ・APIキーを含む)を`dest`のzipへ書き出す。`password`があればAES-256で暗号化する。
#[tauri::command]
pub async fn backup_create(app: AppHandle, dest: String, password: Option<String>) -> Result<Summary, AppError> {
    let dest = zip_path(&dest)?;
    let root = root(&app);
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let password = password.filter(|p| !p.is_empty());
    blocking(move || backup::create(&root, &dest, now, password)).await
}

/// バックアップのzipを検証しながら展開する。現在のデータは変えない。暗号化されていれば`password`が要る。
#[tauri::command]
pub async fn restore_prepare(app: AppHandle, src: String, password: Option<String>) -> Result<Preview, AppError> {
    let src = zip_path(&src)?;
    let root = root(&app);
    let password = password.filter(|p| !p.is_empty());
    blocking(move || backup::prepare(&root, &src, password.as_deref())).await
}

/// 展開済みのバックアップで、再起動して差し替える。
#[tauri::command]
pub fn restore_apply(app: AppHandle) -> Result<(), AppError> {
    backup::schedule(&root(&app))?;
    app.restart()
}

/// 展開済みのバックアップを捨てる。
#[tauri::command]
pub fn restore_discard(app: AppHandle) {
    backup::discard(&root(&app));
}
