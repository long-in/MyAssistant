//! モデルカタログと、ユーザーが手動で選んだモデルの検証。
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::{
    local_llm,
    secrets::Provider,
    workspace::{LEADER_ID, PERSONAL_ID, WorkspaceCtx},
};
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

/// コンテナ内の Claude Code を使う経路の`Route.provider`。Bedrockは、このアプリから直接は呼ばず、この経路で使う。
pub const SANDBOX_PROVIDER: &str = "sandbox";

/// コンテナ内の Claude Code の経路か。`Provider`では表せないので、`resolve`などの前に確かめる。
pub fn is_sandbox(route: &Route) -> bool {
    route.provider == SANDBOX_PROVIDER
}

/// コンテナの経路で、モデルを指定しない(settings.json の`model`に任せる)ときのモデル名。
pub const SANDBOX_DEFAULT_MODEL: &str = "default";

/// コンテナの経路。`model`は settings.json で許可したモデル名。空なら`SANDBOX_DEFAULT_MODEL`。
pub fn sandbox_route(model: &str) -> Route {
    let model = if model.is_empty() { SANDBOX_DEFAULT_MODEL } else { model };
    Route { provider: SANDBOX_PROVIDER.into(), model: model.to_owned() }
}

/// コンテナ内の Claude Code を、モデルの選択肢として選べるエージェントか。
/// ワークは常にコンテナで動くので選ぶものではなく、個人とリーダーが、Anthropic・端末内のモデルと並べて選ぶ。
pub fn offers_sandbox(ctx: &WorkspaceCtx) -> bool {
    ctx.id == PERSONAL_ID || ctx.id == LEADER_ID
}

/// 固定のモデル。モデルの入れ替えはここだけ編集する。
const FIXED: &[(Provider, &str, &str, Tier)] = &[
    (Provider::Anthropic, "claude-haiku-5-5", "Claude Haiku 5.5", Tier::Light),
    (Provider::Anthropic, "claude-sonnet-5-5", "Claude Sonnet 5.5", Tier::Heavy),
    (Provider::Anthropic, "claude-opus-5-5", "Claude Opus 5.5", Tier::Heavy),
];

/// 利用可能モデル。固定のモデルに、取得済みならローカルのモデルを足したもの。
pub fn catalog(_ctx: &WorkspaceCtx) -> Vec<ModelInfo> {
    catalog_with(local_llm::installed())
}

/// ローカルは末尾に置く。既定(`effective_in`の先頭へのフォールバック)ではClaudeを優先するため。
fn catalog_with(local: bool) -> Vec<ModelInfo> {
    let info = |provider, id: &str, label: &str, tier| ModelInfo { provider, id: id.to_owned(), label: label.to_owned(), tier };
    FIXED
        .iter()
        .map(|&(p, id, label, tier)| info(p, id, label, tier))
        .chain(local.then(|| info(Provider::Local, local_llm::MODEL_ID, local_llm::MODEL_LABEL, Tier::Heavy)))
        .collect()
}

/// 画面へ返すモデルの選択肢。`provider`は、`Provider`の名前か、コンテナの経路(`sandbox`)。
#[derive(Debug, Clone, Serialize)]
pub struct ModelEntry {
    pub provider: String,
    pub id: String,
    pub label: String,
    pub tier: Tier,
}

/// 画面に出すモデルの一覧。個人・リーダーには、settings.json で許可したモデルごとにコンテナ内の Claude Code を足す
/// (許可の指定が無ければ、settings.json の`model`に任せる1件)。
pub fn entries(ctx: &WorkspaceCtx, sandbox_models: &[String]) -> Vec<ModelEntry> {
    let mut list: Vec<ModelEntry> = catalog(ctx)
        .into_iter()
        .map(|m| ModelEntry { provider: m.provider.as_str().to_owned(), id: m.id, label: m.label, tier: m.tier })
        .collect();
    if offers_sandbox(ctx) {
        if sandbox_models.is_empty() {
            list.push(ModelEntry { provider: SANDBOX_PROVIDER.into(), id: SANDBOX_DEFAULT_MODEL.into(), label: "settings.json".into(), tier: Tier::Heavy });
        } else {
            list.extend(sandbox_models.iter().map(|id| ModelEntry { provider: SANDBOX_PROVIDER.into(), id: id.clone(), label: id.clone(), tier: Tier::Heavy }));
        }
    }
    list
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
pub fn resolve(ctx: &WorkspaceCtx, route: &Route, available: &[Provider]) -> Result<Resolved, AppError> {
    resolve_in(&catalog(ctx), route, available)
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
/// 個人・リーダーは、コンテナの経路も選べる。以前のBedrockの選択は、コンテナの経路に読み替える。
/// 他に使えるものが無くてもAWSを登録していれば、コンテナの経路を既定にする(以前のBedrockだけの利用者のため)。
pub fn effective(ctx: &WorkspaceCtx, saved: Option<Route>, available: &[Provider]) -> Option<Route> {
    if !offers_sandbox(ctx) {
        return effective_in(&catalog(ctx), saved, available);
    }
    let saved = saved.map(|r| if r.provider == Provider::Bedrock.as_str() { sandbox_route("") } else { r });
    if saved.as_ref().is_some_and(is_sandbox) {
        return saved;
    }
    effective_in(&catalog(ctx), saved, available).or_else(|| ctx.secrets.has_aws().unwrap_or(false).then(|| sandbox_route("")))
}

/// 既定はClaudeのSonnet(名前にsonnetを含むもの)、
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

    #[test]
    fn local_is_listed_only_when_installed_and_never_preferred() {
        assert!(!catalog_with(false).iter().any(|m| m.provider == Provider::Local));
        let c = catalog_with(true);
        let local = route("local", local_llm::MODEL_ID);
        assert_eq!(c.last().unwrap().provider, Provider::Local);
        // Claudeが使えれば既定はClaude、ローカルしか無ければローカル
        let all = [Provider::Anthropic, Provider::Local];
        assert_eq!(effective_in(&c, None, &all), Some(route("anthropic", "claude-sonnet-5-5")));
        assert_eq!(effective_in(&c, None, &[Provider::Local]), Some(local.clone()));
        // 選んだローカルは保たれ、取得済みでなくなれば既定へ戻る
        assert_eq!(effective_in(&c, Some(local.clone()), &all), Some(local.clone()));
        let without = catalog_with(false);
        assert_eq!(effective_in(&without, Some(local), &[Provider::Anthropic]), Some(route("anthropic", "claude-sonnet-5-5")));
    }

    #[test]
    fn effective_keeps_saved_and_falls_back() {
        let c = catalog_with(false);
        let saved = route("anthropic", "claude-opus-5-5");
        let all = [Provider::Anthropic];
        assert_eq!(effective_in(&c, Some(saved.clone()), &all), Some(saved.clone()));
        assert_eq!(effective_in(&c, None, &all), Some(route("anthropic", "claude-sonnet-5-5")));
        // 保存済みのプロバイダーが使えなくなったら既定に戻る(使えるものが無ければ`None`)
        assert_eq!(effective_in(&c, Some(saved), &[Provider::Local]), None);
        // 廃止したプロバイダーの選択も既定に戻る
        assert_eq!(
            effective_in(&c, Some(route("openai", "gpt-4o")), &[Provider::Anthropic]),
            Some(route("anthropic", "claude-sonnet-5-5"))
        );
        // Bedrockは直接呼ばないので、カタログに無く選べない
        assert_eq!(effective_in(&c, Some(route("bedrock", "us.anthropic.claude-sonnet-5-5-v1:0")), &[Provider::Bedrock]), None);
        assert_eq!(effective_in(&c, None, &[]), None);
    }

    #[test]
    fn manual_validates_model_and_key() {
        let c = catalog_with(false);
        let r = route("anthropic", "claude-opus-5-5");
        assert_eq!(resolve_in(&c, &r, &[Provider::Anthropic]).unwrap().model, "claude-opus-5-5");
        assert!(matches!(resolve_in(&c, &r, &[Provider::Local]), Err(AppError::KeyMissing(_))));
        assert!(matches!(resolve_in(&c, &route("anthropic", "nope"), &[Provider::Anthropic]), Err(AppError::InvalidModel(_))));
        assert!(matches!(resolve_in(&c, &route("openai", "gpt-4o"), &[Provider::Anthropic]), Err(AppError::InvalidProvider(_))));
        // コンテナの経路は`Provider`ではないので、`resolve`では解決せず、呼び出し側が`is_sandbox`で分ける
        assert!(is_sandbox(&sandbox_route("")));
        assert!(!is_sandbox(&r));
        assert!(matches!(resolve_in(&c, &sandbox_route(""), &[Provider::Anthropic]), Err(AppError::InvalidProvider(_))));
    }

    #[test]
    fn personal_and_leader_can_choose_the_sandbox_route() {
        let (dp, dw) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let personal = WorkspaceCtx::open(PERSONAL_ID, dp.path()).unwrap();
        let work = WorkspaceCtx::open("0b9c3f5e-1d2a-4c6b-8e7f-123456789abc", dw.path()).unwrap();
        // 選択肢: 個人・リーダーだけに、settings.json のモデルごとのコンテナの項目が付く
        let list = entries(&personal, &["m1".to_owned()]);
        assert!(list.iter().any(|m| m.provider == SANDBOX_PROVIDER && m.id == "m1"));
        assert!(entries(&personal, &[]).iter().any(|m| m.provider == SANDBOX_PROVIDER && m.id == SANDBOX_DEFAULT_MODEL));
        assert!(!entries(&work, &["m1".to_owned()]).iter().any(|m| m.provider == SANDBOX_PROVIDER));
        // 選んだコンテナの経路は保たれ、以前のBedrockの選択はコンテナの経路に読み替える
        let saved = sandbox_route("m1");
        assert_eq!(effective(&personal, Some(saved.clone()), &[]), Some(saved.clone()));
        assert_eq!(effective(&personal, Some(route("bedrock", "x")), &[]), Some(sandbox_route("")));
        // 他に使えるものが無ければ、AWSを登録しているときだけコンテナの経路が既定になる
        assert_eq!(effective(&personal, None, &[]), None);
        personal
            .secrets
            .set_aws_files("[default]\nregion = us-east-1", "[default]\naws_access_key_id = A\naws_secret_access_key = B")
            .unwrap();
        assert_eq!(effective(&personal, None, &[]), Some(sandbox_route("")));
        // Claudeが使えれば、既定はそちら
        assert_eq!(effective(&personal, None, &[Provider::Anthropic]), Some(route("anthropic", "claude-sonnet-5-5")));
        // ワークは選ぶものではない
        assert_eq!(effective(&work, Some(saved), &[Provider::Anthropic]), Some(route("anthropic", "claude-sonnet-5-5")));
    }
}
