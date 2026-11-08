use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    commands::chat::SessionContexts,
    tray,
    services::{
        debug_log,
        settings_transfer::{self, CopyOptions, CopyReport},
        workspace::{ActiveWorkspace, Deleted, PALETTE, Agent, WORKSPACE_CHANGED_EVENT, Workspaces},
    },
    utils::error::AppError,
};

/// ワークスペースのエージェント一覧(作成順)。
#[tauri::command]
pub fn list_agents(ws: State<'_, Workspaces>) -> Vec<Agent> {
    ws.list()
}

/// エージェントに選べるイメージカラー(30色)。
#[tauri::command]
pub fn list_agent_colors() -> Vec<&'static str> {
    PALETTE.to_vec()
}

/// 現在選択中のワークスペース。
#[tauri::command]
pub fn get_active_workspace(ws: State<'_, Workspaces>) -> ActiveWorkspace {
    ws.active_info()
}

/// エージェントを作る(上限30件)。選択はしない。
#[tauri::command]
pub fn create_agent(app: AppHandle, ws: State<'_, Workspaces>, name: String, color: String) -> Result<Agent, AppError> {
    // エージェントを作る前に、リーダーが必要
    ws.ensure_leader()?;
    let agent = ws.create(&name, &color)?;
    tray::refresh(&app); // メニューバー・トレイのAgentsに加える
    Ok(agent)
}

/// エージェントの名称とイメージカラーを変える。選択中なら画面へ通知する。
#[tauri::command]
pub fn update_agent(
    app: AppHandle,
    ws: State<'_, Workspaces>,
    id: String,
    name: String,
    color: String,
) -> Result<Agent, AppError> {
    let agent = ws.update(&id, &name, &color)?;
    if ws.active_info().id == id {
        let _ = app.emit(WORKSPACE_CHANGED_EVENT, ws.active_info());
    }
    tray::refresh(&app);
    Ok(agent)
}

/// 削除後の状態。`kept_knowledge`は、残したナレッジの場所。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteReport {
    pub active: ActiveWorkspace,
    pub kept_knowledge: Option<String>,
}

/// エージェントを削除する。元に戻せないため、呼び出し側で確認を取ること。選択中ならパーソナルへ戻る。
/// `delete_knowledge`が偽なら、既定の場所のナレッジは消さずに残す。
#[tauri::command]
pub fn delete_agent(
    app: AppHandle,
    ws: State<'_, Workspaces>,
    contexts: State<'_, SessionContexts>,
    id: String,
    delete_knowledge: bool,
) -> Result<DeleteReport, AppError> {
    // 選択中だったかは、削除と同じロックの中で判定される(確認と削除の間に切り替えられても食い違わない)
    let Deleted { was_active, kept_knowledge } = ws.delete(&id, delete_knowledge)?;
    contexts.forget_workspace(&id);
    if was_active {
        refresh_after_switch(&app, &ws);
    } else {
        // 一覧が変わったことは、選択中でなくても画面へ知らせる
        let _ = app.emit(WORKSPACE_CHANGED_EVENT, ws.active_info());
        tray::refresh(&app);
    }
    Ok(DeleteReport { active: ws.active_info(), kept_knowledge })
}

/// ワークが有効か。リーダーを作成していない間も偽ではなく、有効(未保存)を返す。
#[tauri::command]
pub fn work_enabled(ws: State<'_, Workspaces>) -> bool {
    ws.work_enabled()
}

/// ワークの有効/無効を切り替える。無効にしても、リーダー・エージェントのデータは残る。
/// 実行中・承認待ちの依頼があるときは無効にできない(`WorkBusy`)。開いていたら、パーソナルへ戻す。
#[tauri::command]
pub fn set_work_enabled(app: AppHandle, ws: State<'_, Workspaces>, enabled: bool) -> Result<(), AppError> {
    let moved = ws.set_work_enabled(enabled)?;
    if moved {
        refresh_after_switch(&app, &ws);
    } else {
        let _ = app.emit(WORKSPACE_CHANGED_EVENT, ws.active_info());
        tray::refresh(&app);
    }
    Ok(())
}

/// ワークスペースを切り替える。`personal`でパーソナルへ戻る。
/// 動いているバックグラウンド処理(ナレッジ整理など)は止めない。会話・音声入力の停止は画面側で行う。
#[tauri::command]
pub fn switch_workspace(app: AppHandle, ws: State<'_, Workspaces>, id: String) -> Result<ActiveWorkspace, AppError> {
    let info = ws.switch(&id)?;
    refresh_after_switch(&app, &ws);
    Ok(info)
}

/// `from`の設定を`to`へコピーする(`personal`またはエージェントID)。`from`は変更しない。
/// `include_keys`でAPIキー・AWS設定、`include_dictionary`で読み上げ辞書も写す。コピー先の既存の設定は上書きされる。
#[tauri::command]
pub fn copy_workspace_settings(
    app: AppHandle,
    ws: State<'_, Workspaces>,
    from: String,
    to: String,
    include_keys: bool,
    include_dictionary: bool,
) -> Result<CopyReport, AppError> {
    let (src, dst) = (ws.resolve(&from)?, ws.resolve(&to)?);
    let result = settings_transfer::copy(&src, &dst, CopyOptions { include_keys, include_dictionary });
    // 選択中のワークスペースへコピーしたときは、結果にかかわらず、デバッグログの出力先などを現在の設定に合わせる
    if ws.active().id == dst.id {
        refresh_after_switch(&app, &ws);
    }
    result
}

/// メニューバー・トレイの「Agents」から切り替える。画面には `workspace-changed` で伝わる。
pub fn switch_from_menu(app: &AppHandle, id: &str) {
    let ws = app.state::<Workspaces>();
    match ws.switch(id) {
        Ok(_) => refresh_after_switch(app, &ws),
        Err(e) => log::warn!("workspace switch from menu failed: {e}"),
    }
}

/// デバッグログの出力先を切替先に合わせ、メニューバー・トレイの表示を更新し、画面へ通知する。
fn refresh_after_switch(app: &AppHandle, ws: &Workspaces) {
    if let Err(e) = debug_log::refresh(&ws.active()) {
        log::warn!("debug log refresh failed: {e}");
    }
    tray::refresh(app);
    let _ = app.emit(WORKSPACE_CHANGED_EVENT, ws.active_info());
}
