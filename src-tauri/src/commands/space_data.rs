use tauri::{AppHandle, Manager};

use crate::{
    services::{
        space_data::{self, Note},
        workspace::{Ctx, PERSONAL_ID, Workspaces},
    },
    utils::error::AppError,
};

/// このスペースのスペースデータ。未保存なら初期値。
#[tauri::command]
pub fn get_space_data(ctx: Ctx) -> Result<Note, AppError> {
    space_data::load(&ctx.db)
}

/// 検証して保存し、整えた後の値を返す。パーソナルには使わない(パーソナルにはパーソナルデータがある)。
#[tauri::command]
pub fn save_space_data(ctx: Ctx, note: Note) -> Result<Note, AppError> {
    if ctx.id == PERSONAL_ID {
        return Err(AppError::InvalidSpace("personal has no space data".into()));
    }
    space_data::save(&ctx.db, &note)
}

/// 全スペース共通の共通データ。未保存なら初期値。
#[tauri::command]
pub fn get_shared_data(app: AppHandle) -> Note {
    space_data::load_shared(app.state::<Workspaces>().root())
}

/// 共通データを検証して保存し、整えた後の値を返す。
#[tauri::command]
pub fn save_shared_data(app: AppHandle, note: Note) -> Result<Note, AppError> {
    space_data::save_shared(app.state::<Workspaces>().root(), &note)
}

