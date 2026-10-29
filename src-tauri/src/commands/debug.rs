use tauri::{AppHandle, State};

use crate::{
    db::Db,
    services::debug_log::{self, LogFile},
    utils::error::AppError,
};

fn log_dir(app: &AppHandle) -> Result<std::path::PathBuf, AppError> {
    debug_log::log_dir(app)
}

/// デバッグログの現在の出力先。
#[tauri::command]
pub fn get_debug_log_dir(app: AppHandle) -> Result<String, AppError> {
    Ok(log_dir(&app)?.to_string_lossy().into_owned())
}

/// デバッグログの出力先を変更する。`None`でナレッジ配下の既定に戻す。既存のログは移動しない。
#[tauri::command]
pub fn set_debug_log_dir(app: AppHandle, db: State<'_, Db>, path: Option<String>) -> Result<String, AppError> {
    let path = path.map(|p| p.trim().to_owned()).filter(|p| !p.is_empty());
    if let Some(p) = &path {
        let dir = debug_log::prepare_dir(std::path::Path::new(p))?;
        db.set_setting(debug_log::DIR_KEY, &dir.to_string_lossy())?;
    } else {
        db.set_setting(debug_log::DIR_KEY, "")?;
    }
    debug_log::refresh(&app)?;
    Ok(log_dir(&app)?.to_string_lossy().into_owned())
}

/// デバッグログのファイル一覧(新しい日付が先頭)。
#[tauri::command]
pub fn list_debug_logs(app: AppHandle) -> Result<Vec<LogFile>, AppError> {
    debug_log::list(&log_dir(&app)?)
}

/// ログ1件の本文(JSON Lines)。
#[tauri::command]
pub fn read_debug_log(app: AppHandle, name: String) -> Result<String, AppError> {
    debug_log::read(&log_dir(&app)?, &name)
}

/// ログを削除する。`name`を省略すると全ログを削除し、削除したファイル数を返す。
#[tauri::command]
pub fn delete_debug_logs(app: AppHandle, name: Option<String>) -> Result<usize, AppError> {
    debug_log::delete(&log_dir(&app)?, name.as_deref())
}
