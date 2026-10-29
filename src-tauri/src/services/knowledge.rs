//! Markdownナレッジの読み書き。すべてのパスは`utils::path`のサンドボックスを通す。
use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::utils::{
    error::AppError,
    path::{category_parts, ensure_dir, resolve_existing, resolve_new_file, slug},
};

fn io(e: impl std::fmt::Display) -> AppError {
    AppError::Io(e.to_string())
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frontmatter {
    pub title: String,
    pub category: String,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub source_log_ids: Vec<String>,
}

fn json<T: Serialize + ?Sized>(v: &T) -> String {
    // JSON文字列/配列はYAMLのflow記法としても有効
    serde_json::to_string(v).unwrap_or_default()
}

impl Frontmatter {
    fn render(&self) -> String {
        format!(
            "---\ntitle: {}\ncategory: {}\ntags: {}\ncreated_at: {}\nupdated_at: {}\nsource_log_ids: {}\n---\n",
            json(&self.title),
            json(&self.category),
            json(&self.tags),
            json(&self.created_at),
            json(&self.updated_at),
            json(&self.source_log_ids),
        )
    }
}

/// YAMLスカラー(JSON文字列・引用符付き・裸の文字列)を文字列にする。
fn scalar(v: &str) -> String {
    let v = v.trim();
    serde_json::from_str::<String>(v).unwrap_or_else(|_| {
        v.trim_matches(|c| c == '\'' || c == '"').to_owned()
    })
}

/// `["a","b"]` / `[a, b]` / `a` の形のインライン値をリストにする。
fn inline_list(v: &str) -> Vec<String> {
    let v = v.trim();
    if let Ok(list) = serde_json::from_str::<Vec<String>>(v) {
        return list;
    }
    match v.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
        Some(inner) => inner.split(',').map(scalar).filter(|s| !s.is_empty()).collect(),
        None if v.is_empty() => Vec::new(),
        None => vec![scalar(v)],
    }
}

/// Frontmatterと本文に分ける。Obsidian等で手編集されても読めるよう、
/// 引用符なしの値やブロック形式のリスト(`- item`)も受け付け、未知のキーは無視する。
/// Frontmatter自体がなければ`None`。
pub fn parse(text: &str) -> Option<(Frontmatter, &str)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let text = text.strip_prefix("---\r\n").or_else(|| text.strip_prefix("---\n"))?;
    let (head, body) = match text.find("\n---\n") {
        Some(end) => (&text[..end], &text[end + 5..]),
        None => {
            let t = text.strip_suffix("\n---")?;
            (t, "")
        }
    };
    let mut fm = Frontmatter::default();
    let mut list_key: Option<&str> = None;
    for line in head.lines() {
        let line = line.trim_end();
        if let Some(item) = line.trim_start().strip_prefix("- ") {
            let item = scalar(item);
            match list_key {
                Some("tags") => fm.tags.push(item),
                Some("source_log_ids") => fm.source_log_ids.push(item),
                _ => {}
            }
            continue;
        }
        let Some((k, v)) = line.split_once(':') else { continue };
        let k = k.trim();
        list_key = Some(k).filter(|_| v.trim().is_empty());
        match k {
            "title" => fm.title = scalar(v),
            "category" => fm.category = scalar(v),
            "tags" => fm.tags = inline_list(v),
            "created_at" => fm.created_at = scalar(v),
            "updated_at" => fm.updated_at = scalar(v),
            "source_log_ids" => fm.source_log_ids = inline_list(v),
            _ => {}
        }
    }
    Some((fm, body.trim_start_matches(['\r', '\n'])))
}

/// 一時ファイル経由で書き込み、途中失敗でファイルを壊さない。
fn atomic_write(path: &Path, content: &str) -> Result<(), AppError> {
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, content).map_err(io)?;
    fs::rename(&tmp, path).map_err(io)
}

pub struct Entry<'a> {
    pub category: &'a str,
    pub filename: &'a str,
    pub title: &'a str,
    pub tags: &'a [String],
    pub content: &'a str,
    pub source_log_ids: &'a [String],
}

/// 既存ファイルがあれば追記・メタデータ更新、なければ新規作成する。書き込んだ相対パスを返す。
pub fn upsert(root: &Path, e: &Entry, now: &str) -> Result<String, AppError> {
    let parts = category_parts(e.category);
    let dir = ensure_dir(root, &parts)?;
    let name = match slug(e.filename, 60) {
        s if s.is_empty() => "note".to_owned(),
        s => s,
    };
    let path = dir.join(format!("{name}.md"));
    let category = parts.join("/");
    let section = format!("\n## {now}\n\n{}\n", e.content.trim());

    let doc = match fs::read_to_string(&path) {
        Ok(existing) => {
            let (mut fm, body) = parse(&existing).unwrap_or_else(|| {
                // 手編集等でFrontmatterが壊れていても本文は保持する
                (
                    Frontmatter { title: e.title.to_owned(), category: category.clone(), created_at: now.to_owned(), ..Default::default() },
                    existing.as_str(),
                )
            });
            for t in e.tags {
                if !fm.tags.contains(t) {
                    fm.tags.push(t.clone());
                }
            }
            for id in e.source_log_ids {
                if !fm.source_log_ids.contains(id) {
                    fm.source_log_ids.push(id.clone());
                }
            }
            fm.updated_at = now.to_owned();
            format!("{}{}{}", fm.render(), body.trim_end_matches('\n'), section)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let fm = Frontmatter {
                title: e.title.to_owned(),
                category,
                tags: e.tags.to_vec(),
                created_at: now.to_owned(),
                updated_at: now.to_owned(),
                source_log_ids: e.source_log_ids.to_vec(),
            };
            format!("{}\n# {}\n{}", fm.render(), e.title, section)
        }
        Err(err) => return Err(io(err)),
    };
    atomic_write(&path, &doc)?;
    let root = root.canonicalize().map_err(io)?;
    let rel = path.strip_prefix(&root).map_err(|_| AppError::PathNotAllowed)?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeFile {
    /// ナレッジルートからの相対パス(`/`区切り)
    pub path: String,
    pub title: String,
    pub category: String,
    pub updated_at: String,
}

/// 既存ファイル一覧(最大3階層)。シンボリックリンクは辿らない。
pub fn list(root: &Path) -> Result<Vec<KnowledgeFile>, AppError> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    let root = root.canonicalize().map_err(io)?;
    walk(&root, &root, 0, &mut out)?;
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(out)
}

fn walk(root: &Path, dir: &Path, depth: u8, out: &mut Vec<KnowledgeFile>) -> Result<(), AppError> {
    if depth > 3 {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(io)? {
        let entry = entry.map_err(io)?;
        let ft = entry.file_type().map_err(io)?;
        let path: PathBuf = entry.path();
        if ft.is_dir() {
            walk(root, &path, depth + 1, out)?;
        } else if ft.is_file() && path.extension().is_some_and(|x| x == "md") {
            let text = fs::read_to_string(&path).unwrap_or_default();
            let rel = path.strip_prefix(root).map_err(|_| AppError::PathNotAllowed)?;
            let rel_s = rel.to_string_lossy().replace('\\', "/");
            let (title, category, updated_at) = match parse(&text) {
                Some((fm, _)) => (fm.title, fm.category, fm.updated_at),
                None => (
                    path.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
                    rel.parent().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default(),
                    String::new(),
                ),
            };
            out.push(KnowledgeFile { path: rel_s, title, category, updated_at });
        }
    }
    Ok(())
}

/// エクスポート・インポート・初期化の結果。
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct TransferReport {
    /// 処理したファイル数
    pub files: usize,
    /// 既存と重複、または安全でない名前のため飛ばした数(インポートのみ)
    pub skipped: usize,
    /// エクスポート先のフォルダ(エクスポートのみ)
    pub dest: Option<String>,
}

/// インポートするファイル1件あたりの上限。
const MAX_IMPORT_BYTES: u64 = 5 * 1024 * 1024;

/// ナレッジ全体を`dest`直下の新しいフォルダへコピーする。元のファイルは変更しない。
pub fn export(root: &Path, dest: &Path, stamp: &str) -> Result<TransferReport, AppError> {
    let out = dest.join(format!("MyAssistant-knowledge-{stamp}"));
    fs::create_dir_all(&out).map_err(io)?;
    let mut report = TransferReport { dest: Some(out.to_string_lossy().into_owned()), ..Default::default() };
    for f in list(root)? {
        let src = resolve_existing(root, &f.path)?;
        let to = out.join(&f.path);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(io)?;
        }
        fs::copy(&src, &to).map_err(io)?;
        report.files += 1;
    }
    Ok(report)
}

/// `src`フォルダ内のMarkdownをナレッジへ取り込む。既存ファイルは上書きしない。
/// 書き込み先は`resolve_new_file`を通すため、安全でない名前のファイルは飛ばす。
pub fn import(root: &Path, src: &Path) -> Result<TransferReport, AppError> {
    let mut report = TransferReport::default();
    for f in list(src)? {
        let from = resolve_existing(src, &f.path)?;
        let too_big = fs::metadata(&from).map_err(io)?.len() > MAX_IMPORT_BYTES;
        let to = match resolve_new_file(root, &f.path) {
            Ok(to) if !too_big && !to.exists() => to,
            _ => {
                report.skipped += 1;
                continue;
            }
        };
        let text = fs::read_to_string(&from).map_err(io)?;
        atomic_write(&to, &text)?;
        report.files += 1;
    }
    Ok(report)
}

/// ナレッジ内のMarkdownをすべて削除し、空になったフォルダを片付ける。
/// 保存先がObsidianのvault等でも、Markdown以外のファイルや設定には触れない。
pub fn reset(root: &Path) -> Result<TransferReport, AppError> {
    let mut report = TransferReport::default();
    if !root.exists() {
        return Ok(report);
    }
    for f in list(root)? {
        fs::remove_file(resolve_existing(root, &f.path)?).map_err(io)?;
        report.files += 1;
    }
    remove_empty_dirs(&root.canonicalize().map_err(io)?, 0);
    Ok(report)
}

/// ルート自身は残し、空のサブフォルダだけ削除する(失敗は無視)。
fn remove_empty_dirs(dir: &Path, depth: u8) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            remove_empty_dirs(&entry.path(), depth + 1);
            let _ = fs::remove_dir(entry.path());
        }
    }
}

/// 会話をそのまま書き出すキャプチャのカテゴリ。整理でカテゴライズされたナレッジとは分けて扱う。
pub const CAPTURE_CATEGORY: &str = "Captures";

/// セッションのキャプチャファイル。ファイル名は`日付_セッションID先頭8文字.md`で、IDで探す。
fn capture_path(dir: &Path, sid: &str) -> Option<PathBuf> {
    let suffix = format!("_{sid}.md");
    fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(&suffix))
    })
}

fn short_session(session_id: &str) -> String {
    match slug(session_id, 8) {
        s if s.is_empty() => "session".to_owned(),
        s => s,
    }
}

/// 1往復の会話を、セッションごとのキャプチャへチャプターとして追記する。LLMは使わない。
pub fn append_capture(root: &Path, session_id: &str, user: &str, assistant: &str, now: &str) -> Result<(), AppError> {
    let sid = short_session(session_id);
    let dir = ensure_dir(root, &[CAPTURE_CATEGORY.to_owned()])?;
    let date = now.get(..10).unwrap_or(now);
    let chapter = format!(
        "\n## {}\n\n**ユーザー**\n\n{}\n\n**アシスタント**\n\n{}\n",
        now.get(11..16).unwrap_or(now),
        user.trim(),
        assistant.trim()
    );
    let (path, doc) = match capture_path(&dir, &sid) {
        Some(path) => {
            let existing = fs::read_to_string(&path).map_err(io)?;
            let (mut fm, body) = parse(&existing).unwrap_or_else(|| {
                (
                    Frontmatter { title: format!("会話キャプチャ {date}"), category: CAPTURE_CATEGORY.into(), created_at: now.to_owned(), ..Default::default() },
                    existing.as_str(),
                )
            });
            fm.updated_at = now.to_owned();
            let doc = format!("{}{}{}", fm.render(), body.trim_end_matches('\n'), chapter);
            (path, doc)
        }
        None => {
            let title = format!("会話キャプチャ {date}");
            let fm = Frontmatter {
                title: title.clone(),
                category: CAPTURE_CATEGORY.into(),
                tags: vec!["capture".into()],
                created_at: now.to_owned(),
                updated_at: now.to_owned(),
                source_log_ids: vec![],
            };
            (dir.join(format!("{date}_{sid}.md")), format!("{}\n# {title}\n{chapter}", fm.render()))
        }
    };
    atomic_write(&path, &doc)
}

/// 整理が済んだセッションのキャプチャを削除する(なければ何もしない)。
pub fn remove_capture(root: &Path, session_id: &str) -> Result<(), AppError> {
    let dir = ensure_dir(root, &[CAPTURE_CATEGORY.to_owned()])?;
    match capture_path(&dir, &short_session(session_id)) {
        Some(path) => fs::remove_file(path).map_err(io),
        None => Ok(()),
    }
}

/// 新しい会話の文脈復元用に、直近更新されたファイルの末尾(最新の追記)を連結して返す。
/// `max_files`件・合計`max_chars`文字まで。該当がなければ`None`。
pub fn recent_context(root: &Path, max_files: usize, max_chars: usize) -> Result<Option<String>, AppError> {
    let mut out = String::new();
    let mut left = max_chars;
    // 訂正履歴は専用の枠でシステムプロンプトへ入れるため、ここでは除く
    for f in list(root)?.into_iter().filter(|f| f.path != CORRECTIONS_PATH && f.category != CAPTURE_CATEGORY).take(max_files) {
        if left == 0 {
            break;
        }
        let Ok(text) = read(root, &f.path) else { continue };
        let body = parse(&text).map_or(text.as_str(), |(_, b)| b).trim();
        if body.is_empty() {
            continue;
        }
        // 追記は末尾に積まれるため、長い場合は末尾側を残す
        let budget = left.min(max_chars / 2);
        let skip = body.chars().count().saturating_sub(budget);
        let tail = match body.char_indices().nth(skip) {
            Some((i, _)) => &body[i..],
            None => body,
        };
        out.push_str(&format!("### {}/{} ({})\n{}\n\n", f.category, f.title, f.updated_at, tail));
        left = left.saturating_sub(tail.chars().count());
    }
    Ok((!out.is_empty()).then_some(out))
}

/// 音声認識の訂正履歴を書き込むファイル(ナレッジルートからの相対パス)。
pub const CORRECTIONS_PATH: &str = "Preferences/stt_corrections.md";
const CORRECTIONS_LINE: &str = "- 「";

/// 先頭・末尾の共通部分を除いた、異なる区間(聞き取り, 正しい)を返す。
fn diff_span(heard: &str, correct: &str) -> (String, String) {
    let (h, c): (Vec<char>, Vec<char>) = (heard.chars().collect(), correct.chars().collect());
    let max = h.len().min(c.len());
    let pre = (0..max).take_while(|&i| h[i] == c[i]).count();
    let suf = (0..max - pre).take_while(|&i| h[h.len() - 1 - i] == c[c.len() - 1 - i]).count();
    (h[pre..h.len() - suf].iter().collect(), c[pre..c.len() - suf].iter().collect())
}

fn one_line(s: &str, max_chars: usize) -> String {
    s.replace(['\n', '\r'], " ").trim().chars().take(max_chars).collect()
}

/// 聞き取り間違いの訂正を`Preferences/stt_corrections.md`へ追記する。
/// 発話全体(`heard`→`correct`)の差分区間を「誤→正」として残し、元の文を例として添える。
/// 差分がない・すでに同じ訂正がある場合は書かずに`false`を返す。Obsidianで直接編集しても読み戻せる。
pub fn add_correction(root: &Path, heard: &str, correct: &str, now: &str) -> Result<bool, AppError> {
    let (h, c) = diff_span(heard.trim(), correct.trim());
    let (h, c) = (one_line(&h, 100), one_line(&c, 100));
    if h.is_empty() && c.is_empty() {
        return Ok(false);
    }
    let key = format!("{CORRECTIONS_LINE}{h}」→「{c}」");
    let line = format!("{key} — 例: {} ({})", one_line(correct, 200), now.get(..10).unwrap_or(now));

    let dir = ensure_dir(root, &["Preferences".to_owned()])?;
    let path = dir.join("stt_corrections.md");
    let doc = match fs::read_to_string(&path) {
        Ok(existing) => {
            let (mut fm, body) = parse(&existing).unwrap_or_else(|| {
                (
                    Frontmatter { title: "音声認識の訂正履歴".into(), category: "Preferences".into(), created_at: now.to_owned(), ..Default::default() },
                    existing.as_str(),
                )
            });
            if body.lines().any(|l| l.starts_with(&key)) {
                return Ok(false);
            }
            fm.updated_at = now.to_owned();
            format!("{}{}\n{}\n", fm.render(), body.trim_end_matches('\n'), line)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let fm = Frontmatter {
                title: "音声認識の訂正履歴".into(),
                category: "Preferences".into(),
                tags: vec!["stt".into(), "correction".into()],
                created_at: now.to_owned(),
                updated_at: now.to_owned(),
                source_log_ids: vec![],
            };
            format!(
                "{}\n# 音声認識の訂正履歴\n\n聞き取り間違いをユーザーが訂正した記録です(「誤」→「正」)。\n\n{line}\n",
                fm.render()
            )
        }
        Err(e) => return Err(io(e)),
    };
    atomic_write(&path, &doc)?;
    Ok(true)
}

/// 訂正履歴の直近`max`件(「誤→正」の行)。なければ`None`。
/// プロンプトに載せるため、例文と日付は落として「誤」→「正」の組だけにする。
pub fn recent_corrections(root: &Path, max: usize) -> Option<String> {
    let text = read(root, CORRECTIONS_PATH).ok()?;
    let body = parse(&text).map_or(text.as_str(), |(_, b)| b);
    let lines: Vec<&str> = body
        .lines()
        .filter(|l| l.starts_with(CORRECTIONS_LINE))
        .map(|l| l.split_once(" — ").map_or(l, |(pair, _)| pair))
        .collect();
    let tail = &lines[lines.len().saturating_sub(max)..];
    (!tail.is_empty()).then(|| tail.join("\n"))
}

/// 訂正履歴の直近`max`件から、訂正後の語(「正」側)を新しい順・重複なしで取り出す。音声認識の語彙ヒント用。
pub fn recent_correction_terms(root: &Path, max: usize) -> Vec<String> {
    let Ok(text) = read(root, CORRECTIONS_PATH) else {
        return Vec::new();
    };
    let body = parse(&text).map_or(text.as_str(), |(_, b)| b);
    let mut out: Vec<String> = Vec::new();
    for line in body.lines().rev().filter(|l| l.starts_with(CORRECTIONS_LINE)) {
        let term = line.split_once("」→「").and_then(|(_, rest)| rest.split_once('」')).map_or("", |(c, _)| c.trim());
        if !term.is_empty() && !out.iter().any(|t| t == term) {
            out.push(term.to_owned());
            if out.len() == max {
                break;
            }
        }
    }
    out
}

pub fn read(root: &Path, rel: &str) -> Result<String, AppError> {
    if !rel.ends_with(".md") {
        return Err(AppError::PathNotAllowed);
    }
    fs::read_to_string(resolve_existing(root, rel)?).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry<'a>(cat: &'a str, file: &'a str, content: &'a str, ids: &'a [String]) -> Entry<'a> {
        Entry { category: cat, filename: file, title: "T", tags: &[], content, source_log_ids: ids }
    }

    #[test]
    fn create_then_append_merges_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        let ids1 = vec!["a".to_owned()];
        let rel = upsert(&root, &Entry { tags: &["Rust".to_owned()], ..entry("Development", "tauri_setup", "first", &ids1) }, "2026-10-27T00:00:00Z").unwrap();
        assert_eq!(rel, "Development/tauri_setup.md");
        let ids2 = vec!["a".to_owned(), "b".to_owned()];
        upsert(&root, &Entry { tags: &["Rust".to_owned(), "Tauri".to_owned()], ..entry("Development", "tauri_setup", "second", &ids2) }, "2026-10-27T01:00:00Z").unwrap();
        let text = read(&root, &rel).unwrap();
        let (fm, body) = parse(&text).unwrap();
        assert_eq!(fm.created_at, "2026-10-27T00:00:00Z");
        assert_eq!(fm.updated_at, "2026-10-27T01:00:00Z");
        assert_eq!(fm.tags, ["Rust", "Tauri"]);
        assert_eq!(fm.source_log_ids, ["a", "b"]);
        assert!(body.contains("first") && body.contains("second"));
        assert_eq!(list(&root).unwrap().len(), 1);
    }

    #[test]
    fn parses_obsidian_style_frontmatter() {
        let text = "---\ntitle: Tauri memo\ncategory: 'Development'\ntags:\n  - Rust\n  - \"Tauri\"\nupdated_at: 2026-10-27T01:00:00Z\naliases: [x]\n---\n\n# Body\nhello\n";
        let (fm, body) = parse(text).unwrap();
        assert_eq!((fm.title.as_str(), fm.category.as_str()), ("Tauri memo", "Development"));
        assert_eq!(fm.tags, ["Rust", "Tauri"]);
        assert_eq!(fm.updated_at, "2026-10-27T01:00:00Z");
        assert!(body.starts_with("# Body"));
        assert!(parse("# no frontmatter").is_none());
    }

    #[test]
    fn recent_context_prefers_latest_and_truncates_tail() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        assert_eq!(recent_context(&root, 5, 1000).unwrap(), None);
        let ids: Vec<String> = vec![];
        upsert(&root, &entry("Ideas", "old", "old note", &ids), "2026-10-01T00:00:00Z").unwrap();
        upsert(&root, &entry("Development", "new", &"あ".repeat(500), &ids), "2026-10-02T00:00:00Z").unwrap();
        let ctx = recent_context(&root, 5, 100).unwrap().unwrap();
        assert!(ctx.find("Development").unwrap() < ctx.find("Ideas").unwrap_or(usize::MAX));
        assert!(ctx.chars().count() < 200);
    }

    #[test]
    fn corrections_are_recorded_once_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        assert!(recent_corrections(&root, 10).is_none());
        assert!(add_correction(&root, "タウリのセットアップ", "Tauriのセットアップ", "2026-10-27T00:00:00Z").unwrap());
        assert!(!add_correction(&root, "タウリのセットアップ", "Tauriのセットアップ", "2026-10-27T01:00:00Z").unwrap());
        assert!(!add_correction(&root, "同じ", "同じ", "2026-10-27T01:00:00Z").unwrap());
        assert!(add_correction(&root, "ラストを学ぶ", "Rustを学ぶ", "2026-10-27T02:00:00Z").unwrap());
        let text = read(&root, CORRECTIONS_PATH).unwrap();
        assert!(parse(&text).is_some());
        let got = recent_corrections(&root, 10).unwrap();
        assert_eq!(got, "- 「タウリ」→「Tauri」\n- 「ラスト」→「Rust」");
        assert_eq!(recent_corrections(&root, 1).unwrap().lines().count(), 1);
        // 訂正後の語を新しい順に取り出す
        assert_eq!(recent_correction_terms(&root, 10), ["Rust", "Tauri"]);
        assert_eq!(recent_correction_terms(&root, 1), ["Rust"]);
        assert!(recent_correction_terms(&dir.path().join("none"), 10).is_empty());
    }

    #[test]
    fn diff_span_trims_common_edges() {
        assert_eq!(diff_span("今日はラストの話", "今日はRustの話"), ("ラスト".into(), "Rust".into()));
        assert_eq!(diff_span("abc", "abc"), (String::new(), String::new()));
        assert_eq!(diff_span("あ", "あい"), (String::new(), "い".into()));
    }

    #[test]
    fn hostile_names_stay_inside_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        let rel = upsert(&root, &entry("../../outside", "../../evil", "x", &[]), "now").unwrap();
        assert_eq!(rel, "outside/evil.md");
        assert!(root.join("outside/evil.md").exists());
        assert!(!dir.path().join("evil.md").exists());
        assert!(read(&root, "../x.md").is_err());
        assert!(read(&root, "outside/evil.txt").is_err());
    }

    #[test]
    fn title_with_quotes_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("k");
        let e = Entry { title: "He said \"hi\": ok\nnext", ..entry("A", "f", "x", &[]) };
        let rel = upsert(&root, &e, "now").unwrap();
        let text = read(&root, &rel).unwrap();
        assert_eq!(parse(&text).unwrap().0.title, "He said \"hi\": ok\nnext");
    }

    #[test]
    fn export_import_reset_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        upsert(&root, &entry("Dev", "a", "x", &[]), "now").unwrap();
        upsert(&root, &entry("Dev/Rust", "b", "y", &[]), "now").unwrap();
        fs::write(root.join("keep.txt"), "not markdown").unwrap();

        let dest = dir.path().join("out");
        let r = export(&root, &dest, "t").unwrap();
        assert_eq!(r.files, 2);
        let exported = dest.join("MyAssistant-knowledge-t");
        assert!(exported.join("Dev/Rust/b.md").exists());

        let r = reset(&root).unwrap();
        assert_eq!(r.files, 2);
        assert!(list(&root).unwrap().is_empty());
        assert!(!root.join("Dev").exists());
        assert!(root.join("keep.txt").exists());

        let r = import(&root, &exported).unwrap();
        assert_eq!((r.files, r.skipped), (2, 0));
        // 再インポートは既存を上書きせず飛ばす
        let r = import(&root, &exported).unwrap();
        assert_eq!((r.files, r.skipped), (0, 2));
        assert_eq!(list(&root).unwrap().len(), 2);
    }

    #[test]
    fn capture_appends_chapters_and_is_removed_when_organized() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("k");
        append_capture(&root, "abcdef123456", "こんにちは", "やあ", "2026-10-27T12:00:00Z").unwrap();
        append_capture(&root, "abcdef123456", "次", "はい", "2026-10-27T12:05:00Z").unwrap();
        let files = list(&root).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "Captures/2026-10-27_abcdef12.md");
        let text = read(&root, &files[0].path).unwrap();
        assert!(text.contains("## 12:00") && text.contains("## 12:05"));

        // 文脈復元にはキャプチャを含めない
        assert!(recent_context(&root, 5, 1000).unwrap().is_none());

        remove_capture(&root, "abcdef123456").unwrap();
        assert!(list(&root).unwrap().is_empty());
        remove_capture(&root, "abcdef123456").unwrap();
        // 削除後の発話は新しいキャプチャになる
        append_capture(&root, "abcdef123456", "さらに", "うん", "2026-10-28T09:00:00Z").unwrap();
        assert_eq!(list(&root).unwrap()[0].path, "Captures/2026-10-28_abcdef12.md");
    }
}
