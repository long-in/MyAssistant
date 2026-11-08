//! LLM呼び出しごとのトークン使用量(`usage_logs`)。
use rusqlite::params;
use serde::Serialize;

use super::Db;
use crate::utils::error::AppError;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UsageRow {
    pub provider: String,
    pub model: String,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl Db {
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
}

#[cfg(test)]
mod tests {
    use super::super::Role;
    use super::*;

    #[test]
    fn usage_is_summed_for_this_month_only() {
        let db = Db::open_in_memory(Role::Space);
        db.record_usage("anthropic", "m", 1, 2).unwrap();
        db.record_usage("anthropic", "m", 10, 20).unwrap();
        db.conn()
            .execute(
                "INSERT INTO usage_logs (provider, model, input_tokens, output_tokens, created_at) VALUES ('anthropic', 'm', 100, 100, '2000-01-01T00:00:00.000Z')",
                [],
            )
            .unwrap();
        assert_eq!(
            db.usage_this_month().unwrap(),
            [UsageRow { provider: "anthropic".into(), model: "m".into(), requests: 2, input_tokens: 11, output_tokens: 22 }]
        );
    }
}
