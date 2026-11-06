use crate::{
    services::{
        batch,
        command_policy::{self, Policy},
        workspace::Ctx,
    },
    utils::error::AppError,
};

/// コマンド権限(許可・都度承認・拒否のリスト)。ファイルが無ければ初期値を書き出して返す。
#[tauri::command]
pub fn get_command_policy(ctx: Ctx) -> Result<Policy, AppError> {
    Ok(command_policy::load(&batch::knowledge_root(&ctx)?))
}

/// コマンド権限を保存し、整形後の内容を返す。
#[tauri::command]
pub fn set_command_policy(ctx: Ctx, policy: Policy) -> Result<Policy, AppError> {
    command_policy::save(&batch::knowledge_root(&ctx)?, &policy).map_err(AppError::Io)
}

/// 初期値のコマンド権限(画面の「初期値に戻す」用)。保存はしない。
#[tauri::command]
pub fn get_default_command_policy() -> Policy {
    Policy::defaults()
}
