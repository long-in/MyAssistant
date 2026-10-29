//! APIキーの保存。アプリデータ配下の`secrets.json`(所有者のみ読み書き可)に保持する。
//! キーの生値はこのモジュールの外へ出さない。
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::utils::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Anthropic,
    Openai,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Anthropic, Provider::Openai];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::Openai => "openai",
        }
    }
}

impl FromStr for Provider {
    type Err = AppError;
    fn from_str(s: &str) -> Result<Self, AppError> {
        Self::ALL
            .into_iter()
            .find(|p| p.as_str() == s)
            .ok_or_else(|| AppError::InvalidProvider(s.to_owned()))
    }
}

/// Google Cloud TTS用のキー。
const TTS_ACCOUNT: &str = "google_tts";

/// Aqua Voice API用のキー。
const AQUA_ACCOUNT: &str = "aqua";

struct Store {
    path: PathBuf,
    map: Mutex<HashMap<String, String>>,
}

impl Store {
    fn open(path: PathBuf) -> Result<Self, AppError> {
        let map = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| AppError::SecretStore(e.to_string()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(e) => return Err(AppError::SecretStore(e.to_string())),
        };
        Ok(Self { path, map: Mutex::new(map) })
    }

    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.map.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn get(&self, account: &str) -> Option<String> {
        self.map().get(account).cloned()
    }

    fn set(&self, account: &str, key: &str) -> Result<(), AppError> {
        let mut map = self.map();
        map.insert(account.to_owned(), key.to_owned());
        write_private(&self.path, &map)
    }

    fn delete(&self, account: &str) -> Result<(), AppError> {
        let mut map = self.map();
        if map.remove(account).is_some() {
            write_private(&self.path, &map)?;
        }
        Ok(())
    }
}

/// 一時ファイルへ書いてからリネームし、書き込み途中の破損を避ける。
fn write_private(path: &Path, map: &HashMap<String, String>) -> Result<(), AppError> {
    use std::io::Write;
    let io = |e: std::io::Error| AppError::SecretStore(e.to_string());
    let tmp = path.with_extension("json.tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts.open(&tmp).map_err(io)?;
    f.write_all(serde_json::to_string(map).map_err(|e| AppError::SecretStore(e.to_string()))?.as_bytes())
        .map_err(io)?;
    f.sync_all().map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)
}

static STORE: OnceLock<Store> = OnceLock::new();

/// 起動時に1回呼ぶ。`dir`はアプリデータディレクトリ。
pub fn init(dir: &Path) -> Result<(), AppError> {
    let store = Store::open(dir.join("secrets.json"))?;
    let _ = STORE.set(store);
    Ok(())
}

fn store() -> Result<&'static Store, AppError> {
    STORE.get().ok_or_else(|| AppError::SecretStore("not initialized".into()))
}

fn get_secret(account: &str) -> Result<Option<String>, AppError> {
    Ok(store()?.get(account))
}

fn set_secret(account: &str, key: &str) -> Result<(), AppError> {
    store()?.set(account, key)
}

fn delete_secret(account: &str) -> Result<(), AppError> {
    store()?.delete(account)
}

pub fn has_tts_key() -> Result<bool, AppError> {
    Ok(get_secret(TTS_ACCOUNT)?.is_some())
}

/// Rust内部のTTSクライアント専用。コマンドとして公開しないこと。
pub fn get_tts_key() -> Result<Option<String>, AppError> {
    get_secret(TTS_ACCOUNT)
}

pub fn set_tts_key(key: &str) -> Result<(), AppError> {
    set_secret(TTS_ACCOUNT, key)
}

pub fn delete_tts_key() -> Result<(), AppError> {
    delete_secret(TTS_ACCOUNT)
}

pub fn has_aqua_key() -> Result<bool, AppError> {
    Ok(get_secret(AQUA_ACCOUNT)?.is_some())
}

/// Rust内部のAqua STTクライアント専用。コマンドとして公開しないこと。
pub fn get_aqua_key() -> Result<Option<String>, AppError> {
    get_secret(AQUA_ACCOUNT)
}

pub fn set_aqua_key(key: &str) -> Result<(), AppError> {
    set_secret(AQUA_ACCOUNT, key)
}

pub fn delete_aqua_key() -> Result<(), AppError> {
    delete_secret(AQUA_ACCOUNT)
}

/// 音声一覧取得(読み取り専用)で疎通確認する。
pub async fn verify_tts_key(key: &str) -> Result<(), AppError> {
    verify_tts_key_inner(key).await.map_err(|e| e.with_source(TTS_ACCOUNT))
}

async fn verify_tts_key_inner(key: &str) -> Result<(), AppError> {
    let res = http_client()?
        .get("https://texttospeech.googleapis.com/v1/voices?languageCode=ja-JP")
        .header("x-goog-api-key", key)
        .send()
        .await
        .map_err(AppError::from_reqwest)?;
    let status = res.status();
    if status.is_success() {
        Ok(())
    } else {
        let body = res.text().await.unwrap_or_default();
        Err(AppError::from_google(status.as_u16(), &body))
    }
}

fn http_client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| AppError::Network)
}

pub fn has_key(provider: Provider) -> Result<bool, AppError> {
    Ok(get_secret(provider.as_str())?.is_some())
}

/// Rust内部のLLMクライアント専用。コマンドとして公開しないこと。
pub fn get_key(provider: Provider) -> Result<Option<String>, AppError> {
    get_secret(provider.as_str())
}

pub fn set_key(provider: Provider, key: &str) -> Result<(), AppError> {
    set_secret(provider.as_str(), key)
}

pub fn delete_key(provider: Provider) -> Result<(), AppError> {
    delete_secret(provider.as_str())
}

/// モデル一覧取得(読み取り専用・無課金)で疎通確認する。
pub async fn verify_key(provider: Provider, key: &str) -> Result<(), AppError> {
    verify_key_inner(provider, key).await.map_err(|e| e.with_source(provider.as_str()))
}

async fn verify_key_inner(provider: Provider, key: &str) -> Result<(), AppError> {
    let client = http_client()?;
    let req = match provider {
        Provider::Anthropic => client
            .get("https://api.anthropic.com/v1/models")
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01"),
        Provider::Openai => client.get("https://api.openai.com/v1/models").bearer_auth(key),
    };
    let res = req.send().await.map_err(AppError::from_reqwest)?;
    if res.status().is_success() {
        Ok(())
    } else {
        Err(AppError::from_status(res.status().as_u16()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_persists_and_deletes() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("secrets.json");
        let s = Store::open(path.clone()).unwrap();
        assert_eq!(s.get("anthropic"), None);
        s.set("anthropic", "k1").unwrap();
        assert_eq!(Store::open(path.clone()).unwrap().get("anthropic").as_deref(), Some("k1"));
        s.delete("anthropic").unwrap();
        assert_eq!(Store::open(path).unwrap().get("anthropic"), None);
    }

    #[cfg(unix)]
    #[test]
    fn store_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("secrets.json");
        Store::open(path.clone()).unwrap().set("a", "b").unwrap();
        assert_eq!(std::fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    /// 実キーでの疎通確認。`TTS_TEST_KEY`が必要。
    #[tokio::test]
    #[ignore]
    async fn verifies_real_tts_key() {
        let key = std::env::var("TTS_TEST_KEY").unwrap();
        verify_tts_key(&key).await.unwrap();
    }

    /// 無効キーのエラーに発生元APIが付くことを確認する(ネットワーク必須)。
    #[tokio::test]
    #[ignore]
    async fn invalid_keys_report_their_source() {
        for p in Provider::ALL {
            let e = verify_key(p, "invalid-test-key").await.unwrap_err();
            let v = serde_json::to_value(&e).unwrap();
            println!("{} -> {v}", p.as_str());
            assert_eq!(v["source"], p.as_str());
            assert_eq!(v["code"], "unauthorized");
        }
        let v = serde_json::to_value(verify_tts_key("invalid-test-key").await.unwrap_err()).unwrap();
        assert_eq!((v["source"].as_str(), v["code"].as_str()), (Some("google_tts"), Some("unauthorized")));
    }
}
