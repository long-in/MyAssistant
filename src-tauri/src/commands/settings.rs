use std::{collections::BTreeMap, str::FromStr};

use crate::{
    services::{
        aws, debug_log,
        secrets::{self, Provider},
        workspace::Ctx,
    },
    utils::error::AppError,
};

/// 各プロバイダーのキー登録有無のみを返す(生値は返さない)。
#[tauri::command]
pub fn get_key_status(ctx: Ctx) -> Result<BTreeMap<&'static str, bool>, AppError> {
    Provider::ALL
        .into_iter()
        .map(|p| Ok((p.as_str(), ctx.secrets.has_key(p)?)))
        .collect()
}

/// 疎通確認に成功した場合のみアプリデータ内のキー保存ファイルへ保存する。
#[tauri::command]
pub async fn save_api_key(ctx: Ctx, provider: String, key: String) -> Result<(), AppError> {
    let provider = Provider::from_str(&provider)?;
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::EmptyKey);
    }
    secrets::verify_key(provider, key).await?;
    ctx.secrets.set_key(provider, key)
}

#[tauri::command]
pub fn delete_api_key(ctx: Ctx, provider: String) -> Result<(), AppError> {
    match Provider::from_str(&provider)? {
        // 保存内容に加えて、メモリ上の一時クレデンシャルも消す
        Provider::Bedrock => aws::delete(&ctx),
        p => ctx.secrets.delete_key(p),
    }
}

#[tauri::command]
pub fn get_debug_mode(ctx: Ctx) -> Result<bool, AppError> {
    debug_log::is_enabled(&ctx.db)
}

/// デバッグモードを切り替える。有効にするとナレッジ配下の`debug-logs`へ記録を始める。
#[tauri::command]
pub fn set_debug_mode(ctx: Ctx, enabled: bool) -> Result<(), AppError> {
    ctx.db.set_setting(debug_log::ENABLED_KEY, if enabled { "1" } else { "0" })?;
    debug_log::refresh(&ctx)
}
