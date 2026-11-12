use crate::{
    services::{
        permissions::{self, Preset, PresetInfo},
        workspace::Ctx,
    },
    utils::error::AppError,
};

/// API 方式の settings.json。未保存なら標準のプリセットを書き出して返す。
#[tauri::command]
pub fn get_api_settings(ctx: Ctx) -> String {
    permissions::load_settings(&ctx.db)
}

/// API 方式の settings.json を検証して保存し、保存後の内容を返す。空は受け付けない。次のコマンドから有効になる。
#[tauri::command]
pub fn set_api_settings(ctx: Ctx, text: String) -> Result<String, AppError> {
    // 検証の誤りは利用者に見せる文面(内容は含まない)なので、そのまま返す
    permissions::save(&ctx.db, &text).map_err(AppError::Sandbox)?;
    Ok(permissions::load_settings(&ctx.db))
}

/// 初期値のプリセットの一覧(先頭が既定)。保存はしない。
#[tauri::command]
pub fn get_api_settings_presets() -> Vec<PresetInfo> {
    permissions::presets()
}

/// 選んだプリセットを保存し(いまの内容は捨てる)、保存した内容を返す。
#[tauri::command]
pub fn reset_api_settings(ctx: Ctx, preset: Preset) -> Result<String, AppError> {
    permissions::reset(&ctx.db, preset).map_err(AppError::Sandbox)
}
