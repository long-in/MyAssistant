//! 各社LLM APIのHTTPクライアント。キーはsecretsから取得し、Rust外へは出さない。
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    aws, debug_log,
    secrets::Provider,
    workspace::WorkspaceCtx,
};
use crate::utils::error::AppError;

const MAX_TOKENS: u32 = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// "user" | "assistant"
    pub role: String,
    pub content: String,
}

/// LLMの応答本文。
pub struct Completion {
    pub text: String,
}

/// 応答を読み、デバッグモードなら往復を記録する。成功以外のステータスはエラーにする。
async fn read_json(ctx: &WorkspaceCtx, api: &str, url: &str, request: &Value, res: reqwest::Response) -> Result<Value, AppError> {
    let status = res.status();
    let text = res.text().await.map_err(|_| AppError::BadResponse)?;
    debug_log::record_api(ctx, api, url, request, status.as_u16(), &debug_log::parse_body(&text));
    if !status.is_success() {
        return Err(AppError::from_status(status.as_u16()));
    }
    serde_json::from_str(&text).map_err(|_| AppError::BadResponse)
}

/// エージェント用の上限。ツール結果を挟むため通常の対話より長く待つ。
const TASK_MAX_TOKENS: u32 = 8192;
const TASK_TIMEOUT: Duration = Duration::from_secs(120);

/// ツール使用付きの1回分の応答。`content`はthinking等を含む生のブロックで、
/// 次のリクエストへそのまま`assistant`メッセージとして戻す必要がある。
#[derive(Debug)]
pub struct ToolTurn {
    pub content: Vec<Value>,
    pub stop_reason: String,
}

/// `tool_use`ブロックの呼び出し内容。
#[derive(Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

impl ToolTurn {
    /// textブロックの連結。
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text")?.as_str())
            .collect()
    }

    pub fn tool_calls(&self) -> Vec<ToolCall> {
        self.content
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
            .filter_map(|b| {
                Some(ToolCall {
                    id: b.get("id")?.as_str()?.to_owned(),
                    name: b.get("name")?.as_str()?.to_owned(),
                    input: b.get("input").cloned().unwrap_or(Value::Null),
                })
            })
            .collect()
    }
}

fn parse_tool_turn(body: &Value) -> Option<ToolTurn> {
    Some(ToolTurn {
        content: body.get("content")?.as_array()?.clone(),
        stop_reason: body.get("stop_reason")?.as_str()?.to_owned(),
    })
}

/// ツール使用API(Anthropic / Bedrock)。`messages`と`tools`はMessages APIのJSON形式のまま渡す。
/// Bedrockはサーバーツール(検索・ページ取得)を持たないため、`tools`にはClient側ツールだけを渡すこと。
pub async fn complete_with_tools(
    ctx: &WorkspaceCtx,
    provider: Provider,
    model: &str,
    system: &str,
    messages: &[Value],
    tools: &[Value],
) -> Result<ToolTurn, AppError> {
    async {
        let request = json!({
            "model": model,
            "max_tokens": TASK_MAX_TOKENS,
            "system": system,
            "messages": messages,
            "tools": tools,
        });
        let body = match provider {
            Provider::Bedrock => invoke_bedrock(ctx, model, &request, TASK_TIMEOUT).await?,
            _ => {
                let key = ctx.secrets.get_key(provider)?
                    .ok_or_else(|| AppError::KeyMissing(provider.as_str().to_owned()))?;
                let client = reqwest::Client::builder()
                    .timeout(TASK_TIMEOUT)
                    .build()
                    .map_err(|_| AppError::Network)?;
                let url = "https://api.anthropic.com/v1/messages";
                let res = client
                    .post(url)
                    .header("x-api-key", &key)
                    .header("anthropic-version", "2023-06-01")
                    .json(&request)
                    .send()
                    .await
                    .map_err(|e| {
                        // 応答が無いまま失敗した場合も、どの要求で止まったかをデバッグログに残す
                        debug_log::record_api(ctx, provider.as_str(), url, &request, 0, &Value::String(e.to_string()));
                        AppError::from_reqwest(e)
                    })?;
                read_json(ctx, provider.as_str(), url, &request, res).await?
            }
        };
        parse_tool_turn(&body).ok_or_else(|| {
            log::error!("unexpected {} tool response: {body}", provider.as_str());
            AppError::BadResponse
        })
    }
    .await
    .map_err(|e| e.with_source(provider.as_str()))
}

/// Bedrockへ送る。モデルIDはURLに入るため本文から外し、往復はデバッグモードなら記録する。
async fn invoke_bedrock(ctx: &WorkspaceCtx, model: &str, request: &Value, timeout: Duration) -> Result<Value, AppError> {
    let mut body = request.clone();
    if let Some(o) = body.as_object_mut() {
        o.remove("model");
    }
    let label = format!("bedrock-runtime InvokeModel {model}");
    match aws::invoke_model(ctx, model, &body, timeout).await {
        Ok(response) => {
            debug_log::record_api(ctx, "bedrock", &label, &body, 200, &response);
            Ok(response)
        }
        Err(e) => {
            debug_log::record_api(ctx, "bedrock", &label, &body, 0, &Value::String(e.to_string()));
            Err(e)
        }
    }
}

/// Messages API形式の応答からtextブロックを連結する。
/// 先頭が"text"とは限らない(thinking等が前に付く)ため、textブロックを連結する。
fn anthropic_text(v: &Value) -> Option<String> {
    let text: String = v
        .get("content")?
        .as_array()?
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text")?.as_str())
        .collect();
    (!text.is_empty()).then_some(text)
}

pub async fn complete(
    ctx: &WorkspaceCtx,
    provider: Provider,
    model: &str,
    system: Option<&str>,
    messages: &[ChatMessage],
) -> Result<Completion, AppError> {
    complete_inner(ctx, provider, model, system, messages)
        .await
        .map_err(|e| e.with_source(provider.as_str()))
}

async fn complete_inner(
    ctx: &WorkspaceCtx,
    provider: Provider,
    model: &str,
    system: Option<&str>,
    messages: &[ChatMessage],
) -> Result<Completion, AppError> {
    let mut request = json!({ "max_tokens": MAX_TOKENS, "messages": messages });
    if let Some(s) = system {
        request["system"] = json!(s);
    }
    let response = match provider {
        Provider::Bedrock => invoke_bedrock(ctx, model, &request, Duration::from_secs(60)).await?,
        Provider::Anthropic => {
            let key = ctx.secrets.get_key(provider)?
                .ok_or_else(|| AppError::KeyMissing(provider.as_str().to_owned()))?;
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .map_err(|_| AppError::Network)?;
            let url = "https://api.anthropic.com/v1/messages";
            request["model"] = json!(model);
            let res = client
                .post(url)
                .header("x-api-key", &key)
                .header("anthropic-version", "2023-06-01")
                .json(&request)
                .send()
                .await
                .map_err(AppError::from_reqwest)?;
            read_json(ctx, provider.as_str(), url, &request, res).await?
        }
    };
    let text = anthropic_text(&response).ok_or_else(|| {
        // 原因調査用。応答本文にAPIキーは含まれない
        log::error!("unexpected {} response: {response}", provider.as_str());
        AppError::BadResponse
    })?;
    Ok(Completion { text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_and_tool_use() {
        let body = json!({
            "content": [
                { "type": "thinking", "thinking": "..." },
                { "type": "text", "text": "調べます" },
                { "type": "tool_use", "id": "t1", "name": "list_files", "input": { "path": "" } }
            ],
            "stop_reason": "tool_use"
        });
        let turn = parse_tool_turn(&body).unwrap();
        assert_eq!(turn.stop_reason, "tool_use");
        assert_eq!(turn.text(), "調べます");
        assert_eq!(
            turn.tool_calls(),
            [ToolCall { id: "t1".into(), name: "list_files".into(), input: json!({ "path": "" }) }]
        );
        assert!(parse_tool_turn(&json!({ "content": [] })).is_none());
    }
}
