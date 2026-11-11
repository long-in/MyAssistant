use tauri::State;

use crate::{
    services::{
        batch::{self, BatchReport},
        debug_log,
        knowledge::{self, KnowledgeFile, TransferReport},
        llm_router::{self, Route},
        memory::{self, Forgot, MemoryView},
        workspace::{Ctx, Workspaces},
    },
    utils::error::AppError,
};

/// ナレッジの保存先。Obsidianのvaultとして開く、外部ツールから直接読み書きするために表示する。
#[tauri::command]
pub fn knowledge_dir(ctx: Ctx) -> Result<String, AppError> {
    let root = batch::knowledge_root(&ctx)?;
    std::fs::create_dir_all(&root).map_err(|e| AppError::Io(e.to_string()))?;
    Ok(root.to_string_lossy().into_owned())
}

/// ナレッジの保存先を Finder で開く(無ければ作る)。
#[tauri::command]
pub fn open_knowledge_dir(ctx: Ctx) -> Result<(), AppError> {
    let root = batch::knowledge_root(&ctx)?;
    std::fs::create_dir_all(&root).map_err(|e| AppError::Io(e.to_string()))?;
    let status = std::process::Command::new("/usr/bin/open").arg(&root).status().map_err(|e| AppError::Io(e.to_string()))?;
    if status.success() { Ok(()) } else { Err(AppError::Io(format!("open failed: {status}"))) }
}

/// ナレッジの保存先を変更する。`None`で既定の場所に戻す。既存ファイルは移動しない。
#[tauri::command]
pub fn set_knowledge_dir(ws: State<'_, Workspaces>, ctx: Ctx, path: Option<String>) -> Result<String, AppError> {
    let path = path.map(|p| p.trim().to_owned()).filter(|p| !p.is_empty());
    let root = match &path {
        Some(p) => {
            let p = std::path::PathBuf::from(p);
            if !p.is_absolute() {
                return Err(AppError::PathNotAllowed);
            }
            p
        }
        None => batch::default_knowledge_root(&ctx),
    };
    // 他のワークスペースのナレッジ・デバッグログの場所やアプリのデータと重なる場所は使えない。
    // 確認から保存までを他の保存先の変更と交差させないよう、まとめて行う(フォルダを作る前に確かめる)
    let resolved = ws.set_location(&ctx.id, &root, || {
        std::fs::create_dir_all(&root).map_err(|e| AppError::Io(e.to_string()))?;
        let root = root.canonicalize().map_err(|e| AppError::Io(e.to_string()))?;
        // 書き込めない場所を保存しないよう、実際に書き込めるか確認する
        check_writable(&root)?;
        let resolved = root.to_string_lossy().into_owned();
        ctx.db.set_setting(batch::KNOWLEDGE_DIR_KEY, if path.is_some() { &resolved } else { "" })?;
        Ok(resolved)
    })?;
    debug_log::refresh(&ctx)?;
    Ok(resolved)
}

fn check_writable(dir: &std::path::Path) -> Result<(), AppError> {
    let probe = dir.join(".write_test");
    std::fs::write(&probe, b"").map_err(|e| AppError::Io(e.to_string()))?;
    let _ = std::fs::remove_file(probe);
    Ok(())
}

#[tauri::command]
pub fn list_knowledge(ctx: Ctx) -> Result<Vec<KnowledgeFile>, AppError> {
    knowledge::list(&batch::knowledge_root(&ctx)?)
}

/// Markdown以外のファイル一覧(閲覧専用)。
#[tauri::command]
pub fn list_other_knowledge(ctx: Ctx) -> Result<Vec<KnowledgeFile>, AppError> {
    knowledge::list_other(&batch::knowledge_root(&ctx)?)
}

/// Markdown以外のファイルの本文(閲覧専用)。
#[tauri::command]
pub fn read_other_knowledge(ctx: Ctx, path: String) -> Result<String, AppError> {
    knowledge::read_other(&batch::knowledge_root(&ctx)?, &path)
}

#[tauri::command]
pub fn read_knowledge(ctx: Ctx, path: String) -> Result<String, AppError> {
    knowledge::read(&batch::knowledge_root(&ctx)?, &path)
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[tauri::command]
pub fn list_knowledge_dirs(ctx: Ctx) -> Result<Vec<String>, AppError> {
    knowledge::list_dirs(&batch::knowledge_root(&ctx)?)
}

/// 既存ノートの本文を保存する。
#[tauri::command]
pub fn write_knowledge(ctx: Ctx, path: String, content: String) -> Result<(), AppError> {
    knowledge::write(&batch::knowledge_root(&ctx)?, &path, &content)
}

#[tauri::command]
pub fn create_knowledge_file(ctx: Ctx, path: String) -> Result<String, AppError> {
    knowledge::create_file(&batch::knowledge_root(&ctx)?, &path, &now())
}

#[tauri::command]
pub fn create_knowledge_dir(ctx: Ctx, path: String) -> Result<(), AppError> {
    knowledge::create_dir(&batch::knowledge_root(&ctx)?, &path)
}

/// ファイル・フォルダの名前変更と移動。
#[tauri::command]
pub fn rename_knowledge(ctx: Ctx, from: String, to: String) -> Result<(), AppError> {
    knowledge::rename(&batch::knowledge_root(&ctx)?, &from, &to)
}

/// ファイル、またはフォルダ(中身ごと)を削除する。確認は呼び出し側で行うこと。
#[tauri::command]
pub fn delete_knowledge(ctx: Ctx, path: String) -> Result<(), AppError> {
    knowledge::delete(&batch::knowledge_root(&ctx)?, &path)
}

/// 手動トリガー。実行中・対象なし・キー未登録の場合は`None`。
#[tauri::command]
pub async fn run_batch(ctx: Ctx) -> Result<Option<BatchReport>, AppError> {
    batch::run_guarded(&ctx).await
}

/// 整理バッチのモデル指定。未設定・使用不可なら既定(Claudeが使えればSonnet)を返す。
#[tauri::command]
pub fn load_batch_route(ctx: Ctx) -> Result<Option<Route>, AppError> {
    // Bedrock 方式(ワークは常に)は、コンテナ内の Claude Code で整理する。選べるのはそのモデルだけ
    if llm_router::uses_container(&ctx) {
        let model = batch::saved_route(&ctx)?.filter(llm_router::is_sandbox).map(|r| r.model).unwrap_or_default();
        return Ok(Some(llm_router::sandbox_route(&model)));
    }
    batch::effective_route(&ctx, &batch::available_providers(&ctx.secrets))
}

#[tauri::command]
pub fn save_batch_route(ctx: Ctx, route: Route) -> Result<(), AppError> {
    if llm_router::is_sandbox(&route) {
        crate::services::sandbox_agent::check_route_model(&ctx.db, &*ctx.global, &route.model)?;
    } else if llm_router::uses_container(&ctx) {
        return Err(AppError::Sandbox("Bedrock 方式の整理は、コンテナ内の Claude Code だけで行います。".into()));
    }
    ctx.db.set_setting(batch::BATCH_ROUTE_KEY, &serde_json::to_string(&route).map_err(|e| AppError::Db(e.to_string()))?)
}

/// 記憶(覚えておくこと)。
#[tauri::command]
pub fn memory_view(ctx: Ctx) -> Result<MemoryView, AppError> {
    Ok(memory::view(&batch::knowledge_root(&ctx)?))
}

/// 覚えていることを外す。外したら`true`。画面からは一覧の文面そのものを渡す。
#[tauri::command]
pub fn memory_forget(ctx: Ctx, text: String) -> Result<bool, AppError> {
    Ok(matches!(memory::forget(&batch::knowledge_root(&ctx)?, &text, &now())?, Forgot::Removed(_)))
}

/// 絶対パスのフォルダのみ受け付ける(ダイアログで選ばれたフォルダを想定)。
fn external_dir(path: &str) -> Result<std::path::PathBuf, AppError> {
    let p = std::path::PathBuf::from(path.trim());
    if !p.is_absolute() || !p.is_dir() {
        return Err(AppError::PathNotAllowed);
    }
    p.canonicalize().map_err(|e| AppError::Io(e.to_string()))
}

/// ナレッジ全体を`dest`直下の新しいフォルダへ書き出す。
#[tauri::command]
pub fn export_knowledge(ctx: Ctx, dest: String) -> Result<TransferReport, AppError> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    knowledge::export(&batch::knowledge_root(&ctx)?, &external_dir(&dest)?, &stamp)
}

/// `src`フォルダのMarkdownをナレッジへ取り込む(既存は上書きしない)。
#[tauri::command]
pub fn import_knowledge(ctx: Ctx, src: String) -> Result<TransferReport, AppError> {
    let root = batch::knowledge_root(&ctx)?;
    let src = external_dir(&src)?;
    std::fs::create_dir_all(&root).map_err(|e| AppError::Io(e.to_string()))?;
    // 保存先自身を取り込むと自己コピーになるため拒否する
    if src == root.canonicalize().map_err(|e| AppError::Io(e.to_string()))? {
        return Err(AppError::PathNotAllowed);
    }
    knowledge::import(&root, &src)
}

/// ナレッジのMarkdownをすべて削除する。元に戻せないため、呼び出し側で確認と事前エクスポートを促すこと。
#[tauri::command]
pub fn reset_knowledge(ctx: Ctx) -> Result<TransferReport, AppError> {
    knowledge::reset(&batch::knowledge_root(&ctx)?)
}
