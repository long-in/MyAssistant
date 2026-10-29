//! メニューバー(トレイ)常駐。ウィンドウを閉じてもプロセスは残し、トレイから再表示・終了する。

use tauri::{
  AppHandle, Emitter, Manager, WebviewWindow, WindowEvent,
  menu::{Menu, MenuItem, PredefinedMenuItem},
  tray::TrayIconBuilder,
};

const MAIN_WINDOW: &str = "main";
const MENU_SHOW: &str = "show";
const MENU_QUIT: &str = "quit";

/// メニュー項目 ID・日本語/英語の表示名・開く画面のパスの対応。
const NAV_ITEMS: [(&str, &str, &str, &str); 4] = [
  ("nav-chat", "対話", "Chat", "/"),
  ("nav-setup", "設定", "Settings", "/setup"),
  ("nav-knowledge", "ナレッジ", "Knowledge", "/knowledge"),
  ("nav-shortcuts", "ショートカット", "Shortcuts", "/shortcuts"),
];

/// フロントエンドへ画面遷移を依頼するイベント名。ペイロードは遷移先パス。
const NAVIGATE_EVENT: &str = "navigate";

/// トレイアイコンを作成し、メインウィンドウの閉じる操作を「隠す」に差し替える。
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
  let menu = build_menu(app, "ja")?;

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
      id => {
        if let Some((_, _, _, path)) = NAV_ITEMS.iter().find(|(item, ..)| *item == id) {
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

/// 言語(`ja` / `en`)に合わせたトレイメニューを作る。`en` 以外は日本語。
fn build_menu(app: &AppHandle, lang: &str) -> tauri::Result<Menu<tauri::Wry>> {
  let en = lang == "en";
  let pick = |ja: &'static str, en_label: &'static str| if en { en_label } else { ja };
  let menu = Menu::new(app)?;
  menu.append(&MenuItem::with_id(app, MENU_SHOW, pick("MyAssistant を開く", "Open MyAssistant"), true, None::<&str>)?)?;
  menu.append(&PredefinedMenuItem::separator(app)?)?;
  for (id, ja, en_label, _) in NAV_ITEMS {
    menu.append(&MenuItem::with_id(app, id, pick(ja, en_label), true, None::<&str>)?)?;
  }
  menu.append(&PredefinedMenuItem::separator(app)?)?;
  menu.append(&MenuItem::with_id(app, MENU_QUIT, pick("終了", "Quit"), true, None::<&str>)?)?;
  Ok(menu)
}

/// フロントエンドの表示言語に合わせて、トレイメニューを作り直す。
#[tauri::command]
pub fn set_tray_language(app: AppHandle, lang: String) -> Result<(), String> {
  let menu = build_menu(&app, &lang).map_err(|e| e.to_string())?;
  let tray = app.tray_by_id("main").ok_or("tray not found")?;
  tray.set_menu(Some(menu)).map_err(|e| e.to_string())
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
