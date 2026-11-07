//! 全体設定のうち、画面の環境に関わる項目(表示言語・OS通知)。
//! ベースプロンプトなどは、従来のコマンドが全体設定へ読み書きする。
use serde::Serialize;

use crate::{
    services::{
        global_settings::{LANGUAGE_KEY, OS_NOTIFICATIONS_KEY, SettingsStore},
        workspace::Ctx,
    },
    utils::error::AppError,
};

/// 画面の環境の設定。未保存の項目は`None`(画面が、この端末の保存値から初期値を決めて保存する)。
#[derive(Debug, Serialize)]
pub struct GlobalPrefs {
    pub language: Option<String>,
    pub os_notifications: Option<bool>,
}

#[tauri::command]
pub fn get_global_prefs(ctx: Ctx) -> Result<GlobalPrefs, AppError> {
    Ok(GlobalPrefs {
        language: ctx.global.get_setting(LANGUAGE_KEY)?.filter(|l| matches!(l.as_str(), "ja" | "en")),
        os_notifications: ctx.global.get_setting(OS_NOTIFICATIONS_KEY)?.map(|v| v != "0"),
    })
}

/// 表示言語(`ja` / `en`)を全体設定へ保存する。
#[tauri::command]
pub fn set_global_language(ctx: Ctx, language: String) -> Result<(), AppError> {
    if !matches!(language.as_str(), "ja" | "en") {
        return Err(AppError::InvalidSettings(format!("language: {language}")));
    }
    ctx.global.set_setting(LANGUAGE_KEY, &language)
}

/// OS通知の使用を全体設定へ保存する。
#[tauri::command]
pub fn set_global_os_notifications(ctx: Ctx, enabled: bool) -> Result<(), AppError> {
    ctx.global.set_setting(OS_NOTIFICATIONS_KEY, if enabled { "1" } else { "0" })
}
