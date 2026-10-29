use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::{
    commands::chat::CORRECTIONS_MAX,
    db::Db,
    services::{
        audio::Recorder,
        batch, debug_log, knowledge,
        llm_router::{self, Route},
        proofread, pronunciation, secrets, stt,
        stt::SttEngine,
        stt_aqua, stt_google, tts,
        tts::Voice,
        voicevox::{self, VoicevoxEngine},
        vad::{self, VadEngine},
        voice_phrases, wake,
    },
    utils::error::AppError,
};

pub const STT_ENGINE_KEY: &str = "stt_engine";

/// 選択できる音声認識エンジン。
pub const STT_ENGINES: [&str; 3] = ["local", "google", "aqua"];

/// 音声認識エンジンの設定値。未設定・不明な値はローカルWhisper。
fn stt_engine(db: &Db) -> Result<&'static str, AppError> {
    let saved = db.get_setting(STT_ENGINE_KEY)?;
    Ok(STT_ENGINES.into_iter().find(|e| Some(*e) == saved.as_deref()).unwrap_or("local"))
}

#[tauri::command]
pub fn get_stt_engine(db: State<'_, Db>) -> Result<&'static str, AppError> {
    stt_engine(&db)
}

#[tauri::command]
pub fn set_stt_engine(db: State<'_, Db>, engine: String) -> Result<(), AppError> {
    if STT_ENGINES.contains(&engine.as_str()) {
        db.set_setting(STT_ENGINE_KEY, &engine)
    } else {
        Err(AppError::InvalidProvider(engine))
    }
}

pub const STT_LANGUAGE_KEY: &str = "stt_language";
/// 選択できる認識言語。`auto`は自動判定(Googleは日本語+英語)。
const STT_LANGUAGES: [&str; 5] = ["auto", "ja", "en", "zh", "ko"];

fn stt_language(db: &Db) -> Result<String, AppError> {
    Ok(db
        .get_setting(STT_LANGUAGE_KEY)?
        .filter(|l| STT_LANGUAGES.contains(&l.as_str()))
        .unwrap_or_else(|| "auto".into()))
}

#[tauri::command]
pub fn get_stt_language(db: State<'_, Db>) -> Result<String, AppError> {
    stt_language(&db)
}

#[tauri::command]
pub fn set_stt_language(db: State<'_, Db>, language: String) -> Result<(), AppError> {
    if !STT_LANGUAGES.contains(&language.as_str()) {
        return Err(AppError::Stt(format!("unsupported language: {language}")));
    }
    db.set_setting(STT_LANGUAGE_KEY, &language)
}

fn model_path(app: &AppHandle) -> Result<std::path::PathBuf, AppError> {
    let dir = app.path().app_data_dir().map_err(|e| AppError::Stt(e.to_string()))?;
    Ok(stt::model_path(&dir))
}

/// 同梱のVADモデル。解決できなければ`None`(発話判定を省いてSTTは続ける)。
fn vad_model_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().resource_dir().ok().map(|dir| vad::model_path(&dir))
}

#[tauri::command]
pub fn stt_model_ready(app: AppHandle) -> Result<bool, AppError> {
    Ok(stt::model_ready(&model_path(&app)?))
}

/// 優先モデルより下位のモデルしか入っておらず、更新できるか。
#[tauri::command]
pub fn stt_model_upgradable(app: AppHandle) -> Result<bool, AppError> {
    let dir = app.path().app_data_dir().map_err(|e| AppError::Stt(e.to_string()))?;
    Ok(stt::model_upgradable(&dir))
}

/// 優先モデルをダウンロードする(インストール済みなら再取得する)。
#[tauri::command]
pub async fn download_stt_model(app: AppHandle) -> Result<(), AppError> {
    let dir = app.path().app_data_dir().map_err(|e| AppError::Stt(e.to_string()))?;
    stt::download_model(&stt::preferred_model_path(&dir)).await
}

/// 認識の手がかりとして渡す語彙の上限(文字数)。長いと無音で語彙を復唱する幻聴を招く。
const PROMPT_MAX_CHARS: usize = 100;
/// 語彙に加える訂正済みの用語の最大件数。
const PROMPT_TERMS_MAX: usize = 8;

/// 承認への返答を聞くときに、語彙の先頭へ置く返事の語。短い返事は言語や語の判定が揺れやすいため、手がかりにする。
const ANSWER_TERMS: [&str; 2] = ["はい", "いいえ"];

/// 起動ワードと訂正済みの用語から、認識の手がかりとなる語彙を作る。なければ`None`。
/// `answering`が真(承認待ち)のときは、返事の語を先頭に加える。
fn stt_prompt(app: &AppHandle, db: &Db, answering: bool) -> Option<String> {
    let mut terms: Vec<String> = if answering { ANSWER_TERMS.iter().map(|t| t.to_string()).collect() } else { Vec::new() };
    let wake_words: Vec<String> = wake::load(db).ok().filter(|c| c.enabled).map(|c| c.words).unwrap_or_default();
    terms.extend(wake_words);
    if let Ok(root) = batch::knowledge_root(app) {
        for t in knowledge::recent_correction_terms(&root, PROMPT_TERMS_MAX) {
            if !terms.contains(&t) {
                terms.push(t);
            }
        }
    }
    let mut prompt = String::new();
    for t in terms {
        let next = if prompt.is_empty() { t } else { format!("、{t}") };
        if prompt.chars().count() + next.chars().count() > PROMPT_MAX_CHARS {
            break;
        }
        prompt.push_str(&next);
    }
    (!prompt.is_empty()).then_some(prompt)
}

#[tauri::command]
pub fn start_recording(recorder: State<'_, Recorder>) -> Result<(), AppError> {
    recorder.start()
}

/// 録音を文字起こしせずに破棄する。録音中でなければ何もしない。
#[tauri::command]
pub fn cancel_recording(recorder: State<'_, Recorder>) {
    let _ = recorder.stop();
}

/// 録音中のマイク入力の直近波形(-1.0〜1.0、128点)。録音中でなければ空。
#[tauri::command]
pub fn recording_waveform(recorder: State<'_, Recorder>) -> Vec<f32> {
    recorder.waveform(128)
}

/// 校正に使うモデル指定。未設定・使用不可なら`None`(校正しない)。
#[tauri::command]
pub fn load_proofread_route(db: State<'_, Db>) -> Result<Option<Route>, AppError> {
    proofread::effective_route(&db, &batch::available_providers())
}

/// 校正モデルを登録する。`None`で校正をオフにする。
#[tauri::command]
pub fn save_proofread_route(db: State<'_, Db>, route: Option<Route>) -> Result<(), AppError> {
    let Some(route) = route else {
        return db.set_setting(proofread::ROUTE_KEY, "");
    };
    llm_router::resolve(&route, &batch::available_providers())?;
    db.set_setting(
        proofread::ROUTE_KEY,
        &serde_json::to_string(&route).map_err(|e| AppError::Db(e.to_string()))?,
    )
}

/// 録音を止めて文字起こしする(校正前の生の認識結果)。
/// 短すぎる・無音の録音はSTTにも後続のLLM送信にも回さず、原因が分かるよう区別して返す。
async fn transcribe_recording(
    app: &AppHandle,
    recorder: &Recorder,
    db: &Db,
    answering: bool,
) -> Result<String, AppError> {
    let language = stt_language(db)?;
    let mut samples = recorder.stop()?;
    let peak = stt::peak(&samples);
    log::info!("recording stopped: {:.2}s, peak={peak:.4}", samples.len() as f32 / crate::services::audio::WHISPER_RATE as f32);
    if samples.len() < crate::services::audio::WHISPER_RATE as usize / 4 {
        return Err(AppError::TooShort);
    }
    if peak == 0.0 {
        return Err(AppError::MicNoSignal);
    }
    if stt::is_silent(&samples) {
        return Err(AppError::MicTooQuiet);
    }
    stt::normalize(&mut samples);
    let engine = stt_engine(db)?;
    let seconds = samples.len() as f32 / crate::services::audio::WHISPER_RATE as f32;
    let vad_model = vad_model_path(app);
    let prompt = if engine == "local" { stt_prompt(app, db, answering) } else { None };
    // 増幅後の音声で判定する(VADは音量ではなく声らしさを見るため、小音量マイクでも取りこぼさない)
    let (samples, speech) = {
        let app = app.clone();
        let vad_model = vad_model.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let speech = vad_model.and_then(|m| match app.state::<VadEngine>().has_speech(&m, &samples, answering) {
                Ok(speech) => Some(speech),
                Err(e) => {
                    log::warn!("VAD unavailable, skipping speech gate: {e}");
                    None
                }
            });
            (samples, speech)
        })
        .await
        .map_err(|e| AppError::Stt(e.to_string()))?
    };
    if speech == Some(false) {
        debug_log::record("transcription", serde_json::json!({ "engine": engine, "seconds": seconds, "skipped": "no speech (VAD)" }));
        return Err(AppError::NoAudio);
    }
    let text = if engine == "google" {
        stt_google::transcribe(&samples, &language).await?
    } else if engine == "aqua" {
        stt_aqua::transcribe(&samples, &language).await?
    } else {
        let model = model_path(app)?;
        let app = app.clone();
        let language = language.clone();
        let prompt = prompt.clone();
        tauri::async_runtime::spawn_blocking(move || {
            app.state::<SttEngine>().transcribe(&model, vad_model.as_deref(), prompt.as_deref(), &samples, &language, answering)
        })
        .await
        .map_err(|e| AppError::Stt(e.to_string()))??
    };
    debug_log::record(
        "transcription",
        serde_json::json!({ "engine": engine, "language": language, "seconds": seconds, "prompt": prompt, "text": text }),
    );
    if text.is_empty() {
        return Err(AppError::NoAudio);
    }
    Ok(text)
}

/// 校正モデルが登録されていれば、`session_id`の直近の会話と訂正履歴を文脈に校正する。
async fn proofread_text(app: &AppHandle, db: &Db, session_id: Option<&str>, text: String) -> Result<String, AppError> {
    let Some(route) = proofread::effective_route(db, &batch::available_providers())? else {
        return Ok(text);
    };
    let corrections = batch::knowledge_root(app)
        .ok()
        .and_then(|root| knowledge::recent_corrections(&root, CORRECTIONS_MAX));
    Ok(proofread::proofread(db, &route, session_id, corrections.as_deref(), &text).await)
}

/// 録音を止めて文字起こしする。認識言語は設定(`stt_language`)に従う。
/// 校正モデルが登録されていれば、`session_id`の直近の会話と訂正履歴を文脈に校正した結果を返す。
/// `answering`(承認への返答)のときは、語彙ヒントに「はい」「いいえ」を加え、短い返事を書き換えないよう校正を省く。
#[tauri::command]
pub async fn stop_recording_and_transcribe(
    app: AppHandle,
    recorder: State<'_, Recorder>,
    db: State<'_, Db>,
    session_id: Option<String>,
    answering: Option<bool>,
) -> Result<String, AppError> {
    let answering = answering.unwrap_or(false);
    let text = transcribe_recording(&app, &recorder, &db, answering).await?;
    if answering {
        return Ok(text);
    }
    proofread_text(&app, &db, session_id.as_deref(), text).await
}

/// 常時待機中の発話の判定結果。
#[derive(Serialize)]
pub struct WakeCheck {
    /// 起動ワードが含まれていたか。
    pub woke: bool,
    /// 起動ワードに続けて話された依頼(校正済み)。なければ空。
    pub text: String,
}

/// 録音を止めて文字起こしし、起動ワードを含むか判定する。
/// 判定は校正の前に行い、含まなければLLMを呼ばない。含む場合も、校正するのは起動ワードに続く依頼だけ。
#[tauri::command]
pub async fn stop_recording_and_check_wake(
    app: AppHandle,
    recorder: State<'_, Recorder>,
    db: State<'_, Db>,
    session_id: Option<String>,
) -> Result<WakeCheck, AppError> {
    let text = transcribe_recording(&app, &recorder, &db, false).await?;
    let config = wake::load(&db)?;
    let Some(rest) = wake::detect(&text, &config.words).filter(|_| config.enabled) else {
        return Ok(WakeCheck { woke: false, text: String::new() });
    };
    let text = if rest.is_empty() { rest } else { proofread_text(&app, &db, session_id.as_deref(), rest).await? };
    Ok(WakeCheck { woke: true, text })
}

/// 直近`keep_ms`ミリ秒の入力だけを残して録音バッファを切り詰める(常時待機用)。
#[tauri::command]
pub fn trim_recording(recorder: State<'_, Recorder>, keep_ms: u32) {
    recorder.trim(keep_ms);
}

#[tauri::command]
pub fn get_wake_config(db: State<'_, Db>) -> Result<wake::WakeConfig, AppError> {
    wake::load(&db)
}

/// 常時待機の設定を保存し、実際に保存した内容(整形後)を返す。
#[tauri::command]
pub fn set_wake_config(db: State<'_, Db>, config: wake::WakeConfig) -> Result<wake::WakeConfig, AppError> {
    wake::save(&db, &config)
}

/// 読み上げ辞書(組み込みとユーザー登録)。
#[tauri::command]
pub fn get_tts_dictionary(app: AppHandle) -> Result<pronunciation::Listing, AppError> {
    Ok(pronunciation::listing(&app_data(&app)?))
}

/// ユーザー辞書を丸ごと保存し、整形後の内容を返す。
#[tauri::command]
pub fn set_tts_dictionary(app: AppHandle, entries: Vec<pronunciation::Entry>) -> Result<Vec<pronunciation::Entry>, AppError> {
    pronunciation::save_user(&app_data(&app)?, entries)
}

fn app_data(app: &AppHandle) -> Result<std::path::PathBuf, AppError> {
    app.path().app_data_dir().map_err(|e| AppError::Io(e.to_string()))
}

pub const TTS_ENGINE_KEY: &str = "tts_engine";
pub const VOICEVOX_STYLE_KEY: &str = "voicevox_style";

/// 選択できる読み上げエンジン。
pub const TTS_ENGINES: [&str; 2] = ["google", "voicevox"];

/// 読み上げエンジンの設定値。未設定・不明な値はGoogle Cloud。
fn tts_engine(db: &Db) -> Result<&'static str, AppError> {
    let saved = db.get_setting(TTS_ENGINE_KEY)?;
    Ok(TTS_ENGINES.into_iter().find(|e| Some(*e) == saved.as_deref()).unwrap_or("google"))
}

fn voicevox_style(db: &Db) -> Result<u32, AppError> {
    Ok(db.get_setting(VOICEVOX_STYLE_KEY)?.and_then(|v| v.parse().ok()).unwrap_or(voicevox::DEFAULT_STYLE))
}

/// 現在の設定で読み上げに使う声。
fn current_voice(db: &Db) -> Result<Voice, AppError> {
    Ok(if tts_engine(db)? == "voicevox" { Voice::Voicevox(voicevox_style(db)?) } else { Voice::Google })
}

#[tauri::command]
pub fn get_tts_engine(db: State<'_, Db>) -> Result<&'static str, AppError> {
    tts_engine(&db)
}

#[tauri::command]
pub fn set_tts_engine(db: State<'_, Db>, engine: String) -> Result<(), AppError> {
    if TTS_ENGINES.contains(&engine.as_str()) {
        db.set_setting(TTS_ENGINE_KEY, &engine)
    } else {
        Err(AppError::InvalidProvider(engine))
    }
}

#[tauri::command]
pub fn get_voicevox_style(db: State<'_, Db>) -> Result<u32, AppError> {
    voicevox_style(&db)
}

#[tauri::command]
pub fn set_voicevox_style(db: State<'_, Db>, style: u32) -> Result<(), AppError> {
    db.set_setting(VOICEVOX_STYLE_KEY, &style.to_string())
}

/// 読み上げに必要なものが揃っているか(Google Cloudはキー登録、VOICEVOXはCOREの取得)。
#[tauri::command]
pub fn tts_ready(app: AppHandle, db: State<'_, Db>) -> Result<bool, AppError> {
    if tts_engine(&db)? == "voicevox" {
        Ok(voicevox::installed(&app_data(&app)?))
    } else {
        secrets::has_tts_key()
    }
}

#[tauri::command]
pub fn voicevox_installed(app: AppHandle) -> Result<bool, AppError> {
    Ok(voicevox::installed(&app_data(&app)?))
}

/// VOICEVOX COREと辞書・音声モデルを取得する(取得済みでも再取得する)。利用規約への同意はフロントエンドで取る。
#[tauri::command]
pub async fn install_voicevox(app: AppHandle) -> Result<(), AppError> {
    voicevox::install(&app_data(&app)?).await?;
    // 差し替えたファイルを次の合成で読み直す
    app.state::<VoicevoxEngine>().reset();
    Ok(())
}

/// 取得済みの音声モデルの話者とスタイル。
#[tauri::command]
pub async fn voicevox_speakers(app: AppHandle) -> Result<Vec<voicevox::Speaker>, AppError> {
    let dir = app_data(&app)?;
    tauri::async_runtime::spawn_blocking(move || app.state::<VoicevoxEngine>().speakers(&dir))
        .await
        .map_err(|e| AppError::Voicevox(e.to_string()))?
}

/// TTSへ送る文字列。英単語やサービス名を読み辞書でカタカナへ置き換える(字幕は元の文字列のまま)。
fn pronounced(app: &AppHandle, text: &str) -> Result<String, AppError> {
    Ok(pronunciation::Dictionary::load(&app_data(app)?).apply(tts::spoken_text(text)))
}

/// 現在のエンジンで`text`を音声化する。Google CloudはMP3、VOICEVOXはWAV。
async fn synthesize_bytes(app: &AppHandle, voice: Voice, text: &str) -> Result<Vec<u8>, AppError> {
    match voice {
        Voice::Google => STANDARD.decode(tts::synthesize(&pronounced(app, text)?).await?).map_err(|_| AppError::BadResponse),
        Voice::Voicevox(style) => {
            if text.trim().is_empty() {
                return Err(AppError::EmptyText);
            }
            let dir = app_data(app)?;
            let app = app.clone();
            let text = pronounced(&app, text)?;
            tauri::async_runtime::spawn_blocking(move || app.state::<VoicevoxEngine>().synthesize(&dir, &text, style))
                .await
                .map_err(|e| AppError::Voicevox(e.to_string()))?
        }
    }
}

/// 定型音声ごとの作成状況(現在の声について)。
#[tauri::command]
pub fn voice_phrases_status(app: AppHandle, db: State<'_, Db>) -> Result<Vec<voice_phrases::PhraseStatus>, AppError> {
    Ok(voice_phrases::status(&app_data(&app)?, current_voice(&db)?))
}

/// 未作成の定型音声を現在のエンジンで合成して保存する。作成済みは送らない。途中で失敗しても、保存済みの分は残る。
#[tauri::command]
pub async fn generate_voice_phrases(app: AppHandle, db: State<'_, Db>) -> Result<Vec<voice_phrases::PhraseStatus>, AppError> {
    let dir = app_data(&app)?;
    let voice = current_voice(&db)?;
    for p in voice_phrases::missing(&dir, voice) {
        voice_phrases::save(&dir, voice, p, synthesize_bytes(&app, voice, p.text).await?)?;
    }
    Ok(voice_phrases::status(&dir, voice))
}

/// 作成済みの定型音声(base64のMP3またはWAV)。未作成なら`None`で、TTSへは送らない。
#[tauri::command]
pub fn voice_phrase_audio(app: AppHandle, db: State<'_, Db>, id: String) -> Result<Option<String>, AppError> {
    Ok(voice_phrases::audio(&app_data(&app)?, current_voice(&db)?, &id))
}

/// 応答テキストを現在のエンジンで音声化し、base64(Google CloudはMP3、VOICEVOXはWAV)を返す。
#[tauri::command]
pub async fn synthesize_speech(app: AppHandle, db: State<'_, Db>, text: String) -> Result<String, AppError> {
    let voice = current_voice(&db)?;
    if voice == Voice::Google {
        return tts::synthesize(&pronounced(&app, &text)?).await;
    }
    Ok(STANDARD.encode(synthesize_bytes(&app, voice, &text).await?))
}

#[tauri::command]
pub fn tts_key_registered() -> Result<bool, AppError> {
    secrets::has_tts_key()
}

#[tauri::command]
pub async fn save_tts_key(key: String) -> Result<(), AppError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::EmptyKey);
    }
    secrets::verify_tts_key(key).await?;
    secrets::set_tts_key(key)
}

#[tauri::command]
pub fn delete_tts_key() -> Result<(), AppError> {
    secrets::delete_tts_key()
}

#[tauri::command]
pub fn aqua_key_registered() -> Result<bool, AppError> {
    secrets::has_aqua_key()
}

#[tauri::command]
pub async fn save_aqua_key(key: String) -> Result<(), AppError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::EmptyKey);
    }
    stt_aqua::verify_key(key).await?;
    secrets::set_aqua_key(key)
}

#[tauri::command]
pub fn delete_aqua_key() -> Result<(), AppError> {
    secrets::delete_aqua_key()
}
