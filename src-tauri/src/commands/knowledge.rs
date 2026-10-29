use tauri::{AppHandle, State};

use crate::{
    db::Db,
    services::{
        batch::{self, BatchReport},
        debug_log,
        knowledge::{self, KnowledgeFile, TransferReport},
        llm_router::Route,
    },
    utils::error::AppError,
};

/// ナレッジの保存先。Obsidianのvaultとして開く、外部ツールから直接読み書きするために表示する。
#[tauri::command]
pub fn knowledge_dir(app: AppHandle) -> Result<String, AppError> {
    let root = batch::knowledge_root(&app)?;
    std::fs::create_dir_all(&root).map_err(|e| AppError::Io(e.to_string()))?;
    Ok(root.to_string_lossy().into_owned())
}

/// ナレッジの保存先を変更する。`None`で既定の場所に戻す。既存ファイルは移動しない。
#[tauri::command]
pub fn set_knowledge_dir(app: AppHandle, db: State<'_, Db>, path: Option<String>) -> Result<String, AppError> {
    let path = path.map(|p| p.trim().to_owned()).filter(|p| !p.is_empty());
    let root = match &path {
        Some(p) => {
            let p = std::path::PathBuf::from(p);
            if !p.is_absolute() {
                return Err(AppError::PathNotAllowed);
            }
            p
        }
        None => batch::default_knowledge_root(&app)?,
    };
    std::fs::create_dir_all(&root).map_err(|e| AppError::Io(e.to_string()))?;
    let root = root.canonicalize().map_err(|e| AppError::Io(e.to_string()))?;
    // 書き込めない場所を保存しないよう、実際に書き込めるか確認する
    check_writable(&root)?;
    let resolved = root.to_string_lossy().into_owned();
    db.set_setting(batch::KNOWLEDGE_DIR_KEY, if path.is_some() { &resolved } else { "" })?;
    debug_log::refresh(&app)?;
    Ok(resolved)
}

fn check_writable(dir: &std::path::Path) -> Result<(), AppError> {
    let probe = dir.join(".write_test");
    std::fs::write(&probe, b"").map_err(|e| AppError::Io(e.to_string()))?;
    let _ = std::fs::remove_file(probe);
    Ok(())
}

#[tauri::command]
pub fn list_knowledge(app: AppHandle) -> Result<Vec<KnowledgeFile>, AppError> {
    knowledge::list(&batch::knowledge_root(&app)?)
}

#[tauri::command]
pub fn read_knowledge(app: AppHandle, path: String) -> Result<String, AppError> {
    knowledge::read(&batch::knowledge_root(&app)?, &path)
}

/// 手動トリガー。実行中・対象なし・キー未登録の場合は`None`。
#[tauri::command]
pub async fn run_batch(app: AppHandle) -> Result<Option<BatchReport>, AppError> {
    batch::run_guarded(&app).await
}

/// 整理バッチの実行間隔(分)。
#[tauri::command]
pub fn get_batch_interval(db: State<'_, Db>) -> Result<u32, AppError> {
    batch::interval_minutes(&db)
}

/// 次の待ち時間から反映される。範囲外は拒否する。
#[tauri::command]
pub fn set_batch_interval(db: State<'_, Db>, minutes: u32) -> Result<(), AppError> {
    if !batch::INTERVAL_RANGE.contains(&minutes) {
        return Err(AppError::Db(format!("interval out of range: {minutes}")));
    }
    db.set_setting(batch::BATCH_INTERVAL_KEY, &minutes.to_string())
}

/// 整理バッチのモデル指定。未設定・使用不可なら既定(Claudeが使えればSonnet)を返す。
#[tauri::command]
pub fn load_batch_route(db: State<'_, Db>) -> Result<Option<Route>, AppError> {
    batch::effective_route(&db, &batch::available_providers())
}

#[tauri::command]
pub fn save_batch_route(db: State<'_, Db>, route: Route) -> Result<(), AppError> {
    db.set_setting(batch::BATCH_ROUTE_KEY, &serde_json::to_string(&route).map_err(|e| AppError::Db(e.to_string()))?)
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
pub fn export_knowledge(app: AppHandle, dest: String) -> Result<TransferReport, AppError> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    knowledge::export(&batch::knowledge_root(&app)?, &external_dir(&dest)?, &stamp)
}

/// `src`フォルダのMarkdownをナレッジへ取り込む(既存は上書きしない)。
#[tauri::command]
pub fn import_knowledge(app: AppHandle, src: String) -> Result<TransferReport, AppError> {
    let root = batch::knowledge_root(&app)?;
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
pub fn reset_knowledge(app: AppHandle) -> Result<TransferReport, AppError> {
    knowledge::reset(&batch::knowledge_root(&app)?)
}
