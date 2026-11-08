//! リーダーの台帳(`leader_tasks`)。表はリーダーのDBにだけある(`Role::Leader`で開いたとき)。
use rusqlite::params;
use serde::Serialize;

use super::Db;
use crate::utils::error::AppError;

/// リーダーの台帳の1行(1回の配布の、1つの宛先)。
/// 宛先の名称は、エージェントが削除されても残せるよう控えておく。結果の本文は持たない。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LedgerRow {
    pub id: String,
    /// 同じ配布(同時に複数の宛先へ送ったもの)をまとめるID。
    pub batch_id: String,
    pub instruction: String,
    pub agent_id: String,
    pub agent_name: String,
    pub session_id: String,
    /// 最後に分かった状態(`running` / `done` / `failed` / `interrupted`)。承認待ちは実行中の一時状態なので保存しない。
    pub status: String,
    pub confirmed: bool,
    /// DBの日時の形式(`super::now`)。
    pub created_at: String,
}

impl Db {
    pub fn ledger_insert(&self, row: &LedgerRow) -> Result<(), AppError> {
        self.conn().execute(
            "INSERT INTO leader_tasks (id, batch_id, instruction, agent_id, agent_name, session_id, status, confirmed, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![row.id, row.batch_id, row.instruction, row.agent_id, row.agent_name, row.session_id, row.status, row.confirmed, row.created_at],
        )?;
        Ok(())
    }

    pub fn ledger_delete(&self, id: &str) -> Result<(), AppError> {
        self.conn().execute("DELETE FROM leader_tasks WHERE id = ?1", [id])?;
        Ok(())
    }

    /// 新しい順に`limit`件。
    pub fn ledger_list(&self, limit: u32) -> Result<Vec<LedgerRow>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, batch_id, instruction, agent_id, agent_name, session_id, status, confirmed, created_at
             FROM leader_tasks ORDER BY created_at DESC, rowid DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit], |r| {
                Ok(LedgerRow {
                    id: r.get(0)?,
                    batch_id: r.get(1)?,
                    instruction: r.get(2)?,
                    agent_id: r.get(3)?,
                    agent_name: r.get(4)?,
                    session_id: r.get(5)?,
                    status: r.get(6)?,
                    confirmed: r.get(7)?,
                    created_at: r.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// セッションの依頼の状態を更新する(依頼が終わったとき)。
    pub fn ledger_set_status_by_session(&self, session_id: &str, status: &str) -> Result<(), AppError> {
        self.conn().execute("UPDATE leader_tasks SET status = ?2 WHERE session_id = ?1", params![session_id, status])?;
        Ok(())
    }

    pub fn ledger_set_status(&self, id: &str, status: &str) -> Result<(), AppError> {
        self.conn().execute("UPDATE leader_tasks SET status = ?2 WHERE id = ?1", params![id, status])?;
        Ok(())
    }

    pub fn ledger_set_confirmed(&self, id: &str, confirmed: bool) -> Result<(), AppError> {
        self.conn().execute("UPDATE leader_tasks SET confirmed = ?2 WHERE id = ?1", params![id, confirmed])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::Role;
    use super::*;

    fn row(id: &str, session: &str, created: &str) -> LedgerRow {
        LedgerRow {
            id: id.into(),
            batch_id: "b".into(),
            instruction: "調べて".into(),
            agent_id: "s1".into(),
            agent_name: "調査".into(),
            session_id: session.into(),
            status: "running".into(),
            confirmed: false,
            created_at: created.into(),
        }
    }

    #[test]
    fn ledger_roundtrip_status_and_confirm() {
        let db = Db::open_in_memory(Role::Leader);
        db.ledger_insert(&row("a", "sa", "2026-01-01T00:00:00.000Z")).unwrap();
        db.ledger_insert(&row("b", "sb", "2026-01-02T00:00:00.000Z")).unwrap();
        let rows = db.ledger_list(10).unwrap();
        assert_eq!(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["b", "a"]); // 新しい順
        db.ledger_set_status_by_session("sa", "done").unwrap();
        db.ledger_set_confirmed("a", true).unwrap();
        let a = db.ledger_list(10).unwrap().into_iter().find(|r| r.id == "a").unwrap();
        assert_eq!((a.status.as_str(), a.confirmed), ("done", true));
        assert_eq!(db.ledger_list(1).unwrap().len(), 1);
        db.ledger_delete("a").unwrap();
        assert_eq!(db.ledger_list(10).unwrap().len(), 1);
    }

    #[test]
    fn agent_database_has_no_ledger() {
        let db = Db::open_in_memory(Role::Agent);
        assert!(db.ledger_list(10).is_err());
    }
}
