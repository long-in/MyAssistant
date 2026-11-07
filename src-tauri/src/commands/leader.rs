//! リーダーのコマンド。選択中のワークスペース(`Ctx`)には依存せず、リーダーのコンテキストを明示的に使う。
//! キーの生値は返さない(登録の有無のみ)。スペースの会話・ナレッジ・結果の本文は読まない。
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
        secrets::{self, Provider},
        space_data::Note,
        workspace::{PERSONAL_ID, Space, Workspaces},
    },
    utils::error::AppError,
};

/// リーダーがあるか。
#[tauri::command]
pub fn leader_exists(ws: State<'_, Workspaces>) -> bool {
    ws.leader().is_some()
}

/// リーダーを作る(1つだけ)。スペースを作る前に必要。
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

/// 別のワークスペース(パーソナルまたはスペース)のAPIキーを、リーダーへコピーする(Anthropicのみ。上書き)。
/// 生値は画面へ返さず、コピーできたかだけを返す。コピー元は変更しない。
#[tauri::command]
pub fn leader_copy_api_key(ws: State<'_, Workspaces>, from: String) -> Result<bool, AppError> {
    let leader = ws.ensure_leader()?;
    let src = ws.resolve(&from)?;
    leader.secrets.copy_api_key_from(&src.secrets, Provider::Anthropic)
}

/// 宛先の提案。過去に配布した指示と宛先の履歴だけを根拠にする。根拠が無ければ空。スペースIDを返す。
#[tauri::command]
pub async fn leader_suggest(ws: State<'_, Workspaces>, text: String) -> Result<Vec<String>, AppError> {
    let leader = ws.ensure_leader()?;
    leader::suggest(&leader, &ws.list(), &text).await
}

/// 配布の宛先ごとの結果。
#[derive(Serialize)]
pub struct DispatchResult {
    pub space_id: String,
    /// 依頼を始めたか。
    pub started: bool,
    /// 始めなかった理由: `not_found` / `active` / `busy` / `no_provider` / `error`。
    pub reason: Option<&'static str>,
}

fn not_started(space_id: &str, reason: &'static str) -> DispatchResult {
    DispatchResult { space_id: space_id.to_owned(), started: false, reason: Some(reason) }
}

/// 指示を、選んだスペースへ配布する。宛先は必ず利用者が選ぶ。
/// - 台帳に記録し、各スペースで依頼を始める(画面は介さない。進捗は台帳の状態で見る)。
/// - 選択中のスペースへは配布しない(開いている画面が進捗を受け取れないため)。依頼の最中のスペースにも配布しない。
/// - 承認が必要になったら、そのスペースで答える(リーダーからは代行しない)。
#[tauri::command]
pub fn leader_dispatch(app: AppHandle, ws: State<'_, Workspaces>, text: String, space_ids: Vec<String>) -> Result<Vec<DispatchResult>, AppError> {
    let leader = ws.ensure_leader()?;
    let text = leader::clean_instruction(&text)?;
    let mut ids: Vec<String> = Vec::new();
    for id in space_ids {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() || ids.len() > MAX_TARGETS {
        return Err(AppError::InvalidSpace("targets".into()));
    }
    let spaces = ws.list();
    let batch_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let mut results = Vec::new();
    for id in ids {
        let Some(space) = spaces.iter().find(|s| s.id == id).filter(|_| id != PERSONAL_ID) else {
            results.push(not_started(&id, "not_found"));
            continue;
        };
        if ws.active().id == space.id {
            results.push(not_started(&id, "active"));
            continue;
        }
        match start_dispatch(&app, &ws, &leader, space, &text, &batch_id, &now) {
            Ok(None) => results.push(DispatchResult { space_id: id, started: true, reason: None }),
            Ok(Some(reason)) => results.push(not_started(&id, reason)),
            Err(e) => {
                log::warn!("dispatch to {} failed: {e}", space.name);
                results.push(not_started(&id, "error"));
            }
        }
    }
    Ok(results)
}

/// 1つの宛先で依頼を始める。始められなかった理由があれば`Some`で返す。
fn start_dispatch(
    app: &AppHandle,
    ws: &Workspaces,
    leader: &crate::services::workspace::WorkspaceCtx,
    space: &Space,
    text: &str,
    batch_id: &str,
    now: &str,
) -> Result<Option<&'static str>, AppError> {
    let ctx = ws.resolve(&space.id)?;
    let available = batch::available_providers(&ctx.secrets);
    let saved: Option<Route> = ctx.db.get_setting(ROUTE_KEY)?.and_then(|v| serde_json::from_str(&v).ok());
    let Some(route) = llm_router::effective(&ctx, saved, &available) else { return Ok(Some("no_provider")) };
    let session_id = uuid::Uuid::new_v4().to_string();
    // 台帳へ先に書く(依頼が早く終わっても、終了の記録が行に当たるように)。始められなければ消す
    let row = LedgerRow {
        id: uuid::Uuid::new_v4().to_string(),
        batch_id: batch_id.to_owned(),
        instruction: text.to_owned(),
        space_id: space.id.clone(),
        space_name: space.name.clone(),
        session_id: session_id.clone(),
        status: STATUS_RUNNING.to_owned(),
        confirmed: false,
        created_at: now.to_owned(),
    };
    leader.db.ledger_insert(&row)?;
    if !ctx.shared.agent.try_begin_detached(&session_id, text) {
        leader.db.ledger_delete(&row.id)?;
        return Ok(Some("busy"));
    }
    let (app, text) = (app.clone(), text.to_owned());
    tauri::async_runtime::spawn(async move {
        let contexts = app.state::<SessionContexts>();
        let graphs = app.state::<GraphCache>();
        if let Err(e) = chat::run_recorded(&app, &ctx, &contexts, &graphs, session_id, &text, route).await {
            log::info!("dispatched run failed: {e}");
        }
    });
    Ok(None)
}

/// 宛先にできるスペースの、いまの依頼の状態(直接の指示によるものを含む)。
#[derive(Serialize)]
pub struct SpaceStatus {
    pub id: String,
    pub name: String,
    pub color: String,
    pub status: crate::services::agent_run::RunStatus,
}

/// 台帳(新しい順)と、各スペースのいまの状態。依頼の記録から読むだけで、スペースのLLMは呼ばない。
#[derive(Serialize)]
pub struct Overview {
    pub tasks: Vec<TaskView>,
    pub spaces: Vec<SpaceStatus>,
}

#[tauri::command]
pub fn leader_overview(ws: State<'_, Workspaces>) -> Result<Overview, AppError> {
    let leader = ws.ensure_leader()?;
    let spaces = ws.list();
    let tasks = leader::overview(&leader, &spaces, |id| (ws.run_status(id), ws.run_session(id)))?;
    let spaces = spaces
        .into_iter()
        .map(|s| SpaceStatus { status: ws.run_status(&s.id), id: s.id, name: s.name, color: s.color })
        .collect();
    Ok(Overview { tasks, spaces })
}

/// 台帳の行に「確認済み」の区切りを付ける/外す(利用者の手動の区切り)。
#[tauri::command]
pub fn leader_set_confirmed(ws: State<'_, Workspaces>, id: String, confirmed: bool) -> Result<(), AppError> {
    ws.ensure_leader()?.db.ledger_set_confirmed(&id, confirmed)
}
