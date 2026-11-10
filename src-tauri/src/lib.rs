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
      commands::task::task_approve,
      commands::task::task_cancel,
      commands::task::task_resume,
      commands::task::task_detach,
      commands::backup::backup_create,
      commands::backup::restore_prepare,
      commands::backup::restore_apply,
      commands::backup::restore_discard,
      commands::aws::get_aws_status,
      commands::aws::reveal_secrets_file,
      commands::aws::get_aws_files,
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
      commands::local_llm::local_llm_status,
      commands::local_llm::download_local_llm,
      commands::local_llm::cancel_local_llm_download,
      commands::local_llm::remove_local_llm,
      commands::profile::get_profile,
      commands::profile::save_profile,
      commands::profile::clear_profile,
      commands::agent_data::get_agent_data,
      commands::agent_data::save_agent_data,
      commands::agent_data::get_shared_data,
      commands::agent_data::save_shared_data,
      commands::knowledge::get_batch_interval,
      commands::knowledge::set_batch_interval,
      commands::settings::get_debug_mode,
      commands::settings::set_debug_mode,
      commands::settings_transfer::export_settings,
      commands::settings_transfer::import_settings,
      commands::global_settings::get_global_prefs,
      commands::global_settings::set_global_language,
      commands::global_settings::set_global_os_notifications,
      commands::proxy::get_proxies,
      commands::proxy::save_proxy,
      commands::proxy::delete_proxy,
      commands::proxy::set_default_proxy,
      commands::proxy::get_proxy_choice,
      commands::proxy::set_proxy_choice,
      commands::chat::add_stt_correction,
      commands::chat::list_models,
      commands::chat::load_routing,
      commands::chat::save_routing,
      commands::chat::send_chat,
      commands::chat::get_web_access,
      commands::chat::get_tuning,
      commands::chat::set_tuning,
      commands::chat::set_web_access,
      commands::sandbox::get_sandbox_enabled,
      commands::sandbox::get_sandbox_status,
      commands::sandbox::get_sandbox_files,
      commands::sandbox::get_sandbox_env,
      commands::sandbox::set_sandbox_env,
      commands::sandbox::get_sandbox_image_status,
      commands::sandbox::get_sandbox_dockerfile,
      commands::sandbox::set_sandbox_dockerfile,
      commands::sandbox::get_sandbox_workspace_dir,
      commands::sandbox::get_sandbox_llm,
      commands::sandbox::set_sandbox_llm,
      commands::sandbox::get_sandbox_claude_settings,
      commands::sandbox::set_sandbox_claude_settings,
      commands::sandbox::open_sandbox_workspace_dir,
      commands::sandbox::get_agent_sandbox_dockerfile,
      commands::sandbox::register_agent_sandbox_dockerfile,
      commands::sandbox::set_agent_sandbox_dockerfile,
      commands::sandbox::unregister_agent_sandbox_dockerfile,
      commands::sandbox::build_agent_sandbox_image,
      commands::sandbox::build_sandbox_image,
      commands::sandbox::start_agent_sandbox,
      commands::sandbox::stop_agent_sandbox,
      commands::sandbox::reset_sandbox,
      commands::sandbox::reset_sandbox_dockerfile,
      commands::sandbox::reveal_sandbox_file,
      commands::chat::get_shell_access,
      commands::chat::set_shell_access,
      commands::chat::get_voice_timing,
      commands::chat::set_voice_timing,
      commands::chat::get_base_instructions,
      commands::chat_session::chat_session_load,
      commands::chat_session::chat_session_new,
      commands::prompts::list_prompts,
      commands::prompts::save_prompt,
      commands::prompts::reset_prompt,
      commands::chat::save_base_instructions,
      commands::voice::get_stt_engine,
      commands::voice::set_stt_engine,
      commands::voice::get_stt_language,
      commands::voice::set_stt_language,
      commands::voice::stt_model_ready,
      commands::voice::stt_model_upgradable,
      commands::voice::download_stt_model,
      commands::voice::stt_download_progress,
      commands::voice::voicevox_download_progress,
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
      commands::knowledge::knowledge_dir,
      commands::knowledge::set_knowledge_dir,
      commands::knowledge::list_knowledge,
      commands::skills::list_skills,
      commands::skills::create_skill,
      commands::skills::update_skill,
      commands::skills::delete_skill,
      commands::skills::export_skills,
      commands::skills::preview_import_skills,
      commands::skills::import_skills,
      commands::skills::load_skills_route,
      commands::skills::save_skills_route,
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
      commands::knowledge_all::list_all_knowledge,
      commands::knowledge_all::read_knowledge_in,
      commands::knowledge_all::write_knowledge_in,
      commands::knowledge_all::create_knowledge_file_in,
      commands::knowledge_all::create_knowledge_dir_in,
      commands::knowledge_all::rename_knowledge_in,
      commands::knowledge_all::delete_knowledge_in,
      commands::knowledge_all::reveal_knowledge_in_finder,
      commands::knowledge::load_batch_route,
      commands::knowledge::save_batch_route,
      commands::knowledge::memory_view,
      commands::knowledge::memory_forget,
      commands::workspace::list_agents,
      commands::workspace::list_agent_colors,
      commands::workspace::get_active_workspace,
      commands::workspace::create_agent,
      commands::workspace::update_agent,
      commands::workspace::delete_agent,
      commands::workspace::switch_workspace,
      commands::workspace::copy_workspace_settings,
      commands::workspace::work_enabled,
      commands::workspace::set_work_enabled,
      commands::leader::leader_exists,
      commands::leader::create_leader,
      commands::leader::leader_get_prompt,
      commands::leader::leader_set_prompt,
      commands::leader::leader_get_limit,
      commands::leader::leader_set_limit,
      commands::leader::leader_key_status,
      commands::leader::leader_save_api_key,
      commands::leader::leader_delete_api_key,
      commands::leader::leader_copy_api_key,
      commands::leader::leader_suggest,
      commands::leader::load_suggest_route,
      commands::leader::leader_dispatch,
      commands::leader::leader_overview,
      commands::leader::leader_set_confirmed,
    ])
    .setup(|app| {
      use tauri::Manager;
      let dir = app.path().app_data_dir()?;
      services::legacy_data::migrate(&dir);
      std::fs::create_dir_all(&dir)?;
      // 予約されたリストアは、DBを開く前に差し替える(失敗しても元に戻して通常どおり起動する)
      match services::backup::apply_pending(&dir) {
        Ok(true) => log::info!("backup restored"),
        Ok(false) => {}
        Err(e) => log::warn!("backup restore failed: {e}"),
      }
      services::aws::init(app.handle());
      // 端末内のLLMは、取得物の場所だけを覚えておき、使うときに起動する
      services::local_llm::init(&dir);
      services::sandbox::init(app.handle());
      // パーソナルは従来どおりアプリデータ直下(`timeline.db` / `secrets.json` / `knowledge`)
      let personal = services::workspace::WorkspaceCtx::open(services::workspace::PERSONAL_ID, &dir)?;
      // 全体設定は、初回だけパーソナルの現在値を初期値として取り込む
      if let Err(e) = personal.global.migrate_from(&personal.db) {
        log::warn!("global settings migration failed: {e}");
      }
      // 通信のプロキシは、保存済みの設定を使い始める
      if let Err(e) = services::proxy::init(&dir, &personal.global) {
        log::warn!("proxy init failed: {e}");
      }
      app.manage(services::workspace::Workspaces::new(&dir, personal));
      app.manage(commands::chat::SessionContexts::default());
      services::sandbox::start(app.handle());
      app.manage(services::graph::GraphCache::default());
      app.manage(services::audio::Recorder::default());
      app.manage(services::stt::SttEngine::default());
      app.manage(services::vad::VadEngine::default());
      app.manage(services::voicevox::VoicevoxEngine::default());
      if let Err(e) = services::debug_log::refresh(&services::workspace::active(app.handle())) {
        log::warn!("debug log init failed: {e}");
      }

      tray::setup(app.handle())?;
      // メニューバーは状態の登録前に作られるため、エージェント一覧を含めて作り直す
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
      // 終了時の整理バッチが済んでから(Exitは最後に来る)、端末内のLLMのサーバーを止める
      services::local_llm::shutdown();
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
        // 動いているサンドボックスのコンテナを止める
        let _ = tokio::time::timeout(Duration::from_secs(20), services::sandbox::stop_all_remembering()).await;
        handle.exit(0);
      });
    }
  });
}
