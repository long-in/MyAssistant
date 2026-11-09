//! 通信のプロキシ設定のコマンド。パスワードは書き込みだけで、読み出しては返さない。
use crate::{
    services::{
        proxy::{self, ProxySettings},
        workspace::Ctx,
    },
    utils::error::AppError,
};

#[tauri::command]
pub fn get_proxy_settings(ctx: Ctx) -> Result<ProxySettings, AppError> {
    proxy::load(&*ctx.global, proxy::secrets()?)
}

/// プロキシ設定を保存して、次の通信から反映する。`password`は`None`なら据え置き、空文字なら削除。
#[tauri::command]
pub fn set_proxy_settings(
    ctx: Ctx,
    enabled: bool,
    url: String,
    username: String,
    password: Option<String>,
) -> Result<ProxySettings, AppError> {
    let saved = proxy::save(&ctx.global, proxy::secrets()?, enabled, &url, &username, password.as_deref())?;
    proxy::reload(&ctx.global)?;
    Ok(saved)
}
