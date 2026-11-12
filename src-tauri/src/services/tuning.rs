//! 設定ページから変更できる数値の調整項目。範囲外の値は保存時に範囲内へ丸める。
//! 検索回数などの上限(`Tuning`)はエージェントごと、音声入力の待ち時間(`VoiceTiming`)は全体設定。
use serde::{Deserialize, Serialize};

use super::global_settings::SettingsStore;
use crate::{db::Db, utils::error::AppError};

pub const KEY: &str = "tuning";
/// 音声入力の待ち時間(全体設定)のキー。
pub const VOICE_KEY: &str = "voice_timing";

/// エージェントごとの上限。未保存・欠けた項目は既定値になる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tuning {
    /// 1回の依頼で行うインターネット検索の上限回数。
    pub web_search_max: u32,
    /// 1回の依頼で行うページ取得の上限回数。
    pub web_fetch_max: u32,
    /// 過去の会話をそのままLLMへ渡す往復数の上限。超えた分は要約して渡す。
    pub history_exchanges: u32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self { web_search_max: 5, web_fetch_max: 5, history_exchanges: 10 }
    }
}

impl Tuning {
    /// 各項目を許容範囲へ収める。
    fn clamped(self) -> Self {
        Self {
            web_search_max: self.web_search_max.clamp(1, 20),
            web_fetch_max: self.web_fetch_max.clamp(1, 20),
            history_exchanges: self.history_exchanges.clamp(2, 30),
        }
    }
}

/// 保存済みの調整項目。読めなければ既定値。
pub fn load(db: &Db) -> Result<Tuning, AppError> {
    Ok(db
        .get_setting(KEY)?
        .and_then(|v| serde_json::from_str::<Tuning>(&v).ok())
        .unwrap_or_default()
        .clamped())
}

/// 範囲内へ丸めて保存し、実際に保存した値を返す。
pub fn save(db: &Db, tuning: Tuning) -> Result<Tuning, AppError> {
    let tuning = tuning.clamped();
    db.set_setting(KEY, &serde_json::to_string(&tuning).map_err(|e| AppError::Db(e.to_string()))?)?;
    Ok(tuning)
}

/// 音声入力の待ち時間。全エージェント共通(全体設定)。未保存・欠けた項目は既定値になる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceTiming {
    /// 声が入らなければ音声入力を終える時間(秒)。
    pub voice_idle_secs: f32,
    /// 話し終わりとみなす無音の長さ(秒)。
    pub utterance_silence_secs: f32,
    /// エージェントを離してから録音を止めるまでの猶予(秒)。
    pub release_tail_secs: f32,
}

impl Default for VoiceTiming {
    fn default() -> Self {
        Self { voice_idle_secs: 3.0, utterance_silence_secs: 1.2, release_tail_secs: 0.5 }
    }
}

impl VoiceTiming {
    /// 各項目を許容範囲へ収める。非数は下限になる。
    fn clamped(self) -> Self {
        let secs = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
        Self {
            voice_idle_secs: secs(self.voice_idle_secs, 1.0, 30.0),
            utterance_silence_secs: secs(self.utterance_silence_secs, 0.5, 5.0),
            release_tail_secs: secs(self.release_tail_secs, 0.0, 3.0),
        }
    }
}

/// 保存済みの音声入力の待ち時間。読めなければ既定値。
pub fn load_voice(store: &impl SettingsStore) -> Result<VoiceTiming, AppError> {
    Ok(store
        .get_setting(VOICE_KEY)?
        .and_then(|v| serde_json::from_str::<VoiceTiming>(&v).ok())
        .unwrap_or_default()
        .clamped())
}

/// 範囲内へ丸めて保存し、実際に保存した値を返す。
pub fn save_voice(store: &impl SettingsStore, voice: VoiceTiming) -> Result<VoiceTiming, AppError> {
    let voice = voice.clamped();
    store.set_setting(VOICE_KEY, &serde_json::to_string(&voice).map_err(|e| AppError::Db(e.to_string()))?)?;
    Ok(voice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_range_values_are_clamped() {
        let t = Tuning { web_search_max: 0, web_fetch_max: 999, history_exchanges: 0 }.clamped();
        assert_eq!((t.web_search_max, t.web_fetch_max, t.history_exchanges), (1, 20, 2));
        let v = VoiceTiming { voice_idle_secs: f32::NAN, release_tail_secs: 99.0, ..VoiceTiming::default() }.clamped();
        assert_eq!((v.voice_idle_secs, v.release_tail_secs), (1.0, 3.0));
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let t: Tuning = serde_json::from_str(r#"{"web_search_max":8}"#).unwrap();
        assert_eq!(t.web_search_max, 8);
        assert_eq!(t.history_exchanges, Tuning::default().history_exchanges);
        // 旧形式(音声の項目を含む)も読める
        let old: Tuning = serde_json::from_str(r#"{"web_search_max":8,"voice_idle_secs":9.0}"#).unwrap();
        assert_eq!(old.web_search_max, 8);
        let v: VoiceTiming = serde_json::from_str(r#"{"voice_idle_secs":9.0}"#).unwrap();
        assert_eq!(v.utterance_silence_secs, VoiceTiming::default().utterance_silence_secs);
    }
}
