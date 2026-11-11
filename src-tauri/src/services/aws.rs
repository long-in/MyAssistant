//! AWS Bedrockの認証と呼び出し。
//!
//! 認証情報は`~/.aws/config`と`~/.aws/credentials`の内容を貼り付けて登録する(`secrets.rs`に保持)。
//! `role_arn` + `source_profile`のプロファイルは、MFA付きの`sts:AssumeRole`で一時クレデンシャルを得る。
//! IAM Identity Center(SSO)のプロファイルは、アプリ自身がデバイス認可フローを行い(ブラウザで承認)、`sso:GetRoleCredentials`で一時クレデンシャルを得る。
//! ホストの`aws sso login`や`~/.aws/sso/cache`には依存しない。SSOのトークンもメモリ上にだけ置く。
//! 一時クレデンシャルはメモリ上にだけ置き、ディスクにもフロントエンドにも出さない。
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant, SystemTime},
};

use aws_credential_types::Credentials;
use aws_sdk_sts::error::{ProvideErrorMetadata, SdkError};
use aws_types::region::Region;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

use super::workspace::WorkspaceCtx;
use crate::utils::error::AppError;

/// フロントエンドへMFAコードの入力を求めるイベント名。
pub const MFA_EVENT: &str = "aws-mfa-required";
/// フロントエンドへSSOの承認(確認コードの表示とブラウザでの承認)を求めるイベント名。
pub const SSO_EVENT: &str = "aws-sso-required";
/// SSOの承認待ちが終わった(成功・失敗・取り消し)ことを知らせるイベント名。ペイロードはワークスペースID。
pub const SSO_FINISHED_EVENT: &str = "aws-sso-finished";
/// SSOのクライアント登録に使う名前。
const SSO_CLIENT_NAME: &str = "myassistant";
/// `[sso-session]`にスコープの指定が無いときの既定(AWS CLIと同じ)。
const SSO_DEFAULT_SCOPE: &str = "sso:account:access";
/// デバイス認可のポーリング間隔の既定と、`SlowDown`を受けたときの加算。
const SSO_POLL_INTERVAL: Duration = Duration::from_secs(5);
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

type Section = BTreeMap<String, String>;
type Ini = BTreeMap<String, Section>;

/// 貼り付けた設定から読み取った内容。
#[derive(Default)]
struct Files {
    profiles: Ini,
    /// `[sso-session x]`のセクション。
    sso_sessions: Ini,
}

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

/// IAM Identity Center(SSO)で得るロール。
#[derive(Clone)]
struct Sso {
    start_url: String,
    sso_region: String,
    account_id: String,
    role_name: String,
    /// クライアント登録に付けるスコープ。`sso_session`形式だけが持つ(リフレッシュトークンを得られる)。旧形式は空。
    scopes: Vec<String>,
}

impl Sso {
    /// トークンのキャッシュのキー。同じ発行元のプロファイルは、1回の承認を共有する。
    fn token_key(&self) -> String {
        format!("{}|{}|{}", self.start_url, self.sso_region, self.scopes.join(","))
    }
}

/// 自分でクレデンシャルを得られるプロファイル(ロールの引き受け元にもなる)。
#[derive(Clone)]
enum Base {
    Keys(Keys),
    Sso(Sso),
}

#[derive(Clone)]
struct AssumeRole {
    role_arn: String,
    session_name: String,
    external_id: Option<String>,
    duration_secs: i32,
    /// 引き受け元がSSOのときは持たない(MFAはSSOの承認側で済んでいる)。
    mfa_serial: Option<String>,
    /// `source_profile`のクレデンシャル。
    source: Base,
}

enum Source {
    Static(Keys),
    Sso(Sso),
    AssumeRole(AssumeRole),
}

impl Source {
    fn uses_sso(&self) -> bool {
        matches!(self, Self::Sso(_) | Self::AssumeRole(AssumeRole { source: Base::Sso(_), .. }))
    }

    fn mfa(&self) -> bool {
        matches!(self, Self::AssumeRole(a) if a.mfa_serial.is_some())
    }
}

struct Resolved {
    region: String,
    source: Source,
}

/// `[profile x]`と`[x]`の両方を受け付け、同名のセクションは後から読んだ側で上書きする。`[sso-session x]`は別に持つ。
fn parse_ini(text: &str, into: &mut Files) {
    // いま読んでいるセクション(SSOセッションかどうかと名前)。扱わないセクションは`None`
    let mut current: Option<(bool, String)> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.split(']').next()) {
            let header = header.trim();
            current = if let Some(name) = header.strip_prefix("sso-session ") {
                Some((true, name.trim().to_owned()))
            } else if header.starts_with("services ") {
                None
            } else {
                Some((false, header.strip_prefix("profile ").map_or(header, str::trim).to_owned()))
            };
            if let Some((sso, name)) = &current {
                into.section(*sso).entry(name.clone()).or_default();
            }
        } else if let (Some((sso, name)), Some((k, v))) = (&current, line.split_once('=')) {
            into.section(*sso).entry(name.clone()).or_default().insert(k.trim().to_lowercase(), v.trim().to_owned());
        }
    }
}

impl Files {
    fn section(&mut self, sso_session: bool) -> &mut Ini {
        if sso_session { &mut self.sso_sessions } else { &mut self.profiles }
    }
}

/// configを先に、credentialsを後に読む(AWS CLIと同じく、credentialsが優先)。
fn merged(config: &str, credentials: &str) -> Files {
    let mut files = Files::default();
    parse_ini(config, &mut files);
    parse_ini(credentials, &mut files);
    files
}

fn keys_of(p: &Section) -> Option<Keys> {
    let non_empty = |k: &str| p.get(k).filter(|v| !v.is_empty()).cloned();
    Some(Keys {
        access_key_id: non_empty("aws_access_key_id")?,
        secret_access_key: non_empty("aws_secret_access_key")?,
        session_token: non_empty("aws_session_token"),
    })
}

/// このアプリが扱えない認証方式の設定。あれば、使えない理由として示す。
fn unsupported_setting(p: &Section) -> Option<&'static str> {
    ["credential_process", "credential_source", "web_identity_token_file"].into_iter().find(|k| p.contains_key(*k))
}

/// クレデンシャルを得られないプロファイルについて、使えない理由。
fn no_keys_reason(name: &str, p: &Section) -> String {
    match unsupported_setting(p) {
        Some(key) => format!("profile {name} uses {key}, which is not supported"),
        None if p.contains_key("role_arn") => format!("profile {name} is a role (chained roles are not supported)"),
        None => format!("profile {name} has no access keys"),
    }
}

/// プロファイルのSSO設定。SSOの設定が無ければ`None`、あるのに不備があればエラー。
/// `sso_session`(`[sso-session x]`を参照する形)と、`sso_start_url`をプロファイルへ直接書く旧形式の両方を受け付ける。
fn sso_of(files: &Files, name: &str, p: &Section) -> Result<Option<Sso>, AppError> {
    let cfg = |m: String| AppError::AwsConfig(m);
    let get = |s: &Section, k: &str| s.get(k).filter(|v| !v.is_empty()).cloned();
    let (start_url, sso_region, scopes) = if let Some(session_name) = get(p, "sso_session") {
        let session = files
            .sso_sessions
            .get(&session_name)
            .ok_or_else(|| cfg(format!("sso-session not found: {session_name}")))?;
        let need = |k: &str| get(session, k).ok_or_else(|| cfg(format!("sso-session {session_name} has no {k}")));
        let scopes = match get(session, "sso_registration_scopes") {
            Some(v) => v.split(',').map(|x| x.trim().to_owned()).filter(|x| !x.is_empty()).collect(),
            None => vec![SSO_DEFAULT_SCOPE.to_owned()],
        };
        (need("sso_start_url")?, need("sso_region")?, scopes)
    } else if let Some(start_url) = get(p, "sso_start_url") {
        let region = get(p, "sso_region").ok_or_else(|| cfg(format!("profile {name} has sso_start_url but no sso_region")))?;
        (start_url, region, Vec::new())
    } else {
        return Ok(None);
    };
    if !start_url.starts_with("https://") {
        return Err(cfg(format!("profile {name}: sso_start_url must start with https://")));
    }
    let need = |k: &str| get(p, k).ok_or_else(|| cfg(format!("profile {name} uses SSO but has no {k}")));
    Ok(Some(Sso { start_url, sso_region, account_id: need("sso_account_id")?, role_name: need("sso_role_name")?, scopes }))
}

/// プロファイル自身のクレデンシャル(アクセスキー、無ければSSO)。
fn base_of(files: &Files, name: &str, p: &Section) -> Result<Base, AppError> {
    if let Some(keys) = keys_of(p) {
        return Ok(Base::Keys(keys));
    }
    match sso_of(files, name, p)? {
        Some(sso) => Ok(Base::Sso(sso)),
        None => Err(AppError::AwsConfig(no_keys_reason(name, p))),
    }
}

fn resolve(files: &Files, name: &str) -> Result<Resolved, AppError> {
    let cfg = |m: String| AppError::AwsConfig(m);
    let p = files.profiles.get(name).ok_or_else(|| cfg(format!("profile not found: {name}")))?;
    let Some(role_arn) = p.get("role_arn").filter(|v| !v.is_empty()) else {
        let source = match base_of(files, name, p)? {
            Base::Keys(keys) => Source::Static(keys),
            Base::Sso(sso) => Source::Sso(sso),
        };
        let region = p.get("region").cloned().unwrap_or_else(|| DEFAULT_REGION.to_owned());
        return Ok(Resolved { region, source });
    };
    let source_name = p
        .get("source_profile")
        .ok_or_else(|| cfg(format!("profile {name} has role_arn but no source_profile")))?;
    let source = files
        .profiles
        .get(source_name)
        .ok_or_else(|| cfg(format!("source profile not found: {source_name}")))?;
    let base = base_of(files, source_name, source).map_err(|e| cfg(format!("source profile {source_name}: {e}")))?;
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
    // SSOで得たセッションでは、MFAコード付きの引き受けはできない(MFAはSSOの承認側で求められる)
    let mfa_serial = match base {
        Base::Keys(_) => p.get("mfa_serial").filter(|v| !v.is_empty()).cloned(),
        Base::Sso(_) => None,
    };
    Ok(Resolved {
        region,
        source: Source::AssumeRole(AssumeRole {
            role_arn: role_arn.clone(),
            session_name: p.get("role_session_name").cloned().unwrap_or_else(|| "myassistant".to_owned()),
            external_id: p.get("external_id").cloned(),
            duration_secs,
            mfa_serial,
            source: base,
        }),
    })
}

/// 選択中のプロファイル。保存済みの選択が今も使えるときだけ返す(他のプロファイルへ黙って切り替えない)。
fn selected(ini: &Files, saved: Option<&str>) -> Option<String> {
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
    /// IAM Identity Center(SSO)で認証する(自身、またはロールの引き受け元がSSO)。
    pub sso: bool,
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
}

fn profiles_of(ini: &Files) -> Vec<ProfileInfo> {
    ini.profiles
        .iter()
        .map(|(name, p)| {
            let resolved = resolve(ini, name);
            ProfileInfo {
                name: name.clone(),
                region: resolved.as_ref().ok().map(|r| r.region.clone()),
                role: p.contains_key("role_arn"),
                mfa: resolved.as_ref().map_or_else(|_| p.contains_key("mfa_serial"), |r| r.source.mfa()),
                sso: resolved.as_ref().is_ok_and(|r| r.source.uses_sso()),
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
    Ok(Status { registered: ctx.secrets.has_aws()?, profiles, selected, session_expires_at })
}

/// 貼り付けた内容と、使うプロファイルを検証して保存する(ネットワークは使わない)。
/// 選んだプロファイルが使えなければ保存しない。
pub fn save(ctx: &WorkspaceCtx, config: &str, credentials: &str, profile: &str) -> Result<(), AppError> {
    resolve(&merged(config, credentials), profile)?;
    ctx.secrets.set_aws_files(config.trim(), credentials.trim())?;
    ctx.secrets.set_aws_profile(profile)?;
    reset(ctx);
    Ok(())
}

pub fn select_profile(ctx: &WorkspaceCtx, name: &str) -> Result<(), AppError> {
    let (config, credentials) = ctx.secrets.get_aws_files()?;
    resolve(&merged(&config, &credentials), name)?;
    ctx.secrets.set_aws_profile(name)?;
    clear_session(ctx);
    Ok(())
}

/// メモリ上の一時クレデンシャル・SSOのトークンと、MFA・SSOの入力待ちを破棄する(保存内容を差し替えたときに呼ぶ)。
pub fn reset(ctx: &WorkspaceCtx) {
    clear_session(ctx);
    sso_tokens(ctx).clear();
    cancel_mfa(ctx);
    cancel_sso(ctx);
}

pub fn delete(ctx: &WorkspaceCtx) -> Result<(), AppError> {
    ctx.secrets.delete_aws()?;
    reset(ctx);
    Ok(())
}

// ---- 一時クレデンシャル ----

struct Session {
    profile: String,
    keys: Keys,
    expires: SystemTime,
}

/// SSOのアクセストークン。メモリ上にだけ置く。
struct SsoToken {
    access_token: String,
    expires: SystemTime,
    /// 黙って更新するための情報(`sso_session`形式で、発行元が返したときだけ)。
    refresh: Option<SsoRefresh>,
}

#[derive(Clone)]
struct SsoRefresh {
    token: String,
    client_id: String,
    client_secret: String,
    /// クライアント登録の期限。過ぎたら承認からやり直す。
    client_expires: SystemTime,
}

/// ブラウザでの承認待ち。
struct PendingSso {
    /// 落とすと、待っている呼び出しが取り消される。
    _cancel: oneshot::Sender<()>,
    /// 承認ページのURL(発行元が返したもの)。画面のボタンから開く。
    url: String,
}

/// ワークスペースごとの一時状態(一時クレデンシャル・SSOのトークン・MFAとSSOの入力待ち)。
#[derive(Default)]
pub struct AwsState {
    session: Mutex<Option<Session>>,
    /// 同時に複数の呼び出しが来ても、MFAの入力・SSOの承認と取得は1回にまとめる。
    refresh: tokio::sync::Mutex<()>,
    pending_mfa: Mutex<Option<oneshot::Sender<String>>>,
    /// 発行元(`Sso::token_key`)ごとのトークン。
    sso_tokens: Mutex<HashMap<String, SsoToken>>,
    pending_sso: Mutex<Option<PendingSso>>,
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

/// 起動時に1回呼ぶ。MFAコードの入力要求とSSOの承認要求をフロントエンドへ送るために使う。
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

fn app() -> Result<&'static AppHandle, AppError> {
    APP.get().ok_or_else(|| AppError::AwsConfig("not initialized".into()))
}

/// エージェントの名称とイメージカラー(パーソナルはどちらも`None`)。
fn workspace_label(app: &AppHandle, ctx: &WorkspaceCtx) -> (Option<String>, Option<String>) {
    app.try_state::<super::workspace::Workspaces>().and_then(|ws| ws.label(&ctx.id)).unzip()
}

async fn ask_mfa(ctx: &WorkspaceCtx, mut prompt: MfaPrompt) -> Result<String, AppError> {
    let app = app()?;
    (prompt.workspace_name, prompt.workspace_color) = workspace_label(app, ctx);
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

async fn assume_role(ctx: &WorkspaceCtx, a: &AssumeRole, base: &Keys, region: &str, code: Option<&str>) -> Result<(Keys, SystemTime), Failure> {
    let mut conf = aws_sdk_sts::Config::builder()
        .behavior_version_latest()
        .region(Region::new(region.to_owned()))
        .credentials_provider(base.credentials());
    conf.set_http_client(super::proxy::aws_http_client_for(ctx));
    let conf = conf.build();
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
/// 引き受け元がSSOなら、先にSSOで引き受け元のクレデンシャルを得る(必要ならブラウザでの承認を求める)。
async fn assume_with_prompt(ctx: &WorkspaceCtx, profile: &str, a: &AssumeRole, region: &str) -> Result<Keys, AppError> {
    let base = match &a.source {
        Base::Keys(keys) => keys.clone(),
        Base::Sso(sso) => sso_role_keys(ctx, profile, sso).await?.0,
    };
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
        match assume_role(ctx, a, &base, region, code.as_deref()).await {
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

// ---- IAM Identity Center(SSO) ----

#[derive(Serialize, Clone)]
struct SsoPrompt {
    profile: String,
    /// ブラウザの承認ページに出るコード。画面に出して、利用者に見比べてもらう。
    user_code: String,
    /// 承認ページのURL(表示用)。開くときは、Rustが控えているURLを使う(`open_sso`)。
    verification_uri: String,
    /// 承認の期限(UNIX秒)。
    expires_at: i64,
    workspace_id: String,
    workspace_name: Option<String>,
    workspace_color: Option<String>,
}

fn sso_tokens(ctx: &WorkspaceCtx) -> MutexGuard<'_, HashMap<String, SsoToken>> {
    ctx.shared.aws.sso_tokens.lock().unwrap_or_else(|p| p.into_inner())
}

fn pending_sso(ctx: &WorkspaceCtx) -> MutexGuard<'_, Option<PendingSso>> {
    ctx.shared.aws.pending_sso.lock().unwrap_or_else(|p| p.into_inner())
}

/// SSOの承認待ちを取りやめる(待っている呼び出しは`SsoRequired`で失敗する)。
pub fn cancel_sso(ctx: &WorkspaceCtx) {
    pending_sso(ctx).take();
}

/// 承認待ちのSSOの承認ページを、既定のブラウザで開く。待っていなければ何もしない。
/// URLは発行元(SSO OIDC)が返したものだけを使い、フロントエンドからは受け取らない。
pub fn open_sso(ctx: &WorkspaceCtx) -> Result<(), AppError> {
    let Some(url) = pending_sso(ctx).as_ref().map(|p| p.url.clone()) else { return Ok(()) };
    if !url.starts_with("https://") {
        return Err(AppError::AwsConfig("unexpected SSO verification URL".to_owned()));
    }
    let status = std::process::Command::new("/usr/bin/open").arg(&url).status().map_err(|e| AppError::Io(e.to_string()))?;
    if status.success() { Ok(()) } else { Err(AppError::Io(format!("open failed: {status}"))) }
}

fn oidc_client(ctx: &WorkspaceCtx, sso: &Sso) -> aws_sdk_ssooidc::Client {
    let mut conf = aws_sdk_ssooidc::Config::builder().behavior_version_latest().region(Region::new(sso.sso_region.clone()));
    conf.set_http_client(super::proxy::aws_http_client_for(ctx));
    aws_sdk_ssooidc::Client::from_conf(conf.build())
}

/// AWS呼び出しを時間制限付きで行い、失敗を`AppError`へ直す。
async fn call<T, E: ProvideErrorMetadata, R>(fut: impl Future<Output = Result<T, SdkError<E, R>>>) -> Result<T, AppError> {
    tokio::time::timeout(STS_TIMEOUT, fut).await.map_err(|_| AppError::Timeout)?.map_err(|e| Failure::of(&e).into_error())
}

fn after_secs(secs: i32) -> SystemTime {
    SystemTime::now() + Duration::from_secs(u64::try_from(secs).unwrap_or(0))
}

fn unix_secs(t: SystemTime) -> i64 {
    t.duration_since(SystemTime::UNIX_EPOCH).ok().map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// 期限内のアクセストークン。無ければリフレッシュトークンで黙って更新し、それもできなければブラウザでの承認を求める。
async fn sso_access_token(ctx: &WorkspaceCtx, profile: &str, sso: &Sso) -> Result<String, AppError> {
    let key = sso.token_key();
    let refresh = {
        let tokens = sso_tokens(ctx);
        match tokens.get(&key) {
            Some(t) if t.expires > SystemTime::now() + EXPIRY_MARGIN => return Ok(t.access_token.clone()),
            Some(t) => t.refresh.clone(),
            None => None,
        }
    };
    let client = oidc_client(ctx, sso);
    let token = match refresh {
        Some(r) if r.client_expires > SystemTime::now() + EXPIRY_MARGIN => match sso_refresh(&client, &r).await {
            Ok(token) => token,
            // 通信の失敗は、承認のやり直しでは直らないのでそのまま返す
            Err(e @ (AppError::Timeout | AppError::Network)) => return Err(e),
            Err(_) => sso_authorize(ctx, &client, profile, sso).await?,
        },
        _ => sso_authorize(ctx, &client, profile, sso).await?,
    };
    let access_token = token.access_token.clone();
    sso_tokens(ctx).insert(key, token);
    Ok(access_token)
}

async fn sso_refresh(client: &aws_sdk_ssooidc::Client, r: &SsoRefresh) -> Result<SsoToken, AppError> {
    let out = call(
        client
            .create_token()
            .client_id(&r.client_id)
            .client_secret(&r.client_secret)
            .grant_type("refresh_token")
            .refresh_token(&r.token)
            .send(),
    )
    .await?;
    let access_token = out.access_token().ok_or(AppError::BadResponse)?.to_owned();
    // 更新のたびにリフレッシュトークンが替わることがある。返らなければ今のものを使い続ける
    let token = out.refresh_token().map_or_else(|| r.token.clone(), str::to_owned);
    Ok(SsoToken { access_token, expires: after_secs(out.expires_in()), refresh: Some(SsoRefresh { token, ..r.clone() }) })
}

/// デバイス認可フロー。画面に確認コードを出し、利用者がブラウザで承認するまで待つ。
async fn sso_authorize(ctx: &WorkspaceCtx, client: &aws_sdk_ssooidc::Client, profile: &str, sso: &Sso) -> Result<SsoToken, AppError> {
    let app = app()?;
    let mut register = client.register_client().client_name(SSO_CLIENT_NAME).client_type("public");
    if !sso.scopes.is_empty() {
        register = register.set_scopes(Some(sso.scopes.clone()));
    }
    let registered = call(register.send()).await?;
    let client_id = registered.client_id().ok_or(AppError::BadResponse)?.to_owned();
    let client_secret = registered.client_secret().ok_or(AppError::BadResponse)?.to_owned();
    let client_expires =
        SystemTime::UNIX_EPOCH + Duration::from_secs(u64::try_from(registered.client_secret_expires_at()).unwrap_or(0));

    let device = call(
        client.start_device_authorization().client_id(&client_id).client_secret(&client_secret).start_url(&sso.start_url).send(),
    )
    .await?;
    let device_code = device.device_code().ok_or(AppError::BadResponse)?.to_owned();
    let user_code = device.user_code().ok_or(AppError::BadResponse)?.to_owned();
    let verification_uri = device.verification_uri().ok_or(AppError::BadResponse)?.to_owned();
    let url = device.verification_uri_complete().unwrap_or(&verification_uri).to_owned();
    let lifetime = Duration::from_secs(u64::try_from(device.expires_in()).unwrap_or(0));
    let mut interval = match device.interval() {
        secs if secs > 0 => Duration::from_secs(u64::try_from(secs).unwrap_or(5)),
        _ => SSO_POLL_INTERVAL,
    };

    let (tx, mut cancelled) = oneshot::channel::<()>();
    *pending_sso(ctx) = Some(PendingSso { _cancel: tx, url });
    let (workspace_name, workspace_color) = workspace_label(app, ctx);
    let prompt = SsoPrompt {
        profile: profile.to_owned(),
        user_code,
        verification_uri,
        expires_at: unix_secs(SystemTime::now() + lifetime),
        workspace_id: ctx.id.clone(),
        workspace_name,
        workspace_color,
    };
    let deadline = Instant::now() + lifetime;
    let result = match app.emit(SSO_EVENT, prompt) {
        Err(e) => Err(AppError::AwsConfig(e.to_string())),
        Ok(()) => loop {
            tokio::select! {
                // 取り消し(送り手が落とされた)
                _ = &mut cancelled => break Err(AppError::SsoRequired),
                () = tokio::time::sleep(interval) => {}
            }
            if Instant::now() >= deadline {
                break Err(AppError::SsoRequired);
            }
            let poll = client
                .create_token()
                .client_id(&client_id)
                .client_secret(&client_secret)
                .grant_type("urn:ietf:params:oauth:grant-type:device_code")
                .device_code(&device_code)
                .send();
            match tokio::time::timeout(STS_TIMEOUT, poll).await {
                Ok(Ok(out)) => break Ok(out),
                // 一時的な通信の失敗では、期限まで待ち続ける
                Err(_) => {}
                Ok(Err(e)) => match e.as_service_error() {
                    Some(s) if s.is_authorization_pending_exception() => {}
                    Some(s) if s.is_slow_down_exception() => interval += SSO_POLL_INTERVAL,
                    Some(s) if s.is_expired_token_exception() => break Err(AppError::SsoRequired),
                    _ => match Failure::of(&e) {
                        Failure::Timeout | Failure::Network => {}
                        f => break Err(f.into_error()),
                    },
                },
            }
        },
    };
    pending_sso(ctx).take();
    let _ = app.emit(SSO_FINISHED_EVENT, ctx.id.clone());
    let out = result?;
    let access_token = out.access_token().ok_or(AppError::BadResponse)?.to_owned();
    let refresh = out.refresh_token().map(|token| SsoRefresh { token: token.to_owned(), client_id, client_secret, client_expires });
    Ok(SsoToken { access_token, expires: after_secs(out.expires_in()), refresh })
}

/// SSOのロールの一時クレデンシャルと期限。トークンが無効と言われたら、1回だけ承認からやり直す。
async fn sso_role_keys(ctx: &WorkspaceCtx, profile: &str, sso: &Sso) -> Result<(Keys, SystemTime), AppError> {
    let mut conf = aws_sdk_sso::Config::builder().behavior_version_latest().region(Region::new(sso.sso_region.clone()));
    conf.set_http_client(super::proxy::aws_http_client_for(ctx));
    let client = aws_sdk_sso::Client::from_conf(conf.build());
    for attempt in 0..2 {
        let token = sso_access_token(ctx, profile, sso).await?;
        let req = client.get_role_credentials().account_id(&sso.account_id).role_name(&sso.role_name).access_token(token).send();
        let out = match tokio::time::timeout(STS_TIMEOUT, req).await.map_err(|_| AppError::Timeout)? {
            Ok(out) => out,
            Err(e) if attempt == 0 && e.as_service_error().is_some_and(|s| s.is_unauthorized_exception()) => {
                sso_tokens(ctx).remove(&sso.token_key());
                continue;
            }
            Err(e) => return Err(Failure::of(&e).into_error()),
        };
        let c = out.role_credentials().ok_or(AppError::BadResponse)?;
        let keys = Keys {
            access_key_id: c.access_key_id().ok_or(AppError::BadResponse)?.to_owned(),
            secret_access_key: c.secret_access_key().ok_or(AppError::BadResponse)?.to_owned(),
            session_token: c.session_token().map(str::to_owned),
        };
        // 期限はUNIXミリ秒
        let expires = SystemTime::UNIX_EPOCH + Duration::from_millis(u64::try_from(c.expiration()).unwrap_or(0));
        return Ok((keys, expires));
    }
    unreachable!("the second attempt always returns")
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
        temporary => match cached(ctx, &name) {
            Some(keys) => keys,
            None => {
                let _guard = ctx.shared.aws.refresh.lock().await;
                match (cached(ctx, &name), &temporary) {
                    (Some(keys), _) => keys,
                    (None, Source::Static(keys)) => keys.clone(),
                    (None, Source::AssumeRole(a)) => assume_with_prompt(ctx, &name, a, &region).await?,
                    (None, Source::Sso(sso)) => {
                        let (keys, expires) = sso_role_keys(ctx, &name, sso).await?;
                        *session(ctx) = Some(Session { profile: name.clone(), keys: keys.clone(), expires });
                        keys
                    }
                }
            }
        },
    };
    Ok((keys, region))
}

/// サンドボックスのコマンドへ渡す、選択中のプロファイルの認証情報(環境変数の名前と値)。
/// ロールを引き受けるプロファイルとSSOのプロファイルは、通常のBedrock呼び出しと同じ流れ(MFA入力・SSOの承認・メモリ上のキャッシュ)で一時キーを得る。
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

/// 認証(選択中のプロファイル・MFA・SSO・ロールの引き受け)が通るかを、STSのGetCallerIdentityで確かめる。MFAやSSOの承認が必要なら画面で求める。
/// モデルは呼ばない(Bedrockは、コンテナ内の Claude Code が settings.json のモデルで呼ぶ)。
pub async fn test_connection(ctx: &WorkspaceCtx) -> Result<(), AppError> {
    for attempt in 0..2 {
        let (keys, region) = credentials(ctx).await?;
        match caller_identity(ctx, &keys, &region).await {
            Err(f) if f.expired() && attempt == 0 => clear_session(ctx),
            result => return result.map_err(Failure::into_error),
        }
    }
    unreachable!("the second attempt always returns")
}

async fn caller_identity(ctx: &WorkspaceCtx, keys: &Keys, region: &str) -> Result<(), Failure> {
    let mut conf = aws_sdk_sts::Config::builder()
        .behavior_version_latest()
        .region(Region::new(region.to_owned()))
        .credentials_provider(keys.credentials());
    conf.set_http_client(super::proxy::aws_http_client_for(ctx));
    let conf = conf.build();
    tokio::time::timeout(STS_TIMEOUT, aws_sdk_sts::Client::from_conf(conf).get_caller_identity().send())
        .await
        .map_err(|_| Failure::Timeout)?
        .map_err(|e| Failure::of(&e))?;
    Ok(())
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
sso_start_url = https://corp.awsapps.com/start
sso_region = us-east-1

[profile dev]
sso_session = corp
sso_account_id = 222222222222
sso_role_name = Developer
region = us-west-2
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
        let files = merged(CONFIG, CREDENTIALS);
        let ini = &files.profiles;
        assert_eq!(ini["default"]["region"], "ap-northeast-1");
        assert_eq!(ini["admin"]["role_arn"], "arn:aws:iam::123456789012:role/Admin");
        assert_eq!(ini["plain"]["aws_session_token"], "tok");
        assert!(!ini.contains_key("sso-session corp") && !ini.contains_key("corp"));
        assert!(!ini.contains_key("profile admin"));
        assert_eq!(files.sso_sessions["corp"]["sso_region"], "us-east-1");
    }

    #[test]
    fn credentials_file_overrides_config() {
        let ini = merged("[p]\nregion = a\naws_access_key_id = old\n", "[p]\naws_access_key_id = new\n").profiles;
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
        assert!(matches!(&a.source, Base::Keys(k) if k.access_key_id == "AKIABASE"));
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
        assert!(reason("[profile p]\ncredential_source = Environment\n", "", "p").unwrap().contains("credential_source"));
        // ロールの連鎖
        let chain = "[profile a]\nrole_arn = arn\nsource_profile = b\n[profile b]\nrole_arn = arn2\nsource_profile = base\n";
        assert!(reason(chain, keys, "a").unwrap().contains("chained roles"));
        // 何も書かれていない
        assert!(reason("[profile e]\nregion = x\n", "", "e").unwrap().contains("no access keys"));
    }

    #[test]
    fn resolves_sso_profiles() {
        let ini = merged(CONFIG, CREDENTIALS);
        let dev = resolve(&ini, "dev").unwrap();
        assert_eq!(dev.region, "us-west-2");
        let Source::Sso(s) = dev.source else { panic!("expected sso") };
        assert_eq!(s.start_url, "https://corp.awsapps.com/start");
        assert_eq!(s.sso_region, "us-east-1");
        assert_eq!((s.account_id.as_str(), s.role_name.as_str()), ("222222222222", "Developer"));
        assert_eq!(s.scopes, [SSO_DEFAULT_SCOPE]);

        // スコープの指定
        let scoped = merged(
            "[profile p]\nsso_session = s\nsso_account_id = 1\nsso_role_name = r\n\
             [sso-session s]\nsso_start_url = https://x/start\nsso_region = eu-west-1\nsso_registration_scopes = a, b\n",
            "",
        );
        let Source::Sso(s) = resolve(&scoped, "p").unwrap().source else { panic!("expected sso") };
        assert_eq!(s.scopes, ["a", "b"]);

        // プロファイルへ直接書く旧形式(スコープは付けない)
        let legacy = merged(
            "[profile old]\nsso_start_url = https://old.awsapps.com/start\nsso_region = ap-northeast-1\nsso_account_id = 1\nsso_role_name = r\n",
            "",
        );
        let old = resolve(&legacy, "old").unwrap();
        assert_eq!(old.region, DEFAULT_REGION);
        let Source::Sso(s) = old.source else { panic!("expected sso") };
        assert_eq!(s.sso_region, "ap-northeast-1");
        assert!(s.scopes.is_empty());
    }

    #[test]
    fn resolves_role_assumed_from_sso() {
        let ini = merged(
            &format!("{CONFIG}\n[profile via]\nrole_arn = arn:aws:iam::333333333333:role/R\nsource_profile = dev\nmfa_serial = arn:mfa\n"),
            CREDENTIALS,
        );
        let via = resolve(&ini, "via").unwrap();
        // リージョンは引き受け元から引き継ぐ
        assert_eq!(via.region, "us-west-2");
        assert!(via.source.uses_sso() && !via.source.mfa());
        let Source::AssumeRole(a) = via.source else { panic!("expected assume role") };
        assert!(matches!(&a.source, Base::Sso(s) if s.role_name == "Developer"));
        // SSOが引き受け元のときは、MFAコードを聞かない
        assert_eq!(a.mfa_serial, None);

        let info = preview(&format!("{CONFIG}\n[profile via]\nrole_arn = arn\nsource_profile = dev\n"), CREDENTIALS);
        let get = |n: &str| info.iter().find(|p| p.name == n).unwrap();
        assert!(get("dev").usable && get("dev").sso && !get("dev").role);
        assert!(get("via").usable && get("via").sso && get("via").role && !get("via").mfa);
        assert!(!get("admin").sso && !get("plain").sso);
    }

    #[test]
    fn explains_incomplete_sso_settings() {
        let reason = |config: &str| preview(config, "").into_iter().find(|p| p.name == "p").unwrap().reason.unwrap();
        assert_eq!(reason("[profile p]\nsso_session = s\n"), "sso-session not found: s");
        assert!(reason("[profile p]\nsso_session = s\n[sso-session s]\nsso_region = r\n").contains("no sso_start_url"));
        assert!(reason("[profile p]\nsso_start_url = https://x\n").contains("no sso_region"));
        assert!(reason("[profile p]\nsso_start_url = http://x\nsso_region = r\n").contains("https://"));
        assert!(reason("[profile p]\nsso_start_url = https://x\nsso_region = r\nsso_role_name = n\n").contains("no sso_account_id"));
        assert!(reason("[profile p]\nsso_start_url = https://x\nsso_region = r\nsso_account_id = 1\n").contains("no sso_role_name"));
        // 引き受け元のSSO設定の不備は、どのプロファイルの問題かを添える
        let chain = "[profile p]\nrole_arn = arn\nsource_profile = s\n[profile s]\nsso_session = gone\n";
        assert_eq!(reason(chain), "source profile s: sso-session not found: gone");
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
    fn expired_token_is_detected() {
        let f = Failure::Service { code: "ExpiredTokenException".into(), message: String::new() };
        assert!(f.expired());
        assert!(!Failure::Timeout.expired());
    }
}
