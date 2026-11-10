//! サンドボックスのエージェント(コンテナ内の Claude Code)の接続の設定と、渡す環境変数。
//!
//! - 接続先(Anthropic API / Amazon Bedrock)・モデル・1回の指示の上限時間を、エージェントごとに持つ(`LLM_KEY`)。
//! - API キーは`sandbox.env`に書く(アプリに登録した API キーは渡さない)。Bedrock は、アプリの AWS 設定から
//!   ホストで発行した一時キーを使う(`sandbox::env_for`)。
//! - アプリが決める環境変数は秘密でない値だけ。`sandbox.env`に同じ名前があれば、そちらが優先される。
use std::{collections::BTreeMap, ops::RangeInclusive};

use serde::{Deserialize, Serialize};

use super::global_settings::SettingsStore;
use crate::utils::error::AppError;

/// 接続の設定(エージェントごと)のキー。
pub const LLM_KEY: &str = "sandbox_llm";
/// 1回の指示の上限時間(分)の範囲。
pub const TIMEOUT_RANGE: RangeInclusive<u32> = 1..=180;
const MODEL_MAX_CHARS: usize = 200;

/// アプリが決める、秘密でない環境変数の名前。出力の伏せ字の対象から外す。
pub const NON_SECRET_NAMES: [&str; 2] = ["CLAUDE_CODE_USE_BEDROCK", "ANTHROPIC_MODEL"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// Anthropic API(`sandbox.env`の`ANTHROPIC_API_KEY`)。
    #[default]
    Anthropic,
    /// Amazon Bedrock(アプリの AWS 設定の一時キー)。
    Bedrock,
}

/// 接続の設定。未保存・欠けた項目は既定値になる。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub provider: Provider,
    /// モデル。空なら Claude Code の既定。Bedrock ではモデル ID か推論プロファイルを指定する。
    pub model: String,
    /// 1回の指示の上限時間(分)。
    pub timeout_mins: u32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self { provider: Provider::Anthropic, model: String::new(), timeout_mins: 30 }
    }
}

impl LlmConfig {
    /// 保存できる形に整える。範囲外の上限時間は丸め、不正なモデル名は拒否する。
    fn normalized(mut self) -> Result<Self, AppError> {
        self.model = self.model.trim().to_owned();
        if self.model.chars().count() > MODEL_MAX_CHARS || self.model.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(AppError::Sandbox("モデルの指定が正しくありません(空白や制御文字は使えません)。".into()));
        }
        self.timeout_mins = self.timeout_mins.clamp(*TIMEOUT_RANGE.start(), *TIMEOUT_RANGE.end());
        Ok(self)
    }
}

/// 保存された設定。読めなければ既定値。
pub fn load(db: &impl SettingsStore) -> Result<LlmConfig, AppError> {
    let cfg = db
        .get_setting(LLM_KEY)?
        .and_then(|v| serde_json::from_str::<LlmConfig>(&v).ok())
        .unwrap_or_default();
    Ok(cfg.normalized().unwrap_or_default())
}

pub fn save(db: &impl SettingsStore, cfg: LlmConfig) -> Result<LlmConfig, AppError> {
    let cfg = cfg.normalized()?;
    db.set_setting(LLM_KEY, &serde_json::to_string(&cfg).map_err(|e| AppError::Io(e.to_string()))?)?;
    Ok(cfg)
}

/// アプリが決める環境変数(秘密でない値だけ)。
pub fn app_env(cfg: &LlmConfig) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    if cfg.provider == Provider::Bedrock {
        vars.insert("CLAUDE_CODE_USE_BEDROCK".to_owned(), "1".to_owned());
    }
    if !cfg.model.is_empty() {
        vars.insert("ANTHROPIC_MODEL".to_owned(), cfg.model.clone());
    }
    vars
}

/// アプリが決める値の上に、認証情報(`sandbox.env`とAWSの一時キー)を重ねる。同じ名前は認証情報の側が優先。
pub fn merge_env(cfg: &LlmConfig, credentials: BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut vars = app_env(cfg);
    vars.extend(credentials);
    vars
}

fn has(vars: &BTreeMap<String, String>, name: &str) -> bool {
    vars.get(name).is_some_and(|v| !v.trim().is_empty())
}

/// 接続先に必要な認証情報がそろっているか。そろっていなければ、実行せずに利用者向けの文面で失敗する。
pub fn check_credentials(cfg: &LlmConfig, vars: &BTreeMap<String, String>) -> Result<(), String> {
    match cfg.provider {
        Provider::Anthropic if !has(vars, "ANTHROPIC_API_KEY") && !has(vars, "ANTHROPIC_AUTH_TOKEN") => {
            Err("Anthropic API のキーがありません。このエージェントの sandbox.env に ANTHROPIC_API_KEY を書いてください。".into())
        }
        Provider::Bedrock if !has(vars, "AWS_ACCESS_KEY_ID") || !has(vars, "AWS_SECRET_ACCESS_KEY") => {
            Err("Bedrock の認証情報がありません。このエージェントに AWS の設定を登録してください。".into())
        }
        Provider::Bedrock if !has(vars, "AWS_REGION") => {
            Err("Bedrock のリージョンがありません。AWS の設定のプロファイルに region を指定するか、sandbox.env に AWS_REGION を書いてください。".into())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    #[test]
    fn config_defaults_and_normalizes() {
        let cfg: LlmConfig = serde_json::from_str(r#"{"provider":"bedrock"}"#).unwrap();
        assert_eq!(cfg, LlmConfig { provider: Provider::Bedrock, model: String::new(), timeout_mins: 30 });
        let n = LlmConfig { timeout_mins: 9999, model: "  claude-x  ".into(), ..Default::default() }.normalized().unwrap();
        assert_eq!((n.timeout_mins, n.model.as_str()), (180, "claude-x"));
        assert_eq!(LlmConfig { timeout_mins: 0, ..Default::default() }.normalized().unwrap().timeout_mins, 1);
        assert!(LlmConfig { model: "a b".into(), ..Default::default() }.normalized().is_err());
        assert!(LlmConfig { model: "x\n".repeat(1) + "y", ..Default::default() }.normalized().is_err());
    }

    #[test]
    fn app_env_carries_only_non_secret_values() {
        let bedrock = LlmConfig { provider: Provider::Bedrock, model: "us.anthropic.claude-x".into(), ..Default::default() };
        let env = app_env(&bedrock);
        assert_eq!(env, vars(&[("CLAUDE_CODE_USE_BEDROCK", "1"), ("ANTHROPIC_MODEL", "us.anthropic.claude-x")]));
        assert!(env.keys().all(|k| NON_SECRET_NAMES.contains(&k.as_str())));
        assert!(app_env(&LlmConfig::default()).is_empty());
    }

    #[test]
    fn sandbox_env_overrides_app_values() {
        let cfg = LlmConfig { model: "from-app".into(), ..Default::default() };
        let merged = merge_env(&cfg, vars(&[("ANTHROPIC_MODEL", "from-env"), ("ANTHROPIC_API_KEY", "k")]));
        assert_eq!(merged.get("ANTHROPIC_MODEL").map(String::as_str), Some("from-env"));
    }

    #[test]
    fn credentials_are_required_per_provider() {
        let anthropic = LlmConfig::default();
        assert!(check_credentials(&anthropic, &vars(&[])).is_err());
        assert!(check_credentials(&anthropic, &vars(&[("ANTHROPIC_API_KEY", " ")])).is_err());
        assert!(check_credentials(&anthropic, &vars(&[("ANTHROPIC_API_KEY", "sk-x")])).is_ok());
        let bedrock = LlmConfig { provider: Provider::Bedrock, ..Default::default() };
        assert!(check_credentials(&bedrock, &vars(&[("ANTHROPIC_API_KEY", "sk-x")])).is_err());
        assert!(check_credentials(&bedrock, &vars(&[("AWS_ACCESS_KEY_ID", "a"), ("AWS_SECRET_ACCESS_KEY", "b")])).is_err());
        assert!(check_credentials(&bedrock, &vars(&[("AWS_ACCESS_KEY_ID", "a"), ("AWS_SECRET_ACCESS_KEY", "b"), ("AWS_REGION", "us-east-1")])).is_ok());
    }
}
