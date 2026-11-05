use crate::{
    services::aws::{self, ProfileInfo, Status},
    utils::error::AppError,
};

/// 登録有無・プロファイル一覧・選択中のプロファイル。キーの生値は返さない。
#[tauri::command]
pub fn get_aws_status() -> Result<Status, AppError> {
    aws::status()
}

/// 貼り付け中の内容から読み取れるプロファイル一覧(保存しない)。使うプロファイルを選ばせるために使う。
#[tauri::command]
pub fn preview_aws_config(config: String, credentials: String) -> Vec<ProfileInfo> {
    aws::preview(&config, &credentials)
}

/// `~/.aws/config`と`~/.aws/credentials`の貼り付け内容と、使うプロファイルを検証して保存する。選んだプロファイルが使えなければ保存しない。
#[tauri::command]
pub fn save_aws_config(config: String, credentials: String, profile: String) -> Result<Status, AppError> {
    aws::save(&config, &credentials, &profile)?;
    aws::status()
}

#[tauri::command]
pub fn select_aws_profile(name: String) -> Result<Status, AppError> {
    aws::select_profile(&name)?;
    aws::status()
}

/// 認証からモデル呼び出しまでを通しで確認する。MFAが必要なら入力要求のイベントが飛ぶ。
#[tauri::command]
pub async fn test_aws_connection() -> Result<Status, AppError> {
    aws::test_connection().await.map_err(|e| e.with_source("bedrock"))?;
    aws::status()
}

/// Bedrockから取得した`global.anthropic.`の推論プロファイルID。MFAが必要なら入力要求のイベントが飛ぶ。
#[tauri::command]
pub async fn fetch_aws_models() -> Result<Vec<String>, AppError> {
    aws::fetch_global_models().await.map_err(|e| e.with_source("bedrock"))
}

/// 使うBedrockのモデルID(推論プロファイルIDやARNも可)を追加する。
#[tauri::command]
pub fn add_aws_model(model: String) -> Result<Status, AppError> {
    aws::add_model(&model)?;
    aws::status()
}

#[tauri::command]
pub fn remove_aws_model(model: String) -> Result<Status, AppError> {
    aws::remove_model(&model)?;
    aws::status()
}

/// 入力されたMFAコードを、待っているBedrock呼び出しへ渡す。
#[tauri::command]
pub fn submit_aws_mfa(code: String) -> Result<(), AppError> {
    aws::submit_mfa(&code)
}

#[tauri::command]
pub fn cancel_aws_mfa() {
    aws::cancel_mfa();
}
