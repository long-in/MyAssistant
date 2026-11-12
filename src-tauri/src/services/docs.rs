//! アプリの設計書(`docs/*.md`)をバイナリへ埋め込み、チャットのLLMが読めるようにする。
//! 読み取り専用で、LLMが渡すのは文書名・検索語・文字位置だけ。ファイルシステムは触らないので、パスのサンドボックスは不要。
//! 文書を追加したら`DOCS`へ足す(`embedded_docs_match_docs_dir`が漏れを検知する)。

/// `read`が1回で返す最大文字数。
const READ_MAX_CHARS: usize = 20_000;
/// `search`の最大件数と、1件あたりの最大文字数。
const SEARCH_MAX_HITS: usize = 50;
const SEARCH_LINE_CHARS: usize = 200;

macro_rules! doc {
    ($name:literal) => {
        ($name, include_str!(concat!("../../../docs/", $name)))
    };
}

/// 埋め込む設計書(ファイル名, 本文)。
const DOCS: &[(&str, &str)] = &[
    doc!("README.md"),
    doc!("agent-command-tasks.md"),
    doc!("agent-file-tools.md"),
    doc!("architecture.md"),
    doc!("backup.md"),
    doc!("chat-flow.md"),
    doc!("command-policy.md"),
    doc!("conversation-session.md"),
    doc!("data-and-settings.md"),
    doc!("debug-log.md"),
    doc!("frontend.md"),
    doc!("global-settings.md"),
    doc!("knowledge-graph.md"),
    doc!("knowledge.md"),
    doc!("leader.md"),
    doc!("llm-settings.md"),
    doc!("local-llm.md"),
    doc!("memory.md"),
    doc!("proxy.md"),
    doc!("sandbox.md"),
    doc!("security.md"),
    doc!("skills.md"),
    doc!("user-profile.md"),
    doc!("voice-input.md"),
    doc!("voice-output.md"),
    doc!("wake-standby.md"),
    doc!("workspace.md"),
];

/// 設計書のファイル名と、先頭の見出しを一覧にする。
pub fn list() -> String {
    DOCS.iter().map(|(name, body)| format!("{name}  {}", title(body))).collect::<Vec<_>>().join("\n")
}

/// 先頭の見出し行(`# `を除く)。
fn title(body: &str) -> &str {
    body.lines().next().unwrap_or("").trim_start_matches('#').trim()
}

/// 設計書を`offset`文字目から最大`READ_MAX_CHARS`文字返す。続きがあるときは次の`offset`を末尾に添える。
pub fn read(name: &str, offset: usize) -> Result<String, String> {
    read_with_limit(name, offset, READ_MAX_CHARS)
}

fn read_with_limit(name: &str, offset: usize, max_chars: usize) -> Result<String, String> {
    let (_, body) = DOCS
        .iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| format!("設計書が見つかりません: {name}(list_docsで一覧を確認してください)"))?;
    let total = body.chars().count();
    if offset >= total && offset > 0 {
        return Err(format!("offsetが末尾({total}文字)を超えています。"));
    }
    let text: String = body.chars().skip(offset).take(max_chars).collect();
    let end = offset + text.chars().count();
    if end < total {
        Ok(format!("{text}\n...(続きがあります。offset={end}で続きを読めます。全{total}文字)"))
    } else {
        Ok(text)
    }
}

/// 全設計書から文字列(大文字小文字を区別しない)を検索し、`ファイル名:行番号: 該当行`を返す。
pub fn search(query: &str) -> Result<String, String> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Err("queryが必要です。".into());
    }
    let mut hits = Vec::new();
    'outer: for (name, body) in DOCS {
        for (i, line) in body.lines().enumerate() {
            if line.to_lowercase().contains(&query) {
                if hits.len() >= SEARCH_MAX_HITS {
                    break 'outer;
                }
                let line: String = line.trim().chars().take(SEARCH_LINE_CHARS).collect();
                hits.push(format!("{name}:{}: {line}", i + 1));
            }
        }
    }
    if hits.is_empty() {
        return Ok("該当なし".into());
    }
    Ok(hits.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_docs_match_docs_dir() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs");
        let mut on_disk: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.ends_with(".md"))
            .collect();
        on_disk.sort();
        let mut embedded: Vec<String> = DOCS.iter().map(|(n, _)| n.to_string()).collect();
        embedded.sort();
        assert_eq!(on_disk, embedded, "docs/にあるのにDOCSへ未登録、または削除済みの文書がある");
    }

    #[test]
    fn list_has_name_and_title() {
        let out = list();
        assert!(out.contains("chat-flow.md  対話の処理"));
        assert_eq!(out.lines().count(), DOCS.len());
    }

    #[test]
    fn read_returns_body_and_rejects_unknown_or_path() {
        assert!(read("chat-flow.md", 0).unwrap().starts_with("# 対話の処理"));
        assert!(read("nope.md", 0).is_err());
        assert!(read("../Cargo.toml", 0).is_err());
        assert!(read("/etc/passwd", 0).is_err());
    }

    #[test]
    fn read_truncates_and_continues_with_offset() {
        let first = read_with_limit("sandbox.md", 0, 100).unwrap();
        assert!(first.contains("offset=100で続きを読めます"));
        let rest = read_with_limit("sandbox.md", 100, 100).unwrap();
        assert!(!rest.contains("# サンドボックス") && !rest.is_empty());
        assert!(read("sandbox.md", usize::MAX / 2).is_err());
    }

    #[test]
    fn search_finds_case_insensitively_and_caps_hits() {
        let out = search("SEND_CHAT").unwrap();
        assert!(out.contains("chat-flow.md:"));
        assert!(search("の").unwrap().lines().count() <= SEARCH_MAX_HITS);
        assert_eq!(search("zzzz存在しない語zzzz").unwrap(), "該当なし");
        assert!(search("  ").is_err());
    }
}
