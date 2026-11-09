//! 端末内で動かすLLM(Qwen 2.5 7B Instruct)。
//!
//! llama.cppの`llama-server`を別プロセスで起動し、OpenAI互換APIで呼ぶ。whisper-rsが静的リンクしているggmlと、
//! 版の違うggmlを同じプロセスへ入れないため。サーバーとモデルは同梱せず、VOICEVOXと同じく設定画面から
//! アプリデータの`llm/`へ取得する。取得物の版とSHA-256はコードに固定し、一致しなければ破棄する。
//!
//! - サーバーは`127.0.0.1`だけで待ち受け、起動ごとに使い捨てのトークンを付ける。トークンはメモリにだけ持つ。
//! - 要求は1つずつ順に処理する(推論中に来た要求は前の要求の完了を待つ)。
//! - 呼び出し側とはAnthropicのMessages API形式でやり取りし、ここでOpenAI形式と相互に変換する。
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::{
    cancel::CancelToken,
    debug_log,
    llm_client::{ChatMessage, SystemPrompt, ToolTurn},
    workspace::WorkspaceCtx,
};
use crate::utils::error::AppError;

/// モデル一覧・保存済みの選択で使うID。
pub const MODEL_ID: &str = "qwen2.5-7b-instruct";
pub const MODEL_LABEL: &str = "Qwen 2.5 7B (Local)";
/// エラーの発生元・デバッグログでの名前(`Provider::Local`と同じ)。
pub const SOURCE: &str = "local";

/// llama.cppのリリース(v0.9.0)。macOS arm64版のアーカイブと、そのSHA-256・サイズ。
const LLAMA_BUILD: &str = "b12110";
const LLAMA_SHA256: &str = "4f5b8de12e0b9516658ca6a5764c6ffb3d7c659c3dd538556b1413c7b4050662";
const LLAMA_SIZE: u64 = 16_650_691;

/// モデルの取得元(Hugging Faceの公式リポジトリ)とリビジョン。Q4_K_Mは2つに分割されて配布されている。
const MODEL_REPO: &str = "Qwen/Qwen2.5-7B-Instruct-GGUF";
const MODEL_REVISION: &str = "bb5d59e06d9551d752d08b292a50eb208b07ab1f";

struct Part {
    file: &'static str,
    size: u64,
    sha256: &'static str,
}

/// 分割されたモデル。llama.cppは先頭のファイルを指定すれば、同じフォルダの残りも読む。
const MODEL_PARTS: [Part; 2] = [
    Part {
        file: "qwen2.5-7b-instruct-q4_k_m-00001-of-00002.gguf",
        size: 3_993_201_344,
        sha256: "dfce12e3862a5283ccfb88221b48480e58745165de856439950d0f22590580db",
    },
    Part {
        file: "qwen2.5-7b-instruct-q4_k_m-00002-of-00002.gguf",
        size: 689_872_288,
        sha256: "539cf93f78e887edea1c04e2d7d8cdaca9d01dae9c9025bcb8accbe29df3d72a",
    },
];

/// 文脈の長さ(トークン)。ツールの説明を含むシステムプロンプトが長いため、Qwen 2.5の上限(32K)にする。
const CTX_SIZE: u32 = 32_768;
/// 1回の応答の上限(トークン)。
const MAX_TOKENS: u32 = 4096;
const TASK_MAX_TOKENS: u32 = 8192;
/// 端末内の推論はAPIより遅いため、長めに待つ。
const TIMEOUT: Duration = Duration::from_secs(300);
/// 起動(モデルの読み込み)の待ち時間の上限。
const START_TIMEOUT: Duration = Duration::from_secs(180);

/// 取得物の置き場所(アプリデータ配下の`llm/`)。
struct Layout {
    root: PathBuf,
}

impl Layout {
    fn new(app_data: &Path) -> Self {
        Self { root: app_data.join("llm") }
    }

    fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }

    fn server(&self) -> PathBuf {
        self.bin().join("llama-server")
    }

    fn models(&self) -> PathBuf {
        self.root.join("models")
    }

    fn part(&self, p: &Part) -> PathBuf {
        self.models().join(p.file)
    }

    fn record(&self) -> PathBuf {
        self.root.join("installed.json")
    }

    fn log(&self) -> PathBuf {
        self.root.join("server.log")
    }

    fn pid(&self) -> PathBuf {
        self.root.join("server.pid")
    }
}

/// 取得の記録。すべて揃ってから書くため、途中で失敗したものは取得済みとして扱わない。
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Record {
    llama: String,
    model: String,
}

impl Record {
    fn current() -> Self {
        Self { llama: LLAMA_BUILD.into(), model: MODEL_REVISION.into() }
    }
}

fn installed_in(app_data: &Path) -> bool {
    let l = Layout::new(app_data);
    let record = std::fs::read_to_string(l.record()).ok().and_then(|s| serde_json::from_str::<Record>(&s).ok());
    record == Some(Record::current()) && l.server().is_file() && MODEL_PARTS.iter().all(|p| part_ready(&l, p))
}

/// サイズが一致すれば取得済みとみなす(SHA-256は取得時に確かめている)。
fn part_ready(l: &Layout, p: &Part) -> bool {
    std::fs::metadata(l.part(p)).map(|m| m.len() == p.size).unwrap_or(false)
}

// ---------- 状態 ----------

/// 起動中のサーバー。
struct Server {
    child: tokio::process::Child,
    base: String,
    token: String,
}

impl Server {
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

struct Engine {
    app_data: PathBuf,
    /// 起動中のサーバー。要求の間ロックを持ち続け、要求を1つずつ処理する。
    server: tokio::sync::Mutex<Option<Server>>,
    /// 起動中のサーバーのPID。終了時はロックを待たずに止めるため、別に持つ。
    pid: Mutex<Option<u32>>,
    /// 取得中なら、その中断用のトークン。
    download: Mutex<Option<CancelToken>>,
}

static ENGINE: OnceLock<Engine> = OnceLock::new();

/// 起動時に1回だけ呼ぶ。前回の異常終了で残ったサーバーがあれば止める。
pub fn init(app_data: &Path) {
    kill_stale(&Layout::new(app_data));
    let _ = ENGINE.set(Engine {
        app_data: app_data.to_owned(),
        server: tokio::sync::Mutex::new(None),
        pid: Mutex::new(None),
        download: Mutex::new(None),
    });
}

fn engine() -> Result<&'static Engine, AppError> {
    ENGINE.get().ok_or(AppError::LocalLlmMissing)
}

/// サーバーとモデルが揃っているか。`init`前(テストなど)は`false`。
pub fn installed() -> bool {
    ENGINE.get().is_some_and(|e| installed_in(&e.app_data))
}

/// 取得中か。
pub fn downloading() -> bool {
    ENGINE.get().is_some_and(|e| lock(&e.download).is_some())
}

/// 取得する合計サイズ(バイト)。
pub fn download_size() -> u64 {
    LLAMA_SIZE + MODEL_PARTS.iter().map(|p| p.size).sum::<u64>()
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// アプリの終了時に呼ぶ。推論中でも待たずに止める。
pub fn shutdown() {
    if let Some(e) = ENGINE.get() {
        if let Some(pid) = lock(&e.pid).take() {
            terminate(pid);
        }
        let _ = std::fs::remove_file(Layout::new(&e.app_data).pid());
    }
}

/// 起動中のサーバーを止める(取り直し・削除の前)。推論中なら、その完了を待ってから止める。
async fn stop(e: &Engine) {
    let mut server = e.server.lock().await;
    if let Some(mut s) = server.take() {
        let _ = s.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), s.child.wait()).await;
    }
    *lock(&e.pid) = None;
    let _ = std::fs::remove_file(Layout::new(&e.app_data).pid());
}

fn terminate(pid: u32) {
    let Ok(pid) = i32::try_from(pid) else { return };
    // SAFETY: シグナルを送るだけで、メモリには触れない
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
}

/// 前回の異常終了で残ったサーバーを止める。PIDが再利用されている場合に備え、プロセス名を確かめる。
fn kill_stale(l: &Layout) {
    let Some(pid) = std::fs::read_to_string(l.pid()).ok().and_then(|s| s.trim().parse::<u32>().ok()) else {
        return;
    };
    let name = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    if name.ends_with("llama-server") {
        log::info!("stopping stale llama-server ({pid})");
        terminate(pid);
    }
    let _ = std::fs::remove_file(l.pid());
}

// ---------- 起動 ----------

/// 空いているポート。一度使ってすぐ閉じたものを渡す。
fn free_port() -> Result<u16, AppError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| AppError::LocalLlm(e.to_string()))?;
    listener.local_addr().map(|a| a.port()).map_err(|e| AppError::LocalLlm(e.to_string()))
}

/// 推測できないトークン(UUID v4は暗号論的な乱数から作られる)。
fn new_token() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

/// ログの末尾(起動に失敗したときの原因の表示用)。
fn log_tail(l: &Layout) -> String {
    let text = std::fs::read_to_string(l.log()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().rev().take(5).collect();
    lines.into_iter().rev().collect::<Vec<_>>().join(" / ")
}

async fn start(e: &Engine) -> Result<Server, AppError> {
    let l = Layout::new(&e.app_data);
    let io = |err: std::io::Error| AppError::LocalLlm(err.to_string());
    let port = free_port()?;
    let token = new_token();
    let log = std::fs::File::create(l.log()).map_err(io)?;
    let mut cmd = tokio::process::Command::new(l.server());
    cmd.arg("--model")
        .arg(l.part(&MODEL_PARTS[0]))
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["--ctx-size", &CTX_SIZE.to_string(), "--n-gpu-layers", "999", "--parallel", "1", "--jinja"])
        // トークンは引数に置くと`ps`で他のユーザーから見えるため、環境変数で渡す
        .env("LLAMA_API_KEY", &token)
        .current_dir(l.bin())
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(io)?)
        .stderr(log)
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(io)?;
    if let Some(pid) = child.id() {
        *lock(&e.pid) = Some(pid);
        let _ = std::fs::write(l.pid(), pid.to_string());
    }
    let base = format!("http://127.0.0.1:{port}");
    match wait_ready(&l, &mut child, &base).await {
        Ok(()) => Ok(Server { child, base, token }),
        Err(err) => {
            // 起動に失敗したプロセスは`child`の破棄で止まる(`kill_on_drop`)
            *lock(&e.pid) = None;
            let _ = std::fs::remove_file(l.pid());
            Err(err)
        }
    }
}

/// 準備ができるまで待ち、トークンが効いていることを確かめる。
async fn wait_ready(l: &Layout, child: &mut tokio::process::Child, base: &str) -> Result<(), AppError> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().map_err(|_| AppError::Network)?;
    let deadline = tokio::time::Instant::now() + START_TIMEOUT;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(AppError::LocalLlm(format!("llama-server exited ({status}): {}", log_tail(l))));
        }
        // 読み込み中は503、準備ができたら200
        if let Ok(res) = client.get(format!("{base}/health")).send().await
            && res.status().is_success()
        {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(AppError::LocalLlm(format!("llama-server did not become ready: {}", log_tail(l))));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    // トークンが効いていない(認証なしで受け付ける)サーバーは使わない
    let unauth = client.post(format!("{base}/v1/chat/completions")).json(&json!({})).send().await;
    if !matches!(unauth, Ok(ref r) if r.status() == reqwest::StatusCode::UNAUTHORIZED) {
        return Err(AppError::LocalLlm("llama-server accepted a request without the token".into()));
    }
    Ok(())
}

// ---------- 要求 ----------

/// サーバーへ1回分の要求を送る。起動していなければ起動する。応答を待つ間ロックを持ち、要求を1つずつ処理する。
/// 呼び出し側が中断(futureを破棄)すると接続が切れ、サーバー側の生成も止まる。
async fn post(ctx: &WorkspaceCtx, request: &Value) -> Result<Value, AppError> {
    let e = engine()?;
    if !installed_in(&e.app_data) {
        return Err(AppError::LocalLlmMissing);
    }
    let mut server = e.server.lock().await;
    if !server.as_mut().is_some_and(Server::alive) {
        *server = None;
        *server = Some(start(e).await?);
    }
    let s = server.as_ref().ok_or(AppError::LocalLlmMissing)?;
    let url = format!("{}/v1/chat/completions", s.base);
    let client = reqwest::Client::builder().timeout(TIMEOUT).build().map_err(|_| AppError::Network)?;
    let res = client.post(&url).bearer_auth(&s.token).json(request).send().await.map_err(|err| {
        debug_log::record_api(ctx, SOURCE, &url, request, 0, &Value::String(err.to_string()));
        AppError::from_reqwest(err)
    })?;
    let status = res.status();
    let text = res.text().await.map_err(|_| AppError::BadResponse)?;
    debug_log::record_api(ctx, SOURCE, &url, request, status.as_u16(), &debug_log::parse_body(&text));
    if !status.is_success() {
        let body = serde_json::from_str::<Value>(&text).ok();
        let message = body.as_ref().and_then(|b| b.pointer("/error/message")?.as_str().map(str::to_owned));
        return Err(AppError::LocalLlm(message.unwrap_or_else(|| format!("HTTP {status}"))));
    }
    serde_json::from_str(&text).map_err(|_| AppError::BadResponse)
}

/// テキストだけの応答(`llm_client::complete`の端末内版)。
pub async fn complete(ctx: &WorkspaceCtx, system: Option<&str>, messages: &[ChatMessage]) -> Result<String, AppError> {
    let mut msgs = Vec::with_capacity(messages.len() + 1);
    if let Some(s) = system.filter(|s| !s.trim().is_empty()) {
        msgs.push(json!({ "role": "system", "content": s }));
    }
    msgs.extend(messages.iter().map(|m| json!({ "role": m.role, "content": m.content })));
    let request = json!({ "model": MODEL_ID, "max_tokens": MAX_TOKENS, "messages": msgs });
    let body = post(ctx, &request).await?;
    parse_text(&body).ok_or_else(|| {
        log::error!("unexpected local response: {body}");
        AppError::BadResponse
    })
}

/// ツール使用付きの応答(`llm_client::complete_with_tools`の端末内版)。
/// `messages`と`tools`はMessages API形式で受け、結果もMessages API形式のブロックで返す。
pub async fn complete_with_tools(
    ctx: &WorkspaceCtx,
    system: &SystemPrompt,
    messages: &[Value],
    tools: &[Value],
) -> Result<ToolTurn, AppError> {
    let mut system_text = system.stable.clone();
    if !system.volatile.trim().is_empty() {
        system_text.push_str("\n\n");
        system_text.push_str(&system.volatile);
    }
    let mut request = json!({
        "model": MODEL_ID,
        "max_tokens": TASK_MAX_TOKENS,
        "messages": to_openai_messages(&system_text, messages),
    });
    let tools = to_openai_tools(tools);
    if !tools.is_empty() {
        request["tools"] = Value::Array(tools);
    }
    let body = post(ctx, &request).await?;
    parse_turn(&body).ok_or_else(|| {
        log::error!("unexpected local tool response: {body}");
        AppError::BadResponse
    })
}

// ---------- 形式の変換(Messages API ⇔ OpenAI) ----------

/// ブロックの配列(または文字列)からテキストを取り出す。
fn block_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text")?.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Messages APIの履歴をOpenAI形式へ変える。
/// - `tool_use`は`assistant`の`tool_calls`、`tool_result`は`role: tool`のメッセージにする。
/// - thinkingやサーバーツール(検索など、Claudeで実行した分)のブロックは渡さない。
fn to_openai_messages(system: &str, messages: &[Value]) -> Vec<Value> {
    let mut out = Vec::with_capacity(messages.len() + 1);
    if !system.trim().is_empty() {
        out.push(json!({ "role": "system", "content": system }));
    }
    for m in messages {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("user");
        let content = m.get("content").unwrap_or(&Value::Null);
        let Value::Array(blocks) = content else {
            out.push(json!({ "role": role, "content": block_text(content) }));
            continue;
        };
        let kind = |b: &Value| b.get("type").and_then(Value::as_str).map(str::to_owned);
        if role == "assistant" {
            let calls: Vec<Value> = blocks
                .iter()
                .filter(|b| kind(b).as_deref() == Some("tool_use"))
                .map(|b| {
                    let input = b.get("input").cloned().unwrap_or_else(|| json!({}));
                    json!({
                        "id": b.get("id").and_then(Value::as_str).unwrap_or_default(),
                        "type": "function",
                        "function": {
                            "name": b.get("name").and_then(Value::as_str).unwrap_or_default(),
                            "arguments": input.to_string(),
                        },
                    })
                })
                .collect();
            let mut msg = json!({ "role": "assistant", "content": block_text(content) });
            if !calls.is_empty() {
                msg["tool_calls"] = Value::Array(calls);
            }
            out.push(msg);
            continue;
        }
        // ツールの結果は、呼び出しの直後に置く必要があるため、同じメッセージのテキストより先に並べる
        for b in blocks.iter().filter(|b| kind(b).as_deref() == Some("tool_result")) {
            let mut text = block_text(b.get("content").unwrap_or(&Value::Null));
            if b.get("is_error").and_then(Value::as_bool) == Some(true) {
                text = format!("Error: {text}");
            }
            out.push(json!({
                "role": "tool",
                "tool_call_id": b.get("tool_use_id").and_then(Value::as_str).unwrap_or_default(),
                "content": text,
            }));
        }
        let text = block_text(content);
        if !text.is_empty() {
            out.push(json!({ "role": role, "content": text }));
        }
    }
    out
}

/// クライアント側のツール定義だけをOpenAI形式へ変える。`input_schema`を持たないもの(サーバーツール)は除く。
fn to_openai_tools(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|t| {
            Some(json!({
                "type": "function",
                "function": {
                    "name": t.get("name")?.as_str()?,
                    "description": t.get("description").and_then(Value::as_str).unwrap_or_default(),
                    "parameters": t.get("input_schema")?,
                },
            }))
        })
        .collect()
}

fn first_message(body: &Value) -> Option<&Value> {
    body.get("choices")?.as_array()?.first()?.get("message")
}

fn parse_text(body: &Value) -> Option<String> {
    let text = first_message(body)?.get("content")?.as_str()?.trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// OpenAI形式の応答を、Messages API形式の`ToolTurn`へ変える。
fn parse_turn(body: &Value) -> Option<ToolTurn> {
    let choice = body.get("choices")?.as_array()?.first()?;
    let message = choice.get("message")?;
    let mut content = Vec::new();
    if let Some(text) = message.get("content").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()) {
        content.push(json!({ "type": "text", "text": text }));
    }
    let calls = message.get("tool_calls").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    for (i, call) in calls.iter().enumerate() {
        let f = call.get("function")?;
        let name = f.get("name")?.as_str()?;
        // 引数が壊れたJSONなら空の入力にする(ツール側が不足をエラーとして返し、モデルが直せる)
        let input = f
            .get("arguments")
            .and_then(Value::as_str)
            .and_then(|a| serde_json::from_str::<Value>(a).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        let id = call
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("call_{i}_{}", uuid::Uuid::new_v4().simple()));
        content.push(json!({ "type": "tool_use", "id": id, "name": name, "input": input }));
    }
    let stop_reason = if !calls.is_empty() {
        "tool_use"
    } else if choice.get("finish_reason").and_then(Value::as_str) == Some("length") {
        "max_tokens"
    } else {
        "end_turn"
    };
    Some(ToolTurn { content, stop_reason: stop_reason.into() })
}

// ---------- 取得 ----------

/// 取得の進み具合(バイト)。
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Progress {
    pub downloaded: u64,
    pub total: u64,
}

/// 進み具合の通知の間隔(バイト)。
const PROGRESS_STEP: u64 = 8 << 20;

/// `url`を`dest`へ保存する。一時ファイル経由で置き、サイズかSHA-256が違えば破棄して失敗にする。
/// `offset`はそれまでに取得した量で、進み具合へ足して通知する。
async fn download(
    url: &str,
    dest: &Path,
    size: u64,
    sha256: &str,
    offset: u64,
    total: u64,
    cancel: &CancelToken,
    on_progress: &(impl Fn(Progress) + Sync),
) -> Result<(), AppError> {
    let io = |e: std::io::Error| AppError::Io(e.to_string());
    let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(30)).build().map_err(|_| AppError::Network)?;
    let mut res = client.get(url).send().await.map_err(AppError::from_reqwest)?;
    if !res.status().is_success() {
        return Err(AppError::from_status(res.status().as_u16()));
    }
    let tmp = dest.with_extension("part");
    let result = async {
        let mut file = tokio::fs::File::create(&tmp).await.map_err(io)?;
        let mut hasher = Sha256::new();
        let (mut done, mut reported) = (0u64, 0u64);
        loop {
            let chunk = tokio::select! {
                c = res.chunk() => c.map_err(AppError::from_reqwest)?,
                () = cancel.cancelled() => return Err(AppError::DownloadCancelled),
            };
            let Some(chunk) = chunk else { break };
            file.write_all(&chunk).await.map_err(io)?;
            hasher.update(&chunk);
            done += chunk.len() as u64;
            if done > size {
                return Err(AppError::ChecksumMismatch);
            }
            if done - reported >= PROGRESS_STEP {
                reported = done;
                on_progress(Progress { downloaded: offset + done, total });
            }
        }
        file.flush().await.map_err(io)?;
        drop(file);
        if done != size || hex(&hasher.finalize()) != sha256 {
            return Err(AppError::ChecksumMismatch);
        }
        tokio::fs::rename(&tmp, dest).await.map_err(io)
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&tmp).await;
    }
    result
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// サーバーとモデルを取得して配置する。取得済みのモデル(サイズが一致するもの)は取り直さない。
/// 取得中に`cancel_download`されると`DownloadCancelled`で止まる。
pub async fn install(on_progress: impl Fn(Progress) + Sync) -> Result<(), AppError> {
    let e = engine()?;
    let cancel = {
        let mut slot = lock(&e.download);
        if slot.is_some() {
            return Err(AppError::LocalLlm("already downloading".into()));
        }
        let token = CancelToken::default();
        *slot = Some(token.clone());
        token
    };
    let result = install_inner(e, &cancel, &on_progress).await;
    *lock(&e.download) = None;
    result.map_err(|err| err.with_source(SOURCE))
}

async fn install_inner(e: &Engine, cancel: &CancelToken, on_progress: &(impl Fn(Progress) + Sync)) -> Result<(), AppError> {
    let io = |err: std::io::Error| AppError::Io(err.to_string());
    let l = Layout::new(&e.app_data);
    tokio::fs::create_dir_all(l.models()).await.map_err(io)?;
    // 入れ替えの途中で失敗したものを取得済みとして扱わない
    let _ = tokio::fs::remove_file(l.record()).await;
    let total = download_size();
    let mut offset = 0;
    on_progress(Progress { downloaded: 0, total });

    // サーバー: アーカイブを検証してから展開し、`bin/`を丸ごと入れ替える
    let work = l.root.join("tmp");
    let _ = tokio::fs::remove_dir_all(&work).await;
    tokio::fs::create_dir_all(&work).await.map_err(io)?;
    let archive = work.join("llama.tar.gz");
    let url = format!("https://github.com/ggml-org/llama.cpp/releases/download/{LLAMA_BUILD}/llama-{LLAMA_BUILD}-bin-macos-arm64.tar.gz");
    let placed = async {
        download(&url, &archive, LLAMA_SIZE, LLAMA_SHA256, offset, total, cancel, on_progress).await?;
        extract(&archive, &work).await?;
        let extracted = work.join(format!("llama-{LLAMA_BUILD}"));
        if !extracted.join("llama-server").is_file() {
            return Err(AppError::LocalLlm("llama-server not found in the archive".into()));
        }
        // 動いているサーバーは、入れ替える前に止める
        stop(e).await;
        let _ = tokio::fs::remove_dir_all(l.bin()).await;
        tokio::fs::rename(&extracted, l.bin()).await.map_err(io)
    }
    .await;
    let _ = tokio::fs::remove_dir_all(&work).await;
    placed?;
    offset += LLAMA_SIZE;

    // モデル
    for p in &MODEL_PARTS {
        if !part_ready(&l, p) {
            let url = format!("https://huggingface.co/{MODEL_REPO}/resolve/{MODEL_REVISION}/{}", p.file);
            download(&url, &l.part(p), p.size, p.sha256, offset, total, cancel, on_progress).await?;
        }
        offset += p.size;
        on_progress(Progress { downloaded: offset, total });
    }
    let record = serde_json::to_string(&Record::current()).map_err(|err| AppError::Io(err.to_string()))?;
    tokio::fs::write(l.record(), record).await.map_err(io)
}

/// システムの`tar`で展開する(`..`を含むパスは`tar`が拒否する)。
async fn extract(archive: &Path, into: &Path) -> Result<(), AppError> {
    let out = tokio::process::Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .output()
        .await
        .map_err(|e| AppError::Io(format!("tar: {e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(AppError::Io(format!("tar failed: {}", String::from_utf8_lossy(&out.stderr).trim())))
    }
}

/// 取得中なら中断する。
pub fn cancel_download() {
    if let Some(e) = ENGINE.get()
        && let Some(token) = lock(&e.download).as_ref()
    {
        token.cancel();
    }
}

/// サーバーを止め、取得したものをすべて消す。取得中は消さない。
pub async fn remove() -> Result<(), AppError> {
    let e = engine()?;
    if lock(&e.download).is_some() {
        return Err(AppError::LocalLlm("download in progress".into()));
    }
    stop(e).await;
    let root = Layout::new(&e.app_data).root;
    match tokio::fs::remove_dir_all(&root).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(AppError::Io(err.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_needs_record_server_and_every_part() {
        let d = tempfile::tempdir().unwrap();
        let l = Layout::new(d.path());
        assert!(!installed_in(d.path()));
        std::fs::create_dir_all(l.bin()).unwrap();
        std::fs::create_dir_all(l.models()).unwrap();
        std::fs::write(l.server(), b"").unwrap();
        for p in &MODEL_PARTS {
            std::fs::File::create(l.part(p)).unwrap().set_len(p.size).unwrap();
        }
        assert!(!installed_in(d.path()), "記録が無ければ未取得");
        std::fs::write(l.record(), serde_json::to_string(&Record::current()).unwrap()).unwrap();
        assert!(installed_in(d.path()));
        // 古い版の記録は未取得として扱う
        std::fs::write(l.record(), r#"{"llama":"b1","model":"x"}"#).unwrap();
        assert!(!installed_in(d.path()));
        std::fs::write(l.record(), serde_json::to_string(&Record::current()).unwrap()).unwrap();
        // サイズが違う(途中切れ)部分があれば未取得
        std::fs::File::create(l.part(&MODEL_PARTS[1])).unwrap().set_len(10).unwrap();
        assert!(!installed_in(d.path()));
    }

    #[test]
    fn converts_history_with_tool_calls_and_results() {
        let history = vec![
            json!({ "role": "user", "content": "メモを探して" }),
            json!({ "role": "assistant", "content": [
                { "type": "thinking", "thinking": "..." },
                { "type": "text", "text": "探します" },
                { "type": "tool_use", "id": "t1", "name": "list_files", "input": { "path": "" } }
            ] }),
            json!({ "role": "user", "content": [
                { "type": "tool_result", "tool_use_id": "t1", "content": "a.md" },
                { "type": "tool_result", "tool_use_id": "t2", "content": [{ "type": "text", "text": "無い" }], "is_error": true }
            ] }),
        ];
        let out = to_openai_messages("前置き", &history);
        assert_eq!(out[0], json!({ "role": "system", "content": "前置き" }));
        assert_eq!(out[1], json!({ "role": "user", "content": "メモを探して" }));
        assert_eq!(out[2]["content"], "探します");
        assert_eq!(out[2]["tool_calls"][0]["id"], "t1");
        assert_eq!(out[2]["tool_calls"][0]["function"]["name"], "list_files");
        assert_eq!(out[2]["tool_calls"][0]["function"]["arguments"], r#"{"path":""}"#);
        assert_eq!(out[3], json!({ "role": "tool", "tool_call_id": "t1", "content": "a.md" }));
        assert_eq!(out[4], json!({ "role": "tool", "tool_call_id": "t2", "content": "Error: 無い" }));
        assert_eq!(out.len(), 5, "ツールの結果だけのメッセージに空のuserを足さない");
        // システムプロンプトが空なら送らない
        assert_eq!(to_openai_messages(" ", &history[..1]).len(), 1);
    }

    #[test]
    fn server_tools_are_not_passed() {
        let tools = vec![
            json!({ "name": "read_file", "description": "読む", "input_schema": { "type": "object" } }),
            json!({ "type": "web_search_20250305", "name": "web_search", "max_uses": 3 }),
        ];
        let out = to_openai_tools(&tools);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["function"]["name"], "read_file");
        assert_eq!(out[0]["function"]["parameters"], json!({ "type": "object" }));
    }

    #[test]
    fn parses_tool_calls_into_tool_use_blocks() {
        let body = json!({ "choices": [{
            "finish_reason": "tool_calls",
            "message": { "role": "assistant", "content": "読みます", "tool_calls": [
                { "id": "c1", "type": "function", "function": { "name": "read_file", "arguments": "{\"path\":\"a.md\"}" } },
                { "type": "function", "function": { "name": "list_files", "arguments": "壊れた" } }
            ] }
        }] });
        let turn = parse_turn(&body).unwrap();
        assert_eq!(turn.stop_reason, "tool_use");
        assert_eq!(turn.text(), "読みます");
        let calls = turn.tool_calls();
        assert_eq!((calls[0].id.as_str(), calls[0].name.as_str()), ("c1", "read_file"));
        assert_eq!(calls[0].input, json!({ "path": "a.md" }));
        // IDが無ければ作り、壊れた引数は空の入力にする
        assert!(calls[1].id.starts_with("call_1_"));
        assert_eq!(calls[1].input, json!({}));
    }

    #[test]
    fn parses_final_text_and_length_stop() {
        let body = |reason: &str| json!({ "choices": [{ "finish_reason": reason, "message": { "content": " 完了です " } }] });
        let turn = parse_turn(&body("stop")).unwrap();
        assert_eq!((turn.stop_reason.as_str(), turn.text().as_str()), ("end_turn", "完了です"));
        assert_eq!(parse_turn(&body("length")).unwrap().stop_reason, "max_tokens");
        assert_eq!(parse_text(&body("stop")).as_deref(), Some("完了です"));
        assert!(parse_text(&json!({ "choices": [{ "message": { "content": "" } }] })).is_none());
        assert!(parse_turn(&json!({})).is_none());
    }

    #[test]
    fn hex_is_lowercase() {
        assert_eq!(hex(&[0x0a, 0xff]), "0aff");
    }

    #[test]
    fn not_installed_before_init() {
        // テストでは`init`しないため、端末に取得済みでも使えない扱いになる
        if ENGINE.get().is_none() {
            assert!(!installed());
        }
    }
}
