//! Aqua Voice API(Avalon・同期文字起こし)。キーはGoogle Cloudとは別に保存する。
use std::time::Duration;

use reqwest::multipart::{Form, Part};
use serde_json::{Value, json};

use super::{audio::WHISPER_RATE, debug_log, secrets};
use crate::utils::error::AppError;

const BASE_URL: &str = "https://api.aquavoice.com/v1";
const MODEL: &str = "avalon-v1.5";
/// 同期リクエストの待機上限は210秒。録音は短いため余裕を見て60秒にする。
const TIMEOUT: Duration = Duration::from_secs(60);

/// 16kHz mono f32を16bit PCMのWAVへ変換する。
fn to_wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&WHISPER_RATE.to_le_bytes());
    wav.extend_from_slice(&(WHISPER_RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        wav.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    wav
}

fn extract_transcript(body: &Value) -> String {
    body.get("text").and_then(Value::as_str).unwrap_or_default().trim().to_owned()
}

/// OpenAI互換のエラー本文(`error.message`)から、原因特定用のメッセージを取り出す。
fn error_from(status: u16, body: &str) -> AppError {
    match status {
        504 => AppError::Timeout,
        400 | 413 | 422 => {
            let message = serde_json::from_str::<Value>(body)
                .ok()
                .and_then(|v| v.pointer("/error/message")?.as_str().map(|m| m.chars().take(300).collect::<String>()));
            match message {
                Some(m) => AppError::ProviderRejected(format!("{m} (HTTP {status})")),
                None => AppError::from_status(status),
            }
        }
        s => AppError::from_status(s),
    }
}

pub async fn transcribe(samples: &[f32], language: &str) -> Result<String, AppError> {
    transcribe_inner(samples, language).await.map_err(|e| e.with_source("aqua_stt"))
}

/// `auto`は言語指定をAPIの自動判定に任せる。
async fn transcribe_inner(samples: &[f32], language: &str) -> Result<String, AppError> {
    let key = secrets::get_aqua_key()?.ok_or_else(|| AppError::KeyMissing("aqua".into()))?;
    let client = reqwest::Client::builder().timeout(TIMEOUT).build().map_err(|_| AppError::Network)?;
    let url = format!("{BASE_URL}/audio/transcriptions");
    let wav = to_wav(samples);
    let wav_len = wav.len();
    let file = Part::bytes(wav).file_name("audio.wav").mime_str("audio/wav").map_err(|_| AppError::BadResponse)?;
    let form = Form::new().part("file", file).text("model", MODEL).text("language", language.to_owned());
    let res = client
        .post(&url)
        .bearer_auth(key)
        .multipart(form)
        .send()
        .await
        .map_err(AppError::from_reqwest)?;
    let status = res.status();
    let raw = res.text().await.unwrap_or_default();
    if debug_log::enabled() {
        let request = json!({ "model": MODEL, "language": language, "file": format!("audio.wav ({wav_len} bytes)") });
        debug_log::record_api("aqua_stt", &url, &request, status.as_u16(), &debug_log::parse_body(&raw));
    }
    if !status.is_success() {
        return Err(error_from(status.as_u16(), &raw));
    }
    let body: Value = serde_json::from_str(&raw).map_err(|_| AppError::BadResponse)?;
    Ok(extract_transcript(&body))
}

/// 認識結果の取得を伴わない疎通確認用に、無音の短い録音で認証だけ確かめる。
pub async fn verify_key(key: &str) -> Result<(), AppError> {
    verify_key_inner(key).await.map_err(|e| e.with_source("aqua_stt"))
}

async fn verify_key_inner(key: &str) -> Result<(), AppError> {
    let client = reqwest::Client::builder().timeout(TIMEOUT).build().map_err(|_| AppError::Network)?;
    let file = Part::bytes(to_wav(&[0.0; 1600]))
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|_| AppError::BadResponse)?;
    let form = Form::new().part("file", file).text("model", MODEL);
    let res = client
        .post(format!("{BASE_URL}/audio/transcriptions"))
        .bearer_auth(key)
        .multipart(form)
        .send()
        .await
        .map_err(AppError::from_reqwest)?;
    let status = res.status().as_u16();
    // 認証に通れば、無音の短い音声でも200または入力内容への4xx(400/422)になる
    match status {
        200..=299 | 400 | 422 => Ok(()),
        401 | 403 => Err(AppError::Unauthorized),
        s => Err(error_from(s, &res.text().await.unwrap_or_default())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_wav_header_and_samples() {
        let wav = to_wav(&[0.0, 1.0, -2.0]);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 6);
        assert_eq!(&wav[44..], [0, 0, 0xFF, 0x7F, 0x01, 0x80]);
        assert_eq!(wav.len(), 44 + 6);
    }

    #[test]
    fn reads_text_field() {
        assert_eq!(extract_transcript(&json!({ "text": " こんにちは ", "usage": {} })), "こんにちは");
        assert_eq!(extract_transcript(&json!({})), "");
    }

    #[test]
    fn maps_errors() {
        assert!(matches!(error_from(401, "{}"), AppError::Unauthorized));
        assert!(matches!(error_from(429, ""), AppError::RateLimited));
        assert!(matches!(error_from(504, ""), AppError::Timeout));
        assert!(matches!(
            error_from(400, r#"{"error":{"message":"bad file"}}"#),
            AppError::ProviderRejected(m) if m.contains("bad file")
        ));
    }
}
