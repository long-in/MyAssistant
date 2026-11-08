//! 会話ごとの要約(`conversation_summaries`)。どこまで畳み込んだか(`upto_id`)と本文を持つ。
use rusqlite::{OptionalExtension, params};

use super::{Db, schema::NOW_SQL};
use crate::utils::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// 要約に畳み込み済みの最後のメッセージID。
    pub upto_id: String,
    pub text: String,
}

impl Db {
    pub fn summary(&self, session_id: &str) -> Result<Option<Summary>, AppError> {
        Ok(self
            .conn()
            .query_row(
                "SELECT upto_id, text FROM conversation_summaries WHERE session_id = ?1",
                [session_id],
                |r| Ok(Summary { upto_id: r.get(0)?, text: r.get(1)? }),
            )
            .optional()?)
    }

    pub fn save_summary(&self, session_id: &str, summary: &Summary) -> Result<(), AppError> {
        self.conn().execute(
            &format!(
                "INSERT INTO conversation_summaries (session_id, upto_id, text) VALUES (?1, ?2, ?3)
                 ON CONFLICT(session_id) DO UPDATE SET upto_id = ?2, text = ?3, updated_at = {NOW_SQL}"
            ),
            params![session_id, summary.upto_id, summary.text],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::Role;
    use super::*;

    #[test]
    fn summary_upsert() {
        let db = Db::open_in_memory(Role::Agent);
        assert_eq!(db.summary("s").unwrap(), None);
        db.save_summary("s", &Summary { upto_id: "a".into(), text: "1".into() }).unwrap();
        db.save_summary("s", &Summary { upto_id: "b".into(), text: "2".into() }).unwrap();
        assert_eq!(db.summary("s").unwrap(), Some(Summary { upto_id: "b".into(), text: "2".into() }));
        assert_eq!(db.summary("other").unwrap(), None);
    }
}
