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

/// 新しい会話を始める。IDは画面が決めて渡す(画面は待たずに新しい会話へ切り替える)。
#[tauri::command]
pub fn chat_session_new(ctx: Ctx, session_id: String) -> Result<(), AppError> {
    chat_session::set(&ctx.db, &session_id)
}
