use tauri::State;

use crate::{
    services::{
        aws::{self, ProfileInfo, Status},
        workspace::{Ctx, Workspaces},
    },
    utils::error::AppError,
};

#[derive(serde::Serialize)]
pub struct AwsFiles {
    pub config: String,
    pub credentials: String,
}

/// このワークスペースの`secrets.json`をFinderで選択した状態で開く。未作成ならワークスペースのフォルダを開く。
/// パスは`ctx.dir`から組み立てるだけで、ファイルの中身は読まない。
#[tauri::command]
pub fn reveal_secrets_file(ctx: Ctx) -> Result<(), AppError> {
    let file = ctx.dir.join("secrets.json");
    let mut cmd = std::process::Command::new("/usr/bin/open");
    if file.exists() {
        cmd.arg("-R").arg(file);
    } else {
        cmd.arg(&ctx.dir);
    }
    let status = cmd.status().map_err(|e| AppError::Io(e.to_string()))?;
    if status.success() { Ok(()) } else { Err(AppError::Io(format!("open failed: {status}"))) }
}

/// 登録有無・プロファイル一覧・選択中のプロファイル。キーの生値は返さない。
#[tauri::command]
pub fn get_aws_status(ctx: Ctx) -> Result<Status, AppError> {
    aws::status(&ctx)
}

/// 保存済みの`~/.aws/config`と`credentials`の全文。ユーザー本人が画面で確認・編集するための例外的な公開で、
/// ログ・LLM・他のコマンドへは渡さない。
#[tauri::command]
pub fn get_aws_files(ctx: Ctx) -> Result<AwsFiles, AppError> {
    let (config, credentials) = ctx.secrets.get_aws_files()?;
    Ok(AwsFiles { config, credentials })
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

/// 認証が通るかを確認する。MFAやSSOの承認が必要なら入力要求のイベントが飛ぶ。
#[tauri::command]
pub async fn test_aws_connection(ctx: Ctx) -> Result<Status, AppError> {
    aws::test_connection(&ctx).await.map_err(|e| e.with_source("bedrock"))?;
    aws::status(&ctx)
}

/// 入力されたMFAコードを、待っているBedrock呼び出しへ渡す。
/// コードは、求めたワークスペース(`workspace_id`)へ返す。選択中とは限らない(裏のエージェントが求めることがある)。
#[tauri::command]
pub fn submit_aws_mfa(ws: State<'_, Workspaces>, workspace_id: String, code: String) -> Result<(), AppError> {
    let ctx = ws.resolve(&workspace_id)?;
    aws::submit_mfa(&ctx, &code)
}

#[tauri::command]
pub fn cancel_aws_mfa(ws: State<'_, Workspaces>, workspace_id: String) {
    if let Ok(ctx) = ws.resolve(&workspace_id) {
        aws::cancel_mfa(&ctx);
    }
}

/// 承認待ちのSSOの承認ページを、既定のブラウザで開く。URLはRustが控えているものを使い、フロントエンドからは受け取らない。
#[tauri::command]
pub fn open_aws_sso(ws: State<'_, Workspaces>, workspace_id: String) -> Result<(), AppError> {
    let ctx = ws.resolve(&workspace_id)?;
    aws::open_sso(&ctx)
}

/// SSOの承認待ちを取りやめる。
#[tauri::command]
pub fn cancel_aws_sso(ws: State<'_, Workspaces>, workspace_id: String) {
    if let Ok(ctx) = ws.resolve(&workspace_id) {
        aws::cancel_sso(&ctx);
    }
}
