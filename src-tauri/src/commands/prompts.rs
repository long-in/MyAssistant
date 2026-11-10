//! 「AI設定」画面から呼ぶ、プロンプトの取得・保存・初期化のコマンド。保存先は選択中のエージェントのDB。
use crate::{
    services::{
        prompts::{self, PromptId, PromptInfo},
        workspace::Ctx,
    },
    utils::error::AppError,
};

/// 全機能のプロンプトの現在の状態。
#[tauri::command]
pub fn list_prompts(ctx: Ctx) -> Result<Vec<PromptInfo>, AppError> {
    prompts::list(&ctx.db, &*ctx.global)
}

/// プロンプトを保存し、保存後の状態を返す。空、または既定と同じ文章は上書きなしになる。
#[tauri::command]
pub fn save_prompt(ctx: Ctx, id: PromptId, text: String) -> Result<PromptInfo, AppError> {
    prompts::save(&ctx.db, &*ctx.global, id, &text)
}

/// 上書きを外して既定へ戻し、戻した後の状態を返す。
#[tauri::command]
pub fn reset_prompt(ctx: Ctx, id: PromptId) -> Result<PromptInfo, AppError> {
    prompts::reset(&ctx.db, &*ctx.global, id)
}

/// 全体設定の側の、全機能のプロンプト。エージェントが上書きしていなければ、これが使われる。
#[tauri::command]
pub fn list_global_prompts(ctx: Ctx) -> Result<Vec<PromptInfo>, AppError> {
    prompts::list_global(&*ctx.global)
}

/// 全体設定のプロンプトを保存し、保存後の状態を返す。空、または組み込みの既定と同じ文章は未設定になる。
#[tauri::command]
pub fn save_global_prompt(ctx: Ctx, id: PromptId, text: String) -> Result<PromptInfo, AppError> {
    prompts::save_global(&*ctx.global, id, &text)
}

/// 全体設定のプロンプトを組み込みの既定へ戻す。
#[tauri::command]
pub fn reset_global_prompt(ctx: Ctx, id: PromptId) -> Result<PromptInfo, AppError> {
    prompts::reset_global(&*ctx.global, id)
}
