//! メニューバー(トレイ)常駐。ウィンドウを閉じてもプロセスは残し、トレイから再表示・終了する。

use tauri::{
  AppHandle, Emitter, Manager, WebviewWindow, WindowEvent,
  menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
  tray::TrayIconBuilder,
};

use crate::services::workspace::{ActiveWorkspace, PERSONAL_ID, Workspaces};

const MAIN_WINDOW: &str = "main";
const MENU_SHOW: &str = "show";
const MENU_QUIT: &str = "quit";

/// メニュー項目 ID・表示名・開く画面のパスの対応。表示名は英語に固定する(左メニュー・メニューバーに合わせる)。
const NAV_ITEMS: [(&str, &str, &str); 4] = [
  ("nav-chat", "Chat", "/"),
  ("nav-knowledge", "Knowledge base", "/knowledge"),
  ("nav-setup", "Settings", "/setup"),
  ("nav-shortcuts", "Shortcuts", "/shortcuts"),
];

/// ワークスペース項目のメニュー ID の接頭辞。続きは `personal` またはスペースの ID。
const WORKSPACE_PREFIX: &str = "ws:";

/// フロントエンドへ画面遷移を依頼するイベント名。ペイロードは遷移先パス。
const NAVIGATE_EVENT: &str = "navigate";

/// トレイアイコンを作成し、メインウィンドウの閉じる操作を「隠す」に差し替える。
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
  let menu = build_menu(app)?;

  let tray = TrayIconBuilder::with_id("main")
    .icon(tauri::include_image!("icons/tray.png"))
    // 黒+透過のテンプレート画像。macOS がメニューバーの配色に合わせて描画する。
    .icon_as_template(true)
    .tooltip("MyAssistant")
    .menu(&menu)
    // 左クリック・右クリックのどちらでもメニューを表示する。
    .show_menu_on_left_click(true)
    .on_menu_event(|app, event| match event.id.as_ref() {
      MENU_SHOW => show_main(app),
      MENU_QUIT => app.exit(0),
      id if id.starts_with(WORKSPACE_PREFIX) => {
        show_main(app);
        on_app_menu_event(app, id);
      }
      id => {
        if let Some((_, _, path)) = NAV_ITEMS.iter().find(|(item, ..)| *item == id) {
          show_main(app);
          let _ = app.emit(NAVIGATE_EVENT, *path);
        }
      }
    });
  tray.build(app)?;

  if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
    let w = window.clone();
    window.on_window_event(move |event| {
      if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        hide_window(&w);
      }
    });
  }
  Ok(())
}

/// トレイメニューを作る。表記は英語に固定する。
fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
  let menu = Menu::new(app)?;
  // いまのワークスペース(表示のみ)と、直接切り替えるためのSpaces。ワークスペース関連の表記は英語に固定する
  let (active, _) = workspace_state(app);
  let name = active.space.as_ref().map_or("Personal", |p| p.name.as_str());
  menu.append(&MenuItem::with_id(app, "ws-current", format!("Workspace: {name}"), false, None::<&str>)?)?;
  menu.append(&spaces_submenu(app)?)?;
  menu.append(&PredefinedMenuItem::separator(app)?)?;
  menu.append(&MenuItem::with_id(app, MENU_SHOW, "Open MyAssistant", true, None::<&str>)?)?;
  menu.append(&PredefinedMenuItem::separator(app)?)?;
  for (id, label, _) in NAV_ITEMS {
    menu.append(&MenuItem::with_id(app, id, label, true, None::<&str>)?)?;
  }
  menu.append(&PredefinedMenuItem::separator(app)?)?;
  menu.append(&MenuItem::with_id(app, MENU_QUIT, "Quit", true, None::<&str>)?)?;
  Ok(menu)
}

/// 選択中のワークスペースと、スペース一覧。状態の登録前(起動の途中)はパーソナルだけ。
fn workspace_state(app: &AppHandle) -> (ActiveWorkspace, Vec<crate::services::workspace::Space>) {
  match app.try_state::<Workspaces>() {
    Some(ws) => (ws.active_info(), ws.list()),
    None => (ActiveWorkspace { id: PERSONAL_ID.to_owned(), space: None }, Vec::new()),
  }
}

/// 「Spaces」サブメニュー。パーソナルと各スペースを並べ、選択中にチェックを付ける。クリックで、そのワークスペースへ切り替える。
fn spaces_submenu(app: &AppHandle) -> tauri::Result<Submenu<tauri::Wry>> {
  let (active, spaces) = workspace_state(app);
  let personal = CheckMenuItem::with_id(app, format!("{WORKSPACE_PREFIX}{PERSONAL_ID}"), "Personal", true, active.id == PERSONAL_ID, None::<&str>)?;
  let items = spaces
    .iter()
    .map(|p| CheckMenuItem::with_id(app, format!("{WORKSPACE_PREFIX}{}", p.id), &p.name, true, active.id == p.id, None::<&str>))
    .collect::<tauri::Result<Vec<_>>>()?;
  let separator = PredefinedMenuItem::separator(app)?;
  let mut all: Vec<&dyn IsMenuItem<tauri::Wry>> = vec![&personal];
  if !items.is_empty() {
    all.push(&separator);
    all.extend(items.iter().map(|i| i as &dyn IsMenuItem<tauri::Wry>));
  }
  Submenu::with_items(app, "Spaces", true, &all)
}

/// アプリのメニューバーと、トレイのメニューを作り直す。ワークスペースの切り替えや、スペースの追加・名称変更・削除のあとに呼ぶ。
pub fn refresh(app: &AppHandle) {
  match (build_menu(app), app.tray_by_id("main")) {
    (Ok(menu), Some(tray)) => {
      let _ = tray.set_menu(Some(menu));
    }
    (Err(e), _) => log::warn!("tray menu not rebuilt: {e}"),
    _ => {}
  }
  match app_menu(app) {
    Ok(menu) => {
      let _ = app.set_menu(menu);
    }
    Err(e) => log::warn!("app menu not rebuilt: {e}"),
  }
}

/// アプリのメニューバーの項目が選ばれたとき。「Spaces」の項目なら、そのワークスペースへ切り替える。
pub fn on_app_menu_event(app: &AppHandle, id: &str) {
  if let Some(workspace_id) = id.strip_prefix(WORKSPACE_PREFIX) {
    crate::commands::workspace::switch_from_menu(app, workspace_id);
  }
}

/// macOS のメニューバー。OS の言語に関わらず英語表記に固定する。「Spaces」から、パーソナルや各スペースへ直接切り替えられる。
pub fn app_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
  use tauri::menu::{PredefinedMenuItem as P, Submenu};
  let app_sub = Submenu::with_items(
    app,
    "MyAssistant",
    true,
    &[
      &P::about(app, Some("About MyAssistant"), None)?,
      &P::separator(app)?,
      &P::services(app, Some("Services"))?,
      &P::separator(app)?,
      &P::hide(app, Some("Hide MyAssistant"))?,
      &P::hide_others(app, Some("Hide Others"))?,
      &P::separator(app)?,
      &P::quit(app, Some("Quit MyAssistant"))?,
    ],
  )?;
  let edit = Submenu::with_items(
    app,
    "Edit",
    true,
    &[
      &P::undo(app, Some("Undo"))?,
      &P::redo(app, Some("Redo"))?,
      &P::separator(app)?,
      &P::cut(app, Some("Cut"))?,
      &P::copy(app, Some("Copy"))?,
      &P::paste(app, Some("Paste"))?,
      &P::select_all(app, Some("Select All"))?,
    ],
  )?;
  let view = Submenu::with_items(app, "View", true, &[&P::fullscreen(app, Some("Enter Full Screen"))?])?;
  let window = Submenu::with_items(
    app,
    "Window",
    true,
    &[&P::minimize(app, Some("Minimize"))?, &P::maximize(app, Some("Zoom"))?, &P::separator(app)?, &P::close_window(app, Some("Close Window"))?],
  )?;
  let spaces = spaces_submenu(app)?;
  Menu::with_items(app, &[&app_sub, &edit, &view, &spaces, &window])
}

/// メインウィンドウを前面に出す。Dock アイコンも復帰させる。
fn show_main(app: &AppHandle) {
  #[cfg(target_os = "macos")]
  let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
  if let Some(w) = app.get_webview_window(MAIN_WINDOW) {
    let _ = w.unminimize();
    let _ = w.show();
    let _ = w.set_focus();
  }
}

/// ウィンドウを隠し、macOS では Dock からも消してメニューバーのみにする。
fn hide_window(window: &WebviewWindow) {
  let _ = window.hide();
  #[cfg(target_os = "macos")]
  let _ = window
    .app_handle()
    .set_activation_policy(tauri::ActivationPolicy::Accessory);
}
