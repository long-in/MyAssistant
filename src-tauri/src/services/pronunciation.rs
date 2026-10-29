//! 読み上げ前の読み置換。英単語やサービス名がアルファベット読みされるのを、カタカナへ置き換えて防ぐ。
//! 字幕には元の文字列を使い、TTSへ送る文字列だけを置換する。
use std::{collections::HashMap, path::Path};

use serde::{Deserialize, Serialize};

use crate::utils::error::AppError;

/// ユーザー辞書のファイル名(アプリデータ直下)。`[{"word": "GitHub", "reading": "ギットハブ"}]`形式のJSON。
pub const DICTIONARY_FILE: &str = "tts_dictionary.json";

/// 組み込みの読み。ユーザー辞書の同名の語は、ユーザー辞書が優先される。
const BUILTIN: &[(&str, &str)] = &[
    ("GitHub", "ギットハブ"),
    ("Google", "グーグル"),
    ("Claude", "クロード"),
    ("Anthropic", "アンスロピック"),
    ("OpenAI", "オープンエーアイ"),
    ("ChatGPT", "チャットジーピーティー"),
    ("YouTube", "ユーチューブ"),
    ("Slack", "スラック"),
    ("Notion", "ノーション"),
    ("macOS", "マックオーエス"),
    ("iPhone", "アイフォーン"),
    ("Tauri", "タウリ"),
    ("Rust", "ラスト"),
    ("VOICEVOX", "ボイスボックス"),
];

/// 登録できる語数・文字数の上限。
const MAX_ENTRIES: usize = 500;
const MAX_WORD_CHARS: usize = 64;
const MAX_READING_CHARS: usize = 128;

/// 語とその読み。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub word: String,
    pub reading: String,
}

/// 画面に出す辞書。`builtin`は読み取り専用で、`user`に同じ語があればそちらが優先される。
#[derive(Debug, Serialize)]
pub struct Listing {
    pub builtin: Vec<Entry>,
    pub user: Vec<Entry>,
}

pub fn listing(app_data: &Path) -> Listing {
    Listing {
        builtin: BUILTIN.iter().map(|(w, r)| Entry { word: (*w).to_owned(), reading: (*r).to_owned() }).collect(),
        user: load_user(app_data),
    }
}

/// ユーザー辞書を登録順に読む。ファイルが無い・壊れている場合は空。
pub fn load_user(app_data: &Path) -> Vec<Entry> {
    std::fs::read_to_string(app_data.join(DICTIONARY_FILE))
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<Entry>>(&s).ok())
        .unwrap_or_default()
}

/// 空白を除き、空の項目と大文字小文字違いの重複(後の登録を採用)を落とす。上限を超える場合は拒否する。
pub fn normalize(entries: Vec<Entry>) -> Result<Vec<Entry>, AppError> {
    let mut out: Vec<Entry> = Vec::new();
    for e in entries {
        let (word, reading) = (e.word.trim().to_owned(), e.reading.trim().to_owned());
        if word.is_empty() || reading.is_empty() {
            continue;
        }
        if word.chars().count() > MAX_WORD_CHARS || reading.chars().count() > MAX_READING_CHARS {
            return Err(AppError::Io("語または読みが長すぎます".into()));
        }
        out.retain(|x| x.word.to_lowercase() != word.to_lowercase());
        out.push(Entry { word, reading });
    }
    if out.len() > MAX_ENTRIES {
        return Err(AppError::Io(format!("登録できるのは{MAX_ENTRIES}語までです")));
    }
    Ok(out)
}

/// 整形してユーザー辞書を保存し、保存した内容を返す。途中で失敗しても元のファイルは壊さない。
pub fn save_user(app_data: &Path, entries: Vec<Entry>) -> Result<Vec<Entry>, AppError> {
    let entries = normalize(entries)?;
    let json = serde_json::to_string_pretty(&entries).map_err(|e| AppError::Io(e.to_string()))?;
    std::fs::create_dir_all(app_data).map_err(|e| AppError::Io(e.to_string()))?;
    let tmp = app_data.join(format!("{DICTIONARY_FILE}.tmp"));
    std::fs::write(&tmp, json).map_err(|e| AppError::Io(e.to_string()))?;
    std::fs::rename(&tmp, app_data.join(DICTIONARY_FILE)).map_err(|e| AppError::Io(e.to_string()))?;
    Ok(entries)
}

/// 置換する語と読みの対応。長い語から順に照合する。
pub struct Dictionary {
    entries: Vec<(Vec<char>, String)>,
}

impl Dictionary {
    /// 組み込みの読みに、アプリデータのユーザー辞書を重ねて作る。ファイルが無い・壊れている場合は組み込みのみ。
    pub fn load(app_data: &Path) -> Self {
        Self::new(load_user(app_data).into_iter().map(|e| (e.word, e.reading)).collect())
    }

    fn new(user: Vec<(String, String)>) -> Self {
        let mut map: HashMap<String, (String, String)> = BUILTIN
            .iter()
            .map(|(w, r)| (w.to_lowercase(), ((*w).to_owned(), (*r).to_owned())))
            .collect();
        for (w, r) in user {
            if !w.is_empty() {
                map.insert(w.to_lowercase(), (w, r));
            }
        }
        let mut entries: Vec<(Vec<char>, String)> = map
            .into_values()
            .map(|(w, r)| (w.to_lowercase().chars().collect(), r))
            .collect();
        entries.sort_by_key(|(w, _)| std::cmp::Reverse(w.len()));
        Self { entries }
    }

    /// 辞書の語を読みへ置き換える。大文字小文字は区別せず、前後が英数字でつながる場合(`GitHubber`など)は置き換えない。
    pub fn apply(&self, text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let lower: Vec<char> = text.to_lowercase().chars().collect();
        // 小文字化で文字数が変わる文字(İなど)があると位置がずれるため、その場合は置換しない
        if lower.len() != chars.len() {
            return text.to_owned();
        }
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while i < chars.len() {
            let starts_word = i == 0 || !is_word(chars[i - 1]);
            let hit = starts_word
                .then(|| {
                    self.entries.iter().find(|(w, _)| {
                        let end = i + w.len();
                        lower[i..].starts_with(w) && chars.get(end).is_none_or(|c| !is_word(*c))
                    })
                })
                .flatten();
            match hit {
                Some((w, reading)) => {
                    out.push_str(reading);
                    i += w.len();
                }
                None => {
                    out.push(chars[i]);
                    i += 1;
                }
            }
        }
        out
    }
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict(user: &[(&str, &str)]) -> Dictionary {
        Dictionary::new(user.iter().map(|(w, r)| ((*w).to_owned(), (*r).to_owned())).collect())
    }

    #[test]
    fn replaces_builtin_words() {
        let d = dict(&[]);
        assert_eq!(d.apply("GitHubのIssueを見て"), "ギットハブのIssueを見て");
        assert_eq!(d.apply("claudeとGOOGLE"), "クロードとグーグル");
    }

    #[test]
    fn user_entry_overrides_builtin() {
        let d = dict(&[("Rust", "ラスト言語"), ("Foo", "フー")]);
        assert_eq!(d.apply("Rust and Foo"), "ラスト言語 and フー");
    }

    #[test]
    fn skips_partial_matches() {
        let d = dict(&[]);
        assert_eq!(d.apply("GitHubber trust Rusty"), "GitHubber trust Rusty");
        assert_eq!(d.apply("(Rust)"), "(ラスト)");
    }

    #[test]
    fn prefers_longest_word() {
        let d = dict(&[("Git", "ギット"), ("Git Hub", "ギットハブ")]);
        assert_eq!(d.apply("Git Hub"), "ギットハブ");
        assert_eq!(d.apply("Git"), "ギット");
    }

    #[test]
    fn missing_or_broken_file_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Dictionary::load(dir.path()).apply("Slack"), "スラック");
        std::fs::write(dir.path().join(DICTIONARY_FILE), "not json").unwrap();
        assert_eq!(Dictionary::load(dir.path()).apply("Slack"), "スラック");
        std::fs::write(dir.path().join(DICTIONARY_FILE), r#"[{"word":"Slack","reading":"スラッく"}]"#).unwrap();
        assert_eq!(Dictionary::load(dir.path()).apply("Slack"), "スラッく");
    }

    fn entry(w: &str, r: &str) -> Entry {
        Entry { word: w.into(), reading: r.into() }
    }

    #[test]
    fn save_normalizes_and_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let saved = save_user(
            dir.path(),
            vec![entry(" Figma ", " フィグマ "), entry("", "x"), entry("a", " "), entry("FIGMA", "フィグマ2")],
        )
        .unwrap();
        assert_eq!(saved, vec![entry("FIGMA", "フィグマ2")]);
        assert_eq!(load_user(dir.path()), saved);
        assert_eq!(Dictionary::load(dir.path()).apply("figma"), "フィグマ2");
    }

    #[test]
    fn save_rejects_oversized() {
        let dir = tempfile::tempdir().unwrap();
        assert!(save_user(dir.path(), vec![entry(&"a".repeat(65), "あ")]).is_err());
        let many = (0..501).map(|i| entry(&format!("w{i}"), "あ")).collect();
        assert!(save_user(dir.path(), many).is_err());
    }
}
