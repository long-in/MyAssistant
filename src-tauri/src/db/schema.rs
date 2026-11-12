//! スキーマと、版(`PRAGMA user_version`)ごとのマイグレーション。
//!
//! - 開くたびに、未適用の版を古い順に1つずつ当てる。各版は1つのトランザクションで当て、版の番号も同じトランザクションで書く。
//! - 既存のDBを書き換える前に、同じフォルダへ`<ファイル名>.bak-v<元の版>`として複製を残す(`VACUUM INTO`)。複製できなければ開かない。
//! - 版0は、版管理を入れる前のDB(または空のDB)。版1はその時点のスキーマと同じなので、既存のDBにはそのまま当たる。
//! - リーダーの台帳(`leader_tasks`)は版とは別に、DBの役割(`Role`)で作る。
use std::path::{Path, PathBuf};

use rusqlite::{Connection, TransactionBehavior};

use crate::utils::error::AppError;

/// 日時の形式(UTC、ミリ秒、`Z`付きのRFC3339)。SQLの既定値と、Rustで作る値(`super::now`)をこれにそろえる。
pub const NOW_SQL: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

/// 同じファイルをほかの接続が書いているとき、待つ時間(ミリ秒)。
const BUSY_TIMEOUT_MS: u32 = 5_000;

/// DBの役割。リーダーのDBだけが台帳を持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// パーソナル・ワークのエージェント。
    Agent,
    /// リーダー。
    Leader,
}

/// 版ごとのマイグレーション。`MIGRATIONS[i]`を当てると版`i + 1`になる。
const MIGRATIONS: &[&str] = &[
    // 版1: 版管理を入れる前のスキーマ(既存のDBには何もしない)
    "
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
    ",
    // 版2: 日時をUTC・ミリ秒・`Z`付きのRFC3339へそろえる(既定値も)。並びを保つため、行のrowidは引き継ぐ
    "
    CREATE TABLE conversation_logs_new (
        id TEXT PRIMARY KEY,
        session_id TEXT NOT NULL,
        role TEXT NOT NULL,
        content TEXT NOT NULL,
        provider TEXT,
        model TEXT,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        is_processed INTEGER NOT NULL DEFAULT 0
    );
    INSERT INTO conversation_logs_new (rowid, id, session_id, role, content, provider, model, created_at, is_processed)
        SELECT rowid, id, session_id, role, content, provider, model,
               COALESCE(strftime('%Y-%m-%dT%H:%M:%fZ', created_at), created_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
               COALESCE(is_processed, 0)
        FROM conversation_logs;
    DROP TABLE conversation_logs;
    ALTER TABLE conversation_logs_new RENAME TO conversation_logs;
    CREATE INDEX idx_logs_session ON conversation_logs(session_id, created_at);
    CREATE INDEX idx_logs_unprocessed ON conversation_logs(is_processed);

    CREATE TABLE usage_logs_new (
        id INTEGER PRIMARY KEY,
        provider TEXT NOT NULL,
        model TEXT NOT NULL,
        input_tokens INTEGER NOT NULL,
        output_tokens INTEGER NOT NULL,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    );
    INSERT INTO usage_logs_new (id, provider, model, input_tokens, output_tokens, created_at)
        SELECT id, provider, model, input_tokens, output_tokens,
               COALESCE(strftime('%Y-%m-%dT%H:%M:%fZ', created_at), created_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        FROM usage_logs;
    DROP TABLE usage_logs;
    ALTER TABLE usage_logs_new RENAME TO usage_logs;
    CREATE INDEX idx_usage_created ON usage_logs(created_at);

    CREATE TABLE app_settings_new (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL,
        updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    );
    INSERT INTO app_settings_new (key, value, updated_at)
        SELECT key, value,
               COALESCE(strftime('%Y-%m-%dT%H:%M:%fZ', updated_at), updated_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        FROM app_settings;
    DROP TABLE app_settings;
    ALTER TABLE app_settings_new RENAME TO app_settings;
    ",
    // 版3: 会話の要約を設定(`app_settings`の`summary:<session_id>`)から専用の表へ移す。読めない値は移さない(従来も読まれていなかった)
    "
    CREATE TABLE conversation_summaries (
        session_id TEXT PRIMARY KEY,
        upto_id TEXT NOT NULL,
        text TEXT NOT NULL,
        updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    );
    INSERT OR REPLACE INTO conversation_summaries (session_id, upto_id, text, updated_at)
        SELECT substr(key, 9), json_extract(value, '$.upto_id'), json_extract(value, '$.text'), updated_at
        FROM app_settings
        WHERE substr(key, 1, 8) = 'summary:' AND length(key) > 8 AND json_valid(value)
          AND json_type(value, '$.upto_id') = 'text' AND json_type(value, '$.text') = 'text';
    DELETE FROM app_settings WHERE substr(key, 1, 8) = 'summary:';
    ",
    // 版4: 「スペース」の「エージェント」への改称にともない、設定キー`space_data`を`agent_data`へ移す(新しいキーがあればそちらを優先)
    "
    UPDATE OR IGNORE app_settings SET key = 'agent_data' WHERE key = 'space_data';
    DELETE FROM app_settings WHERE key = 'space_data';
    ",
    // 版5: 発話ごとの実行ログ(ツール呼び出しとコマンドの出力)。ユーザー発話の行IDに結び付け、会話の再開時に画面へ戻す
    "
    CREATE TABLE turn_events (
        user_log_id TEXT PRIMARY KEY,
        events TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    );
    ",
    // 版6: 整理・棚卸しが出す学びの提案(記憶・スキル・ノートの統合・見直し)。承認されるまで反映しない。
    // 同じ種類・同じ`norm`の提案は、決定済み(却下を含む)でも重ねて出さない
    "
    CREATE TABLE learning_proposals (
        id TEXT PRIMARY KEY,
        kind TEXT NOT NULL,
        norm TEXT NOT NULL,
        text TEXT NOT NULL,
        payload TEXT,
        source_log_ids TEXT NOT NULL DEFAULT '[]',
        status TEXT NOT NULL DEFAULT 'pending',
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        decided_at TEXT
    );
    CREATE UNIQUE INDEX idx_proposals_kind_norm ON learning_proposals(kind, norm);
    CREATE INDEX idx_proposals_status ON learning_proposals(status, created_at);
    ",
    // 版7: API 方式の権限を settings.json(`api_claude_settings`)へ作り直したため、旧形式のコマンド権限と Web 検索のオン/オフを消す(移行はしない)
    "
    DELETE FROM app_settings WHERE key IN ('command_policy', 'web_access');
    ",
];

/// 最新の版。
pub const LATEST: u32 = MIGRATIONS.len() as u32;

/// リーダーの台帳。日時は書き込む側(`super::now`)が決める。
const LEDGER: &str = "
CREATE TABLE IF NOT EXISTS leader_tasks (
    id TEXT PRIMARY KEY,
    batch_id TEXT NOT NULL,
    instruction TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    agent_name TEXT NOT NULL,
    session_id TEXT NOT NULL,
    status TEXT NOT NULL,
    confirmed INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_leader_tasks_created ON leader_tasks(created_at);
";

fn version(conn: &Connection) -> rusqlite::Result<u32> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
}

fn table_exists(conn: &Connection, name: &str) -> rusqlite::Result<bool> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)", [name], |r| r.get(0))
}

/// 接続の設定・マイグレーション・役割ごとの表の準備をまとめて行う。`path`はファイルのDBのとき(複製の置き場所)。
pub fn prepare(conn: &mut Connection, path: Option<&Path>, role: Role) -> Result<(), AppError> {
    conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS.into()))?;
    // 同じファイルを複数の接続で開くことがあるため、読み取りが書き込みを待たないWALにする(メモリ上のDBでは無視される)
    conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
    migrate(conn, path, LATEST)?;
    match role {
        Role::Leader => conn.execute_batch(LEDGER)?,
        Role::Agent => {}
    }
    Ok(())
}

/// `target`の版まで当てる。テストでは途中の版で止めて、古いDBを作るのにも使う。
pub(super) fn migrate(conn: &mut Connection, path: Option<&Path>, target: u32) -> Result<(), AppError> {
    let current = version(conn)?;
    if current >= target {
        return Ok(());
    }
    if let Some(path) = path
        && table_exists(conn, "conversation_logs")?
    {
        backup(conn, path, current)?;
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().take(target as usize) {
        let to = i as u32 + 1;
        // 別の接続が先に当てていたら飛ばす(確かめるのは書き込みのロックを取ってから)
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if version(&tx)? < to {
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", to)?;
        }
        tx.commit()?;
    }
    Ok(())
}

/// マイグレーションの前の複製を残す。同名のファイルがあれば、日時を付けた別名にする。
fn backup(conn: &Connection, path: &Path, from: u32) -> Result<PathBuf, AppError> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "timeline.db".into());
    let mut dest = path.with_file_name(format!("{name}.bak-v{from}"));
    if dest.exists() {
        let stamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
        dest = path.with_file_name(format!("{name}.bak-v{from}-{stamp}"));
    }
    conn.execute("VACUUM INTO ?1", [dest.to_string_lossy()])
        .map_err(|e| AppError::Db(format!("backup before migration failed: {e}")))?;
    log::info!("database backed up before migration: {}", dest.display());
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 版管理を入れる前のアプリが作っていたDB(台帳を含む)。
    const LEGACY: &str = "
    CREATE TABLE conversation_logs (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL,
        provider TEXT, model TEXT, created_at DATETIME DEFAULT CURRENT_TIMESTAMP, is_processed BOOLEAN DEFAULT 0);
    CREATE INDEX idx_logs_session ON conversation_logs(session_id, created_at);
    CREATE INDEX idx_logs_unprocessed ON conversation_logs(is_processed);
    CREATE TABLE usage_logs (id INTEGER PRIMARY KEY AUTOINCREMENT, provider TEXT NOT NULL, model TEXT NOT NULL,
        input_tokens INTEGER NOT NULL, output_tokens INTEGER NOT NULL, created_at DATETIME DEFAULT CURRENT_TIMESTAMP);
    CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at DATETIME DEFAULT CURRENT_TIMESTAMP);
    CREATE TABLE leader_tasks (id TEXT PRIMARY KEY, batch_id TEXT NOT NULL, instruction TEXT NOT NULL, space_id TEXT NOT NULL,
        space_name TEXT NOT NULL, session_id TEXT NOT NULL, status TEXT NOT NULL, confirmed BOOLEAN NOT NULL DEFAULT 0, created_at TEXT NOT NULL);
    INSERT INTO conversation_logs (id, session_id, role, content, created_at) VALUES
        ('b', 's', 'user', '後', '2026-01-01 10:00:00'),
        ('a', 's', 'user', '先', '2026-01-01 09:00:00'),
        ('c', 's', 'assistant', '同時', '2026-01-01 10:00:00');
    INSERT INTO usage_logs (provider, model, input_tokens, output_tokens, created_at) VALUES ('anthropic', 'm', 1, 2, '2026-01-01 00:00:00');
    INSERT INTO app_settings (key, value, updated_at) VALUES
        ('routing', '{}', '2026-01-01 00:00:00'),
        ('summary:s', '{\"upto_id\":\"a\",\"text\":\"要約\"}', '2026-01-02 00:00:00'),
        ('summary:broken', '{', '2026-01-02 00:00:00');
    ";

    fn legacy_file() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("timeline.db");
        Connection::open(&path).unwrap().execute_batch(LEGACY).unwrap();
        (d, path)
    }

    #[test]
    fn fresh_database_gets_latest_schema_without_backup() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("timeline.db");
        let mut conn = Connection::open(&path).unwrap();
        prepare(&mut conn, Some(&path), Role::Agent).unwrap();
        assert_eq!(version(&conn).unwrap(), LATEST);
        assert!(table_exists(&conn, "conversation_summaries").unwrap());
        assert!(!table_exists(&conn, "leader_tasks").unwrap());
        assert!(!d.path().join("timeline.db.bak-v0").exists());
    }

    #[test]
    fn legacy_database_is_backed_up_and_migrated() {
        let (d, path) = legacy_file();
        let mut conn = Connection::open(&path).unwrap();
        prepare(&mut conn, Some(&path), Role::Agent).unwrap();
        assert_eq!(version(&conn).unwrap(), LATEST);

        // 複製は元のまま(版0・要約は設定のまま)
        let bak = Connection::open(d.path().join("timeline.db.bak-v0")).unwrap();
        assert_eq!(version(&bak).unwrap(), 0);
        let n: i64 = bak.query_row("SELECT COUNT(*) FROM app_settings WHERE key LIKE 'summary:%'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);

        // 日時はRFC3339にそろい、同時刻の行の並び(rowid)も保たれる
        let rows: Vec<(String, String)> = conn
            .prepare("SELECT id, created_at FROM conversation_logs ORDER BY created_at, rowid")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            rows,
            [
                ("a".into(), "2026-01-01T09:00:00.000Z".into()),
                ("b".into(), "2026-01-01T10:00:00.000Z".into()),
                ("c".into(), "2026-01-01T10:00:00.000Z".into()),
            ]
        );
        let usage: String = conn.query_row("SELECT created_at FROM usage_logs", [], |r| r.get(0)).unwrap();
        assert_eq!(usage, "2026-01-01T00:00:00.000Z");

        // 要約は専用の表へ移り、設定からは消える(読めない値は移さない)
        let (upto, text, at): (String, String, String) = conn
            .query_row("SELECT upto_id, text, updated_at FROM conversation_summaries WHERE session_id = 's'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!((upto.as_str(), text.as_str(), at.as_str()), ("a", "要約", "2026-01-02T00:00:00.000Z"));
        let left: i64 = conn.query_row("SELECT COUNT(*) FROM app_settings WHERE key LIKE 'summary:%'", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
        let summaries: i64 = conn.query_row("SELECT COUNT(*) FROM conversation_summaries", [], |r| r.get(0)).unwrap();
        assert_eq!(summaries, 1);
    }

    #[test]
    fn reopening_does_nothing_and_takes_no_second_backup() {
        let (d, path) = legacy_file();
        for _ in 0..2 {
            let mut conn = Connection::open(&path).unwrap();
            prepare(&mut conn, Some(&path), Role::Agent).unwrap();
        }
        let backups = std::fs::read_dir(d.path()).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().contains(".bak-")).count();
        assert_eq!(backups, 1);
    }

    #[test]
    fn existing_backup_is_not_overwritten() {
        let (d, path) = legacy_file();
        std::fs::write(d.path().join("timeline.db.bak-v0"), b"older").unwrap();
        let mut conn = Connection::open(&path).unwrap();
        prepare(&mut conn, Some(&path), Role::Agent).unwrap();
        assert_eq!(std::fs::read(d.path().join("timeline.db.bak-v0")).unwrap(), b"older");
        let backups = std::fs::read_dir(d.path()).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with("timeline.db.bak-v0-")).count();
        assert_eq!(backups, 1);
    }

    #[test]
    fn old_command_permissions_are_removed() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, None, 6).unwrap();
        conn.execute_batch("INSERT INTO app_settings (key, value) VALUES ('command_policy', '{}'), ('web_access', '0'), ('routing', '{}');").unwrap();
        prepare(&mut conn, None, Role::Agent).unwrap();
        let keys: Vec<String> = conn.prepare("SELECT key FROM app_settings").unwrap().query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
        assert_eq!(keys, ["routing"]);
    }

    #[test]
    fn only_the_leader_gets_a_ledger() {
        let mut leader = Connection::open_in_memory().unwrap();
        prepare(&mut leader, None, Role::Leader).unwrap();
        assert!(table_exists(&leader, "leader_tasks").unwrap());
        let mut agent = Connection::open_in_memory().unwrap();
        prepare(&mut agent, None, Role::Agent).unwrap();
        assert!(!table_exists(&agent, "leader_tasks").unwrap());
    }
}
