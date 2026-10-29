use std::path::PathBuf;

use tauri::{AppHandle, State};

use crate::{
    db::Db,
    services::{
        batch, command_policy, debug_log,
        settings_transfer::{self, MAX_FILE_BYTES, Report},
    },
    utils::error::AppError,
};

/// ダイアログで選ばれた、拡張子`.json`の絶対パスのみ受け付ける。
fn json_path(path: &str) -> Result<PathBuf, AppError> {
    let p = PathBuf::from(path.trim());
    if !p.is_absolute() || p.extension().and_then(|e| e.to_str()) != Some("json") {
        return Err(AppError::PathNotAllowed);
    }
    Ok(p)
}

/// 設定を`dest`のJSONファイルへ書き出す。APIキーは含まない。
#[tauri::command]
pub fn export_settings(app: AppHandle, db: State<'_, Db>, dest: String) -> Result<Report, AppError> {
    let dest = json_path(&dest)?;
    let policy = command_policy::load(&batch::knowledge_root(&app)?);
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let file = settings_transfer::collect(&db, policy, now)?;
    let json = serde_json::to_string_pretty(&file).map_err(|e| AppError::Io(e.to_string()))?;
    std::fs::write(&dest, json).map_err(|e| AppError::Io(e.to_string()))?;
    Ok(Report { applied: file.item_count(), skipped: 0 })
}

/// 書き出した設定ファイルを読み込んで反映する。不正な項目があれば何も変更しない。
#[tauri::command]
pub fn import_settings(app: AppHandle, db: State<'_, Db>, src: String) -> Result<Report, AppError> {
    let src = json_path(&src)?;
    let meta = std::fs::metadata(&src).map_err(|e| AppError::Io(e.to_string()))?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return Err(AppError::InvalidSettings("ファイルが大きすぎます".into()));
    }
    let text = std::fs::read_to_string(&src).map_err(|e| AppError::Io(e.to_string()))?;
    let (report, policy) = settings_transfer::apply(&db, &settings_transfer::parse(&text)?)?;
    if let Some(policy) = policy {
        command_policy::save(&batch::knowledge_root(&app)?, &policy).map_err(AppError::Io)?;
    }
    debug_log::refresh(&app)?;
    Ok(report)
}
