//! ハーネス画面から呼ぶコマンド。保存先は、そのエージェントのナレッジ配下(`services::harness`)。
use serde::Serialize;

use crate::{
    services::{
        batch,
        harness::{self, Broken, Harness},
        workspace::Ctx,
    },
    utils::error::AppError,
};

/// ハーネスの一覧と、定義として使えなかったファイル(理由つき)。
#[derive(Serialize)]
pub struct HarnessOverview {
    pub harnesses: Vec<Harness>,
    pub broken: Vec<Broken>,
}

#[tauri::command]
pub fn list_harnesses(ctx: Ctx) -> Result<HarnessOverview, AppError> {
    let (harnesses, broken) = harness::list_all(&batch::knowledge_root(&ctx)?);
    Ok(HarnessOverview { harnesses, broken })
}

/// 新規作成する。同名があれば失敗する。
#[tauri::command]
pub fn create_harness(ctx: Ctx, harness: Harness) -> Result<Harness, AppError> {
    harness::create(&batch::knowledge_root(&ctx)?, harness)
}

/// 既存のハーネスを上書きする。
#[tauri::command]
pub fn update_harness(ctx: Ctx, harness: Harness) -> Result<Harness, AppError> {
    harness::update(&batch::knowledge_root(&ctx)?, harness)
}

#[tauri::command]
pub fn delete_harness(ctx: Ctx, name: String) -> Result<(), AppError> {
    harness::delete(&batch::knowledge_root(&ctx)?, &name)
}
