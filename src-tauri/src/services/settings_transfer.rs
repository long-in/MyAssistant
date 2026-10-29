//! 設定のエクスポート・インポート。
//! 対象は`app_settings`のうち下の一覧にある項目(パーソナルデータを含む)とコマンド権限。APIキーと、端末ごとに異なるナレッジの保存先、
//! 会話ごとの要約は含めない。取り込み時は全項目を検証してから書き込み、不正な項目があれば何も変更しない。
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{command_policy::Policy, debug_log, profile, proofread, tuning, wake};
use crate::{
    commands::{chat, voice},
    db::Db,
    utils::error::AppError,
};

/// ファイルの形式を識別する値と版。
const APP_ID: &str = "tachicoma-agent";
const FORMAT_VERSION: u32 = 1;
/// 取り込むファイルの大きさの上限(バイト)。
pub const MAX_FILE_BYTES: u64 = 1_000_000;
const MAX_TEXT_CHARS: usize = 20_000;

/// 値の検証方法。
#[derive(Clone, Copy)]
enum Kind {
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
}

const ENTRIES: &[(&str, Kind)] = &[
    (chat::ROUTE_KEY, Kind::Json),
    (chat::BASE_INSTRUCTIONS_KEY, Kind::Text),
    (chat::WEB_ACCESS_KEY, Kind::Flag),
    (chat::SHELL_ACCESS_KEY, Kind::Flag),
    (voice::STT_ENGINE_KEY, Kind::OneOf(&["local", "google", "aqua"])),
    (voice::STT_LANGUAGE_KEY, Kind::Text),
    (proofread::ROUTE_KEY, Kind::Json),
    (wake::ENABLED_KEY, Kind::Flag),
    (wake::WORDS_KEY, Kind::Json),
    (tuning::KEY, Kind::Json),
    (super::batch::BATCH_ROUTE_KEY, Kind::Json),
    (super::batch::BATCH_INTERVAL_KEY, Kind::Text),
    (debug_log::ENABLED_KEY, Kind::Flag),
    (profile::KEY, Kind::Profile),
];

#[derive(Debug, Serialize, Deserialize)]
pub struct SettingsFile {
    app: String,
    version: u32,
    exported_at: String,
    settings: BTreeMap<String, String>,
    command_policy: Option<Policy>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Report {
    pub applied: usize,
    pub skipped: usize,
}

fn valid(kind: Kind, value: &str) -> bool {
    match kind {
        Kind::Flag => matches!(value, "0" | "1"),
        Kind::Json => value.is_empty() || serde_json::from_str::<Value>(value).is_ok(),
        Kind::OneOf(allowed) => allowed.contains(&value),
        Kind::Text => value.chars().count() <= MAX_TEXT_CHARS,
        Kind::Profile => profile::valid_json(value, chrono::Local::now().date_naive()),
    }
}

/// 現在の設定をファイルの内容にまとめる。未設定の項目は含めない。
pub fn collect(db: &Db, policy: Policy, exported_at: String) -> Result<SettingsFile, AppError> {
    let mut settings = BTreeMap::new();
    for (key, _) in ENTRIES {
        if let Some(v) = db.get_setting(key)? {
            settings.insert((*key).to_owned(), v);
        }
    }
    Ok(SettingsFile {
        app: APP_ID.into(),
        version: FORMAT_VERSION,
        exported_at,
        settings,
        command_policy: Some(policy),
    })
}

impl SettingsFile {
    /// 書き出される項目数(コマンド権限を含む)。
    pub fn item_count(&self) -> usize {
        self.settings.len() + usize::from(self.command_policy.is_some())
    }
}

/// 文字列からファイルの内容を読み、形式を確認する。
pub fn parse(text: &str) -> Result<SettingsFile, AppError> {
    let file: SettingsFile = serde_json::from_str(text).map_err(|e| AppError::InvalidSettings(e.to_string()))?;
    if file.app != APP_ID {
        return Err(AppError::InvalidSettings("このアプリの設定ファイルではありません".into()));
    }
    if file.version != FORMAT_VERSION {
        return Err(AppError::InvalidSettings(format!("未対応の版です: {}", file.version)));
    }
    Ok(file)
}

/// 検証を通った項目を書き込む。ファイルに無い項目は変更せず、未知の項目は読み飛ばす。
/// 戻り値の2番目は、反映するコマンド権限(あれば)。
pub fn apply(db: &Db, file: &SettingsFile) -> Result<(Report, Option<Policy>), AppError> {
    let mut accepted = Vec::new();
    let mut skipped = 0;
    for (key, value) in &file.settings {
        match ENTRIES.iter().find(|(k, _)| *k == key) {
            Some((k, kind)) if valid(*kind, value) => accepted.push((*k, value.as_str())),
            Some(_) => return Err(AppError::InvalidSettings(format!("{key}の値が不正です"))),
            None => skipped += 1,
        }
    }
    let policy = file
        .command_policy
        .as_ref()
        .map(|p| p.cleaned().map_err(AppError::InvalidSettings))
        .transpose()?;
    for (k, v) in &accepted {
        db.set_setting(k, v)?;
    }
    let applied = accepted.len() + usize::from(policy.is_some());
    Ok((Report { applied, skipped }, policy))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db")).unwrap();
        (dir, db)
    }

    fn file(settings: &[(&str, &str)]) -> SettingsFile {
        SettingsFile {
            app: APP_ID.into(),
            version: FORMAT_VERSION,
            exported_at: String::new(),
            settings: settings.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
            command_policy: None,
        }
    }

    #[test]
    fn roundtrip_between_databases() {
        let (_da, a) = mem();
        a.set_setting(chat::SHELL_ACCESS_KEY, "0").unwrap();
        a.set_setting(chat::ROUTE_KEY, r#"{"provider":"openai","model":"x"}"#).unwrap();
        a.set_setting("knowledge_dir", "/somewhere").unwrap();
        a.set_setting("summary:abc", "{}").unwrap();
        let text = serde_json::to_string(&collect(&a, Policy::defaults(), "t".into()).unwrap()).unwrap();
        assert!(!text.contains("/somewhere") && !text.contains("summary:abc"));

        let (_db, b) = mem();
        let (report, policy) = apply(&b, &parse(&text).unwrap()).unwrap();
        assert_eq!(report, Report { applied: 3, skipped: 0 });
        assert!(policy.is_some());
        assert_eq!(b.get_setting(chat::SHELL_ACCESS_KEY).unwrap().as_deref(), Some("0"));
        assert_eq!(b.get_setting("knowledge_dir").unwrap(), None);
    }

    #[test]
    fn invalid_value_changes_nothing() {
        let (_d, db) = mem();
        let f = file(&[(chat::SHELL_ACCESS_KEY, "1"), (chat::WEB_ACCESS_KEY, "yes")]);
        assert!(apply(&db, &f).is_err());
        assert_eq!(db.get_setting(chat::SHELL_ACCESS_KEY).unwrap(), None);
    }

    #[test]
    fn profile_roundtrips_and_bad_profile_changes_nothing() {
        let (_da, a) = mem();
        let json = r#"{"enabled":true,"name":"山田","birthday":"1990-05-20"}"#;
        a.set_setting(profile::KEY, json).unwrap();
        let text = serde_json::to_string(&collect(&a, Policy::defaults(), "t".into()).unwrap()).unwrap();
        let (_db, b) = mem();
        apply(&b, &parse(&text).unwrap()).unwrap();
        assert_eq!(b.get_setting(profile::KEY).unwrap().as_deref(), Some(json));

        let bad = file(&[(chat::SHELL_ACCESS_KEY, "1"), (profile::KEY, r#"{"name":"a<b"}"#)]);
        assert!(apply(&b, &bad).is_err());
        assert_eq!(b.get_setting(chat::SHELL_ACCESS_KEY).unwrap(), None);
    }

    #[test]
    fn unknown_keys_are_skipped_and_foreign_files_rejected() {
        let (_d, db) = mem();
        let (report, _) = apply(&db, &file(&[("knowledge_dir", "/x"), ("secrets", "y")])).unwrap();
        assert_eq!(report, Report { applied: 0, skipped: 2 });
        assert_eq!(db.get_setting("knowledge_dir").unwrap(), None);
        assert!(parse("{}").is_err());
        assert!(parse(r#"{"app":"other","version":1,"exported_at":"","settings":{},"command_policy":null}"#).is_err());
    }
}
