use std::{path::Path, time::Duration};

use serde_json::Value;
use tauri::{AppHandle, State, ipc::Channel};

use crate::{
    services::{
        agent::{self, AgentEvent, AgentOutcome, Env},
        agent_run::RunSnapshot,
        attention::{self, AttentionKind},
        cancel::CancelToken,
        llm_client,
        secrets::Provider,
        workspace::{Ctx, WorkspaceCtx, Workspaces},
    },
    utils::error::AppError,
};

/// 承認の待ち時間上限。超えたら拒否として扱う。そのワークスペースを選んでいる間だけ数える
/// (ほかのワークスペースにいる間は止まり、戻れば残りの時間で続く)。
const APPROVAL_TIMEOUT_SECS: u32 = 120;

/// 承認への応答を待つ。画面からの応答・時間切れ・中断のどれかが先に来るまで待ち、応答が無ければ拒否(`false`)。
/// 時間切れは、`is_foreground`が真の間(そのワークスペースを選んでいる間)だけ数える。
async fn wait_for_answer(
    rx: &mut tokio::sync::oneshot::Receiver<bool>,
    cancel: &CancelToken,
    is_foreground: &impl Fn() -> bool,
    limit_secs: u32,
) -> bool {
    let mut waited = 0;
    loop {
        tokio::select! {
            answer = &mut *rx => return matches!(answer, Ok(true)),
            () = cancel.cancelled() => return false,
            () = tokio::time::sleep(Duration::from_secs(1)) => {
                if is_foreground() {
                    waited += 1;
                    if waited >= limit_secs {
                        return false;
                    }
                }
            }
        }
    }
}

/// 音声対話の1ターンをファイル操作ツール付きで実行する(Claude専用。Anthropic直接またはBedrock)。
/// `history`は直前までの会話(`user`/`assistant`のテキスト)で、今回の発話は`user_text`で渡す。
/// `web_access`が有効なときは、インターネット検索・ページ取得(サーバーツール)も渡す。Bedrockでは使えない。
/// `shell_access`が有効なときは、作業計画の提示とシェルコマンドの実行も渡す。
/// 進捗(ツール呼び出し・承認要求・コマンド出力)は`on_event`へ流す。実行中は`agent_cancel`で中断できる。
/// `is_foreground`は、このワークスペースを選んでいるかどうか。承認待ちの時間切れを数えるかどうかを決める。
pub async fn run_with_tools(
    ctx: &WorkspaceCtx,
    root: &Path,
    provider: Provider,
    model: &str,
    system: &str,
    web_access: Option<(u32, u32)>,
    shell_access: bool,
    mut history: Vec<Value>,
    user_text: &str,
    on_event: &(impl Fn(AgentEvent) + Sync),
    on_usage: impl Fn(u64, u64),
    is_foreground: impl Fn() -> bool,
) -> Result<AgentOutcome, AppError> {
    let state = &ctx.shared.agent;
    std::fs::create_dir_all(root).map_err(|e| AppError::Io(e.to_string()))?;
    let mut system = format!("{system}\n\n{}", agent::TOOL_GUIDE);
    let mut tools = agent::tool_definitions(shell_access);
    if shell_access {
        system.push_str(&format!("\n\n{}", agent::shell_guide()));
    }
    // 検索・ページ取得はAnthropicのサーバーツールで、Bedrockには無い
    if let Some((search_max, fetch_max)) = web_access.filter(|_| provider == Provider::Anthropic) {
        system.push_str(&format!("\n\n{}", agent::WEB_GUIDE));
        tools.extend(agent::web_tool_definitions(search_max, fetch_max));
    }
    let (system, tools) = (&system, &tools);
    let cancel = CancelToken::default();
    state.set_cancel(cancel.clone());
    let env = Env::new(root).with_cancel(cancel.clone()).with_shell(shell_access);
    let is_foreground = &is_foreground;
    let result = agent::run(
        &env,
        &mut history,
        user_text,
        |msgs| {
            let on_usage = &on_usage;
            async move {
                let turn = llm_client::complete_with_tools(ctx, provider, model, system, &msgs, tools).await?;
                // 途中で失敗しても、ここまでに使った分は記録する
                on_usage(turn.input_tokens, turn.output_tokens);
                Ok(turn)
            }
        },
        |ask| {
            let id = ask.id.clone();
            let mut rx = state.register_approval(&id);
            let cancel = cancel.clone();
            async move {
                let answer = wait_for_answer(&mut rx, &cancel, is_foreground, APPROVAL_TIMEOUT_SECS).await;
                state.finish_approval(&id);
                answer
            }
        },
        |event| on_event(event),
    )
    .await;
    state.clear_cancel();
    result
}

/// 承認への応答。待っていない`id`は無視する。
#[tauri::command]
pub fn agent_approve(ctx: Ctx, id: String, approved: bool) {
    ctx.shared.agent.answer(&id, approved);
}

/// 実行中の依頼を中断する。動いているコマンドは強制終了し、承認待ちは拒否として扱う。実行中でなければ何もしない。
#[tauri::command]
pub fn agent_cancel(ctx: Ctx) {
    ctx.shared.agent.cancel();
}

/// 切り替えから戻った画面が、そのワークスペースの依頼の状況を受け取り、続きの進捗を受ける接続を張り直す。
/// 実行中なら以降の進捗と結果が`on_event`へ流れ、承認待ちがあれば`pending`で返る。裏で完了していた結果は`finished`で返る。
#[tauri::command]
pub fn agent_resume(
    ws: State<'_, Workspaces>,
    workspace_id: String,
    on_event: Channel<AgentEvent>,
) -> Result<RunSnapshot, AppError> {
    let ctx = ws.resolve(&workspace_id)?;
    Ok(ctx.shared.agent.snapshot_and_attach(on_event))
}

/// 画面がワークスペースを離れた。以降の進捗は記録だけにして、依頼は止めない。
#[tauri::command]
pub fn agent_detach(app: AppHandle, ws: State<'_, Workspaces>, workspace_id: String) {
    if let Ok(ctx) = ws.resolve(&workspace_id) {
        ctx.shared.agent.detach();
        // 承認待ちのまま別のスペースへ移った: 対応が必要なことを、改めて知らせる
        if let Some(a) = ctx.shared.agent.pending_approval()
            && attention::is_background(&app, &workspace_id)
        {
            attention::notify(&app, &workspace_id, AttentionKind::Approval, Some(&a), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn foreground_wait_times_out_and_denies() {
        let (_tx, mut rx) = oneshot::channel::<bool>();
        let started = std::time::Instant::now();
        assert!(!wait_for_answer(&mut rx, &CancelToken::default(), &|| true, 1).await);
        assert!(started.elapsed() >= Duration::from_secs(1));
    }

    #[tokio::test]
    async fn background_wait_does_not_count_down_and_waits_for_the_answer() {
        let (tx, mut rx) = oneshot::channel::<bool>();
        let foreground = std::sync::Arc::new(AtomicBool::new(false));
        let fg = foreground.clone();
        let waiter = tokio::spawn(async move {
            wait_for_answer(&mut rx, &CancelToken::default(), &|| fg.load(Ordering::SeqCst), 1).await
        });
        // 裏にいる間は、制限(1秒)を超えても拒否にならない
        tokio::time::sleep(Duration::from_millis(2500)).await;
        assert!(!waiter.is_finished(), "裏にいる間は時間切れにならない");
        // 戻ってきて回答すれば、そのまま再開する
        foreground.store(true, Ordering::SeqCst);
        tx.send(true).unwrap();
        assert!(waiter.await.unwrap());
    }

    #[tokio::test]
    async fn cancelling_denies_even_in_the_background() {
        let (_tx, mut rx) = oneshot::channel::<bool>();
        let cancel = CancelToken::default();
        let c = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            c.cancel();
        });
        assert!(!wait_for_answer(&mut rx, &cancel, &|| false, 120).await);
    }
}
