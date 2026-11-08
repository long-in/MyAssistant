//! 全体設定。すべてのスペース(パーソナルを含む)で共通の設定を1か所に持つ。
//!
//! - 保存先はアプリデータの`workspaces/global_settings.json`(キーと文字列値の対応)。スペースを削除しても残る。
//! - 各スペースのDBに同名の設定があっても読まない(各スペースから上書きできない)。値は消さない。
//! - 初回(ファイルが無いとき)だけ、パーソナルのDBの現在値を初期値として取り込む(`migrate_from`)。
//!
//! 項目の読み書きは`SettingsStore`を通すので、DBの設定と同じ形で扱える。
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde_json::Value;

use crate::{db::Db, utils::error::AppError};

const FILE: &str = "global_settings.json";
const DIR: &str = "workspaces";

/// 表示言語(`ja` / `en`)。画面が最初に決めた値を保存する。未保存ならnull扱い。
pub const LANGUAGE_KEY: &str = "language";
/// アプリが背面のときのOS通知を使うか(`0` / `1`)。未保存なら使う。
pub const OS_NOTIFICATIONS_KEY: &str = "os_notifications";

/// 全体設定に入れる項目。ファイルのキーはDBの設定キーと同じ名前を使う。
pub const KEYS: &[&str] = &[
    crate::commands::chat::BASE_INSTRUCTIONS_KEY,
    OS_NOTIFICATIONS_KEY,
    LANGUAGE_KEY,
    super::tuning::VOICE_KEY,
    super::wake::ENABLED_KEY,
    super::wake::WORDS_KEY,
    super::profile::KEY,
    super::batch::BATCH_INTERVAL_KEY,
];

/// 文字列の設定の読み書き。DBの設定と全体設定を、同じ形で扱うために使う。
pub trait SettingsStore {
    fn get_setting(&self, key: &str) -> Result<Option<String>, AppError>;
    fn set_setting(&self, key: &str, value: &str) -> Result<(), AppError>;
}

impl SettingsStore for Db {
    fn get_setting(&self, key: &str) -> Result<Option<String>, AppError> {
        Db::get_setting(self, key)
    }

    fn set_setting(&self, key: &str, value: &str) -> Result<(), AppError> {
        Db::set_setting(self, key, value)
    }
}

/// 全体設定のファイル。読み書きは1つのロックで直列にし、書き込みは一時ファイルからのリネームで途中の破損を避ける。
#[derive(Debug)]
pub struct GlobalStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl GlobalStore {
    /// `root`(アプリデータ)の全体設定を指す。ファイルは書き込むときまで作らない。
    pub fn open(root: &Path) -> Self {
        Self { path: root.join(DIR).join(FILE), lock: Mutex::new(()) }
    }

    /// 読めなければ(無い・壊れている)空。
    fn read(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(&self.path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    fn write(&self, map: &BTreeMap<String, String>) -> Result<(), AppError> {
        let io = |e: std::io::Error| AppError::Io(e.to_string());
        std::fs::create_dir_all(self.path.parent().unwrap_or(Path::new("."))).map_err(io)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(map).map_err(|e| AppError::Io(e.to_string()))?).map_err(io)?;
        std::fs::rename(&tmp, &self.path).map_err(io)
    }

    /// 壊れたファイルは`.corrupt`へ退避する(次の書き込みで消してしまわないため)。
    fn read_for_update(&self) -> BTreeMap<String, String> {
        match std::fs::read_to_string(&self.path) {
            Ok(t) => serde_json::from_str(&t).unwrap_or_else(|_| {
                let _ = std::fs::rename(&self.path, self.path.with_extension("json.corrupt"));
                BTreeMap::new()
            }),
            Err(_) => BTreeMap::new(),
        }
    }

    /// 複数の項目をまとめて書く(途中で失敗したら、どれも書かれない)。
    pub fn set_settings(&self, items: &[(&str, &str)]) -> Result<(), AppError> {
        let _g = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let mut map = self.read_for_update();
        for (k, v) in items {
            map.insert((*k).to_owned(), (*v).to_owned());
        }
        self.write(&map)
    }

    /// ファイルがまだ無いときだけ、パーソナルのDBの現在値を初期値として写す。VOICE TIMINGは調整項目から取り出す。
    pub fn migrate_from(&self, personal: &Db) -> Result<(), AppError> {
        let _g = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        if self.path.exists() {
            return Ok(());
        }
        let mut map = BTreeMap::new();
        for key in KEYS {
            if let Some(v) = personal.get_setting(key)? {
                map.insert((*key).to_owned(), v);
            }
        }
        if let Some(t) = personal.get_setting(super::tuning::KEY)?.and_then(|v| serde_json::from_str::<Value>(&v).ok()) {
            let pick: serde_json::Map<String, Value> = super::tuning::VOICE_FIELDS
                .iter()
                .filter_map(|f| t.get(*f).map(|v| ((*f).to_owned(), v.clone())))
                .collect();
            if !pick.is_empty() {
                map.insert(super::tuning::VOICE_KEY.to_owned(), Value::Object(pick).to_string());
            }
        }
        self.write(&map)
    }
}

impl SettingsStore for GlobalStore {
    fn get_setting(&self, key: &str) -> Result<Option<String>, AppError> {
        let _g = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        Ok(self.read().remove(key))
    }

    fn set_setting(&self, key: &str, value: &str) -> Result<(), AppError> {
        self.set_settings(&[(key, value)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn personal(dir: &Path) -> Db {
        Db::open(&dir.join("t.db")).unwrap()
    }

    #[test]
    fn roundtrips_and_survives_reopen() {
        let d = tempfile::tempdir().unwrap();
        let g = GlobalStore::open(d.path());
        assert_eq!(g.get_setting("a").unwrap(), None);
        g.set_setting("a", "1").unwrap();
        g.set_settings(&[("b", "2"), ("a", "3")]).unwrap();
        let g = GlobalStore::open(d.path());
        assert_eq!(g.get_setting("a").unwrap().as_deref(), Some("3"));
        assert_eq!(g.get_setting("b").unwrap().as_deref(), Some("2"));
    }

    #[test]
    fn first_run_adopts_personal_values_once() {
        let d = tempfile::tempdir().unwrap();
        let db = personal(d.path());
        db.set_setting("base_instructions", "敬語で").unwrap();
        db.set_setting("batch_interval_minutes", "60").unwrap();
        db.set_setting("web_access", "0").unwrap(); // 全体設定の対象外
        db.set_setting("tuning", r#"{"web_search_max":9,"voice_idle_secs":7.0}"#).unwrap();
        let g = GlobalStore::open(d.path());
        g.migrate_from(&db).unwrap();
        assert_eq!(g.get_setting("base_instructions").unwrap().as_deref(), Some("敬語で"));
        assert_eq!(g.get_setting("web_access").unwrap(), None);
        let voice: Value = serde_json::from_str(&g.get_setting("voice_timing").unwrap().unwrap()).unwrap();
        assert_eq!(voice["voice_idle_secs"], 7.0);
        assert!(voice.get("web_search_max").is_none());
        // 2回目以降は、パーソナルが変わっても取り込み直さない
        db.set_setting("base_instructions", "別の値").unwrap();
        g.migrate_from(&db).unwrap();
        assert_eq!(g.get_setting("base_instructions").unwrap().as_deref(), Some("敬語で"));
    }

    #[test]
    fn corrupt_file_is_set_aside_not_overwritten() {
        let d = tempfile::tempdir().unwrap();
        let g = GlobalStore::open(d.path());
        g.set_setting("a", "1").unwrap();
        std::fs::write(d.path().join("workspaces").join(FILE), "{broken").unwrap();
        assert_eq!(g.get_setting("a").unwrap(), None);
        g.set_setting("b", "2").unwrap();
        assert!(d.path().join("workspaces").join("global_settings.json.corrupt").exists());
        assert_eq!(g.get_setting("b").unwrap().as_deref(), Some("2"));
    }

    #[test]
    fn space_db_values_are_ignored_and_all_spaces_share_one_file() {
        use crate::services::{workspace::WorkspaceCtx, wake};
        let d = tempfile::tempdir().unwrap();
        let global = std::sync::Arc::new(GlobalStore::open(d.path()));
        let a = WorkspaceCtx::open_shared("a", &d.path().join("a"), Default::default(), global.clone()).unwrap();
        let b = WorkspaceCtx::open_shared("b", &d.path().join("b"), Default::default(), global).unwrap();
        // スペースのDBに古い値があっても、全体設定は読まない
        a.db.set_setting(wake::WORDS_KEY, r#"["旧"]"#).unwrap();
        a.db.set_setting(wake::ENABLED_KEY, "1").unwrap();
        assert!(wake::load(&*a.global).unwrap().words.is_empty());
        // どのスペースから保存しても、全スペースに同じ値が見える
        wake::save(&*b.global, &wake::WakeConfig { enabled: true, words: vec!["やあ".into()] }).unwrap();
        assert_eq!(wake::load(&*a.global).unwrap().words, vec!["やあ".to_owned()]);
    }
}
