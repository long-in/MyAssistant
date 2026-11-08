//! SQLite(timeline.db): 時系列の一次ログ・要約・使用量・アプリ設定と、リーダーの台帳。
//!
//! 表ごとの読み書きは下位のモジュールに分け、どれも`Db`のメソッドとして使う。スキーマと版の管理は`schema`。
use std::{path::Path, sync::Mutex};

use rusqlite::Connection;

use crate::utils::error::AppError;

mod ledger;
mod logs;
pub mod schema;
mod settings;
mod summaries;
mod usage;

pub use ledger::LedgerRow;
pub use logs::{LogRow, MemoryLog, PendingLog};
pub use schema::Role;
pub use summaries::Summary;
pub use usage::UsageRow;

pub struct Db(Mutex<Connection>);

/// いまの日時(DBに保存する形式: UTC、ミリ秒、`Z`付きのRFC3339)。SQLの既定値(`schema::NOW_SQL`)と同じ形にする。
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

impl Db {
    /// テスト用: パーソナル・スペースのDBとして開く。アプリは役割を決めて`open_as`で開く。
    #[cfg(test)]
    pub fn open(path: &Path) -> Result<Self, AppError> {
        Self::open_as(path, Role::Space)
    }

    /// 役割を指定して開く(無ければ作る)。未適用のマイグレーションがあれば当てる。リーダーのDBだけが台帳を持つ。
    pub fn open_as(path: &Path, role: Role) -> Result<Self, AppError> {
        let mut conn = Connection::open(path)?;
        schema::prepare(&mut conn, Some(path), role)?;
        Ok(Self(Mutex::new(conn)))
    }

    #[cfg(test)]
    pub(crate) fn open_in_memory(role: Role) -> Self {
        let mut conn = Connection::open_in_memory().unwrap();
        schema::prepare(&mut conn, None, role).unwrap();
        Self(Mutex::new(conn))
    }

    /// 接続。ロックが毒されても(パニック後)中身を使い続ける。
    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}
