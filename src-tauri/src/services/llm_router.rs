//! モデルカタログと、ユーザーが手動で選んだモデルの検証。
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::secrets::Provider;
use crate::utils::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Light,
    Heavy,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ModelInfo {
    pub provider: Provider,
    pub id: &'static str,
    pub label: &'static str,
    pub tier: Tier,
}

/// 利用可能モデル。モデルの入れ替えはここだけ編集する。
pub const CATALOG: &[ModelInfo] = &[
    ModelInfo { provider: Provider::Anthropic, id: "claude-haiku-5-5", label: "Claude Haiku 5.5", tier: Tier::Light },
    ModelInfo { provider: Provider::Anthropic, id: "claude-sonnet-5-5", label: "Claude Sonnet 5.5", tier: Tier::Heavy },
    ModelInfo { provider: Provider::Anthropic, id: "claude-opus-5-5", label: "Claude Opus 5.5", tier: Tier::Heavy },
    ModelInfo { provider: Provider::Openai, id: "gpt-4o-mini", label: "GPT-4o mini", tier: Tier::Light },
    ModelInfo { provider: Provider::Openai, id: "gpt-4o", label: "GPT-4o", tier: Tier::Heavy },
];

/// ユーザーが手動で選んだプロバイダーとモデル。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Route {
    pub provider: String,
    pub model: String,
}

pub struct Resolved {
    pub provider: Provider,
    pub model: &'static str,
}

/// `available`はAPIキー登録済みのプロバイダー。
pub fn resolve(route: &Route, available: &[Provider]) -> Result<Resolved, AppError> {
    let provider = Provider::from_str(&route.provider)?;
    let m = CATALOG
        .iter()
        .find(|m| m.provider == provider && m.id == route.model)
        .ok_or_else(|| AppError::InvalidModel(route.model.clone()))?;
    if !available.contains(&provider) {
        return Err(AppError::KeyMissing(provider.as_str().to_owned()));
    }
    Ok(Resolved { provider, model: m.id })
}

/// 保存済みの選択が今も使えればそれを、なければ既定を返す。使えるプロバイダーが無ければ`None`。
/// 既定はClaudeが使えればSonnet、それ以外は使えるプロバイダーのカタログ先頭。
pub fn effective(saved: Option<Route>, available: &[Provider]) -> Option<Route> {
    if let Some(r) = saved.filter(|r| resolve(r, available).is_ok()) {
        return Some(r);
    }
    let m = if available.contains(&Provider::Anthropic) {
        CATALOG.iter().find(|m| m.id == "claude-sonnet-5-5")
    } else {
        CATALOG.iter().find(|m| available.contains(&m.provider))
    }?;
    Some(Route { provider: m.provider.as_str().into(), model: m.id.into() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_keeps_saved_and_falls_back() {
        let route = |p: &str, m: &str| Route { provider: p.into(), model: m.into() };
        let saved = route("openai", "gpt-4o");
        let all = [Provider::Anthropic, Provider::Openai];
        assert_eq!(effective(Some(saved.clone()), &all), Some(saved.clone()));
        assert_eq!(effective(None, &all), Some(route("anthropic", "claude-sonnet-5-5")));
        assert_eq!(effective(Some(saved), &[Provider::Anthropic]), Some(route("anthropic", "claude-sonnet-5-5")));
        assert_eq!(effective(None, &[]), None);
    }

    #[test]
    fn manual_validates_model_and_key() {
        let route = Route { provider: "openai".into(), model: "gpt-4o".into() };
        assert_eq!(resolve(&route, &[Provider::Openai]).unwrap().model, "gpt-4o");
        assert!(matches!(resolve(&route, &[Provider::Anthropic]), Err(AppError::KeyMissing(_))));
        let bad = Route { provider: "openai".into(), model: "nope".into() };
        assert!(matches!(resolve(&bad, &[Provider::Openai]), Err(AppError::InvalidModel(_))));
        let wrong = Route { provider: "anthropic".into(), model: "gpt-4o".into() };
        assert!(matches!(resolve(&wrong, &[Provider::Anthropic]), Err(AppError::InvalidModel(_))));
    }
}
