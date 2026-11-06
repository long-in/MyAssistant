use tauri::State;

use crate::{
    services::{
        debug_log::{self, LogFile},
        workspace::{Ctx, WorkspaceCtx, Workspaces},
    },
    utils::error::AppError,
};

fn log_dir(ctx: &WorkspaceCtx) -> Result<std::path::PathBuf, AppError> {
    debug_log::log_dir(ctx)
}

/// デバッグログの現在の出力先。
#[tauri::command]
pub fn get_debug_log_dir(ctx: Ctx) -> Result<String, AppError> {
    Ok(log_dir(&ctx)?.to_string_lossy().into_owned())
}

/// デバッグログの出力先を変更する。`None`でナレッジ配下の既定に戻す。既存のログは移動しない。
#[tauri::command]
pub fn set_debug_log_dir(ws: State<'_, Workspaces>, ctx: Ctx, path: Option<String>) -> Result<String, AppError> {
    let path = path.map(|p| p.trim().to_owned()).filter(|p| !p.is_empty());
    if let Some(p) = &path {
        // 他のワークスペースのナレッジ・デバッグログの場所やアプリのデータと重なる場所は使えない。確認と保存はまとめて行う
        ws.set_location(&ctx.id, std::path::Path::new(p), || {
            let dir = debug_log::prepare_dir(std::path::Path::new(p))?;
            ctx.db.set_setting(debug_log::DIR_KEY, &dir.to_string_lossy())
        })?;
    } else {
        ctx.db.set_setting(debug_log::DIR_KEY, "")?;
    }
    debug_log::refresh(&ctx)?;
    Ok(log_dir(&ctx)?.to_string_lossy().into_owned())
}

/// デバッグログのファイル一覧(新しい日付が先頭)。
#[tauri::command]
pub fn list_debug_logs(ctx: Ctx) -> Result<Vec<LogFile>, AppError> {
    debug_log::list(&log_dir(&ctx)?)
}

/// ログ1件の本文(JSON Lines)。
#[tauri::command]
pub fn read_debug_log(ctx: Ctx, name: String) -> Result<String, AppError> {
    debug_log::read(&log_dir(&ctx)?, &name)
}

/// ログを削除する。`name`を省略すると全ログを削除し、削除したファイル数を返す。
#[tauri::command]
pub fn delete_debug_logs(ctx: Ctx, name: Option<String>) -> Result<usize, AppError> {
    debug_log::delete(&log_dir(&ctx)?, name.as_deref())
}
