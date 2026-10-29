//! Google Cloud Speech-to-Text(REST・同期認識)。キーはTTSと共通のGoogle Cloudキーを使う。
use std::time::Duration;

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

use super::{audio::WHISPER_RATE, debug_log, secrets};
use crate::utils::error::AppError;

/// 同期認識の上限は約1分。余裕を見て超えたら送らない。
const MAX_SECONDS: usize = 58;

/// 16kHz mono f32をLINEAR16(リトルエンディアン)へ変換する。
fn to_pcm16(samples: &[f32]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|s| ((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes())
        .collect()
}

/// `auto`は日本語を主言語にして英語を候補に加える。
fn request_body(samples: &[f32], language: &str) -> Value {
    let mut config = json!({
        "encoding": "LINEAR16",
        "sampleRateHertz": WHISPER_RATE,
        "audioChannelCount": 1,
        "enableAutomaticPunctuation": true,
    });
    match language {
        "auto" => {
            config["languageCode"] = json!("ja-JP");
            config["alternativeLanguageCodes"] = json!(["en-US"]);
        }
        "ja" => config["languageCode"] = json!("ja-JP"),
        "en" => config["languageCode"] = json!("en-US"),
        "zh" => config["languageCode"] = json!("zh-CN"),
        "ko" => config["languageCode"] = json!("ko-KR"),
        other => config["languageCode"] = json!(other),
    }
    json!({ "config": config, "audio": { "content": STANDARD.encode(to_pcm16(samples)) } })
}

/// 各区間の第一候補をつなぐ。発話がなければ空文字列。
fn extract_transcript(body: &Value) -> String {
    body.get("results")
        .and_then(Value::as_array)
        .map(|rs| {
            rs.iter()
                .filter_map(|r| r.pointer("/alternatives/0/transcript")?.as_str())
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_owned()
        })
        .unwrap_or_default()
}

pub async fn transcribe(samples: &[f32], language: &str) -> Result<String, AppError> {
    transcribe_inner(samples, language).await.map_err(|e| e.with_source("google_stt"))
}

async fn transcribe_inner(samples: &[f32], language: &str) -> Result<String, AppError> {
    if samples.len() > MAX_SECONDS * WHISPER_RATE as usize {
        return Err(AppError::TooLong);
    }
    let key = secrets::get_tts_key()?.ok_or_else(|| AppError::KeyMissing("google_tts".into()))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| AppError::Network)?;
    let url = "https://speech.googleapis.com/v1/speech:recognize";
    let request = request_body(samples, language);
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
        let mut logged = request.clone();
        debug_log::redact_at(&mut logged, "/audio/content");
        debug_log::record_api("google_stt", url, &logged, status.as_u16(), &debug_log::parse_body(&raw));
    }
    if !status.is_success() {
        return Err(AppError::from_google(status.as_u16(), &raw));
    }
    let body: Value = serde_json::from_str(&raw).map_err(|_| AppError::BadResponse)?;
    Ok(extract_transcript(&body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_pcm16_little_endian() {
        assert_eq!(to_pcm16(&[0.0, 1.0, -2.0]), [0, 0, 0xFF, 0x7F, 0x01, 0x80]);
    }

    #[test]
    fn builds_request_by_language() {
        let auto = request_body(&[0.0; 4], "auto");
        assert_eq!(auto["config"]["languageCode"], "ja-JP");
        assert_eq!(auto["config"]["alternativeLanguageCodes"][0], "en-US");
        assert_eq!(auto["config"]["sampleRateHertz"], 16_000);
        assert_eq!(request_body(&[0.0; 4], "en")["config"]["languageCode"], "en-US");
    }

    #[test]
    fn joins_first_alternatives() {
        let body = json!({ "results": [
            { "alternatives": [{ "transcript": "こんにちは" }, { "transcript": "x" }] },
            { "alternatives": [{ "transcript": " 世界 " }] },
        ]});
        assert_eq!(extract_transcript(&body), "こんにちは  世界");
        assert_eq!(extract_transcript(&json!({})), "");
    }
}
