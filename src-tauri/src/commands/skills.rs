//! スキル画面から呼ぶコマンド。保存先は、そのエージェントのナレッジ配下(`services::skills`)。
use serde::Serialize;

use crate::{
    services::{
        batch,
        llm_router::Route,
        skills::{self, Skill},
    },
    utils::error::AppError,
    services::workspace::Ctx,
};

/// スキルの一覧と、選定で毎回送る前置きの概算トークン数。
#[derive(Serialize)]
pub struct SkillsOverview {
    pub skills: Vec<Skill>,
    pub index_tokens: usize,
}

#[tauri::command]
pub fn list_skills(ctx: Ctx) -> Result<SkillsOverview, AppError> {
    let skills = skills::list(&batch::knowledge_root(&ctx)?);
    let index_tokens = skills::index_tokens(&skills);
    Ok(SkillsOverview { skills, index_tokens })
}

/// 新規作成する。同名があれば失敗する。
#[tauri::command]
pub fn create_skill(ctx: Ctx, skill: Skill) -> Result<Skill, AppError> {
    skills::create(&batch::knowledge_root(&ctx)?, skill)
}

/// 既存のスキルを上書きする。
#[tauri::command]
pub fn update_skill(ctx: Ctx, skill: Skill) -> Result<Skill, AppError> {
    skills::update(&batch::knowledge_root(&ctx)?, skill)
}

#[tauri::command]
pub fn delete_skill(ctx: Ctx, name: String) -> Result<(), AppError> {
    skills::delete(&batch::knowledge_root(&ctx)?, &name)
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
