//! 全エージェントのナレッジを、切り替えずに一覧・操作するコマンド。
//!
//! 対象のエージェントは`agent_id`で指定する。パスは既存と同じく`services::knowledge`(サンドボックス経由)で解決する。
use serde::Serialize;
use tauri::State;

use crate::{
    services::{
        batch,
        knowledge::{self, KnowledgeFile},
        workspace::{PERSONAL_ID, WorkspaceCtx, Workspaces},
    },
    utils::{error::AppError, path},
};
use std::path::PathBuf;

/// 1エージェント分のナレッジ。`error`があるときは一覧を取れなかった(ほかのエージェントは影響を受けない)。
#[derive(Debug, Serialize)]
pub struct AgentKnowledge {
    pub id: String,
    pub name: String,
    /// イメージカラー(`#rrggbb`)。パーソナルは`None`。
    pub color: Option<String>,
    /// ナレッジの保存先(絶対パス)。
    pub root: String,
    pub files: Vec<KnowledgeFile>,
    pub dirs: Vec<String>,
    pub error: Option<String>,
}

fn collect(ws: &Workspaces, ctx: &WorkspaceCtx, id: &str) -> AgentKnowledge {
    let (name, color) = if id == PERSONAL_ID {
        ("Personal".to_owned(), None)
    } else {
        ws.label(id).map(|(n, c)| (n, Some(c))).unwrap_or_else(|| (id.to_owned(), None))
    };
    let root = batch::knowledge_root(ctx);
    let mut out = AgentKnowledge {
        id: id.to_owned(),
        name,
        color,
        root: root.as_ref().map(|r| r.to_string_lossy().into_owned()).unwrap_or_default(),
        files: Vec::new(),
        dirs: Vec::new(),
        error: None,
    };
    let load = || -> Result<(Vec<KnowledgeFile>, Vec<String>), AppError> {
        let root = root?;
        let mut files = knowledge::list(&root)?;
        files.extend(knowledge::list_other(&root)?);
        let dirs = if root.exists() { knowledge::list_dirs(&root)? } else { Vec::new() };
        Ok((files, dirs))
    };
    match load() {
        Ok((files, dirs)) => {
            out.files = files;
            out.dirs = dirs;
        }
        Err(e) => out.error = Some(e.to_string()),
    }
    out
}

/// パーソナル・リーダー・全エージェントのナレッジ。ワークが無効のあいだはパーソナルだけ。
#[tauri::command]
pub fn list_all_knowledge(ws: State<'_, Workspaces>) -> Vec<AgentKnowledge> {
    ws.batch_ids()
        .into_iter()
        .filter_map(|id| match ws.open_for_knowledge(&id) {
            Ok(ctx) => Some(collect(&ws, &*ctx, &id)),
            Err(e) => {
                log::warn!("knowledge of {id} not listed: {e}");
                None
            }
        })
        .collect()
}

fn root_of(ws: &Workspaces, agent_id: &str) -> Result<PathBuf, AppError> {
    batch::knowledge_root(&*ws.open_for_knowledge(agent_id)?)
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Markdownはノートとして、それ以外は閲覧専用の本文として読む。
#[tauri::command]
pub fn read_knowledge_in(ws: State<'_, Workspaces>, agent_id: String, path: String) -> Result<String, AppError> {
    let root = root_of(&ws, &agent_id)?;
    if path.ends_with(".md") { knowledge::read(&root, &path) } else { knowledge::read_other(&root, &path) }
}

#[tauri::command]
pub fn write_knowledge_in(ws: State<'_, Workspaces>, agent_id: String, path: String, content: String) -> Result<(), AppError> {
    knowledge::write(&root_of(&ws, &agent_id)?, &path, &content)
}

#[tauri::command]
pub fn create_knowledge_file_in(ws: State<'_, Workspaces>, agent_id: String, path: String) -> Result<String, AppError> {
    knowledge::create_file(&root_of(&ws, &agent_id)?, &path, &now())
}

#[tauri::command]
pub fn create_knowledge_dir_in(ws: State<'_, Workspaces>, agent_id: String, path: String) -> Result<(), AppError> {
    knowledge::create_dir(&root_of(&ws, &agent_id)?, &path)
}

#[tauri::command]
pub fn rename_knowledge_in(ws: State<'_, Workspaces>, agent_id: String, from: String, to: String) -> Result<(), AppError> {
    knowledge::rename(&root_of(&ws, &agent_id)?, &from, &to)
}

/// ファイル、またはフォルダ(中身ごと)を削除する。確認は呼び出し側で行うこと。
#[tauri::command]
pub fn delete_knowledge_in(ws: State<'_, Workspaces>, agent_id: String, path: String) -> Result<(), AppError> {
    knowledge::delete(&root_of(&ws, &agent_id)?, &path)
}

/// Finderで開く。`path`があればそのファイル(フォルダ)を選択した状態で、なければエージェントのナレッジフォルダを開く。
/// パスはサンドボックスで解決したものだけを渡す。
#[tauri::command]
pub fn reveal_knowledge_in_finder(ws: State<'_, Workspaces>, agent_id: String, path: Option<String>) -> Result<(), AppError> {
    let root = root_of(&ws, &agent_id)?;
    let mut cmd = std::process::Command::new("/usr/bin/open");
    match path.as_deref().filter(|p| !p.is_empty()) {
        Some(rel) => cmd.arg("-R").arg(path::resolve_existing(&root, rel)?),
        None => {
            std::fs::create_dir_all(&root).map_err(|e| AppError::Io(e.to_string()))?;
            cmd.arg(root.canonicalize().map_err(|e| AppError::Io(e.to_string()))?)
        }
    };
    let status = cmd.status().map_err(|e| AppError::Io(e.to_string()))?;
    if status.success() { Ok(()) } else { Err(AppError::Io(format!("open failed: {status}"))) }
}
