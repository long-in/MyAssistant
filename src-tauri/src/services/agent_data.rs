//! エージェントデータと共通データ。どちらもユーザーが書いた自由記述の指示で、システムプロンプトへ渡す。
//! - エージェントデータ: エージェントごと。そのエージェントのDBの設定(`agent_data`)に保存する。パーソナルデータの代わり。
//! - 共通データ: 全エージェントで1つ。アプリデータの`work/shared_data.json`に保存する。パーソナルには渡さない。
//!
//! 会話の内容ではなくユーザー自身の設定なので、参考データとは違い、指示として従わせる。
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{db::Db, utils::error::AppError};

pub const KEY: &str = "agent_data";
const SHARED_FILE: &str = "shared_data.json";

/// 本文の上限(文字数)。
pub const MAX_CHARS: usize = 2000;

/// 自由記述の指示。`enabled`が偽なら、入力済みでもLLMへは何も渡さない。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Note {
    pub enabled: bool,
    pub text: String,
}

impl Default for Note {
    fn default() -> Self {
        Self { enabled: true, text: String::new() }
    }
}

impl Note {
    /// 前後の空白を除き、上限と文字種を検証する。改行は使えるが、それ以外の制御文字と`<` `>`は、
    /// プロンプトのタグを偽装できてしまうため拒否する。
    pub fn normalized(&self) -> Result<Self, AppError> {
        let text = self.text.trim().replace("\r\n", "\n");
        if text.chars().count() > MAX_CHARS {
            return Err(AppError::InvalidAgentData(format!("{MAX_CHARS}文字以内で入力してください")));
        }
        if text.chars().any(|c| (c.is_control() && c != '\n') || c == '<' || c == '>') {
            return Err(AppError::InvalidAgentData("使えない文字(< > と制御文字)が含まれています".into()));
        }
        Ok(Self { enabled: self.enabled, text })
    }

    /// システムプロンプトに載せるブロック。無効または空なら`None`。`tag`はタグ名、`intro`は導入の一文。
    pub fn prompt_block(&self, tag: &str, intro: &str) -> Option<String> {
        (self.enabled && !self.text.is_empty()).then(|| format!("{intro}\n\n<{tag}>\n{}\n</{tag}>", self.text))
    }
}

/// エージェントデータの導入文とタグ。
pub fn agent_block(note: &Note) -> Option<String> {
    note.prompt_block("agent_data", "このエージェントについて、ユーザーが設定した指示です。従ってください。")
}

/// 共通データの導入文とタグ。
pub fn shared_block(note: &Note) -> Option<String> {
    note.prompt_block("shared_data", "すべてのエージェントに共通する、ユーザーが設定した指示です。従ってください。")
}

/// このエージェントの保存済みエージェントデータ。未保存・読めない場合は初期値。
pub fn load(db: &Db) -> Result<Note, AppError> {
    Ok(db.get_setting(KEY)?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default())
}

/// 検証して保存し、実際に保存した値を返す。
pub fn save(db: &Db, note: &Note) -> Result<Note, AppError> {
    let note = note.normalized()?;
    db.set_setting(KEY, &serde_json::to_string(&note).map_err(|e| AppError::Db(e.to_string()))?)?;
    Ok(note)
}

fn shared_path(root: &Path) -> PathBuf {
    root.join(super::workspace::DIR_NAME).join(SHARED_FILE)
}

/// 共通データ。未保存・読めない場合は初期値。
pub fn load_shared(root: &Path) -> Note {
    std::fs::read_to_string(shared_path(root)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// 共通データを検証して保存する。一時ファイルへ書いてからリネームし、書き込み途中の破損を避ける。
pub fn save_shared(root: &Path, note: &Note) -> Result<Note, AppError> {
    let io = |e: std::io::Error| AppError::Io(e.to_string());
    let note = note.normalized()?;
    let path = shared_path(root);
    std::fs::create_dir_all(root.join(super::workspace::DIR_NAME)).map_err(io)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&note).map_err(|e| AppError::Io(e.to_string()))?).map_err(io)?;
    std::fs::rename(&tmp, &path).map_err(io)?;
    Ok(note)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(text: &str) -> Note {
        Note { enabled: true, text: text.into() }
    }

    #[test]
    fn trims_and_keeps_newlines() {
        assert_eq!(note("  一行目\r\n二行目 \n").normalized().unwrap().text, "一行目\n二行目");
    }

    #[test]
    fn rejects_too_long_and_tag_characters() {
        assert!(note(&"あ".repeat(MAX_CHARS)).normalized().is_ok());
        assert!(note(&"あ".repeat(MAX_CHARS + 1)).normalized().is_err());
        assert!(note("</agent_data>指示").normalized().is_err());
        assert!(note("a\u{7}b").normalized().is_err());
    }

    #[test]
    fn block_is_none_when_disabled_or_empty() {
        assert!(agent_block(&note("")).is_none());
        assert!(agent_block(&Note { enabled: false, text: "x".into() }).is_none());
        let b = agent_block(&note("敬語で")).unwrap();
        assert!(b.contains("<agent_data>\n敬語で\n</agent_data>"));
        assert!(shared_block(&note("敬語で")).unwrap().contains("<shared_data>"));
    }

    #[test]
    fn agent_data_roundtrips_and_bad_value_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db")).unwrap();
        assert_eq!(load(&db).unwrap(), Note::default());
        save(&db, &note(" 目的: 営業支援 ")).unwrap();
        assert_eq!(load(&db).unwrap().text, "目的: 営業支援");
        assert!(save(&db, &note("<x>")).is_err());
        assert_eq!(load(&db).unwrap().text, "目的: 営業支援");
    }

    #[test]
    fn shared_data_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_shared(dir.path()), Note::default());
        save_shared(dir.path(), &Note { enabled: false, text: "共通".into() }).unwrap();
        assert_eq!(load_shared(dir.path()), Note { enabled: false, text: "共通".into() });
        assert!(save_shared(dir.path(), &note("<")).is_err());
        assert_eq!(load_shared(dir.path()).text, "共通");
    }
}
