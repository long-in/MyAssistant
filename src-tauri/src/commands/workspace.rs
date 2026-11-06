use tauri::{AppHandle, Emitter, State};

use crate::{
    services::{
        debug_log,
        workspace::{ActiveWorkspace, Project, WORKSPACE_CHANGED_EVENT, Workspaces},
    },
    utils::error::AppError,
};

/// ワークスペースのプロジェクト一覧(作成順)。
#[tauri::command]
pub fn list_projects(ws: State<'_, Workspaces>) -> Vec<Project> {
    ws.list()
}

/// 現在選択中のワークスペース。
#[tauri::command]
pub fn get_active_workspace(ws: State<'_, Workspaces>) -> ActiveWorkspace {
    ws.active_info()
}

/// プロジェクトを作る(上限30件)。選択はしない。
#[tauri::command]
pub fn create_project(ws: State<'_, Workspaces>, name: String, color: String) -> Result<Project, AppError> {
    ws.create(&name, &color)
}

/// プロジェクトの名称とイメージカラーを変える。選択中なら画面へ通知する。
#[tauri::command]
pub fn update_project(
    app: AppHandle,
    ws: State<'_, Workspaces>,
    id: String,
    name: String,
    color: String,
) -> Result<Project, AppError> {
    let project = ws.update(&id, &name, &color)?;
    if ws.active_info().id == id {
        let _ = app.emit(WORKSPACE_CHANGED_EVENT, ws.active_info());
    }
    Ok(project)
}

/// プロジェクトを、データごと削除する。元に戻せないため、呼び出し側で確認を取ること。選択中ならパーソナルへ戻る。
#[tauri::command]
pub fn delete_project(app: AppHandle, ws: State<'_, Workspaces>, id: String) -> Result<ActiveWorkspace, AppError> {
    let was_active = ws.active_info().id == id;
    ws.delete(&id)?;
    if was_active {
        refresh_after_switch(&app, &ws);
    }
    Ok(ws.active_info())
}

/// ワークスペースを切り替える。`personal`でパーソナルへ戻る。
/// 動いているバックグラウンド処理(ナレッジ整理など)は止めない。会話・音声入力の停止は画面側で行う。
#[tauri::command]
pub fn switch_workspace(app: AppHandle, ws: State<'_, Workspaces>, id: String) -> Result<ActiveWorkspace, AppError> {
    let info = ws.switch(&id)?;
    refresh_after_switch(&app, &ws);
    Ok(info)
}

/// デバッグログの出力先を切替先に合わせ、画面へ通知する。
fn refresh_after_switch(app: &AppHandle, ws: &Workspaces) {
    if let Err(e) = debug_log::refresh(&ws.active()) {
        log::warn!("debug log refresh failed: {e}");
    }
    let _ = app.emit(WORKSPACE_CHANGED_EVENT, ws.active_info());
}
