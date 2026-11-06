use crate::{
    services::{
        aws::{self, ProfileInfo, Status},
        workspace::Ctx,
    },
    utils::error::AppError,
};

/// 登録有無・プロファイル一覧・選択中のプロファイル。キーの生値は返さない。
#[tauri::command]
pub fn get_aws_status(ctx: Ctx) -> Result<Status, AppError> {
    aws::status(&ctx)
}

/// 貼り付け中の内容から読み取れるプロファイル一覧(保存しない)。使うプロファイルを選ばせるために使う。
#[tauri::command]
pub fn preview_aws_config(config: String, credentials: String) -> Vec<ProfileInfo> {
    aws::preview(&config, &credentials)
}

/// `~/.aws/config`と`~/.aws/credentials`の貼り付け内容と、使うプロファイルを検証して保存する。選んだプロファイルが使えなければ保存しない。
#[tauri::command]
pub fn save_aws_config(ctx: Ctx, config: String, credentials: String, profile: String) -> Result<Status, AppError> {
    aws::save(&ctx, &config, &credentials, &profile)?;
    aws::status(&ctx)
}

#[tauri::command]
pub fn select_aws_profile(ctx: Ctx, name: String) -> Result<Status, AppError> {
    aws::select_profile(&ctx, &name)?;
    aws::status(&ctx)
}

/// 認証からモデル呼び出しまでを通しで確認する。MFAが必要なら入力要求のイベントが飛ぶ。
#[tauri::command]
pub async fn test_aws_connection(ctx: Ctx) -> Result<Status, AppError> {
    aws::test_connection(&ctx).await.map_err(|e| e.with_source("bedrock"))?;
    aws::status(&ctx)
}

/// Bedrockから取得した`global.anthropic.`の推論プロファイルID。MFAが必要なら入力要求のイベントが飛ぶ。
#[tauri::command]
pub async fn fetch_aws_models(ctx: Ctx) -> Result<Vec<String>, AppError> {
    aws::fetch_global_models(&ctx).await.map_err(|e| e.with_source("bedrock"))
}

/// 使うBedrockのモデルID(推論プロファイルIDやARNも可)を追加する。
#[tauri::command]
pub fn add_aws_model(ctx: Ctx, model: String) -> Result<Status, AppError> {
    aws::add_model(&ctx, &model)?;
    aws::status(&ctx)
}

#[tauri::command]
pub fn remove_aws_model(ctx: Ctx, model: String) -> Result<Status, AppError> {
    aws::remove_model(&ctx, &model)?;
    aws::status(&ctx)
}

/// 入力されたMFAコードを、待っているBedrock呼び出しへ渡す。
#[tauri::command]
pub fn submit_aws_mfa(ctx: Ctx, code: String) -> Result<(), AppError> {
    aws::submit_mfa(&ctx, &code)
}

#[tauri::command]
pub fn cancel_aws_mfa(ctx: Ctx) {
    aws::cancel_mfa(&ctx);
}
