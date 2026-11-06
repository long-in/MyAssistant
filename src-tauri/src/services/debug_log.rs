//! デバッグモード。有効な間、各APIのリクエスト・レスポンスと音声の文字起こしを
//! 出力先(既定はナレッジ配下の`debug-logs`)へ日付ごとのJSON Lines(`YYYY-MM-DD.jsonl`)で追記する。
//! APIキーはヘッダーにのみ載るため、ここへ渡す本文には含まれない。
use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use super::batch;
use crate::{db::Db, utils::{error::AppError, path}};

pub const ENABLED_KEY: &str = "debug_mode";
/// 出力先を利用者が指定したときの絶対パス。空・未設定ならナレッジ直下の`DIR_NAME`。
pub const DIR_KEY: &str = "debug_log_dir";
/// ナレッジ直下に作る既定のログ用ディレクトリ名。
pub const DIR_NAME: &str = "debug-logs";

/// 有効なときだけ出力先ディレクトリが入る。
static LOG_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// デバッグモードの設定値。未設定は無効。
pub fn is_enabled(db: &Db) -> Result<bool, AppError> {
    Ok(db.get_setting(ENABLED_KEY)?.as_deref() == Some("1"))
}

/// 指定された出力先。未指定なら`None`(ナレッジ配下の既定を使う)。
fn custom_dir(db: &Db) -> Result<Option<PathBuf>, AppError> {
    Ok(db.get_setting(DIR_KEY)?.filter(|p| !p.is_empty()).map(PathBuf::from))
}

/// ログの出力先(作成はしない)。
pub fn log_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    match custom_dir(&app.state::<Db>())? {
        Some(dir) => Ok(dir),
        None => Ok(batch::knowledge_root(app)?.join(DIR_NAME)),
    }
}

/// 出力先として使える絶対パスのディレクトリを用意し、書き込めることを確かめて正規化して返す。
pub fn prepare_dir(dir: &Path) -> Result<PathBuf, AppError> {
    if !dir.is_absolute() {
        return Err(AppError::PathNotAllowed);
    }
    std::fs::create_dir_all(dir).map_err(|e| AppError::Io(e.to_string()))?;
    let dir = dir.canonicalize().map_err(|e| AppError::Io(e.to_string()))?;
    let probe = dir.join(".write_test");
    std::fs::write(&probe, b"").map_err(|e| AppError::Io(e.to_string()))?;
    let _ = std::fs::remove_file(probe);
    Ok(dir)
}

/// 設定とナレッジの保存先から出力先を決め直す。ナレッジの保存先や設定を変えたときに呼ぶ。
pub fn refresh(app: &AppHandle) -> Result<(), AppError> {
    let db = app.state::<Db>();
    let dir = if !is_enabled(&db)? {
        None
    } else if let Some(custom) = custom_dir(&db)? {
        Some(prepare_dir(&custom)?)
    } else {
        Some(path::ensure_dir(&batch::knowledge_root(app)?, &[DIR_NAME.to_owned()])?)
    };
    *LOG_DIR.lock().unwrap_or_else(|e| e.into_inner()) = dir;
    Ok(())
}

pub fn enabled() -> bool {
    LOG_DIR.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

/// 1件を追記する。無効なときは何もせず、書き込みに失敗しても本来の処理は止めない。
pub fn record(kind: &str, data: Value) {
    let Some(dir) = LOG_DIR.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
        return;
    };
    let now = chrono::Local::now();
    let line = json!({ "time": now.to_rfc3339(), "kind": kind, "data": data });
    if let Err(e) = append(&dir, &now.format("%Y-%m-%d").to_string(), &line) {
        log::warn!("debug log failed: {e}");
    }
}

fn append(dir: &Path, day: &str, line: &Value) -> std::io::Result<()> {
    let mut f = OpenOptions::new().create(true).append(true).open(dir.join(format!("{day}.jsonl")))?;
    writeln!(f, "{line}")
}

/// APIの1往復を記録する。`response`が本文として読めなかったときは文字列のまま渡す。
pub fn record_api(api: &str, url: &str, request: &Value, status: u16, response: &Value) {
    if enabled() {
        record("api", json!({ "api": api, "url": url, "request": request, "status": status, "response": response }));
    }
}

/// 応答本文をJSONとして読む。読めなければ文字列として扱う。
pub fn parse_body(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned()))
}

/// 音声データなど長大なbase64の値を、サイズだけ残して置き換える。
pub fn redact_at(v: &mut Value, pointer: &str) {
    if let Some(slot) = v.pointer_mut(pointer)
        && let Some(s) = slot.as_str()
    {
        *slot = Value::String(format!("<base64 {} chars>", s.len()));
    }
}

/// ログファイル1件の概要。
#[derive(Debug, Serialize, PartialEq)]
pub struct LogFile {
    /// ファイル名(`YYYY-MM-DD.jsonl`)。
    pub name: String,
    pub bytes: u64,
}

/// 画面で読み込むログの上限。超えた分は古い側を切り捨てる。
const MAX_READ_BYTES: usize = 5 * 1024 * 1024;

/// ログファイルとして扱う名前か。
fn is_log_name(name: &str) -> bool {
    name.strip_suffix(".jsonl").is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit() || c == '-'))
}

/// ログのファイル一覧(新しい日付が先頭)。ディレクトリがなければ空。
pub fn list(dir: &Path) -> Result<Vec<LogFile>, AppError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| AppError::Io(e.to_string()))? {
        let entry = entry.map_err(|e| AppError::Io(e.to_string()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let meta = entry.metadata().map_err(|e| AppError::Io(e.to_string()))?;
        if meta.is_file() && is_log_name(&name) {
            out.push(LogFile { name, bytes: meta.len() });
        }
    }
    out.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(out)
}

/// ログ1件の本文。大きい場合は新しい側の`MAX_READ_BYTES`だけを、行の区切りから返す。
pub fn read(dir: &Path, name: &str) -> Result<String, AppError> {
    if !is_log_name(name) {
        return Err(AppError::PathNotAllowed);
    }
    let bytes = std::fs::read(path::resolve_existing(dir, name)?).map_err(|e| AppError::Io(e.to_string()))?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if text.len() <= MAX_READ_BYTES {
        return Ok(text);
    }
    let mut start = text.len() - MAX_READ_BYTES;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let tail = &text[start..];
    Ok(tail.split_once('\n').map_or(tail, |(_, rest)| rest).to_owned())
}

/// `name`のログを削除する。`None`なら全ログを削除する。削除したファイル数を返す。
pub fn delete(dir: &Path, name: Option<&str>) -> Result<usize, AppError> {
    let names: Vec<String> = match name {
        Some(n) if is_log_name(n) => vec![n.to_owned()],
        Some(_) => return Err(AppError::PathNotAllowed),
        None => list(dir)?.into_iter().map(|f| f.name).collect(),
    };
    let mut removed = 0;
    for n in names {
        std::fs::remove_file(path::resolve_existing(dir, &n)?).map_err(|e| AppError::Io(e.to_string()))?;
        removed += 1;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_only_the_target() {
        let mut v = json!({ "audio": { "content": "QUJDRA==" }, "config": { "a": 1 } });
        redact_at(&mut v, "/audio/content");
        redact_at(&mut v, "/missing");
        assert_eq!(v["audio"]["content"], "<base64 8 chars>");
        assert_eq!(v["config"]["a"], 1);
    }

    #[test]
    fn appends_json_lines() {
        let dir = tempfile::tempdir().unwrap();
        append(dir.path(), "2026-01-01", &json!({ "a": 1 })).unwrap();
        append(dir.path(), "2026-01-01", &json!({ "a": 2 })).unwrap();
        let text = std::fs::read_to_string(dir.path().join("2026-01-01.jsonl")).unwrap();
        assert_eq!(text.lines().count(), 2);
    }

    #[test]
    fn lists_reads_and_deletes_only_log_files() {
        let dir = tempfile::tempdir().unwrap();
        append(dir.path(), "2026-01-01", &json!({ "a": 1 })).unwrap();
        append(dir.path(), "2026-01-02", &json!({ "a": 2 })).unwrap();
        std::fs::write(dir.path().join("note.md"), "x").unwrap();
        let names: Vec<_> = list(dir.path()).unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["2026-01-02.jsonl", "2026-01-01.jsonl"]);
        assert!(read(dir.path(), "2026-01-01.jsonl").unwrap().contains("\"a\":1"));
        for bad in ["note.md", "../x.jsonl", ".jsonl"] {
            assert!(matches!(read(dir.path(), bad), Err(AppError::PathNotAllowed)), "{bad}");
            assert!(matches!(delete(dir.path(), Some(bad)), Err(AppError::PathNotAllowed)), "{bad}");
        }
        assert_eq!(delete(dir.path(), Some("2026-01-01.jsonl")).unwrap(), 1);
        assert_eq!(delete(dir.path(), None).unwrap(), 1);
        assert!(dir.path().join("note.md").exists());
        assert!(list(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn non_json_body_is_kept_as_string() {
        assert_eq!(parse_body("oops"), json!("oops"));
        assert_eq!(parse_body(r#"{"a":1}"#), json!({ "a": 1 }));
    }
}
