//! 常時音声待機の起動ワード検知。応答音声は`voice_phrases`の定型音声を使う。
//!
//! 検知は文字起こしの直後、LLMによる校正より前に行う。起動ワードを含まない発話は
//! LLMへ一切送らないことで、常時待機中の利用料金を抑える。
use serde::{Deserialize, Serialize};

use crate::{db::Db, utils::error::AppError};

pub const ENABLED_KEY: &str = "wake_enabled";
pub const WORDS_KEY: &str = "wake_words";
/// 登録できる起動ワードの数。
pub const MAX_WORDS: usize = 10;
/// 起動ワードの長さ(表記ゆれを揃えた後の文字数)の範囲。短すぎると普通の会話で誤検知する。
const WORD_CHARS: std::ops::RangeInclusive<usize> = 2..=20;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WakeConfig {
    pub enabled: bool,
    pub words: Vec<String>,
}

/// 表記ゆれを揃えた1文字を返す。句読点・空白・長音符などは`None`(比較から除く)。
/// 全角英数は半角へ、カタカナはひらがなへ、英字は小文字へ寄せる。
fn fold(c: char) -> Option<char> {
    let c = match c {
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0)?,
        '\u{30A1}'..='\u{30F6}' => char::from_u32(c as u32 - 0x60)?,
        _ => c,
    };
    (c.is_alphanumeric() && c != 'ー').then(|| c.to_lowercase().next().unwrap_or(c))
}

/// 比較用に揃えた文字列と、各文字が元の文字列の何文字目に由来するか。
fn fold_with_origin(text: &str) -> (Vec<char>, Vec<usize>) {
    let mut folded = Vec::new();
    let mut origin = Vec::new();
    for (i, c) in text.chars().enumerate() {
        if let Some(f) = fold(c) {
            folded.push(f);
            origin.push(i);
        }
    }
    (folded, origin)
}

/// 登録できる形に整える。前後の空白を除き、短すぎる・長すぎる語、表記ゆれを除いて重複する語を捨て、上限まで残す。
pub fn sanitize_words(words: &[String]) -> Vec<String> {
    let mut seen: Vec<Vec<char>> = Vec::new();
    let mut out = Vec::new();
    for w in words {
        let w = w.trim();
        let (folded, _) = fold_with_origin(w);
        if !WORD_CHARS.contains(&folded.len()) || seen.contains(&folded) {
            continue;
        }
        seen.push(folded);
        out.push(w.to_owned());
        if out.len() == MAX_WORDS {
            break;
        }
    }
    out
}

/// `text`に起動ワードが含まれていれば、そのワードより後ろの文章(依頼の続き)を返す。
/// 続きがなければ空文字。複数含まれる場合は最初に現れたもの(同位置なら長い方)を使う。
pub fn detect(text: &str, words: &[String]) -> Option<String> {
    let (folded, origin) = fold_with_origin(text);
    let (end, _) = words
        .iter()
        .filter_map(|w| {
            let (word, _) = fold_with_origin(w);
            if word.is_empty() {
                return None;
            }
            let start = folded.windows(word.len()).position(|win| win == word.as_slice())?;
            Some((start + word.len(), (start, std::cmp::Reverse(word.len()))))
        })
        .min_by_key(|(_, rank)| *rank)?;
    let after = origin.get(end - 1).map_or(0, |i| i + 1);
    let rest: String = text.chars().skip(after).collect();
    Some(rest.trim_start_matches(|c: char| !c.is_alphanumeric()).trim().to_owned())
}

pub fn load(db: &Db) -> Result<WakeConfig, AppError> {
    let words = db
        .get_setting(WORDS_KEY)?
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default();
    Ok(WakeConfig { enabled: db.get_setting(ENABLED_KEY)?.as_deref() == Some("1"), words })
}

/// 保存し、実際に保存した設定を返す。起動ワードがなければ待機は有効にしない。
pub fn save(db: &Db, config: &WakeConfig) -> Result<WakeConfig, AppError> {
    let words = sanitize_words(&config.words);
    let enabled = config.enabled && !words.is_empty();
    db.set_setting(WORDS_KEY, &serde_json::to_string(&words).map_err(|e| AppError::Db(e.to_string()))?)?;
    db.set_setting(ENABLED_KEY, if enabled { "1" } else { "0" })?;
    Ok(WakeConfig { enabled, words })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(w: &[&str]) -> Vec<String> {
        w.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn detects_across_script_and_width_variants() {
        let w = words(&["たちこま"]);
        assert_eq!(detect("タチコマ", &w).as_deref(), Some(""));
        assert_eq!(detect("ねえ、タチコマ。", &w).as_deref(), Some(""));
        assert_eq!(detect("Ｈｅｙ ＡＩ", &words(&["hey ai"])).as_deref(), Some(""));
        assert!(detect("今日はいい天気ですね", &w).is_none());
        // 登録がカタカナでも、認識結果がひらがなでも一致する
        assert_eq!(detect("たちこま", &words(&["タチコマ"])).as_deref(), Some(""));
        assert_eq!(detect("ガッチャン、おはよう", &words(&["がっちゃん"])).as_deref(), Some("おはよう"));
    }

    #[test]
    fn returns_the_request_following_the_wake_word() {
        let w = words(&["タチコマ"]);
        assert_eq!(detect("タチコマ、今日の天気は?", &w).as_deref(), Some("今日の天気は?"));
        assert_eq!(detect("ねえ たち こま 明日の予定", &w).as_deref(), Some("明日の予定"));
    }

    #[test]
    fn earliest_then_longest_word_wins() {
        let w = words(&["こんにちは", "ねえタチコマ", "タチコマ"]);
        assert_eq!(detect("ねえタチコマ、お願い", &w).as_deref(), Some("お願い"));
        assert_eq!(detect("タチコマ、こんにちは", &w).as_deref(), Some("こんにちは"));
    }

    #[test]
    fn sanitize_drops_short_duplicate_and_excess_words() {
        let got = sanitize_words(&words(&[" タチコマ ", "たちこま", "あ", "", "ヘイ、AI"]));
        assert_eq!(got, words(&["タチコマ", "ヘイ、AI"]));
        let many: Vec<String> = (0..30).map(|i| format!("ワード{i}")).collect();
        assert_eq!(sanitize_words(&many).len(), MAX_WORDS);
    }

}
