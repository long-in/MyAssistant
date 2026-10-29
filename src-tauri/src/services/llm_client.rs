//! 各社LLM APIのHTTPクライアント。キーはsecretsから取得し、Rust外へは出さない。
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    debug_log,
    secrets::{self, Provider},
};
use crate::utils::error::AppError;

const MAX_TOKENS: u32 = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// "user" | "assistant"
    pub role: String,
    pub content: String,
}

/// LLMの応答本文と、プロバイダーが返したトークン使用量(返さない場合は0)。
pub struct Completion {
    pub text: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

fn usage_of(provider: Provider, body: &Value) -> (u64, u64) {
    let (i, o) = match provider {
        Provider::Anthropic => ("/usage/input_tokens", "/usage/output_tokens"),
        Provider::Openai => ("/usage/prompt_tokens", "/usage/completion_tokens"),
    };
    let get = |p: &str| body.pointer(p).and_then(Value::as_u64).unwrap_or(0);
    (get(i), get(o))
}

/// 応答を読み、デバッグモードなら往復を記録する。成功以外のステータスはエラーにする。
async fn read_json(api: &str, url: &str, request: &Value, res: reqwest::Response) -> Result<Value, AppError> {
    let status = res.status();
    let text = res.text().await.map_err(|_| AppError::BadResponse)?;
    debug_log::record_api(api, url, request, status.as_u16(), &debug_log::parse_body(&text));
    if !status.is_success() {
        return Err(AppError::from_status(status.as_u16()));
    }
    serde_json::from_str(&text).map_err(|_| AppError::BadResponse)
}

/// エージェント用の上限。ツール結果を挟むため通常の対話より長く待つ。
const AGENT_MAX_TOKENS: u32 = 8192;
const AGENT_TIMEOUT: Duration = Duration::from_secs(120);

/// ツール使用付きの1回分の応答。`content`はthinking等を含む生のブロックで、
/// 次のリクエストへそのまま`assistant`メッセージとして戻す必要がある。
#[derive(Debug)]
pub struct ToolTurn {
    pub content: Vec<Value>,
    pub stop_reason: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
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
    let (input_tokens, output_tokens) = usage_of(Provider::Anthropic, body);
    Some(ToolTurn {
        content: body.get("content")?.as_array()?.clone(),
        stop_reason: body.get("stop_reason")?.as_str()?.to_owned(),
        input_tokens,
        output_tokens,
    })
}

/// Anthropic専用のツール使用API。`messages`と`tools`はMessages APIのJSON形式のまま渡す。
pub async fn complete_with_tools(
    model: &str,
    system: &str,
    messages: &[Value],
    tools: &[Value],
) -> Result<ToolTurn, AppError> {
    let provider = Provider::Anthropic;
    async {
        let key = secrets::get_key(provider)?
            .ok_or_else(|| AppError::KeyMissing(provider.as_str().to_owned()))?;
        let client = reqwest::Client::builder()
            .timeout(AGENT_TIMEOUT)
            .build()
            .map_err(|_| AppError::Network)?;
        let url = "https://api.anthropic.com/v1/messages";
        let request = json!({
            "model": model,
            "max_tokens": AGENT_MAX_TOKENS,
            "system": system,
            "messages": messages,
            "tools": tools,
        });
        let res = client
            .post(url)
            .header("x-api-key", &key)
            .header("anthropic-version", "2023-06-01")
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                // 応答が無いまま失敗した場合も、どの要求で止まったかをデバッグログに残す
                debug_log::record_api(provider.as_str(), url, &request, 0, &Value::String(e.to_string()));
                AppError::from_reqwest(e)
            })?;
        let body = read_json(provider.as_str(), url, &request, res).await?;
        parse_tool_turn(&body).ok_or_else(|| {
            log::error!("unexpected anthropic tool response: {body}");
            AppError::BadResponse
        })
    }
    .await
    .map_err(|e| e.with_source(provider.as_str()))
}

pub async fn complete(
    provider: Provider,
    model: &str,
    system: Option<&str>,
    messages: &[ChatMessage],
) -> Result<Completion, AppError> {
    complete_inner(provider, model, system, messages)
        .await
        .map_err(|e| e.with_source(provider.as_str()))
}

async fn complete_inner(
    provider: Provider,
    model: &str,
    system: Option<&str>,
    messages: &[ChatMessage],
) -> Result<Completion, AppError> {
    let key = secrets::get_key(provider)?
        .ok_or_else(|| AppError::KeyMissing(provider.as_str().to_owned()))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|_| AppError::Network)?;

    let (url, req, body, extract): (String, _, Value, fn(&Value) -> Option<String>) = match provider {
        Provider::Anthropic => {
            let url = "https://api.anthropic.com/v1/messages";
            let body = match system {
                Some(s) => json!({ "model": model, "max_tokens": MAX_TOKENS, "system": s, "messages": messages }),
                None => json!({ "model": model, "max_tokens": MAX_TOKENS, "messages": messages }),
            };
            (
            url.to_owned(),
            client.post(url).header("x-api-key", &key).header("anthropic-version", "2023-06-01").json(&body),
            body,
            // 先頭が"text"とは限らない(thinking等が前に付く)ため、textブロックを連結する
            |v| {
                let text: String = v
                    .get("content")?
                    .as_array()?
                    .iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|b| b.get("text")?.as_str())
                    .collect();
                (!text.is_empty()).then_some(text)
            },
            )
        }
        Provider::Openai => {
            let url = "https://api.openai.com/v1/chat/completions";
            let mut all: Vec<Value> = system.map(|s| json!({ "role": "system", "content": s })).into_iter().collect();
            all.extend(messages.iter().map(|m| json!(m)));
            let body = json!({ "model": model, "messages": all });
            (
                url.to_owned(),
                client.post(url).bearer_auth(&key).json(&body),
                body,
                |v| Some(v.pointer("/choices/0/message/content")?.as_str()?.to_owned()),
            )
        }
    };

    let res = req.send().await.map_err(AppError::from_reqwest)?;
    let response = read_json(provider.as_str(), &url, &body, res).await?;
    let (input_tokens, output_tokens) = usage_of(provider, &response);
    let text = extract(&response).ok_or_else(|| {
        // 原因調査用。応答本文にAPIキーは含まれない
        log::error!("unexpected {} response: {response}", provider.as_str());
        AppError::BadResponse
    })?;
    Ok(Completion { text, input_tokens, output_tokens })
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
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        });
        let turn = parse_tool_turn(&body).unwrap();
        assert_eq!(turn.stop_reason, "tool_use");
        assert_eq!(turn.text(), "調べます");
        assert_eq!((turn.input_tokens, turn.output_tokens), (10, 5));
        assert_eq!(
            turn.tool_calls(),
            [ToolCall { id: "t1".into(), name: "list_files".into(), input: json!({ "path": "" }) }]
        );
        assert!(parse_tool_turn(&json!({ "content": [] })).is_none());
    }
}
