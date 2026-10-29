use tauri::AppHandle;

use crate::{
    services::{
        batch,
        debug_log::{self, LogFile},
    },
    utils::error::AppError,
};

fn log_dir(app: &AppHandle) -> Result<std::path::PathBuf, AppError> {
    Ok(batch::knowledge_root(app)?.join(debug_log::DIR_NAME))
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
