//! 過去の発話で実行したツール(コマンドなど)の記録を、LLMへ渡す会話に添える。
//!
//! 会話ログ(`conversation_logs`)は発話と返答のテキストだけなので、そのままではモデルは過去に自分が何を実行したかを知らない。
//! 実行ログ(`turn_events`)を読み、返答の直前に`<execution_log>`として足す。
//! 直近の数ターンはコマンドの出力を(上限つきで)全文渡し、古いターンは呼び出しと短い結果だけにする。

use std::collections::HashMap;

use serde_json::Value;

use crate::db::LogRow;

/// 出力を全文で渡すターンの数(直近から数える)。
pub const FULL_TURNS: usize = 3;
/// 1回のコマンドの出力として渡す最大文字数。超えたら先頭と末尾を残す。
const OUTPUT_MAX_CHARS: usize = 20_000;
/// 呼び出しの入力として渡す最大文字数。
const INPUT_MAX_CHARS: usize = 500;

const HEADER: &str = "<execution_log>\n(システムが記録した、この発話で実際に実行したツールの記録です。あなたの出力には書かないでください。)";

/// 先頭と末尾を残して`max`文字に収める。
fn clip_middle(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_owned();
    }
    let half = max / 2;
    let head: String = s.chars().take(half).collect();
    let tail: String = s.chars().skip(n - half).collect();
    format!("{head}\n…(中略 {} 文字)…\n{tail}", n - half * 2)
}

fn clip_end(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_owned(),
    }
}

/// 1発話の実行ログ(`TaskEvent`のJSON配列)を、LLMへ渡す文面にする。呼び出しが無ければ`None`。
/// `full`なら、コマンドの出力を全文(上限つき)で含める。
pub fn render(events_json: &str, full: bool) -> Option<String> {
    let events: Vec<Value> = serde_json::from_str(events_json).ok()?;
    let field = |e: &Value, k: &str| e.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
    let mut out = String::from(HEADER);
    let mut calls = 0;
    for e in &events {
        if field(e, "type") != "tool_call" {
            continue;
        }
        calls += 1;
        let id = field(e, "id");
        let input = e.get("input").map(Value::to_string).unwrap_or_default();
        out.push_str(&format!("\n- {} {}", field(e, "name"), clip_end(&input, INPUT_MAX_CHARS)));
        let result = events.iter().find(|r| field(r, "type") == "tool_result" && field(r, "id") == id);
        match result {
            Some(r) => out.push_str(if r.get("ok").and_then(Value::as_bool).unwrap_or(false) { " → 成功" } else { " → 失敗" }),
            None => out.push_str(" → 結果なし(中断など)"),
        }
        let output: String = events
            .iter()
            .filter(|o| field(o, "type") == "command_output" && field(o, "id") == id)
            .map(|o| field(o, "text"))
            .collect();
        let body = if full && !output.trim().is_empty() {
            clip_middle(output.trim_end(), OUTPUT_MAX_CHARS)
        } else {
            result.map(|r| field(r, "preview")).unwrap_or_default()
        };
        if !body.trim().is_empty() {
            out.push_str("\n  結果:\n");
            out.push_str(&body.lines().map(|l| format!("  {l}")).collect::<Vec<_>>().join("\n"));
        }
    }
    out.push_str("\n</execution_log>");
    (calls > 0).then_some(out)
}

/// 返答の行の先頭に、その発話の実行記録を足す。`events`は発話の行IDごとの実行ログ。
/// 直近`full_turns`件の記録(実行記録のある発話の数)は出力を全文で、それより古いものは短く添える。
pub fn attach(rows: Vec<LogRow>, events: &HashMap<String, String>, full_turns: usize) -> Vec<LogRow> {
    if events.is_empty() {
        return rows;
    }
    // (返答の行の位置, 発話の行ID)
    let mut targets: Vec<(usize, String)> = Vec::new();
    let mut pending: Option<&str> = None;
    for (i, r) in rows.iter().enumerate() {
        if r.role == "user" {
            pending = Some(&r.id);
        } else if let Some(u) = pending.take().filter(|u| events.contains_key(*u)) {
            targets.push((i, u.to_owned()));
        }
    }
    let mut rows = rows;
    let first_full = targets.len().saturating_sub(full_turns);
    for (n, (i, user_id)) in targets.into_iter().enumerate() {
        if let Some(log) = render(&events[&user_id], n >= first_full) {
            rows[i].content = format!("{log}\n\n{}", rows[i].content);
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn events() -> String {
        json!([
            { "type": "tool_call", "id": "a", "name": "run_command", "input": { "command": "curl ipinfo.io" } },
            { "type": "command_output", "id": "a", "stream": "stdout", "text": "{\"ip\":\"1.2.3.4\"}\n" },
            { "type": "tool_result", "id": "a", "ok": true, "preview": "短い結果" },
        ])
        .to_string()
    }

    fn row(id: &str, role: &str, content: &str) -> LogRow {
        LogRow { id: id.into(), role: role.into(), content: content.into() }
    }

    #[test]
    fn full_includes_output_and_short_uses_preview() {
        let full = render(&events(), true).unwrap();
        assert!(full.contains("curl ipinfo.io") && full.contains("成功") && full.contains("1.2.3.4"));
        let short = render(&events(), false).unwrap();
        assert!(short.contains("curl ipinfo.io") && short.contains("短い結果") && !short.contains("1.2.3.4"));
    }

    #[test]
    fn no_calls_means_no_log() {
        assert!(render("[]", true).is_none());
        assert!(render("not json", true).is_none());
    }

    #[test]
    fn huge_output_keeps_head_and_tail() {
        let big = format!("HEAD{}TAIL", "x".repeat(OUTPUT_MAX_CHARS * 2));
        let ev = json!([
            { "type": "tool_call", "id": "a", "name": "run_command", "input": {} },
            { "type": "command_output", "id": "a", "stream": "stdout", "text": big },
        ])
        .to_string();
        let r = render(&ev, true).unwrap();
        assert!(r.contains("HEAD") && r.contains("TAIL") && r.contains("中略"));
        assert!(r.chars().count() < OUTPUT_MAX_CHARS + 500);
        assert!(r.contains("結果なし"));
    }

    #[test]
    fn attaches_to_the_reply_and_only_recent_turns_get_full_output() {
        let rows = vec![
            row("u1", "user", "一つ目"),
            row("a1", "assistant", "返答1"),
            row("u2", "user", "二つ目"),
            row("a2", "assistant", "返答2"),
            row("u3", "user", "今回"),
        ];
        let map: HashMap<String, String> = [("u1".to_owned(), events()), ("u2".to_owned(), events())].into();
        let got = attach(rows, &map, 1);
        assert!(got[1].content.starts_with("<execution_log>") && got[1].content.ends_with("返答1"));
        assert!(!got[1].content.contains("1.2.3.4"), "古いターンは出力を省く");
        assert!(got[3].content.contains("1.2.3.4"), "直近のターンは出力を渡す");
        assert_eq!(got[0].content, "一つ目");
        assert_eq!(got[4].content, "今回");
    }

    #[test]
    fn rows_are_unchanged_without_events() {
        let rows = vec![row("u1", "user", "a"), row("a1", "assistant", "b")];
        let got = attach(rows, &HashMap::new(), 3);
        assert_eq!(got[1].content, "b");
    }
}
