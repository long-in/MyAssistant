//! VOICEVOX COREによるローカル音声合成。
//!
//! COREのC API(`libvoicevox_core.dylib`)は同梱せず、設定画面から公式の配布物をアプリデータへ取得して
//! 実行時に動的ロードする(Whisperモデルと同じ運用)。音声はWAVのバイト列で返す。
use std::{
    ffi::{CStr, CString, c_char, c_void},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use libloading::Library;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::{db::Db, utils::{error::AppError, version}};

/// 取得バージョンの設定キー(JSON)。未設定の項目は`Versions::default()`の値。
pub const VERSIONS_KEY: &str = "voicevox_versions";
/// 取得する音声モデル。`0.vvm`には四国めたん・ずんだもん・春日部つむぎ・雨晴はうが入っている。
const MODEL_FILES: [&str; 1] = ["0.vvm"];
/// 既定の声(ずんだもん ノーマル)。
pub const DEFAULT_STYLE: u32 = 3;

/// 取得する配布物のバージョン。取得元のホストとURLの形はコードに固定し、ここでは識別子だけを差し替えられる。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Versions {
    /// `VOICEVOX/voicevox_core`のリリース。
    pub core: String,
    /// `VOICEVOX/onnxruntime-builder`のリリース(`voicevox_onnxruntime-<この値>`)。
    pub onnxruntime: String,
    /// `VOICEVOX/voicevox_vvm`のリリース。
    pub models: String,
    /// `r9y9/open_jtalk`のリリースタグ。
    pub dict_release: String,
    /// 辞書のバージョン(ファイル名`open_jtalk_dic_utf_8-<この値>`)。
    pub dict: String,
}

impl Default for Versions {
    fn default() -> Self {
        Self { core: "0.17.1".into(), onnxruntime: "1.23.2".into(), models: "0.17.0".into(), dict_release: "v1.11.1".into(), dict: "1.11".into() }
    }
}

impl Versions {
    /// URLやパスへ埋め込める識別子だけか。
    pub fn validate(&self) -> Result<(), AppError> {
        [&self.core, &self.onnxruntime, &self.models, &self.dict_release, &self.dict].into_iter().try_for_each(|v| version::validate(v))
    }

    /// 設定から読む。未設定・壊れた値・不正な識別子は既定値に戻す(壊れた設定で取得先を変えさせない)。
    pub fn load(db: &Db) -> Result<Self, AppError> {
        let saved = db.get_setting(VERSIONS_KEY)?.and_then(|v| serde_json::from_str::<Self>(&v).ok());
        Ok(saved.filter(|v| v.validate().is_ok()).unwrap_or_default())
    }

    pub fn save(&self, db: &Db) -> Result<(), AppError> {
        self.validate()?;
        db.set_setting(VERSIONS_KEY, &serde_json::to_string(self).map_err(|e| AppError::Voicevox(e.to_string()))?)
    }

    fn dict_name(&self) -> String {
        format!("open_jtalk_dic_utf_8-{}", self.dict)
    }
}

/// アーキテクチャごとの配布物名。対応外なら`None`。
fn archs() -> Option<(&'static str, &'static str)> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some(("osx-arm64", "osx-arm64"))
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some(("osx-x64", "osx-x86_64"))
    } else {
        None
    }
}

/// 取得物の置き場所。
struct Layout {
    root: PathBuf,
    onnxruntime_file: String,
    dict_name: String,
}

impl Layout {
    fn new(app_data: &Path, v: &Versions) -> Self {
        Self { root: app_data.join("voicevox"), onnxruntime_file: format!("libvoicevox_onnxruntime.{}.dylib", v.onnxruntime), dict_name: v.dict_name() }
    }

    fn core(&self) -> PathBuf {
        self.root.join("core").join("libvoicevox_core.dylib")
    }

    fn onnxruntime(&self) -> PathBuf {
        self.root.join("onnxruntime").join(&self.onnxruntime_file)
    }

    fn dict(&self) -> PathBuf {
        self.root.join("dict").join(&self.dict_name)
    }

    fn models(&self) -> PathBuf {
        self.root.join("models")
    }
}

/// 取得済みのバージョンの記録。取得の途中(`complete`が偽)で失敗した状態も区別する。
#[derive(Serialize, Deserialize)]
struct Record {
    complete: bool,
    versions: Versions,
}

fn record_path(app_data: &Path) -> PathBuf {
    app_data.join("voicevox").join("installed.json")
}

fn read_record(app_data: &Path) -> Option<Record> {
    serde_json::from_slice(&std::fs::read(record_path(app_data)).ok()?).ok()
}

fn write_record(app_data: &Path, record: &Record) -> Result<(), AppError> {
    let io = |e: std::io::Error| AppError::Io(e.to_string());
    std::fs::create_dir_all(app_data.join("voicevox")).map_err(io)?;
    std::fs::write(record_path(app_data), serde_json::to_vec(record).map_err(|e| AppError::Io(e.to_string()))?).map_err(io)
}

fn files_exist(l: &Layout) -> bool {
    l.core().is_file() && l.onnxruntime().is_file() && l.dict().is_dir() && MODEL_FILES.iter().all(|m| l.models().join(m).is_file())
}

/// 取得済みで使えるもののバージョン。記録がない取得物(記録を付ける前に取得したもの)は既定値のものとして扱う。
pub fn installed_versions(app_data: &Path) -> Option<Versions> {
    let versions = match read_record(app_data) {
        Some(r) if r.complete => r.versions,
        Some(_) => return None,
        None => Versions::default(),
    };
    files_exist(&Layout::new(app_data, &versions)).then_some(versions)
}

/// 必要なファイルがすべて揃っているか。
pub fn installed(app_data: &Path) -> bool {
    installed_versions(app_data).is_some()
}

/// `url`を`dest`へ保存する。一時ファイル経由で置き、`min_bytes`未満(途中切れ)は失敗にする。
async fn download(url: &str, dest: &Path, min_bytes: u64) -> Result<(), AppError> {
    let mut res = reqwest::get(url).await.map_err(AppError::from_reqwest)?;
    if !res.status().is_success() {
        return Err(AppError::from_status(res.status().as_u16()));
    }
    let tmp = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&tmp).await.map_err(|e| AppError::Io(e.to_string()))?;
    while let Some(chunk) = res.chunk().await.map_err(AppError::from_reqwest)? {
        file.write_all(&chunk).await.map_err(|e| AppError::Io(e.to_string()))?;
    }
    file.flush().await.map_err(|e| AppError::Io(e.to_string()))?;
    drop(file);
    let size = tokio::fs::metadata(&tmp).await.map(|m| m.len()).unwrap_or(0);
    if size < min_bytes {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(AppError::BadResponse);
    }
    tokio::fs::rename(&tmp, dest).await.map_err(|e| AppError::Io(e.to_string()))
}

/// システムの`tar`/`unzip`でアーカイブを`into`へ展開する(`..`を含むパスは各コマンドが拒否する)。
async fn extract(program: &str, args: &[&str], archive: &Path, into: &Path) -> Result<(), AppError> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args).arg(archive);
    if program == "unzip" {
        cmd.arg("-d").arg(into);
    } else {
        cmd.arg("-C").arg(into);
    }
    let out = cmd.output().await.map_err(|e| AppError::Io(format!("{program}: {e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(AppError::Io(format!("{program} failed: {}", String::from_utf8_lossy(&out.stderr).trim())))
    }
}

/// 展開した`from`を`to`へ置く。親ディレクトリは作り、既存の`to`は置き換える。
fn place(from: &Path, to: &Path) -> Result<(), AppError> {
    let io = |e: std::io::Error| AppError::Io(e.to_string());
    std::fs::create_dir_all(to.parent().ok_or_else(|| AppError::Io("no parent".into()))?).map_err(io)?;
    if to.exists() {
        if to.is_dir() { std::fs::remove_dir_all(to) } else { std::fs::remove_file(to) }.map_err(io)?;
    }
    std::fs::rename(from, to).map_err(io)
}

/// `versions`のCOREと、ONNX Runtime・辞書・音声モデルを取得して配置する。取得済みでも再取得する。
pub async fn install(app_data: &Path, versions: &Versions) -> Result<(), AppError> {
    install_inner(app_data, versions).await.map_err(|e| e.with_source("voicevox"))
}

async fn install_inner(app_data: &Path, v: &Versions) -> Result<(), AppError> {
    v.validate()?;
    let (core_arch, onnx_arch) = archs().ok_or_else(|| AppError::Voicevox("unsupported platform".into()))?;
    let l = Layout::new(app_data, v);
    let work = l.root.join("tmp");
    let _ = tokio::fs::remove_dir_all(&work).await;
    tokio::fs::create_dir_all(&work).await.map_err(|e| AppError::Io(e.to_string()))?;
    // 置き換えの途中で失敗したときに、新旧が混ざったものを取得済みとして扱わない
    write_record(app_data, &Record { complete: false, versions: v.clone() })?;
    let result = fetch_all(&l, &work, core_arch, onnx_arch, v).await;
    let _ = tokio::fs::remove_dir_all(&work).await;
    result?;
    write_record(app_data, &Record { complete: true, versions: v.clone() })
}

async fn fetch_all(l: &Layout, work: &Path, core_arch: &str, onnx_arch: &str, v: &Versions) -> Result<(), AppError> {
    let gh = "https://github.com";

    let zip = work.join("core.zip");
    download(&format!("{gh}/VOICEVOX/voicevox_core/releases/download/{core}/voicevox_core-{core_arch}-{core}.zip", core = v.core), &zip, 1 << 20).await?;
    let out = work.join("core");
    extract("unzip", &["-q", "-o"], &zip, &out).await?;
    place(&out.join(format!("voicevox_core-{core_arch}-{}/lib/libvoicevox_core.dylib", v.core)), &l.core())?;

    let tgz = work.join("onnxruntime.tgz");
    download(&format!("{gh}/VOICEVOX/onnxruntime-builder/releases/download/voicevox_onnxruntime-{ort}/voicevox_onnxruntime-{onnx_arch}-{ort}.tgz", ort = v.onnxruntime), &tgz, 1 << 20).await?;
    let out = work.join("onnxruntime");
    tokio::fs::create_dir_all(&out).await.map_err(|e| AppError::Io(e.to_string()))?;
    extract("tar", &["-xzf"], &tgz, &out).await?;
    place(
        &out.join(format!("voicevox_onnxruntime-{onnx_arch}-{ort}/lib/libvoicevox_onnxruntime.{ort}.dylib", ort = v.onnxruntime)),
        &l.onnxruntime(),
    )?;

    let tgz = work.join("dict.tgz");
    download(&format!("{gh}/r9y9/open_jtalk/releases/download/{}/{}.tar.gz", v.dict_release, v.dict_name()), &tgz, 1 << 20).await?;
    let out = work.join("dict");
    tokio::fs::create_dir_all(&out).await.map_err(|e| AppError::Io(e.to_string()))?;
    extract("tar", &["-xzf"], &tgz, &out).await?;
    place(&out.join(v.dict_name()), &l.dict())?;

    // 音声モデルは最後に置く(`installed`の判定に使うため、これが揃えば完了)
    for model in MODEL_FILES {
        let tmp = work.join(model);
        download(&format!("{gh}/VOICEVOX/voicevox_vvm/releases/download/{}/{model}", v.models), &tmp, 1 << 20).await?;
        place(&tmp, &l.models().join(model))?;
    }
    Ok(())
}

type ResultCode = i32;

#[repr(C)]
struct LoadOnnxruntimeOptions {
    filename: *const c_char,
}

/// `voicevox_make_default_*`が返す値をそのまま渡すための型(レイアウトはC APIのヘッダに合わせる)。
#[repr(C)]
#[derive(Clone, Copy)]
struct InitializeOptions {
    acceleration_mode: i32,
    cpu_num_threads: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LoadVoiceModelOptions {
    on_existing: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TtsOptions {
    enable_interrogative_upspeak: bool,
}

/// ロード済みのCORE。`Library`を保持している間だけ関数ポインタが有効。
struct Loaded {
    _lib: Library,
    synthesizer: *mut c_void,
    open_jtalk: *mut c_void,
    synthesizer_delete: unsafe extern "C" fn(*mut c_void),
    open_jtalk_delete: unsafe extern "C" fn(*mut c_void),
    tts: unsafe extern "C" fn(*const c_void, *const c_char, u32, TtsOptions, *mut usize, *mut *mut u8) -> ResultCode,
    wav_free: unsafe extern "C" fn(*mut u8),
    metas_json: unsafe extern "C" fn(*const c_void) -> *mut c_char,
    json_free: unsafe extern "C" fn(*mut c_char),
    error_message: unsafe extern "C" fn(ResultCode) -> *const c_char,
    tts_options: TtsOptions,
}

// COREのシンセサイザは複数スレッドから使える(公式のドキュメントでスレッドセーフ)
unsafe impl Send for Loaded {}
unsafe impl Sync for Loaded {}

impl Drop for Loaded {
    fn drop(&mut self) {
        unsafe {
            (self.synthesizer_delete)(self.synthesizer);
            (self.open_jtalk_delete)(self.open_jtalk);
        }
    }
}

fn voicevox_err(msg: impl Into<String>) -> AppError {
    AppError::Voicevox(msg.into())
}

fn c_path(path: &Path) -> Result<CString, AppError> {
    CString::new(path.to_string_lossy().as_bytes()).map_err(|_| voicevox_err("path contains NUL"))
}

impl Loaded {
    fn check(&self, code: ResultCode) -> Result<(), AppError> {
        check(self.error_message, code)
    }

    /// COREを読み込み、ONNX Runtime・辞書・音声モデルでシンセサイザを作る。数秒かかる。
    fn load(app_data: &Path) -> Result<Self, AppError> {
        let versions = installed_versions(app_data).ok_or(AppError::VoicevoxMissing)?;
        let l = Layout::new(app_data, &versions);
        unsafe {
            let lib = Library::new(l.core()).map_err(|e| voicevox_err(format!("load core: {e}")))?;
            macro_rules! sym {
                ($name:literal, $ty:ty) => {
                    *lib.get::<$ty>(concat!($name, "\0").as_bytes()).map_err(|e| voicevox_err(format!("{}: {e}", $name)))?
                };
            }
            let error_message = sym!("voicevox_error_result_to_message", unsafe extern "C" fn(ResultCode) -> *const c_char);
            let ok = |code: ResultCode| check(error_message, code);

            let onnx_load = sym!("voicevox_onnxruntime_load_once", unsafe extern "C" fn(LoadOnnxruntimeOptions, *mut *const c_void) -> ResultCode);
            let filename = c_path(&l.onnxruntime())?;
            let mut onnxruntime = std::ptr::null();
            ok(onnx_load(LoadOnnxruntimeOptions { filename: filename.as_ptr() }, &mut onnxruntime))?;

            let jtalk_new = sym!("voicevox_open_jtalk_rc_new", unsafe extern "C" fn(*const c_char, *mut *mut c_void) -> ResultCode);
            let open_jtalk_delete = sym!("voicevox_open_jtalk_rc_delete", unsafe extern "C" fn(*mut c_void));
            let dict = c_path(&l.dict())?;
            let mut open_jtalk = std::ptr::null_mut();
            ok(jtalk_new(dict.as_ptr(), &mut open_jtalk))?;

            let default_init = sym!("voicevox_make_default_initialize_options", unsafe extern "C" fn() -> InitializeOptions);
            let synth_new = sym!(
                "voicevox_synthesizer_new",
                unsafe extern "C" fn(*const c_void, *const c_void, InitializeOptions, *mut *mut c_void) -> ResultCode
            );
            let synthesizer_delete = sym!("voicevox_synthesizer_delete", unsafe extern "C" fn(*mut c_void));
            let mut synthesizer = std::ptr::null_mut();
            let created = ok(synth_new(onnxruntime, open_jtalk, default_init(), &mut synthesizer));
            if let Err(e) = created {
                open_jtalk_delete(open_jtalk);
                return Err(e);
            }
            // 以降の失敗でも、作ったものを`Drop`で解放する
            let tts = sym!(
                "voicevox_synthesizer_tts",
                unsafe extern "C" fn(*const c_void, *const c_char, u32, TtsOptions, *mut usize, *mut *mut u8) -> ResultCode
            );
            let wav_free = sym!("voicevox_bytes_free", unsafe extern "C" fn(*mut u8));
            let metas_json = sym!("voicevox_synthesizer_create_metas_json", unsafe extern "C" fn(*const c_void) -> *mut c_char);
            let json_free = sym!("voicevox_string_free", unsafe extern "C" fn(*mut c_char));
            let default_tts = sym!("voicevox_make_default_tts_options", unsafe extern "C" fn() -> TtsOptions);
            let model_open = sym!("voicevox_voice_model_file_open", unsafe extern "C" fn(*const c_char, *mut *mut c_void) -> ResultCode);
            let model_delete = sym!("voicevox_voice_model_file_delete", unsafe extern "C" fn(*mut c_void));
            let load_model = sym!(
                "voicevox_synthesizer_load_voice_model",
                unsafe extern "C" fn(*const c_void, *const c_void, LoadVoiceModelOptions) -> ResultCode
            );
            let default_load = sym!("voicevox_make_default_load_voice_model_options", unsafe extern "C" fn() -> LoadVoiceModelOptions);
            let this = Self {
                tts,
                wav_free,
                metas_json,
                json_free,
                tts_options: default_tts(),
                error_message,
                synthesizer_delete,
                open_jtalk_delete,
                synthesizer,
                open_jtalk,
                _lib: lib,
            };

            for file in MODEL_FILES {
                let path = c_path(&l.models().join(file))?;
                let mut model = std::ptr::null_mut();
                this.check(model_open(path.as_ptr(), &mut model))?;
                let loaded = this.check(load_model(this.synthesizer, model, default_load()));
                model_delete(model);
                loaded?;
            }
            Ok(this)
        }
    }

    fn synthesize(&self, text: &str, style: u32) -> Result<Vec<u8>, AppError> {
        let text = CString::new(text).map_err(|_| voicevox_err("text contains NUL"))?;
        let mut len = 0usize;
        let mut wav = std::ptr::null_mut();
        unsafe {
            self.check((self.tts)(self.synthesizer, text.as_ptr(), style, self.tts_options, &mut len, &mut wav))?;
            let bytes = std::slice::from_raw_parts(wav, len).to_vec();
            (self.wav_free)(wav);
            Ok(bytes)
        }
    }

    fn metas(&self) -> Result<String, AppError> {
        unsafe {
            let ptr = (self.metas_json)(self.synthesizer);
            if ptr.is_null() {
                return Err(AppError::BadResponse);
            }
            let json = CStr::from_ptr(ptr).to_string_lossy().into_owned();
            (self.json_free)(ptr);
            Ok(json)
        }
    }
}

fn check(error_message: unsafe extern "C" fn(ResultCode) -> *const c_char, code: ResultCode) -> Result<(), AppError> {
    if code == 0 {
        return Ok(());
    }
    let msg = unsafe {
        let ptr = error_message(code);
        if ptr.is_null() { String::new() } else { CStr::from_ptr(ptr).to_string_lossy().into_owned() }
    };
    Err(voicevox_err(format!("code {code}: {msg}")))
}

/// ロード済みCOREの保持。初回の利用時にロードし、以降は使い回す。
#[derive(Default)]
pub struct VoicevoxEngine {
    loaded: Mutex<Option<Arc<Loaded>>>,
}

impl VoicevoxEngine {
    fn get(&self, app_data: &Path) -> Result<Arc<Loaded>, AppError> {
        let mut guard = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(l) = guard.as_ref() {
            return Ok(l.clone());
        }
        if !installed(app_data) {
            return Err(AppError::VoicevoxMissing);
        }
        let l = Arc::new(Loaded::load(app_data)?);
        *guard = Some(l.clone());
        Ok(l)
    }

    /// ロード済みのCOREを破棄する(ファイルを取得し直したとき用)。
    pub fn reset(&self) {
        *self.loaded.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// WAVを合成する。ブロッキングなので`spawn_blocking`から呼ぶ。
    pub fn synthesize(&self, app_data: &Path, text: &str, style: u32) -> Result<Vec<u8>, AppError> {
        self.get(app_data)?.synthesize(text, style).map_err(|e| e.with_source("voicevox"))
    }

    /// 取得した音声モデルの話者とスタイル。
    pub fn speakers(&self, app_data: &Path) -> Result<Vec<Speaker>, AppError> {
        let json = self.get(app_data)?.metas().map_err(|e| e.with_source("voicevox"))?;
        parse_speakers(&json)
    }
}

/// 話者(キャラクター)と、その声のスタイル。
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Speaker {
    pub name: String,
    pub styles: Vec<Style>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Style {
    pub id: u32,
    pub name: String,
}

/// `create_metas_json`の結果から、話者とスタイルだけを取り出す(歌唱用などのスタイルは除く)。
fn parse_speakers(json: &str) -> Result<Vec<Speaker>, AppError> {
    #[derive(Deserialize)]
    struct Meta {
        name: String,
        styles: Vec<MetaStyle>,
    }
    #[derive(Deserialize)]
    struct MetaStyle {
        id: u32,
        name: String,
        #[serde(default, rename = "type")]
        kind: Option<String>,
    }
    let metas: Vec<Meta> = serde_json::from_str(json).map_err(|_| AppError::BadResponse)?;
    Ok(metas
        .into_iter()
        .map(|m| Speaker {
            name: m.name,
            styles: m.styles.into_iter().filter(|s| s.kind.as_deref().is_none_or(|k| matches!(k, "talk" | "streaming_talk"))).map(|s| Style { id: s.id, name: s.name }).collect(),
        })
        .filter(|s| !s.styles.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_installed_in_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!installed(dir.path()));
    }

    #[test]
    fn installed_needs_every_file() {
        let dir = tempfile::tempdir().unwrap();
        let l = Layout::new(dir.path(), &Versions::default());
        for p in [l.core(), l.onnxruntime(), l.models().join(MODEL_FILES[0])] {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }
        assert!(!installed(dir.path()));
        std::fs::create_dir_all(l.dict()).unwrap();
        assert!(installed(dir.path()));
    }

    /// 全ファイルを`versions`のレイアウトで置く。
    fn put_files(dir: &Path, versions: &Versions) {
        let l = Layout::new(dir, versions);
        for p in [l.core(), l.onnxruntime(), l.models().join(MODEL_FILES[0])] {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }
        std::fs::create_dir_all(l.dict()).unwrap();
    }

    #[test]
    fn installed_versions_follow_the_record() {
        let dir = tempfile::tempdir().unwrap();
        let custom = Versions { onnxruntime: "9.9.9".into(), dict: "2.0".into(), ..Versions::default() };
        put_files(dir.path(), &custom);
        // 記録がなければ既定値のレイアウトを探すので、別バージョンのファイルは見つからない
        assert_eq!(installed_versions(dir.path()), None);
        write_record(dir.path(), &Record { complete: false, versions: custom.clone() }).unwrap();
        assert_eq!(installed_versions(dir.path()), None);
        write_record(dir.path(), &Record { complete: true, versions: custom.clone() }).unwrap();
        assert_eq!(installed_versions(dir.path()), Some(custom));
    }

    #[test]
    fn legacy_install_without_record_counts_as_default_versions() {
        let dir = tempfile::tempdir().unwrap();
        put_files(dir.path(), &Versions::default());
        assert_eq!(installed_versions(dir.path()), Some(Versions::default()));
    }

    #[test]
    fn versions_reject_unsafe_values() {
        assert!(Versions::default().validate().is_ok());
        assert!(Versions { core: "../evil".into(), ..Versions::default() }.validate().is_err());
        assert!(Versions { dict: String::new(), ..Versions::default() }.validate().is_err());
    }

    #[test]
    fn partial_json_fills_defaults() {
        let v: Versions = serde_json::from_str(r#"{"core":"0.18.0"}"#).unwrap();
        assert_eq!(v, Versions { core: "0.18.0".into(), ..Versions::default() });
    }

    #[test]
    fn parses_talk_styles_only() {
        let json = r#"[{"name":"ずんだもん","styles":[{"id":3,"name":"ノーマル","type":"talk"},{"id":4,"name":"ストリーミング","type":"streaming_talk"},{"id":6000,"name":"ソング","type":"sing"}]},
                       {"name":"歌専用","styles":[{"id":9,"name":"x","type":"frame_decode"}]}]"#;
        let speakers = parse_speakers(json).unwrap();
        assert_eq!(speakers, vec![Speaker { name: "ずんだもん".into(), styles: vec![Style { id: 3, name: "ノーマル".into() }, Style { id: 4, name: "ストリーミング".into() }] }]);
        assert!(matches!(parse_speakers("{}"), Err(AppError::BadResponse)));
    }

    #[test]
    fn place_replaces_existing() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a");
        let to = dir.path().join("sub/b");
        std::fs::write(&from, b"new").unwrap();
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(&to, b"old").unwrap();
        place(&from, &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"new");
    }

    /// 実際に取得して合成する(約90MBをダウンロードする)。`VOICEVOX_TEST_DIR`に空のディレクトリを指す。
    #[tokio::test]
    #[ignore]
    async fn installs_and_synthesizes_real_wav() {
        let dir = std::path::PathBuf::from(std::env::var("VOICEVOX_TEST_DIR").unwrap());
        install(&dir, &Versions::default()).await.unwrap();
        assert!(installed(&dir));
        let engine = VoicevoxEngine::default();
        let wav = tokio::task::spawn_blocking({
            let dir = dir.clone();
            move || {
                let wav = engine.synthesize(&dir, "こんにちは、VOICEVOXです。", DEFAULT_STYLE).unwrap();
                assert!(engine.speakers(&dir).unwrap().iter().any(|s| s.styles.iter().any(|st| st.id == DEFAULT_STYLE)));
                wav
            }
        })
        .await
        .unwrap();
        assert_eq!(&wav[..4], b"RIFF");
        std::fs::write(dir.join("out.wav"), &wav).unwrap();
    }
}
