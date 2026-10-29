use std::{collections::BTreeMap, str::FromStr};

use tauri::{AppHandle, State};

use crate::{
    db::Db,
    services::{
        debug_log,
        secrets::{self, Provider},
    },
    utils::error::AppError,
};

/// 各プロバイダーのキー登録有無のみを返す(生値は返さない)。
#[tauri::command]
pub fn get_key_status() -> Result<BTreeMap<&'static str, bool>, AppError> {
    Provider::ALL
        .into_iter()
        .map(|p| Ok((p.as_str(), secrets::has_key(p)?)))
        .collect()
}

/// 疎通確認に成功した場合のみアプリデータ内のキー保存ファイルへ保存する。
#[tauri::command]
pub async fn save_api_key(provider: String, key: String) -> Result<(), AppError> {
    let provider = Provider::from_str(&provider)?;
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::EmptyKey);
    }
    secrets::verify_key(provider, key).await?;
    secrets::set_key(provider, key)
}

#[tauri::command]
pub fn delete_api_key(provider: String) -> Result<(), AppError> {
    secrets::delete_key(Provider::from_str(&provider)?)
}

#[tauri::command]
pub fn get_debug_mode(db: State<'_, Db>) -> Result<bool, AppError> {
    debug_log::is_enabled(&db)
}

/// デバッグモードを切り替える。有効にするとナレッジ配下の`debug-logs`へ記録を始める。
#[tauri::command]
pub fn set_debug_mode(app: AppHandle, db: State<'_, Db>, enabled: bool) -> Result<(), AppError> {
    db.set_setting(debug_log::ENABLED_KEY, if enabled { "1" } else { "0" })?;
    debug_log::refresh(&app)
}
