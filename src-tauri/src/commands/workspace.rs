use tauri::{AppHandle, Emitter, State};

use crate::{
    commands::chat::SessionContexts,
    services::{
        debug_log,
        settings_transfer::{self, CopyOptions, CopyReport},
        workspace::{ActiveWorkspace, PALETTE, Project, WORKSPACE_CHANGED_EVENT, Workspaces},
    },
    utils::error::AppError,
};

/// ワークスペースのプロジェクト一覧(作成順)。
#[tauri::command]
pub fn list_projects(ws: State<'_, Workspaces>) -> Vec<Project> {
    ws.list()
}

/// プロジェクトに選べるイメージカラー(30色)。
#[tauri::command]
pub fn list_project_colors() -> Vec<&'static str> {
    PALETTE.to_vec()
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
pub fn delete_project(
    app: AppHandle,
    ws: State<'_, Workspaces>,
    contexts: State<'_, SessionContexts>,
    id: String,
) -> Result<ActiveWorkspace, AppError> {
    // 選択中だったかは、削除と同じロックの中で判定される(確認と削除の間に切り替えられても食い違わない)
    let was_active = ws.delete(&id)?;
    contexts.forget_workspace(&id);
    if was_active {
        refresh_after_switch(&app, &ws);
    } else {
        // 一覧が変わったことは、選択中でなくても画面へ知らせる
        let _ = app.emit(WORKSPACE_CHANGED_EVENT, ws.active_info());
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

/// `from`の設定を`to`へコピーする(`personal`またはプロジェクトID)。`from`は変更しない。
/// `include_keys`でAPIキー・AWS設定、`include_profile`でパーソナルデータも写す。コピー先の既存の設定は上書きされる。
#[tauri::command]
pub fn copy_workspace_settings(
    app: AppHandle,
    ws: State<'_, Workspaces>,
    from: String,
    to: String,
    include_keys: bool,
    include_profile: bool,
) -> Result<CopyReport, AppError> {
    let (src, dst) = (ws.resolve(&from)?, ws.resolve(&to)?);
    let result = settings_transfer::copy(&src, &dst, CopyOptions { include_keys, include_profile });
    // 選択中のワークスペースへコピーしたときは、結果にかかわらず、デバッグログの出力先などを現在の設定に合わせる
    if ws.active().id == dst.id {
        refresh_after_switch(&app, &ws);
    }
    result
}

/// デバッグログの出力先を切替先に合わせ、画面へ通知する。
fn refresh_after_switch(app: &AppHandle, ws: &Workspaces) {
    if let Err(e) = debug_log::refresh(&ws.active()) {
        log::warn!("debug log refresh failed: {e}");
    }
    let _ = app.emit(WORKSPACE_CHANGED_EVENT, ws.active_info());
}
