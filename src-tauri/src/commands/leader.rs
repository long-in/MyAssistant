//! リーダーのコマンド。選択中のワークスペース(`Ctx`)には依存せず、リーダーのコンテキストを明示的に使う。
//! キーの生値は返さない(登録の有無のみ)。エージェントの会話・ナレッジ・結果の本文は読まない。
use std::{collections::BTreeMap, str::FromStr};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::{
    commands::chat::{self, ROUTE_KEY, SessionContexts},
    db::LedgerRow,
    services::{
        batch,
        graph::GraphCache,
        leader::{self, MAX_TARGETS, STATUS_RUNNING, TaskView},
        llm_router::{self, Route},
        sandbox,
        secrets::{self, Provider},
        agent_data::Note,
        workspace::{Ctx, PERSONAL_ID, Agent, Workspaces},
    },
    utils::error::AppError,
};

/// リーダーがあるか。
#[tauri::command]
pub fn leader_exists(ws: State<'_, Workspaces>) -> bool {
    ws.leader().is_some()
}

/// リーダーを作る(1つだけ)。エージェントを作る前に必要。
#[tauri::command]
pub fn create_leader(app: AppHandle, ws: State<'_, Workspaces>) -> Result<(), AppError> {
    ws.create_leader()?;
    crate::tray::refresh(&app); // メニューバー・トレイに加える
    Ok(())
}

/// リーダープロンプト(行動原理・役割)。未保存なら初期値。
#[tauri::command]
pub fn leader_get_prompt(ws: State<'_, Workspaces>) -> Result<Note, AppError> {
    leader::load_prompt(&ws.ensure_leader()?.db)
}

/// リーダープロンプトを検証して保存し、整えた後の値を返す。次の提案から反映する。
#[tauri::command]
pub fn leader_set_prompt(ws: State<'_, Workspaces>, note: Note) -> Result<Note, AppError> {
    leader::save_prompt(&ws.ensure_leader()?.db, &note)
}

/// 同時に指示できるエージェント数の上限。未保存なら既定値。
#[tauri::command]
pub fn leader_get_limit(ws: State<'_, Workspaces>) -> Result<usize, AppError> {
    leader::load_limit(&ws.ensure_leader()?.db)
}

/// 上限を範囲(1〜エージェントの上限)に収めて保存し、保存した値を返す。次の配布から反映する。
#[tauri::command]
pub fn leader_set_limit(ws: State<'_, Workspaces>, limit: usize) -> Result<usize, AppError> {
    leader::save_limit(&ws.ensure_leader()?.db, limit)
}

/// リーダーのキーの登録有無のみを返す(生値は返さない)。
#[tauri::command]
pub fn leader_key_status(ws: State<'_, Workspaces>) -> Result<BTreeMap<&'static str, bool>, AppError> {
    let leader = ws.ensure_leader()?;
    Provider::ALL.into_iter().map(|p| Ok((p.as_str(), leader.secrets.has_key(p)?))).collect()
}

/// リーダー専用のAPIキーを、疎通確認に成功したときだけ保存する(Anthropicのみ)。
#[tauri::command]
pub async fn leader_save_api_key(ws: State<'_, Workspaces>, provider: String, key: String) -> Result<(), AppError> {
    let leader = ws.ensure_leader()?;
    let provider = Provider::from_str(&provider)?;
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::EmptyKey);
    }
    secrets::verify_key(provider, key).await?;
    leader.secrets.set_key(provider, key)
}

#[tauri::command]
pub fn leader_delete_api_key(ws: State<'_, Workspaces>, provider: String) -> Result<(), AppError> {
    let leader = ws.ensure_leader()?;
    match Provider::from_str(&provider)? {
        Provider::Bedrock => Err(AppError::InvalidProvider(provider)),
        p => leader.secrets.delete_key(p),
    }
}

/// パーソナルのAPIキーを、リーダーへコピーする(Anthropicのみ。上書き)。ワークのエージェントはAnthropicのキーを持たないので、コピー元にできない。
/// 生値は画面へ返さず、コピーできたかだけを返す。コピー元は変更しない。
#[tauri::command]
pub fn leader_copy_api_key(ws: State<'_, Workspaces>, from: String) -> Result<bool, AppError> {
    let leader = ws.ensure_leader()?;
    let src = ws.resolve(&from)?;
    if sandbox::runs_in_sandbox(&src.id) {
        return Err(AppError::InvalidProvider(Provider::Anthropic.as_str().to_owned()));
    }
    leader.secrets.copy_api_key_from(&src.secrets, Provider::Anthropic)
}

/// 宛先の提案。過去に配布した指示と宛先の履歴だけを根拠にする。根拠が無ければ空。エージェントIDを返す。
#[tauri::command]
pub async fn leader_suggest(ws: State<'_, Workspaces>, text: String) -> Result<Vec<String>, AppError> {
    let leader = ws.ensure_leader()?;
    leader::suggest(&leader, &ws.list(), &text).await
}

/// 宛先の提案に使うモデル(軽量なモデルを自動で選ぶ)。設定画面の「使用中の機能」の表示に使う。キーが無ければ`None`。
#[tauri::command]
pub fn load_suggest_route(ctx: Ctx) -> Option<Route> {
    leader::suggest_route(&ctx)
}

/// 配布の宛先ごとの結果。
#[derive(Serialize)]
pub struct DispatchResult {
    pub agent_id: String,
    /// 依頼を始めたか。
    pub started: bool,
    /// 始めなかった理由: `not_found` / `active` / `limit` / `busy` / `no_provider` / `error`。
    pub reason: Option<&'static str>,
}

fn not_started(agent_id: &str, reason: &'static str) -> DispatchResult {
    DispatchResult { agent_id: agent_id.to_owned(), started: false, reason: Some(reason) }
}

/// 指示を、選んだエージェントへ配布する。宛先は必ず利用者が選ぶ。
/// - 台帳に記録し、各エージェントで依頼を始める(画面は介さない。進捗は台帳の状態で見る)。
/// - 同時に指示できるエージェント数(リーダーの設定)を超える宛先へは配布しない(`limit`。順番待ちにはしない)。
///   数えるのは、リーダーが配布した実行中・承認待ちのエージェントだけ。
/// - 選択中のエージェントへは配布しない(開いている画面が進捗を受け取れないため)。依頼の最中のエージェントにも配布しない。
/// - 承認が必要になったら、そのエージェントで答える(リーダーからは代行しない)。
#[tauri::command]
pub fn leader_dispatch(app: AppHandle, ws: State<'_, Workspaces>, text: String, agent_ids: Vec<String>) -> Result<Vec<DispatchResult>, AppError> {
    let leader = ws.ensure_leader()?;
    let text = leader::clean_instruction(&text)?;
    let mut ids: Vec<String> = Vec::new();
    for id in agent_ids {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() || ids.len() > MAX_TARGETS {
        return Err(AppError::InvalidAgent("targets".into()));
    }
    let agents = ws.list();
    // 空き枠: 上限 − リーダーが配布して、いま動いている(実行中・承認待ち)エージェントの数
    let limit = leader::load_limit(&leader.db)?;
    let running = leader::count_active(&leader.db.ledger_list(leader::LEDGER_LIMIT)?, |id| (ws.run_status(id), ws.run_session(id)));
    let mut slots = limit.saturating_sub(running);
    let batch_id = uuid::Uuid::new_v4().to_string();
    let now = crate::db::now();
    let mut results = Vec::new();
    for id in ids {
        let Some(agent) = agents.iter().find(|s| s.id == id).filter(|_| id != PERSONAL_ID) else {
            results.push(not_started(&id, "not_found"));
            continue;
        };
        if ws.active().id == agent.id {
            results.push(not_started(&id, "active"));
            continue;
        }
        if slots == 0 {
            results.push(not_started(&id, "limit"));
            continue;
        }
        match start_dispatch(&app, &ws, &leader, agent, &text, &batch_id, &now) {
            Ok(None) => {
                slots -= 1;
                results.push(DispatchResult { agent_id: id, started: true, reason: None });
            }
            Ok(Some(reason)) => results.push(not_started(&id, reason)),
            Err(e) => {
                log::warn!("dispatch to {} failed: {e}", agent.name);
                results.push(not_started(&id, "error"));
            }
        }
    }
    Ok(results)
}

/// 配布先のエージェントが使う経路。ワークのエージェントは、会話が常にコンテナ内の Claude Code なので、モデルの選択に関わらずコンテナの経路。
/// それ以外(使えるモデルが無ければ`None`)は、保存済みの選択か既定。
fn dispatch_route(ctx: &crate::services::workspace::WorkspaceCtx) -> Result<Option<Route>, AppError> {
    if sandbox::runs_in_sandbox(&ctx.id) {
        return Ok(Some(llm_router::sandbox_route("")));
    }
    let available = batch::available_providers(&ctx.secrets);
    let saved: Option<Route> = ctx.db.get_setting(ROUTE_KEY)?.and_then(|v| serde_json::from_str(&v).ok());
    Ok(llm_router::effective(ctx, saved, &available))
}

/// 1つの宛先で依頼を始める。始められなかった理由があれば`Some`で返す。
fn start_dispatch(
    app: &AppHandle,
    ws: &Workspaces,
    leader: &crate::services::workspace::WorkspaceCtx,
    agent: &Agent,
    text: &str,
    batch_id: &str,
    now: &str,
) -> Result<Option<&'static str>, AppError> {
    let ctx = ws.resolve(&agent.id)?;
    let Some(route) = dispatch_route(&ctx)? else { return Ok(Some("no_provider")) };
    let session_id = uuid::Uuid::new_v4().to_string();
    // 台帳へ先に書く(依頼が早く終わっても、終了の記録が行に当たるように)。始められなければ消す
    let row = LedgerRow {
        id: uuid::Uuid::new_v4().to_string(),
        batch_id: batch_id.to_owned(),
        instruction: text.to_owned(),
        agent_id: agent.id.clone(),
        agent_name: agent.name.clone(),
        session_id: session_id.clone(),
        status: STATUS_RUNNING.to_owned(),
        confirmed: false,
        created_at: now.to_owned(),
    };
    leader.db.ledger_insert(&row)?;
    if !ctx.shared.task.try_begin_detached(&session_id, text) {
        leader.db.ledger_delete(&row.id)?;
        return Ok(Some("busy"));
    }
    let (app, text) = (app.clone(), text.to_owned());
    tauri::async_runtime::spawn(async move {
        let contexts = app.state::<SessionContexts>();
        let graphs = app.state::<GraphCache>();
        if let Err(e) = chat::run_recorded(&app, &ctx, &contexts, &graphs, session_id, &text, route, None).await {
            log::info!("dispatched run failed: {e}");
        }
    });
    Ok(None)
}

/// 宛先にできるエージェントの、いまの依頼の状態(直接の指示によるものを含む)。
#[derive(Serialize)]
pub struct AgentStatus {
    pub id: String,
    pub name: String,
    pub color: String,
    pub status: crate::services::task_run::RunStatus,
}

/// 台帳(新しい順)と、各エージェントのいまの状態。依頼の記録から読むだけで、エージェントのLLMは呼ばない。
#[derive(Serialize)]
pub struct Overview {
    pub tasks: Vec<TaskView>,
    pub agents: Vec<AgentStatus>,
    /// 同時に指示できるエージェント数の上限と、リーダーが配布していま動いているエージェントの数。
    pub limit: usize,
    pub active: usize,
}

#[tauri::command]
pub fn leader_overview(ws: State<'_, Workspaces>) -> Result<Overview, AppError> {
    let leader = ws.ensure_leader()?;
    let agents = ws.list();
    let tasks = leader::overview(&leader, &agents, |id| (ws.run_status(id), ws.run_session(id)))?;
    let agents = agents
        .into_iter()
        .map(|s| AgentStatus { status: ws.run_status(&s.id), id: s.id, name: s.name, color: s.color })
        .collect();
    let active = leader::count_active(&leader.db.ledger_list(leader::LEDGER_LIMIT)?, |id| (ws.run_status(id), ws.run_session(id)));
    Ok(Overview { tasks, agents, limit: leader::load_limit(&leader.db)?, active })
}

/// 台帳の行に「確認済み」の区切りを付ける/外す(利用者の手動の区切り)。
#[tauri::command]
pub fn leader_set_confirmed(ws: State<'_, Workspaces>, id: String, confirmed: bool) -> Result<(), AppError> {
    ws.ensure_leader()?.db.ledger_set_confirmed(&id, confirmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_agents_dispatch_through_the_sandbox_even_without_any_api_key() {
        use crate::services::workspace::WorkspaceCtx;
        let (dw, dp) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let work = WorkspaceCtx::open("0b9c3f5e-1d2a-4c6b-8e7f-123456789abc", dw.path()).unwrap();
        assert_eq!(dispatch_route(&work).unwrap(), Some(llm_router::sandbox_route("")));
        // 個人はキーが無ければ始められない
        let personal = WorkspaceCtx::open(crate::services::workspace::PERSONAL_ID, dp.path()).unwrap();
        assert_eq!(dispatch_route(&personal).unwrap(), None);
    }
}
