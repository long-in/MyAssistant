//! モデルカタログと、ユーザーが手動で選んだモデルの検証。
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::{aws, secrets::Provider};
use crate::utils::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Light,
    Heavy,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub provider: Provider,
    pub id: String,
    pub label: String,
    pub tier: Tier,
}

/// 固定のモデル。Bedrockは利用者が登録したIDを使う(`catalog`)。モデルの入れ替えはここだけ編集する。
const FIXED: &[(Provider, &str, &str, Tier)] = &[
    (Provider::Anthropic, "claude-haiku-5-5", "Claude Haiku 5.5", Tier::Light),
    (Provider::Anthropic, "claude-sonnet-5-5", "Claude Sonnet 5.5", Tier::Heavy),
    (Provider::Anthropic, "claude-opus-5-5", "Claude Opus 5.5", Tier::Heavy),
];

/// 利用可能モデル。固定のモデルに、登録済みのBedrockモデルを足したもの。
pub fn catalog() -> Vec<ModelInfo> {
    catalog_with(&aws::models())
}

fn catalog_with(bedrock: &[String]) -> Vec<ModelInfo> {
    let info = |provider, id: &str, label: &str, tier| ModelInfo { provider, id: id.to_owned(), label: label.to_owned(), tier };
    FIXED
        .iter()
        .map(|&(p, id, label, tier)| info(p, id, label, tier))
        .chain(bedrock.iter().map(|id| info(Provider::Bedrock, id, id, Tier::Heavy)))
        .collect()
}

/// ユーザーが手動で選んだプロバイダーとモデル。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Route {
    pub provider: String,
    pub model: String,
}

pub struct Resolved {
    pub provider: Provider,
    pub model: String,
}

/// `available`はAPIキー登録済みのプロバイダー。
pub fn resolve(route: &Route, available: &[Provider]) -> Result<Resolved, AppError> {
    resolve_in(&catalog(), route, available)
}

fn resolve_in(catalog: &[ModelInfo], route: &Route, available: &[Provider]) -> Result<Resolved, AppError> {
    let provider = Provider::from_str(&route.provider)?;
    let m = catalog
        .iter()
        .find(|m| m.provider == provider && m.id == route.model)
        .ok_or_else(|| AppError::InvalidModel(route.model.clone()))?;
    if !available.contains(&provider) {
        return Err(AppError::KeyMissing(provider.as_str().to_owned()));
    }
    Ok(Resolved { provider, model: m.id.clone() })
}

/// 保存済みの選択が今も使えればそれを、なければ既定を返す。使えるプロバイダーが無ければ`None`。
pub fn effective(saved: Option<Route>, available: &[Provider]) -> Option<Route> {
    effective_in(&catalog(), saved, available)
}

/// 既定はClaudeのSonnet(Anthropic直接を優先、次にBedrockで名前にsonnetを含むもの)、
/// それが無ければ使えるプロバイダーのカタログ先頭。
fn effective_in(catalog: &[ModelInfo], saved: Option<Route>, available: &[Provider]) -> Option<Route> {
    if let Some(r) = saved.filter(|r| resolve_in(catalog, r, available).is_ok()) {
        return Some(r);
    }
    let usable = |m: &&ModelInfo| available.contains(&m.provider);
    let m = catalog
        .iter()
        .filter(usable)
        .find(|m| m.id.contains("sonnet"))
        .or_else(|| catalog.iter().find(usable))?;
    Some(Route { provider: m.provider.as_str().into(), model: m.id.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(p: &str, m: &str) -> Route {
        Route { provider: p.into(), model: m.into() }
    }

    fn bedrock() -> Vec<String> {
        vec!["us.anthropic.claude-haiku-5-5-v1:0".into(), "us.anthropic.claude-sonnet-5-5-v1:0".into()]
    }

    #[test]
    fn effective_keeps_saved_and_falls_back() {
        let c = catalog_with(&bedrock());
        let saved = route("anthropic", "claude-opus-5-5");
        let all = [Provider::Anthropic, Provider::Bedrock];
        assert_eq!(effective_in(&c, Some(saved.clone()), &all), Some(saved.clone()));
        assert_eq!(effective_in(&c, None, &all), Some(route("anthropic", "claude-sonnet-5-5")));
        // 保存済みのプロバイダーが使えなくなったら既定に戻る
        assert_eq!(
            effective_in(&c, Some(saved), &[Provider::Bedrock]),
            Some(route("bedrock", "us.anthropic.claude-sonnet-5-5-v1:0"))
        );
        // 廃止したプロバイダーの選択も既定に戻る
        assert_eq!(
            effective_in(&c, Some(route("openai", "gpt-4o")), &[Provider::Anthropic]),
            Some(route("anthropic", "claude-sonnet-5-5"))
        );
        assert_eq!(effective_in(&c, None, &[]), None);
    }

    #[test]
    fn bedrock_defaults_to_a_sonnet_model() {
        let c = catalog_with(&bedrock());
        let sonnet = route("bedrock", "us.anthropic.claude-sonnet-5-5-v1:0");
        assert_eq!(effective_in(&c, None, &[Provider::Bedrock]), Some(sonnet.clone()));
        // sonnetが無ければ登録済みの先頭
        let only_haiku = catalog_with(&["x.haiku".to_owned()]);
        assert_eq!(effective_in(&only_haiku, None, &[Provider::Bedrock]), Some(route("bedrock", "x.haiku")));
        // モデルが1つも登録されていないBedrockは選べない
        assert_eq!(effective_in(&catalog_with(&[]), None, &[Provider::Bedrock]), None);
    }

    #[test]
    fn manual_validates_model_and_key() {
        let c = catalog_with(&bedrock());
        let r = route("anthropic", "claude-opus-5-5");
        assert_eq!(resolve_in(&c, &r, &[Provider::Anthropic]).unwrap().model, "claude-opus-5-5");
        assert!(matches!(resolve_in(&c, &r, &[Provider::Bedrock]), Err(AppError::KeyMissing(_))));
        assert!(matches!(resolve_in(&c, &route("anthropic", "nope"), &[Provider::Anthropic]), Err(AppError::InvalidModel(_))));
        assert!(matches!(resolve_in(&c, &route("openai", "gpt-4o"), &[Provider::Anthropic]), Err(AppError::InvalidProvider(_))));
        // Bedrockは登録済みのIDだけ
        let b = route("bedrock", "us.anthropic.claude-haiku-5-5-v1:0");
        assert!(resolve_in(&c, &b, &[Provider::Bedrock]).is_ok());
        assert!(matches!(resolve_in(&c, &route("bedrock", "other"), &[Provider::Bedrock]), Err(AppError::InvalidModel(_))));
    }
}
