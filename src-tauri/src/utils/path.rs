//! ナレッジディレクトリのサンドボックス。LLM出力・フロントエンド入力のパスはすべてここを通す。
use std::path::{Component, Path, PathBuf};

use crate::utils::error::AppError;

/// ファイル名/ディレクトリ名として安全なスラッグへ変換する。
/// 英数字(日本語含む)・`_`・`-`以外は`_`に置換し、区切り文字や`.`は残さない。
pub fn slug(s: &str, max_chars: usize) -> String {
    let cleaned: String = s
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .take(max_chars)
        .collect();
    cleaned.trim_matches('_').to_owned()
}

/// `a/b`形式のカテゴリを最大2階層の安全なパス要素へ分解する。空なら`Inbox`。
pub fn category_parts(category: &str) -> Vec<String> {
    let parts: Vec<String> = category
        .split(['/', '\\'])
        .map(|p| slug(p, 40))
        .filter(|p| !p.is_empty())
        .take(2)
        .collect();
    if parts.is_empty() { vec!["Inbox".to_owned()] } else { parts }
}

/// `root`配下の相対パスのみ許可する。絶対パス・`..`・シンボリックリンク経由の脱出を拒否する。
/// 戻り値は存在する場合は正規化済みパス。
pub fn resolve_existing(root: &Path, rel: &str) -> Result<PathBuf, AppError> {
    let rel = Path::new(rel);
    if rel.as_os_str().is_empty()
        || !rel.components().all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(AppError::PathNotAllowed);
    }
    let root = root.canonicalize().map_err(|e| AppError::Io(e.to_string()))?;
    let full = root.join(rel).canonicalize().map_err(|e| AppError::Io(e.to_string()))?;
    if full.starts_with(&root) { Ok(full) } else { Err(AppError::PathNotAllowed) }
}

/// 書き込み先ディレクトリを作成し、正規化後も`root`配下であることを確認して返す。
pub fn ensure_dir(root: &Path, parts: &[String]) -> Result<PathBuf, AppError> {
    std::fs::create_dir_all(root).map_err(|e| AppError::Io(e.to_string()))?;
    let root = root.canonicalize().map_err(|e| AppError::Io(e.to_string()))?;
    let mut dir = root.clone();
    for p in parts {
        dir.push(p);
    }
    std::fs::create_dir_all(&dir).map_err(|e| AppError::Io(e.to_string()))?;
    let dir = dir.canonicalize().map_err(|e| AppError::Io(e.to_string()))?;
    if dir.starts_with(&root) { Ok(dir) } else { Err(AppError::PathNotAllowed) }
}

/// 新規作成するファイル名として安全か。英数字(日本語含む)・`_`・`-`・`.`のみで、先頭の`.`と`..`は不可。
fn is_safe_leaf(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 100
        && !name.starts_with('.')
        && !name.contains("..")
        && name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// `root`配下に新規作成するファイルの書き込み先を返す。親ディレクトリは必要なら作成する。
/// 各要素は`slug`で変化しない安全な名前に限る。既存のシンボリックリンクは拒否する(上書き可否は呼び出し側が決める)。
pub fn resolve_new_file(root: &Path, rel: &str) -> Result<PathBuf, AppError> {
    let rel = Path::new(rel);
    let names: Vec<&str> = rel
        .components()
        .map(|c| match c {
            Component::Normal(n) => n.to_str().ok_or(AppError::PathNotAllowed),
            _ => Err(AppError::PathNotAllowed),
        })
        .collect::<Result<_, _>>()?;
    let Some((leaf, dirs)) = names.split_last() else {
        return Err(AppError::PathNotAllowed);
    };
    if !is_safe_leaf(leaf) || dirs.iter().any(|d| slug(d, 40) != *d) {
        return Err(AppError::PathNotAllowed);
    }
    let parts: Vec<String> = dirs.iter().map(|d| (*d).to_owned()).collect();
    let file = ensure_dir(root, &parts)?.join(leaf);
    if file.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(AppError::PathNotAllowed);
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_file_accepts_safe_and_rejects_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        let ok = resolve_new_file(&root, "Dev/Rust/memo.md").unwrap();
        assert!(ok.starts_with(root.canonicalize().unwrap()));
        assert!(ok.parent().unwrap().is_dir());
        assert!(resolve_new_file(&root, "memo.md").is_ok());
        for bad in ["", "/etc/x.md", "../x.md", "A/../x.md", ".hidden", "A/..md", "A B/x.md"] {
            assert!(matches!(resolve_new_file(&root, bad), Err(AppError::PathNotAllowed)), "{bad}");
        }
        assert!(!dir.path().join("x.md").exists());
    }

    #[cfg(unix)]
    #[test]
    fn new_file_rejects_symlink_leaf_and_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(dir.path().join("secret.txt"), "s").unwrap();
        std::os::unix::fs::symlink(dir.path().join("secret.txt"), root.join("link.md")).unwrap();
        std::os::unix::fs::symlink(dir.path(), root.join("out")).unwrap();
        assert!(matches!(resolve_new_file(&root, "link.md"), Err(AppError::PathNotAllowed)));
        assert!(matches!(resolve_new_file(&root, "out/x.md"), Err(AppError::PathNotAllowed)));
    }

    #[test]
    fn slug_strips_separators_and_dots() {
        assert_eq!(slug("../../etc/passwd", 60), "etc_passwd");
        assert_eq!(slug("..", 60), "");
        assert_eq!(slug("Tauri 設計メモ", 60), "Tauri_設計メモ");
        assert_eq!(slug(&"a".repeat(100), 10).len(), 10);
    }

    #[test]
    fn category_defaults_and_limits_depth() {
        assert_eq!(category_parts(""), ["Inbox"]);
        assert_eq!(category_parts("../.."), ["Inbox"]);
        assert_eq!(category_parts("Dev/Rust/Async"), ["Dev", "Rust"]);
    }

    #[test]
    fn resolve_rejects_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        std::fs::create_dir_all(root.join("A")).unwrap();
        std::fs::write(root.join("A/x.md"), "x").unwrap();
        std::fs::write(dir.path().join("secret.txt"), "s").unwrap();
        assert!(resolve_existing(&root, "A/x.md").is_ok());
        assert!(matches!(resolve_existing(&root, "../secret.txt"), Err(AppError::PathNotAllowed)));
        assert!(matches!(resolve_existing(&root, "/etc/passwd"), Err(AppError::PathNotAllowed)));
        assert!(matches!(resolve_existing(&root, ""), Err(AppError::PathNotAllowed)));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_rejects_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(dir.path().join("secret.txt"), "s").unwrap();
        std::os::unix::fs::symlink(dir.path().join("secret.txt"), root.join("link.md")).unwrap();
        assert!(matches!(resolve_existing(&root, "link.md"), Err(AppError::PathNotAllowed)));
    }
}
