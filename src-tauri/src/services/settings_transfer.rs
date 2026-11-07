//! 設定のエクスポート・インポート。
//! 対象は`app_settings`のうち下の一覧にある項目(パーソナルデータを含む)とコマンド権限。APIキーと、端末ごとに異なるナレッジの保存先、
//! 会話ごとの要約は含めない。取り込み時は全項目を検証してから書き込み、不正な項目があれば何も変更しない。
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    aws, batch,
    command_policy::{self, Policy},
    debug_log, profile, pronunciation, proofread, tuning, wake,
    workspace::WorkspaceCtx,
};
use crate::{
    commands::{chat, voice},
    db::Db,
    utils::error::AppError,
};

/// ファイルの形式を識別する値と版。
const APP_ID: &str = "MyAssistant";
/// 改名前のアプリ名で書き出されたファイルも取り込めるようにする。
const LEGACY_APP_ID: &str = "tachicoma-agent";
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
    (voice::TTS_ENGINE_KEY, Kind::OneOf(&["google", "voicevox"])),
    (voice::VOICEVOX_STYLE_KEY, Kind::Text),
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
    if file.app != APP_ID && file.app != LEGACY_APP_ID {
        return Err(AppError::InvalidSettings("このアプリの設定ファイルではありません".into()));
    }
    if file.version != FORMAT_VERSION {
        return Err(AppError::InvalidSettings(format!("未対応の版です: {}", file.version)));
    }
    Ok(file)
}

/// 検証を通った項目。書き込みはまだ行っていない。
struct Validated {
    accepted: Vec<(&'static str, String)>,
    skipped: usize,
    policy: Option<Policy>,
}

/// 全項目を検証する(何も書かない)。ファイルに無い項目は対象外、未知の項目は読み飛ばす。
fn validate(file: &SettingsFile) -> Result<Validated, AppError> {
    let mut accepted = Vec::new();
    let mut skipped = 0;
    for (key, value) in &file.settings {
        match ENTRIES.iter().find(|(k, _)| *k == key) {
            Some((k, kind)) if valid(*kind, value) => accepted.push((*k, value.clone())),
            Some(_) => return Err(AppError::InvalidSettings(format!("{key}の値が不正です"))),
            None => skipped += 1,
        }
    }
    let policy = file
        .command_policy
        .as_ref()
        .map(|p| p.cleaned().map_err(AppError::InvalidSettings))
        .transpose()?;
    Ok(Validated { accepted, skipped, policy })
}

impl Validated {
    /// 設定を1つのトランザクションで書く(途中で失敗したら、どれも書かれない)。
    fn write(&self, db: &Db) -> Result<(), AppError> {
        let items: Vec<(&str, &str)> = self.accepted.iter().map(|(k, v)| (*k, v.as_str())).collect();
        db.set_settings(&items)
    }

    fn report(&self) -> Report {
        Report { applied: self.accepted.len() + usize::from(self.policy.is_some()), skipped: self.skipped }
    }
}

/// 検証を通った項目を書き込む。ファイルに無い項目は変更せず、未知の項目は読み飛ばす。
/// 戻り値の2番目は、反映するコマンド権限(あれば)。
pub fn apply(db: &Db, file: &SettingsFile) -> Result<(Report, Option<Policy>), AppError> {
    let v = validate(file)?;
    v.write(db)?;
    Ok((v.report(), v.policy))
}

/// ワークスペース間の設定コピーの対象。
#[derive(Debug, Clone, Copy, Default)]
pub struct CopyOptions {
    /// APIキー・AWS設定もコピーする(コピー先の同名のキーは上書き)。
    pub include_keys: bool,
    /// 読み上げ辞書(ユーザー登録の語)もコピーする(コピー先の辞書は置き換わる)。
    pub include_dictionary: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct CopyReport {
    /// コピーした設定の項目数(コマンド権限を含む)。
    pub settings: usize,
    /// コピーしたキーの件数。
    pub keys: usize,
    /// コピーした読み上げ辞書の語数(コピーしなかったときは0)。
    pub words: usize,
}

/// `from`の設定を`to`へコピーする。`from`は変更しない。
/// 対象は書き出しと同じ項目とコマンド権限で、ナレッジの保存先・会話・要約・ナレッジ本体は含めない。読み上げ辞書は選んだときだけ写す。
/// 先に全項目を検証し(何も書かない)、失敗しうるファイル書き込み(キー・コマンド権限)を行ってから、設定をトランザクションで書く。
/// 途中で失敗したら、書いたキーとコマンド権限を元へ戻し、`to`は変わらない。
/// キーの生値はRust内で写し、画面には件数だけ返す。
pub fn copy(from: &WorkspaceCtx, to: &WorkspaceCtx, opts: CopyOptions) -> Result<CopyReport, AppError> {
    if from.id == to.id {
        return Err(AppError::InvalidSpace("same workspace".into()));
    }
    let policy = command_policy::load(&batch::knowledge_root(from)?);
    let mut file = collect(&from.db, policy, String::new())?;
    // パーソナルデータはパーソナル専用で、コピーしない(スペースにはスペースデータがある)
    file.settings.remove(profile::KEY);
    let v = validate(&file)?;
    // 辞書は、書き込みの前に整形・検証しておく(上限を超えるなら、何も書かずに失敗する)
    let words = if opts.include_dictionary { Some(pronunciation::normalize(pronunciation::load_user(&from.dir))?) } else { None };
    let prev_dictionary = words.as_ref().map(|_| std::fs::read(to.dir.join(pronunciation::DICTIONARY_FILE)).ok());

    // キーは選んだときだけ写す。Bedrockのモデル一覧は秘密ではない設定項目なので、キーなしでも写す(失敗したら戻す)
    let keys_backup = to.secrets.backup();
    let keys = if opts.include_keys {
        to.secrets.copy_from(&from.secrets)?
    } else {
        to.secrets.copy_aws_models_from(&from.secrets)?;
        0
    };
    let policy_root = batch::knowledge_root(to);
    let prev_policy = v.policy.as_ref().and_then(|_| policy_root.as_ref().ok()).map(|root| command_policy::load(root));
    let written = (|| {
        if let Some(policy) = &v.policy {
            // 保存先のフォルダは、作ったばかりのワークスペースに無くても`save`が作る
            let root = policy_root.as_ref().map_err(|e| AppError::Io(e.to_string()))?;
            command_policy::save(root, policy).map_err(AppError::Io)?;
        }
        if let Some(words) = &words {
            pronunciation::save_user(&to.dir, words.clone())?;
        }
        v.write(&to.db)
    })();
    if let Err(e) = written {
        // 元へ戻す(戻せなければ警告だけ残す)。コピー先の設定は書かれていない
        if let (Some(prev), Ok(root)) = (&prev_policy, &policy_root)
            && let Err(err) = command_policy::save(root, prev)
        {
            log::warn!("command policy not restored: {err}");
        }
        if let Some(prev) = &prev_dictionary {
            let file = to.dir.join(pronunciation::DICTIONARY_FILE);
            let restored = match prev {
                Some(bytes) => std::fs::write(&file, bytes),
                None => std::fs::remove_file(&file),
            };
            if let Err(err) = restored {
                log::warn!("dictionary not restored: {err}");
            }
        }
        if let Err(err) = to.secrets.restore(keys_backup) {
            log::warn!("secrets not restored: {err}");
        }
        return Err(e);
    }
    if opts.include_keys {
        aws::reset(to); // 差し替えたAWS設定で取り直すため、古い一時クレデンシャルは捨てる
    }
    Ok(CopyReport { settings: v.report().applied, keys, words: words.map_or(0, |w| w.len()) })
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
        a.set_setting(chat::ROUTE_KEY, r#"{"provider":"anthropic","model":"x"}"#).unwrap();
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

    fn ctx(id: &str) -> (tempfile::TempDir, WorkspaceCtx) {
        let d = tempfile::tempdir().unwrap();
        let c = WorkspaceCtx::open(id, d.path()).unwrap();
        (d, c)
    }

    #[test]
    fn copy_moves_settings_and_policy_but_not_profile_or_keys_by_default() {
        let (_da, a) = ctx("a");
        let (_db, b) = ctx("b");
        a.db.set_setting(chat::SHELL_ACCESS_KEY, "0").unwrap();
        a.db.set_setting(profile::KEY, r#"{"enabled":true,"name":"山田"}"#).unwrap();
        let custom = tempfile::tempdir().unwrap(); // コピー元は別の場所をナレッジの保存先にしている
        a.db.set_setting("knowledge_dir", &custom.path().to_string_lossy()).unwrap();
        a.db.set_setting("summary:abc", "{}").unwrap();
        a.secrets.set_tts_key("tts-a").unwrap();
        command_policy::add_allow(custom.path(), &["mytool".to_owned()]).unwrap();
        b.db.set_setting(chat::WEB_ACCESS_KEY, "0").unwrap(); // コピー元に無い項目は残る

        let report = copy(&a, &b, CopyOptions::default()).unwrap();
        assert_eq!(report.keys, 0);
        assert_eq!(b.db.get_setting(chat::SHELL_ACCESS_KEY).unwrap().as_deref(), Some("0"));
        assert_eq!(b.db.get_setting(chat::WEB_ACCESS_KEY).unwrap().as_deref(), Some("0"));
        for key in [profile::KEY, "knowledge_dir", "summary:abc"] {
            assert_eq!(b.db.get_setting(key).unwrap(), None, "{key}");
        }
        assert!(!b.secrets.has_tts_key().unwrap());
        assert!(command_policy::load(&batch::knowledge_root(&b).unwrap()).allow.contains(&"mytool".to_owned()));
        // コピー元は変わらない
        assert_eq!(a.db.get_setting(profile::KEY).unwrap().as_deref(), Some(r#"{"enabled":true,"name":"山田"}"#));
        assert!(a.secrets.has_tts_key().unwrap());
    }

    #[test]
    fn copy_options_include_keys_but_never_the_profile() {
        let (_da, a) = ctx("a");
        let (_db, b) = ctx("b");
        a.db.set_setting(profile::KEY, r#"{"enabled":true,"name":"山田"}"#).unwrap();
        a.db.set_setting(crate::services::space_data::KEY, r#"{"enabled":true,"text":"x"}"#).unwrap();
        a.secrets.set_tts_key("tts-a").unwrap();
        a.secrets.set_key(crate::services::secrets::Provider::Anthropic, "ka").unwrap();
        let report = copy(&a, &b, CopyOptions { include_keys: true, ..Default::default() }).unwrap();
        assert_eq!(report.keys, 2);
        assert_eq!(b.db.get_setting(profile::KEY).unwrap(), None, "パーソナルデータは、キーを写すときでも写さない");
        assert_eq!(b.db.get_setting(crate::services::space_data::KEY).unwrap(), None, "スペースデータも写さない");
        assert_eq!(b.secrets.get_tts_key().unwrap().as_deref(), Some("tts-a"));
    }

    #[test]
    fn dictionary_is_copied_only_when_asked_and_rolled_back_on_failure() {
        use crate::services::pronunciation::{load_user, save_user, Entry};
        let e = |w: &str, r: &str| Entry { word: w.into(), reading: r.into() };
        let (_da, a) = ctx("a");
        let (_db, b) = ctx("b");
        a.db.set_setting(chat::SHELL_ACCESS_KEY, "0").unwrap();
        save_user(&a.dir, vec![e("Figma", "フィグマ")]).unwrap();
        save_user(&b.dir, vec![e("Zoom", "ズーム")]).unwrap();
        // 選ばなければ、コピー先の辞書は変わらない
        let r = copy(&a, &b, CopyOptions::default()).unwrap();
        assert_eq!(r.words, 0);
        assert_eq!(load_user(&b.dir), vec![e("Zoom", "ズーム")]);
        // 選ぶと、コピー先の辞書が置き換わる。コピー元は変わらない
        let r = copy(&a, &b, CopyOptions { include_dictionary: true, ..Default::default() }).unwrap();
        assert_eq!(r.words, 1);
        assert_eq!(load_user(&b.dir), vec![e("Figma", "フィグマ")]);
        assert_eq!(load_user(&a.dir), vec![e("Figma", "フィグマ")]);
        // 設定の書き込みに失敗したら、辞書も元へ戻る
        save_user(&a.dir, vec![e("Miro", "ミロ")]).unwrap();
        b.db.break_settings_for_test();
        assert!(copy(&a, &b, CopyOptions { include_dictionary: true, ..Default::default() }).is_err());
        assert_eq!(load_user(&b.dir), vec![e("Figma", "フィグマ")]);
    }

    #[test]
    fn copy_to_same_workspace_is_rejected_and_bad_source_changes_nothing() {
        let (_da, a) = ctx("a");
        assert!(copy(&a, &a, CopyOptions::default()).is_err());
        let (_db, b) = ctx("b");
        b.db.set_setting(chat::SHELL_ACCESS_KEY, "1").unwrap();
        a.db.set_setting(chat::WEB_ACCESS_KEY, "yes").unwrap(); // 不正な値
        a.secrets.set_tts_key("tts-a").unwrap();
        assert!(copy(&a, &b, CopyOptions { include_keys: true, ..Default::default() }).is_err());
        assert_eq!(b.db.get_setting(chat::WEB_ACCESS_KEY).unwrap(), None);
        assert!(!b.secrets.has_tts_key().unwrap(), "キーも写さない");
    }

    #[test]
    fn key_copy_failure_leaves_destination_untouched() {
        let (_da, a) = ctx("a");
        let (_db, b) = ctx("b");
        a.db.set_setting(chat::SHELL_ACCESS_KEY, "0").unwrap();
        a.secrets.set_tts_key("tts-a").unwrap();
        // コピー先のキー保管庫を書けなくする(保存先に同名のフォルダがある)
        std::fs::create_dir(b.dir.join("secrets.json")).unwrap();
        assert!(copy(&a, &b, CopyOptions { include_keys: true, ..Default::default() }).is_err());
        assert_eq!(b.db.get_setting(chat::SHELL_ACCESS_KEY).unwrap(), None, "設定は書かれない");
        assert!(!b.secrets.has_tts_key().unwrap(), "メモリ上のキーも変わらない");
    }

    #[test]
    fn policy_and_keys_are_rolled_back_when_settings_write_fails() {
        let (_da, a) = ctx("a");
        let (_db, b) = ctx("b");
        let custom = tempfile::tempdir().unwrap();
        a.db.set_setting("knowledge_dir", &custom.path().to_string_lossy()).unwrap();
        command_policy::add_allow(custom.path(), &["mytool".to_owned()]).unwrap();
        a.db.set_setting(chat::SHELL_ACCESS_KEY, "0").unwrap(); // 書き込む設定がないと、失敗が起きない
        a.secrets.set_tts_key("tts-a").unwrap();
        b.secrets.set_aqua_key("aqua-b").unwrap();
        // 設定の書き込みを失敗させる(設定テーブルを壊す)
        b.db.break_settings_for_test();
        assert!(copy(&a, &b, CopyOptions { include_keys: true, ..Default::default() }).is_err());
        assert!(!b.secrets.has_tts_key().unwrap(), "コピーしたキーは戻る");
        assert!(b.secrets.has_aqua_key().unwrap(), "元からあったキーは残る");
        let policy = command_policy::load(&batch::knowledge_root(&b).unwrap());
        assert!(!policy.allow.contains(&"mytool".to_owned()), "コマンド権限も戻る");
    }

    #[test]
    fn bedrock_models_are_copied_even_without_keys() {
        let (_da, a) = ctx("a");
        let (_db, b) = ctx("b");
        a.secrets.set_aws_models(r#"["global.anthropic.claude-sonnet-5-5"]"#).unwrap();
        a.secrets.set_tts_key("tts-a").unwrap();
        let report = copy(&a, &b, CopyOptions::default()).unwrap();
        assert_eq!(report.keys, 0, "キーの件数には含めない");
        assert_eq!(b.secrets.get_aws_models().unwrap().as_deref(), Some(r#"["global.anthropic.claude-sonnet-5-5"]"#));
        assert!(!b.secrets.has_tts_key().unwrap(), "キーは写さない");
        assert!(!b.secrets.has_aws().unwrap(), "AWSの認証情報は写さない");
        // コピー元に一覧が無ければ、コピー先の一覧はそのまま
        let (_dc, c) = ctx("c");
        b.secrets.set_aws_models("[\"keep\"]").unwrap();
        copy(&c, &b, CopyOptions::default()).unwrap();
        assert_eq!(b.secrets.get_aws_models().unwrap().as_deref(), Some("[\"keep\"]"));
    }
}
