use crate::{
    services::{
        command_policy::{self, Policy},
        workspace::Ctx,
    },
    utils::error::AppError,
};

/// コマンド権限(許可・都度承認・拒否のリスト)。未保存なら初期値を保存して返す。
#[tauri::command]
pub fn get_command_policy(ctx: Ctx) -> Result<Policy, AppError> {
    Ok(command_policy::load(&ctx.db))
}

/// コマンド権限を保存し、整形後の内容を返す。
#[tauri::command]
pub fn set_command_policy(ctx: Ctx, policy: Policy) -> Result<Policy, AppError> {
    command_policy::save(&ctx.db, &policy).map_err(AppError::Io)
}

/// 初期値のコマンド権限(画面の「初期値に戻す」用)。保存はしない。
#[tauri::command]
pub fn get_default_command_policy() -> Policy {
    Policy::defaults()
}
