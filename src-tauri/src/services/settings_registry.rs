//! 設定キーの一覧。各キーの保存先(スペースのDBか、全体設定か)と、書き出し・コピーでの扱いを1か所で決める。
//!
//! - キーの名前そのものは、従来どおり各モジュールの定数に置く(この一覧はそれを参照する)。
//! - 保存先の取り違えを防ぐため、DBは全体設定のキーを、全体設定はスペースのキーを書き込みで拒否する(`check_db_write` / `check_global_write`)。
//! - 一覧に無いキーはどちらにも書ける(テスト用の仮のキーなど)。新しい設定を足すときは、ここにも登録する。
use crate::{
    commands::{chat, voice},
    utils::error::AppError,
};

use super::{batch, debug_log, global_settings, leader, profile, proofread, space_data, stt, tuning, voicevox, wake, workspace};

/// 設定の保存先。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// ワークスペース(パーソナル・スペース・リーダー)ごとのDB(`app_settings`)。
    Space,
    /// 全スペース共通の全体設定(`workspaces/global_settings.json`)。
    Global,
}

/// 書き出し・取り込み時の値の検証方法。
#[derive(Debug, Clone, Copy)]
pub enum Kind {
    /// `"0"`または`"1"`。
    Flag,
    /// JSONとして読める文字列(空文字は未設定を表す)。
    Json,
    /// 指定した値のどれか。
    OneOf(&'static [&'static str]),
    /// 自由な文字列。
    Text,
    /// 検証を通る基本情報のJSON(空文字は未設定を表す)。
    Profile,
    /// 整理バッチの間隔(分)。
    Interval,
}

/// 設定キー1つ分の定義。
#[derive(Debug, Clone, Copy)]
pub struct Setting {
    pub key: &'static str,
    pub scope: Scope,
    /// 書き出し・取り込み(とスペース間のコピー)の対象なら、その検証方法。`None`は対象外。
    /// 全体設定は全体設定のバックアップだけが扱い、スペースの書き出しには入らない。
    pub transfer: Option<Kind>,
}

const fn space(key: &'static str, transfer: Option<Kind>) -> Setting {
    Setting { key, scope: Scope::Space, transfer }
}

const fn global(key: &'static str, transfer: Option<Kind>) -> Setting {
    Setting { key, scope: Scope::Global, transfer }
}

/// すべての設定キー。書き出しの項目の並びは、この順に従う。
pub const ALL: &[Setting] = &[
    // ---- スペースごと(書き出し・コピーの対象) ----
    space(chat::ROUTE_KEY, Some(Kind::Json)),
    space(chat::WEB_ACCESS_KEY, Some(Kind::Flag)),
    space(chat::SHELL_ACCESS_KEY, Some(Kind::Flag)),
    space(voice::STT_ENGINE_KEY, Some(Kind::OneOf(&["local", "google", "aqua"]))),
    space(voice::STT_LANGUAGE_KEY, Some(Kind::Text)),
    space(voice::TTS_ENGINE_KEY, Some(Kind::OneOf(&["google", "voicevox"]))),
    space(voice::VOICEVOX_STYLE_KEY, Some(Kind::Text)),
    space(tuning::KEY, Some(Kind::Json)),
    space(batch::BATCH_ROUTE_KEY, Some(Kind::Json)),
    space(proofread::ROUTE_KEY, Some(Kind::Json)),
    space(debug_log::ENABLED_KEY, Some(Kind::Flag)),
    // ---- スペースごと(対象外: 端末ごとに異なるもの、スペース固有のもの) ----
    space(batch::KNOWLEDGE_DIR_KEY, None),
    space(debug_log::DIR_KEY, None),
    space(stt::REVISION_KEY, None),
    space(voicevox::VERSIONS_KEY, None),
    space(space_data::KEY, None),
    space(leader::PROMPT_KEY, None),
    space(leader::LIMIT_KEY, None),
    // ---- 全体設定 ----
    global(chat::BASE_INSTRUCTIONS_KEY, Some(Kind::Text)),
    global(global_settings::OS_NOTIFICATIONS_KEY, Some(Kind::Flag)),
    global(global_settings::LANGUAGE_KEY, Some(Kind::OneOf(&["ja", "en"]))),
    global(tuning::VOICE_KEY, Some(Kind::Json)),
    global(wake::ENABLED_KEY, Some(Kind::Flag)),
    global(wake::WORDS_KEY, Some(Kind::Json)),
    global(profile::KEY, Some(Kind::Profile)),
    global(batch::BATCH_INTERVAL_KEY, Some(Kind::Interval)),
    global(workspace::WORK_ENABLED_KEY, None),
];

/// キーの定義。一覧に無ければ`None`。
pub fn find(key: &str) -> Option<&'static Setting> {
    ALL.iter().find(|s| s.key == key)
}

/// 保存先が`scope`のキー。
pub fn keys(scope: Scope) -> impl Iterator<Item = &'static str> {
    ALL.iter().filter(move |s| s.scope == scope).map(|s| s.key)
}

/// 保存先が`scope`で、書き出しの対象のキーと検証方法。
pub fn transferable(scope: Scope) -> impl Iterator<Item = (&'static str, Kind)> {
    ALL.iter().filter(move |s| s.scope == scope).filter_map(|s| s.transfer.map(|k| (s.key, k)))
}

fn check(key: &str, forbidden: Scope) -> Result<(), AppError> {
    match find(key) {
        Some(s) if s.scope == forbidden => Err(AppError::Db(format!("setting `{key}` belongs to another store"))),
        _ => Ok(()),
    }
}

/// DBへの書き込みを確かめる(全体設定のキーは書かせない)。
pub fn check_db_write(key: &str) -> Result<(), AppError> {
    check(key, Scope::Global)
}

/// 全体設定への書き込みを確かめる(スペースごとのキーは書かせない)。
pub fn check_global_write(key: &str) -> Result<(), AppError> {
    check(key, Scope::Space)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for s in ALL {
            assert!(seen.insert(s.key), "duplicated key: {}", s.key);
        }
    }

    #[test]
    fn writes_are_routed_to_their_own_store() {
        assert!(check_db_write(chat::ROUTE_KEY).is_ok());
        assert!(check_db_write(chat::BASE_INSTRUCTIONS_KEY).is_err());
        assert!(check_global_write(chat::BASE_INSTRUCTIONS_KEY).is_ok());
        assert!(check_global_write(chat::ROUTE_KEY).is_err());
        // 一覧に無いキーはどちらにも書ける
        assert!(check_db_write("k").is_ok() && check_global_write("k").is_ok());
    }

    #[test]
    fn global_backup_covers_all_global_keys_except_work_toggle() {
        let exported: Vec<_> = transferable(Scope::Global).map(|(k, _)| k).collect();
        let expected: Vec<_> = keys(Scope::Global).filter(|k| *k != workspace::WORK_ENABLED_KEY).collect();
        assert_eq!(exported, expected);
    }
}
