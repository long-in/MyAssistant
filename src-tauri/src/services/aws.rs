//! AWS Bedrockの認証と呼び出し。
//!
//! 認証情報は`~/.aws/config`と`~/.aws/credentials`の内容を貼り付けて登録する(`secrets.rs`に保持)。
//! `role_arn` + `source_profile`のプロファイルは、MFA付きの`sts:AssumeRole`で一時クレデンシャルを得る。
//! 一時クレデンシャルはメモリ上にだけ置き、ディスクにもフロントエンドにも出さない。
use std::{
    collections::BTreeMap,
    sync::{Mutex, MutexGuard, OnceLock},
    time::{Duration, SystemTime},
};

use aws_credential_types::Credentials;
use aws_sdk_bedrockruntime::primitives::Blob;
use aws_sdk_sts::error::{ProvideErrorMetadata, SdkError};
use aws_types::region::Region;
use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

use super::workspace::WorkspaceCtx;
use crate::utils::error::AppError;

/// フロントエンドへMFAコードの入力を求めるイベント名。
pub const MFA_EVENT: &str = "aws-mfa-required";
/// プロファイルにリージョンが無いときの既定。
const DEFAULT_REGION: &str = "us-east-1";
/// 期限の何秒前から再取得するか。
const EXPIRY_MARGIN: Duration = Duration::from_secs(60);
/// MFAコードの入力を待つ上限。
const MFA_TIMEOUT: Duration = Duration::from_secs(180);
/// コードの誤入力を許す回数。
const MFA_ATTEMPTS: usize = 3;
const STS_TIMEOUT: Duration = Duration::from_secs(20);
/// セッション時間の既定(ロールチェーンの上限と同じ1時間)。
const DEFAULT_DURATION_SECS: i32 = 3600;

type Ini = BTreeMap<String, BTreeMap<String, String>>;

#[derive(Clone)]
struct Keys {
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
}

impl Keys {
    fn credentials(&self) -> Credentials {
        Credentials::new(
            &self.access_key_id,
            &self.secret_access_key,
            self.session_token.clone(),
            None,
            "myassistant",
        )
    }
}

#[derive(Clone)]
struct AssumeRole {
    role_arn: String,
    session_name: String,
    external_id: Option<String>,
    duration_secs: i32,
    mfa_serial: Option<String>,
    /// `source_profile`のアクセスキー。
    source: Keys,
}

enum Source {
    Static(Keys),
    AssumeRole(AssumeRole),
}

struct Resolved {
    region: String,
    source: Source,
}

/// `[profile x]`と`[x]`の両方を受け付け、同名のセクションは後から読んだ側で上書きする。
fn parse_ini(text: &str, into: &mut Ini) {
    let mut current: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.split(']').next()) {
            let header = header.trim();
            let name = header.strip_prefix("profile ").map_or(header, str::trim);
            // SSOセッションなど、プロファイルではないセクション
            let name = (!name.starts_with("sso-session ") && !name.starts_with("services ")).then(|| name.to_owned());
            if let Some(n) = &name {
                into.entry(n.clone()).or_default();
            }
            current = name;
        } else if let (Some(section), Some((k, v))) = (&current, line.split_once('=')) {
            into.entry(section.clone()).or_default().insert(k.trim().to_lowercase(), v.trim().to_owned());
        }
    }
}

/// configを先に、credentialsを後に読む(AWS CLIと同じく、credentialsが優先)。
fn merged(config: &str, credentials: &str) -> Ini {
    let mut ini = Ini::new();
    parse_ini(config, &mut ini);
    parse_ini(credentials, &mut ini);
    ini
}

fn keys_of(p: &BTreeMap<String, String>) -> Option<Keys> {
    let non_empty = |k: &str| p.get(k).filter(|v| !v.is_empty()).cloned();
    Some(Keys {
        access_key_id: non_empty("aws_access_key_id")?,
        secret_access_key: non_empty("aws_secret_access_key")?,
        session_token: non_empty("aws_session_token"),
    })
}

/// このアプリが扱えない認証方式の設定。あれば、使えない理由として示す。
fn unsupported_setting(p: &BTreeMap<String, String>) -> Option<&'static str> {
    ["credential_process", "sso_session", "sso_start_url", "credential_source", "web_identity_token_file"]
        .into_iter()
        .find(|k| p.contains_key(*k))
}

/// アクセスキーが無いプロファイルについて、使えない理由。
fn no_keys_reason(name: &str, p: &BTreeMap<String, String>) -> String {
    match unsupported_setting(p) {
        Some(key) => format!("profile {name} uses {key}, which is not supported"),
        None if p.contains_key("role_arn") => format!("profile {name} is a role (chained roles are not supported)"),
        None => format!("profile {name} has no access keys"),
    }
}

fn resolve(ini: &Ini, name: &str) -> Result<Resolved, AppError> {
    let cfg = |m: String| AppError::AwsConfig(m);
    let p = ini.get(name).ok_or_else(|| cfg(format!("profile not found: {name}")))?;
    let Some(role_arn) = p.get("role_arn").filter(|v| !v.is_empty()) else {
        let keys = keys_of(p).ok_or_else(|| cfg(no_keys_reason(name, p)))?;
        let region = p.get("region").cloned().unwrap_or_else(|| DEFAULT_REGION.to_owned());
        return Ok(Resolved { region, source: Source::Static(keys) });
    };
    let source_name = p
        .get("source_profile")
        .ok_or_else(|| cfg(format!("profile {name} has role_arn but no source_profile")))?;
    let source = ini
        .get(source_name)
        .ok_or_else(|| cfg(format!("source profile not found: {source_name}")))?;
    let source_keys = keys_of(source)
        .ok_or_else(|| cfg(format!("source profile {source_name}: {}", no_keys_reason(source_name, source))))?;
    let region = p
        .get("region")
        .or_else(|| source.get("region"))
        .cloned()
        .unwrap_or_else(|| DEFAULT_REGION.to_owned());
    let duration_secs = p
        .get("duration_seconds")
        .map(|v| v.parse::<i32>().map_err(|_| cfg(format!("invalid duration_seconds: {v}"))))
        .transpose()?
        .unwrap_or(DEFAULT_DURATION_SECS);
    Ok(Resolved {
        region,
        source: Source::AssumeRole(AssumeRole {
            role_arn: role_arn.clone(),
            session_name: p.get("role_session_name").cloned().unwrap_or_else(|| "myassistant".to_owned()),
            external_id: p.get("external_id").cloned(),
            duration_secs,
            mfa_serial: p.get("mfa_serial").filter(|v| !v.is_empty()).cloned(),
            source: source_keys,
        }),
    })
}

/// 選択中のプロファイル。保存済みの選択が今も使えるときだけ返す(他のプロファイルへ黙って切り替えない)。
fn selected(ini: &Ini, saved: Option<&str>) -> Option<String> {
    saved.filter(|n| resolve(ini, n).is_ok()).map(str::to_owned)
}

// ---- 状態(フロントエンドへ返す。秘密値は含めない) ----

#[derive(Serialize)]
pub struct ProfileInfo {
    pub name: String,
    pub region: Option<String>,
    /// `role_arn`を持つ(AssumeRoleで使う)。
    pub role: bool,
    pub mfa: bool,
    /// 呼び出しに必要なキーが揃っている。
    pub usable: bool,
    /// 使えない場合の理由。
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct Status {
    pub registered: bool,
    pub profiles: Vec<ProfileInfo>,
    pub selected: Option<String>,
    /// 一時クレデンシャルの期限(UNIX秒)。取得していなければnull。
    pub session_expires_at: Option<i64>,
    /// 登録済みのBedrockモデルID。
    pub models: Vec<String>,
    /// 初期候補のうち、未登録のもの。
    pub suggested_models: Vec<String>,
}

fn profiles_of(ini: &Ini) -> Vec<ProfileInfo> {
    ini.iter()
        .map(|(name, p)| {
            let resolved = resolve(ini, name);
            ProfileInfo {
                name: name.clone(),
                region: resolved.as_ref().ok().map(|r| r.region.clone()),
                role: p.contains_key("role_arn"),
                mfa: p.contains_key("mfa_serial"),
                usable: resolved.is_ok(),
                reason: resolved.as_ref().err().map(|e| e.to_string()),
            }
        })
        .collect()
}

/// 貼り付け中の内容から読み取れるプロファイル一覧。保存はしない。
pub fn preview(config: &str, credentials: &str) -> Vec<ProfileInfo> {
    profiles_of(&merged(config, credentials))
}

pub fn status(ctx: &WorkspaceCtx) -> Result<Status, AppError> {
    let (config, credentials) = ctx.secrets.get_aws_files()?;
    let ini = merged(&config, &credentials);
    let saved = ctx.secrets.get_aws_profile()?;
    let profiles = profiles_of(&ini);
    let session_expires_at = session(ctx).as_ref().and_then(|s| {
        s.expires.duration_since(SystemTime::UNIX_EPOCH).ok().map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
    });
    let selected = selected(&ini, saved.as_deref());
    let models = models(ctx);
    let suggested_models = suggested_models().into_iter().filter(|m| !models.contains(m)).collect();
    Ok(Status { registered: ctx.secrets.has_aws()?, profiles, selected, session_expires_at, models, suggested_models })
}

/// 貼り付けた内容と、使うプロファイルを検証して保存する(ネットワークは使わない)。モデルが未登録なら初期候補を登録する。
/// 選んだプロファイルが使えなければ保存しない。
pub fn save(ctx: &WorkspaceCtx, config: &str, credentials: &str, profile: &str) -> Result<(), AppError> {
    resolve(&merged(config, credentials), profile)?;
    ctx.secrets.set_aws_files(config.trim(), credentials.trim())?;
    ctx.secrets.set_aws_profile(profile)?;
    // 初回だけ初期候補を登録しておく(後から画面で入力し直せる)
    if models(ctx).is_empty() {
        set_models(ctx, &suggested_models())?;
    }
    clear_session(ctx);
    Ok(())
}

pub fn select_profile(ctx: &WorkspaceCtx, name: &str) -> Result<(), AppError> {
    let (config, credentials) = ctx.secrets.get_aws_files()?;
    resolve(&merged(&config, &credentials), name)?;
    ctx.secrets.set_aws_profile(name)?;
    clear_session(ctx);
    Ok(())
}

/// メモリ上の一時クレデンシャルとMFA入力待ちを破棄する(保存内容を差し替えたときに呼ぶ)。
pub fn reset(ctx: &WorkspaceCtx) {
    clear_session(ctx);
    cancel_mfa(ctx);
}

pub fn delete(ctx: &WorkspaceCtx) -> Result<(), AppError> {
    ctx.secrets.delete_aws()?;
    clear_session(ctx);
    cancel_mfa(ctx);
    Ok(())
}

// ---- 一時クレデンシャル ----

struct Session {
    profile: String,
    keys: Keys,
    expires: SystemTime,
}

/// ワークスペースごとの一時状態(一時クレデンシャル・MFA入力待ち)。
#[derive(Default)]
pub struct AwsState {
    session: Mutex<Option<Session>>,
    /// 同時に複数の呼び出しが来ても、MFAの入力と取得は1回にまとめる。
    refresh: tokio::sync::Mutex<()>,
    pending_mfa: Mutex<Option<oneshot::Sender<String>>>,
}

fn session(ctx: &WorkspaceCtx) -> MutexGuard<'_, Option<Session>> {
    ctx.shared.aws.session.lock().unwrap_or_else(|p| p.into_inner())
}

fn clear_session(ctx: &WorkspaceCtx) {
    *session(ctx) = None;
}

fn cached(ctx: &WorkspaceCtx, profile: &str) -> Option<Keys> {
    session(ctx)
        .as_ref()
        .filter(|s| s.profile == profile && s.expires > SystemTime::now() + EXPIRY_MARGIN)
        .map(|s| s.keys.clone())
}

// ---- MFAコードの入力待ち ----

static APP: OnceLock<AppHandle> = OnceLock::new();

/// 起動時に1回呼ぶ。MFAコードの入力要求をフロントエンドへ送るために使う。
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

#[derive(Serialize, Clone)]
struct MfaPrompt {
    profile: String,
    mfa_serial: String,
    /// 直前のコードが拒否されたときの理由。
    error: Option<String>,
    /// どのワークスペース(`personal`またはエージェントID)が求めているか。コードはこのIDで返す。
    workspace_id: String,
    /// エージェントの名称とイメージカラー(パーソナルは`None`)。入力欄で、どのエージェントのコードかを示す。
    workspace_name: Option<String>,
    workspace_color: Option<String>,
}

fn pending(ctx: &WorkspaceCtx) -> MutexGuard<'_, Option<oneshot::Sender<String>>> {
    ctx.shared.aws.pending_mfa.lock().unwrap_or_else(|p| p.into_inner())
}

async fn ask_mfa(ctx: &WorkspaceCtx, mut prompt: MfaPrompt) -> Result<String, AppError> {
    let app = APP.get().ok_or_else(|| AppError::AwsConfig("not initialized".into()))?;
    if let Some(ws) = app.try_state::<super::workspace::Workspaces>()
        && let Some((name, color)) = ws.label(&ctx.id)
    {
        (prompt.workspace_name, prompt.workspace_color) = (Some(name), Some(color));
    }
    let (tx, rx) = oneshot::channel();
    *pending(ctx) = Some(tx);
    app.emit(MFA_EVENT, prompt).map_err(|e| AppError::AwsConfig(e.to_string()))?;
    match tokio::time::timeout(MFA_TIMEOUT, rx).await {
        Ok(Ok(code)) => Ok(code),
        _ => {
            pending(ctx).take();
            Err(AppError::MfaRequired)
        }
    }
}

fn valid_mfa_code(code: &str) -> bool {
    code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit())
}

/// 入力されたMFAコードを、待っている呼び出しへ渡す。待っていなければ何もしない。
pub fn submit_mfa(ctx: &WorkspaceCtx, code: &str) -> Result<(), AppError> {
    let code = code.trim();
    if !valid_mfa_code(code) {
        return Err(AppError::AwsConfig("MFA code must be 6 digits".to_owned()));
    }
    if let Some(tx) = pending(ctx).take() {
        let _ = tx.send(code.to_owned());
    }
    Ok(())
}

/// MFAコードの入力を取りやめる(待っている呼び出しは`MfaRequired`で失敗する)。
pub fn cancel_mfa(ctx: &WorkspaceCtx) {
    pending(ctx).take();
}

// ---- AWS呼び出し ----

enum Failure {
    Timeout,
    Network,
    Service { code: String, message: String },
    Other,
}

impl Failure {
    fn of<E: ProvideErrorMetadata, R>(e: &SdkError<E, R>) -> Self {
        match e {
            SdkError::TimeoutError(_) => Self::Timeout,
            SdkError::DispatchFailure(_) => Self::Network,
            SdkError::ServiceError(s) => Self::Service {
                code: s.err().code().unwrap_or("Unknown").to_owned(),
                message: s.err().message().unwrap_or_default().chars().take(300).collect(),
            },
            _ => Self::Other,
        }
    }

    fn expired(&self) -> bool {
        matches!(self, Self::Service { code, .. } if code.contains("ExpiredToken"))
    }

    fn into_error(self) -> AppError {
        match self {
            Self::Timeout => AppError::Timeout,
            Self::Network => AppError::Network,
            Self::Other => AppError::BadResponse,
            Self::Service { code, message } => match code.as_str() {
                "ThrottlingException" | "TooManyRequestsException" => AppError::RateLimited,
                "InternalServerException"
                | "ServiceUnavailableException"
                | "ModelNotReadyException"
                | "ModelTimeoutException" => AppError::Server(503),
                _ => AppError::ProviderRejected(format!("{code}: {message}")),
            },
        }
    }
}

async fn assume_role(a: &AssumeRole, region: &str, code: Option<&str>) -> Result<(Keys, SystemTime), Failure> {
    let conf = aws_sdk_sts::Config::builder()
        .behavior_version_latest()
        .region(Region::new(region.to_owned()))
        .credentials_provider(a.source.credentials())
        .build();
    let mut req = aws_sdk_sts::Client::from_conf(conf)
        .assume_role()
        .role_arn(&a.role_arn)
        .role_session_name(&a.session_name)
        .duration_seconds(a.duration_secs);
    if let Some(id) = &a.external_id {
        req = req.external_id(id);
    }
    if let (Some(serial), Some(code)) = (&a.mfa_serial, code) {
        req = req.serial_number(serial).token_code(code);
    }
    let out = tokio::time::timeout(STS_TIMEOUT, req.send())
        .await
        .map_err(|_| Failure::Timeout)?
        .map_err(|e| Failure::of(&e))?;
    let c = out.credentials().ok_or(Failure::Other)?;
    let expires = SystemTime::try_from(*c.expiration()).map_err(|_| Failure::Other)?;
    let keys = Keys {
        access_key_id: c.access_key_id().to_owned(),
        secret_access_key: c.secret_access_key().to_owned(),
        session_token: Some(c.session_token().to_owned()),
    };
    Ok((keys, expires))
}

/// MFAが必要なら画面でコードを入力してもらい、AssumeRoleする。コードが拒否されたら理由付きで再入力を求める。
async fn assume_with_prompt(ctx: &WorkspaceCtx, profile: &str, a: &AssumeRole, region: &str) -> Result<Keys, AppError> {
    let mut last_error: Option<String> = None;
    for _ in 0..MFA_ATTEMPTS {
        let code = match &a.mfa_serial {
            Some(serial) => Some(
                ask_mfa(ctx, MfaPrompt {
                    profile: profile.to_owned(),
                    mfa_serial: serial.clone(),
                    error: last_error.take(),
                    workspace_id: ctx.id.clone(),
                    workspace_name: None,
                    workspace_color: None,
                })
                .await?,
            ),
            None => None,
        };
        match assume_role(a, region, code.as_deref()).await {
            Ok((keys, expires)) => {
                *session(ctx) = Some(Session { profile: profile.to_owned(), keys: keys.clone(), expires });
                return Ok(keys);
            }
            Err(f) => match (f.into_error(), code) {
                (AppError::ProviderRejected(m), Some(_)) => last_error = Some(m),
                (e, _) => return Err(e),
            },
        }
    }
    Err(AppError::ProviderRejected(last_error.unwrap_or_default()))
}

/// 選択中のプロファイルで使うクレデンシャルとリージョン。
async fn credentials(ctx: &WorkspaceCtx) -> Result<(Keys, String), AppError> {
    let (config, credentials) = ctx.secrets.get_aws_files()?;
    let ini = merged(&config, &credentials);
    let saved = ctx.secrets.get_aws_profile()?;
    let name = selected(&ini, saved.as_deref()).ok_or_else(|| AppError::KeyMissing("bedrock".to_owned()))?;
    let Resolved { region, source } = resolve(&ini, &name)?;
    let keys = match source {
        Source::Static(keys) => keys,
        Source::AssumeRole(a) => match cached(ctx, &name) {
            Some(keys) => keys,
            None => {
                let _guard = ctx.shared.aws.refresh.lock().await;
                match cached(ctx, &name) {
                    Some(keys) => keys,
                    None => assume_with_prompt(ctx, &name, &a, &region).await?,
                }
            }
        },
    };
    Ok((keys, region))
}

/// サンドボックスのコマンドへ渡す、選択中のプロファイルの認証情報(環境変数の名前と値)。
/// ロールを引き受けるプロファイルは、通常のBedrock呼び出しと同じ流れ(MFA入力・メモリ上のキャッシュ)で一時キーを得る。
pub async fn env_pairs(ctx: &WorkspaceCtx) -> Result<Vec<(String, String)>, AppError> {
    let (keys, region) = credentials(ctx).await?;
    let mut pairs = vec![
        ("AWS_ACCESS_KEY_ID".to_owned(), keys.access_key_id),
        ("AWS_SECRET_ACCESS_KEY".to_owned(), keys.secret_access_key),
    ];
    if let Some(token) = keys.session_token {
        pairs.push(("AWS_SESSION_TOKEN".to_owned(), token));
    }
    pairs.push(("AWS_REGION".to_owned(), region.clone()));
    pairs.push(("AWS_DEFAULT_REGION".to_owned(), region));
    Ok(pairs)
}

/// 初期候補にするClaudeのモデルID(`global.`付きの推論プロファイルID)。
/// 他のモデルや`jp.`などの接頭辞は、画面の「取得」ボタンか手入力で追加する。
const SUGGESTED_MODELS: &[&str] = &[
    "global.anthropic.claude-haiku-5-5",
    "global.anthropic.claude-sonnet-5-5",
    "global.anthropic.claude-opus-5-5",
];

fn suggested_models() -> Vec<String> {
    SUGGESTED_MODELS.iter().map(|m| (*m).to_owned()).collect()
}

/// モデルID・推論プロファイルID・ARNに使える文字だけを許す。
fn valid_model_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b':' | b'_' | b'/' | b'-'))
}

/// 登録済みのBedrockモデルID(追加順)。
pub fn models(ctx: &WorkspaceCtx) -> Vec<String> {
    ctx.secrets.get_aws_models()
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default()
}

fn set_models(ctx: &WorkspaceCtx, list: &[String]) -> Result<(), AppError> {
    ctx.secrets.set_aws_models(&serde_json::to_string(list).map_err(|e| AppError::AwsConfig(e.to_string()))?)
}

pub fn add_model(ctx: &WorkspaceCtx, id: &str) -> Result<(), AppError> {
    let id = id.trim();
    if !valid_model_id(id) {
        return Err(AppError::AwsConfig(format!("invalid model id: {id}")));
    }
    let mut list = models(ctx);
    if !list.iter().any(|m| m == id) {
        list.push(id.to_owned());
        set_models(ctx, &list)?;
    }
    Ok(())
}

pub fn remove_model(ctx: &WorkspaceCtx, id: &str) -> Result<(), AppError> {
    let list: Vec<String> = models(ctx).into_iter().filter(|m| m != id).collect();
    set_models(ctx, &list)
}

/// Messages API形式のリクエスト(`model`なし)でInvokeModelを呼ぶ。`model_id`はBedrockのモデルID(または推論プロファイルID)。
/// 応答はMessages API形式のJSON。
/// クレデンシャルの期限切れで失敗したときは、取り直して1回だけ再試行する。
pub async fn invoke_model(ctx: &WorkspaceCtx, model_id: &str, request: &Value, timeout: Duration) -> Result<Value, AppError> {
    for attempt in 0..2 {
        let (keys, region) = credentials(ctx).await?;
        match invoke_once(&keys, &region, model_id, request, timeout).await {
            Err(f) if f.expired() && attempt == 0 => clear_session(ctx),
            result => return result.map_err(Failure::into_error),
        }
    }
    unreachable!("the second attempt always returns")
}

async fn invoke_once(
    keys: &Keys,
    region: &str,
    model_id: &str,
    request: &Value,
    timeout: Duration,
) -> Result<Value, Failure> {
    let mut body = request.clone();
    body["anthropic_version"] = json!("bedrock-2023-05-31");
    let conf = aws_sdk_bedrockruntime::Config::builder()
        .behavior_version_latest()
        .region(Region::new(region.to_owned()))
        .credentials_provider(keys.credentials())
        .build();
    let out = tokio::time::timeout(
        timeout,
        aws_sdk_bedrockruntime::Client::from_conf(conf)
            .invoke_model()
            .model_id(model_id)
            .content_type("application/json")
            .accept("application/json")
            .body(Blob::new(serde_json::to_vec(&body).map_err(|_| Failure::Other)?))
            .send(),
    )
    .await
    .map_err(|_| Failure::Timeout)?
    .map_err(|e| Failure::of(&e))?;
    serde_json::from_slice(out.body().as_ref()).map_err(|_| Failure::Other)
}

/// Bedrockの推論プロファイルから、`global.anthropic.`で始まるClaudeのIDを取得する。
/// `list-foundation-models`が返すのは接頭辞なしの基盤モデルIDだけで、`global.`付きのIDは推論プロファイル側にある。
pub async fn fetch_global_models(ctx: &WorkspaceCtx) -> Result<Vec<String>, AppError> {
    for attempt in 0..2 {
        let (keys, region) = credentials(ctx).await?;
        match list_global_profiles(&keys, &region).await {
            Err(f) if f.expired() && attempt == 0 => clear_session(ctx),
            result => return result.map_err(Failure::into_error),
        }
    }
    unreachable!("the second attempt always returns")
}

async fn list_global_profiles(keys: &Keys, region: &str) -> Result<Vec<String>, Failure> {
    use aws_sdk_bedrock::types::InferenceProfileType;
    let conf = aws_sdk_bedrock::Config::builder()
        .behavior_version_latest()
        .region(Region::new(region.to_owned()))
        .credentials_provider(keys.credentials())
        .build();
    let client = aws_sdk_bedrock::Client::from_conf(conf);
    let mut ids = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let page = tokio::time::timeout(
            Duration::from_secs(30),
            client
                .list_inference_profiles()
                .type_equals(InferenceProfileType::SystemDefined)
                .set_next_token(token.take())
                .send(),
        )
        .await
        .map_err(|_| Failure::Timeout)?
        .map_err(|e| Failure::of(&e))?;
        ids.extend(page.inference_profile_summaries().iter().map(|p| p.inference_profile_id().to_owned()));
        token = page.next_token().map(str::to_owned);
        if token.is_none() {
            break;
        }
    }
    Ok(global_claude_ids(ids))
}

/// `global.anthropic.`で始まるIDだけを、重複なく昇順で返す。
fn global_claude_ids(mut ids: Vec<String>) -> Vec<String> {
    ids.retain(|id| id.starts_with("global.anthropic."));
    ids.sort();
    ids.dedup();
    ids
}

/// 認証からモデル呼び出し(登録済みの先頭モデル)までを通しで確認する。MFAが必要なら画面でコード入力を求める。
pub async fn test_connection(ctx: &WorkspaceCtx) -> Result<(), AppError> {
    let model = models(ctx)
        .into_iter()
        .next()
        .ok_or_else(|| AppError::AwsConfig("no model registered".to_owned()))?;
    let request = json!({ "max_tokens": 8, "messages": [{ "role": "user", "content": "ping" }] });
    invoke_model(ctx, &model, &request, Duration::from_secs(30)).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "\
# comment
[default]
region = ap-northeast-1

[profile admin]
role_arn = arn:aws:iam::123456789012:role/Admin
source_profile = base
mfa_serial = arn:aws:iam::111111111111:mfa/me
role_session_name = me-session
duration_seconds = 1800

[profile nokeys]
region = eu-west-1

[sso-session corp]
sso_region = us-east-1
";
    const CREDENTIALS: &str = "\
[base]
aws_access_key_id = AKIABASE
aws_secret_access_key = secretbase
; comment
[plain]
aws_access_key_id=AKIAPLAIN
aws_secret_access_key=secretplain
aws_session_token=tok
";

    #[test]
    fn parses_profiles_from_both_files() {
        let ini = merged(CONFIG, CREDENTIALS);
        assert_eq!(ini["default"]["region"], "ap-northeast-1");
        assert_eq!(ini["admin"]["role_arn"], "arn:aws:iam::123456789012:role/Admin");
        assert_eq!(ini["plain"]["aws_session_token"], "tok");
        assert!(!ini.contains_key("sso-session corp"));
        assert!(!ini.contains_key("profile admin"));
    }

    #[test]
    fn credentials_file_overrides_config() {
        let ini = merged("[p]\nregion = a\naws_access_key_id = old\n", "[p]\naws_access_key_id = new\n");
        assert_eq!(ini["p"]["aws_access_key_id"], "new");
        assert_eq!(ini["p"]["region"], "a");
    }

    #[test]
    fn resolves_static_and_assume_role() {
        let ini = merged(CONFIG, CREDENTIALS);
        let plain = resolve(&ini, "plain").unwrap();
        assert_eq!(plain.region, DEFAULT_REGION);
        assert!(matches!(plain.source, Source::Static(k) if k.access_key_id == "AKIAPLAIN" && k.session_token.as_deref() == Some("tok")));

        let admin = resolve(&ini, "admin").unwrap();
        assert_eq!(admin.region, DEFAULT_REGION);
        let Source::AssumeRole(a) = admin.source else { panic!("expected assume role") };
        assert_eq!(a.source.access_key_id, "AKIABASE");
        assert_eq!(a.session_name, "me-session");
        assert_eq!(a.duration_secs, 1800);
        assert_eq!(a.mfa_serial.as_deref(), Some("arn:aws:iam::111111111111:mfa/me"));
    }

    #[test]
    fn role_region_falls_back_to_source_profile() {
        let ini = merged(
            "[profile r]\nrole_arn = arn\nsource_profile = s\n[profile s]\nregion = eu-west-1\n",
            "[s]\naws_access_key_id = a\naws_secret_access_key = b\n",
        );
        assert_eq!(resolve(&ini, "r").unwrap().region, "eu-west-1");
    }

    #[test]
    fn rejects_unusable_profiles() {
        let ini = merged(CONFIG, CREDENTIALS);
        assert!(matches!(resolve(&ini, "nokeys"), Err(AppError::AwsConfig(_))));
        assert!(matches!(resolve(&ini, "missing"), Err(AppError::AwsConfig(_))));
        let no_source = merged("[profile r]\nrole_arn = arn\n", "");
        assert!(matches!(resolve(&no_source, "r"), Err(AppError::AwsConfig(_))));
        let bad_duration = merged(
            "[profile r]\nrole_arn = arn\nsource_profile = s\nduration_seconds = x\n",
            "[s]\naws_access_key_id = a\naws_secret_access_key = b\n",
        );
        assert!(matches!(resolve(&bad_duration, "r"), Err(AppError::AwsConfig(_))));
    }

    #[test]
    fn explains_why_a_profile_is_unusable() {
        let reason = |config: &str, credentials: &str, name: &str| {
            preview(config, credentials).into_iter().find(|p| p.name == name).unwrap().reason
        };
        let keys = "[base]\naws_access_key_id = a\naws_secret_access_key = b\n";
        // 参照先のプロファイルが無い(credentialsを貼っていない)
        let role = "[profile r]\nrole_arn = arn\nsource_profile = base\n";
        assert_eq!(reason(role, "", "r").unwrap(), "source profile not found: base");
        assert!(reason(role, keys, "r").is_none());
        // 未対応の方式
        assert!(reason("[profile p]\ncredential_process = x\n", "", "p").unwrap().contains("credential_process"));
        assert!(reason("[profile p]\nsso_session = s\n", "", "p").unwrap().contains("sso_session"));
        // ロールの連鎖
        let chain = "[profile a]\nrole_arn = arn\nsource_profile = b\n[profile b]\nrole_arn = arn2\nsource_profile = base\n";
        assert!(reason(chain, keys, "a").unwrap().contains("chained roles"));
        // 何も書かれていない
        assert!(reason("[profile e]\nregion = x\n", "", "e").unwrap().contains("no access keys"));
    }

    #[test]
    fn selection_never_falls_back_silently() {
        let ini = merged(CONFIG, CREDENTIALS);
        assert_eq!(selected(&ini, Some("plain")).as_deref(), Some("plain"));
        // 保存済みが使えない・無いときは、他のプロファイルを選ばず未選択にする
        assert_eq!(selected(&ini, Some("nokeys")), None);
        assert_eq!(selected(&ini, Some("gone")), None);
        assert_eq!(selected(&ini, None), None);
    }

    #[test]
    fn preview_lists_profiles_with_usability() {
        let list = preview(CONFIG, CREDENTIALS);
        let get = |n: &str| list.iter().find(|p| p.name == n).unwrap();
        assert!(get("admin").usable && get("admin").role && get("admin").mfa);
        assert!(get("plain").usable && !get("plain").role);
        assert!(!get("nokeys").usable && !get("default").usable);
        assert!(preview("", "").is_empty());
    }

    #[test]
    fn mfa_code_must_be_six_digits() {
        assert!(valid_mfa_code("012345"));
        assert!(!valid_mfa_code("12345"));
        assert!(!valid_mfa_code("12345a"));
        assert!(!valid_mfa_code("１２３４５６"));
    }

    #[test]
    fn suggested_models_are_valid_ids() {
        let list = suggested_models();
        assert_eq!(list, ["global.anthropic.claude-haiku-5-5", "global.anthropic.claude-sonnet-5-5", "global.anthropic.claude-opus-5-5"]);
        assert!(list.iter().all(|m| valid_model_id(m)));
    }

    #[test]
    fn keeps_only_global_claude_profiles() {
        let ids = ["us.anthropic.claude-x", "global.anthropic.claude-b", "global.amazon.nova", "global.anthropic.claude-a", "global.anthropic.claude-b"]
            .map(String::from)
            .to_vec();
        assert_eq!(global_claude_ids(ids), ["global.anthropic.claude-a", "global.anthropic.claude-b"]);
        assert!(global_claude_ids(Vec::new()).is_empty());
    }

    #[test]
    fn model_id_accepts_ids_and_arns_only() {
        assert!(valid_model_id("us.anthropic.claude-sonnet-4-5-20250929-v1:0"));
        assert!(valid_model_id("arn:aws:bedrock:us-east-1:123456789012:inference-profile/us.anthropic.x-v1:0"));
        assert!(!valid_model_id(""));
        assert!(!valid_model_id("a b"));
        assert!(!valid_model_id("a\nb"));
        assert!(!valid_model_id(&"a".repeat(257)));
    }

    #[test]
    fn expired_token_is_detected() {
        let f = Failure::Service { code: "ExpiredTokenException".into(), message: String::new() };
        assert!(f.expired());
        assert!(!Failure::Timeout.expired());
    }
}
