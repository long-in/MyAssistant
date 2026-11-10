//! エージェントごとのDockerサンドボックスの設定・状態・ファイル操作。環境変数の値は画面へ返さない。
use serde::Serialize;

use crate::{
    services::{
        cancel::CancelToken,
        sandbox::{self, DockerfileState, ImageStatus, Status},
        sandbox_agent, sandbox_env,
        workspace::{Ctx, WorkspaceCtx},
    },
    utils::error::AppError,
};

fn supported(ctx: &WorkspaceCtx) -> Result<(), AppError> {
    if sandbox::is_supported(&ctx.id) {
        Ok(())
    } else {
        Err(AppError::Sandbox("リーダーではサンドボックスを使えません。".into()))
    }
}

/// このエージェントがサンドボックスで動くか(ワークのエージェントは常に動く。パーソナルとリーダーは動かない)。
#[tauri::command]
pub fn get_sandbox_enabled(ctx: Ctx) -> bool {
    crate::commands::chat::sandbox_enabled(&ctx)
}

/// このエージェントのコンテナの状態。
#[tauri::command]
pub async fn get_sandbox_status(ctx: Ctx) -> Result<Status, AppError> {
    supported(&ctx)?;
    let (id, dir) = (ctx.id.clone(), ctx.dir.clone());
    let app = sandbox::app_dir().map_err(AppError::Sandbox)?;
    Ok(sandbox::status(&id, &app, &dir).await)
}

/// 共通イメージの状態(Dockerの状態と、現在のDockerfileのイメージがビルド済みか)。
#[tauri::command]
pub async fn get_sandbox_image_status() -> Result<ImageStatus, AppError> {
    let dir = sandbox::app_dir().map_err(AppError::Sandbox)?;
    Ok(sandbox::image_status(&dir).await)
}

#[derive(Serialize)]
pub struct Files {
    /// 環境変数の名前と検証結果。値は含めない。
    pub env: sandbox_env::Summary,
}

#[tauri::command]
pub fn get_sandbox_files(ctx: Ctx) -> Result<Files, AppError> {
    supported(&ctx)?;
    Ok(Files { env: sandbox_env::summary(&ctx.dir) })
}

#[derive(Serialize)]
pub struct EnvFile {
    /// ファイルの内容(値を含む)。
    pub content: String,
    pub summary: sandbox_env::Summary,
}

/// このエージェントの`sandbox.env`の内容。無ければ既定の内容で作る。
#[tauri::command]
pub fn get_sandbox_env(ctx: Ctx) -> Result<EnvFile, AppError> {
    supported(&ctx)?;
    let content = sandbox_env::read(&ctx.dir).map_err(AppError::Sandbox)?;
    Ok(EnvFile { content, summary: sandbox_env::summary(&ctx.dir) })
}

/// `sandbox.env`を保存する。書式に誤りがあれば保存せず、行番号つきのエラーを返す。
#[tauri::command]
pub fn set_sandbox_env(ctx: Ctx, content: String) -> Result<sandbox_env::Summary, AppError> {
    supported(&ctx)?;
    sandbox_env::write(&ctx.dir, &content).map_err(|e| {
        AppError::Sandbox(match e.line {
            Some(n) => format!("{n}行目が正しくありません: {}", e.message),
            None => e.message,
        })
    })?;
    Ok(sandbox_env::summary(&ctx.dir))
}

#[derive(Serialize)]
pub struct DockerfileInfo {
    pub content: String,
    pub state: DockerfileState,
}

/// 全エージェント共通のDockerfile。無ければ標準を作る。
#[tauri::command]
pub fn get_sandbox_dockerfile() -> Result<DockerfileInfo, AppError> {
    let dir = sandbox::app_dir().map_err(AppError::Sandbox)?;
    let content = sandbox::read_dockerfile(&dir).map_err(AppError::Sandbox)?;
    Ok(DockerfileInfo { content, state: sandbox::dockerfile_state(&dir) })
}

/// 画面で編集したDockerfileを保存する。次のコマンド実行時(またはビルド)に新しいイメージになる。
#[tauri::command]
pub fn set_sandbox_dockerfile(content: String) -> Result<DockerfileState, AppError> {
    let dir = sandbox::app_dir().map_err(AppError::Sandbox)?;
    sandbox::write_dockerfile(&dir, &content).map_err(AppError::Sandbox)?;
    Ok(sandbox::dockerfile_state(&dir))
}

/// 標準のDockerfileで上書きする。
#[tauri::command]
pub fn reset_sandbox_dockerfile() -> Result<(), AppError> {
    let dir = sandbox::app_dir().map_err(AppError::Sandbox)?;
    sandbox::reset_dockerfile(&dir).map_err(AppError::Sandbox)
}

/// 共通イメージをビルドする。進捗は`sandbox-build-progress`イベントで流れる。`force`ならビルド済みでも作り直す。
#[tauri::command]
pub async fn build_sandbox_image(force: bool) -> Result<(), AppError> {
    let dir = sandbox::app_dir().map_err(AppError::Sandbox)?;
    let spec = sandbox::shared_image(&dir).map_err(AppError::Sandbox)?;
    sandbox::build_image(&spec, force, &CancelToken::default(), &mut |_| {})
        .await
        .map(|_| ())
        .map_err(AppError::Sandbox)
}

#[derive(Serialize)]
pub struct AgentDockerfileInfo {
    /// 専用のDockerfileが登録されているか。
    pub registered: bool,
    /// 登録があれば専用の内容、なければ共通の内容(読み取り専用の表示)。
    pub content: String,
}

/// このエージェントのDockerfile。専用の登録があればそれ、なければ共通の内容を返す。
#[tauri::command]
pub fn get_agent_sandbox_dockerfile(ctx: Ctx) -> Result<AgentDockerfileInfo, AppError> {
    supported(&ctx)?;
    let registered = sandbox::agent_registered(&ctx.dir);
    let content = if registered {
        sandbox::read_agent_dockerfile(&ctx.dir)
    } else {
        sandbox::app_dir().and_then(|d| sandbox::read_dockerfile(&d))
    }
    .map_err(AppError::Sandbox)?;
    Ok(AgentDockerfileInfo { registered, content })
}

/// 共通のDockerfileをコピーして、このエージェント専用として登録する。
#[tauri::command]
pub fn register_agent_sandbox_dockerfile(ctx: Ctx) -> Result<(), AppError> {
    supported(&ctx)?;
    let app = sandbox::app_dir().map_err(AppError::Sandbox)?;
    sandbox::register_agent_dockerfile(&app, &ctx.dir).map_err(AppError::Sandbox)
}

/// 専用のDockerfileを保存する(登録済みのときだけ)。
#[tauri::command]
pub fn set_agent_sandbox_dockerfile(ctx: Ctx, content: String) -> Result<(), AppError> {
    supported(&ctx)?;
    sandbox::write_agent_dockerfile(&ctx.dir, &content).map_err(AppError::Sandbox)
}

/// 専用のDockerfileの登録を解除する。以後は共通のDockerfileを使う。
#[tauri::command]
pub fn unregister_agent_sandbox_dockerfile(ctx: Ctx) -> Result<(), AppError> {
    supported(&ctx)?;
    sandbox::unregister_agent_dockerfile(&ctx.dir).map_err(AppError::Sandbox)
}

/// このエージェントが使うイメージ(専用の登録があれば専用)をビルドする。
#[tauri::command]
pub async fn build_agent_sandbox_image(ctx: Ctx, force: bool) -> Result<(), AppError> {
    supported(&ctx)?;
    let app = sandbox::app_dir().map_err(AppError::Sandbox)?;
    let spec = sandbox::resolve_image(&app, &ctx.id, &ctx.dir).map_err(AppError::Sandbox)?;
    sandbox::build_image(&spec, force, &CancelToken::default(), &mut |_| {})
        .await
        .map(|_| ())
        .map_err(AppError::Sandbox)
}

/// このエージェントのコンテナを作って起動する(イメージはビルド済みであること)。チャットは、起動してから使える。
#[tauri::command]
pub async fn start_agent_sandbox(ctx: Ctx) -> Result<(), AppError> {
    supported(&ctx)?;
    let root = crate::services::batch::knowledge_root(&ctx)?;
    let mounts = sandbox::mounts_for(&ctx, &root).map_err(AppError::Sandbox)?;
    sandbox::start_container(&ctx, &mounts).await.map(|_| ()).map_err(AppError::Sandbox)
}

/// このエージェントのコンテナを止める(消さない)。
#[tauri::command]
pub async fn stop_agent_sandbox(ctx: Ctx) -> Result<(), AppError> {
    supported(&ctx)?;
    let id = ctx.id.clone();
    sandbox::stop_container(&id).await.map_err(AppError::Sandbox)
}

/// `/workspace`にマウントするフォルダ(エージェントごとに決まった場所)。無ければ作る。
#[tauri::command]
pub fn get_sandbox_workspace_dir(ctx: Ctx) -> Result<String, AppError> {
    supported(&ctx)?;
    let dir = sandbox::ensure_workspace_dir(&ctx).map_err(AppError::Sandbox)?;
    Ok(dir.to_string_lossy().into_owned())
}

/// `/workspace`にマウントするフォルダを Finder で開く(無ければ作る)。
#[tauri::command]
pub fn open_sandbox_workspace_dir(ctx: Ctx) -> Result<(), AppError> {
    supported(&ctx)?;
    let dir = sandbox::ensure_workspace_dir(&ctx).map_err(AppError::Sandbox)?;
    let status = std::process::Command::new("/usr/bin/open").arg(&dir).status().map_err(|e| AppError::Io(e.to_string()))?;
    if status.success() { Ok(()) } else { Err(AppError::Io(format!("open failed: {status}"))) }
}

/// コンテナ内の Claude Code の接続先・1回の指示の上限時間(モデルと許可するツールは settings.json で決める)。
#[tauri::command]
pub fn get_sandbox_llm(ctx: Ctx) -> Result<sandbox_agent::LlmConfig, AppError> {
    supported(&ctx)?;
    sandbox_agent::load(&ctx.db)
}

#[tauri::command]
pub fn set_sandbox_llm(ctx: Ctx, config: sandbox_agent::LlmConfig) -> Result<sandbox_agent::LlmConfig, AppError> {
    supported(&ctx)?;
    sandbox_agent::save(&ctx.db, config)
}

/// 貼り付けた Claude Code の settings.json(未設定は空文字)。コンテナ内の Claude Code へ、実行ごとに`--settings`で渡す。
#[tauri::command]
pub fn get_sandbox_claude_settings(ctx: Ctx) -> Result<String, AppError> {
    supported(&ctx)?;
    // 旧設定のモデル・許可するツールは、初めて開いたときに settings.json へ移す
    sandbox_agent::migrate_legacy_llm(&ctx.db)?;
    sandbox_agent::load_settings(&ctx.db)
}

/// 検証して保存する。JSON のオブジェクトでなければ保存しない。空にすると未設定に戻る。
#[tauri::command]
pub fn set_sandbox_claude_settings(ctx: Ctx, content: String) -> Result<(), AppError> {
    supported(&ctx)?;
    sandbox_agent::save_settings(&ctx.db, &content)
}

/// このエージェントのコンテナを作り直す(ボリュームは残す)。次のコマンドで新しく作られる。
#[tauri::command]
pub async fn reset_sandbox(ctx: Ctx) -> Result<(), AppError> {
    supported(&ctx)?;
    let id = ctx.id.clone();
    sandbox::reset_container(&id).await.map_err(AppError::Sandbox)
}

/// Finderでファイルを選択した状態で開く。`kind`は`env`または`dockerfile`。
#[tauri::command]
pub fn reveal_sandbox_file(ctx: Ctx, kind: String) -> Result<(), AppError> {
    let file = match kind.as_str() {
        "env" => {
            supported(&ctx)?;
            sandbox_env::ensure_template(&ctx.dir)
        }
        "dockerfile" => sandbox::app_dir().and_then(|d| sandbox::ensure_template(&d)),
        "agent_dockerfile" => {
            supported(&ctx)?;
            if sandbox::agent_registered(&ctx.dir) {
                Ok(sandbox::agent_dockerfile_path(&ctx.dir))
            } else {
                Err("専用のDockerfileが登録されていません。".to_owned())
            }
        }
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
