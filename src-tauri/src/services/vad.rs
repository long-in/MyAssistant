//! Silero VADによる発話判定。振幅では区別できない雑音(空調・打鍵・衣擦れ)をSTTへ回さず、
//! Whisperの幻聴(「ご視聴ありがとうございました」等の定型句)を防ぐ。
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use whisper_rs::{WhisperVadContext, WhisperVadContextParams, WhisperVadParams};

use crate::utils::error::AppError;

/// アプリに同梱するモデル(`tauri.conf.json`の`bundle.resources`)。
pub const MODEL_FILE: &str = "ggml-silero-v5.1.2.bin";

pub fn model_path(resource_dir: &Path) -> PathBuf {
    resource_dir.join("resources").join(MODEL_FILE)
}

/// 発話判定のパラメータ。Whisper内部のVADにも同じ値を使う。
/// これより短い有声区間は雑音として捨てる。
/// `short`は承認への返事向けで、「はい」だけのような0.3秒に満たない発話も残すよう、閾値と最短の有声区間を緩める。
pub fn params(short: bool) -> WhisperVadParams {
    let mut p = WhisperVadParams::new();
    if short {
        p.set_threshold(0.35);
        p.set_min_speech_duration(80);
        p.set_speech_pad(100);
    } else {
        p.set_threshold(0.5);
        p.set_min_speech_duration(250);
        p.set_speech_pad(30);
    }
    p
}

/// モデルのロードは初回のみ行い使い回す。
#[derive(Default)]
pub struct VadEngine(Mutex<Option<WhisperVadContext>>);

impl VadEngine {
    /// ロード済みのモデルを解放する(終了時のMetalリソース解放漏れによるabort対策。`SttEngine::unload`参照)。
    pub fn unload(&self) {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).take();
    }

    /// `samples`(16kHz mono f32)に発話が含まれるか。モデルが無い・ロードできない場合はエラー。
    /// `short`は短い返事向けに判定を緩める(`params`参照)。
    pub fn has_speech(&self, model: &Path, samples: &[f32], short: bool) -> Result<bool, AppError> {
        let mut slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_none() {
            if !model.is_file() {
                return Err(AppError::Stt(format!("VAD model not found: {}", model.display())));
            }
            let path = model.to_str().ok_or_else(|| AppError::Stt("non-utf8 VAD model path".into()))?;
            let ctx = WhisperVadContext::new(path, WhisperVadContextParams::default())
                .map_err(|e| AppError::Stt(e.to_string()))?;
            *slot = Some(ctx);
        }
        let ctx = slot.as_mut().expect("VAD context is loaded above");
        let segments = ctx.segments_from_samples(params(short), samples).map_err(|e| AppError::Stt(e.to_string()))?;
        Ok(segments.num_segments() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_is_an_error() {
        let vad = VadEngine::default();
        assert!(vad.has_speech(Path::new("/nonexistent/vad.bin"), &[0.0; 16_000], false).is_err());
    }

    fn bundled_model() -> PathBuf {
        model_path(Path::new(env!("CARGO_MANIFEST_DIR")))
    }

    /// 同梱モデルで、声の無い入力を発話とみなさないこと。
    #[test]
    #[ignore]
    fn rejects_non_speech() {
        let vad = VadEngine::default();
        let model = bundled_model();
        assert!(!vad.has_speech(&model, &[0.0; 32_000], false).unwrap());
        // 白色雑音(線形合同法で生成。外部クレート不要)
        let mut x = 12345u32;
        let noise: Vec<f32> = (0..48_000)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (x >> 8) as f32 / (1u32 << 24) as f32 - 0.5
            })
            .collect();
        assert!(!vad.has_speech(&model, &noise, false).unwrap());
        let tone: Vec<f32> = (0..48_000).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 16_000.0).sin() * 0.5).collect();
        assert!(!vad.has_speech(&model, &tone, false).unwrap());
    }

    /// `WHISPER_WAV`(16kHz mono 16bit WAV、発話入り)を発話とみなすこと。
    #[test]
    #[ignore]
    fn accepts_real_speech() {
        let wav = std::env::var("WHISPER_WAV").unwrap();
        let samples: Vec<f32> = hound::WavReader::open(wav)
            .unwrap()
            .into_samples::<i16>()
            .map(|s| s.unwrap() as f32 / i16::MAX as f32)
            .collect();
        assert!(VadEngine::default().has_speech(&bundled_model(), &samples, false).unwrap());
    }
}
