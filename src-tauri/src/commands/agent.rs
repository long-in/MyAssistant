use std::{collections::HashMap, path::Path, sync::Mutex, time::Duration};

use serde_json::Value;
use tauri::{State, ipc::Channel};
use tokio::sync::oneshot;

use crate::{
    services::{
        agent::{self, AgentEvent, AgentOutcome, Env},
        cancel::CancelToken,
        llm_client,
    },
    utils::error::AppError,
};

/// 承認の待ち時間上限。超えたら拒否として扱う。
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);

/// 承認の応答口と、実行中の依頼の中断トークン。
#[derive(Default)]
pub struct AgentState {
    /// 承認要求の`id`ごとに、画面からの応答を待つ。
    approvals: Mutex<HashMap<String, oneshot::Sender<bool>>>,
    /// 実行中の依頼。同時には1つだけ。
    cancel: Mutex<Option<CancelToken>>,
}

/// 音声対話の1ターンをファイル操作ツール付きで実行する(Claude専用)。
/// `history`は直前までの会話(`user`/`assistant`のテキスト)で、今回の発話は`user_text`で渡す。
/// `web_access`が有効なときは、インターネット検索・ページ取得(サーバーツール)も渡す。
/// `shell_access`が有効なときは、作業計画の提示とシェルコマンドの実行も渡す。
/// 進捗(ツール呼び出し・承認要求・コマンド出力)は`on_event`へ流す。実行中は`agent_cancel`で中断できる。
pub async fn run_with_tools(
    state: &AgentState,
    root: &Path,
    model: &str,
    system: &str,
    web_access: Option<(u32, u32)>,
    shell_access: bool,
    mut history: Vec<Value>,
    user_text: &str,
    on_event: &Channel<AgentEvent>,
    on_usage: impl Fn(u64, u64),
) -> Result<AgentOutcome, AppError> {
    std::fs::create_dir_all(root).map_err(|e| AppError::Io(e.to_string()))?;
    let mut system = format!("{system}\n\n{}", agent::TOOL_GUIDE);
    let mut tools = agent::tool_definitions(shell_access);
    if shell_access {
        system.push_str(&format!("\n\n{}", agent::shell_guide()));
    }
    if let Some((search_max, fetch_max)) = web_access {
        system.push_str(&format!("\n\n{}", agent::WEB_GUIDE));
        tools.extend(agent::web_tool_definitions(search_max, fetch_max));
    }
    let (system, tools) = (&system, &tools);
    let cancel = CancelToken::default();
    *state.cancel.lock().unwrap_or_else(|p| p.into_inner()) = Some(cancel.clone());
    let env = Env::new(root).with_cancel(cancel.clone()).with_shell(shell_access);
    let result = agent::run(
        &env,
        &mut history,
        user_text,
        |msgs| {
            let on_usage = &on_usage;
            async move {
                let turn = llm_client::complete_with_tools(model, system, &msgs, tools).await?;
                // 途中で失敗しても、ここまでに使った分は記録する
                on_usage(turn.input_tokens, turn.output_tokens);
                Ok(turn)
            }
        },
        |ask| {
            let id = ask.id.clone();
            let (tx, rx) = oneshot::channel();
            state.approvals.lock().unwrap_or_else(|p| p.into_inner()).insert(id.clone(), tx);
            let cancel = cancel.clone();
            async move {
                // 画面からの応答・時間切れ・中断のどれかが先に来るまで待つ。応答が無ければ拒否
                let answer = tokio::select! {
                    answer = tokio::time::timeout(APPROVAL_TIMEOUT, rx) => matches!(answer, Ok(Ok(true))),
                    () = cancel.cancelled() => false,
                };
                state.approvals.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
                answer
            }
        },
        |event| {
            let _ = on_event.send(event);
        },
    )
    .await;
    state.cancel.lock().unwrap_or_else(|p| p.into_inner()).take();
    result
}

/// 承認への応答。待っていない`id`は無視する。
#[tauri::command]
pub fn agent_approve(state: State<'_, AgentState>, id: String, approved: bool) {
    if let Some(tx) = state.approvals.lock().unwrap_or_else(|p| p.into_inner()).remove(&id) {
        let _ = tx.send(approved);
    }
}

/// 実行中の依頼を中断する。動いているコマンドは強制終了し、承認待ちは拒否として扱う。実行中でなければ何もしない。
#[tauri::command]
pub fn agent_cancel(state: State<'_, AgentState>) {
    if let Some(token) = state.cancel.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
        token.cancel();
    }
}
