//! エージェントごとのDockerサンドボックスの設定・状態・ファイル操作。環境変数の値は画面へ返さない。
use serde::Serialize;

use crate::{
    commands::chat::{SANDBOX_ENABLED_KEY, sandbox_enabled},
    services::{
        cancel::CancelToken,
        sandbox::{self, DockerfileState, Status},
        sandbox_env,
        workspace::{Ctx, WorkspaceCtx},
    },
    utils::error::AppError,
};

fn agent_only(ctx: &WorkspaceCtx) -> Result<(), AppError> {
    if sandbox::is_agent(&ctx.id) {
        Ok(())
    } else {
        Err(AppError::Sandbox("サンドボックスを使えるのはエージェントだけです。".into()))
    }
}

#[tauri::command]
pub fn get_sandbox_enabled(ctx: Ctx) -> Result<bool, AppError> {
    sandbox_enabled(&ctx)
}

/// 有効にするときは、標準のDockerfileと環境変数ファイルを(無ければ)用意する。
#[tauri::command]
pub fn set_sandbox_enabled(ctx: Ctx, enabled: bool) -> Result<(), AppError> {
    agent_only(&ctx)?;
    if enabled {
        sandbox::ensure_template(&ctx.dir).map_err(AppError::Sandbox)?;
        sandbox_env::ensure_template(&ctx.dir).map_err(AppError::Sandbox)?;
    }
    ctx.db.set_setting(SANDBOX_ENABLED_KEY, if enabled { "1" } else { "0" })
}

#[tauri::command]
pub async fn get_sandbox_status(ctx: Ctx) -> Result<Status, AppError> {
    agent_only(&ctx)?;
    let (id, dir) = (ctx.id.clone(), ctx.dir.clone());
    Ok(sandbox::status(&id, &dir).await)
}

#[derive(Serialize)]
pub struct Files {
    pub dockerfile: DockerfileState,
    /// 環境変数の名前と検証結果。値は含めない。
    pub env: sandbox_env::Summary,
}

#[tauri::command]
pub fn get_sandbox_files(ctx: Ctx) -> Result<Files, AppError> {
    agent_only(&ctx)?;
    Ok(Files { dockerfile: sandbox::dockerfile_state(&ctx.dir), env: sandbox_env::summary(&ctx.dir) })
}

/// イメージをビルドする。進捗は`sandbox-build-progress`イベントで流れる。`force`ならビルド済みでも作り直す。
#[tauri::command]
pub async fn build_sandbox_image(ctx: Ctx, force: bool) -> Result<(), AppError> {
    agent_only(&ctx)?;
    let (id, dir) = (ctx.id.clone(), ctx.dir.clone());
    sandbox::build_image(&id, &dir, force, &CancelToken::default(), &mut |_| {})
        .await
        .map(|_| ())
        .map_err(AppError::Sandbox)
}

/// コンテナを作り直す(ボリュームは残す)。次のコマンドで新しく作られる。
#[tauri::command]
pub async fn reset_sandbox(ctx: Ctx) -> Result<(), AppError> {
    agent_only(&ctx)?;
    let id = ctx.id.clone();
    sandbox::reset_container(&id).await.map_err(AppError::Sandbox)
}

/// 標準のDockerfileで上書きする。
#[tauri::command]
pub fn reset_sandbox_dockerfile(ctx: Ctx) -> Result<(), AppError> {
    agent_only(&ctx)?;
    sandbox::reset_dockerfile(&ctx.dir).map_err(AppError::Sandbox)
}

/// Finderでファイルを選択した状態で開く。`kind`は`env`または`dockerfile`。
#[tauri::command]
pub fn reveal_sandbox_file(ctx: Ctx, kind: String) -> Result<(), AppError> {
    agent_only(&ctx)?;
    let file = match kind.as_str() {
        "env" => sandbox_env::ensure_template(&ctx.dir),
        "dockerfile" => sandbox::ensure_template(&ctx.dir),
        _ => return Err(AppError::Sandbox("不明なファイルです。".into())),
    }
    .map_err(AppError::Sandbox)?;
    let status = std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(file)
        .status()
        .map_err(|e| AppError::Io(e.to_string()))?;
    if status.success() { Ok(()) } else { Err(AppError::Io(format!("open failed: {status}"))) }
}
