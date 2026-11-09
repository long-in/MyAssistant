//! 端末内のLLM(Qwen 2.5 7B)の取得・削除。取得物は全ワークスペース共通。
use serde::Serialize;
use tauri::ipc::Channel;

use crate::{
    services::local_llm::{self, Progress},
    utils::error::AppError,
};

#[derive(Serialize)]
pub struct LocalLlmStatus {
    installed: bool,
    downloading: bool,
    label: &'static str,
    /// 取得する合計サイズ(バイト)。
    size: u64,
}

#[tauri::command]
pub fn local_llm_status() -> LocalLlmStatus {
    LocalLlmStatus {
        installed: local_llm::installed(),
        downloading: local_llm::downloading(),
        label: local_llm::MODEL_LABEL,
        size: local_llm::download_size(),
    }
}

/// llama-serverとモデルを取得する。進み具合は`on_progress`へ流す。取得済みのモデルは取り直さない。
#[tauri::command]
pub async fn download_local_llm(on_progress: Channel<Progress>) -> Result<(), AppError> {
    local_llm::install(|p| {
        let _ = on_progress.send(p);
    })
    .await
}

/// 取得中なら中断する(取得途中のファイルは消える)。
#[tauri::command]
pub fn cancel_local_llm_download() {
    local_llm::cancel_download();
}

/// サーバーを止め、取得したものをすべて消す。
#[tauri::command]
pub async fn remove_local_llm() -> Result<(), AppError> {
    local_llm::remove().await
}
