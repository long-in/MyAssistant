//! 旧形式のスキルと、廃止したハーネスを`Skills/<名前>/SKILL.md`(`services::skills`)へ移す。
//!
//! 1. `.my-assistant/skills/<名前>.md`(旧形式のスキル)。
//! 2. `Harness/<名前>.md`(ハーネス)。`output`は本文の末尾へ「出力形式」として足し、`paths` `tools` `max_steps`は捨てる。
//!
//! 移せたものは元のファイルを消し、空になった旧フォルダも消す。同名のスキルがある、または検証に通らないものは元のまま残す
//! (`Harness/`に残ったものは通常のナレッジになる)。旧フォルダが無ければ存在を確かめるだけで終わるので、スキルを読む前に毎回呼んでよい。
use std::{fs, path::Path};

use super::skills::{self, Skill};

/// 旧形式のスキルのフォルダ(ナレッジ直下からの相対)。
const LEGACY_SKILLS_DIR: [&str; 2] = [".my-assistant", "skills"];
/// ハーネスのフォルダ(ナレッジ直下)。
const HARNESS_DIR: &str = "Harness";
/// 1ファイルとして読むサイズの上限(バイト)。
const FILE_MAX_BYTES: u64 = 100_000;

/// 旧形式のスキルとハーネスを移す。失敗はログに残すだけで、呼び出し元の処理は止めない。
pub fn migrate(root: &Path) {
    let legacy = LEGACY_SKILLS_DIR.iter().fold(root.to_path_buf(), |p, part| p.join(part));
    migrate_dir(root, &legacy, skills::parse);
    migrate_dir(root, &root.join(HARNESS_DIR), from_harness);
}

fn is_symlink(path: &Path) -> bool {
    path.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink())
}

/// `dir`直下の`<名前>.md`をスキルとして作り、作れたら元のファイルを消す。移した件数を返す。
fn migrate_dir(root: &Path, dir: &Path, convert: impl Fn(&str, &str) -> Skill) -> usize {
    if is_symlink(dir) || !dir.is_dir() {
        return 0;
    }
    // ナレッジの外へ出るフォルダには触れない
    let (Ok(root_real), Ok(dir_real)) = (root.canonicalize(), dir.canonicalize()) else { return 0 };
    if !dir_real.starts_with(&root_real) {
        return 0;
    }
    let Ok(entries) = fs::read_dir(&dir_real) else { return 0 };
    let mut moved = 0;
    for path in entries.flatten().map(|e| e.path()) {
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        if path.extension().is_none_or(|x| x != "md") || !skills::valid_name(name) || is_symlink(&path) {
            continue;
        }
        let Ok(meta) = path.metadata() else { continue };
        if !meta.is_file() || meta.len() > FILE_MAX_BYTES {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else { continue };
        match skills::create(root, convert(name, &text)) {
            Ok(s) => match fs::remove_file(&path) {
                Ok(()) => {
                    moved += 1;
                    log::info!("migrated {} to skill {}", path.display(), s.name);
                }
                Err(e) => log::warn!("migrated {} but could not remove it: {e}", path.display()),
            },
            Err(e) => log::warn!("skill migration skipped {}: {e}", path.display()),
        }
    }
    // 空になったときだけ消える(残ったファイルがあれば失敗する)
    let _ = fs::remove_dir(&dir_real);
    moved
}

/// ハーネスのファイルをスキルにする。`output`があれば本文の末尾へ足す。
fn from_harness(name: &str, text: &str) -> Skill {
    let mut skill = skills::parse(name, text);
    if let Some(output) = frontmatter_value(text, "output").filter(|o| !o.trim().is_empty()) {
        skill.body = format!("{}\n\n出力形式: {}", skill.body, output.trim());
    }
    skill
}

/// Frontmatterの文字列の値を読む(JSONの文字列でなければ、そのままの文字列)。
fn frontmatter_value(text: &str, key: &str) -> Option<String> {
    let text = text.replace("\r\n", "\n");
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    rest[..end].lines().find_map(|line| {
        let (k, v) = line.split_once(':')?;
        let v = v.trim();
        (k.trim() == key).then(|| serde_json::from_str::<String>(v).unwrap_or_else(|_| v.to_owned()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        fs::create_dir_all(&root).unwrap();
        (dir, root)
    }

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    const HARNESS: &str = "---\ndescription: \"週報を作る\"\ntriggers: [\"週報\"]\npaths: [\"Projects\"]\ntools: [\"read_file\"]\nmax_steps: 10\noutput: \"箇条書きで\"\n---\n\n今週のノートを読む";

    #[test]
    fn legacy_skills_move_to_the_new_folder() {
        let (_d, root) = root();
        write(&root, ".my-assistant/skills/daily.md", "---\nname: \"daily\"\ndescription: \"日報\"\ntriggers: [\"日報\"]\n---\n\n手順");
        write(&root, ".my-assistant/command-policy.json", "{}");
        migrate(&root);
        let s = skills::get(&root, "daily").unwrap();
        assert_eq!((s.description.as_str(), s.triggers.as_slice(), s.body.as_str()), ("日報", ["日報".to_owned()].as_slice(), "手順"));
        assert!(!root.join(".my-assistant/skills").exists(), "空になった旧フォルダは消す");
        assert!(root.join(".my-assistant/command-policy.json").is_file(), "コマンドの権限には触れない");
    }

    #[test]
    fn harness_becomes_a_skill_with_output_appended() {
        let (_d, root) = root();
        write(&root, "Harness/週報.md", HARNESS);
        migrate(&root);
        let s = skills::get(&root, "週報").unwrap();
        assert_eq!(s.description, "週報を作る");
        assert_eq!(s.triggers, ["週報"]);
        assert_eq!(s.body, "今週のノートを読む\n\n出力形式: 箇条書きで");
        assert!(!root.join("Harness").exists());
    }

    #[test]
    fn conflicts_and_invalid_files_stay_in_place() {
        let (_d, root) = root();
        skills::create(&root, Skill { name: "dup".into(), description: "既存".into(), triggers: vec![], body: "既存の手順".into() }).unwrap();
        write(&root, "Harness/dup.md", HARNESS);
        write(&root, "Harness/no-front.md", "Frontmatterの無いメモ");
        write(&root, "Harness/note.txt", "md以外");
        migrate(&root);
        assert_eq!(skills::get(&root, "dup").unwrap().body, "既存の手順", "既存のスキルは上書きしない");
        for rest in ["Harness/dup.md", "Harness/no-front.md", "Harness/note.txt"] {
            assert!(root.join(rest).is_file(), "{rest}");
        }
        assert!(skills::get(&root, "no-front").is_none());
    }

    #[test]
    fn migration_is_idempotent_and_noop_without_legacy_folders() {
        let (_d, root) = root();
        migrate(&root);
        assert!(!root.join("Skills").exists(), "移すものが無ければ何も作らない");
        write(&root, "Harness/a.md", HARNESS);
        migrate(&root);
        migrate(&root);
        assert_eq!(skills::list(&root).len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed() {
        let (d, root) = root();
        write(d.path(), "outside/x.md", HARNESS);
        std::os::unix::fs::symlink(d.path().join("outside"), root.join("Harness")).unwrap();
        fs::create_dir_all(root.join(".my-assistant/skills")).unwrap();
        std::os::unix::fs::symlink(d.path().join("outside/x.md"), root.join(".my-assistant/skills/y.md")).unwrap();
        migrate(&root);
        assert!(skills::list(&root).is_empty());
        assert!(d.path().join("outside/x.md").is_file(), "ナレッジの外のファイルは消さない");
    }
}
