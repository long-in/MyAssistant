//! キー・値の設定(`app_settings`)。値は文字列で、構造のあるものはJSON。
//!
//! 全体設定のキーは書き込みで拒否する(`settings_registry::check_db_write`)。全体設定は`workspaces/global_settings.json`にある。
use rusqlite::{OptionalExtension, Transaction, params};

use super::{Db, schema::NOW_SQL};
use crate::{services::settings_registry, utils::error::AppError};

fn upsert(tx: &Transaction<'_>, key: &str, value: &str) -> Result<(), AppError> {
    settings_registry::check_db_write(key)?;
    tx.execute(
        &format!(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = {NOW_SQL}"
        ),
        params![key, value],
    )?;
    Ok(())
}

impl Db {
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, AppError> {
        Ok(self.conn().query_row("SELECT value FROM app_settings WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), AppError> {
        self.set_settings(&[(key, value)])
    }

    /// 複数の設定を1つのトランザクションで書く。途中で失敗したら、どれも書かれない。
    pub fn set_settings(&self, items: &[(&str, &str)]) -> Result<(), AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        for (key, value) in items {
            upsert(&tx, key, value)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 指定したキーの設定を消す(無ければ何もしない)。
    pub fn delete_settings<'a>(&self, keys: impl IntoIterator<Item = &'a str>) -> Result<usize, AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let mut n = 0;
        for key in keys {
            n += tx.execute("DELETE FROM app_settings WHERE key = ?1", [key])?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// テスト用: 確認を通さずに書く(全体設定へ移す前の、古いDBを作るため)。
    #[cfg(test)]
    pub fn set_legacy_setting(&self, key: &str, value: &str) {
        self.conn()
            .execute("INSERT OR REPLACE INTO app_settings (key, value) VALUES (?1, ?2)", params![key, value])
            .unwrap();
    }

    /// テスト用: 設定の書き込みだけを失敗させる(読み取りはできる)。
    #[cfg(test)]
    pub fn break_settings_for_test(&self) {
        self.conn()
            .execute_batch("CREATE TRIGGER fail_writes BEFORE INSERT ON app_settings BEGIN SELECT RAISE(ABORT, 'broken'); END;")
            .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::super::Role;
    use super::*;

    #[test]
    fn settings_upsert() {
        let db = Db::open_in_memory(Role::Agent);
        assert_eq!(db.get_setting("k").unwrap(), None);
        db.set_setting("k", "1").unwrap();
        db.set_setting("k", "2").unwrap();
        assert_eq!(db.get_setting("k").unwrap().as_deref(), Some("2"));
    }

    #[test]
    fn global_keys_are_rejected_and_nothing_is_written() {
        let db = Db::open_in_memory(Role::Agent);
        let err = db.set_settings(&[("routing", "{}"), (crate::commands::chat::BASE_INSTRUCTIONS_KEY, "x")]);
        assert!(err.is_err());
        assert_eq!(db.get_setting("routing").unwrap(), None);
    }

    #[test]
    fn delete_settings_removes_only_given_keys() {
        let db = Db::open_in_memory(Role::Agent);
        db.set_settings(&[("a", "1"), ("b", "2")]).unwrap();
        assert_eq!(db.delete_settings(["a", "missing"]).unwrap(), 1);
        assert_eq!(db.get_setting("a").unwrap(), None);
        assert_eq!(db.get_setting("b").unwrap().as_deref(), Some("2"));
    }
}
