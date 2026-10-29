//! 音声合成。Google Cloud Text-to-Speech(REST)はbase64のMP3、VOICEVOX CORE([`super::voicevox`])はWAVで返す。
use std::time::Duration;

use serde_json::{Value, json};

use super::{debug_log, secrets};
use crate::utils::error::AppError;

/// TTS APIの入力上限は5000バイト。
const MAX_INPUT_BYTES: usize = 4500;

/// 読み上げに使うエンジン。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Voice {
    Google,
    /// VOICEVOXのスタイルID。
    Voicevox(u32),
}

impl Voice {
    /// 定型音声のファイル名に使う識別子。声か文言が変わると別の音声として扱う。
    pub fn id(&self, text: &str) -> String {
        match self {
            Self::Google => voice_for(text).1.to_owned(),
            Self::Voicevox(style) => format!("voicevox-{style}"),
        }
    }

    /// 保存する音声ファイルの拡張子。
    pub fn ext(&self) -> &'static str {
        match self {
            Self::Google => "mp3",
            Self::Voicevox(_) => "wav",
        }
    }
}

/// 読み上げる本文。長文はGoogleの入力上限に合わせて切る(VOICEVOXも同じ長さに揃え、字幕の同期範囲を変えない)。
pub fn spoken_text(text: &str) -> &str {
    truncate_utf8(text, MAX_INPUT_BYTES)
}

fn truncate_utf8(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// かな・漢字を含めば日本語、それ以外は英語の音声を使う。
pub fn voice_for(text: &str) -> (&'static str, &'static str) {
    let is_ja = text.chars().any(|c| matches!(c, '\u{3040}'..='\u{30FF}' | '\u{4E00}'..='\u{9FFF}'));
    if is_ja { ("ja-JP", "ja-JP-Neural2-B") } else { ("en-US", "en-US-Neural2-F") }
}

fn request_body(text: &str) -> Value {
    let (lang, name) = voice_for(text);
    json!({
        "input": { "text": truncate_utf8(text, MAX_INPUT_BYTES) },
        "voice": { "languageCode": lang, "name": name },
        "audioConfig": { "audioEncoding": "MP3" },
    })
}

fn extract_audio(body: &Value) -> Result<String, AppError> {
    body.get("audioContent")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or(AppError::BadResponse)
}

pub async fn synthesize(text: &str) -> Result<String, AppError> {
    synthesize_inner(text).await.map_err(|e| e.with_source("google_tts"))
}

async fn synthesize_inner(text: &str) -> Result<String, AppError> {
    if text.trim().is_empty() {
        return Err(AppError::EmptyText);
    }
    let key = secrets::get_tts_key()?.ok_or_else(|| AppError::KeyMissing("google_tts".into()))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| AppError::Network)?;
    let url = "https://texttospeech.googleapis.com/v1/text:synthesize";
    let request = request_body(text);
    let res = client
        .post(url)
        .header("x-goog-api-key", key)
        .json(&request)
        .send()
        .await
        .map_err(AppError::from_reqwest)?;
    let status = res.status();
    let raw = res.text().await.unwrap_or_default();
    if debug_log::enabled() {
        let mut logged = debug_log::parse_body(&raw);
        debug_log::redact_at(&mut logged, "/audioContent");
        debug_log::record_api("google_tts", url, &request, status.as_u16(), &logged);
    }
    if !status.is_success() {
        return Err(AppError::from_google(status.as_u16(), &raw));
    }
    let body: Value = serde_json::from_str(&raw).map_err(|_| AppError::BadResponse)?;
    extract_audio(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_voice_by_script() {
        assert_eq!(voice_for("こんにちは").0, "ja-JP");
        assert_eq!(voice_for("hello").0, "en-US");
    }

    #[test]
    fn truncates_on_char_boundary() {
        let s = "あ".repeat(2000); // 6000 bytes
        let t = truncate_utf8(&s, MAX_INPUT_BYTES);
        assert!(t.len() <= MAX_INPUT_BYTES);
        assert_eq!(t.len() % 3, 0);
    }

    #[test]
    fn voice_id_follows_engine() {
        assert_eq!(Voice::Google.id("こんにちは"), "ja-JP-Neural2-B");
        assert_eq!(Voice::Voicevox(3).id("こんにちは"), "voicevox-3");
        assert_eq!((Voice::Google.ext(), Voice::Voicevox(3).ext()), ("mp3", "wav"));
    }

    #[test]
    fn parses_response() {
        assert_eq!(extract_audio(&json!({ "audioContent": "QUJD" })).unwrap(), "QUJD");
        assert!(matches!(extract_audio(&json!({})), Err(AppError::BadResponse)));
        assert_eq!(request_body("hi")["audioConfig"]["audioEncoding"], "MP3");
    }
}
