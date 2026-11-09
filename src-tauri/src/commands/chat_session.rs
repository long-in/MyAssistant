//! 会話の継続のコマンド。保存先は選択中のエージェントのDB。
use crate::{
    services::{
        chat_session::{self, SessionView},
        workspace::Ctx,
    },
    utils::error::AppError,
};

/// 現在の会話のIDと、画面に戻すやり取り(アプリを再起動したあとの再開に使う)。
#[tauri::command]
pub fn chat_session_load(ctx: Ctx) -> Result<SessionView, AppError> {
    chat_session::load(&ctx.db)
}

/// 新しい会話を始め、そのIDを返す。
#[tauri::command]
pub fn chat_session_new(ctx: Ctx) -> Result<String, AppError> {
    chat_session::start_new(&ctx.db)
}
