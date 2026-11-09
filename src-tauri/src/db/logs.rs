//! 会話の一次ログ(`conversation_logs`)。整理バッチがナレッジへ反映したかを`is_processed`で持つ。
use rusqlite::params;
use serde::Serialize;

use super::Db;
use crate::utils::error::AppError;

#[derive(Debug, Clone, Serialize)]
pub struct LogRow {
    pub id: String,
    pub role: String,
    pub content: String,
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

    /// セッションの全メッセージを古い順に返す(user/assistantのみ。要約の対象を決めるため)。
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

    /// セッションの直近`limit`件を時系列順で返す(user/assistantのみ)。
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

    /// セッションごとの「未処理ログが残っているか」。ログが1件もないセッションは含まれない。
    pub fn session_states(&self) -> Result<Vec<(String, bool)>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT session_id, MAX(is_processed = 0) FROM conversation_logs GROUP BY session_id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)))?.collect::<Result<_, _>>()?;
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
}

#[cfg(test)]
mod tests {
    use super::super::Role;
    use super::*;

    fn mem() -> Db {
        Db::open_in_memory(Role::Agent)
    }

    #[test]
    fn logs_roundtrip_in_order_and_unprocessed() {
        let db = mem();
        db.insert_log("s", "user", "hi", None, None).unwrap();
        db.insert_log("s", "assistant", "hello", Some("anthropic"), Some("claude-sonnet-5-5")).unwrap();
        db.insert_log("other", "user", "x", None, None).unwrap();
        let rows = db.recent_messages("s", 10).unwrap();
        assert_eq!(rows.iter().map(|r| r.content.as_str()).collect::<Vec<_>>(), ["hi", "hello"]);
        assert_eq!(db.unprocessed(10).unwrap().len(), 3);
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
    fn created_at_uses_the_unified_format() {
        let db = mem();
        db.insert_log("s", "user", "a", None, None).unwrap();
        let at = &db.unprocessed(1).unwrap()[0].created_at;
        assert!(chrono::DateTime::parse_from_rfc3339(at).is_ok() && at.ends_with('Z') && at.len() == super::super::now().len(), "{at}");
    }
}
