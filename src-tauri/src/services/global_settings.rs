//! 全体設定。すべてのエージェント(パーソナルを含む)で共通の設定を1か所に持つ。
//!
//! - 保存先はアプリデータの`work/global_settings.json`(キーと文字列値の対応)。エージェントを削除しても残る。
//! - 項目は`settings_registry`で保存先が全体設定(`Scope::Global`)のキー。エージェントごとのキーは書き込みで拒否する。
//!
//! 項目の読み書きは`SettingsStore`を通すので、DBの設定と同じ形で扱える。
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use super::settings_registry;
use crate::{db::Db, utils::error::AppError};

const FILE: &str = "global_settings.json";

/// 表示言語(`ja` / `en`)。画面が最初に決めた値を保存する。未保存ならnull扱い。
pub const LANGUAGE_KEY: &str = "language";
/// アプリが背面のときのOS通知を使うか(`0` / `1`)。未保存なら使う。
pub const OS_NOTIFICATIONS_KEY: &str = "os_notifications";

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
        Self { path: root.join(super::workspace::DIR_NAME).join(FILE), lock: Mutex::new(()) }
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
        for (k, _) in items {
            settings_registry::check_global_write(k)?;
        }
        let _g = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let mut map = self.read_for_update();
        for (k, v) in items {
            map.insert((*k).to_owned(), (*v).to_owned());
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
    fn agent_keys_are_rejected() {
        let d = tempfile::tempdir().unwrap();
        let g = GlobalStore::open(d.path());
        assert!(g.set_settings(&[("base_instructions", "x"), ("routing", "{}")]).is_err());
        assert_eq!(g.get_setting("base_instructions").unwrap(), None);
    }

    #[test]
    fn corrupt_file_is_set_aside_not_overwritten() {
        let d = tempfile::tempdir().unwrap();
        let g = GlobalStore::open(d.path());
        g.set_setting("a", "1").unwrap();
        std::fs::write(d.path().join("work").join(FILE), "{broken").unwrap();
        assert_eq!(g.get_setting("a").unwrap(), None);
        g.set_setting("b", "2").unwrap();
        assert!(d.path().join("work").join("global_settings.json.corrupt").exists());
        assert_eq!(g.get_setting("b").unwrap().as_deref(), Some("2"));
    }

    #[test]
    fn agent_db_values_are_ignored_and_all_agents_share_one_file() {
        use crate::services::{workspace::WorkspaceCtx, wake};
        let d = tempfile::tempdir().unwrap();
        let global = std::sync::Arc::new(GlobalStore::open(d.path()));
        let a = WorkspaceCtx::open_shared("a", &d.path().join("a"), Default::default(), global.clone()).unwrap();
        let b = WorkspaceCtx::open_shared("b", &d.path().join("b"), Default::default(), global).unwrap();
        // エージェントのDBに古い値があっても、全体設定は読まない
        a.db.set_legacy_setting(wake::WORDS_KEY, r#"["旧"]"#);
        a.db.set_legacy_setting(wake::ENABLED_KEY, "1");
        assert!(wake::load(&*a.global).unwrap().words.is_empty());
        // どのエージェントから保存しても、全エージェントに同じ値が見える
        wake::save(&*b.global, &wake::WakeConfig { enabled: true, words: vec!["やあ".into()] }).unwrap();
        assert_eq!(wake::load(&*a.global).unwrap().words, vec!["やあ".to_owned()]);
    }
}
