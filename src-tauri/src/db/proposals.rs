//! 学びの提案(`learning_proposals`)。整理・棚卸しが出し、ユーザーが承認するまで反映しない。
use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;
use serde_json::Value;

use super::{Db, schema::NOW_SQL};
use crate::utils::error::AppError;

/// 提案1件。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Proposal {
    pub id: String,
    /// `memory` | `skill` | `merge` | `review`
    pub kind: String,
    /// 画面に出す本文(記憶の文面、スキル・ノートの説明など)
    pub text: String,
    /// 種類ごとの付加情報(スキルの下書き、統合する2つのパスなど)
    pub payload: Option<Value>,
    pub source_log_ids: Vec<String>,
    /// `pending` | `accepted` | `rejected`
    pub status: String,
    pub created_at: String,
}

const COLUMNS: &str = "id, kind, text, payload, source_log_ids, status, created_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<Proposal> {
    let payload: Option<String> = r.get(3)?;
    let ids: String = r.get(4)?;
    Ok(Proposal {
        id: r.get(0)?,
        kind: r.get(1)?,
        text: r.get(2)?,
        payload: payload.and_then(|p| serde_json::from_str(&p).ok()),
        source_log_ids: serde_json::from_str(&ids).unwrap_or_default(),
        status: r.get(5)?,
        created_at: r.get(6)?,
    })
}

impl Db {
    /// 提案を足す。同じ種類・`norm`の提案が(決定済みでも)あれば足さず`false`を返す。
    pub fn add_proposal(&self, kind: &str, norm: &str, text: &str, payload: Option<&Value>, source_log_ids: &[String]) -> Result<bool, AppError> {
        let id = uuid::Uuid::new_v4().to_string();
        let ids = serde_json::to_string(source_log_ids).unwrap_or_else(|_| "[]".into());
        let n = self.conn().execute(
            "INSERT OR IGNORE INTO learning_proposals (id, kind, norm, text, payload, source_log_ids) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, kind, norm, text, payload.map(Value::to_string), ids],
        )?;
        Ok(n > 0)
    }

    /// 未決の提案(古い順)。
    pub fn pending_proposals(&self, limit: u32) -> Result<Vec<Proposal>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM learning_proposals WHERE status = 'pending' ORDER BY created_at, rowid LIMIT ?1"
        ))?;
        let rows = stmt.query_map([limit], from_row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn proposal(&self, id: &str) -> Result<Option<Proposal>, AppError> {
        Ok(self
            .conn()
            .query_row(&format!("SELECT {COLUMNS} FROM learning_proposals WHERE id = ?1"), [id], from_row)
            .optional()?)
    }

    /// 決定を記録する(`accepted` / `rejected`)。
    pub fn decide_proposal(&self, id: &str, status: &str) -> Result<(), AppError> {
        self.conn().execute(
            &format!("UPDATE learning_proposals SET status = ?2, decided_at = {NOW_SQL} WHERE id = ?1"),
            params![id, status],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::Role;
    use super::*;

    #[test]
    fn duplicates_are_ignored_even_after_decision() {
        let db = Db::open_in_memory(Role::Agent);
        assert!(db.add_proposal("memory", "結論から", "結論から話す", None, &["l1".into()]).unwrap());
        assert!(!db.add_proposal("memory", "結論から", "結論から話す。", None, &[]).unwrap());
        // 種類が違えば別の提案
        assert!(db.add_proposal("review", "結論から", "x", Some(&serde_json::json!({"path": "a.md"})), &[]).unwrap());
        let list = db.pending_proposals(10).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].source_log_ids, ["l1"]);
        assert_eq!(list[1].payload, Some(serde_json::json!({"path": "a.md"})));
        db.decide_proposal(&list[0].id, "rejected").unwrap();
        assert_eq!(db.pending_proposals(10).unwrap().len(), 1);
        assert_eq!(db.proposal(&list[0].id).unwrap().unwrap().status, "rejected");
        assert!(!db.add_proposal("memory", "結論から", "結論から話す", None, &[]).unwrap(), "却下済みは二度と出さない");
    }
}
