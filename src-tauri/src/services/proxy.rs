//! 通信のプロキシ設定。有効にすると、アプリの外向きの通信(LLM・音声・モデル取得・AWS)をプロキシ経由にする。
//!
//! - 有効/無効・URL・ユーザー名は全体設定(`global_settings.json`)、パスワードは専用の保管庫(`<アプリデータ>/proxy/secrets.json`)に置く。
//! - 無効にしても設定はすべて残す。再び有効にすれば、残した設定でそのまま使える。
//! - 使う設定はメモリに持ち(`reload`)、通信のたびに`client_builder` / `aws_http_client`が読む。保存すれば次の通信から反映される。
//! - `localhost` / `127.0.0.1` / `::1`宛て(VOICEVOX・ローカルLLM)は常にプロキシを通さない。
//! - パスワードの生値はこのモジュールと`secrets.rs`の外へ出さない。画面へは登録の有無だけを返す。
use std::{
    path::Path,
    sync::{OnceLock, RwLock},
};

use serde::Serialize;

use super::{
    global_settings::{GlobalStore, SettingsStore},
    secrets::Secrets,
};
use crate::utils::error::AppError;

pub const ENABLED_KEY: &str = "proxy_enabled";
pub const URL_KEY: &str = "proxy_url";
pub const USERNAME_KEY: &str = "proxy_username";

/// プロキシを通さない宛先。
const NO_PROXY: &str = "localhost,127.0.0.1,::1";

/// いま使っているプロキシ。無効、またはURLが不正なら`None`。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Active {
    url: String,
    auth: Option<(String, String)>,
}

static ACTIVE: RwLock<Option<Active>> = RwLock::new(None);
static SECRETS: OnceLock<Secrets> = OnceLock::new();

/// 画面へ返す設定。パスワードは返さず、登録の有無だけ。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProxySettings {
    pub enabled: bool,
    pub url: String,
    pub username: String,
    pub has_password: bool,
}

/// 起動時に呼ぶ。パスワードの保管庫を開き、保存済みの設定を使い始める。
pub fn init(root: &Path, global: &GlobalStore) -> Result<(), AppError> {
    let dir = root.join("proxy");
    std::fs::create_dir_all(&dir).map_err(|e| AppError::Io(e.to_string()))?;
    let _ = SECRETS.set(Secrets::open(&dir)?);
    reload(global)
}

/// パスワードの保管庫。`init`前は使えない。
pub fn secrets() -> Result<&'static Secrets, AppError> {
    SECRETS.get().ok_or_else(|| AppError::SecretStore("proxy store is not initialized".into()))
}

/// 保存済みの設定を読み直して、通信に使うプロキシを切り替える。
pub fn reload(global: &GlobalStore) -> Result<(), AppError> {
    let next = active_from(global, secrets()?)?;
    *ACTIVE.write().unwrap_or_else(|p| p.into_inner()) = next;
    Ok(())
}

fn active_from(global: &impl SettingsStore, secrets: &Secrets) -> Result<Option<Active>, AppError> {
    if global.get_setting(ENABLED_KEY)?.as_deref() != Some("1") {
        return Ok(None);
    }
    let Some(url) = global.get_setting(URL_KEY)?.filter(|u| validate_url(u).is_ok()) else {
        return Ok(None);
    };
    let username = global.get_setting(USERNAME_KEY)?.unwrap_or_default();
    let auth = if username.is_empty() {
        None
    } else {
        Some((username, secrets.get_proxy_password()?.unwrap_or_default()))
    };
    Ok(Some(Active { url, auth }))
}

/// 保存済みの設定(画面用)。
pub fn load(global: &impl SettingsStore, secrets: &Secrets) -> Result<ProxySettings, AppError> {
    Ok(ProxySettings {
        enabled: global.get_setting(ENABLED_KEY)?.as_deref() == Some("1"),
        url: global.get_setting(URL_KEY)?.unwrap_or_default(),
        username: global.get_setting(USERNAME_KEY)?.unwrap_or_default(),
        has_password: secrets.has_proxy_password()?,
    })
}

/// 設定を保存する。`password`は`None`なら据え置き、空文字なら削除、それ以外なら更新。
/// 無効のままでもURLなどは保存し、有効にしたときに使えるようにする。有効にするにはURLが要る。
pub fn save(
    global: &GlobalStore,
    secrets: &Secrets,
    enabled: bool,
    url: &str,
    username: &str,
    password: Option<&str>,
) -> Result<ProxySettings, AppError> {
    let url = url.trim();
    if !url.is_empty() {
        validate_url(url)?;
    } else if enabled {
        return Err(AppError::InvalidSettings("proxy url is required".into()));
    }
    match password {
        Some("") => secrets.delete_proxy_password()?,
        Some(p) => secrets.set_proxy_password(p)?,
        None => {}
    }
    global.set_settings(&[(ENABLED_KEY, if enabled { "1" } else { "0" }), (URL_KEY, url), (USERNAME_KEY, username.trim())])?;
    load(global, secrets)
}

/// プロキシのURLを確かめる。`http` / `https`で、ホストがあり、認証情報(ユーザー名・パスワード)を含まないもの。
fn validate_url(url: &str) -> Result<(), AppError> {
    let bad = || AppError::InvalidSettings("proxy url".into());
    let parsed = reqwest::Url::parse(url.trim()).map_err(|_| bad())?;
    let ok = matches!(parsed.scheme(), "http" | "https")
        && parsed.host_str().is_some_and(|h| !h.is_empty())
        && parsed.username().is_empty()
        && parsed.password().is_none();
    if ok { Ok(()) } else { Err(bad()) }
}

fn active() -> Option<Active> {
    ACTIVE.read().unwrap_or_else(|p| p.into_inner()).clone()
}

/// 外向きの通信に使う`reqwest`のビルダー。プロキシが有効ならそれを通す(ローカル宛ては除く)。
pub fn client_builder() -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder();
    let Some(a) = active() else { return builder };
    let Ok(mut proxy) = reqwest::Proxy::all(&a.url) else { return builder };
    proxy = proxy.no_proxy(reqwest::NoProxy::from_string(NO_PROXY));
    if let Some((user, pass)) = &a.auth {
        proxy = proxy.basic_auth(user, pass);
    }
    builder.proxy(proxy)
}

/// 指定したURLを取得する。
pub async fn get(url: &str) -> Result<reqwest::Response, AppError> {
    client_builder().build().map_err(|_| AppError::Network)?.get(url).send().await.map_err(AppError::from_reqwest)
}

/// AWS SDK用のHTTPクライアント。プロキシが無効なら`None`(SDKの既定のまま)。
pub fn aws_http_client() -> Option<aws_sdk_bedrockruntime::config::SharedHttpClient> {
    use aws_smithy_http_client::{Connector, proxy::ProxyConfig, tls};
    let a = active()?;
    let mut config = ProxyConfig::all(a.url.as_str()).ok()?.no_proxy(NO_PROXY);
    if let Some((user, pass)) = &a.auth {
        config = config.with_basic_auth(user.as_str(), pass.as_str());
    }
    Some(aws_smithy_http_client::Builder::new().build_with_connector_fn(move |settings, components| {
        let mut builder = Connector::builder()
            .proxy_config(config.clone())
            .tls_provider(tls::Provider::Rustls(tls::rustls_provider::CryptoMode::AwsLc));
        builder.set_connector_settings(settings.cloned());
        if let Some(c) = components {
            builder.set_sleep_impl(c.sleep_impl());
        }
        builder.build()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stores(dir: &Path) -> (GlobalStore, Secrets) {
        (GlobalStore::open(dir), Secrets::open(dir).unwrap())
    }

    #[test]
    fn url_must_be_http_without_credentials() {
        assert!(validate_url("http://proxy.example.com:8080").is_ok());
        assert!(validate_url("https://proxy.example.com").is_ok());
        for bad in ["", "proxy.example.com:8080", "socks5://p:1080", "ftp://p", "http://u:p@proxy:8080", "http://u@proxy:8080", "http://"] {
            assert!(validate_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn enabling_requires_a_url_but_disabled_may_be_empty() {
        let d = tempfile::tempdir().unwrap();
        let (g, s) = stores(d.path());
        assert!(save(&g, &s, true, "  ", "", None).is_err());
        assert!(save(&g, &s, false, "", "", None).is_ok());
        assert!(save(&g, &s, false, "socks5://x", "", None).is_err());
    }

    #[test]
    fn disabling_keeps_settings_and_reenabling_restores_them() {
        let d = tempfile::tempdir().unwrap();
        let (g, s) = stores(d.path());
        let on = save(&g, &s, true, "http://proxy:8080", "alice", Some("secret")).unwrap();
        assert_eq!(on, ProxySettings { enabled: true, url: "http://proxy:8080".into(), username: "alice".into(), has_password: true });
        let a = active_from(&g, &s).unwrap().unwrap();
        assert_eq!(a.auth, Some(("alice".into(), "secret".into())));

        // 無効にしても、使われなくなるだけで設定は残る(パスワードは据え置き)
        let off = save(&g, &s, false, "http://proxy:8080", "alice", None).unwrap();
        assert!(!off.enabled && off.has_password);
        assert_eq!(active_from(&g, &s).unwrap(), None);
        assert_eq!(load(&g, &s).unwrap().url, "http://proxy:8080");

        // 再び有効にすると、保持した設定で復活する
        save(&g, &s, true, "http://proxy:8080", "alice", None).unwrap();
        assert_eq!(active_from(&g, &s).unwrap(), Some(a));

        // 空文字でパスワードだけを消せる
        assert!(!save(&g, &s, true, "http://proxy:8080", "alice", Some("")).unwrap().has_password);
    }

    #[test]
    fn no_username_means_no_auth() {
        let d = tempfile::tempdir().unwrap();
        let (g, s) = stores(d.path());
        save(&g, &s, true, "http://proxy:8080", "", Some("ignored")).unwrap();
        assert_eq!(active_from(&g, &s).unwrap().unwrap().auth, None);
    }
}
