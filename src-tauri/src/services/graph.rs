//! ナレッジのグラフ索引と想起。ノート(点)をタグ・リンク・同じ会話由来(線)で結び、
//! 発話に一致するノートを起点に最大2ホップだけ辿って、関連ノートの要約をシステムプロンプトへ渡す。
//! 全文を渡さず、本文はモデルが必要なときに`read_file`で読む(段階読み)。LLM・ネットワークは使わない。
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    sync::{Arc, Mutex},
    time::UNIX_EPOCH,
};

use super::knowledge::{self, CAPTURE_CATEGORY, CORRECTIONS_PATH};
use crate::utils::error::AppError;

/// 起点にするノート数と、辿るホップ数。
const MAX_SEEDS: usize = 3;
const MAX_HOPS: usize = 2;
/// 1ホップごとの減衰と、結果に残す最小の関連度。
const HOP_DECAY: f32 = 0.5;
const MIN_ACTIVATION: f32 = 0.1;
/// 照合に使う本文の上限(文字)と、注入する要約の長さ。
const MATCH_BODY_CHARS: usize = 20_000;
const SUMMARY_CHARS: usize = 120;
/// 想起の対象外にするノート(専用の枠で渡す、または生の会話)。
const EXCLUDED_PATHS: [&str; 1] = [CORRECTIONS_PATH];

#[derive(Debug, Clone)]
struct Node {
    path: String,
    title: String,
    summary: String,
    tags: Vec<String>,
    /// タイトル・タグ由来の語(照合で重み付けする)
    head_terms: HashSet<String>,
    body_terms: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum Reason {
    Tag(String),
    Link,
    SameConversation,
}

#[derive(Debug, Clone)]
struct Edge {
    to: usize,
    weight: f32,
    reason: Reason,
}

/// 想起したノート1件。
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub path: String,
    pub title: String,
    pub summary: String,
    /// なぜ選ばれたか(例: キーワード一致、〇〇とタグ「x」を共有)
    pub reason: String,
    pub score: f32,
}

#[derive(Debug, Default)]
pub struct Graph {
    nodes: Vec<Node>,
    edges: Vec<Vec<Edge>>,
    /// 語ごとの出現ノート数(照合のIDF用)
    df: HashMap<String, usize>,
}

/// 照合用の語に分ける。英数字は単語(2文字以上)、それ以外(日本語など)は2文字ずつのN-gram。
/// ひらがなだけのN-gram(「ので」「ます」など)は手がかりにならないため捨てる。
pub fn terms(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut word = String::new();
    let mut run: Vec<char> = Vec::new();
    let flush_word = |w: &mut String, out: &mut HashSet<String>| {
        if w.chars().count() >= 2 {
            out.insert(w.to_lowercase());
        }
        w.clear();
    };
    let flush_run = |r: &mut Vec<char>, out: &mut HashSet<String>| {
        if r.len() == 1 && !is_hiragana(r[0]) {
            out.insert(r[0].to_string());
        }
        for pair in r.windows(2) {
            if !(is_hiragana(pair[0]) && is_hiragana(pair[1])) {
                out.insert(pair.iter().collect());
            }
        }
        r.clear();
    };
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            flush_run(&mut run, &mut out);
            word.push(c);
        } else if c.is_alphanumeric() {
            flush_word(&mut word, &mut out);
            run.push(c);
        } else {
            flush_word(&mut word, &mut out);
            flush_run(&mut run, &mut out);
        }
    }
    flush_word(&mut word, &mut out);
    flush_run(&mut run, &mut out);
    out
}

fn is_hiragana(c: char) -> bool {
    ('\u{3041}'..='\u{309f}').contains(&c)
}

/// 本文から見出しを除いた最初の段落を、1行の要約にする。
fn summarize(body: &str) -> String {
    let text: String = body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && *l != "---")
        .collect::<Vec<_>>()
        .join(" ");
    match text.char_indices().nth(SUMMARY_CHARS) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text,
    }
}

/// 本文中の`[[リンク]]`の参照先(`|`以降の表示名と`#`以降の見出しは除く)。
fn wikilinks(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("[[") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find("]]") else { break };
        let target = rest[..end].split(['|', '#']).next().unwrap_or("").trim();
        if !target.is_empty() && !target.contains('\n') {
            out.push(target.to_owned());
        }
        rest = &rest[end + 2..];
    }
    out
}

fn is_excluded(path: &str, category: &str) -> bool {
    EXCLUDED_PATHS.contains(&path)
        || category == CAPTURE_CATEGORY
        || path.split('/').any(|p| p.starts_with('.'))
}

/// ナレッジの更新検出用の指紋(ファイルのパス・サイズ・更新時刻)。中身は読まない。
pub fn fingerprint(root: &Path) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    fn walk(dir: &Path, depth: u8, h: &mut DefaultHasher) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() && depth < 4 {
                walk(&e.path(), depth + 1, h);
            } else if ft.is_file() && e.path().extension().is_some_and(|x| x == "md") {
                e.file_name().hash(h);
                if let Ok(m) = e.metadata() {
                    m.len().hash(h);
                    m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).hash(h);
                }
            }
        }
    }
    walk(root, 0, &mut h);
    h.finish()
}

impl Graph {
    /// ナレッジ全体から索引を作る。キャプチャ・訂正履歴・隠しフォルダは含めない。
    pub fn build(root: &Path) -> Result<Self, AppError> {
        let mut nodes = Vec::new();
        let mut links: Vec<Vec<String>> = Vec::new();
        let mut log_ids: Vec<Vec<String>> = Vec::new();
        for f in knowledge::list(root)? {
            if is_excluded(&f.path, &f.category) {
                continue;
            }
            let Ok(text) = knowledge::read(root, &f.path) else { continue };
            let (fm, body) = knowledge::parse(&text).unwrap_or_default();
            let body = if body.is_empty() && fm.title.is_empty() { text.as_str() } else { body };
            let head = format!("{} {} {}", f.title, fm.tags.join(" "), f.path.trim_end_matches(".md").replace(['/', '_'], " "));
            let body_head: String = body.chars().take(MATCH_BODY_CHARS).collect();
            nodes.push(Node {
                path: f.path,
                title: f.title,
                summary: summarize(body),
                tags: fm.tags.iter().map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty()).collect(),
                head_terms: terms(&head),
                body_terms: terms(&body_head),
            });
            links.push(wikilinks(body));
            log_ids.push(fm.source_log_ids);
        }

        let mut df: HashMap<String, usize> = HashMap::new();
        for n in &nodes {
            for t in n.head_terms.union(&n.body_terms) {
                *df.entry(t.clone()).or_default() += 1;
            }
        }

        let mut graph = Graph { edges: vec![Vec::new(); nodes.len()], nodes, df };
        graph.link_tags();
        graph.link_wikilinks(&links);
        graph.link_conversations(&log_ids);
        Ok(graph)
    }

    fn add_edge(&mut self, a: usize, b: usize, weight: f32, reason: Reason) {
        if a == b {
            return;
        }
        for (from, to) in [(a, b), (b, a)] {
            match self.edges[from].iter_mut().find(|e| e.to == to) {
                // 同じ組が複数の理由でつながるときは重みを足し、強い方の理由を残す
                Some(e) => {
                    if weight > e.weight {
                        e.reason = reason.clone();
                    }
                    e.weight += weight;
                }
                None => self.edges[from].push(Edge { to, weight, reason: reason.clone() }),
            }
        }
    }

    /// 共通タグでつなぐ。珍しいタグほど強く、全ノートが持つタグではつながない。
    fn link_tags(&mut self) {
        let n = self.nodes.len();
        let mut by_tag: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, node) in self.nodes.iter().enumerate() {
            for t in &node.tags {
                by_tag.entry(t.clone()).or_default().push(i);
            }
        }
        let mut tags: Vec<_> = by_tag.into_iter().filter(|(_, ids)| ids.len() >= 2 && ids.len() < n).collect();
        tags.sort();
        for (tag, ids) in tags {
            let weight = (n as f32 / ids.len() as f32).ln();
            for (k, &a) in ids.iter().enumerate() {
                for &b in &ids[k + 1..] {
                    self.add_edge(a, b, weight, Reason::Tag(tag.clone()));
                }
            }
        }
    }

    /// `[[リンク]]`でつなぐ。参照先はパス(拡張子の有無を問わない)・ファイル名・タイトルで探す。
    fn link_wikilinks(&mut self, links: &[Vec<String>]) {
        let find = |target: &str| -> Option<usize> {
            let t = target.trim_end_matches(".md").to_lowercase();
            self.nodes.iter().position(|n| {
                let p = n.path.trim_end_matches(".md").to_lowercase();
                p == t || p.rsplit('/').next() == Some(t.as_str()) || n.title.to_lowercase() == t
            })
        };
        let pairs: Vec<(usize, usize)> = links
            .iter()
            .enumerate()
            .flat_map(|(i, ls)| ls.iter().filter_map(|l| find(l)).map(move |j| (i, j)).collect::<Vec<_>>())
            .collect();
        for (a, b) in pairs {
            self.add_edge(a, b, 2.0, Reason::Link);
        }
    }

    /// 同じ会話ログから整理されたノート同士をつなぐ。
    fn link_conversations(&mut self, log_ids: &[Vec<String>]) {
        let mut by_log: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, ids) in log_ids.iter().enumerate() {
            for id in ids {
                by_log.entry(id.as_str()).or_default().push(i);
            }
        }
        let mut pairs: HashSet<(usize, usize)> = HashSet::new();
        for ids in by_log.values() {
            for (k, &a) in ids.iter().enumerate() {
                for &b in &ids[k + 1..] {
                    pairs.insert((a.min(b), a.max(b)));
                }
            }
        }
        let mut pairs: Vec<_> = pairs.into_iter().collect();
        pairs.sort();
        for (a, b) in pairs {
            self.add_edge(a, b, 1.0, Reason::SameConversation);
        }
    }

    fn idf(&self, term: &str) -> f32 {
        let n = self.nodes.len() as f32;
        let df = *self.df.get(term).unwrap_or(&0) as f32;
        1.0 + ((n + 1.0) / (df + 1.0)).ln()
    }

    /// 発話の語と一致するノートの得点(タイトル・タグの一致は本文の3倍)。
    /// 半数を超えるノートに出る語はありふれているため数えない(ノートが少ないうちは除く)。
    fn seed_scores(&self, query: &str) -> Vec<(usize, f32, Vec<String>)> {
        let q = terms(query);
        let common = (self.nodes.len() / 2).max(2);
        let mut out = Vec::new();
        for (i, n) in self.nodes.iter().enumerate() {
            let mut score = 0.0;
            let mut matched: Vec<&String> = Vec::new();
            for t in &q {
                if self.df.get(t).is_some_and(|&d| d > common) {
                    continue;
                }
                let w = if n.head_terms.contains(t) {
                    3.0
                } else if n.body_terms.contains(t) {
                    1.0
                } else {
                    continue;
                };
                score += w * self.idf(t);
                matched.push(t);
            }
            // 1語だけの本文一致(「設定」など)は偶然の一致が多いので起点にしない
            let head_hit = matched.iter().any(|t| n.head_terms.contains(*t));
            if matched.len() >= 2 || head_hit {
                matched.sort_by_key(|t| std::cmp::Reverse(t.chars().count()));
                out.push((i, score, matched.into_iter().take(3).cloned().collect()));
            }
        }
        out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        out.truncate(MAX_SEEDS);
        out
    }

    /// 発話から関連ノートを最大`max_nodes`件想起する。起点が見つからなければ空。
    pub fn recall(&self, query: &str, max_nodes: usize) -> Vec<Hit> {
        let seeds = self.seed_scores(query);
        let Some(top) = seeds.first().map(|s| s.1) else { return Vec::new() };
        let mut best: HashMap<usize, (f32, String)> = HashMap::new();
        let mut frontier: Vec<usize> = Vec::new();
        for (i, score, matched) in &seeds {
            best.insert(*i, (score / top, format!("キーワード一致: {}", matched.join("、"))));
            frontier.push(*i);
        }
        for _ in 0..MAX_HOPS {
            let mut next = Vec::new();
            for &u in &frontier {
                let a = best[&u].0;
                let max_w = self.edges[u].iter().map(|e| e.weight).fold(0.0, f32::max);
                if max_w <= 0.0 {
                    continue;
                }
                for e in &self.edges[u] {
                    let act = a * HOP_DECAY * (e.weight / max_w);
                    if act < MIN_ACTIVATION || best.get(&e.to).is_some_and(|(b, _)| *b >= act) {
                        continue;
                    }
                    let via = &self.nodes[u].title;
                    let why = match &e.reason {
                        Reason::Tag(t) => format!("「{via}」とタグ「{t}」を共有"),
                        Reason::Link => format!("「{via}」とリンク"),
                        Reason::SameConversation => format!("「{via}」と同じ会話から整理"),
                    };
                    best.insert(e.to, (act, why));
                    next.push(e.to);
                }
            }
            frontier = next;
        }
        let mut hits: Vec<Hit> = best
            .into_iter()
            .map(|(i, (score, reason))| {
                let n = &self.nodes[i];
                Hit { path: n.path.clone(), title: n.title.clone(), summary: n.summary.clone(), reason, score }
            })
            .collect();
        hits.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
        hits.truncate(max_nodes);
        hits
    }
}

/// 想起したノートを、システムプロンプトに載せる行にする(合計`max_chars`文字まで)。
pub fn render(hits: &[Hit], max_chars: usize) -> String {
    let mut out = String::new();
    for h in hits {
        let line = format!("- {} | {} — {} ({})\n", h.path, h.title, h.summary, h.reason);
        if out.chars().count() + line.chars().count() > max_chars {
            break;
        }
        out.push_str(&line);
    }
    out
}

/// 本文の末尾(最新の追記)を`max_chars`文字まで返す。ツールを使えない経路で最上位の1件だけ同梱する。
pub fn excerpt(root: &Path, path: &str, max_chars: usize) -> Option<String> {
    let text = knowledge::read(root, path).ok()?;
    let body = knowledge::parse(&text).map_or(text.as_str(), |(_, b)| b).trim();
    let skip = body.chars().count().saturating_sub(max_chars);
    let tail = body.char_indices().nth(skip).map_or(body, |(i, _)| &body[i..]);
    (!tail.is_empty()).then(|| tail.to_owned())
}

/// 索引のキャッシュ。ナレッジの指紋が変わったときだけ作り直す(Obsidian等での直接編集にも追従する)。
#[derive(Default)]
pub struct GraphCache(Mutex<Option<(u64, Arc<Graph>)>>);

impl GraphCache {
    pub fn get(&self, root: &Path) -> Result<Arc<Graph>, AppError> {
        let fp = fingerprint(root);
        let mut slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((saved, g)) = slot.as_ref()
            && *saved == fp
        {
            return Ok(g.clone());
        }
        let g = Arc::new(Graph::build(root)?);
        *slot = Some((fp, g.clone()));
        Ok(g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::knowledge::{Entry, upsert};

    fn note(root: &Path, cat: &str, file: &str, title: &str, tags: &[&str], content: &str, ids: &[&str]) {
        let tags: Vec<String> = tags.iter().map(|s| s.to_string()).collect();
        let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        upsert(root, &Entry { category: cat, filename: file, title, tags: &tags, content, source_log_ids: &ids }, "2026-10-29T00:00:00Z").unwrap();
    }

    #[test]
    fn terms_split_words_and_bigrams() {
        let t = terms("Tauriの設定を確認します");
        assert!(t.contains("tauri"));
        assert!(t.contains("設定"));
        // ひらがなだけの組は捨てる
        assert!(!t.contains("しま") && !t.contains("ます"));
        assert!(terms("a 1").is_empty());
    }

    #[test]
    fn wikilinks_strip_alias_and_heading() {
        assert_eq!(wikilinks("see [[Dev/a|A]] and [[b#h]] [[ ]] [[x"), ["Dev/a", "b"]);
    }

    #[test]
    fn recall_follows_tags_up_to_two_hops() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("k");
        note(&root, "Development", "tauri_setup", "Tauriのセットアップ", &["tauri", "rust"], "Tauri 2 のビルド手順", &[]);
        note(&root, "Development", "rust_errors", "Rustのエラー処理", &["rust", "error"], "thiserror を使う", &[]);
        note(&root, "Research", "error_logging", "ログ設計", &["error", "logging"], "構造化ログにする", &[]);
        note(&root, "Ideas", "cooking", "夕飯の献立", &["food"], "カレーを作る", &[]);
        note(&root, "Ideas", "garden", "庭の手入れ", &["food"], "トマトを植える", &[]);
        let g = Graph::build(&root).unwrap();
        let hits = g.recall("Tauriのビルドでつまずいた", 5);
        let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(paths[0], "Development/tauri_setup.md");
        assert!(hits[0].reason.starts_with("キーワード一致"));
        // 1ホップ(タグ rust)と2ホップ(タグ error)
        assert!(paths.contains(&"Development/rust_errors.md"));
        assert!(hits.iter().any(|h| h.path == "Development/rust_errors.md" && h.reason.contains("rust")));
        assert!(paths.contains(&"Research/error_logging.md"));
        // つながりのないノートは出さない
        assert!(!paths.contains(&"Ideas/cooking.md"));
        assert!(g.recall("全然関係ない話題", 5).is_empty());
        assert_eq!(g.recall("Tauriのビルド", 1).len(), 1);
    }

    #[test]
    fn links_and_conversations_connect_notes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("k");
        note(&root, "Development", "deploy", "デプロイ手順", &[], "[[Research/ci]] を参照", &[]);
        note(&root, "Research", "ci", "CIの比較", &[], "GitHub Actions を採用", &["log1"]);
        note(&root, "Ideas", "release_note", "リリースノート案", &[], "毎週まとめる", &["log1"]);
        let g = Graph::build(&root).unwrap();
        let hits = g.recall("デプロイ手順を見直したい", 5);
        let ci = hits.iter().find(|h| h.path == "Research/ci.md").unwrap();
        assert!(ci.reason.contains("リンク"));
        let rel = hits.iter().find(|h| h.path == "Ideas/release_note.md").unwrap();
        assert!(rel.reason.contains("同じ会話"));
    }

    #[test]
    fn captures_corrections_and_hidden_are_excluded() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("k");
        knowledge::append_capture(&root, "sid12345", "Tauriの話", "はい", "2026-10-29T00:00:00Z").unwrap();
        knowledge::add_correction(&root, "タウリ", "Tauri", "2026-10-29T00:00:00Z").unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::write(root.join(".hidden/tauri.md"), "---\ntitle: Tauri\n---\nTauri").unwrap();
        let g = Graph::build(&root).unwrap();
        assert!(g.nodes.is_empty());
        assert!(g.recall("Tauri", 5).is_empty());
    }

    #[test]
    fn render_respects_budget_and_cache_tracks_changes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("k");
        note(&root, "Dev", "a", "Alpha", &[], &"本文".repeat(200), &[]);
        let cache = GraphCache::default();
        let g1 = cache.get(&root).unwrap();
        assert!(Arc::ptr_eq(&g1, &cache.get(&root).unwrap()));
        let hits = g1.recall("Alpha", 5);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].summary.chars().count() <= SUMMARY_CHARS + 1);
        assert!(render(&hits, 10).is_empty());
        assert!(render(&hits, 1000).starts_with("- Dev/a.md | Alpha"));
        assert!(excerpt(&root, "Dev/a.md", 10).unwrap().chars().count() <= 10);

        note(&root, "Dev", "b", "Beta", &[], "x", &[]);
        let g2 = cache.get(&root).unwrap();
        assert!(!Arc::ptr_eq(&g1, &g2));
        assert_eq!(g2.recall("Beta", 5)[0].path, "Dev/b.md");
    }
}
