//! ローカルWhisperによる音声認識。
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use tokio::io::AsyncWriteExt;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::{
    db::Db,
    services::vad,
    utils::{error::AppError, version},
};

/// モデルを取得するHugging Faceのリビジョン(ブランチ・タグ・コミット)の設定キー。
pub const REVISION_KEY: &str = "whisper_revision";
pub const DEFAULT_REVISION: &str = "main";

/// 設定のリビジョン。未設定・不正な値は既定値(壊れた設定で取得先を変えさせない)。
pub fn revision(db: &Db) -> Result<String, AppError> {
    Ok(db.get_setting(REVISION_KEY)?.filter(|r| version::validate(r).is_ok()).unwrap_or_else(|| DEFAULT_REVISION.into()))
}

pub fn set_revision(db: &Db, revision: &str) -> Result<(), AppError> {
    version::validate(revision)?;
    db.set_setting(REVISION_KEY, revision)
}

/// 使えるモデル。先頭ほど優先する(日本語の精度が高い量子化版large-v3-turbo、次にsmall)。
/// 最小サイズは破損・途中切れの検出用(実サイズはそれぞれ約574MB、約466MB)。
const MODELS: [Model; 2] = [
    Model { file: "ggml-large-v3-turbo-q5_0.bin", min_bytes: 500 * 1024 * 1024 },
    Model { file: "ggml-small.bin", min_bytes: 400 * 1024 * 1024 },
];

struct Model {
    file: &'static str,
    min_bytes: u64,
}

impl Model {
    fn url(&self, revision: &str) -> String {
        format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/{revision}/{}", self.file)
    }

    fn path(&self, app_data_dir: &Path) -> PathBuf {
        app_data_dir.join("models").join(self.file)
    }

    fn ready(&self, app_data_dir: &Path) -> bool {
        std::fs::metadata(self.path(app_data_dir)).map(|m| m.len() >= self.min_bytes).unwrap_or(false)
    }
}

/// 認識に使うモデル。インストール済みのうち最も優先度が高いもの。なければ優先モデルの置き場所。
pub fn model_path(app_data_dir: &Path) -> PathBuf {
    MODELS.iter().find(|m| m.ready(app_data_dir)).unwrap_or(&MODELS[0]).path(app_data_dir)
}

/// 優先モデルの置き場所。
pub fn preferred_model_path(app_data_dir: &Path) -> PathBuf {
    MODELS[0].path(app_data_dir)
}

/// 認識に使えるモデルがインストール済みか。
pub fn model_ready(path: &Path) -> bool {
    let min = MODELS.iter().find(|m| path.file_name().is_some_and(|f| f == m.file)).map_or(MODELS[0].min_bytes, |m| m.min_bytes);
    std::fs::metadata(path).map(|m| m.len() >= min).unwrap_or(false)
}

/// 優先モデルより下位のモデルしか入っていない(優先モデルへ更新できる)か。
pub fn model_upgradable(app_data_dir: &Path) -> bool {
    !MODELS[0].ready(app_data_dir) && MODELS[1..].iter().any(|m| m.ready(app_data_dir))
}

/// 一時ファイルへ書き出してからリネームする(中断時に壊れたモデルを残さない)。
pub async fn download_model(path: &Path, revision: &str) -> Result<(), AppError> {
    version::validate(revision)?;
    let model = MODELS
        .iter()
        .find(|m| path.file_name().is_some_and(|f| f == m.file))
        .ok_or_else(|| AppError::Stt("unknown model file".into()))?;
    let dir = path.parent().ok_or_else(|| AppError::Stt("invalid model path".into()))?;
    tokio::fs::create_dir_all(dir).await.map_err(|e| AppError::Stt(e.to_string()))?;
    let mut res = reqwest::get(model.url(revision)).await.map_err(AppError::from_reqwest)?;
    if !res.status().is_success() {
        return Err(AppError::from_status(res.status().as_u16()));
    }
    let tmp = path.with_extension("part");
    let mut file = tokio::fs::File::create(&tmp).await.map_err(|e| AppError::Stt(e.to_string()))?;
    while let Some(chunk) = res.chunk().await.map_err(AppError::from_reqwest)? {
        file.write_all(&chunk).await.map_err(|e| AppError::Stt(e.to_string()))?;
    }
    file.flush().await.map_err(|e| AppError::Stt(e.to_string()))?;
    drop(file);
    let size = tokio::fs::metadata(&tmp).await.map(|m| m.len()).unwrap_or(0);
    if size < model.min_bytes {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(AppError::BadResponse);
    }
    tokio::fs::rename(&tmp, path).await.map_err(|e| AppError::Stt(e.to_string()))
}

/// 無音とみなす最大振幅。小音量のマイクでも発話は上回る(超えたものは後段で増幅する)。
const SILENCE_PEAK: f32 = 0.004;

/// 録音全体が実質無音か。Whisperを呼ぶ前に弾いて、幻聴(無音への勝手な文字起こし)を防ぐ。
pub fn is_silent(samples: &[f32]) -> bool {
    samples.iter().all(|s| s.abs() < SILENCE_PEAK)
}

/// 録音の最大振幅。診断用。
pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0, |m, s| m.max(s.abs()))
}

/// 振幅の99.9パーセンタイル。単発のクリック音に引きずられないレベル指標。
fn level(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut abs: Vec<f32> = samples.iter().map(|s| s.abs()).collect();
    let idx = ((abs.len() - 1) as f32 * 0.999) as usize;
    *abs.select_nth_unstable_by(idx, |a, b| a.total_cmp(b)).1
}

/// 入力が小さいマイク向けに、レベルが0.5程度になるよう増幅する(無音・十分大きい入力は変更しない)。
/// 増幅後は±1.0にクリップする。
pub fn normalize(samples: &mut [f32]) {
    let l = level(samples);
    if l > 0.0 && l < 0.3 {
        let gain = 0.5 / l;
        samples.iter_mut().for_each(|s| *s = (*s * gain).clamp(-1.0, 1.0));
    }
}

/// Whisperが非発話区間に出す`[BLANK_AUDIO]`や`(音楽)`などの注釈と、♪を取り除く。
/// 発話が残らなければ空文字列を返す。
pub fn clean_transcript(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0u32;
    for c in text.chars() {
        match c {
            '[' | '(' | '（' | '【' => depth += 1,
            ']' | ')' | '）' | '】' => depth = depth.saturating_sub(1),
            '♪' | '♫' => {}
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// ビームサーチの幅。広いほど精度が上がるが遅くなる(Metalで高速化している前提で控えめにする)。
const BEAM_SIZE: i32 = 3;

/// モデルのロードは重いため、初回のみ行い使い回す。
#[derive(Default)]
pub struct SttEngine(Mutex<Option<(PathBuf, Arc<WhisperContext>)>>);

impl SttEngine {
    /// ロード済みのモデルを解放する。プロセス終了時にggmlのMetalデバイスの静的デストラクタが
    /// 未解放のリソースを検出してabortするため、終了前に呼ぶ。
    pub fn unload(&self) {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).take();
    }

    fn context(&self, model: &Path) -> Result<Arc<WhisperContext>, AppError> {
        let mut slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
        // モデルが入れ替わったら(更新ダウンロード後など)読み直す
        if let Some((_, ctx)) = slot.as_ref().filter(|(loaded, _)| loaded == model) {
            return Ok(ctx.clone());
        }
        if !model_ready(model) {
            return Err(AppError::SttModelMissing);
        }
        let path = model.to_str().ok_or_else(|| AppError::Stt("non-utf8 model path".into()))?;
        let ctx = Arc::new(
            WhisperContext::new_with_params(path, WhisperContextParameters::default())
                .map_err(|e| AppError::Stt(e.to_string()))?,
        );
        *slot = Some((model.to_path_buf(), ctx.clone()));
        Ok(ctx)
    }

    /// `samples`は16kHz mono f32。CPU負荷が高いためブロッキングスレッドで呼ぶこと。
    /// `vad_model`があれば発話区間だけを認識する(発話後の無音に定型句を足す幻聴を防ぐ)。
    /// `prompt`は認識の手がかりとなる語彙(起動ワードや訂正済みの用語)。短く保つこと。
    /// `short`は承認への短い返事向けで、VADの判定を緩める。
    pub fn transcribe(
        &self,
        model: &Path,
        vad_model: Option<&Path>,
        prompt: Option<&str>,
        samples: &[f32],
        language: &str,
        short: bool,
    ) -> Result<String, AppError> {
        let ctx = self.context(model)?;
        let mut state = ctx.create_state().map_err(|e| AppError::Stt(e.to_string()))?;
        let mut params = FullParams::new(SamplingStrategy::BeamSearch { beam_size: BEAM_SIZE, patience: -1.0 });
        params.set_language(Some(language));
        if let Some(vad) = vad_model.filter(|p| p.is_file()).and_then(Path::to_str) {
            params.set_vad_model_path(Some(vad));
            params.set_vad_params(vad::params(short));
            params.enable_vad(true);
        }
        if let Some(prompt) = prompt.filter(|p| !p.is_empty()) {
            params.set_initial_prompt(prompt);
        }
        // 直前の文脈に引きずられた定型句の連鎖と、非発話トークンの出力を抑える
        params.set_no_context(true);
        params.set_suppress_nst(true);
        params.set_no_speech_thold(0.6);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        state.full(params, samples).map_err(|e| AppError::Stt(e.to_string()))?;
        let text: String = state.as_iter().map(|s| s.to_string()).collect();
        Ok(clean_transcript(&text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_url_uses_the_revision() {
        assert_eq!(MODELS[1].url("v1.8.0"), "https://huggingface.co/ggerganov/whisper.cpp/resolve/v1.8.0/ggml-small.bin");
    }

    #[test]
    fn strips_non_speech_annotations() {
        assert_eq!(clean_transcript(" [BLANK_AUDIO]"), "");
        assert_eq!(clean_transcript("(音楽)"), "");
        assert_eq!(clean_transcript("（拍手）"), "");
        assert_eq!(clean_transcript("[ Silence ]"), "");
        assert_eq!(clean_transcript("♪~"), "~");
        assert_eq!(clean_transcript("こんにちは [BLANK_AUDIO]"), "こんにちは");
        assert_eq!(clean_transcript("Hello,  world."), "Hello, world.");
        assert_eq!(clean_transcript("「こんにちは」と言った"), "「こんにちは」と言った");
    }

    #[test]
    fn normalizes_quiet_input_only() {
        let mut quiet = vec![0.1f32; 1000];
        quiet[0] = -0.1;
        normalize(&mut quiet);
        assert!((peak(&quiet) - 0.5).abs() < 1e-6);
        let mut loud = [0.0, 0.8];
        normalize(&mut loud);
        assert_eq!(loud, [0.0, 0.8]);
        // 単発のクリックがあっても、通常の発話レベルを基準に増幅する
        let mut clicky: Vec<f32> = vec![0.05; 10_000];
        clicky[0] = 0.9;
        normalize(&mut clicky);
        assert!(clicky[5] > 0.4 && clicky[0] <= 1.0);
        let mut silent = [0.0; 4];
        normalize(&mut silent);
        assert_eq!(silent, [0.0; 4]);
    }

    #[test]
    fn detects_silence() {
        assert!(is_silent(&[0.0; 100]));
        assert!(is_silent(&[0.003, -0.002]));
        assert!(!is_silent(&[0.0, 0.2, 0.0]));
        assert!(is_silent(&[]));
    }

    #[test]
    fn prefers_the_best_installed_model() {
        let dir = std::env::temp_dir().join(format!("stt-models-{}", std::process::id()));
        let models = dir.join("models");
        std::fs::create_dir_all(&models).unwrap();
        let fill = |name: &str, bytes: u64| std::fs::File::create(models.join(name)).unwrap().set_len(bytes).unwrap();
        // 何も無ければ優先モデルの置き場所(ダウンロード先)を返し、更新対象ではない
        assert_eq!(model_path(&dir), preferred_model_path(&dir));
        assert!(!model_upgradable(&dir));
        // smallだけ入っていればそれを使い、更新できる
        fill("ggml-small.bin", 450 * 1024 * 1024);
        assert_eq!(model_path(&dir), models.join("ggml-small.bin"));
        assert!(model_ready(&model_path(&dir)) && model_upgradable(&dir));
        // 途中切れの優先モデルは無視する
        fill("ggml-large-v3-turbo-q5_0.bin", 10 * 1024 * 1024);
        assert_eq!(model_path(&dir), models.join("ggml-small.bin"));
        fill("ggml-large-v3-turbo-q5_0.bin", 574 * 1024 * 1024);
        assert_eq!(model_path(&dir), preferred_model_path(&dir));
        assert!(!model_upgradable(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_model_is_not_ready() {
        assert!(!model_ready(Path::new("/nonexistent/ggml-small.bin")));
    }

    /// `WHISPER_MODEL`(ggmlモデル)と`WHISPER_WAV`(16kHz mono 16bit WAV)が必要。
    /// 期待語は`WHISPER_EXPECT`(小文字・部分一致)。語彙ヒントは任意で`WHISPER_PROMPT`。
    #[test]
    #[ignore]
    fn transcribes_real_wav() {
        let model = PathBuf::from(std::env::var("WHISPER_MODEL").unwrap());
        let wav = std::env::var("WHISPER_WAV").unwrap();
        let expect = std::env::var("WHISPER_EXPECT").unwrap().to_lowercase();
        let mut samples: Vec<f32> = hound::WavReader::open(wav)
            .unwrap()
            .into_samples::<i16>()
            .map(|s| s.unwrap() as f32 / i16::MAX as f32)
            .collect();
        normalize(&mut samples); // 実運用(コマンド)と同じ前処理
        let engine = SttEngine::default();
        let lang = std::env::var("WHISPER_LANG").unwrap_or_else(|_| "auto".into());
        let vad_model = vad::model_path(Path::new(env!("CARGO_MANIFEST_DIR")));
        let prompt = std::env::var("WHISPER_PROMPT").ok();
        let text = engine.transcribe(&model, Some(&vad_model), prompt.as_deref(), &samples, &lang, false).unwrap();
        println!("transcript: {text}");
        assert!(text.to_lowercase().contains(&expect), "got: {text}");
    }

    /// 実マイクで5秒録音して文字起こしする(`WHISPER_MODEL`とマイク権限が必要)。
    #[test]
    #[ignore]
    fn transcribes_from_default_mic() {
        let model = PathBuf::from(std::env::var("WHISPER_MODEL").unwrap());
        let rec = crate::services::audio::Recorder::default();
        let t = std::time::Instant::now();
        rec.start().unwrap();
        println!("start() returned after {:?}", t.elapsed());
        std::thread::sleep(std::time::Duration::from_secs(6));
        let mut samples = rec.stop().unwrap();
        normalize(&mut samples);
        let lang = std::env::var("WHISPER_LANG").unwrap_or_else(|_| "auto".into());
        let text = SttEngine::default().transcribe(&model, None, None, &samples, &lang, false).unwrap();
        println!("peak={:.4} transcript: {text}", peak(&samples));
        if let Ok(path) = std::env::var("DUMP_WAV") {
            let spec = hound::WavSpec { channels: 1, sample_rate: 16_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
            let mut w = hound::WavWriter::create(path, spec).unwrap();
            for s in &samples {
                w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).unwrap();
            }
            w.finalize().unwrap();
        }
    }
}
