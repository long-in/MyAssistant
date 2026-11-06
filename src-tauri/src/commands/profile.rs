use crate::{
    services::{
        profile::{self, UserProfile},
        workspace::Ctx,
    },
    utils::error::AppError,
};

/// 保存済みのパーソナルデータ。未保存なら初期値。
#[tauri::command]
pub fn get_profile(ctx: Ctx) -> Result<UserProfile, AppError> {
    profile::load(&ctx.db)
}

/// 検証して保存し、整えた後の値を返す。
#[tauri::command]
pub fn save_profile(ctx: Ctx, profile: UserProfile) -> Result<UserProfile, AppError> {
    profile::save(&ctx.db, &profile, chrono::Local::now().date_naive())
}

/// 保存したパーソナルデータをすべて消す。
#[tauri::command]
pub fn clear_profile(ctx: Ctx) -> Result<(), AppError> {
    profile::clear(&ctx.db)
}
