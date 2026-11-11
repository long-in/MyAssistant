//! 通信のプロキシ設定。複数のプロキシを登録でき、通信ごとに使うものを選べる。アプリの外向きの通信(LLM・音声・モデル取得・AWS)をプロキシ経由にする。
//!
//! - 一覧(ID・名前・URL・ユーザー名)と既定のIDは全体設定(`global_settings.json`)、パスワードは専用の保管庫(`<アプリデータ>/proxy/secrets.json`)にIDごとに置く。
//! - エージェントごとの選択(`proxy_choice`)はエージェントのDBに置く。空は「既定を使う」、`none`は「プロキシを使わない」、それ以外は登録済みのID。
//!   選んだIDが一覧に無いとき(削除・取り込み先に無い)は、既定として扱う。
//! - エージェントに紐づく通信(AI・音声・AWS)は`client_builder_for` / `aws_http_client_for`でそのエージェントの選択を使う。
//!   モデルのダウンロードやキー確認など、エージェントに紐づかない通信は`client_builder` / `get`で既定のプロキシを使う。
//! - 使う設定はメモリに持ち(`reload`)、通信のたびに読む。保存すれば次の通信から反映される。
//! - `localhost` / `127.0.0.1` / `::1`宛て(VOICEVOX・ローカルLLM)は常にプロキシを通さない。
//! - パスワードの生値はこのモジュールと`secrets.rs`の外へ出さない。画面へは登録の有無だけを返す。
//! - プロキシが1つだった頃の設定(`proxy_enabled` / `proxy_url` / `proxy_username`と単一のパスワード)は、起動時に1件目の「Default」へ移す(`migrate_legacy`)。
use std::{
    collections::HashMap,
    path::Path,
    sync::{Mutex, OnceLock, RwLock},
};

use serde::{Deserialize, Serialize};

use super::{
    global_settings::{GlobalStore, SettingsStore},
    secrets::Secrets,
    workspace::WorkspaceCtx,
};
use crate::utils::error::AppError;

/// プロキシの一覧(JSON配列)。
pub const LIST_KEY: &str = "proxy_list";
/// 既定のプロキシのID。空なら既定なし。
pub const DEFAULT_KEY: &str = "proxy_default";
/// エージェントごとの選択(エージェントのDB)。
pub const CHOICE_KEY: &str = "proxy_choice";
/// 選択の値: プロキシを使わない。
pub const NONE_CHOICE: &str = "none";

/// プロキシが1つだった頃のキー。移行のためにだけ読む。
pub const LEGACY_ENABLED_KEY: &str = "proxy_enabled";
pub const LEGACY_URL_KEY: &str = "proxy_url";
pub const LEGACY_USERNAME_KEY: &str = "proxy_username";

/// 一覧の名前の長さの上限(文字数)。
const MAX_NAME_CHARS: usize = 60;
/// プロキシを通さない宛先。
const NO_PROXY: &str = "localhost,127.0.0.1,::1";

/// 登録したプロキシ1件(パスワードを除く)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Entry {
    id: String,
    name: String,
    url: String,
    #[serde(default)]
    username: String,
}

/// 通信に使うプロキシ。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Active {
    url: String,
    auth: Option<(String, String)>,
}

/// メモリに持つ、通信に使えるプロキシの一覧と既定。
#[derive(Debug, Default)]
struct Registry {
    default: Option<String>,
    entries: HashMap<String, Active>,
}

impl Registry {
    /// 選択(空・`none`・ID)から、使うプロキシを決める。一覧に無いIDは既定として扱う。
    fn resolve(&self, choice: &str) -> Option<Active> {
        if choice == NONE_CHOICE {
            return None;
        }
        self.entries.get(choice).or_else(|| self.default.as_ref().and_then(|d| self.entries.get(d))).cloned()
    }
}

static REGISTRY: RwLock<Option<Registry>> = RwLock::new(None);
static SECRETS: OnceLock<Secrets> = OnceLock::new();
/// 一覧の読み取り・書き込みを直列にする。
static EDIT: Mutex<()> = Mutex::new(());

/// 画面へ返すプロキシ1件。パスワードは返さず、登録の有無だけ。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProxyView {
    pub id: String,
    pub name: String,
    pub url: String,
    pub username: String,
    pub has_password: bool,
}

/// 画面へ返す一覧と既定のID(なければ空)。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProxyList {
    pub proxies: Vec<ProxyView>,
    pub default_id: String,
}

/// 起動時に呼ぶ。パスワードの保管庫を開き、古い設定を移して、保存済みの設定を使い始める。
pub fn init(root: &Path, global: &GlobalStore) -> Result<(), AppError> {
    let dir = root.join("proxy");
    std::fs::create_dir_all(&dir).map_err(|e| AppError::Io(e.to_string()))?;
    let _ = SECRETS.set(Secrets::open(&dir)?);
    migrate_legacy(global, secrets()?)?;
    reload(global)
}

/// パスワードの保管庫。`init`前は使えない。
pub fn secrets() -> Result<&'static Secrets, AppError> {
    SECRETS.get().ok_or_else(|| AppError::SecretStore("proxy store is not initialized".into()))
}

/// 保存済みの設定を読み直して、通信に使うプロキシを切り替える。
pub fn reload(global: &impl SettingsStore) -> Result<(), AppError> {
    let next = registry_from(global, secrets()?)?;
    *REGISTRY.write().unwrap_or_else(|p| p.into_inner()) = Some(next);
    Ok(())
}

/// プロキシのIDとして使える文字だけか(`none`は選択の予約語なので除く)。
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id != NONE_CHOICE && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// 保存済みの一覧。壊れている・取り込みで不正になった件(ID・名前・URLが不正、IDが重複)は除く。
fn read_entries(global: &impl SettingsStore) -> Result<Vec<Entry>, AppError> {
    let Some(text) = global.get_setting(LIST_KEY)? else { return Ok(Vec::new()) };
    let parsed: Vec<Entry> = serde_json::from_str(&text).unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    Ok(parsed
        .into_iter()
        .filter(|e| valid_id(&e.id) && !e.name.trim().is_empty() && validate_url(&e.url).is_ok() && seen.insert(e.id.clone()))
        .collect())
}

fn write_entries(global: &GlobalStore, entries: &[Entry]) -> Result<(), AppError> {
    let json = serde_json::to_string(entries).map_err(|e| AppError::Io(e.to_string()))?;
    global.set_settings(&[(LIST_KEY, &json)])
}

fn default_id(global: &impl SettingsStore, entries: &[Entry]) -> Result<String, AppError> {
    Ok(global.get_setting(DEFAULT_KEY)?.filter(|d| entries.iter().any(|e| &e.id == d)).unwrap_or_default())
}

fn registry_from(global: &impl SettingsStore, secrets: &Secrets) -> Result<Registry, AppError> {
    let entries = read_entries(global)?;
    let default = Some(default_id(global, &entries)?).filter(|d| !d.is_empty());
    let mut map = HashMap::new();
    for e in entries {
        let auth = if e.username.is_empty() {
            None
        } else {
            Some((e.username.clone(), secrets.get_proxy_password(&e.id)?.unwrap_or_default()))
        };
        map.insert(e.id, Active { url: e.url, auth });
    }
    Ok(Registry { default, entries: map })
}

/// プロキシが1つだった頃の設定を、一覧の1件目(Default)へ移す。一覧が保存済みなら何もしない。
/// 有効だったものは既定にして、従来どおり全通信がプロキシ経由になるようにする。
fn migrate_legacy(global: &GlobalStore, secrets: &Secrets) -> Result<(), AppError> {
    if global.get_setting(LIST_KEY)?.is_some() {
        return Ok(());
    }
    let url = global.get_setting(LEGACY_URL_KEY)?.unwrap_or_default();
    if validate_url(&url).is_err() {
        return Ok(());
    }
    let enabled = global.get_setting(LEGACY_ENABLED_KEY)?.as_deref() == Some("1");
    let entry = Entry {
        id: new_id(),
        name: "Default".to_owned(),
        url,
        username: global.get_setting(LEGACY_USERNAME_KEY)?.unwrap_or_default(),
    };
    secrets.move_legacy_proxy_password(&entry.id)?;
    let json = serde_json::to_string(&[&entry]).map_err(|e| AppError::Io(e.to_string()))?;
    let default = if enabled { entry.id.as_str() } else { "" };
    global.set_settings(&[(LIST_KEY, &json), (DEFAULT_KEY, default)])
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// 一覧と既定(画面用)。
pub fn load(global: &impl SettingsStore, secrets: &Secrets) -> Result<ProxyList, AppError> {
    let entries = read_entries(global)?;
    let default_id = default_id(global, &entries)?;
    let proxies = entries
        .into_iter()
        .map(|e| {
            let has_password = secrets.has_proxy_password(&e.id)?;
            Ok(ProxyView { id: e.id, name: e.name, url: e.url, username: e.username, has_password })
        })
        .collect::<Result<_, AppError>>()?;
    Ok(ProxyList { proxies, default_id })
}

/// プロキシを追加(`id`が`None`)または更新する。`password`は`None`なら据え置き、空文字なら削除、それ以外なら更新。
pub fn save(
    global: &GlobalStore,
    secrets: &Secrets,
    id: Option<&str>,
    name: &str,
    url: &str,
    username: &str,
    password: Option<&str>,
) -> Result<ProxyList, AppError> {
    let _g = EDIT.lock().unwrap_or_else(|p| p.into_inner());
    let (name, url, username) = (name.trim(), url.trim(), username.trim());
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(AppError::InvalidSettings("proxy name".into()));
    }
    validate_url(url)?;
    let mut entries = read_entries(global)?;
    let entry_id = match id {
        Some(id) => {
            let Some(e) = entries.iter_mut().find(|e| e.id == id) else {
                return Err(AppError::InvalidSettings("proxy id".into()));
            };
            e.name = name.to_owned();
            e.url = url.to_owned();
            e.username = username.to_owned();
            id.to_owned()
        }
        None => {
            let id = new_id();
            entries.push(Entry { id: id.clone(), name: name.to_owned(), url: url.to_owned(), username: username.to_owned() });
            id
        }
    };
    match password {
        Some("") => secrets.delete_proxy_password(&entry_id)?,
        Some(p) => secrets.set_proxy_password(&entry_id, p)?,
        None => {}
    }
    write_entries(global, &entries)?;
    load(global, secrets)
}

/// プロキシを削除する(パスワードも消す)。既定だったときは既定なしにする。選んでいたエージェントは、既定として扱われる。
pub fn delete(global: &GlobalStore, secrets: &Secrets, id: &str) -> Result<ProxyList, AppError> {
    let _g = EDIT.lock().unwrap_or_else(|p| p.into_inner());
    let mut entries = read_entries(global)?;
    let before = entries.len();
    entries.retain(|e| e.id != id);
    if entries.len() == before {
        return Err(AppError::InvalidSettings("proxy id".into()));
    }
    secrets.delete_proxy_password(id)?;
    // 削除した件が既定だったときは、一覧に無いIDになるので既定なしになる
    let default = default_id(global, &entries)?;
    let json = serde_json::to_string(&entries).map_err(|e| AppError::Io(e.to_string()))?;
    global.set_settings(&[(LIST_KEY, &json), (DEFAULT_KEY, &default)])?;
    load(global, secrets)
}

/// 既定のプロキシを決める。空文字なら既定なし(エージェントが「既定を使う」なら、プロキシを使わない)。
pub fn set_default(global: &GlobalStore, secrets: &Secrets, id: &str) -> Result<ProxyList, AppError> {
    let _g = EDIT.lock().unwrap_or_else(|p| p.into_inner());
    if !id.is_empty() && !read_entries(global)?.iter().any(|e| e.id == id) {
        return Err(AppError::InvalidSettings("proxy id".into()));
    }
    global.set_settings(&[(DEFAULT_KEY, id)])?;
    load(global, secrets)
}

/// エージェントの選択として保存できる値か(空・`none`・登録済みのID)。
pub fn validate_choice(global: &impl SettingsStore, choice: &str) -> Result<(), AppError> {
    if choice.is_empty() || choice == NONE_CHOICE || read_entries(global)?.iter().any(|e| e.id == choice) {
        Ok(())
    } else {
        Err(AppError::InvalidSettings("proxy choice".into()))
    }
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

/// 選択(空・`none`・ID)に対して使うプロキシ。
fn active_for(choice: &str) -> Option<Active> {
    REGISTRY.read().unwrap_or_else(|p| p.into_inner()).as_ref()?.resolve(choice)
}

/// そのエージェントの選択。読めなければ(未設定を含め)既定として扱う。
fn choice_of(ctx: &WorkspaceCtx) -> String {
    ctx.db.get_setting(CHOICE_KEY).ok().flatten().unwrap_or_default()
}

fn builder_with(active: Option<Active>) -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder();
    let Some(a) = active else { return builder };
    let Ok(mut proxy) = reqwest::Proxy::all(&a.url) else { return builder };
    proxy = proxy.no_proxy(reqwest::NoProxy::from_string(NO_PROXY));
    if let Some((user, pass)) = &a.auth {
        proxy = proxy.basic_auth(user, pass);
    }
    builder.proxy(proxy)
}

/// エージェントに紐づかない通信(モデルの取得・キーの確認)に使う`reqwest`のビルダー。既定のプロキシがあれば通す(ローカル宛ては除く)。
pub fn client_builder() -> reqwest::ClientBuilder {
    builder_with(active_for(""))
}

/// そのエージェントの通信に使う`reqwest`のビルダー。エージェントが選んだプロキシ(「既定を使う」なら既定)を通す。
pub fn client_builder_for(ctx: &WorkspaceCtx) -> reqwest::ClientBuilder {
    builder_with(active_for(&choice_of(ctx)))
}

/// 指定したURLを取得する(エージェントに紐づかない通信)。
pub async fn get(url: &str) -> Result<reqwest::Response, AppError> {
    client_builder().build().map_err(|_| AppError::Network)?.get(url).send().await.map_err(AppError::from_reqwest)
}

/// そのエージェントのAWS SDK用のHTTPクライアント。プロキシを使わないなら`None`(SDKの既定のまま)。
pub fn aws_http_client_for(ctx: &WorkspaceCtx) -> Option<aws_sdk_sts::config::SharedHttpClient> {
    use aws_smithy_http_client::{Connector, proxy::ProxyConfig, tls};
    let a = active_for(&choice_of(ctx))?;
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

    fn add(g: &GlobalStore, s: &Secrets, name: &str, url: &str, user: &str, pw: Option<&str>) -> String {
        let list = save(g, s, None, name, url, user, pw).unwrap();
        list.proxies.last().unwrap().id.clone()
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
    fn name_and_url_are_required() {
        let d = tempfile::tempdir().unwrap();
        let (g, s) = stores(d.path());
        assert!(save(&g, &s, None, "  ", "http://p:1", "", None).is_err());
        assert!(save(&g, &s, None, "a", "", "", None).is_err());
        assert!(save(&g, &s, None, "a", "socks5://x", "", None).is_err());
        assert!(save(&g, &s, None, &"a".repeat(61), "http://p:1", "", None).is_err());
        assert!(save(&g, &s, Some("missing"), "a", "http://p:1", "", None).is_err());
    }

    #[test]
    fn several_proxies_can_be_registered_and_edited() {
        let d = tempfile::tempdir().unwrap();
        let (g, s) = stores(d.path());
        let a = add(&g, &s, "Office", "http://office:8080", "alice", Some("secret"));
        let b = add(&g, &s, "Home", "https://home:3128", "", None);
        let list = load(&g, &s).unwrap();
        assert_eq!(list.proxies.len(), 2);
        assert!(list.proxies[0].has_password && !list.proxies[1].has_password);
        assert_eq!(list.default_id, "");

        // 更新: パスワードは据え置き(None)・更新・削除(空文字)を区別する
        let list = save(&g, &s, Some(&a), "Office2", "http://office:9090", "alice", None).unwrap();
        assert_eq!((list.proxies[0].name.as_str(), list.proxies[0].url.as_str(), list.proxies[0].has_password), ("Office2", "http://office:9090", true));
        assert!(!save(&g, &s, Some(&a), "Office2", "http://office:9090", "alice", Some("")).unwrap().proxies[0].has_password);
        assert_eq!(list.proxies[1].id, b);
    }

    #[test]
    fn default_and_choice_resolve_to_the_right_proxy() {
        let d = tempfile::tempdir().unwrap();
        let (g, s) = stores(d.path());
        let a = add(&g, &s, "A", "http://a:1", "u", Some("pw"));
        let b = add(&g, &s, "B", "http://b:2", "", None);
        assert!(set_default(&g, &s, "nope").is_err());
        assert_eq!(set_default(&g, &s, &a).unwrap().default_id, a);

        let reg = registry_from(&g, &s).unwrap();
        assert_eq!(reg.resolve("").unwrap().url, "http://a:1"); // 既定を使う
        assert_eq!(reg.resolve("").unwrap().auth, Some(("u".into(), "pw".into())));
        assert_eq!(reg.resolve(&b).unwrap().url, "http://b:2"); // 個別の選択
        assert_eq!(reg.resolve(&b).unwrap().auth, None);
        assert_eq!(reg.resolve(NONE_CHOICE), None); // 使わない
        assert_eq!(reg.resolve("gone").unwrap().url, "http://a:1"); // 一覧に無いIDは既定

        // 既定なしなら、「既定を使う」はプロキシなし。個別の選択は効く
        set_default(&g, &s, "").unwrap();
        let reg = registry_from(&g, &s).unwrap();
        assert_eq!(reg.resolve(""), None);
        assert!(reg.resolve(&b).is_some());
    }

    #[test]
    fn deleting_clears_password_default_and_falls_back() {
        let d = tempfile::tempdir().unwrap();
        let (g, s) = stores(d.path());
        let a = add(&g, &s, "A", "http://a:1", "u", Some("pw"));
        let b = add(&g, &s, "B", "http://b:2", "", None);
        set_default(&g, &s, &a).unwrap();
        let list = delete(&g, &s, &a).unwrap();
        assert_eq!((list.proxies.len(), list.default_id.as_str()), (1, ""));
        assert!(!s.has_proxy_password(&a).unwrap());
        assert!(delete(&g, &s, &a).is_err());
        // 削除したIDを選んでいたエージェントは、既定(いまは無し)として扱われる
        assert_eq!(registry_from(&g, &s).unwrap().resolve(&a), None);
        assert!(registry_from(&g, &s).unwrap().resolve(&b).is_some());
    }

    #[test]
    fn choice_validation() {
        let d = tempfile::tempdir().unwrap();
        let (g, s) = stores(d.path());
        let a = add(&g, &s, "A", "http://a:1", "", None);
        for ok in ["", NONE_CHOICE, a.as_str()] {
            assert!(validate_choice(&g, ok).is_ok(), "{ok}");
        }
        assert!(validate_choice(&g, "other").is_err());
    }

    #[test]
    fn no_username_means_no_auth() {
        let d = tempfile::tempdir().unwrap();
        let (g, s) = stores(d.path());
        let a = add(&g, &s, "A", "http://a:1", "", Some("ignored"));
        set_default(&g, &s, &a).unwrap();
        assert_eq!(registry_from(&g, &s).unwrap().resolve("").unwrap().auth, None);
    }

    #[test]
    fn invalid_imported_entries_are_ignored() {
        let d = tempfile::tempdir().unwrap();
        let (g, s) = stores(d.path());
        let json = r#"[{"id":"ok","name":"A","url":"http://a:1"},{"id":"ok","name":"dup","url":"http://d:1"},{"id":"none","name":"x","url":"http://x:1"},{"id":"bad url","name":"x","url":"http://x:1"},{"id":"b","name":"B","url":"socks5://b"},{"id":"c","name":" ","url":"http://c:1"}]"#;
        g.set_settings(&[(LIST_KEY, json), (DEFAULT_KEY, "missing")]).unwrap();
        let list = load(&g, &s).unwrap();
        assert_eq!(list.proxies.len(), 1);
        assert_eq!(list.proxies[0].id, "ok");
        assert_eq!(list.default_id, "");
        g.set_settings(&[(LIST_KEY, "not json")]).unwrap();
        assert!(load(&g, &s).unwrap().proxies.is_empty());
    }

    #[test]
    fn legacy_settings_move_to_the_first_entry() {
        // 有効だった: 既定にして、パスワードも移る
        let d = tempfile::tempdir().unwrap();
        let (g, s) = stores(d.path());
        g.set_settings(&[(LEGACY_ENABLED_KEY, "1"), (LEGACY_URL_KEY, "http://old:8080"), (LEGACY_USERNAME_KEY, "alice")]).unwrap();
        s.set_legacy_proxy_password_for_test("pw");
        migrate_legacy(&g, &s).unwrap();
        let list = load(&g, &s).unwrap();
        assert_eq!((list.proxies.len(), list.proxies[0].name.as_str(), list.proxies[0].username.as_str()), (1, "Default", "alice"));
        assert!(list.proxies[0].has_password);
        assert_eq!(list.default_id, list.proxies[0].id);
        assert_eq!(registry_from(&g, &s).unwrap().resolve("").unwrap().auth, Some(("alice".into(), "pw".into())));
        // 2回目は何もしない(削除しても復活しない)
        delete(&g, &s, &list.proxies[0].id).unwrap();
        migrate_legacy(&g, &s).unwrap();
        assert!(load(&g, &s).unwrap().proxies.is_empty());

        // 無効だった: 1件目として残すが、既定にはしない
        let d = tempfile::tempdir().unwrap();
        let (g, s) = stores(d.path());
        g.set_settings(&[(LEGACY_ENABLED_KEY, "0"), (LEGACY_URL_KEY, "http://old:8080")]).unwrap();
        migrate_legacy(&g, &s).unwrap();
        let list = load(&g, &s).unwrap();
        assert_eq!((list.proxies.len(), list.default_id.as_str()), (1, ""));

        // 古い設定が無い(またはURLが不正): 何も作らない
        let d = tempfile::tempdir().unwrap();
        let (g, s) = stores(d.path());
        migrate_legacy(&g, &s).unwrap();
        assert!(load(&g, &s).unwrap().proxies.is_empty());
        assert!(g.get_setting(LIST_KEY).unwrap().is_none());
    }
}
