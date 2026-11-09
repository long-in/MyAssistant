//! チャットの会話の継続。会話(セッション)のIDをエージェントごとに保存し、アプリを再起動しても同じ会話の続きから再開する。
//!
//! - 会話を切り替えるのは、利用者が「新しい会話」を選んだときだけ。
//! - 画面に戻すのは直近の発話・返答と、発話ごとの実行ログ(ツール呼び出しとコマンドの出力)。
use serde::Serialize;
use serde_json::Value;

use super::tool_loop::TaskEvent;
use crate::{db::Db, utils::error::AppError};

/// 現在の会話のIDを保存する設定キー。
pub const KEY: &str = "chat_session";
/// 画面に戻す行数の上限(発話と返答で1往復2行)。これより前の会話はDBに残り、AIへの文脈としては要約を通じて使われる。
const RESTORE_ROWS: u32 = 200;
/// 1回の発話の実行ログとして保存する、JSONの最大文字数。超えたらコマンドの出力を捨てる(呼び出しと結果は残す)。
const EVENTS_MAX_CHARS: usize = 200_000;

/// 画面に戻す1往復。返答が無いのは、失敗・中断した発話。
#[derive(Debug, Serialize, PartialEq)]
pub struct TurnView {
    pub user: String,
    pub user_at: String,
    pub reply: Option<String>,
    pub reply_at: Option<String>,
    pub via: Option<String>,
    /// 実行ログ(`TaskEvent`のJSON配列)。無ければ空。
    pub events: Vec<Value>,
}

#[derive(Debug, Serialize)]
pub struct SessionView {
    pub session_id: String,
    pub turns: Vec<TurnView>,
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 現在の会話のID。未保存なら新しく作って保存する。
pub fn current(db: &Db) -> Result<String, AppError> {
    match db.get_setting(KEY)?.filter(|v| !v.trim().is_empty()) {
        Some(id) => Ok(id),
        None => start_new(db),
    }
}

/// 新しい会話を始め、そのIDを保存する。
pub fn start_new(db: &Db) -> Result<String, AppError> {
    let id = new_id();
    db.set_setting(KEY, &id)?;
    Ok(id)
}

/// 現在の会話と、画面に戻すやり取り。
pub fn load(db: &Db) -> Result<SessionView, AppError> {
    let session_id = current(db)?;
    let mut turns: Vec<TurnView> = Vec::new();
    for row in db.turn_rows(&session_id, RESTORE_ROWS)? {
        match row.role.as_str() {
            "user" => turns.push(TurnView {
                user: row.content,
                user_at: row.created_at,
                reply: None,
                reply_at: None,
                via: None,
                events: row.events.and_then(|e| serde_json::from_str(&e).ok()).unwrap_or_default(),
            }),
            // 先頭が返答から始まるとき(行数の上限で発話が切れた)は、対応する発話が無いので捨てる
            _ => {
                if let Some(t) = turns.last_mut().filter(|t| t.reply.is_none()) {
                    t.reply = Some(row.content);
                    t.reply_at = Some(row.created_at);
                    t.via = row.model;
                }
            }
        }
    }
    Ok(SessionView { session_id, turns })
}

/// 実行ログに残すイベントだけをJSONにする。承認・説明テキスト・完了の通知は画面に出さないので残さない。
fn events_json(events: &[TaskEvent]) -> Option<String> {
    let mut kept: Vec<Value> = Vec::new();
    let mut size = 0usize;
    for e in events {
        let keep = matches!(e, TaskEvent::ToolCall { .. } | TaskEvent::ToolResult { .. } | TaskEvent::CommandOutput { .. } | TaskEvent::DispatchDraft { .. });
        if !keep {
            continue;
        }
        let Ok(v) = serde_json::to_value(e) else { continue };
        let n = v.to_string().chars().count();
        if size + n > EVENTS_MAX_CHARS && matches!(e, TaskEvent::CommandOutput { .. }) {
            continue;
        }
        size += n;
        kept.push(v);
    }
    (!kept.is_empty()).then(|| Value::Array(kept).to_string())
}

/// 直近の発話の実行ログを保存する。残すものが無ければ何もしない。実行ログは補助なので、失敗しても依頼の結果には影響させない。
pub fn record_events(db: &Db, session_id: &str, events: &[TaskEvent]) {
    let Some(json) = events_json(events) else { return };
    let saved = db.last_user_log_id(session_id).and_then(|id| match id {
        Some(id) => db.save_turn_events(&id, &json),
        None => Ok(()),
    });
    if let Err(e) = saved {
        log::warn!("turn events not saved: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Role;
    use crate::services::shell::Stream;
    use serde_json::json;

    fn call(id: &str) -> TaskEvent {
        TaskEvent::ToolCall { id: id.into(), name: "run_command".into(), input: json!({ "command": "ls" }) }
    }

    #[test]
    fn session_id_is_kept_until_a_new_conversation_starts() {
        let db = Db::open_in_memory(Role::Agent);
        let first = current(&db).unwrap();
        assert_eq!(current(&db).unwrap(), first, "再起動(再読み込み)しても同じ会話");
        let next = start_new(&db).unwrap();
        assert_ne!(next, first);
        assert_eq!(current(&db).unwrap(), next);
    }

    #[test]
    fn load_pairs_replies_and_attaches_events() {
        let db = Db::open_in_memory(Role::Agent);
        let id = current(&db).unwrap();
        db.insert_log(&id, "user", "一つ目", None, None).unwrap();
        db.insert_log(&id, "assistant", "返答", Some("anthropic"), Some("m1")).unwrap();
        db.insert_log(&id, "user", "失敗した発話", None, None).unwrap();
        record_events(&db, &id, &[call("a"), TaskEvent::Text { text: "捨てる".into() }]);
        let view = load(&db).unwrap();
        assert_eq!(view.turns.len(), 2);
        assert_eq!(view.turns[0].reply.as_deref(), Some("返答"));
        assert_eq!(view.turns[0].via.as_deref(), Some("m1"));
        assert!(view.turns[0].events.is_empty());
        assert!(view.turns[1].reply.is_none());
        assert_eq!(view.turns[1].events.len(), 1, "実行ログに残すのは呼び出しなどだけ");
    }

    #[test]
    fn nothing_is_recorded_without_events_and_old_turn_is_not_overwritten() {
        let db = Db::open_in_memory(Role::Agent);
        let id = current(&db).unwrap();
        db.insert_log(&id, "user", "発話", None, None).unwrap();
        record_events(&db, &id, &[TaskEvent::Text { text: "x".into() }]);
        assert!(load(&db).unwrap().turns[0].events.is_empty());
    }

    #[test]
    fn huge_command_output_is_dropped_but_calls_are_kept() {
        let big = "あ".repeat(EVENTS_MAX_CHARS);
        let events = vec![
            call("a"),
            TaskEvent::CommandOutput { id: "a".into(), stream: Stream::Stdout, text: big },
            TaskEvent::ToolResult { id: "a".into(), ok: true, preview: "ok".into() },
        ];
        let v: Vec<Value> = serde_json::from_str(&events_json(&events).unwrap()).unwrap();
        assert_eq!(v.len(), 2);
    }
}
