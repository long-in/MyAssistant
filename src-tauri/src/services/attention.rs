//! 開いていないエージェントで、対応が必要になったことを画面へ知らせる。
//!
//! 依頼が完了・失敗したとき、承認が必要になったときに、選択中ではないワークスペースのものなら、
//! 全体のイベント(`ATTENTION_EVENT`)で通知する。画面はトーストで示し、そこから、そのエージェントを開ける。
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use super::{
    tool_loop::{Approval, ApprovalKind},
    workspace::{Workspaces, active},
};

/// 対応が必要になったことを知らせるイベント名。ペイロードは`Attention`。
pub const ATTENTION_EVENT: &str = "task-attention";

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
    /// 対象のワークスペース(`personal`またはエージェントID)。
    pub workspace_id: String,
    /// エージェントの名称とイメージカラー(パーソナルは`None`)。
    pub workspace_name: Option<String>,
    pub workspace_color: Option<String>,
    pub kind: AttentionKind,
    /// 承認の種類(`file` / `plan` / `command` / `allow_command` / `skill_update` / `skill_delete`)。承認のときだけ。
    pub approval_kind: Option<&'static str>,
    /// 承認を求められている内容(ファイルは作成先、コマンドはその内容)を、1行にまとめたもの。計画では空。
    pub detail: Option<String>,
    /// 失敗の内容(`AppError`のシリアライズ)。
    pub error: Option<Value>,
}

/// `workspace_id`が、いま選択中のワークスペースではないか(=裏で動いているか)。
pub fn is_background<R: Runtime>(app: &impl Manager<R>, workspace_id: &str) -> bool {
    active(app).id != workspace_id
}

/// 通知を出す。呼び出す側で、裏のワークスペースのときだけ呼ぶこと。
pub fn notify(app: &AppHandle, workspace_id: &str, kind: AttentionKind, approval: Option<&Approval>, error: Option<Value>) {
    let label = app.try_state::<Workspaces>().and_then(|ws| ws.label(workspace_id));
    let event = Attention {
        workspace_id: workspace_id.to_owned(),
        workspace_name: label.as_ref().map(|(n, _)| n.clone()),
        workspace_color: label.map(|(_, c)| c),
        kind,
        approval_kind: approval.map(|a| kind_name(a.kind)),
        detail: approval.map(detail_of),
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

/// 承認の種類の名前。画面が、言語に合わせた文言を選ぶのに使う。
fn kind_name(kind: ApprovalKind) -> &'static str {
    match kind {
        ApprovalKind::File => "file",
        ApprovalKind::Plan => "plan",
        ApprovalKind::Command => "command",
        ApprovalKind::AllowCommand => "allow_command",
        ApprovalKind::SkillUpdate => "skill_update",
        ApprovalKind::SkillDelete => "skill_delete",
    }
}

/// 承認を求められている内容(ファイルは作成先、コマンドはその内容)を、1行にまとめる。計画は内容が長いので空にする。
pub fn detail_of(a: &Approval) -> String {
    match a.kind {
        ApprovalKind::File => truncate(&a.path),
        ApprovalKind::Plan => String::new(),
        ApprovalKind::Command => truncate(&a.preview),
        ApprovalKind::AllowCommand => truncate(&a.steps.join(", ")),
        ApprovalKind::SkillUpdate | ApprovalKind::SkillDelete => truncate(&a.path),
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
    fn describes_each_kind_in_one_line() {
        let d = |kind, path, preview, steps: &[&str]| detail_of(&approval(kind, path, preview, steps));
        assert_eq!(d(ApprovalKind::File, "notes/a.md", "body", &[]), "notes/a.md");
        assert_eq!(d(ApprovalKind::Plan, "", "overview", &["1", "2"]), "", "計画は内容を載せない");
        assert_eq!(d(ApprovalKind::Command, "", "git\n  status", &[]), "git status");
        assert_eq!(d(ApprovalKind::AllowCommand, "", "", &["git status", "ls"]), "git status, ls");
        assert_eq!(d(ApprovalKind::SkillUpdate, "daily", "全文", &[]), "daily");
        assert_eq!(d(ApprovalKind::SkillDelete, "daily", "全文", &[]), "daily");
        let names = [
            ApprovalKind::File,
            ApprovalKind::Plan,
            ApprovalKind::Command,
            ApprovalKind::AllowCommand,
            ApprovalKind::SkillUpdate,
            ApprovalKind::SkillDelete,
        ]
        .map(kind_name);
        assert_eq!(names, ["file", "plan", "command", "allow_command", "skill_update", "skill_delete"]);
    }

    #[test]
    fn long_details_are_cut_with_an_ellipsis() {
        let long = "x".repeat(500);
        let s = detail_of(&approval(ApprovalKind::Command, "", &long, &[]));
        assert_eq!(s.chars().count(), DETAIL_MAX_CHARS + 1);
        assert!(s.ends_with('…'));
        // 日本語など、複数バイトの文字でも、文字の途中で切らない
        let ja = "あ".repeat(500);
        assert!(detail_of(&approval(ApprovalKind::Command, "", &ja, &[])).ends_with('…'));
    }
}
