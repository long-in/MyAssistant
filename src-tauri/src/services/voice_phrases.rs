//! 繰り返し使う定型の音声。設定ページのボタンで一度だけ合成してアプリデータへ保存し、
//! 実行時はTTSへ送らず保存済みの音声を再生する。声(エンジン)ごとに別のファイルとして保存する。
use std::path::{Path, PathBuf};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Serialize;

use super::tts::Voice;
use crate::utils::error::AppError;

/// 定型音声の定義。追加するときはここへ足す。
pub struct Phrase {
    pub id: &'static str,
    pub text: &'static str,
}

pub const PHRASES: [Phrase; 2] = [
    // 起動ワードだけが言われたときの応答
    Phrase { id: "wake_ack", text: "用件は、なんでしょうか" },
    // 会話を切り替えてよいかの確認
    Phrase { id: "switch_confirm", text: "新しい会話を始めてもよいですか" },
];

/// 画面に出す作成状況。
#[derive(Debug, Serialize, PartialEq)]
pub struct PhraseStatus {
    pub id: &'static str,
    pub text: &'static str,
    pub ready: bool,
}

/// 文字列のFNV-1aハッシュ。ファイル名用で、実行環境によらず値が変わらない。
fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// 保存先。文言か声が変わればファイル名も変わり、古い音声は未作成として扱われる。
fn path_of(app_data: &Path, p: &Phrase, voice: Voice) -> PathBuf {
    app_data.join("voice").join(format!("{}-{:016x}.{}", p.id, fnv1a(&format!("{}|{}", voice.id(p.text), p.text)), voice.ext()))
}

fn is_ready(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() > 0)
}

pub fn status(app_data: &Path, voice: Voice) -> Vec<PhraseStatus> {
    PHRASES
        .iter()
        .map(|p| PhraseStatus { id: p.id, text: p.text, ready: is_ready(&path_of(app_data, p, voice)) })
        .collect()
}

/// まだ作成されていない定型音声。
pub fn missing(app_data: &Path, voice: Voice) -> Vec<&'static Phrase> {
    PHRASES.iter().filter(|p| !is_ready(&path_of(app_data, p, voice))).collect()
}

/// 合成した音声`bytes`を保存する。
pub fn save(app_data: &Path, voice: Voice, p: &Phrase, bytes: Vec<u8>) -> Result<(), AppError> {
    if bytes.is_empty() {
        return Err(AppError::BadResponse);
    }
    let path = path_of(app_data, p, voice);
    let dir = path.parent().ok_or_else(|| AppError::Io("no parent".into()))?;
    std::fs::create_dir_all(dir).map_err(|e| AppError::Io(e.to_string()))?;
    // 書き込み途中のファイルを作成済みと見なさないよう、一時ファイル経由で置く
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| AppError::Io(e.to_string()))
}

/// 保存済みの音声(base64のMP3またはWAV)。未作成・未知のidなら`None`。
pub fn audio(app_data: &Path, voice: Voice, id: &str) -> Option<String> {
    let p = PHRASES.iter().find(|p| p.id == id)?;
    let bytes = std::fs::read(path_of(app_data, p, voice)).ok().filter(|b| !b.is_empty())?;
    Some(STANDARD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: Voice = Voice::Google;

    #[test]
    fn file_name_is_stable_and_per_phrase() {
        let a = path_of(Path::new("/data"), &PHRASES[0], G);
        assert_eq!(a, path_of(Path::new("/data"), &PHRASES[0], G));
        assert_ne!(a, path_of(Path::new("/data"), &PHRASES[1], G));
        assert!(a.starts_with("/data/voice") && a.extension().is_some_and(|e| e == "mp3"));
    }

    #[test]
    fn file_name_differs_per_voice() {
        let g = path_of(Path::new("/data"), &PHRASES[0], G);
        let v = path_of(Path::new("/data"), &PHRASES[0], Voice::Voicevox(3));
        assert_ne!(g, v);
        assert!(v.extension().is_some_and(|e| e == "wav"));
        assert_ne!(v, path_of(Path::new("/data"), &PHRASES[0], Voice::Voicevox(2)));
    }

    #[test]
    fn status_and_audio_follow_saved_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(status(dir.path(), G).iter().all(|s| !s.ready));
        assert_eq!(missing(dir.path(), G).len(), PHRASES.len());
        assert!(audio(dir.path(), G, "wake_ack").is_none());
        save(dir.path(), G, &PHRASES[0], b"ABC".to_vec()).unwrap();
        let s = status(dir.path(), G);
        assert!(s[0].ready && !s[1].ready);
        assert_eq!(missing(dir.path(), G).len(), PHRASES.len() - 1);
        assert_eq!(audio(dir.path(), G, "wake_ack").as_deref(), Some("QUJD"));
        assert!(audio(dir.path(), G, "unknown").is_none());
        // 別の声では未作成として扱う
        assert!(audio(dir.path(), Voice::Voicevox(3), "wake_ack").is_none());
        // 空の音声は保存しない
        assert!(matches!(save(dir.path(), G, &PHRASES[1], Vec::new()), Err(AppError::BadResponse)));
        assert!(!status(dir.path(), G)[1].ready);
    }
}
