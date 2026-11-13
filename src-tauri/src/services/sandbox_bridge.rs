//! サンドボックスのコンテナとホストの間の中継(ホスト側)。AWSの認証情報をコンテナへ直接渡さないために使う。
//!
//! - ホストの`127.0.0.1`の空いているポートで待ち受ける(コンテナからは`host.docker.internal`で届く)。最初に使うときに起動する。
//! - 実行(`docker exec`)ごとにランダムなトークンを発行し、実行が終わると無効にする(`Grant`を落としたとき)。
//! - Bedrock: コンテナ内の Claude Code は、トークン入りのURL(`ANTHROPIC_BEDROCK_BASE_URL`)へ署名せずに送り(`CLAUDE_CODE_SKIP_BEDROCK_AUTH`)、
//!   ここでBedrock用のAWS設定で署名して転送する。コンテナにはBedrock用のキーが入らない。
//!   通すのはモデルの呼び出し(`invoke`・`invoke-with-response-stream`・`count-tokens`)と推論プロファイルの一覧(`GET /inference-profiles`)だけで、
//!   モデルは settings.json などで指定したもの(`ModelPolicy`)に限る。
//! - コンテナ用のAWS: コンテナのAWS CLI・SDKは、`credential_process`(curl)でここから一時キーを取る(`/aws/credentials`。トークンは`Authorization`ヘッダー)。
//!   期限が近づけばCLI・SDKが自動で取り直し、MFA・SSOの承認が必要ならその時点で画面で求める。コンテナに入るのは一時キーだけ。
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, SystemTime},
};

use bytes::Bytes;
use futures_util::TryStreamExt;
use http_body_util::{BodyExt, Full, Limited, StreamBody, combinators::BoxBody};
use hyper::{
    Method, Request, Response, StatusCode,
    body::{Frame, Incoming},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use tauri::{AppHandle, Manager};

use super::{
    aws::{self, AwsSlot},
    workspace::{WorkspaceCtx, Workspaces},
};
use crate::utils::error::AppError;

/// コンテナから見たホストの名前(Docker Desktop が用意する)。
pub(crate) const HOST_FROM_CONTAINER: &str = "host.docker.internal";
/// コンテナ内に置く、コンテナ用のAWS設定のフォルダ(`/tmp`はコンテナのtmpfs)。
pub const AWS_DIR: &str = "/tmp/myassistant-aws";
/// トークンを渡す環境変数(値は伏せ字の対象)。
const TOKEN_VAR: &str = "MA_AWS_TOKEN";
/// 一時キーを取りに行くURLを渡す環境変数(秘密ではない)。
const URL_VAR: &str = "MYASSISTANT_AWS_CREDENTIALS_URL";
/// 受け付けるリクエストの本文の上限(画像を含む会話を想定)。
const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;
/// Bedrockへの接続の待ち時間(応答は長く続くので、全体の時間は区切らない)。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// Bedrockのモデルの呼び出しのうち、通すもの。
const MODEL_OPERATIONS: [&str; 3] = ["invoke", "invoke-with-response-stream", "count-tokens"];
/// Bedrockの応答のうち、コンテナへ返すヘッダー。
const PASS_HEADERS: [&str; 3] = ["content-type", "x-amzn-requestid", "x-amzn-errortype"];

/// コンテナのAWS CLI・SDKが読む設定(`AWS_CONFIG_FILE`)。一時キーは`credential_process`でホストから取る。
/// 失敗したときは、ホストが返した理由を標準エラーへ出す(CLI・SDKはそれをエラーの文面に使う)。
const AWS_CONFIG: &str = r#"[default]
credential_process = sh -c 'out=$(curl -sS --max-time 900 --fail-with-body -H "Authorization: Bearer $MA_AWS_TOKEN" "$MYASSISTANT_AWS_CREDENTIALS_URL") || { echo "MyAssistant: $out" >&2; exit 1; }; printf "%s" "$out"'
"#;

/// コンテナ内に、コンテナ用のAWS設定を置くスクリプト。`$1`に設定の内容を渡す。
/// `~/.aws/credentials`のキーが混ざらないよう、credentials は空のファイルにする。
pub const AWS_SETUP_SCRIPT: &str = r#"d=/tmp/myassistant-aws; umask 077; mkdir -p "$d" && printf '%s' "$1" > "$d/config.$$" && mv -f "$d/config.$$" "$d/config" && : > "$d/credentials""#;

/// `AWS_SETUP_SCRIPT`へ渡す設定の内容。
pub fn aws_config() -> &'static str {
    AWS_CONFIG
}

/// 呼び出してよいBedrockのモデル。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelPolicy {
    /// Claude のモデルなら呼べる(settings.json でモデルを ID で指定していないとき)。
    AnyClaude,
    /// 指定したモデルと、Claude Code が補助に使う Haiku のモデルだけ。
    Only(Vec<String>),
}

impl ModelPolicy {
    /// 設定で指定されたモデルから決める。どれもBedrockのID(またはARN)なら、それに限る。
    /// 1つでも別名(`sonnet`など。Claude Code がIDへ直す)があれば、Claude のモデルなら通す。
    pub fn from_configured(models: impl IntoIterator<Item = String>) -> Self {
        let mut list: Vec<String> = Vec::new();
        for m in models {
            let m = m.trim().to_owned();
            if !m.is_empty() && !list.contains(&m) {
                list.push(m);
            }
        }
        if list.is_empty() || list.iter().any(|m| !looks_like_bedrock_id(m)) {
            Self::AnyClaude
        } else {
            Self::Only(list)
        }
    }

    fn allows(&self, model: &str) -> bool {
        let claude = model.contains("anthropic.");
        match self {
            Self::AnyClaude => claude,
            Self::Only(list) => list.iter().any(|m| m == model) || (claude && model.contains("haiku")),
        }
    }
}

/// BedrockのモデルID・推論プロファイルID(`us.anthropic.claude-...`)またはARNの形か。
fn looks_like_bedrock_id(m: &str) -> bool {
    m.starts_with("arn:") || m.contains('.')
}

/// 配った一時キー(実行の出力で伏せるため)。
type Handed = Arc<Mutex<Vec<String>>>;

/// トークンの用途。
#[derive(Clone)]
enum Kind {
    Bedrock(ModelPolicy),
    Aws(Handed),
}

#[derive(Clone)]
struct Entry {
    workspace_id: String,
    kind: Kind,
}

struct Bridge {
    port: u16,
    grants: Mutex<HashMap<String, Entry>>,
}

static APP: OnceLock<AppHandle> = OnceLock::new();
static BRIDGE: tokio::sync::OnceCell<Bridge> = tokio::sync::OnceCell::const_new();

/// 起動時に1回呼ぶ。トークンからワークスペースを開くために使う。
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

fn grants() -> Option<std::sync::MutexGuard<'static, HashMap<String, Entry>>> {
    BRIDGE.get().map(|b| b.grants.lock().unwrap_or_else(|p| p.into_inner()))
}

/// 待ち受けを(まだなら)始めて、ポートを返す。
async fn bridge() -> Result<&'static Bridge, String> {
    BRIDGE
        .get_or_try_init(|| async {
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
                .await
                .map_err(|e| format!("サンドボックスの中継を起動できませんでした: {e}"))?;
            let port = listener.local_addr().map_err(|e| e.to_string())?.port();
            tauri::async_runtime::spawn(serve(listener));
            Ok(Bridge { port, grants: Mutex::new(HashMap::new()) })
        })
        .await
}

async fn serve(listener: tokio::net::TcpListener) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        };
        tauri::async_runtime::spawn(async move {
            let service = service_fn(|req| async move { Ok::<_, std::convert::Infallible>(handle(req).await) });
            let _ = http1::Builder::new().serve_connection(TokioIo::new(stream), service).await;
        });
    }
}

/// 発行したトークン。落とすと無効になる(実行が終わるまで持っておく)。
pub struct Grant {
    token: String,
    handed: Option<Handed>,
}

impl Grant {
    /// コンテナ用のAWS設定をコンテナ内に置く必要があるか。
    pub fn needs_aws_files(&self) -> bool {
        self.handed.is_some()
    }

    /// このトークンで配った一時キー(アクセスキーID・シークレット・セッショントークン)。出力の伏せ字に使う。
    pub fn handed_values(&self) -> Vec<String> {
        self.handed.as_ref().map(|h| h.lock().unwrap_or_else(|p| p.into_inner()).clone()).unwrap_or_default()
    }
}

impl Drop for Grant {
    fn drop(&mut self) {
        if let Some(mut g) = grants() {
            g.remove(&self.token);
        }
    }
}

fn new_token() -> String {
    // UUID v4 は OS の乱数から作る。2つつないで 244 ビット
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

async fn issue(ctx: &WorkspaceCtx, kind: Kind) -> Result<(Grant, u16, String), String> {
    let b = bridge().await?;
    let token = new_token();
    let handed = match &kind {
        Kind::Aws(h) => Some(h.clone()),
        Kind::Bedrock(_) => None,
    };
    b.grants.lock().unwrap_or_else(|p| p.into_inner()).insert(token.clone(), Entry { workspace_id: ctx.id.clone(), kind });
    Ok((Grant { token: token.clone(), handed }, b.port, token))
}

/// コンテナ内の Claude Code が Bedrock を使うための環境変数。Bedrock用のAWS設定が無ければ失敗する。
/// 認証(MFA・SSOの承認を含む)は実行の前に済ませておく(長い会話の途中で止まらないように)。
pub async fn bedrock_env(ctx: &WorkspaceCtx, policy: ModelPolicy) -> Result<(Grant, BTreeMap<String, String>), String> {
    if !ctx.secrets.has_aws(AwsSlot::Bedrock).unwrap_or(false) {
        return Err("Bedrock の認証情報がありません。このエージェントに Amazon Bedrock の AWS 設定を登録してください。".into());
    }
    aws::bedrock_signing(ctx).await.map_err(|e| format!("Bedrock の認証情報を取得できませんでした: {e}"))?;
    let (grant, port, token) = issue(ctx, Kind::Bedrock(policy)).await?;
    let mut vars = BTreeMap::new();
    vars.insert("CLAUDE_CODE_SKIP_BEDROCK_AUTH".to_owned(), "1".to_owned());
    vars.insert("ANTHROPIC_BEDROCK_BASE_URL".to_owned(), format!("http://{HOST_FROM_CONTAINER}:{port}/bedrock/{token}"));
    Ok((grant, vars))
}

/// コンテナのコマンドが AWS を使うための環境変数。コンテナ用のAWS設定が無ければ`None`(AWSの認証情報は渡さない)。
/// 一時キーは、コマンドがAWSを使うときに取りに来る(MFA・SSOの承認もそのときに求める)。
pub async fn aws_env(ctx: &WorkspaceCtx) -> Result<Option<(Grant, BTreeMap<String, String>)>, String> {
    if !ctx.secrets.has_aws(AwsSlot::Sandbox).unwrap_or(false) {
        return Ok(None);
    }
    let region = aws::selected_region(ctx, AwsSlot::Sandbox)
        .ok_or("コンテナ用の AWS 設定で、使うプロファイルが選ばれていないか、使えない状態です。Sandbox の設定を確認してください。")?;
    let (grant, port, token) = issue(ctx, Kind::Aws(Handed::default())).await?;
    let mut vars = BTreeMap::new();
    vars.insert("AWS_CONFIG_FILE".to_owned(), format!("{AWS_DIR}/config"));
    vars.insert("AWS_SHARED_CREDENTIALS_FILE".to_owned(), format!("{AWS_DIR}/credentials"));
    vars.insert("AWS_REGION".to_owned(), region.clone());
    vars.insert("AWS_DEFAULT_REGION".to_owned(), region);
    vars.insert(TOKEN_VAR.to_owned(), token);
    vars.insert(URL_VAR.to_owned(), format!("http://{HOST_FROM_CONTAINER}:{port}/aws/credentials"));
    Ok(Some((grant, vars)))
}

// ------------------------------------------------------------------ リクエストの処理

type Body = BoxBody<Bytes, Box<dyn std::error::Error + Send + Sync>>;

/// 受け付けるリクエスト。
#[derive(Debug, PartialEq, Eq)]
enum Route {
    /// `POST /bedrock/{token}/model/{model}/{operation}`。`model`は復号したもの。
    Model { token: String, model: String, operation: String },
    /// `GET /bedrock/{token}/inference-profiles?...`。`query`はそのまま。
    InferenceProfiles { token: String, query: String },
    /// `GET /aws/credentials`(トークンは`Authorization`ヘッダー)。
    AwsCredentials,
}

/// クエリに許す文字(Bedrockの一覧の指定だけを想定)。
fn safe_query(q: &str) -> bool {
    q.len() <= 512 && q.bytes().all(|b| b.is_ascii_alphanumeric() || b"=&_-.%".contains(&b))
}

fn route(method: &Method, path: &str, query: Option<&str>) -> Option<Route> {
    if method == Method::GET && path == "/aws/credentials" {
        return Some(Route::AwsCredentials);
    }
    let rest = path.strip_prefix("/bedrock/")?;
    let (token, rest) = rest.split_once('/')?;
    let token = token.to_owned();
    if method == Method::GET && rest == "inference-profiles" {
        let query = query.unwrap_or_default();
        return safe_query(query).then(|| Route::InferenceProfiles { token, query: query.to_owned() });
    }
    if method != Method::POST {
        return None;
    }
    let rest = rest.strip_prefix("model/")?;
    let (model, operation) = rest.rsplit_once('/')?;
    let model = percent_decode_str(model).decode_utf8().ok()?.into_owned();
    if model.is_empty() || model.len() > 512 || model.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    MODEL_OPERATIONS.contains(&operation).then(|| Route::Model { token, model, operation: operation.to_owned() })
}

/// URLのパスの1区間として使うときに符号化する文字(英数字と`-._~`以外)。
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');

fn message(status: StatusCode, text: &str) -> Response<Body> {
    let body = serde_json::json!({ "message": text }).to_string();
    let mut res = Response::new(Full::new(Bytes::from(body)).map_err(|e| match e {}).boxed());
    *res.status_mut() = status;
    res.headers_mut().insert("content-type", hyper::header::HeaderValue::from_static("application/json"));
    res
}

fn lookup(token: &str) -> Option<Entry> {
    grants().and_then(|g| g.get(token).cloned())
}

/// テストで、アプリを起動せずにワークスペースを引けるようにする。
#[cfg(test)]
static TEST_WORKSPACES: Mutex<Vec<Arc<WorkspaceCtx>>> = Mutex::new(Vec::new());

#[cfg(test)]
pub fn register_for_test(ctx: Arc<WorkspaceCtx>) {
    TEST_WORKSPACES.lock().unwrap_or_else(|p| p.into_inner()).push(ctx);
}

fn workspace(id: &str) -> Result<Arc<WorkspaceCtx>, AppError> {
    #[cfg(test)]
    if let Some(ctx) = TEST_WORKSPACES.lock().unwrap_or_else(|p| p.into_inner()).iter().find(|c| c.id == id) {
        return Ok(ctx.clone());
    }
    let app = APP.get().ok_or(AppError::AgentNotFound)?;
    app.try_state::<Workspaces>().ok_or(AppError::AgentNotFound)?.resolve(id)
}

async fn handle(req: Request<Incoming>) -> Response<Body> {
    let Some(r) = route(req.method(), req.uri().path(), req.uri().query()) else {
        return message(StatusCode::NOT_FOUND, "not found");
    };
    match r {
        Route::AwsCredentials => {
            let token = req
                .headers()
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .unwrap_or_default();
            match lookup(token) {
                Some(Entry { workspace_id, kind: Kind::Aws(handed) }) => aws_credentials(&workspace_id, &handed).await,
                _ => message(StatusCode::FORBIDDEN, "この実行では使えないトークンです(実行が終わると無効になります)。"),
            }
        }
        Route::InferenceProfiles { token, query } => match lookup(&token) {
            Some(Entry { workspace_id, kind: Kind::Bedrock(_) }) => {
                forward(&workspace_id, Method::GET, Target::Control { query }, Vec::new(), Bytes::new()).await
            }
            _ => message(StatusCode::FORBIDDEN, "この実行では使えないトークンです。"),
        },
        Route::Model { token, model, operation } => {
            let Some(Entry { workspace_id, kind: Kind::Bedrock(policy) }) = lookup(&token) else {
                return message(StatusCode::FORBIDDEN, "この実行では使えないトークンです。");
            };
            if !policy.allows(&model) {
                return message(
                    StatusCode::FORBIDDEN,
                    &format!("MyAssistant: モデル {model} は許可されていません。Sandbox の settings.json の model・availableModels に追加してください。"),
                );
            }
            let headers = forwarded_headers(req.headers());
            let body = match Limited::new(req.into_body(), MAX_BODY_BYTES).collect().await {
                Ok(b) => b.to_bytes(),
                Err(_) => return message(StatusCode::PAYLOAD_TOO_LARGE, "リクエストが大きすぎます。"),
            };
            forward(&workspace_id, Method::POST, Target::Model { model, operation }, headers, body).await
        }
    }
}

/// Claude Code が付けたヘッダーのうち、Bedrockへ渡すもの(認証・接続に関わるものは除く)。
fn forwarded_headers(h: &hyper::HeaderMap) -> Vec<(String, String)> {
    h.iter()
        .filter(|(k, _)| {
            let k = k.as_str();
            k == "content-type" || k == "accept" || k.starts_with("anthropic-") || k.starts_with("x-amzn-bedrock-")
        })
        .filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned())))
        .collect()
}

async fn aws_credentials(workspace_id: &str, handed: &Handed) -> Response<Body> {
    let ctx = match workspace(workspace_id) {
        Ok(ctx) => ctx,
        Err(_) => return message(StatusCode::NOT_FOUND, "エージェントが見つかりません。"),
    };
    match aws::handout_json(&ctx).await {
        Ok((json, values)) => {
            let mut h = handed.lock().unwrap_or_else(|p| p.into_inner());
            for v in values {
                if !h.contains(&v) {
                    h.push(v);
                }
            }
            drop(h);
            let mut res = Response::new(Full::new(Bytes::from(json)).map_err(|e| match e {}).boxed());
            res.headers_mut().insert("content-type", hyper::header::HeaderValue::from_static("application/json"));
            res.headers_mut().insert("cache-control", hyper::header::HeaderValue::from_static("no-store"));
            res
        }
        Err(e) => message(StatusCode::BAD_GATEWAY, &format!("コンテナ用の AWS 認証情報を取得できませんでした: {e}")),
    }
}

/// 転送先。
enum Target {
    /// bedrock-runtime のモデルの呼び出し。
    Model { model: String, operation: String },
    /// bedrock(コントロールプレーン)の推論プロファイルの一覧。
    Control { query: String },
}

fn valid_region(r: &str) -> bool {
    !r.is_empty() && r.len() <= 32 && r.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn target_url(target: &Target, region: &str) -> String {
    match target {
        Target::Model { model, operation } => {
            format!("https://bedrock-runtime.{region}.amazonaws.com/model/{}/{operation}", utf8_percent_encode(model, SEGMENT))
        }
        Target::Control { query } if query.is_empty() => format!("https://bedrock.{region}.amazonaws.com/inference-profiles"),
        Target::Control { query } => format!("https://bedrock.{region}.amazonaws.com/inference-profiles?{query}"),
    }
}

/// Bedrock用のAWS設定で署名して転送し、応答をそのまま(逐次)返す。
async fn forward(workspace_id: &str, method: Method, target: Target, headers: Vec<(String, String)>, body: Bytes) -> Response<Body> {
    let ctx = match workspace(workspace_id) {
        Ok(ctx) => ctx,
        Err(_) => return message(StatusCode::NOT_FOUND, "エージェントが見つかりません。"),
    };
    let (credentials, region) = match aws::bedrock_signing(&ctx).await {
        Ok(found) => found,
        Err(e) => return message(StatusCode::FORBIDDEN, &format!("MyAssistant: Bedrock の認証情報を取得できませんでした: {e}")),
    };
    if !valid_region(&region) {
        return message(StatusCode::BAD_REQUEST, "Bedrock のリージョンが正しくありません。");
    }
    let url = target_url(&target, &region);
    let signed = match sign(method.as_str(), &url, &headers, &body, credentials, &region) {
        Ok(h) => h,
        Err(e) => return message(StatusCode::INTERNAL_SERVER_ERROR, &format!("署名できませんでした: {e}")),
    };
    let client = match super::proxy::client_builder_for(&ctx).connect_timeout(CONNECT_TIMEOUT).build() {
        Ok(c) => c,
        Err(_) => return message(StatusCode::BAD_GATEWAY, "通信の準備に失敗しました。"),
    };
    let reqwest_method = if method == Method::GET { reqwest::Method::GET } else { reqwest::Method::POST };
    let mut rb = client.request(reqwest_method, &url).body(body);
    for (k, v) in headers.iter().chain(signed.iter()) {
        rb = rb.header(k.as_str(), v.as_str());
    }
    let res = match rb.send().await {
        Ok(r) => r,
        Err(e) => return message(StatusCode::BAD_GATEWAY, &format!("Bedrock へ接続できませんでした: {e}")),
    };
    let status = StatusCode::from_u16(res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut builder = Response::builder().status(status);
    for name in PASS_HEADERS {
        if let Some(v) = res.headers().get(name).and_then(|v| v.to_str().ok()) {
            builder = builder.header(name, v);
        }
    }
    // 一時キーの期限切れは、次の呼び出しで取り直す(Claude Code は再送する)
    if res.headers().get("x-amzn-errortype").and_then(|v| v.to_str().ok()).is_some_and(|t| t.contains("ExpiredToken")) {
        aws::invalidate(&ctx, AwsSlot::Bedrock);
    }
    let stream = res.bytes_stream().map_ok(Frame::data).map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>);
    builder
        .body(BodyExt::boxed(StreamBody::new(stream)))
        .unwrap_or_else(|_| message(StatusCode::BAD_GATEWAY, "応答を返せませんでした。"))
}

/// SigV4 で署名し、付けるヘッダーを返す。
fn sign(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: &[u8],
    credentials: aws_credential_types::Credentials,
    region: &str,
) -> Result<Vec<(String, String)>, String> {
    use aws_sigv4::{
        http_request::{SignableBody, SignableRequest, SigningSettings, sign},
        sign::v4,
    };
    let identity: aws_smithy_runtime_api::client::identity::Identity = credentials.into();
    let params = v4::SigningParams::builder()
        .identity(&identity)
        .region(region)
        .name("bedrock")
        .time(SystemTime::now())
        .settings(SigningSettings::default())
        .build()
        .map_err(|e| e.to_string())?
        .into();
    let signable = SignableRequest::new(
        method,
        url,
        headers.iter().filter(|(k, _)| k == "content-type").map(|(k, v)| (k.as_str(), v.as_str())),
        SignableBody::Bytes(body),
    )
    .map_err(|e| e.to_string())?;
    let (instructions, _) = sign(signable, &params).map_err(|e| e.to_string())?.into_parts();
    Ok(instructions.headers().map(|(k, v)| (k.to_owned(), v.to_owned())).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_only_model_calls_profiles_and_credentials() {
        let post = Method::POST;
        let get = Method::GET;
        assert_eq!(
            route(&post, "/bedrock/tok/model/us.anthropic.claude-sonnet-4-5-20250929-v1:0/invoke-with-response-stream", None),
            Some(Route::Model {
                token: "tok".into(),
                model: "us.anthropic.claude-sonnet-4-5-20250929-v1:0".into(),
                operation: "invoke-with-response-stream".into()
            })
        );
        // ARN は符号化されて届く
        assert_eq!(
            route(&post, "/bedrock/t/model/arn%3Aaws%3Abedrock%3Aus-east-1%3A1%3Aapplication-inference-profile%2Fabc/invoke", None),
            Some(Route::Model {
                token: "t".into(),
                model: "arn:aws:bedrock:us-east-1:1:application-inference-profile/abc".into(),
                operation: "invoke".into()
            })
        );
        assert_eq!(
            route(&get, "/bedrock/t/inference-profiles", Some("type=SYSTEM_DEFINED")),
            Some(Route::InferenceProfiles { token: "t".into(), query: "type=SYSTEM_DEFINED".into() })
        );
        assert_eq!(route(&get, "/aws/credentials", None), Some(Route::AwsCredentials));
        // それ以外の操作・メソッドは通さない
        assert_eq!(route(&get, "/bedrock/t/model/m.x/invoke", None), None);
        assert_eq!(route(&post, "/bedrock/t/model/m.x/converse", None), None);
        assert_eq!(route(&post, "/bedrock/t/guardrails", None), None);
        assert_eq!(route(&post, "/aws/credentials", None), None);
        assert_eq!(route(&get, "/bedrock/t/inference-profiles", Some("a=<x>")), None);
        assert_eq!(route(&post, "/bedrock/t/model/a%20b/invoke", None), None);
        assert_eq!(route(&get, "/other", None), None);
    }

    #[test]
    fn model_policy_follows_configured_models() {
        let only = ModelPolicy::from_configured(["us.anthropic.claude-sonnet-x".to_owned(), " arn:aws:bedrock:p ".to_owned()]);
        assert_eq!(only, ModelPolicy::Only(vec!["us.anthropic.claude-sonnet-x".into(), "arn:aws:bedrock:p".into()]));
        assert!(only.allows("us.anthropic.claude-sonnet-x") && only.allows("arn:aws:bedrock:p"));
        assert!(!only.allows("us.anthropic.claude-opus-y") && !only.allows("meta.llama3"));
        // Claude Code が補助に使う Haiku は通す
        assert!(only.allows("us.anthropic.claude-haiku-4-5-v1:0"));
        // 指定が無い・別名があるときは、Claude のモデルに限る
        for p in [ModelPolicy::from_configured(Vec::new()), ModelPolicy::from_configured(["sonnet".to_owned(), "us.anthropic.x".to_owned()])] {
            assert_eq!(p, ModelPolicy::AnyClaude);
            assert!(p.allows("global.anthropic.claude-opus") && !p.allows("amazon.nova-pro-v1:0"));
        }
    }

    #[test]
    fn urls_are_built_from_the_profile_region_only() {
        let m = Target::Model { model: "arn:aws:bedrock:us-east-1:1:x/y".into(), operation: "invoke".into() };
        assert_eq!(target_url(&m, "us-west-2"), "https://bedrock-runtime.us-west-2.amazonaws.com/model/arn%3Aaws%3Abedrock%3Aus-east-1%3A1%3Ax%2Fy/invoke");
        let c = Target::Control { query: "type=SYSTEM_DEFINED".into() };
        assert_eq!(target_url(&c, "ap-northeast-1"), "https://bedrock.ap-northeast-1.amazonaws.com/inference-profiles?type=SYSTEM_DEFINED");
        assert!(valid_region("us-east-1") && !valid_region("evil.com/") && !valid_region(""));
    }

    #[test]
    fn only_safe_headers_are_forwarded() {
        let mut h = hyper::HeaderMap::new();
        for (k, v) in [("content-type", "application/json"), ("authorization", "x"), ("anthropic-beta", "b"), ("host", "h"), ("x-stainless-os", "Linux")] {
            h.insert(hyper::header::HeaderName::from_static(k), hyper::header::HeaderValue::from_static(v));
        }
        let mut got = forwarded_headers(&h);
        got.sort();
        assert_eq!(got, vec![("anthropic-beta".to_owned(), "b".to_owned()), ("content-type".to_owned(), "application/json".to_owned())]);
    }

    #[test]
    fn signing_adds_sigv4_headers() {
        let creds = aws_credential_types::Credentials::new("AKIDEXAMPLE", "secret", Some("session".into()), None, "test");
        let headers = vec![("content-type".to_owned(), "application/json".to_owned())];
        let out = sign("POST", "https://bedrock-runtime.us-east-1.amazonaws.com/model/m%3A0/invoke", &headers, b"{}", creds, "us-east-1").unwrap();
        let get = |k: &str| out.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let auth = get("authorization").unwrap();
        assert!(auth.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/") && auth.contains("/us-east-1/bedrock/aws4_request"), "{auth}");
        assert_eq!(get("x-amz-security-token").as_deref(), Some("session"));
        assert!(get("x-amz-date").is_some());
    }

    /// 実際の Bedrock への転送の確認(ネットワークが必要)。ダミーのキーで署名し、AWS が認証エラーを返すこと
    /// (中継が署名して届けていること)と、許可していないモデル・無効なトークンを中継が止めることを確かめる。
    /// `cargo test -- --ignored bridge_forwards_to_bedrock`で実行する。
    #[tokio::test]
    #[ignore]
    async fn bridge_forwards_to_bedrock() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(WorkspaceCtx::open("ffffffff-0000-4000-8000-00000000b1d9", dir.path()).unwrap());
        register_for_test(ctx.clone());
        ctx.secrets
            .set_aws_files(AwsSlot::Bedrock, "[default]\nregion = us-east-1", "[default]\naws_access_key_id = AKIADUMMYNOTREAL0000\naws_secret_access_key = dummy")
            .unwrap();
        ctx.secrets.set_aws_profile(AwsSlot::Bedrock, "default").unwrap();
        let policy = ModelPolicy::Only(vec!["us.anthropic.claude-sonnet-4-5-20250929-v1:0".into()]);
        let (grant, vars) = bedrock_env(&ctx, policy).await.unwrap();
        let base = vars["ANTHROPIC_BEDROCK_BASE_URL"].replace(HOST_FROM_CONTAINER, "127.0.0.1");
        let client = reqwest::Client::new();
        let post = |model: &str| client.post(format!("{base}/model/{model}/invoke")).header("content-type", "application/json").body("{}").send();
        // 署名されて AWS に届き、ダミーのキーなので AWS が拒否する
        let res = post("us.anthropic.claude-sonnet-4-5-20250929-v1:0").await.unwrap();
        let kind = res.headers().get("x-amzn-errortype").map(|v| v.to_str().unwrap().to_owned()).unwrap_or_default();
        assert_eq!(res.status(), 403);
        assert!(kind.contains("UnrecognizedClient"), "{kind}: {}", res.text().await.unwrap());
        // 許可していないモデルは、中継が止める(AWS へは送らない)
        let res = post("us.anthropic.claude-opus-4-1-v1:0").await.unwrap();
        assert_eq!(res.status(), 403);
        assert!(res.text().await.unwrap().contains("許可されていません"));
        // 実行が終わる(トークンを落とす)と、使えない
        drop(grant);
        let res = post("us.anthropic.claude-sonnet-4-5-20250929-v1:0").await.unwrap();
        assert!(res.status() == 403 && res.text().await.unwrap().contains("トークン"));
    }

    #[test]
    fn aws_config_reads_the_token_from_the_environment() {
        // トークンとURLは設定に埋め込まない(設定は固定で、値は実行ごとの環境変数)
        assert!(AWS_CONFIG.contains("$MA_AWS_TOKEN") && AWS_CONFIG.contains("$MYASSISTANT_AWS_CREDENTIALS_URL"));
        assert!(AWS_SETUP_SCRIPT.starts_with(&format!("d={AWS_DIR};")));
        assert_eq!(TOKEN_VAR, "MA_AWS_TOKEN");
        assert_eq!(URL_VAR, "MYASSISTANT_AWS_CREDENTIALS_URL");
        let a = new_token();
        assert!(a.len() == 64 && a != new_token());
    }
}
