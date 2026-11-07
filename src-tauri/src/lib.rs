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
    .menu(tray::app_menu)
    .on_menu_event(|app, event| tray::on_app_menu_event(app, event.id().as_ref()))
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_notification::init())
    .invoke_handler(tauri::generate_handler![
      commands::agent::agent_approve,
      commands::agent::agent_cancel,
      commands::agent::agent_resume,
      commands::agent::agent_detach,
      commands::aws::get_aws_status,
      commands::aws::preview_aws_config,
      commands::aws::save_aws_config,
      commands::aws::select_aws_profile,
      commands::aws::fetch_aws_models,
      commands::aws::add_aws_model,
      commands::aws::remove_aws_model,
      commands::aws::test_aws_connection,
      commands::aws::submit_aws_mfa,
      commands::aws::cancel_aws_mfa,
      commands::command_policy::get_command_policy,
      commands::command_policy::get_default_command_policy,
      commands::command_policy::set_command_policy,
      commands::debug::delete_debug_logs,
      commands::debug::get_debug_log_dir,
      commands::debug::set_debug_log_dir,
      commands::debug::list_debug_logs,
      commands::debug::read_debug_log,
      commands::settings::get_key_status,
      commands::settings::save_api_key,
      commands::settings::delete_api_key,
      commands::profile::get_profile,
      commands::profile::save_profile,
      commands::profile::clear_profile,
      commands::space_data::get_space_data,
      commands::space_data::save_space_data,
      commands::space_data::get_shared_data,
      commands::space_data::save_shared_data,
      commands::knowledge::get_batch_interval,
      commands::knowledge::set_batch_interval,
      commands::settings::get_debug_mode,
      commands::settings::set_debug_mode,
      commands::settings_transfer::export_settings,
      commands::settings_transfer::import_settings,
      commands::settings_transfer::export_global_settings,
      commands::settings_transfer::import_global_settings,
      commands::global_settings::get_global_prefs,
      commands::global_settings::set_global_language,
      commands::global_settings::set_global_os_notifications,
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
      commands::chat::get_voice_timing,
      commands::chat::set_voice_timing,
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
      commands::voice::transcribe_wake_sample,
      commands::voice::get_wake_config,
      commands::voice::set_wake_config,
      commands::voice::voice_phrases_status,
      commands::voice::generate_voice_phrases,
      commands::voice::voice_phrase_audio,
      commands::voice::synthesize_speech,
      commands::voice::get_tts_dictionary,
      commands::voice::set_tts_dictionary,
      commands::voice::export_tts_dictionary,
      commands::voice::import_tts_dictionary,
      commands::voice::get_tts_engine,
      commands::voice::set_tts_engine,
      commands::voice::get_voicevox_style,
      commands::voice::set_voicevox_style,
      commands::voice::tts_ready,
      commands::voice::voicevox_installed,
      commands::voice::install_voicevox,
      commands::voice::get_model_sources,
      commands::voice::set_whisper_revision,
      commands::voice::set_voicevox_versions,
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
      commands::knowledge::list_other_knowledge,
      commands::knowledge::read_other_knowledge,
      commands::knowledge::list_knowledge_dirs,
      commands::knowledge::write_knowledge,
      commands::knowledge::create_knowledge_file,
      commands::knowledge::create_knowledge_dir,
      commands::knowledge::rename_knowledge,
      commands::knowledge::delete_knowledge,
      commands::knowledge::export_knowledge,
      commands::knowledge::import_knowledge,
      commands::knowledge::reset_knowledge,
      commands::knowledge::run_batch,
      commands::knowledge::load_batch_route,
      commands::knowledge::save_batch_route,
      commands::workspace::list_spaces,
      commands::workspace::list_space_colors,
      commands::workspace::get_active_workspace,
      commands::workspace::create_space,
      commands::workspace::update_space,
      commands::workspace::delete_space,
      commands::workspace::switch_workspace,
      commands::workspace::copy_workspace_settings,
      commands::workspace::work_enabled,
      commands::workspace::set_work_enabled,
      commands::leader::leader_exists,
      commands::leader::create_leader,
      commands::leader::leader_get_prompt,
      commands::leader::leader_set_prompt,
      commands::leader::leader_key_status,
      commands::leader::leader_save_api_key,
      commands::leader::leader_delete_api_key,
      commands::leader::leader_copy_api_key,
      commands::leader::leader_suggest,
      commands::leader::leader_dispatch,
      commands::leader::leader_overview,
      commands::leader::leader_set_confirmed,
    ])
    .setup(|app| {
      use tauri::Manager;
      let dir = app.path().app_data_dir()?;
      services::legacy_data::migrate(&dir);
      std::fs::create_dir_all(&dir)?;
      services::aws::init(app.handle());
      // パーソナルは従来どおりアプリデータ直下(`timeline.db` / `secrets.json` / `knowledge`)
      let personal = services::workspace::WorkspaceCtx::open(services::workspace::PERSONAL_ID, &dir)?;
      // 全体設定は、初回だけパーソナルの現在値を初期値として取り込む
      if let Err(e) = personal.global.migrate_from(&personal.db) {
        log::warn!("global settings migration failed: {e}");
      }
      app.manage(services::workspace::Workspaces::new(&dir, personal));
      app.manage(commands::chat::SessionContexts::default());
      app.manage(services::graph::GraphCache::default());
      app.manage(services::audio::Recorder::default());
      app.manage(services::stt::SttEngine::default());
      app.manage(services::vad::VadEngine::default());
      app.manage(services::voicevox::VoicevoxEngine::default());
      if let Err(e) = services::debug_log::refresh(&services::workspace::active(app.handle())) {
        log::warn!("debug log init failed: {e}");
      }

      tray::setup(app.handle())?;
      // メニューバーは状態の登録前に作られるため、スペース一覧を含めて作り直す
      tray::refresh(app.handle());

      // 整理バッチは、ワークスペースごとの間隔で独立して動かす
      services::scheduler::spawn(app.handle().clone());

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
        let run = async {
          for ctx in handle.state::<services::workspace::Workspaces>().all() {
            if let Err(e) = services::batch::run_guarded(&ctx).await {
              log::warn!("exit batch failed ({}): {e}", ctx.id);
            }
          }
        };
        let _ = tokio::time::timeout(EXIT_BATCH_TIMEOUT, run).await;
        handle.exit(0);
      });
    }
  });
}
