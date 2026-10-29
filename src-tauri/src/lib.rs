mod commands;
mod db;
mod services;
mod tray;
mod utils;

use std::{
  sync::atomic::{AtomicBool, Ordering},
  time::Duration,
};

use tauri::{Manager, RunEvent};

/// 終了時バッチの待ち時間上限。
const EXIT_BATCH_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  let app = tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .invoke_handler(tauri::generate_handler![
      tray::set_tray_language,
      commands::agent::agent_approve,
      commands::agent::agent_cancel,
      commands::command_policy::get_command_policy,
      commands::command_policy::get_default_command_policy,
      commands::command_policy::set_command_policy,
      commands::debug::delete_debug_logs,
      commands::debug::list_debug_logs,
      commands::debug::read_debug_log,
      commands::settings::get_key_status,
      commands::settings::save_api_key,
      commands::settings::delete_api_key,
      commands::profile::get_profile,
      commands::profile::save_profile,
      commands::profile::clear_profile,
      commands::knowledge::get_batch_interval,
      commands::knowledge::set_batch_interval,
      commands::settings::get_debug_mode,
      commands::settings::set_debug_mode,
      commands::settings_transfer::export_settings,
      commands::settings_transfer::import_settings,
      commands::chat::add_stt_correction,
      commands::chat::get_usage_this_month,
      commands::chat::list_models,
      commands::chat::load_routing,
      commands::chat::save_routing,
      commands::chat::send_chat,
      commands::chat::get_web_access,
      commands::chat::get_tuning,
      commands::chat::set_tuning,
      commands::chat::set_web_access,
      commands::chat::get_shell_access,
      commands::chat::set_shell_access,
      commands::chat::get_base_instructions,
      commands::chat::save_base_instructions,
      commands::voice::get_stt_engine,
      commands::voice::set_stt_engine,
      commands::voice::get_stt_language,
      commands::voice::set_stt_language,
      commands::voice::stt_model_ready,
      commands::voice::stt_model_upgradable,
      commands::voice::download_stt_model,
      commands::voice::load_proofread_route,
      commands::voice::save_proofread_route,
      commands::voice::start_recording,
      commands::voice::cancel_recording,
      commands::voice::recording_waveform,
      commands::voice::stop_recording_and_transcribe,
      commands::voice::stop_recording_and_check_wake,
      commands::voice::trim_recording,
      commands::voice::get_wake_config,
      commands::voice::set_wake_config,
      commands::voice::voice_phrases_status,
      commands::voice::generate_voice_phrases,
      commands::voice::voice_phrase_audio,
      commands::voice::synthesize_speech,
      commands::voice::get_tts_engine,
      commands::voice::set_tts_engine,
      commands::voice::get_voicevox_style,
      commands::voice::set_voicevox_style,
      commands::voice::tts_ready,
      commands::voice::voicevox_installed,
      commands::voice::install_voicevox,
      commands::voice::voicevox_speakers,
      commands::voice::tts_key_registered,
      commands::voice::save_tts_key,
      commands::voice::delete_tts_key,
      commands::voice::aqua_key_registered,
      commands::voice::save_aqua_key,
      commands::voice::delete_aqua_key,
      commands::knowledge::knowledge_dir,
      commands::knowledge::set_knowledge_dir,
      commands::knowledge::list_knowledge,
      commands::knowledge::read_knowledge,
      commands::knowledge::export_knowledge,
      commands::knowledge::import_knowledge,
      commands::knowledge::reset_knowledge,
      commands::knowledge::run_batch,
      commands::knowledge::load_batch_route,
      commands::knowledge::save_batch_route,
    ])
    .setup(|app| {
      use tauri::Manager;
      let dir = app.path().app_data_dir()?;
      std::fs::create_dir_all(&dir)?;
      services::secrets::init(&dir)?;
      app.manage(db::Db::open(&dir.join("timeline.db"))?);
      app.manage(commands::chat::SessionContexts::default());
      app.manage(services::graph::GraphCache::default());
      app.manage(commands::agent::AgentState::default());
      app.manage(services::audio::Recorder::default());
      app.manage(services::stt::SttEngine::default());
      app.manage(services::vad::VadEngine::default());
      app.manage(services::voicevox::VoicevoxEngine::default());
      app.manage(services::batch::BatchGuard::default());
      if let Err(e) = services::debug_log::refresh(app.handle()) {
        log::warn!("debug log init failed: {e}");
      }

      tray::setup(app.handle())?;

      let handle = app.handle().clone();
      tauri::async_runtime::spawn(async move {
        loop {
          // 設定変更を次の待ちから反映するため、毎回読み直す
          let minutes = services::batch::interval_minutes(&handle.state::<db::Db>())
            .unwrap_or(services::batch::DEFAULT_INTERVAL_MINUTES);
          tokio::time::sleep(Duration::from_secs(u64::from(minutes) * 60)).await;
          if let Err(e) = services::batch::run_guarded(&handle).await {
            log::warn!("periodic batch failed: {e}");
          }
        }
      });

      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      Ok(())
    })
    .build(tauri::generate_context!())
    .expect("error while building tauri application");

  // 終了要求時に未処理ログを1回だけバッチ処理してから終了する。
  let flushed = AtomicBool::new(false);
  app.run(move |handle, event| {
    // ggmlのMetalリソースは終了処理の静的デストラクタより前に解放しておく。
    if let RunEvent::Exit = event {
      handle.state::<services::stt::SttEngine>().unload();
      handle.state::<services::vad::VadEngine>().unload();
    }
    if let RunEvent::ExitRequested { api, .. } = event
      && !flushed.swap(true, Ordering::SeqCst)
    {
      api.prevent_exit();
      let handle = handle.clone();
      tauri::async_runtime::spawn(async move {
        let run = services::batch::run_guarded(&handle);
        if let Ok(Err(e)) = tokio::time::timeout(EXIT_BATCH_TIMEOUT, run).await {
          log::warn!("exit batch failed: {e}");
        }
        handle.exit(0);
      });
    }
  });
}
