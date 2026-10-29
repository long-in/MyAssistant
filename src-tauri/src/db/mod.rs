//! SQLite(timeline.db): 時系列の一次ログとアプリ設定。
use std::{path::Path, sync::Mutex};

use rusqlite::{Connection, params};
use serde::Serialize;

use crate::utils::error::AppError;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS conversation_logs (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    provider TEXT,
    model TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    is_processed BOOLEAN DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_logs_session ON conversation_logs(session_id, created_at);
CREATE INDEX IF NOT EXISTS idx_logs_unprocessed ON conversation_logs(is_processed);
CREATE TABLE IF NOT EXISTS usage_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
";

pub struct Db(Mutex<Connection>);

#[derive(Debug, Clone, Serialize)]
pub struct LogRow {
    pub id: String,
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UsageRow {
    pub provider: String,
    pub model: String,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct PendingLog {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        Self::init(Connection::open(path)?)
    }

    fn init(conn: Connection) -> Result<Self, AppError> {
        conn.execute_batch(SCHEMA)?;
        Ok(Self(Mutex::new(conn)))
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// 保存した行のIDを返す。
    pub fn insert_log(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> Result<String, AppError> {
        let id = uuid::Uuid::new_v4().to_string();
        self.conn().execute(
            "INSERT INTO conversation_logs (id, session_id, role, content, provider, model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, session_id, role, content, provider, model],
        )?;
        Ok(id)
    }

    /// セッションの直近`limit`件を時系列順で返す(user/assistantのみ)。
    /// セッションの全メッセージを古い順に返す(要約の対象を決めるため)。
    pub fn session_messages(&self, session_id: &str) -> Result<Vec<LogRow>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, role, content FROM conversation_logs
             WHERE session_id = ?1 AND role IN ('user', 'assistant')
             ORDER BY created_at ASC, rowid ASC",
        )?;
        let rows = stmt
            .query_map(params![session_id], |r| Ok(LogRow { id: r.get(0)?, role: r.get(1)?, content: r.get(2)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn recent_messages(&self, session_id: &str, limit: u32) -> Result<Vec<LogRow>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, role, content FROM (
                SELECT id, role, content, created_at, rowid FROM conversation_logs
                WHERE session_id = ?1 AND role IN ('user', 'assistant')
                ORDER BY created_at DESC, rowid DESC LIMIT ?2
             ) ORDER BY created_at ASC, rowid ASC",
        )?;
        let rows = stmt
            .query_map(params![session_id, limit], |r| {
                Ok(LogRow { id: r.get(0)?, role: r.get(1)?, content: r.get(2)? })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// 未処理ログをセッション・時系列順で返す。
    pub fn unprocessed(&self, limit: u32) -> Result<Vec<PendingLog>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, created_at FROM conversation_logs
             WHERE is_processed = 0 ORDER BY session_id, created_at, rowid LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit], |r| {
                Ok(PendingLog {
                    id: r.get(0)?,
                    session_id: r.get(1)?,
                    role: r.get(2)?,
                    content: r.get(3)?,
                    created_at: r.get(4)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// 指定セッションに未処理ログが残っているか。
    pub fn has_unprocessed(&self, session_id: &str) -> Result<bool, AppError> {
        Ok(self.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM conversation_logs WHERE session_id = ?1 AND is_processed = 0)",
            [session_id],
            |r| r.get(0),
        )?)
    }

    /// LLM呼び出し1回分のトークン使用量を記録する。
    pub fn record_usage(&self, provider: &str, model: &str, input: u64, output: u64) -> Result<(), AppError> {
        self.conn().execute(
            "INSERT INTO usage_logs (provider, model, input_tokens, output_tokens) VALUES (?1, ?2, ?3, ?4)",
            params![provider, model, input as i64, output as i64],
        )?;
        Ok(())
    }

    /// 当月(ローカル時間)のプロバイダー・モデル別使用量。
    pub fn usage_this_month(&self) -> Result<Vec<UsageRow>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT provider, model, COUNT(*), SUM(input_tokens), SUM(output_tokens) FROM usage_logs
             WHERE strftime('%Y-%m', created_at, 'localtime') = strftime('%Y-%m', 'now', 'localtime')
             GROUP BY provider, model ORDER BY provider, model",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(UsageRow {
                    provider: r.get(0)?,
                    model: r.get(1)?,
                    requests: r.get::<_, i64>(2)? as u64,
                    input_tokens: r.get::<_, i64>(3)? as u64,
                    output_tokens: r.get::<_, i64>(4)? as u64,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn mark_processed(&self, ids: &[String]) -> Result<(), AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        for id in ids {
            tx.execute("UPDATE conversation_logs SET is_processed = 1 WHERE id = ?1", [id])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, AppError> {
        match self.conn().query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            [key],
            |r| r.get(0),
        ) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), AppError> {
        self.conn().execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = CURRENT_TIMESTAMP",
            params![key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Db {
        Db::init(Connection::open_in_memory().unwrap()).unwrap()
    }

    #[test]
    fn logs_roundtrip_in_order_and_unprocessed() {
        let db = mem();
        db.insert_log("s", "user", "hi", None, None).unwrap();
        db.insert_log("s", "assistant", "hello", Some("openai"), Some("gpt-4o")).unwrap();
        db.insert_log("other", "user", "x", None, None).unwrap();
        let rows = db.recent_messages("s", 10).unwrap();
        assert_eq!(rows.iter().map(|r| r.content.as_str()).collect::<Vec<_>>(), ["hi", "hello"]);
        let n: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM conversation_logs WHERE is_processed = 0", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 3);
    }

    #[test]
    fn recent_limit_keeps_latest() {
        let db = mem();
        for i in 0..5 {
            db.insert_log("s", "user", &i.to_string(), None, None).unwrap();
        }
        let rows = db.recent_messages("s", 2).unwrap();
        assert_eq!(rows.iter().map(|r| r.content.as_str()).collect::<Vec<_>>(), ["3", "4"]);
    }

    #[test]
    fn unprocessed_and_mark() {
        let db = mem();
        let a = db.insert_log("s", "user", "a", None, None).unwrap();
        let b = db.insert_log("s", "assistant", "b", None, None).unwrap();
        assert_eq!(db.unprocessed(10).unwrap().len(), 2);
        db.mark_processed(&[a]).unwrap();
        let rest = db.unprocessed(10).unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].id, b);
    }

    #[test]
    fn settings_upsert() {
        let db = mem();
        assert_eq!(db.get_setting("k").unwrap(), None);
        db.set_setting("k", "1").unwrap();
        db.set_setting("k", "2").unwrap();
        assert_eq!(db.get_setting("k").unwrap().as_deref(), Some("2"));
    }
}
