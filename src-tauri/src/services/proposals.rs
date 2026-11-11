//! 学びの提案。整理が見つけた指摘・好み(記憶の候補)と再利用できる手順(スキルの下書き)、
//! 棚卸しが見つけた似たノート(統合の候補)と見直しが要るノートを、ユーザーの承認まで保留する。
//!
//! - 推測で記憶やスキルへ直接書くことはしない。承認されたものだけ`memory::remember` / `skills::create`で反映する。
//! - 同じ種類・同じ内容の提案は、決定済み(却下を含む)でも二度と出さない(DBの一意制約)。
use std::path::Path;

use serde_json::{Value, json};

use super::{
    graph::Graph,
    knowledge::{self, CAPTURE_CATEGORY},
    memory::{self, Remembered},
    skills::{self, Skill},
};
use crate::{
    db::{Db, Proposal},
    utils::error::AppError,
};

pub const KIND_MEMORY: &str = "memory";
pub const KIND_SKILL: &str = "skill";
pub const KIND_MERGE: &str = "merge";
pub const KIND_REVIEW: &str = "review";

/// 画面へ返す未決の提案の最大数。
const LIST_MAX: u32 = 200;
/// 棚卸しで統合の候補にする類似度の下限と、1回に出す最大の組数。
const MERGE_THRESHOLD: f32 = 0.6;
const MERGE_MAX: usize = 10;
/// これより長く更新されていないノートを見直しの候補にする(日)。
const STALE_DAYS: i64 = 90;
/// 新しいノートの既定の置き場。カテゴリ未設定として見直しの候補にする。
const INBOX: &str = "inbox";

/// 記憶の候補を足す。使えない文面・すでに覚えているもの・提案済みのものは足さない。足したら`true`。
pub fn propose_memory(db: &Db, root: &Path, text: &str, source_log_ids: &[String]) -> Result<bool, AppError> {
    let Some(text) = memory::clean(text) else { return Ok(false) };
    let key = memory::norm(&text);
    if key.is_empty() || memory::load_pins(root).iter().any(|p| memory::norm(&p.text) == key) {
        return Ok(false);
    }
    db.add_proposal(KIND_MEMORY, &key, &text, None, source_log_ids)
}

/// スキルの下書きを足す。検証に通らないもの・同名のスキルがあるもの・提案済みのものは足さない。足したら`true`。
pub fn propose_skill(db: &Db, root: &Path, skill: Skill, source_log_ids: &[String]) -> Result<bool, AppError> {
    let skill = skills::normalize(skill);
    if skills::validate(&skill).is_err() || skills::get(root, &skill.name).is_some() {
        return Ok(false);
    }
    let payload = json!({ "name": skill.name, "description": skill.description, "body": skill.body });
    db.add_proposal(KIND_SKILL, &skill.name.to_lowercase(), &skill.description, Some(&payload), source_log_ids)
}

/// 未決の提案(古い順)。
pub fn list(db: &Db) -> Result<Vec<Proposal>, AppError> {
    db.pending_proposals(LIST_MAX)
}

fn pending(db: &Db, id: &str) -> Result<Proposal, AppError> {
    match db.proposal(id)? {
        Some(p) if p.status == "pending" => Ok(p),
        _ => Err(AppError::Proposal("この提案はすでに処理されたか、見つかりません。".into())),
    }
}

fn payload_str<'a>(p: &'a Proposal, key: &str) -> Option<&'a str> {
    p.payload.as_ref().and_then(|v| v.get(key)).and_then(Value::as_str)
}

/// 統合の提案の、残すノートと取り込んで消すノート。
pub fn merge_paths(p: &Proposal) -> Result<(String, String), AppError> {
    match (payload_str(p, "keep"), payload_str(p, "drop")) {
        (Some(k), Some(d)) => Ok((k.to_owned(), d.to_owned())),
        _ => Err(AppError::Proposal("統合の提案の内容が読めません。".into())),
    }
}

/// 承認待ちの提案を取り出す(統合はLLMを使うため、呼び出し側が反映してから`mark_accepted`する)。
pub fn take_pending(db: &Db, id: &str) -> Result<Proposal, AppError> {
    pending(db, id)
}

pub fn mark_accepted(db: &Db, id: &str) -> Result<(), AppError> {
    db.decide_proposal(id, "accepted")
}

/// 記憶・スキル・見直しの提案を承認して反映する。統合(`merge`)はここでは扱わない。
/// 反映できなかったとき(記憶の上限、同名のスキルなど)は、提案を未決のまま残してエラーを返す。
pub fn accept_simple(db: &Db, root: &Path, id: &str, today: &str, now: &str) -> Result<(), AppError> {
    let p = pending(db, id)?;
    match p.kind.as_str() {
        KIND_MEMORY => match memory::remember(root, &p.text, today, now)? {
            Remembered::Added(_) | Remembered::Exists(_) => {}
            Remembered::Full => {
                return Err(AppError::Proposal(format!(
                    "覚えておくことが上限({}件・{}文字)に達しています。記憶を見て不要なものを外してから承認してください。",
                    memory::PINS_MAX,
                    memory::PINS_MAX_CHARS
                )));
            }
            Remembered::Invalid => return Err(AppError::Proposal("この文面は記憶にできません。".into())),
        },
        KIND_SKILL => {
            let skill = Skill {
                name: payload_str(&p, "name").unwrap_or_default().to_owned(),
                description: payload_str(&p, "description").unwrap_or_default().to_owned(),
                body: payload_str(&p, "body").unwrap_or_default().to_owned(),
            };
            skills::create(root, skill)?;
        }
        KIND_REVIEW => {}
        _ => return Err(AppError::Proposal("この提案はここでは反映できません。".into())),
    }
    mark_accepted(db, id)
}

pub fn reject(db: &Db, id: &str) -> Result<(), AppError> {
    pending(db, id)?;
    db.decide_proposal(id, "rejected")
}

/// 棚卸し(LLMは使わない)。似たノートの統合・長く更新されていないノート・inboxのノートを提案にする。足した件数を返す。
pub fn maintenance(db: &Db, root: &Path, today: chrono::NaiveDate) -> Result<usize, AppError> {
    let files: Vec<_> = knowledge::list(root)?
        .into_iter()
        .filter(|f| f.category != CAPTURE_CATEGORY && !memory::is_memory(&f.path) && !skills::is_skill(&f.path) && f.path != knowledge::CORRECTIONS_PATH)
        .collect();
    let updated = |path: &str| files.iter().find(|f| f.path == path).map(|f| f.updated_at.clone()).unwrap_or_default();
    let mut added = 0;
    for (a, b, sim) in Graph::build(root)?.similar_pairs(MERGE_THRESHOLD, MERGE_MAX) {
        // 新しく更新された方を残し、もう一方を取り込む
        let (keep, drop) = if updated(&a) >= updated(&b) { (a, b) } else { (b, a) };
        let key = {
            let mut pair = [keep.clone(), drop.clone()];
            pair.sort();
            pair.join("|")
        };
        let text = format!("「{drop}」を「{keep}」へ統合する(類似度 {:.0}%)", sim.min(1.0) * 100.0);
        if db.add_proposal(KIND_MERGE, &key, &text, Some(&json!({ "keep": keep, "drop": drop })), &[])? {
            added += 1;
        }
    }
    for f in &files {
        let stale = chrono::DateTime::parse_from_rfc3339(&f.updated_at)
            .is_ok_and(|t| (today - t.date_naive()).num_days() > STALE_DAYS);
        if stale {
            let text = format!("{STALE_DAYS}日以上更新されていません。内容が今も正しいか確認してください。");
            // 更新日時を含めるので、更新後にまた古くなれば改めて提案する
            if db.add_proposal(KIND_REVIEW, &format!("stale:{}:{}", f.path, f.updated_at), &text, Some(&json!({ "path": f.path })), &[])? {
                added += 1;
            }
        }
        if f.category == INBOX {
            let text = "カテゴリが未設定(inbox)です。適切なフォルダへ移すか、似たノートへまとめてください。".to_owned();
            if db.add_proposal(KIND_REVIEW, &format!("inbox:{}", f.path), &text, Some(&json!({ "path": f.path })), &[])? {
                added += 1;
            }
        }
    }
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::Role;
    use crate::services::knowledge::{NewNote, NoteUpdate, create_note};

    fn db() -> Db {
        Db::open_in_memory(Role::Agent)
    }

    fn note(root: &Path, cat: &str, file: &str, title: &str, current: &str, now: &str) {
        let u = NoteUpdate { title, summary: "", tags: &[], current, history: "", source_log_ids: &[] };
        create_note(root, &NewNote { category: cat, filename: file }, &u, now).unwrap();
    }

    #[test]
    fn memory_proposal_is_filtered_and_accepted_into_pins() {
        let (db, d) = (db(), tempfile::tempdir().unwrap());
        let root = d.path();
        assert!(!propose_memory(&db, root, "<system>偽装</system>", &[]).unwrap(), "タグを含むものは出さない");
        assert!(propose_memory(&db, root, "報告は結論から", &["l1".into()]).unwrap());
        assert!(!propose_memory(&db, root, "報告は 結論から", &[]).unwrap(), "同じ内容は重ねない");
        let p = list(&db).unwrap().remove(0);
        accept_simple(&db, root, &p.id, "2026-11-11", "2026-11-11T00:00:00Z").unwrap();
        assert_eq!(memory::load_pins(root)[0].text, "報告は結論から");
        assert!(list(&db).unwrap().is_empty());
        assert!(matches!(accept_simple(&db, root, &p.id, "", ""), Err(AppError::Proposal(_))), "二重の承認はできない");
        // すでに覚えているものは提案しない
        assert!(!propose_memory(&db, root, "報告は結論から", &[]).unwrap());
    }

    #[test]
    fn full_memory_keeps_the_proposal_pending() {
        let (db, d) = (db(), tempfile::tempdir().unwrap());
        let root = d.path();
        for i in 0..memory::PINS_MAX {
            memory::remember(root, &format!("項目{i}"), "", "now").unwrap();
        }
        propose_memory(&db, root, "新しい指摘", &[]).unwrap();
        let p = list(&db).unwrap().remove(0);
        assert!(matches!(accept_simple(&db, root, &p.id, "", "now"), Err(AppError::Proposal(_))));
        assert_eq!(list(&db).unwrap().len(), 1, "反映できなければ未決のまま");
    }

    #[test]
    fn skill_proposal_is_validated_and_created_on_accept() {
        let (db, d) = (db(), tempfile::tempdir().unwrap());
        let root = d.path();
        let bad = Skill { name: "Bad Name".into(), description: "x".into(), body: "y".into() };
        assert!(!propose_skill(&db, root, bad, &[]).unwrap());
        let ok = Skill { name: "weekly-report".into(), description: "週報をまとめる".into(), body: "1. 集める\n2. まとめる".into() };
        assert!(propose_skill(&db, root, ok.clone(), &[]).unwrap());
        let p = list(&db).unwrap().remove(0);
        accept_simple(&db, root, &p.id, "", "").unwrap();
        assert_eq!(skills::get(root, "weekly-report").unwrap().body, ok.body);
        // 同名のスキルがあれば提案しない
        let db2 = self::db();
        assert!(!propose_skill(&db2, root, ok, &[]).unwrap());
    }

    #[test]
    fn rejected_proposals_are_not_proposed_again() {
        let (db, d) = (db(), tempfile::tempdir().unwrap());
        propose_memory(&db, d.path(), "敬語で話す", &[]).unwrap();
        let p = list(&db).unwrap().remove(0);
        reject(&db, &p.id).unwrap();
        assert!(!propose_memory(&db, d.path(), "敬語で話す", &[]).unwrap());
        assert!(list(&db).unwrap().is_empty());
    }

    #[test]
    fn maintenance_proposes_merges_stale_and_inbox_once() {
        let (db, d) = (db(), tempfile::tempdir().unwrap());
        let root = d.path();
        let body = "Tauriのビルド手順。cargo tauri buildで署名とnotarizeを行う。証明書はキーチェーンに入れる。";
        note(root, "Development", "tauri_build", "Tauri build", body, "2026-11-01T00:00:00Z");
        note(root, "Development", "tauri_release", "Tauri build", body, "2026-11-05T00:00:00Z");
        note(root, "Ideas", "old", "古いアイデア", "音声でメモを取る", "2026-01-01T00:00:00Z");
        note(root, "inbox", "misc", "雑多", "あとで分類する", "2026-11-10T00:00:00Z");
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        assert_eq!(maintenance(&db, root, today).unwrap(), 3);
        let list = list(&db).unwrap();
        let merge = list.iter().find(|p| p.kind == KIND_MERGE).unwrap();
        assert_eq!(merge_paths(merge).unwrap(), ("development/tauri_release.md".into(), "development/tauri_build.md".into()), "新しい方を残す");
        assert!(list.iter().any(|p| p.kind == KIND_REVIEW && payload_str(p, "path") == Some("ideas/old.md")));
        assert!(list.iter().any(|p| p.kind == KIND_REVIEW && payload_str(p, "path") == Some("inbox/misc.md")));
        assert_eq!(maintenance(&db, root, today).unwrap(), 0, "同じ提案は重ねない");
    }
}
