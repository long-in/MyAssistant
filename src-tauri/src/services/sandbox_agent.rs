//! サンドボックスのエージェント(コンテナ内の Claude Code)の接続の設定と、渡す環境変数。
//!
//! - 接続先は Amazon Bedrock に固定。モデル・1回の指示の上限時間を、エージェントごとに持つ(`LLM_KEY`)。
//! - 認証は、アプリの AWS 設定からホストで発行した一時キーを、実行ごとの環境変数で渡す(`sandbox::env_for`。`sandbox.env`の同名が優先)。
//! - アプリが決める環境変数は秘密でない値だけ。`sandbox.env`に同じ名前があれば、そちらが優先される。
//! - 1回の指示は`run_turn`で、コンテナ内の`claude -p`(ヘッドレスモード)を起動し、`stream-json`の出力を逐次解析する。
//!   システムプロンプト・セッションID・ツールのリストなどの可変の値は、シェルへ埋め込まず`MYASSISTANT_`の環境変数で渡す。
use std::{collections::BTreeMap, ops::RangeInclusive, path::Path, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    cancel::CancelToken,
    command_policy::{self, Policy},
    global_settings::SettingsStore,
    sandbox,
    shell::Stream,
    workspace::WorkspaceCtx,
};
use crate::utils::error::AppError;

/// 接続の設定(エージェントごと)のキー。
pub const LLM_KEY: &str = "sandbox_llm";
/// Claude Code の settings.json(貼り付けたもの)のキー。値は JSON の文字列で、空は未設定。
pub const CLAUDE_SETTINGS_KEY: &str = "sandbox_claude_settings";
/// settings.json の上限(環境変数1つの上限 128KB より小さく)。
const SETTINGS_MAX_BYTES: usize = 64 * 1024;
/// 1回の指示の上限時間(分)の範囲。
pub const TIMEOUT_RANGE: RangeInclusive<u32> = 1..=180;
/// Claude Code の標準のツール。ツールなしの単発の呼び出しでは、これをすべて拒否する。
/// 許可は settings.json の`permissions.allow`で決める(画面の雛形も同じ一覧)。拒否は`command-policy.json`の拒否で絞る。
pub const DEFAULT_ALLOWED_TOOLS: [&str; 12] =
    ["Bash", "Read", "Edit", "Write", "Glob", "Grep", "NotebookEdit", "WebFetch", "WebSearch", "Task", "TodoWrite", "Skill"];
/// システムプロンプトの上限(Linux の環境変数1つの上限 128KB より小さく)。
const SYSTEM_MAX_BYTES: usize = 100 * 1024;

/// コンテナ内で実行するコマンド。可変の値はすべて環境変数で受け取る(シェルへ値を埋め込まない)。
const CLAUDE_COMMAND: &str = r#"exec claude -p --output-format stream-json --verbose --permission-mode acceptEdits ${MYASSISTANT_DISALLOWED_TOOLS:+--disallowedTools "$MYASSISTANT_DISALLOWED_TOOLS"} ${MYASSISTANT_RESUME:+--resume "$MYASSISTANT_RESUME"} ${MYASSISTANT_SETTINGS:+--settings "$MYASSISTANT_SETTINGS"} --append-system-prompt "$MYASSISTANT_SYSTEM_PROMPT""#;

/// アプリが決める、秘密でない環境変数の名前。出力の伏せ字の対象から外す。
pub const NON_SECRET_NAMES: [&str; 2] = ["CLAUDE_CODE_USE_BEDROCK", "ANTHROPIC_MODEL"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// 旧設定の読み込み用。`normalized`で Bedrock に直すので、実行では使われない。
    Anthropic,
    /// Amazon Bedrock(アプリの AWS 設定の一時キー)。
    #[default]
    Bedrock,
}

/// 接続の設定。未保存・欠けた項目は既定値になる。
/// モデルと許可するツールは settings.json で決める(旧設定の値は`migrate_legacy_llm`が settings.json へ移す)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    /// 常に Bedrock(保存済みの旧設定が Anthropic でも、読み込み時に Bedrock へ直す)。
    pub provider: Provider,
    /// 1回の指示の上限時間(分)。
    pub timeout_mins: u32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self { provider: Provider::Bedrock, timeout_mins: 30 }
    }
}

impl LlmConfig {
    /// 保存できる形に整える。範囲外の上限時間は丸める。
    fn normalized(mut self) -> Result<Self, AppError> {
        self.provider = Provider::Bedrock;
        self.timeout_mins = self.timeout_mins.clamp(*TIMEOUT_RANGE.start(), *TIMEOUT_RANGE.end());
        Ok(self)
    }
}

/// 保存された設定。読めなければ既定値。
pub fn load(db: &impl SettingsStore) -> Result<LlmConfig, AppError> {
    let cfg = db
        .get_setting(LLM_KEY)?
        .and_then(|v| serde_json::from_str::<LlmConfig>(&v).ok())
        .unwrap_or_default();
    Ok(cfg.normalized().unwrap_or_default())
}

pub fn save(db: &impl SettingsStore, cfg: LlmConfig) -> Result<LlmConfig, AppError> {
    let cfg = cfg.normalized()?;
    db.set_setting(LLM_KEY, &serde_json::to_string(&cfg).map_err(|e| AppError::Io(e.to_string()))?)?;
    Ok(cfg)
}

/// 旧設定(`sandbox_llm`のモデル・許可するツール)を、settings.json(`model`・`permissions.allow`)へ一度だけ移す。
/// settings.json がすでに書かれているときは上書きせず、そちらを優先する。移したあと(または不要なとき)は、旧項目を`sandbox_llm`から外す。
pub fn migrate_legacy_llm(db: &impl SettingsStore) -> Result<(), AppError> {
    let Some(raw) = db.get_setting(LLM_KEY)? else { return Ok(()) };
    let Ok(Value::Object(old)) = serde_json::from_str::<Value>(&raw) else { return Ok(()) };
    if !old.contains_key("model") && !old.contains_key("allowed_tools") {
        return Ok(());
    }
    if load_settings(db)?.trim().is_empty() {
        let mut root = serde_json::Map::new();
        if let Some(model) = old.get("model").and_then(Value::as_str).map(str::trim).filter(|m| !m.is_empty()) {
            root.insert("model".into(), Value::String(model.to_owned()));
        }
        let tools: Vec<Value> = old
            .get("allowed_tools")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).map(|t| Value::String(t.to_owned())).collect())
            .unwrap_or_default();
        if !tools.is_empty() {
            root.insert("permissions".into(), serde_json::json!({ "allow": tools }));
        }
        if !root.is_empty() {
            let text = serde_json::to_string_pretty(&Value::Object(root)).map_err(|e| AppError::Io(e.to_string()))?;
            save_settings(db, &text)?;
        }
    }
    // 旧項目を外す(読めない・範囲外の値は既定値へ丸める)
    let cfg: LlmConfig = serde_json::from_value(Value::Object(old)).unwrap_or_default();
    save(db, cfg)?;
    Ok(())
}

/// 貼り付けた settings.json を検証する。空(空白のみ)は未設定として通す。JSON のオブジェクトで、上限以内であること。
/// エラーの文面には内容を含めない(位置と理由だけ)。
pub fn validate_settings(text: &str) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(());
    }
    if text.len() > SETTINGS_MAX_BYTES {
        return Err(format!("settings.json が大きすぎます(上限{}KB)。", SETTINGS_MAX_BYTES / 1024));
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(_)) => Ok(()),
        Ok(_) => Err("settings.json は JSON のオブジェクト({ ... })で書いてください。".into()),
        Err(e) => Err(format!("settings.json を JSON として読めません({}行{}列目)。", e.line(), e.column())),
    }
}

/// 保存された settings.json(未設定は空文字)。
pub fn load_settings(db: &impl SettingsStore) -> Result<String, AppError> {
    Ok(db.get_setting(CLAUDE_SETTINGS_KEY)?.unwrap_or_default())
}

/// settings.json の`model`(ログの表示用)。未設定・読めないときは`None`。
pub fn configured_model(db: &impl SettingsStore) -> Option<String> {
    let Ok(Value::Object(root)) = serde_json::from_str::<Value>(&load_settings(db).ok()?) else { return None };
    root.get("model").and_then(Value::as_str).map(str::trim).filter(|m| !m.is_empty()).map(str::to_owned)
}

/// 検証して保存する。空にすると未設定に戻る。
pub fn save_settings(db: &impl SettingsStore, text: &str) -> Result<(), AppError> {
    validate_settings(text).map_err(AppError::Sandbox)?;
    db.set_setting(CLAUDE_SETTINGS_KEY, text.trim())
}

/// settings.json の`env`に書かれた値(秘密が入りうる)。出力の伏せ字の対象にする。`MYASSISTANT_SETTINGS`の値(JSON)を渡す。
pub fn settings_secret_values(settings: Option<&String>) -> Vec<String> {
    let Some(Ok(Value::Object(root))) = settings.map(|s| serde_json::from_str::<Value>(s)) else { return Vec::new() };
    match root.get("env") {
        Some(Value::Object(env)) => env.values().filter_map(|v| v.as_str().map(str::to_owned)).collect(),
        _ => Vec::new(),
    }
}

/// コンテナの Claude Code へ`--settings`で渡す値の環境変数。未設定なら何も足さない。
fn settings_env(text: &str) -> Result<Option<(String, String)>, String> {
    validate_settings(text)?;
    let text = text.trim();
    Ok((!text.is_empty()).then(|| ("MYASSISTANT_SETTINGS".to_owned(), text.to_owned())))
}

/// アプリが決める環境変数(秘密でない値だけ)。
pub fn app_env() -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    vars.insert("CLAUDE_CODE_USE_BEDROCK".to_owned(), "1".to_owned());
    vars
}

/// アプリが決める値の上に、認証情報(`sandbox.env`とAWSの一時キー)を重ねる。同じ名前は認証情報の側が優先。
pub fn merge_env(credentials: BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut vars = app_env();
    vars.extend(credentials);
    vars
}

fn has(vars: &BTreeMap<String, String>, name: &str) -> bool {
    vars.get(name).is_some_and(|v| !v.trim().is_empty())
}

/// Bedrock に必要な認証情報がそろっているか。そろっていなければ、実行せずに利用者向けの文面で失敗する。
pub fn check_credentials(vars: &BTreeMap<String, String>) -> Result<(), String> {
    if !has(vars, "AWS_ACCESS_KEY_ID") || !has(vars, "AWS_SECRET_ACCESS_KEY") {
        return Err("Bedrock の認証情報がありません。このエージェントに AWS の設定を登録してください。".into());
    }
    if !has(vars, "AWS_REGION") {
        return Err("Bedrock のリージョンがありません。AWS の設定のプロファイルに region を指定するか、sandbox.env に AWS_REGION を書いてください。".into());
    }
    Ok(())
}

// ------------------------------------------------------------------ 1回の指示の実行

/// コマンド権限の拒否(コマンドの接頭辞と、どこにあっても拒否する語)を、Claude Code の拒否ルールへ変換する。
pub fn deny_rules(policy: &Policy) -> Vec<String> {
    let mut rules: Vec<String> = Vec::new();
    for p in policy.deny.iter().chain(policy.deny_words.iter()).map(|p| p.trim()) {
        // 区切り(,)や括弧を含むものは、ルールの書式を壊すので渡さない
        if p.is_empty() || p.contains([',', '(', ')']) || p.chars().any(char::is_control) {
            continue;
        }
        let rule = format!("Bash({p}:*)");
        if !rules.contains(&rule) {
            rules.push(rule);
        }
    }
    rules
}

/// セッションIDとして受け付ける形(英数字と`-`だけ)。
fn valid_session_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// アプリが内部で渡す値(`MYASSISTANT_`)。
fn internal_vars(policy: &Policy, extra_deny: &[String], system: &str, resume: Option<&str>) -> Result<BTreeMap<String, String>, String> {
    if system.len() > SYSTEM_MAX_BYTES {
        return Err(format!("システムプロンプトが長すぎます(上限{}KB)。", SYSTEM_MAX_BYTES / 1024));
    }
    if system.contains('\0') {
        return Err("システムプロンプトに使えない文字が含まれています。".into());
    }
    let mut v = BTreeMap::new();
    v.insert("MYASSISTANT_SYSTEM_PROMPT".to_owned(), system.to_owned());
    let deny: Vec<String> = extra_deny.iter().cloned().chain(deny_rules(policy)).collect();
    v.insert("MYASSISTANT_DISALLOWED_TOOLS".to_owned(), deny.join(","));
    if let Some(id) = resume {
        if !valid_session_id(id) {
            return Err("会話のセッションIDが正しくありません。".into());
        }
        v.insert("MYASSISTANT_RESUME".to_owned(), id.to_owned());
    }
    Ok(v)
}

/// 途中経過。画面の進捗表示へ渡す。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentEvent {
    /// Claude Code のテキストの応答(途中のものを含む)。
    Text { text: String },
    /// ツールの呼び出し。
    ToolUse { id: String, name: String, input: Value },
    /// ツールの結果(テキストにまとめたもの)。
    ToolResult { id: String, content: String, is_error: bool },
    /// イメージのビルドなど、実行の準備の進捗。
    Status { line: String },
}

/// 1回の指示の結果。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TurnResult {
    /// 最終回答。結果が届かなかった(中断・時間切れ)ときは、それまでのテキストをつないだもの。
    pub text: String,
    /// 続きの指示で`--resume`に渡すセッションID。
    pub session_id: Option<String>,
    /// Claude Code がエラーとして終えたか(認証の失敗など)。
    pub is_error: bool,
    pub cost_usd: Option<f64>,
    pub usage: Option<Value>,
    pub num_turns: Option<u64>,
    pub timed_out: bool,
    pub cancelled: bool,
}

/// `stream-json`の1行を解釈したもの。
#[derive(Debug, Clone, PartialEq)]
enum Parsed {
    Session(String),
    Event(AgentEvent),
    Result(TurnResult),
}

/// ツールの結果の`content`(文字列、またはブロックの配列)をテキストにまとめる。
fn content_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// `stream-json`の1行を解釈する。知らない種類・JSONでない行は読み飛ばす。
fn parse_line(line: &str) -> Vec<Parsed> {
    let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { return Vec::new() };
    let str_of = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    match v.get("type").and_then(Value::as_str) {
        Some("system") if v.get("subtype").and_then(Value::as_str) == Some("init") => {
            str_of(&v, "session_id").map(Parsed::Session).into_iter().collect()
        }
        Some("assistant") => {
            let blocks = v.pointer("/message/content").and_then(Value::as_array).cloned().unwrap_or_default();
            blocks
                .iter()
                .filter_map(|b| match b.get("type").and_then(Value::as_str) {
                    Some("text") => str_of(b, "text").filter(|t| !t.is_empty()).map(|text| Parsed::Event(AgentEvent::Text { text })),
                    Some("tool_use") => Some(Parsed::Event(AgentEvent::ToolUse {
                        id: str_of(b, "id").unwrap_or_default(),
                        name: str_of(b, "name").unwrap_or_default(),
                        input: b.get("input").cloned().unwrap_or(Value::Null),
                    })),
                    _ => None,
                })
                .collect()
        }
        Some("user") => {
            let blocks = v.pointer("/message/content").and_then(Value::as_array).cloned().unwrap_or_default();
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
                .map(|b| {
                    Parsed::Event(AgentEvent::ToolResult {
                        id: str_of(b, "tool_use_id").unwrap_or_default(),
                        content: b.get("content").map(content_text).unwrap_or_default(),
                        is_error: b.get("is_error").and_then(Value::as_bool).unwrap_or(false),
                    })
                })
                .collect()
        }
        Some("result") => vec![Parsed::Result(TurnResult {
            text: str_of(&v, "result").unwrap_or_default(),
            session_id: str_of(&v, "session_id"),
            is_error: v.get("is_error").and_then(Value::as_bool).unwrap_or(false),
            cost_usd: v.get("total_cost_usd").and_then(Value::as_f64),
            usage: v.get("usage").cloned(),
            num_turns: v.get("num_turns").and_then(Value::as_u64),
            timed_out: false,
            cancelled: false,
        })],
        _ => Vec::new(),
    }
}

/// 出力を行に切り分けて解釈する。読み取りの境界で分かれた行は、次の入力までためておく。
#[derive(Default)]
struct LineParser {
    pending: String,
}

impl LineParser {
    fn push(&mut self, chunk: &str) -> Vec<Parsed> {
        self.pending.push_str(chunk);
        let mut out = Vec::new();
        while let Some(pos) = self.pending.find('\n') {
            let line: String = self.pending.drain(..=pos).collect();
            out.extend(parse_line(&line));
        }
        out
    }

    fn finish(&mut self) -> Vec<Parsed> {
        let rest = std::mem::take(&mut self.pending);
        parse_line(&rest)
    }
}

/// 1回の指示。
pub struct Turn<'a> {
    pub ctx: &'a WorkspaceCtx,
    /// ナレッジのルート(ホストのパス)。`/knowledge`にマウントし、コマンド権限もここから読む。
    pub knowledge_root: &'a Path,
    /// ユーザーの指示(標準入力で渡す)。
    pub prompt: &'a str,
    /// ルール(ベースプロンプトとエージェントのプロンプト)。`--append-system-prompt`で渡す。
    pub system: &'a str,
    /// 続きの指示なら、前回のセッションID。
    pub resume: Option<&'a str>,
    /// 追加で拒否するツール(コマンド実行やWeb検索を無効にしているときの`Bash`・`WebFetch`など)。
    pub deny: Vec<String>,
    /// ツールを使わない単発の呼び出しか(ナレッジ整理・校正)。許可を空にし、既定のツールをすべて拒否する。
    pub no_tools: bool,
    /// 上限時間。`None`なら設定の「1回の指示の上限時間」。
    pub timeout: Option<Duration>,
}

/// エラーの文面に添える標準エラーの末尾の文字数。
const STDERR_TAIL_CHARS: usize = 1_000;

fn tail(s: &str, n: usize) -> String {
    let count = s.chars().count();
    s.chars().skip(count.saturating_sub(n)).collect::<String>().trim().to_owned()
}

/// コンテナ内の Claude Code で1回の指示を実行する。途中経過は`on_event`へ逐次渡す。
/// 認証情報が足りない・Dockerが使えないなどは、実行せずに`Err`。中断・時間切れは`Ok`で、フラグを立てて返す。
pub async fn run_turn(turn: Turn<'_>, cancel: &CancelToken, mut on_event: impl FnMut(AgentEvent)) -> Result<TurnResult, String> {
    if !sandbox::is_supported(&turn.ctx.id) {
        return Err("リーダーではサンドボックスを使えません。".into());
    }
    if turn.prompt.trim().is_empty() {
        return Err("指示が空です。".into());
    }
    migrate_legacy_llm(&turn.ctx.db).map_err(|e| e.to_string())?;
    let cfg = load(&turn.ctx.db).map_err(|e| e.to_string())?;
    let mounts = sandbox::mounts_for(turn.ctx, turn.knowledge_root)?;
    let mut vars = merge_env(sandbox::env_for(turn.ctx).await?);
    check_credentials(&vars)?;
    // ツールなしの単発の呼び出しは、標準のツールをすべて拒否する(拒否は settings.json の許可より優先される)
    let deny = if turn.no_tools {
        let all: Vec<String> = DEFAULT_ALLOWED_TOOLS.iter().map(|s| (*s).to_owned()).collect();
        [turn.deny.clone(), all].concat()
    } else {
        turn.deny.clone()
    };
    vars.extend(internal_vars(&command_policy::load(turn.knowledge_root), &deny, turn.system, turn.resume)?);
    let settings = load_settings(&turn.ctx.db).map_err(|e| e.to_string())?;
    vars.extend(settings_env(&settings)?);

    let mut parser = LineParser::default();
    let mut session: Option<String> = None;
    let mut result: Option<TurnResult> = None;
    let mut texts: Vec<String> = Vec::new();
    let mut handle = |p: Parsed, on_event: &mut dyn FnMut(AgentEvent)| match p {
        Parsed::Session(id) => session = Some(id),
        Parsed::Result(r) => result = Some(r),
        Parsed::Event(e) => {
            if let AgentEvent::Text { text } = &e {
                texts.push(text.clone());
            }
            on_event(e);
        }
    };
    let exec = sandbox::Exec {
        ctx: turn.ctx,
        mounts: &mounts,
        cwd: mounts.default_dir(),
        command: CLAUDE_COMMAND,
        vars,
        input: Some(turn.prompt.to_owned()),
        timeout: turn.timeout.unwrap_or_else(|| Duration::from_secs(u64::from(cfg.timeout_mins) * 60)),
    };
    let out = sandbox::exec(exec, cancel, |stream, text| match stream {
        Stream::Stdout => {
            for p in parser.push(text) {
                handle(p, &mut on_event);
            }
        }
        // 実行の準備(イメージのビルド)の進捗だけを流す。Claude Code の標準エラーは、失敗したときに文面へ添える
        Stream::Stderr => {
            for line in text.lines().filter_map(|l| l.strip_prefix("[sandbox] ")) {
                on_event(AgentEvent::Status { line: line.to_owned() });
            }
        }
    })
    .await?;
    for p in parser.finish() {
        handle(p, &mut on_event);
    }

    let mut r = match result {
        Some(r) => r,
        None if out.timed_out || out.cancelled => TurnResult { text: texts.join("\n\n"), ..Default::default() },
        None => {
            let detail = tail(&out.stderr, STDERR_TAIL_CHARS);
            return Err(if detail.is_empty() {
                format!("Claude Code が結果を返さずに終了しました(終了コード: {:?})。", out.code)
            } else {
                format!("Claude Code が結果を返さずに終了しました: {detail}")
            });
        }
    };
    r.session_id = r.session_id.or(session);
    r.timed_out = out.timed_out;
    r.cancelled = out.cancelled;
    Ok(r)
}

/// ツールを使わない単発の呼び出し(ナレッジ整理・校正)。会話のセッションは作らず、応答のテキストだけを返す。
/// Claude Code がエラーで終えた・中断・時間切れは`Err`。
pub async fn complete(ctx: &WorkspaceCtx, knowledge_root: &Path, system: Option<&str>, prompt: &str, timeout: Option<Duration>, cancel: &CancelToken) -> Result<String, String> {
    let turn = Turn { ctx, knowledge_root, prompt, system: system.unwrap_or(""), resume: None, deny: Vec::new(), no_tools: true, timeout };
    let r = run_turn(turn, cancel, |_| {}).await?;
    if r.cancelled {
        return Err("中断されました。".into());
    }
    if r.timed_out {
        return Err("時間切れになりました。".into());
    }
    if r.is_error {
        return Err(if r.text.trim().is_empty() { "Claude Code がエラーで終了しました。".into() } else { r.text });
    }
    Ok(r.text)
}

// ------------------------------------------------------------------ 会話との対応

/// アプリの会話IDと、Claude Code のセッションIDの対応(エージェントごと。端末の会話と対なので書き出しの対象外)。
pub const SESSIONS_KEY: &str = "sandbox_claude_sessions";
/// 対応を覚えておく会話の数。古いものから捨てる。
const SESSIONS_MAX: usize = 50;

/// アプリの会話に対応する Claude Code のセッションID。無ければ`None`(新しいセッションで始める)。
pub fn claude_session(db: &impl SettingsStore, app_session: &str) -> Option<String> {
    let map: Vec<(String, String)> = db.get_setting(SESSIONS_KEY).ok().flatten().and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default();
    map.into_iter().find(|(a, _)| a == app_session).map(|(_, c)| c).filter(|c| valid_session_id(c))
}

/// アプリの会話と Claude Code のセッションの対応を覚える。
pub fn remember_session(db: &impl SettingsStore, app_session: &str, claude: &str) -> Result<(), AppError> {
    if !valid_session_id(claude) {
        return Ok(());
    }
    let mut map: Vec<(String, String)> = db.get_setting(SESSIONS_KEY)?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default();
    map.retain(|(a, _)| a != app_session);
    map.push((app_session.to_owned(), claude.to_owned()));
    let skip = map.len().saturating_sub(SESSIONS_MAX);
    let map: Vec<_> = map.into_iter().skip(skip).collect();
    db.set_setting(SESSIONS_KEY, &serde_json::to_string(&map).map_err(|e| AppError::Io(e.to_string()))?)
}

/// コンテナ内の作業環境の説明。システムプロンプトの末尾に加える。
pub fn guide() -> String {
    let workspace = "- /workspace: 担当システムのフォルダ(ソースコードなど)。作業ディレクトリの既定。";
    format!(
        "作業環境(サンドボックス)\n\
- このエージェント専用のLinuxコンテナ(Debian)の中で動いている。macOSのコマンド(brew、open、osascriptなど)は使えない。\n\
{workspace}\n\
- /knowledge: このエージェントのナレッジ(Markdown)。調べた結果や覚えておくべきことは、既存の構成に合わせてここへ書く。\n\
- /workspace と /knowledge はホストのフォルダで、変更はそのままホストに反映される。消す・上書きする前によく確かめる。\n\
- 認証情報(APIキー・AWS)は環境変数として渡されている。値を読み取ったり出力したりしない。AWSのリソースは aws コマンドで扱える。\n\
- 最終回答は、何をしてどうなったかを短く伝える。音声で読み上げられることがある。"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    #[test]
    fn allowed_tools_come_from_settings_not_flags() {
        // 許可は settings.json の`permissions.allow`で決める。`--allowedTools`は渡さない
        assert!(!CLAUDE_COMMAND.contains("--allowedTools") && !CLAUDE_COMMAND.contains("MYASSISTANT_ALLOWED_TOOLS"));
        // ツールなしの単発の呼び出しは、標準のツールをすべて拒否する
        let all: Vec<String> = DEFAULT_ALLOWED_TOOLS.iter().map(|s| (*s).to_owned()).collect();
        let v = internal_vars(&Policy::defaults(), &all, "sys", None).unwrap();
        assert!(!v.contains_key("MYASSISTANT_ALLOWED_TOOLS"));
        assert!(v["MYASSISTANT_DISALLOWED_TOOLS"].contains("Bash") && v["MYASSISTANT_DISALLOWED_TOOLS"].contains("WebFetch"));
    }

    #[test]
    fn legacy_llm_settings_are_moved_into_claude_settings_once() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open(&dir.path().join("t.db")).unwrap();
        // 旧設定がなければ何もしない
        migrate_legacy_llm(&db).unwrap();
        assert_eq!(load_settings(&db).unwrap(), "");
        db.set_setting(LLM_KEY, r#"{"provider":"bedrock","model":" us.anthropic.claude-x ","timeout_mins":45,"allowed_tools":["Bash"," Read ","Bash(git status:*)",""]}"#).unwrap();
        migrate_legacy_llm(&db).unwrap();
        let moved: Value = serde_json::from_str(&load_settings(&db).unwrap()).unwrap();
        assert_eq!(moved["model"], "us.anthropic.claude-x");
        assert_eq!(moved["permissions"]["allow"], serde_json::json!(["Bash", "Read", "Bash(git status:*)"]));
        // 上限時間は残り、旧項目は外れる
        assert_eq!(load(&db).unwrap().timeout_mins, 45);
        let left: Value = serde_json::from_str(&db.get_setting(LLM_KEY).unwrap().unwrap()).unwrap();
        assert!(left.get("model").is_none() && left.get("allowed_tools").is_none());
        // 2回目は何もしない(利用者が書き換えた内容を上書きしない)
        save_settings(&db, r#"{"model":"mine"}"#).unwrap();
        migrate_legacy_llm(&db).unwrap();
        assert_eq!(load_settings(&db).unwrap(), r#"{"model":"mine"}"#);
    }

    #[test]
    fn configured_model_is_read_from_claude_settings() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open(&dir.path().join("t.db")).unwrap();
        assert_eq!(configured_model(&db), None);
        save_settings(&db, r#"{"model":" us.anthropic.claude-x "}"#).unwrap();
        assert_eq!(configured_model(&db).as_deref(), Some("us.anthropic.claude-x"));
        save_settings(&db, r#"{"permissions":{}}"#).unwrap();
        assert_eq!(configured_model(&db), None);
    }

    #[test]
    fn legacy_llm_settings_do_not_overwrite_existing_claude_settings() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open(&dir.path().join("t.db")).unwrap();
        save_settings(&db, r#"{"model":"mine"}"#).unwrap();
        db.set_setting(LLM_KEY, r#"{"model":"old","timeout_mins":10,"allowed_tools":["Bash"]}"#).unwrap();
        migrate_legacy_llm(&db).unwrap();
        assert_eq!(load_settings(&db).unwrap(), r#"{"model":"mine"}"#);
        assert_eq!(load(&db).unwrap().timeout_mins, 10);
    }

    #[test]
    fn claude_settings_are_validated_and_passed_by_environment() {
        assert!(CLAUDE_COMMAND.contains(r#"${MYASSISTANT_SETTINGS:+--settings "$MYASSISTANT_SETTINGS"}"#));
        assert!(validate_settings("").is_ok() && validate_settings(" \n ").is_ok());
        assert!(validate_settings(r#"{"model":"x","permissions":{"deny":["Bash(rm:*)"]}}"#).is_ok());
        // 内容は、エラーの文面に出ない
        let e = validate_settings(r#"{"env":{"TOKEN":"secret-value""#).unwrap_err();
        assert!(e.contains("JSON") && !e.contains("secret-value"), "{e}");
        assert!(validate_settings("[1]").is_err() && validate_settings("\"x\"").is_err());
        assert!(validate_settings(&format!(r#"{{"a":"{}"}}"#, "x".repeat(SETTINGS_MAX_BYTES))).is_err());
        // `env`の値は、出力の伏せ字の対象になる
        let json = r#"{"env":{"TOKEN":"abcdefghijkl","N":1},"model":"x"}"#.to_owned();
        assert_eq!(settings_secret_values(Some(&json)), vec!["abcdefghijkl".to_owned()]);
        assert!(settings_secret_values(None).is_empty() && settings_secret_values(Some(&"{".to_owned())).is_empty());
        assert_eq!(settings_env("  ").unwrap(), None);
        assert_eq!(settings_env(" {\"a\":1} ").unwrap(), Some(("MYASSISTANT_SETTINGS".to_owned(), "{\"a\":1}".to_owned())));
        assert!(settings_env("{").is_err());
    }

    #[test]
    fn config_defaults_and_normalizes() {
        let cfg: LlmConfig = serde_json::from_str(r#"{"provider":"bedrock"}"#).unwrap();
        assert_eq!(cfg, LlmConfig::default());
        // 旧設定の Anthropic は、読み込み時に Bedrock へ直る。旧項目(model など)は無視される
        let old: LlmConfig = serde_json::from_str(r#"{"provider":"anthropic","model":"m"}"#).unwrap();
        assert_eq!(old.normalized().unwrap().provider, Provider::Bedrock);
        assert_eq!(LlmConfig::default().provider, Provider::Bedrock);
        assert_eq!(LlmConfig { timeout_mins: 9999, ..Default::default() }.normalized().unwrap().timeout_mins, 180);
        assert_eq!(LlmConfig { timeout_mins: 0, ..Default::default() }.normalized().unwrap().timeout_mins, 1);
    }

    #[test]
    fn app_env_carries_only_non_secret_values() {
        let env = app_env();
        assert_eq!(env, vars(&[("CLAUDE_CODE_USE_BEDROCK", "1")]));
        assert!(env.keys().all(|k| NON_SECRET_NAMES.contains(&k.as_str())));
    }

    #[test]
    fn sandbox_env_overrides_app_values() {
        let merged = merge_env(vars(&[("CLAUDE_CODE_USE_BEDROCK", "0"), ("ANTHROPIC_API_KEY", "k")]));
        assert_eq!(merged.get("CLAUDE_CODE_USE_BEDROCK").map(String::as_str), Some("0"));
    }

    #[test]
    fn bedrock_credentials_are_required() {
        // Anthropic のキーがあっても、Bedrock の認証情報の代わりにはならない
        assert!(check_credentials(&vars(&[("ANTHROPIC_API_KEY", "sk-x")])).is_err());
        assert!(check_credentials(&vars(&[("AWS_ACCESS_KEY_ID", "a"), ("AWS_SECRET_ACCESS_KEY", "b")])).is_err());
        assert!(check_credentials(&vars(&[("AWS_ACCESS_KEY_ID", "a"), ("AWS_SECRET_ACCESS_KEY", " "), ("AWS_REGION", "us-east-1")])).is_err());
        assert!(check_credentials(&vars(&[("AWS_ACCESS_KEY_ID", "a"), ("AWS_SECRET_ACCESS_KEY", "b"), ("AWS_REGION", "us-east-1")])).is_ok());
    }

    #[test]
    fn deny_rules_come_from_the_command_policy() {
        let mut p = Policy::defaults();
        p.deny = vec!["git push --force".into(), "bad,rule".into(), "x(y)".into(), "git push --force".into()];
        p.deny_words = vec!["sudo".into(), " ".into()];
        assert_eq!(deny_rules(&p), vec!["Bash(git push --force:*)".to_owned(), "Bash(sudo:*)".to_owned()]);
    }

    #[test]
    fn internal_values_are_passed_by_environment_only() {
        let p = Policy::defaults();
        let v = internal_vars(&p, &[], "ルール\n'\"$(rm -rf /)", Some("cd6a4042-73d8-4e11-84bc-f7e196eab226")).unwrap();
        assert_eq!(v["MYASSISTANT_SYSTEM_PROMPT"], "ルール\n'\"$(rm -rf /)");
        assert_eq!(v["MYASSISTANT_RESUME"], "cd6a4042-73d8-4e11-84bc-f7e196eab226");
        assert!(v.keys().all(|k| k.starts_with("MYASSISTANT_")));
        // コマンドには値を埋め込まない
        assert!(!CLAUDE_COMMAND.contains("ルール") && CLAUDE_COMMAND.contains("\"$MYASSISTANT_SYSTEM_PROMPT\""));
        assert!(internal_vars(&p, &[], "x", Some("bad id;rm")).is_err());
        assert!(internal_vars(&p, &[], &"a".repeat(SYSTEM_MAX_BYTES + 1), None).is_err());
        assert!(!internal_vars(&p, &[], "x", None).unwrap().contains_key("MYASSISTANT_RESUME"));
    }

    #[test]
    fn extra_deny_is_added_before_policy_rules() {
        let v = internal_vars(&Policy::defaults(), &["Bash".into(), "WebFetch".into()], "x", None).unwrap();
        assert!(v["MYASSISTANT_DISALLOWED_TOOLS"].starts_with("Bash,WebFetch,Bash("));
    }

    #[test]
    fn sessions_are_remembered_per_conversation_and_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open(&dir.path().join("t.db")).unwrap();
        assert_eq!(claude_session(&db, "a"), None);
        remember_session(&db, "a", "s-1").unwrap();
        remember_session(&db, "b", "s-2").unwrap();
        remember_session(&db, "a", "s-3").unwrap();
        assert_eq!(claude_session(&db, "a").as_deref(), Some("s-3"));
        assert_eq!(claude_session(&db, "b").as_deref(), Some("s-2"));
        // 不正なIDは覚えない
        remember_session(&db, "c", "bad id").unwrap();
        assert_eq!(claude_session(&db, "c"), None);
        for i in 0..SESSIONS_MAX + 5 {
            remember_session(&db, &format!("x{i}"), &format!("s-x{i}")).unwrap();
        }
        assert_eq!(claude_session(&db, "a"), None);
        assert!(claude_session(&db, &format!("x{}", SESSIONS_MAX + 4)).is_some());
    }

    #[test]
    fn guide_describes_the_mounts() {
        assert!(guide().contains("/workspace: 担当システム") && guide().contains("/knowledge"));
    }

    #[test]
    fn stream_json_is_parsed_into_events_and_result() {
        let lines = [
            r#"{"type":"system","subtype":"init","session_id":"s-1","tools":["Bash"]}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"調べます"},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"a.txt"}],"is_error":false}]}}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"result":"a.txt があります","session_id":"s-1","total_cost_usd":0.01,"num_turns":2,"usage":{"input_tokens":5}}"#,
        ];
        // 行の途中で分かれて届いても解釈できる
        let joined = lines.join("\n") + "\n";
        let (a, b) = joined.split_at(37);
        let mut parser = LineParser::default();
        let mut got = parser.push(a);
        got.extend(parser.push(b));
        got.extend(parser.finish());
        assert_eq!(got[0], Parsed::Session("s-1".into()));
        assert_eq!(got[1], Parsed::Event(AgentEvent::Text { text: "調べます".into() }));
        assert!(matches!(&got[2], Parsed::Event(AgentEvent::ToolUse { id, name, .. }) if id == "t1" && name == "Bash"));
        assert_eq!(got[3], Parsed::Event(AgentEvent::ToolResult { id: "t1".into(), content: "a.txt".into(), is_error: false }));
        let Parsed::Result(r) = &got[4] else { panic!("{got:?}") };
        assert_eq!((r.text.as_str(), r.session_id.as_deref(), r.is_error, r.cost_usd, r.num_turns), ("a.txt があります", Some("s-1"), false, Some(0.01), Some(2)));
        assert_eq!(got.len(), 5);
        // JSONでない行・知らない種類は読み飛ばす
        assert!(parse_line("not json").is_empty() && parse_line(r#"{"type":"stream_event"}"#).is_empty());
    }

    #[test]
    fn auth_failure_is_reported_as_an_error_result() {
        // 実際の Claude Code 2.1.311 が API キー無しで返す結果の形
        let line = r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login","session_id":"x","total_cost_usd":0,"num_turns":1}"#;
        let Parsed::Result(r) = &parse_line(line)[0] else { panic!() };
        assert!(r.is_error && r.text.contains("Not logged in"));
    }

    /// 実Dockerでの通し確認。Dockerの起動とネットワークが必要で、初回はイメージのビルドに数分かかる。
    /// ダミーの AWS キーで実行し、Claude Code が認証エラーの結果を返すことで、環境変数・標準入力・オプション・マウント・解析が通ることを確かめる。
    /// `cargo test -- --ignored claude_turn_end_to_end`で実行する。
    #[tokio::test]
    #[ignore]
    async fn claude_turn_end_to_end() {
        let tmp = tempfile::tempdir().unwrap();
        sandbox::set_app_dir_for_test(&tmp.path().join("app"));
        let dir = tmp.path().join("agent");
        let id = "ffffffff-0000-4000-8000-00000000c1a0";
        let ctx = WorkspaceCtx::open(id, &dir).unwrap();
        let knowledge = dir.join("knowledge");
        std::fs::create_dir_all(&knowledge).unwrap();
        std::fs::write(super::super::sandbox_env::path(&ctx.dir), "AWS_ACCESS_KEY_ID=AKIADUMMYNOTREAL0000\nAWS_SECRET_ACCESS_KEY=dummy-not-a-real-secret-0000\nAWS_REGION=us-east-1\n").unwrap();
        let cancel = CancelToken::default();
        let mut events = Vec::new();
        let turn = Turn { ctx: &ctx, knowledge_root: &knowledge, prompt: "こんにちは", system: "日本語で答える。'\"$(x)", resume: None, deny: vec!["WebFetch".into()], no_tools: false, timeout: None };
        let r = run_turn(turn, &cancel, |e| events.push(e)).await.unwrap();
        assert!(r.is_error, "{r:?}");
        assert!(r.session_id.is_some(), "{r:?}");
        // キーの値は出力に出ない(出ても伏せ字)
        assert!(!r.text.contains("dummy-not-a-real-secret"), "{}", r.text);
        // 続きの指示(--resume)も同じ流れで通る
        let resume = r.session_id.clone().unwrap();
        let turn = Turn { ctx: &ctx, knowledge_root: &knowledge, prompt: "続き", system: "x", resume: Some(&resume), deny: Vec::new(), no_tools: false, timeout: None };
        let r2 = run_turn(turn, &cancel, |_| {}).await.unwrap();
        assert!(r2.is_error, "{r2:?}");
        sandbox::remove_agent_blocking_for_test(id);
    }
}
