use tauri::State;

use crate::{
    db::Db,
    services::profile::{self, UserProfile},
    utils::error::AppError,
};

/// 保存済みのパーソナルデータ。未保存なら初期値。
#[tauri::command]
pub fn get_profile(db: State<'_, Db>) -> Result<UserProfile, AppError> {
    profile::load(&db)
}

/// 検証して保存し、整えた後の値を返す。
#[tauri::command]
pub fn save_profile(db: State<'_, Db>, profile: UserProfile) -> Result<UserProfile, AppError> {
    profile::save(&db, &profile, chrono::Local::now().date_naive())
}

/// 保存したパーソナルデータをすべて消す。
#[tauri::command]
pub fn clear_profile(db: State<'_, Db>) -> Result<(), AppError> {
    profile::clear(&db)
}
