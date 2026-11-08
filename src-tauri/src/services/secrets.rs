//! APIキーの保存。アプリデータ配下の`secrets.json`(所有者のみ読み書き可)に保持する。
//! キーの生値はこのモジュールの外へ出さない。
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Mutex,
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::utils::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Anthropic,
    /// AWS Bedrock経由のClaude。認証はAPIキーではなく、貼り付けたAWSの設定ファイル(`aws.rs`)。
    Bedrock,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Anthropic, Provider::Bedrock];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::Bedrock => "bedrock",
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

/// AWS Bedrock用。`~/.aws/config`と`~/.aws/credentials`の貼り付け内容をそのまま保持する。
const AWS_CONFIG_ACCOUNT: &str = "aws_config";
const AWS_CREDENTIALS_ACCOUNT: &str = "aws_credentials";
/// 使うプロファイル名(秘密ではないが、保存先を分けないため同じストアに置く)。
const AWS_PROFILE_ACCOUNT: &str = "aws_profile";
/// 使うBedrockのモデルID一覧(JSON配列)。秘密ではないが、プロファイルと同じ理由で同じストアに置く。
const AWS_MODELS_ACCOUNT: &str = "aws_models";

/// `Secrets::backup`で控えた内容。中身はこのモジュールの外へ出さない。
pub struct SecretsBackup(HashMap<String, String>);

/// 1つのワークスペース(`secrets.json`)分のキー保管庫。ワークスペースごとに別インスタンスを持つ。
pub struct Secrets {
    path: PathBuf,
    map: Mutex<HashMap<String, String>>,
}

impl Secrets {
    /// `dir`直下の`secrets.json`を開く。無ければ空で始める。
    pub fn open(dir: &Path) -> Result<Self, AppError> {
        let path = dir.join("secrets.json");
        let map = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| AppError::SecretStore(e.to_string()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(e) => return Err(AppError::SecretStore(e.to_string())),
        };
        Ok(Self { path, map: Mutex::new(map) })
    }

    /// `src`の保存内容をすべてこの保管庫へ写す(同じ名前は上書き、`src`にだけ無いものは残す)。写した件数を返す。
    /// 生値はこのモジュールの外へ出さない。ワークスペース間で設定をコピーするときに使う。
    pub fn copy_from(&self, src: &Secrets) -> Result<usize, AppError> {
        if std::ptr::eq(self, src) {
            return Ok(0);
        }
        let snapshot = src.map().clone();
        let count = snapshot.len();
        let mut map = self.map();
        // 書き込みに成功してからメモリへ反映する(失敗したときに、メモリだけ新しい内容になるのを避ける)
        let mut merged = map.clone();
        merged.extend(snapshot);
        write_private(&self.path, &merged)?;
        *map = merged;
        Ok(count)
    }

    /// `src`のAPIキー(Anthropic)だけを写す。TTS・音声認識・AWSの設定は写さない。写したら`true`、`src`に無ければ`false`。
    /// 生値はこのモジュールの外へ出さない。リーダーが他のワークスペースのキーを使うときに使う。
    pub fn copy_api_key_from(&self, src: &Secrets, provider: Provider) -> Result<bool, AppError> {
        if std::ptr::eq(self, src) || provider == Provider::Bedrock {
            return Ok(false);
        }
        match src.get_key(provider)? {
            Some(key) => self.set_key(provider, &key).map(|()| true),
            None => Ok(false),
        }
    }

    /// Bedrockのモデル一覧(秘密ではない設定項目)だけを`src`から写す。キーは写さない。`src`に無ければ何もしない。
    pub fn copy_aws_models_from(&self, src: &Secrets) -> Result<(), AppError> {
        if std::ptr::eq(self, src) {
            return Ok(());
        }
        match src.get_aws_models()? {
            Some(models) => self.set_aws_models(&models),
            None => Ok(()),
        }
    }

    /// 現在の保存内容を控える。`restore`で元へ戻すためのもので、中身は外から読めない。
    pub fn backup(&self) -> SecretsBackup {
        SecretsBackup(self.map().clone())
    }

    /// `backup`で控えた内容へ戻す。
    pub fn restore(&self, backup: SecretsBackup) -> Result<(), AppError> {
        let mut map = self.map();
        write_private(&self.path, &backup.0)?;
        *map = backup.0;
        Ok(())
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

impl Secrets {
    fn get_secret(&self, account: &str) -> Result<Option<String>, AppError> {
        Ok(self.get(account))
    }

    fn set_secret(&self, account: &str, key: &str) -> Result<(), AppError> {
        self.set(account, key)
    }

    fn delete_secret(&self, account: &str) -> Result<(), AppError> {
        self.delete(account)
    }

    pub fn has_tts_key(&self) -> Result<bool, AppError> {
        Ok(self.get_secret(TTS_ACCOUNT)?.is_some())
    }

    /// Rust内部のTTSクライアント専用。コマンドとして公開しないこと。
    pub fn get_tts_key(&self) -> Result<Option<String>, AppError> {
        self.get_secret(TTS_ACCOUNT)
    }

    pub fn set_tts_key(&self, key: &str) -> Result<(), AppError> {
        self.set_secret(TTS_ACCOUNT, key)
    }

    pub fn delete_tts_key(&self) -> Result<(), AppError> {
        self.delete_secret(TTS_ACCOUNT)
    }

    pub fn has_aqua_key(&self) -> Result<bool, AppError> {
        Ok(self.get_secret(AQUA_ACCOUNT)?.is_some())
    }

    /// Rust内部のAqua STTクライアント専用。コマンドとして公開しないこと。
    pub fn get_aqua_key(&self) -> Result<Option<String>, AppError> {
        self.get_secret(AQUA_ACCOUNT)
    }

    pub fn set_aqua_key(&self, key: &str) -> Result<(), AppError> {
        self.set_secret(AQUA_ACCOUNT, key)
    }

    pub fn delete_aqua_key(&self) -> Result<(), AppError> {
        self.delete_secret(AQUA_ACCOUNT)
    }

    pub fn has_key(&self, provider: Provider) -> Result<bool, AppError> {
        match provider {
            Provider::Bedrock => self.has_aws(),
            p => Ok(self.get_secret(p.as_str())?.is_some()),
        }
    }

    /// Rust内部のLLMクライアント専用。コマンドとして公開しないこと。Bedrockは対象外(`get_aws_files`)。
    pub fn get_key(&self, provider: Provider) -> Result<Option<String>, AppError> {
        match provider {
            Provider::Bedrock => Ok(None),
            p => self.get_secret(p.as_str()),
        }
    }

    pub fn set_key(&self, provider: Provider, key: &str) -> Result<(), AppError> {
        match provider {
            Provider::Bedrock => Err(AppError::InvalidProvider(provider.as_str().to_owned())),
            p => self.set_secret(p.as_str(), key),
        }
    }

    pub fn delete_key(&self, provider: Provider) -> Result<(), AppError> {
        match provider {
            Provider::Bedrock => self.delete_aws(),
            p => self.delete_secret(p.as_str()),
        }
    }

    pub fn has_aws(&self) -> Result<bool, AppError> {
        Ok(self.get_secret(AWS_CONFIG_ACCOUNT)?.is_some() || self.get_secret(AWS_CREDENTIALS_ACCOUNT)?.is_some())
    }

    /// Rust内部のAWS認証(`aws.rs`)専用。コマンドとして公開しないこと。未登録の側は空文字を返す。
    pub fn get_aws_files(&self) -> Result<(String, String), AppError> {
        Ok((
            self.get_secret(AWS_CONFIG_ACCOUNT)?.unwrap_or_default(),
            self.get_secret(AWS_CREDENTIALS_ACCOUNT)?.unwrap_or_default(),
        ))
    }

    pub fn set_aws_files(&self, config: &str, credentials: &str) -> Result<(), AppError> {
        self.set_secret(AWS_CONFIG_ACCOUNT, config)?;
        self.set_secret(AWS_CREDENTIALS_ACCOUNT, credentials)
    }

    pub fn get_aws_profile(&self) -> Result<Option<String>, AppError> {
        self.get_secret(AWS_PROFILE_ACCOUNT)
    }

    pub fn set_aws_profile(&self, name: &str) -> Result<(), AppError> {
        self.set_secret(AWS_PROFILE_ACCOUNT, name)
    }

    pub fn get_aws_models(&self) -> Result<Option<String>, AppError> {
        self.get_secret(AWS_MODELS_ACCOUNT)
    }

    pub fn set_aws_models(&self, json: &str) -> Result<(), AppError> {
        self.set_secret(AWS_MODELS_ACCOUNT, json)
    }

    pub fn delete_aws(&self) -> Result<(), AppError> {
        self.delete_secret(AWS_MODELS_ACCOUNT)?;
        self.delete_secret(AWS_CONFIG_ACCOUNT)?;
        self.delete_secret(AWS_CREDENTIALS_ACCOUNT)?;
        self.delete_secret(AWS_PROFILE_ACCOUNT)
    }
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
        Provider::Bedrock => return Err(AppError::InvalidProvider(provider.as_str().to_owned())),
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
        let s = Secrets::open(d.path()).unwrap();
        assert_eq!(s.get("anthropic"), None);
        s.set("anthropic", "k1").unwrap();
        assert_eq!(Secrets::open(d.path()).unwrap().get("anthropic").as_deref(), Some("k1"));
        s.delete("anthropic").unwrap();
        assert_eq!(Secrets::open(d.path()).unwrap().get("anthropic"), None);
    }

    #[test]
    fn copy_api_key_copies_only_the_provider_key() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (src, leader) = (Secrets::open(a.path()).unwrap(), Secrets::open(b.path()).unwrap());
        assert!(!leader.copy_api_key_from(&src, Provider::Anthropic).unwrap(), "コピー元にキーが無ければ何も写さない");
        src.set_key(Provider::Anthropic, "ka").unwrap();
        src.set_tts_key("tts-a").unwrap();
        leader.set_key(Provider::Anthropic, "old").unwrap();
        assert!(leader.copy_api_key_from(&src, Provider::Anthropic).unwrap());
        assert_eq!(leader.get_key(Provider::Anthropic).unwrap().as_deref(), Some("ka")); // 上書き
        assert!(!leader.has_tts_key().unwrap(), "キー以外(TTSなど)は写さない");
        assert!(!leader.copy_api_key_from(&src, Provider::Bedrock).unwrap(), "Bedrockは対象外");
        assert_eq!(src.get_key(Provider::Anthropic).unwrap().as_deref(), Some("ka")); // コピー元は変えない
    }

    #[test]
    fn copy_from_merges_without_touching_source() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (sa, sb) = (Secrets::open(a.path()).unwrap(), Secrets::open(b.path()).unwrap());
        sa.set_key(Provider::Anthropic, "ka").unwrap();
        sa.set_tts_key("tts-a").unwrap();
        sb.set_tts_key("tts-b").unwrap();
        sb.set_aqua_key("aqua-b").unwrap();
        assert_eq!(sb.copy_from(&sa).unwrap(), 2);
        assert_eq!(sb.get_key(Provider::Anthropic).unwrap().as_deref(), Some("ka"));
        assert_eq!(sb.get_tts_key().unwrap().as_deref(), Some("tts-a")); // 上書き
        assert_eq!(sb.get_aqua_key().unwrap().as_deref(), Some("aqua-b")); // 残る
        assert_eq!(sa.get_tts_key().unwrap().as_deref(), Some("tts-a"));
        // 保存済みで、開き直しても残る
        assert!(Secrets::open(b.path()).unwrap().has_key(Provider::Anthropic).unwrap());
        // 自分自身へのコピーは何もしない(ロックの二重取得をしない)
        assert_eq!(sa.copy_from(&sa).unwrap(), 0);
    }

    /// ワークスペース間でキーが混ざらないこと。
    #[test]
    fn stores_in_different_dirs_are_isolated() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (sa, sb) = (Secrets::open(a.path()).unwrap(), Secrets::open(b.path()).unwrap());
        sa.set_key(Provider::Anthropic, "ka").unwrap();
        assert!(sa.has_key(Provider::Anthropic).unwrap());
        assert!(!sb.has_key(Provider::Anthropic).unwrap());
        assert_eq!(Secrets::open(b.path()).unwrap().get_key(Provider::Anthropic).unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn store_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        Secrets::open(d.path()).unwrap().set("a", "b").unwrap();
        assert_eq!(std::fs::metadata(d.path().join("secrets.json")).unwrap().permissions().mode() & 0o777, 0o600);
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
        for p in Provider::ALL.into_iter().filter(|p| *p != Provider::Bedrock) {
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
