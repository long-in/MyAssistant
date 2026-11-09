//! LLMへ渡す指示(プロンプト)の一覧と、エージェントごとの上書き。
//!
//! - 機能ごとに既定のプロンプトを持ち、エージェントのDBの設定(`prompt_*`)に保存した文章があればそれを使う。
//! - 保存する文章は全文。空、または既定と同じ文章は「上書きなし」として扱い、既定に戻す(既定の更新にも追従する)。
//! - チャットだけは全体設定の「ベース指示」も既定の代わりになる(エージェントの上書き → 全体設定 → 組み込みの既定)。
//! - 会話ログなどのデータ部分は各機能が後ろへ付けるので、ここでは扱わない。
use serde::Serialize;

use super::{
    batch, compact, global_settings::SettingsStore, leader, memory, proofread, skills,
};
use crate::{commands::chat, utils::error::AppError};

/// 1つのプロンプトの文字数の上限。
pub const MAX_CHARS: usize = 20_000;

pub const CHAT_KEY: &str = "prompt_chat";
pub const SUMMARY_KEY: &str = "prompt_summary";
pub const PROOFREAD_KEY: &str = "prompt_proofread";
pub const ORGANIZE_KEY: &str = "prompt_organize";
pub const MEMORY_KEY: &str = "prompt_memory";
pub const SKILLS_KEY: &str = "prompt_skills";
pub const SUGGEST_KEY: &str = "prompt_suggest";

/// プロンプトを持つ機能。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptId {
    /// チャット本体のベース指示。
    Chat,
    /// 長い会話の要約。
    Summary,
    /// 音声認識結果の校正。
    Proofread,
    /// ナレッジ整理バッチ。
    Organize,
    /// 記憶の更新。
    Memory,
    /// スキルの選定。
    Skills,
    /// リーダーの宛先の提案。
    Suggest,
}

impl PromptId {
    pub const ALL: [PromptId; 7] = [
        PromptId::Chat,
        PromptId::Summary,
        PromptId::Proofread,
        PromptId::Organize,
        PromptId::Memory,
        PromptId::Skills,
        PromptId::Suggest,
    ];

    /// 保存先の設定キー。
    pub fn key(self) -> &'static str {
        match self {
            PromptId::Chat => CHAT_KEY,
            PromptId::Summary => SUMMARY_KEY,
            PromptId::Proofread => PROOFREAD_KEY,
            PromptId::Organize => ORGANIZE_KEY,
            PromptId::Memory => MEMORY_KEY,
            PromptId::Skills => SKILLS_KEY,
            PromptId::Suggest => SUGGEST_KEY,
        }
    }

    /// 組み込みの既定。
    fn builtin(self) -> String {
        match self {
            PromptId::Chat => chat::DEFAULT_BASE_INSTRUCTIONS.to_owned(),
            PromptId::Summary => compact::DEFAULT_SYSTEM.to_owned(),
            PromptId::Proofread => proofread::DEFAULT_SYSTEM.to_owned(),
            PromptId::Organize => batch::default_instructions(),
            PromptId::Memory => memory::default_instructions(),
            PromptId::Skills => skills::DEFAULT_SELECT_SYSTEM.to_owned(),
            PromptId::Suggest => leader::DEFAULT_SYSTEM.to_owned(),
        }
    }
}

/// 画面へ渡す、1つ分のプロンプト。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptInfo {
    pub id: PromptId,
    /// いま使われる文章。
    pub text: String,
    /// 「初期値へ戻す」で戻る文章。チャットは全体設定のベース指示があればそれ。
    pub default: String,
    /// このエージェントで上書きしているか。
    pub custom: bool,
}

/// 既定の文章。チャットだけは、全体設定のベース指示があればそれ。
fn default_text(global: &impl SettingsStore, id: PromptId) -> Result<String, AppError> {
    match id {
        PromptId::Chat => chat::base_instructions(global),
        _ => Ok(id.builtin()),
    }
}

/// このエージェントで保存した上書き。空白だけなら未保存と同じ。
fn stored(db: &impl SettingsStore, id: PromptId) -> Result<Option<String>, AppError> {
    Ok(db.get_setting(id.key())?.filter(|v| !v.trim().is_empty()))
}

/// 機能のプロンプト(チャット以外)。上書きがなければ既定。
pub fn load(db: &impl SettingsStore, id: PromptId) -> Result<String, AppError> {
    Ok(stored(db, id)?.unwrap_or_else(|| id.builtin()))
}

/// チャットのベース指示。エージェントの上書き → 全体設定 → 組み込みの既定の順。
pub fn load_chat(db: &impl SettingsStore, global: &impl SettingsStore) -> Result<String, AppError> {
    match stored(db, PromptId::Chat)? {
        Some(v) => Ok(v),
        None => default_text(global, PromptId::Chat),
    }
}

/// 全プロンプトの現在の状態。
pub fn list(db: &impl SettingsStore, global: &impl SettingsStore) -> Result<Vec<PromptInfo>, AppError> {
    PromptId::ALL
        .into_iter()
        .map(|id| {
            let default = default_text(global, id)?;
            let saved = stored(db, id)?;
            Ok(PromptInfo { id, custom: saved.is_some(), text: saved.unwrap_or_else(|| default.clone()), default })
        })
        .collect()
}

/// 検証して保存し、保存後の状態を返す。空、または既定と同じ文章は上書きなし(既定に戻す)として保存する。
pub fn save(db: &impl SettingsStore, global: &impl SettingsStore, id: PromptId, text: &str) -> Result<PromptInfo, AppError> {
    let text = text.trim().replace("\r\n", "\n");
    if text.chars().count() > MAX_CHARS {
        return Err(AppError::InvalidAgentData(format!("{MAX_CHARS}文字以内で入力してください")));
    }
    if text.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err(AppError::InvalidAgentData("使えない文字(制御文字)が含まれています".into()));
    }
    let keep = text != default_text(global, id)?.trim();
    db.set_setting(id.key(), if keep { &text } else { "" })?;
    info(db, global, id)
}

/// 上書きを外して既定へ戻し、戻した後の状態を返す。
pub fn reset(db: &impl SettingsStore, global: &impl SettingsStore, id: PromptId) -> Result<PromptInfo, AppError> {
    db.set_setting(id.key(), "")?;
    info(db, global, id)
}

fn info(db: &impl SettingsStore, global: &impl SettingsStore, id: PromptId) -> Result<PromptInfo, AppError> {
    list(db, global)?.into_iter().find(|p| p.id == id).ok_or_else(|| AppError::Db("unknown prompt".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::HashMap};

    #[derive(Default)]
    struct Mem(RefCell<HashMap<String, String>>);

    impl SettingsStore for Mem {
        fn get_setting(&self, key: &str) -> Result<Option<String>, AppError> {
            Ok(self.0.borrow().get(key).cloned())
        }

        fn set_setting(&self, key: &str, value: &str) -> Result<(), AppError> {
            self.0.borrow_mut().insert(key.to_owned(), value.to_owned());
            Ok(())
        }
    }

    #[test]
    fn keys_are_unique_and_prefixed() {
        let keys: std::collections::HashSet<_> = PromptId::ALL.iter().map(|p| p.key()).collect();
        assert_eq!(keys.len(), PromptId::ALL.len());
        assert!(keys.iter().all(|k| k.starts_with("prompt_")));
    }

    #[test]
    fn falls_back_to_default_until_saved() {
        let (db, global) = (Mem::default(), Mem::default());
        assert_eq!(load(&db, PromptId::Summary).unwrap(), compact::DEFAULT_SYSTEM);
        let saved = save(&db, &global, PromptId::Summary, "  独自の指示  ").unwrap();
        assert!(saved.custom);
        assert_eq!(load(&db, PromptId::Summary).unwrap(), "独自の指示");
        let back = reset(&db, &global, PromptId::Summary).unwrap();
        assert!(!back.custom);
        assert_eq!(load(&db, PromptId::Summary).unwrap(), compact::DEFAULT_SYSTEM);
    }

    #[test]
    fn empty_or_default_text_is_not_an_override() {
        let (db, global) = (Mem::default(), Mem::default());
        assert!(!save(&db, &global, PromptId::Proofread, "   ").unwrap().custom);
        let same = format!("{}\n", proofread::DEFAULT_SYSTEM);
        assert!(!save(&db, &global, PromptId::Proofread, &same).unwrap().custom);
        assert_eq!(db.get_setting(PROOFREAD_KEY).unwrap().as_deref(), Some(""));
    }

    #[test]
    fn rejects_too_long_and_control_chars() {
        let (db, global) = (Mem::default(), Mem::default());
        assert!(save(&db, &global, PromptId::Skills, &"あ".repeat(MAX_CHARS + 1)).is_err());
        assert!(save(&db, &global, PromptId::Skills, "a\u{0}b").is_err());
        assert!(save(&db, &global, PromptId::Skills, "改行\nとタブ\tは可 <tag>").is_ok());
    }

    #[test]
    fn chat_prefers_agent_then_global_then_builtin() {
        let (db, global) = (Mem::default(), Mem::default());
        assert_eq!(load_chat(&db, &global).unwrap(), chat::DEFAULT_BASE_INSTRUCTIONS);
        global.set_setting(chat::BASE_INSTRUCTIONS_KEY, "全体の指示").unwrap();
        assert_eq!(load_chat(&db, &global).unwrap(), "全体の指示");
        let chat_info = save(&db, &global, PromptId::Chat, "エージェントの指示").unwrap();
        assert_eq!(chat_info.default, "全体の指示");
        assert_eq!(load_chat(&db, &global).unwrap(), "エージェントの指示");
        reset(&db, &global, PromptId::Chat).unwrap();
        assert_eq!(load_chat(&db, &global).unwrap(), "全体の指示");
    }
}
