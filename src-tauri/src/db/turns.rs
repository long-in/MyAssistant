//! 会話の再開に使う読み書き。画面に戻す発話・返答の行(`conversation_logs`)と、発話ごとの実行ログ(`turn_events`)。
use rusqlite::{OptionalExtension, params};

use super::Db;
use crate::utils::error::AppError;

/// 画面に戻す1行(ユーザー発話またはAIの返答)。`events`は発話の行にだけ付く実行ログ(JSON配列)。
#[derive(Debug, Clone)]
pub struct TurnRow {
    pub role: String,
    pub content: String,
    pub model: Option<String>,
    pub created_at: String,
    pub events: Option<String>,
}

impl Db {
    /// セッションの直近`limit`行(user/assistantのみ)を古い順に返す。発話の行には実行ログを添える。
    pub fn turn_rows(&self, session_id: &str, limit: u32) -> Result<Vec<TurnRow>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT role, content, model, created_at, events FROM (
                SELECT l.role, l.content, l.model, l.created_at, l.rowid AS seq, e.events
                FROM conversation_logs l LEFT JOIN turn_events e ON e.user_log_id = l.id
                WHERE l.session_id = ?1 AND l.role IN ('user', 'assistant')
                ORDER BY l.created_at DESC, l.rowid DESC LIMIT ?2
             ) ORDER BY created_at ASC, seq ASC",
        )?;
        let rows = stmt
            .query_map(params![session_id, limit], |r| {
                Ok(TurnRow { role: r.get(0)?, content: r.get(1)?, model: r.get(2)?, created_at: r.get(3)?, events: r.get(4)? })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// セッションの最新のユーザー発話の行ID。
    pub fn last_user_log_id(&self, session_id: &str) -> Result<Option<String>, AppError> {
        Ok(self
            .conn()
            .query_row(
                "SELECT id FROM conversation_logs WHERE session_id = ?1 AND role = 'user' ORDER BY created_at DESC, rowid DESC LIMIT 1",
                params![session_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// 発話の実行ログ(JSON配列)を保存する。同じ発話なら置き換える。
    pub fn save_turn_events(&self, user_log_id: &str, events: &str) -> Result<(), AppError> {
        self.conn().execute("INSERT OR REPLACE INTO turn_events (user_log_id, events) VALUES (?1, ?2)", params![user_log_id, events])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Role;

    #[test]
    fn rows_are_in_order_with_events_on_the_user_row() {
        let db = Db::open_in_memory(Role::Agent);
        let u1 = db.insert_log("s", "user", "一つ目", None, None).unwrap();
        db.insert_log("s", "assistant", "返答1", Some("anthropic"), Some("m")).unwrap();
        db.insert_log("other", "user", "別の会話", None, None).unwrap();
        let u2 = db.insert_log("s", "user", "二つ目", None, None).unwrap();
        db.save_turn_events(&u1, "[1]").unwrap();
        let rows = db.turn_rows("s", 10).unwrap();
        assert_eq!(rows.iter().map(|r| r.content.as_str()).collect::<Vec<_>>(), ["一つ目", "返答1", "二つ目"]);
        assert_eq!(rows[0].events.as_deref(), Some("[1]"));
        assert_eq!(rows[1].model.as_deref(), Some("m"));
        assert!(rows[2].events.is_none());
        assert_eq!(db.last_user_log_id("s").unwrap(), Some(u2));
        assert_eq!(db.turn_rows("s", 2).unwrap().len(), 2, "直近の行だけ");
    }
}
