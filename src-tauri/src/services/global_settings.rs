//! 全体設定。すべてのエージェント(パーソナルを含む)で共通の設定を1か所に持つ。
//!
//! - 保存先はアプリデータの`work/global_settings.db`(専用のSQLite。キーと文字列値の対応)。エージェントを削除しても残る。
//! - 項目は`settings_registry`で保存先が全体設定(`Scope::Global`)のキー。エージェントごとのキーは書き込みで拒否する。
//! - 以前の`work/global_settings.json`があれば、開くときに一度だけ取り込んで削除する(`import_json`)。
//!
//! 項目の読み書きは`SettingsStore`を通すので、DBの設定と同じ形で扱える。
use std::{collections::BTreeMap, path::Path, sync::Mutex};

use rusqlite::{Connection, OptionalExtension, params};

use super::settings_registry;
use crate::{db::Db, utils::error::AppError};

/// 全体設定のDBのファイル名(`work/`の下)。
pub const DB_FILE: &str = "global_settings.db";
/// 以前の保存先(JSON)。あれば開くときに取り込む。
pub const LEGACY_JSON_FILE: &str = "global_settings.json";

/// 同じファイルをほかの接続が書いているとき、待つ時間(ミリ秒)。
const BUSY_TIMEOUT_MS: u64 = 5_000;

/// 表示言語(`ja` / `en`)。画面が最初に決めた値を保存する。未保存ならnull扱い。
pub const LANGUAGE_KEY: &str = "language";
/// アプリが背面のときのOS通知を使うか(`0` / `1`)。未保存なら使う。
pub const OS_NOTIFICATIONS_KEY: &str = "os_notifications";

/// 文字列の設定の読み書き。DBの設定と全体設定を、同じ形で扱うために使う。
pub trait SettingsStore {
    fn get_setting(&self, key: &str) -> Result<Option<String>, AppError>;
    fn set_setting(&self, key: &str, value: &str) -> Result<(), AppError>;
}

impl SettingsStore for Db {
    fn get_setting(&self, key: &str) -> Result<Option<String>, AppError> {
        Db::get_setting(self, key)
    }

    fn set_setting(&self, key: &str, value: &str) -> Result<(), AppError> {
        Db::set_setting(self, key, value)
    }
}

/// 全体設定のDB。読み書きは1つの接続で直列にし、複数項目の書き込みは1つのトランザクションにする。
#[derive(Debug)]
pub struct GlobalStore {
    conn: Mutex<Connection>,
}

impl GlobalStore {
    /// `root`(アプリデータ)の全体設定を開く(無ければ作る)。以前のJSONがあれば取り込んで削除する。
    pub fn open(root: &Path) -> Result<Self, AppError> {
        let dir = root.join(super::workspace::DIR_NAME);
        std::fs::create_dir_all(&dir).map_err(|e| AppError::Io(e.to_string()))?;
        let conn = Connection::open(dir.join(DB_FILE))?;
        conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS global_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );",
        )?;
        let store = Self { conn: Mutex::new(conn) };
        store.import_json(&dir.join(LEGACY_JSON_FILE));
        Ok(store)
    }

    /// 接続。ロックが毒されても(パニック後)中身を使い続ける。
    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// 以前のJSONを取り込んで削除する。すでにDBにある項目は上書きしない。読めない・書き込めないときはファイルを残す。
    fn import_json(&self, path: &Path) {
        let Ok(text) = std::fs::read_to_string(path) else { return };
        let map: BTreeMap<String, String> = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("global settings file is invalid, kept as is: {e}");
                return;
            }
        };
        let result = (|| -> Result<(), AppError> {
            let mut conn = self.conn();
            let tx = conn.transaction()?;
            for (key, value) in map.iter().filter(|(k, _)| settings_registry::check_global_write(k).is_ok()) {
                tx.execute("INSERT OR IGNORE INTO global_settings (key, value) VALUES (?1, ?2)", params![key, value])?;
            }
            tx.commit()?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                if let Err(e) = std::fs::remove_file(path) {
                    log::warn!("global settings file imported but not removed: {e}");
                }
            }
            Err(e) => log::warn!("global settings import failed, file kept: {e}"),
        }
    }

    /// 複数の項目をまとめて書く(途中で失敗したら、どれも書かれない)。
    pub fn set_settings(&self, items: &[(&str, &str)]) -> Result<(), AppError> {
        for (k, _) in items {
            settings_registry::check_global_write(k)?;
        }
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        for (key, value) in items {
            tx.execute(
                "INSERT INTO global_settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![key, value],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

impl SettingsStore for GlobalStore {
    fn get_setting(&self, key: &str) -> Result<Option<String>, AppError> {
        Ok(self.conn().query_row("SELECT value FROM global_settings WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    fn set_setting(&self, key: &str, value: &str) -> Result<(), AppError> {
        self.set_settings(&[(key, value)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_and_survives_reopen() {
        let d = tempfile::tempdir().unwrap();
        let g = GlobalStore::open(d.path()).unwrap();
        assert_eq!(g.get_setting("a").unwrap(), None);
        g.set_setting("a", "1").unwrap();
        g.set_settings(&[("b", "2"), ("a", "3")]).unwrap();
        drop(g);
        let g = GlobalStore::open(d.path()).unwrap();
        assert_eq!(g.get_setting("a").unwrap().as_deref(), Some("3"));
        assert_eq!(g.get_setting("b").unwrap().as_deref(), Some("2"));
    }

    #[test]
    fn agent_keys_are_rejected() {
        let d = tempfile::tempdir().unwrap();
        let g = GlobalStore::open(d.path()).unwrap();
        assert!(g.set_settings(&[("base_instructions", "x"), ("routing", "{}")]).is_err());
        assert_eq!(g.get_setting("base_instructions").unwrap(), None);
    }

    #[test]
    fn json_file_is_imported_once_and_removed() {
        let d = tempfile::tempdir().unwrap();
        let work = d.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let file = work.join(LEGACY_JSON_FILE);
        std::fs::write(&file, r#"{"language":"en","os_notifications":"0","routing":"{}"}"#).unwrap();
        let g = GlobalStore::open(d.path()).unwrap();
        assert_eq!(g.get_setting("language").unwrap().as_deref(), Some("en"));
        assert_eq!(g.get_setting("os_notifications").unwrap().as_deref(), Some("0"));
        // エージェントごとのキーは取り込まない
        assert_eq!(g.get_setting("routing").unwrap(), None);
        assert!(!file.exists());
        // すでにDBにある値は、残っていたJSONで上書きしない
        g.set_setting("language", "ja").unwrap();
        std::fs::write(&file, r#"{"language":"en"}"#).unwrap();
        drop(g);
        let g = GlobalStore::open(d.path()).unwrap();
        assert_eq!(g.get_setting("language").unwrap().as_deref(), Some("ja"));
    }

    #[test]
    fn broken_json_file_is_kept() {
        let d = tempfile::tempdir().unwrap();
        let work = d.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let file = work.join(LEGACY_JSON_FILE);
        std::fs::write(&file, "{broken").unwrap();
        let g = GlobalStore::open(d.path()).unwrap();
        assert_eq!(g.get_setting("language").unwrap(), None);
        assert!(file.exists());
    }

    #[test]
    fn agent_db_values_are_ignored_and_all_agents_share_one_store() {
        use crate::services::{workspace::WorkspaceCtx, wake};
        let d = tempfile::tempdir().unwrap();
        let global = std::sync::Arc::new(GlobalStore::open(d.path()).unwrap());
        let a = WorkspaceCtx::open_shared("a", &d.path().join("a"), Default::default(), global.clone()).unwrap();
        let b = WorkspaceCtx::open_shared("b", &d.path().join("b"), Default::default(), global).unwrap();
        // エージェントのDBに古い値があっても、全体設定は読まない
        a.db.set_legacy_setting(wake::WORDS_KEY, r#"["旧"]"#);
        a.db.set_legacy_setting(wake::ENABLED_KEY, "1");
        assert!(wake::load(&*a.global).unwrap().words.is_empty());
        // どのエージェントから保存しても、全エージェントに同じ値が見える
        wake::save(&*b.global, &wake::WakeConfig { enabled: true, words: vec!["やあ".into()] }).unwrap();
        assert_eq!(wake::load(&*a.global).unwrap().words, vec!["やあ".to_owned()]);
    }
}
