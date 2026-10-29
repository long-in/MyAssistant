//! 改名前のアプリデータディレクトリからの移行。
use std::path::Path;

/// 改名前のアプリ識別子(データディレクトリ名)。
const LEGACY_IDENTIFIER: &str = "dev.longin.tachicoma-agent";

/// 旧ディレクトリがあり新ディレクトリが無ければ、丸ごと改名して引き継ぐ。
/// 失敗しても起動は妨げず、旧データはそのまま残す。
pub fn migrate(dir: &Path) {
    let Some(old) = dir.parent().map(|p| p.join(LEGACY_IDENTIFIER)) else {
        return;
    };
    if !old.is_dir() || dir.exists() {
        return;
    }
    match std::fs::rename(&old, dir) {
        Ok(()) => log::info!("app data migrated from {}", old.display()),
        Err(e) => log::warn!("app data migration failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_legacy_dir_when_new_is_absent() {
        let d = tempfile::tempdir().unwrap();
        let old = d.path().join(LEGACY_IDENTIFIER);
        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join("secrets.json"), "{}").unwrap();
        let new = d.path().join("MyAssistant");
        migrate(&new);
        assert!(new.join("secrets.json").is_file());
        assert!(!old.exists());
    }

    #[test]
    fn keeps_both_when_new_exists() {
        let d = tempfile::tempdir().unwrap();
        let old = d.path().join(LEGACY_IDENTIFIER);
        let new = d.path().join("MyAssistant");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        migrate(&new);
        assert!(old.is_dir());
    }
}
