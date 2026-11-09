//! スキル画面から呼ぶコマンド。保存先は、そのエージェントのナレッジ直下の`Skills/`(`services::skills`)。
use serde::Serialize;

use super::settings_transfer::json_path;
use crate::services::settings_transfer::MAX_FILE_BYTES;

use crate::{
    services::{
        batch,
        llm_router::Route,
        prompts::{self, PromptId},
        skills::{self, Skill},
        skills_migration,
    },
    utils::error::AppError,
    services::workspace::Ctx,
};

/// そのエージェントのナレッジのルート。旧形式のスキル・ハーネスが残っていれば、先に移す。
fn skills_root(ctx: &Ctx) -> Result<std::path::PathBuf, AppError> {
    let root = batch::knowledge_root(ctx)?;
    skills_migration::migrate(&root);
    Ok(root)
}

/// スキルの一覧と、選定で毎回送る前置きの概算トークン数。
#[derive(Serialize)]
pub struct SkillsOverview {
    pub skills: Vec<Skill>,
    pub index_tokens: usize,
}

#[tauri::command]
pub fn list_skills(ctx: Ctx) -> Result<SkillsOverview, AppError> {
    let skills = skills::list(&skills_root(&ctx)?);
    let index_tokens = skills::index_tokens(&prompts::load(&ctx.db, PromptId::Skills)?, &skills);
    Ok(SkillsOverview { skills, index_tokens })
}

/// 新規作成する。同名があれば失敗する。
#[tauri::command]
pub fn create_skill(ctx: Ctx, skill: Skill) -> Result<Skill, AppError> {
    skills::create(&skills_root(&ctx)?, skill)
}

/// 既存のスキルを上書きする。
#[tauri::command]
pub fn update_skill(ctx: Ctx, skill: Skill) -> Result<Skill, AppError> {
    skills::update(&skills_root(&ctx)?, skill)
}

#[tauri::command]
pub fn delete_skill(ctx: Ctx, name: String) -> Result<(), AppError> {
    skills::delete(&skills_root(&ctx)?, &name)
}

/// 選定に使うモデル。未設定・使用不可なら既定(Haiku)。使えるモデルが無ければ`None`。
#[tauri::command]
pub fn load_skills_route(ctx: Ctx) -> Option<Route> {
    skills::effective_route(&ctx, &batch::available_providers(&ctx.secrets))
}

#[tauri::command]
pub fn save_skills_route(ctx: Ctx, route: Route) -> Result<(), AppError> {
    skills::save_route(&ctx, &route)
}

/// 全スキルを`dest`のJSONファイルへ書き出す。件数を返す。
#[tauri::command]
pub fn export_skills(ctx: Ctx, dest: String) -> Result<usize, AppError> {
    let dest = json_path(&dest)?;
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let (json, n) = skills::export_json(&skills_root(&ctx)?, now)?;
    std::fs::write(&dest, json).map_err(|e| AppError::Io(e.to_string()))?;
    Ok(n)
}

fn read_import(src: &str) -> Result<String, AppError> {
    let src = json_path(src)?;
    let meta = std::fs::metadata(&src).map_err(|e| AppError::Io(e.to_string()))?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return Err(AppError::InvalidSkill("ファイルが大きすぎます。".into()));
    }
    std::fs::read_to_string(&src).map_err(|e| AppError::Io(e.to_string()))
}

/// 取り込み前の確認。件数と、上書きになる名前を返す。何も変更しない。
#[tauri::command]
pub fn preview_import_skills(ctx: Ctx, src: String) -> Result<skills::ImportPreview, AppError> {
    skills::preview_import(&skills_root(&ctx)?, &read_import(&src)?)
}

/// 書き出したファイルを取り込む。不正な項目が1つでもあれば何も変更しない。同名は`overwrite`なら上書き、そうでなければ飛ばす。
#[tauri::command]
pub fn import_skills(ctx: Ctx, src: String, overwrite: bool) -> Result<skills::ImportReport, AppError> {
    skills::import_json(&skills_root(&ctx)?, &read_import(&src)?, overwrite)
}
