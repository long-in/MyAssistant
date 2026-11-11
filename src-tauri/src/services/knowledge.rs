//! Markdownナレッジの読み書き。すべてのパスは`utils::path`のサンドボックスを通す。
use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::utils::{
    error::AppError,
    path::{category_parts, ensure_dir, lower_dirs, resolve_existing, resolve_new_file, slug},
};

fn io(e: impl std::fmt::Display) -> AppError {
    AppError::Io(e.to_string())
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frontmatter {
    pub title: String,
    /// 1行の現在の要約。整理の統合で書き直し、想起の一覧に使う。空なら書き出さない
    pub summary: String,
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
    pub(crate) fn render(&self) -> String {
        let summary = if self.summary.is_empty() { String::new() } else { format!("summary: {}\n", json(&self.summary)) };
        format!(
            "---\ntitle: {}\n{summary}category: {}\ntags: {}\ncreated_at: {}\nupdated_at: {}\nsource_log_ids: {}\n---\n",
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

/// Frontmatterと本文に分ける。手編集されても読めるよう、
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
        list_key = v.trim().is_empty().then_some(k);
        match k {
            "title" => fm.title = scalar(v),
            "summary" => fm.summary = scalar(v),
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
pub(crate) fn atomic_write(path: &Path, content: &str) -> Result<(), AppError> {
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, content).map_err(io)?;
    fs::rename(&tmp, path).map_err(io)
}

/// 旧形式(日時見出しの追記)のノートの内容。旧形式からの移行を確かめるテストで使う。
#[cfg(test)]
pub struct Entry<'a> {
    pub category: &'a str,
    pub filename: &'a str,
    pub title: &'a str,
    pub tags: &'a [String],
    pub content: &'a str,
    pub source_log_ids: &'a [String],
}

/// 旧形式のノートを書く(既存ファイルがあれば日時見出しの節を追記、なければ新規作成)。テスト用。
#[cfg(test)]
pub fn upsert(root: &Path, e: &Entry, now: &str) -> Result<String, AppError> {
    let parts = category_parts(e.category);
    reject_read_only(&parts[0])?;
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
                ..Default::default()
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

// ---------- 統合型のノート(現在の内容 + 経緯) ----------

/// 経緯の節の見出し。これより前が「現在の内容」(整理で書き直す)、以降が「経緯」(追記のみ)。
pub const HISTORY_HEADING: &str = "## 経緯";
/// 書き換え前の版を退避するフォルダ。隠しフォルダなので一覧・グラフ・LLMのツールからは見えない。
const VERSIONS_DIR: [&str; 2] = [".my-assistant", "history"];
/// ノートごとに残す版の数。
const VERSIONS_KEEP: usize = 10;
/// これ以上の長さの現在の内容が半分未満に縮む書き換えは採らない(要約しすぎて情報を落とすのを防ぐ)。
const SHRINK_MIN_CHARS: usize = 400;
/// Frontmatterの要約の最大文字数。
const SUMMARY_MAX: usize = 200;

/// ノートの本文を分けたもの。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NoteParts {
    pub fm: Frontmatter,
    /// タイトル行(`# …`)を除いた、経緯より前の本文
    pub current: String,
    /// 経緯の節の中身(見出しを除く)
    pub history: String,
}

/// 旧形式(`upsert`の追記)の節見出し`## <RFC3339>`か。
fn is_stamp_heading(line: &str) -> bool {
    line.strip_prefix("## ").is_some_and(|t| chrono::DateTime::parse_from_rfc3339(t.trim()).is_ok())
}

/// 本文を現在の内容と経緯に分ける。経緯の見出しが無い旧形式は、最初の日時見出し以降を経緯とし、見出しを1段下げる。
pub fn split_body(body: &str) -> (String, String) {
    let lines: Vec<&str> = body.lines().collect();
    // 先頭のタイトル行は現在の内容に含めない(書き戻すときに付け直す)
    let start = lines.iter().position(|l| !l.trim().is_empty()).filter(|&i| lines[i].starts_with("# ")).map_or(0, |i| i + 1);
    let lines = &lines[start..];
    if let Some(i) = lines.iter().position(|l| l.trim_end() == HISTORY_HEADING) {
        return (lines[..i].join("\n").trim().to_owned(), lines[i + 1..].join("\n").trim().to_owned());
    }
    match lines.iter().position(|l| is_stamp_heading(l)) {
        Some(i) => {
            let history: Vec<String> =
                lines[i..].iter().map(|l| if is_stamp_heading(l) { format!("#{l}") } else { (*l).to_owned() }).collect();
            (lines[..i].join("\n").trim().to_owned(), history.join("\n").trim().to_owned())
        }
        None => (lines.join("\n").trim().to_owned(), String::new()),
    }
}

/// LLMの書いた現在の内容に経緯の見出しが紛れても、構造を壊さないよう1段下げる。
fn sanitize_current(current: &str) -> String {
    current
        .trim()
        .lines()
        .map(|l| if l.trim_end() == HISTORY_HEADING { format!("#{l}") } else { l.to_owned() })
        .collect::<Vec<_>>()
        .join("\n")
}

fn compose(fm: &Frontmatter, current: &str, history: &str) -> String {
    let mut doc = format!("{}\n# {}\n", fm.render(), fm.title);
    if !current.trim().is_empty() {
        doc.push_str(&format!("\n{}\n", current.trim()));
    }
    if !history.trim().is_empty() {
        doc.push_str(&format!("\n{HISTORY_HEADING}\n\n{}\n", history.trim()));
    }
    doc
}

/// 既存ノートを読み、Frontmatter・現在の内容・経緯に分ける。Frontmatterが無ければファイル名をタイトルにする。
pub fn read_note(root: &Path, rel: &str) -> Result<NoteParts, AppError> {
    let text = read(root, rel)?;
    let (fm, body) = match parse(&text) {
        Some((fm, body)) => (fm, body),
        None => {
            let stem = rel.rsplit('/').next().unwrap_or(rel).trim_end_matches(".md");
            let category = rel.rsplit_once('/').map(|(c, _)| c).unwrap_or("");
            (Frontmatter { title: stem.to_owned(), category: category.to_owned(), ..Default::default() }, text.as_str())
        }
    };
    let (current, history) = split_body(body);
    Ok(NoteParts { fm, current, history })
}

/// 整理による書き込みの内容。
pub struct NoteUpdate<'a> {
    pub title: &'a str,
    pub summary: &'a str,
    pub tags: &'a [String],
    /// 新しい現在の内容(Markdown、タイトル行なし)
    pub current: &'a str,
    /// 経緯へ追記する内容。空なら追記しない
    pub history: &'a str,
    pub source_log_ids: &'a [String],
}

/// 書き込みの結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub path: String,
    pub created: bool,
    /// 縮み判定で現在の内容を書き換えなかった
    pub rewrite_skipped: bool,
}

fn merge_unique(dst: &mut Vec<String>, src: &[String]) {
    for v in src {
        let v = v.trim();
        if !v.is_empty() && !dst.iter().any(|d| d == v) {
            dst.push(v.to_owned());
        }
    }
}

fn history_section(now: &str, text: &str) -> String {
    format!("### {now}\n\n{}", text.trim())
}

/// 既存ノートの現在の内容を書き換え、経緯に追記する。書き換え前の版を退避する。
/// 長い内容が半分未満に縮む書き換えは採らず、経緯への追記だけ行う。
pub fn rewrite(root: &Path, rel: &str, u: &NoteUpdate, now: &str) -> Result<Written, AppError> {
    reject_read_only(rel)?;
    let path = resolve_existing(root, rel)?;
    if !rel.ends_with(".md") {
        return Err(AppError::PathNotAllowed);
    }
    let old = fs::read_to_string(&path).map_err(io)?;
    let NoteParts { mut fm, current, mut history } = read_note(root, rel)?;
    let new_current = sanitize_current(u.current);
    let skipped = new_current.is_empty() || would_shrink(&current, &new_current);
    let current = if skipped { current } else { new_current };
    if !u.title.trim().is_empty() {
        fm.title = u.title.trim().to_owned();
    }
    if !skipped || fm.summary.is_empty() {
        let s = one_line(u.summary, SUMMARY_MAX);
        if !s.is_empty() {
            fm.summary = s;
        }
    }
    merge_unique(&mut fm.tags, u.tags);
    merge_unique(&mut fm.source_log_ids, u.source_log_ids);
    if fm.created_at.is_empty() {
        fm.created_at = now.to_owned();
    }
    fm.updated_at = now.to_owned();
    if !u.history.trim().is_empty() {
        let section = history_section(now, u.history);
        history = if history.is_empty() { section } else { format!("{history}\n\n{section}") };
    }
    save_version(root, rel, &old, now)?;
    atomic_write(&path, &compose(&fm, &current, &history))?;
    Ok(Written { path: rel.to_owned(), created: false, rewrite_skipped: skipped && !u.current.trim().is_empty() })
}

/// 新しいノートの置き場所。
pub struct NewNote<'a> {
    pub category: &'a str,
    pub filename: &'a str,
}

fn note_name(filename: &str) -> String {
    match slug(filename, 60) {
        s if s.is_empty() => "note".to_owned(),
        s => s,
    }
}

/// 新しいノートを置く相対パス(`create_note`が使う場所と同じ)。
pub fn note_rel(at: &NewNote) -> String {
    format!("{}/{}.md", category_parts(at.category).join("/"), note_name(at.filename))
}

/// 書き換えると現在の内容が縮みすぎるか(長い内容が半分未満になる)。
pub fn would_shrink(old_current: &str, new_current: &str) -> bool {
    let old_len = old_current.trim().chars().count();
    old_len >= SHRINK_MIN_CHARS && new_current.trim().chars().count() * 2 < old_len
}

/// 新しいノートを作る。同じ場所に既にあれば、そのノートを書き換える(`rewrite`)。
pub fn create_note(root: &Path, at: &NewNote, u: &NoteUpdate, now: &str) -> Result<Written, AppError> {
    let parts = category_parts(at.category);
    reject_read_only(&parts[0])?;
    let dir = ensure_dir(root, &parts)?;
    let name = note_name(at.filename);
    let rel = note_rel(at);
    let path = dir.join(format!("{name}.md"));
    if path.symlink_metadata().is_ok() {
        return rewrite(root, &rel, u, now);
    }
    let mut fm = Frontmatter {
        title: if u.title.trim().is_empty() { name.clone() } else { u.title.trim().to_owned() },
        summary: one_line(u.summary, SUMMARY_MAX),
        category: parts.join("/"),
        created_at: now.to_owned(),
        updated_at: now.to_owned(),
        ..Default::default()
    };
    merge_unique(&mut fm.tags, u.tags);
    merge_unique(&mut fm.source_log_ids, u.source_log_ids);
    let history = if u.history.trim().is_empty() { String::new() } else { history_section(now, u.history) };
    atomic_write(&path, &compose(&fm, &sanitize_current(u.current), &history))?;
    Ok(Written { path: rel, created: true, rewrite_skipped: false })
}

// ---------- 版の退避と復元 ----------

/// ノートの版を置くフォルダ(`.my-assistant/history/<カテゴリ>/<ファイル名>`)。`create`なら作る。
fn versions_dir(root: &Path, rel: &str, create: bool) -> Result<Option<PathBuf>, AppError> {
    let stem = rel.strip_suffix(".md").ok_or(AppError::PathNotAllowed)?;
    let mut parts: Vec<String> = VERSIONS_DIR.iter().map(|s| (*s).to_owned()).collect();
    for p in stem.split('/') {
        if p.is_empty() || p == "." || p == ".." {
            return Err(AppError::PathNotAllowed);
        }
        parts.push(p.to_owned());
    }
    if create {
        return ensure_dir(root, &parts).map(Some);
    }
    let rel_dir = parts.join("/");
    match resolve_existing(root, &rel_dir) {
        Ok(dir) if dir.is_dir() => Ok(Some(dir)),
        _ => Ok(None),
    }
}

/// 版のファイル名として正しいか(`save_version`が作る形だけを受け付ける)。
fn valid_version(name: &str) -> bool {
    name.ends_with(".md")
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !name.contains("..")
}

/// 書き換え前の内容を版として退避し、古い版を`VERSIONS_KEEP`件まで減らす。
pub fn save_version(root: &Path, rel: &str, content: &str, now: &str) -> Result<(), AppError> {
    let Some(dir) = versions_dir(root, rel, true)? else { return Ok(()) };
    let stamp = now.replace(':', "-");
    let stamp: String = stamp.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')).collect();
    // 同じ時刻に複数回書き換えても前の版を潰さないよう、連番を付ける(名前順=時刻順になる)。
    // 古い版を消して空いた番号は使わず、同じ時刻の最大の番号の次にする
    let mut names = version_names(&dir);
    let next = names
        .iter()
        .filter_map(|n| n.strip_prefix(&format!("{stamp}_"))?.strip_suffix(".md")?.parse::<u32>().ok())
        .max()
        .map_or(0, |n| n + 1);
    let name = format!("{stamp}_{next:03}.md");
    fs::write(dir.join(&name), content).map_err(io)?;
    names.insert(0, name);
    names.sort_by(|a, b| b.cmp(a));
    while names.len() > VERSIONS_KEEP {
        let oldest = names.remove(names.len() - 1);
        let _ = fs::remove_file(dir.join(oldest));
    }
    Ok(())
}

/// 版の名前(新しい順)。
fn version_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|it| it.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_file())).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| valid_version(n)).collect())
        .unwrap_or_default();
    names.sort_by(|a, b| b.cmp(a));
    names
}

/// ノートの退避済みの版(新しい順)。
pub fn versions(root: &Path, rel: &str) -> Result<Vec<String>, AppError> {
    read(root, rel)?;
    Ok(versions_dir(root, rel, false)?.map(|d| version_names(&d)).unwrap_or_default())
}

/// 退避済みの版の内容。
pub fn read_version(root: &Path, rel: &str, version: &str) -> Result<String, AppError> {
    if !valid_version(version) {
        return Err(AppError::PathNotAllowed);
    }
    read(root, rel)?;
    let dir = versions_dir(root, rel, false)?.ok_or(AppError::PathNotAllowed)?;
    let path = dir.join(version).canonicalize().map_err(io)?;
    if !path.starts_with(&dir) {
        return Err(AppError::PathNotAllowed);
    }
    fs::read_to_string(path).map_err(io)
}

/// 退避済みの版へ戻す。今の内容も版として退避する。
pub fn restore_version(root: &Path, rel: &str, version: &str, now: &str) -> Result<(), AppError> {
    reject_read_only(rel)?;
    let content = read_version(root, rel, version)?;
    let current = read(root, rel)?;
    save_version(root, rel, &current, now)?;
    atomic_write(&resolve_existing(root, rel)?, &content)
}

/// ノートを削除する前に版として退避する(統合で取り込んだノートを消すときに使う)。
pub fn delete_with_version(root: &Path, rel: &str, now: &str) -> Result<(), AppError> {
    reject_read_only(rel)?;
    let content = read(root, rel)?;
    save_version(root, rel, &content, now)?;
    fs::remove_file(resolve_existing(root, rel)?).map_err(io)
}

#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeFile {
    /// ナレッジルートからの相対パス(`/`区切り)
    pub path: String,
    pub title: String,
    pub category: String,
    pub updated_at: String,
}

/// 一覧のために読む、ファイルの先頭の長さ。Frontmatter(タイトル・更新日時)はここに収まる。
const LIST_HEAD_BYTES: u64 = 8 * 1024;

/// ファイルの先頭だけを読む(一覧のたびに全ノートの全文を読まないため)。読めなければ空。
fn read_head(path: &Path) -> String {
    use std::io::Read;
    let mut bytes = Vec::new();
    if let Ok(f) = fs::File::open(path) {
        let _ = f.take(LIST_HEAD_BYTES).read_to_end(&mut bytes);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Frontmatterのタイトルと更新日時。
/// 先頭だけを読んだためFrontmatterが途中で切れていても(`source_log_ids`が長いノート)、切れた行を捨てて読めた分を使う。
fn head_meta(head: &str) -> Option<(String, String)> {
    let meta = |(fm, _): (Frontmatter, &str)| (fm.title, fm.updated_at);
    if let Some(found) = parse(head) {
        return Some(meta(found));
    }
    let complete = head.rsplit_once('\n').map_or("", |(lines, _)| lines);
    parse(&format!("{complete}\n---\n")).map(meta)
}

/// ノート一覧(最大3階層)。シンボリックリンクは辿らない。整理・索引・提案が使う。
/// `.`始まり(`.my-assistant`のコマンド権限など)と`data/`(ソースコードなど)はノートではないので除く。
pub fn list(root: &Path) -> Result<Vec<KnowledgeFile>, AppError> {
    list_md(root, false)
}

/// 画面に出すMarkdownの一覧。`list`と違い、`.`始まりと`data/`も含める(閲覧専用。`is_read_only`)。
pub fn list_for_screen(root: &Path) -> Result<Vec<KnowledgeFile>, AppError> {
    list_md(root, true)
}

fn list_md(root: &Path, all: bool) -> Result<Vec<KnowledgeFile>, AppError> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    let root = root.canonicalize().map_err(io)?;
    walk(&root, &root, 0, all, &mut out)?;
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(out)
}

fn walk(root: &Path, dir: &Path, depth: u8, all: bool, out: &mut Vec<KnowledgeFile>) -> Result<(), AppError> {
    if depth > 3 {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(io)? {
        let entry = entry.map_err(io)?;
        if !all && (entry.file_name().to_string_lossy().starts_with('.') || is_data_entry(depth, &entry.file_name())) {
            continue;
        }
        let ft = entry.file_type().map_err(io)?;
        let path: PathBuf = entry.path();
        if ft.is_dir() {
            walk(root, &path, depth + 1, all, out)?;
        } else if ft.is_file() && path.extension().is_some_and(|x| x == "md") {
            let head = read_head(&path);
            let rel = path.strip_prefix(root).map_err(|_| AppError::PathNotAllowed)?;
            let rel_s = rel.to_string_lossy().replace('\\', "/");
            let category = rel.parent().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            let (title, updated_at) = match head_meta(&head) {
                Some(meta) => meta,
                None => (path.file_stem().unwrap_or_default().to_string_lossy().into_owned(), String::new()),
            };
            out.push(KnowledgeFile { path: rel_s, title, category, updated_at });
        }
    }
    Ok(())
}

/// Markdown以外のファイル一覧(最大3階層)。画面用で、閲覧専用。`.`始まりと`data/`も含め、シンボリックリンクは辿らない。
pub fn list_other(root: &Path) -> Result<Vec<KnowledgeFile>, AppError> {
    fn walk_other(root: &Path, dir: &Path, depth: u8, out: &mut Vec<KnowledgeFile>) -> Result<(), AppError> {
        if depth > 3 {
            return Ok(());
        }
        for entry in fs::read_dir(dir).map_err(io)? {
            let entry = entry.map_err(io)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let ft = entry.file_type().map_err(io)?;
            let path = entry.path();
            if ft.is_dir() {
                walk_other(root, &path, depth + 1, out)?;
            } else if ft.is_file() && path.extension().is_none_or(|x| x != "md") {
                let rel = path.strip_prefix(root).map_err(|_| AppError::PathNotAllowed)?;
                let category = rel.parent().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
                let updated_at = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
                    .unwrap_or_default();
                out.push(KnowledgeFile { path: rel.to_string_lossy().replace('\\', "/"), title: name, category, updated_at });
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    let root = root.canonicalize().map_err(io)?;
    walk_other(&root, &root, 0, &mut out)?;
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// バイナリかどうかを調べるために、先頭から読む長さ。
const BINARY_PROBE_BYTES: usize = 8 * 1024;

/// Markdown以外のファイルの本文(閲覧専用。`.`始まりと`data/`も読める)。バイナリは拒否し、大きい場合は新しい側の末尾だけを行の区切りから返す。
/// 巨大なファイルでもメモリを使い切らないよう、全体は読まず、先頭(バイナリ判定)と末尾の`MAX_EDIT_BYTES`だけを読む。
pub fn read_other(root: &Path, rel: &str) -> Result<String, AppError> {
    use std::io::{Read, Seek, SeekFrom};
    if rel.ends_with(".md") {
        return Err(AppError::PathNotAllowed);
    }
    let binary = || AppError::Io("バイナリファイルは表示できません".to_owned());
    let mut file = fs::File::open(resolve_existing(root, rel)?).map_err(io)?;
    let len = file.metadata().map_err(io)?.len();
    let max = MAX_EDIT_BYTES as u64;

    // 先頭にNULがあればバイナリ(大きいファイルでも、先頭だけで判定する)
    let mut head = vec![0; BINARY_PROBE_BYTES.min(len as usize)];
    file.read_exact(&mut head).map_err(io)?;
    if head.contains(&0) {
        return Err(binary());
    }

    // 表示する部分(小さければ全体、大きければ末尾)だけを読む
    let start = len.saturating_sub(max);
    file.seek(SeekFrom::Start(start)).map_err(io)?;
    let mut bytes = Vec::with_capacity((len - start) as usize);
    file.take(max).read_to_end(&mut bytes).map_err(io)?;
    if bytes.contains(&0) {
        return Err(binary());
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if start == 0 {
        return Ok(text);
    }
    // 途中から読んだので、先頭の欠けた行(と欠けた文字)を捨てる
    Ok(text.split_once('\n').map_or(text.as_str(), |(_, rest)| rest).to_owned())
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
/// Markdown以外のファイルや設定には触れない。スキル(`skills/`)は残す。
pub fn reset(root: &Path) -> Result<TransferReport, AppError> {
    let mut report = TransferReport::default();
    if !root.exists() {
        return Ok(report);
    }
    for f in list(root)?.into_iter().filter(|f| !super::skills::is_skill(&f.path)) {
        fs::remove_file(resolve_existing(root, &f.path)?).map_err(io)?;
        report.files += 1;
    }
    // 退避した版も消す(コマンドの権限など、ほかの`.my-assistant`の中身は残す)
    if let Ok(versions) = resolve_existing(root, &VERSIONS_DIR.join("/"))
        && versions.is_dir()
    {
        fs::remove_dir_all(versions).map_err(io)?;
    }
    remove_empty_dirs(&root.canonicalize().map_err(io)?, 0);
    Ok(report)
}

/// ルート自身は残し、空のサブフォルダだけ削除する(失敗は無視)。`data/`には触れない。
fn remove_empty_dirs(dir: &Path, depth: u8) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_dir()) && !is_data_entry(depth, &entry.file_name()) {
            remove_empty_dirs(&entry.path(), depth + 1);
            let _ = fs::remove_dir(entry.path());
        }
    }
}

/// 会話をそのまま書き出すキャプチャのカテゴリ。整理でカテゴライズされたナレッジとは分けて扱う。
pub const CAPTURE_CATEGORY: &str = "captures";

/// 担当システムのソースコードなどを置くフォルダ(ナレッジ直下)。サンドボックスでは`/knowledge/data`として見え、作業ディレクトリの既定になる。
/// ノートではないので、一覧・索引・整理・初期化・書き出し・バックアップの対象にせず、画面やLLMのツールからも書き換えさせない。
pub const DATA_DIR: &str = "data";

/// カテゴリ未設定のノートの置き場(ナレッジ直下)。
pub const INBOX_DIR: &str = "inbox";

/// `data/`配下(`data`自身を含む)か。
pub fn is_data(path: &str) -> bool {
    path.trim_start_matches("./").split(['/', '\\']).next().is_some_and(|p| p.eq_ignore_ascii_case(DATA_DIR))
}

/// ナレッジ直下の要素が`data/`か(走査で飛ばす判定)。
fn is_data_entry(depth: u8, name: &std::ffi::OsStr) -> bool {
    depth == 0 && name.to_string_lossy().eq_ignore_ascii_case(DATA_DIR)
}

/// ナレッジ配下のフォルダ名を、すべて小文字に改名する(起動・ワークスペースを開くたびに呼ぶ。済んでいれば何もしない)。
/// `.`始まりのフォルダ・シンボリックリンク・`data/`(担当システムのフォルダ)は触らない。
/// 小文字の名前の別のフォルダがすでにあるとき(大文字小文字を区別するボリューム)は、何も変えずに飛ばして警告する。
/// 大文字小文字を区別しないボリュームでも改名できるよう、いったん仮の名前を挟む。改名した数を返す。
pub fn lowercase_dirs(root: &Path) -> usize {
    fn walk(dir: &Path, depth: u8) -> usize {
        let Ok(entries) = fs::read_dir(dir) else { return 0 };
        let mut names: Vec<std::ffi::OsString> = entries.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.file_name()).collect();
        names.sort();
        let mut renamed = 0;
        for name in names {
            let Some(name_s) = name.to_str() else { continue };
            if name_s.starts_with('.') || is_data_entry(depth, &name) {
                continue;
            }
            let lower = name_s.to_lowercase();
            let mut path = dir.join(&name);
            if lower != name_s {
                let target = dir.join(&lower);
                if !same_entry(&path, &target) {
                    log::warn!("knowledge folder not lowercased (already exists): {}", path.display());
                    continue;
                }
                let tmp = dir.join(format!(".{lower}.lowercase-tmp"));
                if tmp.symlink_metadata().is_ok() || fs::rename(&path, &tmp).and_then(|()| fs::rename(&tmp, &target)).is_err() {
                    log::warn!("knowledge folder not lowercased: {}", path.display());
                    continue;
                }
                renamed += 1;
                path = target;
            }
            if depth < 8 {
                renamed += walk(&path, depth + 1);
            }
        }
        renamed
    }
    walk(root, 0)
}

/// ナレッジ直下に、アプリが使うフォルダ(`data`・`inbox`・`skills`・`captures`・`preferences`、`with_memory`なら`memory`)を、無ければ作る。
/// 小文字化の後に呼ぶ(大文字の同名フォルダと並べて作らないため)。
pub fn ensure_system_dirs(root: &Path, with_memory: bool) -> Result<(), AppError> {
    let mut names = vec![DATA_DIR, INBOX_DIR, super::skills::DIR, CAPTURE_CATEGORY, CORRECTIONS_DIR];
    if with_memory {
        names.push(super::memory::MEMORY_CATEGORY);
    }
    for name in names {
        ensure_dir(root, &[name.to_owned()])?;
    }
    Ok(())
}

/// `target`が無い、または`path`と同じ実体(大文字小文字を区別しないボリュームでの別名)か。
fn same_entry(path: &Path, target: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (path.symlink_metadata(), target.symlink_metadata()) {
        (_, Err(_)) => true,
        (Ok(a), Ok(b)) => (a.dev(), a.ino()) == (b.dev(), b.ino()),
        _ => false,
    }
}

/// 画面では閲覧だけにする場所か(`data/`配下と、`.`始まりを含むパス)。
pub fn is_read_only(rel: &str) -> bool {
    is_data(rel) || rel.split(['/', '\\']).any(|p| p.starts_with('.'))
}

/// 書き込み先が閲覧専用の場所(`is_read_only`)なら拒否する。
fn reject_read_only(rel: &str) -> Result<(), AppError> {
    if is_read_only(rel) { Err(AppError::PathNotAllowed) } else { Ok(()) }
}

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
                ..Default::default()
            };
            (dir.join(format!("{date}_{sid}.md")), format!("{}\n# {title}\n{chapter}", fm.render()))
        }
    };
    atomic_write(&path, &doc)
}

/// 整理が済んだセッションのキャプチャをまとめて削除し、消した件数を返す。
/// `states`は`(セッションID, 未処理ログが残っているか)`。
/// - ファイル名にはセッションIDの先頭だけが入るので、先頭が同じセッションのどれかに未処理があれば残す。
/// - DBにログが1件もないキャプチャ(履歴の削除済み・別の保存先など)は、整理済みと判断できないので残す。
pub fn sweep_captures(root: &Path, states: &[(String, bool)]) -> Result<usize, AppError> {
    // キャプチャがまだ無いなら、フォルダを作らずに終える
    if !root.join(CAPTURE_CATEGORY).is_dir() {
        return Ok(0);
    }
    let dir = ensure_dir(root, &[CAPTURE_CATEGORY.to_owned()])?;
    // 先頭が同じセッションをまとめ、1つでも未処理があれば「未処理あり」とする
    let mut pending: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    for (sid, unprocessed) in states {
        *pending.entry(short_session(sid)).or_default() |= *unprocessed;
    }
    let mut removed = 0;
    for entry in fs::read_dir(&dir).map_err(io)?.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        // `日付_セッションID先頭.md`
        let Some(sid) = name.strip_suffix(".md").and_then(|n| n.split_once('_')).map(|(_, sid)| sid) else { continue };
        if path.is_file() && pending.get(sid) == Some(&false) {
            fs::remove_file(&path).map_err(io)?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// 新しい会話の文脈復元用に、直近更新されたファイルの末尾(最新の追記)を連結して返す。
/// `max_files`件・合計`max_chars`文字まで。該当がなければ`None`。
pub fn recent_context(root: &Path, max_files: usize, max_chars: usize) -> Result<Option<String>, AppError> {
    let mut out = String::new();
    let mut left = max_chars;
    // 訂正履歴と記憶は専用の枠でシステムプロンプトへ入れるため、ここでは除く
    for f in list(root)?
        .into_iter()
        .filter(|f| {
            f.path != CORRECTIONS_PATH
                && f.category != CAPTURE_CATEGORY
                && !super::memory::is_memory(&f.path)
                && !super::skills::is_skill(&f.path)
        })
        .take(max_files)
    {
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
const CORRECTIONS_DIR: &str = "preferences";
pub const CORRECTIONS_PATH: &str = "preferences/stt_corrections.md";
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

/// 聞き取り間違いの訂正を`preferences/stt_corrections.md`へ追記する。
/// 発話全体(`heard`→`correct`)の差分区間を「誤→正」として残し、元の文を例として添える。
/// 差分がない・すでに同じ訂正がある場合は書かずに`false`を返す。直接編集しても読み戻せる。
pub fn add_correction(root: &Path, heard: &str, correct: &str, now: &str) -> Result<bool, AppError> {
    let (h, c) = diff_span(heard.trim(), correct.trim());
    let (h, c) = (one_line(&h, 100), one_line(&c, 100));
    if h.is_empty() && c.is_empty() {
        return Ok(false);
    }
    let key = format!("{CORRECTIONS_LINE}{h}」→「{c}」");
    let line = format!("{key} — 例: {} ({})", one_line(correct, 200), now.get(..10).unwrap_or(now));

    let dir = ensure_dir(root, &[CORRECTIONS_DIR.to_owned()])?;
    let path = dir.join("stt_corrections.md");
    let doc = match fs::read_to_string(&path) {
        Ok(existing) => {
            let (mut fm, body) = parse(&existing).unwrap_or_else(|| {
                (
                    Frontmatter { title: "音声認識の訂正履歴".into(), category: CORRECTIONS_DIR.into(), created_at: now.to_owned(), ..Default::default() },
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
                category: CORRECTIONS_DIR.into(),
                tags: vec!["stt".into(), "correction".into()],
                created_at: now.to_owned(),
                updated_at: now.to_owned(),
                source_log_ids: vec![],
                ..Default::default()
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

/// 編集可能なMarkdownの上限。
const MAX_EDIT_BYTES: usize = 5 * 1024 * 1024;

/// 既存ノートの本文を丸ごと置き換える。
pub fn write(root: &Path, rel: &str, content: &str) -> Result<(), AppError> {
    reject_read_only(rel)?;
    if !rel.ends_with(".md") || content.len() > MAX_EDIT_BYTES {
        return Err(AppError::PathNotAllowed);
    }
    atomic_write(&resolve_existing(root, rel)?, content)
}

/// フォルダ名として安全か(`slug`で変化しない名前)。
fn is_safe_dir_name(name: &str) -> bool {
    !name.is_empty() && slug(name, 40) == name
}

/// 新規フォルダの相対パスを検証し、親までを作成して実パスを返す。既存なら拒否する。
fn resolve_new_dir(root: &Path, rel: &str) -> Result<PathBuf, AppError> {
    reject_read_only(rel)?;
    let names: Vec<String> = rel.split('/').map(str::to_lowercase).collect();
    if names.len() > 3 || !names.iter().all(|n| is_safe_dir_name(n)) {
        return Err(AppError::PathNotAllowed);
    }
    let (leaf, parents) = names.split_last().ok_or(AppError::PathNotAllowed)?;
    let dir = ensure_dir(root, parents)?.join(leaf);
    if dir.symlink_metadata().is_ok() {
        return Err(AppError::Io("already_exists".to_owned()));
    }
    Ok(dir)
}

/// 新規ノートを作る。`rel`は`.md`付きの相対パス。既存なら拒否する。作成した相対パスを返す。
pub fn create_file(root: &Path, rel: &str, now: &str) -> Result<String, AppError> {
    let Some(stem) = rel.strip_suffix(".md") else {
        return Err(AppError::PathNotAllowed);
    };
    reject_read_only(rel)?;
    let rel = &lower_dirs(rel);
    let path = resolve_new_file(root, rel)?;
    if path.symlink_metadata().is_ok() {
        return Err(AppError::Io("already_exists".to_owned()));
    }
    let title = stem.rsplit('/').next().unwrap_or(stem);
    let category = rel.rsplit_once('/').map(|(c, _)| c).unwrap_or("");
    let fm = Frontmatter {
        title: title.to_owned(),
        category: category.to_owned(),
        created_at: now.to_owned(),
        updated_at: now.to_owned(),
        ..Default::default()
    };
    atomic_write(&path, &format!("{}\n# {title}\n", fm.render()))?;
    Ok(rel.to_owned())
}

/// 新規フォルダ(空)を作る。既存なら拒否する。
pub fn create_dir(root: &Path, rel: &str) -> Result<(), AppError> {
    let dir = resolve_new_dir(root, rel)?;
    fs::create_dir(dir).map_err(io)
}

/// ファイルまたはフォルダの名前変更・移動。移動先が既存なら拒否する。
pub fn rename(root: &Path, from: &str, to: &str) -> Result<(), AppError> {
    reject_read_only(from)?;
    reject_read_only(to)?;
    let src = resolve_existing(root, from)?;
    let dst = if src.is_dir() {
        let dst = resolve_new_dir(root, to)?;
        if dst.starts_with(&src) {
            return Err(AppError::PathNotAllowed);
        }
        dst
    } else {
        if !from.ends_with(".md") || !to.ends_with(".md") {
            return Err(AppError::PathNotAllowed);
        }
        let dst = resolve_new_file(root, to)?;
        if dst.symlink_metadata().is_ok() {
            return Err(AppError::Io("already_exists".to_owned()));
        }
        dst
    };
    fs::rename(src, dst).map_err(io)
}

/// ファイルまたはフォルダ(中身ごと)を削除する。ルート自体は削除できない。
pub fn delete(root: &Path, rel: &str) -> Result<(), AppError> {
    reject_read_only(rel)?;
    let target = resolve_existing(root, rel)?;
    if target.is_dir() {
        fs::remove_dir_all(target).map_err(io)
    } else if rel.ends_with(".md") {
        fs::remove_file(target).map_err(io)
    } else {
        Err(AppError::PathNotAllowed)
    }
}

/// フォルダ一覧(空フォルダ含む、最大3階層)。画面用で、`.`始まりと`data/`も含め、シンボリックリンクは辿らない。
pub fn list_dirs(root: &Path) -> Result<Vec<String>, AppError> {
    fn walk_dirs(root: &Path, dir: &Path, depth: u8, out: &mut Vec<String>) -> Result<(), AppError> {
        if depth >= 3 {
            return Ok(());
        }
        for entry in fs::read_dir(dir).map_err(io)? {
            let entry = entry.map_err(io)?;
            if !entry.file_type().map_err(io)?.is_dir() {
                continue;
            }
            let path = entry.path();
            let rel = path.strip_prefix(root).map_err(|_| AppError::PathNotAllowed)?;
            out.push(rel.to_string_lossy().replace('\\', "/"));
            walk_dirs(root, &path, depth + 1, out)?;
        }
        Ok(())
    }
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    let root = root.canonicalize().map_err(io)?;
    walk_dirs(&root, &root, 0, &mut out)?;
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_reads_the_title_even_when_the_frontmatter_is_longer_than_the_head() {
        let dir = tempfile::tempdir().unwrap();
        let ids: Vec<String> = (0..2000).map(|i| format!("log-{i:08}")).collect();
        let fm = Frontmatter { title: "長いノート".into(), updated_at: "2026-01-02T03:04:05Z".into(), source_log_ids: ids, ..Default::default() };
        fs::write(dir.path().join("a.md"), format!("{}\n本文", fm.render())).unwrap();
        fs::write(dir.path().join("b.md"), "frontmatterなし").unwrap();
        let files = list(dir.path()).unwrap();
        let a = files.iter().find(|f| f.path == "a.md").unwrap();
        assert_eq!((a.title.as_str(), a.updated_at.as_str()), ("長いノート", "2026-01-02T03:04:05Z"));
        assert_eq!(files.iter().find(|f| f.path == "b.md").unwrap().title, "b");
    }

    #[test]
    fn lists_and_reads_non_markdown_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        create_dir(&root, "debug-logs").unwrap();
        fs::write(root.join("debug-logs/2026-01-01.jsonl"), "{\"a\":1}\n").unwrap();
        fs::write(root.join("debug-logs/.hidden"), "x").unwrap();
        fs::write(root.join("debug-logs/bin.dat"), [0u8, 1, 2]).unwrap();
        create_file(&root, "debug-logs/note.md", "2026-10-29T00:00:00Z").unwrap();
        let paths: Vec<_> = list_other(&root).unwrap().into_iter().map(|f| f.path).collect();
        assert_eq!(paths, ["debug-logs/.hidden", "debug-logs/2026-01-01.jsonl", "debug-logs/bin.dat"], "`.`始まりも画面に出す");
        assert!(list(&root).unwrap().iter().all(|f| f.path.ends_with(".md")));
        assert!(read_other(&root, "debug-logs/2026-01-01.jsonl").unwrap().contains("\"a\":1"));
        assert!(read_other(&root, "debug-logs/bin.dat").is_err());
        assert!(read_other(&root, "debug-logs/note.md").is_err());
        assert_eq!(read_other(&root, "debug-logs/.hidden").unwrap(), "x");
        assert!(write(&root, "debug-logs/.hidden.md", "x").is_err() && delete(&root, "debug-logs/.hidden").is_err(), "`.`始まりは閲覧だけ");
        assert!(read_other(&root, "../x.txt").is_err());
        assert!(write(&root, "debug-logs/2026-01-01.jsonl", "x").is_err());
        assert!(delete(&root, "debug-logs/2026-01-01.jsonl").is_err());
    }

    #[test]
    fn dot_folders_are_not_notes_and_skills_survive_reset() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        create_file(&root, "dev/a.md", "2026-10-29T00:00:00Z").unwrap();
        fs::create_dir_all(root.join(".my-assistant")).unwrap();
        fs::write(root.join(".my-assistant/note.md"), "x").unwrap();
        fs::create_dir_all(root.join("skills/daily")).unwrap();
        fs::write(root.join("skills/daily/SKILL.md"), "---\ndescription: \"d\"\n---\n手順").unwrap();
        let mut paths: Vec<_> = list(&root).unwrap().into_iter().map(|f| f.path).collect();
        paths.sort();
        assert_eq!(paths, ["dev/a.md", "skills/daily/SKILL.md"], "`.`始まりはノートではなく、スキルはノートとして見える");
        assert!(list_for_screen(&root).unwrap().iter().any(|f| f.path == ".my-assistant/note.md"), "画面には出す");
        assert!(write(&root, ".my-assistant/note.md", "y").is_err(), "画面からは書き換えない");
        reset(&root).unwrap();
        assert!(root.join(".my-assistant/note.md").is_file());
        assert!(root.join("skills/daily/SKILL.md").is_file(), "初期化はスキルを消さない");
        assert!(!root.join("dev").exists());
    }

    #[test]
    fn crud_roundtrip_and_rejects_unsafe() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        create_dir(&root, "dev").unwrap();
        assert!(create_dir(&root, "dev").is_err());
        assert!(create_dir(&root, "../x").is_err());
        assert!(create_dir(&root, "a b").is_err());
        let rel = create_file(&root, "dev/memo.md", "2026-10-29T00:00:00Z").unwrap();
        assert!(create_file(&root, "dev/memo.md", "x").is_err());
        assert!(create_file(&root, "dev/memo.txt", "x").is_err());
        write(&root, &rel, "# edited\n").unwrap();
        assert_eq!(read(&root, &rel).unwrap(), "# edited\n");
        assert!(write(&root, "../x.md", "x").is_err());
        rename(&root, "dev/memo.md", "dev/note.md").unwrap();
        create_dir(&root, "ops").unwrap();
        rename(&root, "dev/note.md", "ops/note.md").unwrap();
        assert_eq!(list(&root).unwrap()[0].category, "ops");
        assert!(rename(&root, "ops", "ops/Sub").is_err());
        rename(&root, "ops", "ops2").unwrap();
        assert_eq!(list_dirs(&root).unwrap(), ["dev", "ops2"]);
        delete(&root, "ops2").unwrap();
        assert!(list(&root).unwrap().is_empty());
        assert!(delete(&root, "").is_err());
        assert!(delete(&root, "..").is_err());
    }

    fn entry<'a>(cat: &'a str, file: &'a str, content: &'a str, ids: &'a [String]) -> Entry<'a> {
        Entry { category: cat, filename: file, title: "T", tags: &[], content, source_log_ids: ids }
    }

    #[test]
    fn create_then_append_merges_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        let ids1 = vec!["a".to_owned()];
        let rel = upsert(&root, &Entry { tags: &["Rust".to_owned()], ..entry("Development", "tauri_setup", "first", &ids1) }, "2026-10-27T00:00:00Z").unwrap();
        assert_eq!(rel, "development/tauri_setup.md");
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

    fn update<'a>(current: &'a str, history: &'a str, ids: &'a [String]) -> NoteUpdate<'a> {
        NoteUpdate { title: "", summary: "", tags: &[], current, history, source_log_ids: ids }
    }

    #[test]
    fn summary_roundtrips_and_is_omitted_when_empty() {
        let fm = Frontmatter { title: "t".into(), summary: "1行の要約".into(), ..Default::default() };
        assert_eq!(parse(&fm.render()).unwrap().0.summary, "1行の要約");
        assert!(!Frontmatter::default().render().contains("summary"));
    }

    #[test]
    fn create_and_rewrite_keep_current_and_history_apart() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let ids = vec!["a".to_owned()];
        let u = NoteUpdate { title: "Tauri", summary: "要約1", tags: &["rust".to_owned()], current: "- npm", history: "- 作成", source_log_ids: &ids };
        let w = create_note(root, &NewNote { category: "dev", filename: "tauri" }, &u, "2026-11-01T00:00:00Z").unwrap();
        assert_eq!(w, Written { path: "dev/tauri.md".into(), created: true, rewrite_skipped: false });
        assert_eq!(note_rel(&NewNote { category: "dev", filename: "tauri" }), w.path);
        // LLMが経緯の見出しを書いても、構造は壊さない
        let ids2 = vec!["b".to_owned()];
        let u2 = NoteUpdate { summary: "要約2", ..update("- pnpm\n## 経緯\nx", "- 変更", &ids2) };
        let w2 = rewrite(root, &w.path, &u2, "2026-11-02T00:00:00Z").unwrap();
        assert!(!w2.created && !w2.rewrite_skipped);
        let n = read_note(root, &w.path).unwrap();
        assert_eq!(n.current, "- pnpm\n### 経緯\nx");
        assert!(n.history.starts_with("### 2026-11-01T00:00:00Z\n\n- 作成"));
        assert!(n.history.ends_with("### 2026-11-02T00:00:00Z\n\n- 変更"));
        assert_eq!((n.fm.title.as_str(), n.fm.summary.as_str()), ("Tauri", "要約2"));
        assert_eq!(n.fm.source_log_ids, ["a", "b"]);
        assert_eq!(n.fm.created_at, "2026-11-01T00:00:00Z");
        // 同じ場所への作成は書き換えになる
        let w3 = create_note(root, &NewNote { category: "dev", filename: "tauri" }, &update("- yarn", "", &[]), "2026-11-03T00:00:00Z").unwrap();
        assert!(!w3.created);
        assert_eq!(read_note(root, &w.path).unwrap().current, "- yarn");
    }

    #[test]
    fn legacy_notes_move_their_dated_sections_into_history() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        upsert(root, &entry("dev", "old", "最初のメモ", &[]), "2026-10-01T00:00:00Z").unwrap();
        upsert(root, &entry("dev", "old", "追記", &[]), "2026-10-02T00:00:00Z").unwrap();
        let n = read_note(root, "dev/old.md").unwrap();
        assert_eq!(n.current, "");
        assert!(n.history.starts_with("### 2026-10-01T00:00:00Z") && n.history.contains("追記"));
        rewrite(root, "dev/old.md", &update("- まとめ", "- 統合", &[]), "2026-11-01T00:00:00Z").unwrap();
        let text = read(root, "dev/old.md").unwrap();
        let body = parse(&text).unwrap().1;
        assert!(body.contains("\n\n- まとめ\n\n## 経緯\n\n### 2026-10-01T00:00:00Z"), "{body}");
        // 手書きのノート(Frontmatterなし)も読める
        fs::write(root.join("dev/hand.md"), "# 手書き\n\n自由に書いた本文\n\n## 節\n内容").unwrap();
        let n = read_note(root, "dev/hand.md").unwrap();
        assert_eq!((n.fm.title.as_str(), n.current.as_str(), n.history.as_str()), ("hand", "自由に書いた本文\n\n## 節\n内容", ""));
    }

    #[test]
    fn shrinking_rewrites_only_append_history() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let long = "- 大事な情報\n".repeat(80);
        let w = create_note(root, &NewNote { category: "dev", filename: "x" }, &NoteUpdate { summary: "元の要約", ..update(&long, "", &[]) }, "t1").unwrap();
        let r = rewrite(root, &w.path, &NoteUpdate { summary: "新しい要約", ..update("- 短い", "- 変更", &[]) }, "t2").unwrap();
        assert!(r.rewrite_skipped);
        let n = read_note(root, &w.path).unwrap();
        assert_eq!(n.current, long.trim());
        assert_eq!(n.fm.summary, "元の要約", "書き換えなかったときは要約も変えない");
        assert!(n.history.contains("- 変更"));
        assert!(would_shrink(&long, "- 短い") && !would_shrink("- 短い", ""));
    }

    #[test]
    fn versions_are_kept_up_to_ten_and_can_be_restored() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let w = create_note(root, &NewNote { category: "dev", filename: "v" }, &update("- v0", "", &[]), "2026-11-01T00:00:00Z").unwrap();
        for i in 1..=12 {
            // 同じ時刻の書き換えでも、前の版を潰さない
            rewrite(root, &w.path, &update(&format!("- v{i}"), "", &[]), "2026-11-02T00:00:00Z").unwrap();
        }
        let vs = versions(root, &w.path).unwrap();
        assert_eq!(vs.len(), 10);
        assert!(read_version(root, &w.path, &vs[0]).unwrap().contains("- v11"), "新しい順");
        assert!(read_version(root, &w.path, &vs[9]).unwrap().contains("- v2"));
        for bad in ["../x.md", ".x.md", "a/b.md", "x.txt"] {
            assert!(read_version(root, &w.path, bad).is_err(), "{bad}");
        }
        restore_version(root, &w.path, &vs[9], "2026-11-03T00:00:00Z").unwrap();
        assert_eq!(read_note(root, &w.path).unwrap().current, "- v2");
        assert!(read_version(root, &w.path, &versions(root, &w.path).unwrap()[0]).unwrap().contains("- v12"), "戻す前の内容も退避する");
        // 版は一覧・グラフに出ず、初期化で消える(コマンドの権限は残す)
        assert!(list(root).unwrap().iter().all(|f| !f.path.starts_with('.')));
        fs::write(root.join(".my-assistant/command-policy.json"), "{}").unwrap();
        reset(root).unwrap();
        assert!(!root.join(".my-assistant/history").exists());
        assert!(root.join(".my-assistant/command-policy.json").exists());
    }

    #[test]
    fn lowercase_dirs_renames_nested_folders_but_not_data_or_hidden() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        for p in ["Skills/Daily", "Dev/Rust", "Inbox", "data/Src/Lib", ".Hidden/Keep", "ok"] {
            fs::create_dir_all(root.join(p)).unwrap();
        }
        fs::write(root.join("Dev/Rust/Note.md"), "x").unwrap();
        assert_eq!(lowercase_dirs(root), 5);
        let mut names: Vec<String> = fs::read_dir(root).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, [".Hidden", "data", "dev", "inbox", "ok", "skills"]);
        assert!(root.join("skills/daily").is_dir() && root.join("data/Src/Lib").is_dir() && root.join(".Hidden/Keep").is_dir());
        assert_eq!(fs::read_to_string(root.join("dev/rust/Note.md")).unwrap(), "x");
        assert_eq!(lowercase_dirs(root), 0);
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
        assert!(ctx.find("development").unwrap() < ctx.find("ideas").unwrap_or(usize::MAX));
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
        upsert(&root, &entry("dev", "a", "x", &[]), "now").unwrap();
        upsert(&root, &entry("dev/Rust", "b", "y", &[]), "now").unwrap();
        fs::write(root.join("keep.txt"), "not markdown").unwrap();

        let dest = dir.path().join("out");
        let r = export(&root, &dest, "t").unwrap();
        assert_eq!(r.files, 2);
        let exported = dest.join("MyAssistant-knowledge-t");
        assert!(exported.join("dev/Rust/b.md").exists());

        let r = reset(&root).unwrap();
        assert_eq!(r.files, 2);
        assert!(list(&root).unwrap().is_empty());
        assert!(!root.join("dev").exists());
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
        assert_eq!(files[0].path, "captures/2026-10-27_abcdef12.md");
        let text = read(&root, &files[0].path).unwrap();
        assert!(text.contains("## 12:00") && text.contains("## 12:05"));

        // 文脈復元にはキャプチャを含めない
        assert!(recent_context(&root, 5, 1000).unwrap().is_none());

        let done = [("abcdef123456".to_owned(), false)];
        assert_eq!(sweep_captures(&root, &done).unwrap(), 1);
        assert!(list(&root).unwrap().is_empty());
        assert_eq!(sweep_captures(&root, &done).unwrap(), 0);
        // 削除後の発話は新しいキャプチャになる
        append_capture(&root, "abcdef123456", "さらに", "うん", "2026-10-28T09:00:00Z").unwrap();
        assert_eq!(list(&root).unwrap()[0].path, "captures/2026-10-28_abcdef12.md");
    }

    #[test]
    fn sweep_removes_only_captures_of_fully_organized_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("k");
        for sid in ["done0001xx", "busy0002xx", "none0003xx", "same0004aa", "same0004bb"] {
            append_capture(&root, sid, "q", "a", "2026-10-27T12:00:00Z").unwrap();
        }
        let states = vec![
            ("done0001xx".to_owned(), false),
            ("busy0002xx".to_owned(), true),
            ("same0004aa".to_owned(), false),
            ("same0004bb".to_owned(), true), // 先頭8文字が同じ → 未処理があるので両方残す
        ];
        assert_eq!(sweep_captures(&root, &states).unwrap(), 1);
        let left: Vec<String> = list(&root).unwrap().into_iter().map(|f| f.path).collect();
        assert_eq!(left.len(), 3, "{left:?}"); // busy, none(ログなし), same
        assert!(!left.iter().any(|p| p.contains("done0001")));
    }

    #[test]
    fn read_other_reads_only_the_tail_of_a_huge_file() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().to_path_buf();
        fs::create_dir_all(root.join("logs")).unwrap();
        // 上限(5MiB)を超える、行ごとに番号を振ったテキスト
        let lines: Vec<String> = (0..500_000).map(|i| format!("line-{i:07}")).collect();
        let text = lines.join("\n") + "\n";
        assert!(text.len() > MAX_EDIT_BYTES);
        fs::write(root.join("logs/big.log"), &text).unwrap();
        let out = read_other(&root, "logs/big.log").unwrap();
        assert!(out.len() <= MAX_EDIT_BYTES);
        assert!(out.starts_with("line-"), "先頭の欠けた行は捨てて、行の区切りから返す");
        assert!(out.ends_with("line-0499999\n"), "末尾(新しい側)を返す");
        assert!(!out.contains("line-0000000"));
        // 小さいファイルは全体を返す
        fs::write(root.join("logs/small.log"), "a\nb\n").unwrap();
        assert_eq!(read_other(&root, "logs/small.log").unwrap(), "a\nb\n");
        // 空のファイル
        fs::write(root.join("logs/empty.log"), "").unwrap();
        assert_eq!(read_other(&root, "logs/empty.log").unwrap(), "");
    }

    #[test]
    fn read_other_rejects_binary_even_when_huge() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().to_path_buf();
        fs::create_dir_all(root.join("logs")).unwrap();
        // 先頭にNULがある大きなファイル
        let mut head_nul = vec![0u8; 16];
        head_nul.extend(std::iter::repeat_n(b'a', MAX_EDIT_BYTES + 10));
        fs::write(root.join("logs/head.bin"), &head_nul).unwrap();
        assert!(read_other(&root, "logs/head.bin").is_err());
        // 末尾にNULがある大きなファイル
        let mut tail_nul = vec![b'a'; MAX_EDIT_BYTES + 10];
        tail_nul.push(0);
        fs::write(root.join("logs/tail.bin"), &tail_nul).unwrap();
        assert!(read_other(&root, "logs/tail.bin").is_err());
    }

    #[test]
    fn data_folder_is_not_knowledge_and_survives_reset() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        fs::create_dir_all(root.join("data/app/empty")).unwrap();
        fs::write(root.join("data/README.md"), "# readme").unwrap();
        fs::write(root.join("data/app/main.rs"), "fn main() {}").unwrap();
        fs::create_dir_all(root.join("dev")).unwrap();
        fs::write(root.join("dev/note.md"), "# note").unwrap();
        fs::create_dir_all(root.join("dev/data")).unwrap();
        fs::write(root.join("dev/data/x.md"), "# x").unwrap();

        // 整理・索引が使うノート一覧には出ない(ナレッジ直下の data だけが対象)
        let notes: Vec<String> = list(&root).unwrap().into_iter().map(|f| f.path).collect();
        assert!(notes.contains(&"dev/note.md".to_owned()) && notes.contains(&"dev/data/x.md".to_owned()));
        assert!(notes.iter().all(|p| !is_data(p)), "{notes:?}");
        // 画面の一覧には出し、読める
        assert!(list_for_screen(&root).unwrap().iter().any(|f| f.path == "data/README.md"));
        assert!(list_dirs(&root).unwrap().iter().any(|p| p == "data/app/empty"));
        assert_eq!(list_other(&root).unwrap().into_iter().map(|f| f.path).collect::<Vec<_>>(), ["data/app/main.rs"]);
        assert_eq!(read_other(&root, "data/app/main.rs").unwrap(), "fn main() {}");

        // 画面からの書き換え・作成・移動・削除は拒否する
        assert!(write(&root, "data/README.md", "x").is_err());
        assert!(create_file(&root, "data/new.md", "2026-01-01T00:00:00Z").is_err());
        assert!(create_dir(&root, "Data").is_err());
        assert!(rename(&root, "data", "moved").is_err());
        assert!(rename(&root, "dev/note.md", "data/note.md").is_err());
        assert!(delete(&root, "data").is_err());
        assert!(delete(&root, "DATA/README.md").is_err());

        // 初期化しても data の中身(.md や空のフォルダも)は残る
        reset(&root).unwrap();
        assert!(root.join("data/README.md").is_file() && root.join("data/app/main.rs").is_file());
        assert!(root.join("data/app/empty").is_dir());
        assert!(!root.join("dev/note.md").exists());
    }
}
