//! 通信のプロキシ設定のコマンド。一覧と既定は全体設定、使うプロキシの選択はエージェントごと。パスワードは書き込みだけで、読み出しては返さない。
use crate::{
    services::{
        proxy::{self, ProxyList},
        workspace::Ctx,
    },
    utils::error::AppError,
};

#[tauri::command]
pub fn get_proxies(ctx: Ctx) -> Result<ProxyList, AppError> {
    proxy::load(&*ctx.global, proxy::secrets()?)
}

/// プロキシを追加(`id`なし)または更新して、次の通信から反映する。`password`は`None`なら据え置き、空文字なら削除。
#[tauri::command]
pub fn save_proxy(
    ctx: Ctx,
    id: Option<String>,
    name: String,
    url: String,
    username: String,
    password: Option<String>,
) -> Result<ProxyList, AppError> {
    let saved = proxy::save(&ctx.global, proxy::secrets()?, id.as_deref(), &name, &url, &username, password.as_deref())?;
    proxy::reload(&*ctx.global)?;
    Ok(saved)
}

#[tauri::command]
pub fn delete_proxy(ctx: Ctx, id: String) -> Result<ProxyList, AppError> {
    let saved = proxy::delete(&ctx.global, proxy::secrets()?, &id)?;
    proxy::reload(&*ctx.global)?;
    Ok(saved)
}

/// 既定のプロキシを決める。空文字なら既定なし。
#[tauri::command]
pub fn set_default_proxy(ctx: Ctx, id: String) -> Result<ProxyList, AppError> {
    let saved = proxy::set_default(&ctx.global, proxy::secrets()?, &id)?;
    proxy::reload(&*ctx.global)?;
    Ok(saved)
}

/// このエージェントが使うプロキシの選択。空は既定を使う、`none`は使わない、それ以外は登録済みのID。
#[tauri::command]
pub fn get_proxy_choice(ctx: Ctx) -> Result<String, AppError> {
    Ok(ctx.db.get_setting(proxy::CHOICE_KEY)?.unwrap_or_default())
}

#[tauri::command]
pub fn set_proxy_choice(ctx: Ctx, choice: String) -> Result<(), AppError> {
    proxy::validate_choice(&*ctx.global, &choice)?;
    ctx.db.set_setting(proxy::CHOICE_KEY, &choice)
}
