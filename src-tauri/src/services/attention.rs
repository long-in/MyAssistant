//! 開いていないスペースで、対応が必要になったことを画面へ知らせる。
//!
//! 依頼が完了・失敗したとき、承認が必要になったときに、選択中ではないワークスペースのものなら、
//! 全体のイベント(`ATTENTION_EVENT`)で通知する。画面はトーストで示し、そこから、そのスペースを開ける。
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use super::{
    agent::{Approval, ApprovalKind},
    workspace::{Workspaces, active},
};

/// 対応が必要になったことを知らせるイベント名。ペイロードは`Attention`。
pub const ATTENTION_EVENT: &str = "agent-attention";

/// 詳細として載せる文字数の上限。
const DETAIL_MAX_CHARS: usize = 120;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttentionKind {
    /// 承認が必要。
    Approval,
    /// 依頼が完了した。
    Finished,
    /// 依頼が失敗した。
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Attention {
    /// 対象のワークスペース(`personal`またはスペースID)。
    pub workspace_id: String,
    /// スペースの名称とイメージカラー(パーソナルは`None`)。
    pub workspace_name: Option<String>,
    pub workspace_color: Option<String>,
    pub kind: AttentionKind,
    /// 承認を求められている内容の要約。
    pub detail: Option<String>,
    /// 失敗の内容(`AppError`のシリアライズ)。
    pub error: Option<Value>,
}

/// `workspace_id`が、いま選択中のワークスペースではないか(=裏で動いているか)。
pub fn is_background<R: Runtime>(app: &impl Manager<R>, workspace_id: &str) -> bool {
    active(app).id != workspace_id
}

/// 通知を出す。呼び出す側で、裏のワークスペースのときだけ呼ぶこと。
pub fn notify(app: &AppHandle, workspace_id: &str, kind: AttentionKind, detail: Option<String>, error: Option<Value>) {
    let space = app
        .try_state::<Workspaces>()
        .and_then(|ws| ws.list().into_iter().find(|s| s.id == workspace_id));
    let event = Attention {
        workspace_id: workspace_id.to_owned(),
        workspace_name: space.as_ref().map(|s| s.name.clone()),
        workspace_color: space.map(|s| s.color),
        kind,
        detail,
        error,
    };
    if let Err(e) = app.emit(ATTENTION_EVENT, event) {
        log::warn!("attention not sent: {e}");
    }
}

fn truncate(text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= DETAIL_MAX_CHARS {
        return one_line;
    }
    let cut: String = one_line.chars().take(DETAIL_MAX_CHARS).collect();
    format!("{cut}…")
}

/// 承認を求められている内容を、1行にまとめる。
pub fn summarize(a: &Approval) -> String {
    match a.kind {
        ApprovalKind::File => truncate(&format!("Create file: {}", a.path)),
        ApprovalKind::Plan => "Review the plan".to_owned(),
        ApprovalKind::Command => truncate(&format!("Run command: {}", a.preview)),
        ApprovalKind::AllowCommand => truncate(&format!("Allow command: {}", a.steps.join(", "))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approval(kind: ApprovalKind, path: &str, preview: &str, steps: &[&str]) -> Approval {
        Approval {
            id: "t1".into(),
            kind,
            path: path.into(),
            preview: preview.into(),
            steps: steps.iter().map(|s| (*s).to_owned()).collect(),
            reason: String::new(),
        }
    }

    #[test]
    fn summarizes_each_kind_in_one_line() {
        assert_eq!(summarize(&approval(ApprovalKind::File, "notes/a.md", "body", &[])), "Create file: notes/a.md");
        assert_eq!(summarize(&approval(ApprovalKind::Plan, "", "overview", &["1", "2"])), "Review the plan");
        assert_eq!(summarize(&approval(ApprovalKind::Command, "", "git\n  status", &[])), "Run command: git status");
        assert_eq!(summarize(&approval(ApprovalKind::AllowCommand, "", "", &["git status", "ls"])), "Allow command: git status, ls");
    }

    #[test]
    fn long_details_are_cut_with_an_ellipsis() {
        let long = "x".repeat(500);
        let s = summarize(&approval(ApprovalKind::Command, "", &long, &[]));
        assert_eq!(s.chars().count(), DETAIL_MAX_CHARS + 1);
        assert!(s.ends_with('…'));
        // 日本語など、複数バイトの文字でも、文字の途中で切らない
        let ja = "あ".repeat(500);
        assert!(summarize(&approval(ApprovalKind::Command, "", &ja, &[])).ends_with('…'));
    }
}
