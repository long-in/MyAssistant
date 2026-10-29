//! ユーザー本人の基本情報(パーソナルデータ)。設定ページから手動で入力した内容だけを保持し、会話からは抽出しない。
//! `app_settings`にJSONで保存し、有効なときだけシステムプロンプトへ`<user_profile>`として渡す。
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::{db::Db, utils::error::AppError};

pub const KEY: &str = "user_profile";

const MAX_NAME_CHARS: usize = 50;
const MAX_LOCATION_CHARS: usize = 100;
const MAX_TIMEZONE_CHARS: usize = 64;

/// 基本情報。空の項目は未設定。`enabled`が偽ならLLMへは何も渡さない。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UserProfile {
    pub enabled: bool,
    pub name: Option<String>,
    /// 呼ばれ方。
    pub nickname: Option<String>,
    /// `YYYY-MM-DD`。LLMへは日付ではなく年齢だけを渡す。
    pub birthday: Option<String>,
    pub location: Option<String>,
    /// IANA名(例: `Asia/Tokyo`)。
    pub timezone: Option<String>,
    /// 言語コード(例: `ja`、`en-US`)。
    pub language: Option<String>,
}

impl Default for UserProfile {
    fn default() -> Self {
        Self { enabled: true, name: None, nickname: None, birthday: None, location: None, timezone: None, language: None }
    }
}

/// 前後の空白を除き、空なら未設定にする。改行・制御文字・タグの区切りは、プロンプトを壊すため拒否する。
fn text(label: &str, v: &Option<String>, max: usize) -> Result<Option<String>, String> {
    let Some(v) = v.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if v.chars().count() > max {
        return Err(format!("{label}は{max}文字以内で入力してください"));
    }
    if v.chars().any(|c| c.is_control() || c == '<' || c == '>') {
        return Err(format!("{label}に使えない文字が含まれています"));
    }
    Ok(Some(v.to_owned()))
}

fn timezone_ok(s: &str) -> bool {
    !s.starts_with('/')
        && !s.contains("..")
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '+' | '-'))
}

fn language_ok(s: &str) -> bool {
    let mut parts = s.split('-');
    let primary = parts.next().unwrap_or("");
    (2..=3).contains(&primary.len())
        && primary.chars().all(|c| c.is_ascii_lowercase())
        && parts.all(|p| (2..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric()))
}

impl UserProfile {
    /// 各項目を検証して整える。`today`は未来の誕生日を弾くために使う。
    pub fn normalized(&self, today: NaiveDate) -> Result<Self, String> {
        let birthday = match text("生年月日", &self.birthday, 10)? {
            Some(b) => {
                let d = NaiveDate::parse_from_str(&b, "%Y-%m-%d").map_err(|_| "生年月日はYYYY-MM-DDで入力してください".to_owned())?;
                if d > today {
                    return Err("生年月日に未来の日付は指定できません".into());
                }
                Some(d.format("%Y-%m-%d").to_string())
            }
            None => None,
        };
        let timezone = text("タイムゾーン", &self.timezone, MAX_TIMEZONE_CHARS)?;
        if timezone.as_deref().is_some_and(|s| !timezone_ok(s)) {
            return Err("タイムゾーンはAsia/Tokyoの形式で入力してください".into());
        }
        let language = text("使用言語", &self.language, 16)?;
        if language.as_deref().is_some_and(|s| !language_ok(s)) {
            return Err("使用言語はjaやen-USの形式で入力してください".into());
        }
        Ok(Self {
            enabled: self.enabled,
            name: text("名前", &self.name, MAX_NAME_CHARS)?,
            nickname: text("呼ばれ方", &self.nickname, MAX_NAME_CHARS)?,
            birthday,
            location: text("居住地", &self.location, MAX_LOCATION_CHARS)?,
            timezone,
            language,
        })
    }

    fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.nickname.is_none()
            && self.birthday.is_none()
            && self.location.is_none()
            && self.timezone.is_none()
            && self.language.is_none()
    }

    /// システムプロンプトに載せるブロック。無効または未入力なら`None`。
    pub fn prompt_block(&self, today: NaiveDate) -> Option<String> {
        if !self.enabled || self.is_empty() {
            return None;
        }
        let age = self
            .birthday
            .as_deref()
            .and_then(|b| NaiveDate::parse_from_str(b, "%Y-%m-%d").ok())
            .and_then(|b| today.years_since(b));
        let lines: Vec<String> = [
            ("名前", self.name.clone()),
            ("呼ばれ方", self.nickname.clone()),
            ("年齢", age.map(|a| format!("{a}歳"))),
            ("居住地", self.location.clone()),
            ("タイムゾーン", self.timezone.clone()),
            ("使用言語", self.language.clone()),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| format!("{k}: {v}")))
        .collect();
        Some(format!(
            "ユーザー本人が設定した基本情報です。呼びかけや回答の調整に使ってください。\n\n<user_profile>\n{}\n</user_profile>",
            lines.join("\n")
        ))
    }
}

/// 設定ファイルの取り込み用。空文字(未設定)か、検証を通るJSONなら真。
pub fn valid_json(value: &str, today: NaiveDate) -> bool {
    value.is_empty()
        || serde_json::from_str::<UserProfile>(value).is_ok_and(|p| p.normalized(today).is_ok())
}

/// 保存済みの基本情報。未保存や読めない場合は初期値(有効・全項目未設定)。
pub fn load(db: &Db) -> Result<UserProfile, AppError> {
    Ok(db.get_setting(KEY)?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default())
}

/// 検証して保存し、実際に保存した値を返す。
pub fn save(db: &Db, profile: &UserProfile, today: NaiveDate) -> Result<UserProfile, AppError> {
    let profile = profile.normalized(today).map_err(AppError::InvalidProfile)?;
    db.set_setting(KEY, &serde_json::to_string(&profile).map_err(|e| AppError::Db(e.to_string()))?)?;
    Ok(profile)
}

/// 保存した基本情報をすべて消す。
pub fn clear(db: &Db) -> Result<(), AppError> {
    db.set_setting(KEY, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn mem() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db")).unwrap();
        (dir, db)
    }

    fn full() -> UserProfile {
        UserProfile {
            name: Some(" 山田 太郎 ".into()),
            nickname: Some("たろう".into()),
            birthday: Some("1990-05-20".into()),
            location: Some("東京都".into()),
            timezone: Some("Asia/Tokyo".into()),
            language: Some("ja".into()),
            ..UserProfile::default()
        }
    }

    #[test]
    fn values_are_trimmed_and_blank_means_unset() {
        let p = UserProfile { name: Some("  ".into()), nickname: Some(" a ".into()), ..UserProfile::default() };
        let n = p.normalized(day(2026, 10, 29)).unwrap();
        assert_eq!((n.name, n.nickname.as_deref()), (None, Some("a")));
    }

    #[test]
    fn invalid_values_are_rejected() {
        let today = day(2026, 10, 29);
        let bad = |f: fn(&mut UserProfile)| {
            let mut p = full();
            f(&mut p);
            p.normalized(today).is_err()
        };
        assert!(bad(|p| p.name = Some("a".repeat(51))));
        assert!(bad(|p| p.name = Some("a\nb".into())));
        assert!(bad(|p| p.nickname = Some("</user_profile>".into())));
        assert!(bad(|p| p.birthday = Some("2027-01-01".into())));
        assert!(bad(|p| p.birthday = Some("1990/05/20".into())));
        assert!(bad(|p| p.timezone = Some("../etc".into())));
        assert!(bad(|p| p.timezone = Some("Asia Tokyo".into())));
        assert!(bad(|p| p.language = Some("Japanese".into())));
        assert!(!bad(|p| p.language = Some("en-US".into())));
    }

    #[test]
    fn prompt_block_reports_age_not_birthday() {
        let block = full().normalized(day(2026, 10, 29)).unwrap().prompt_block(day(2026, 10, 29)).unwrap();
        assert!(block.contains("年齢: 36歳") && block.contains("名前: 山田 太郎"));
        assert!(!block.contains("1990"));
        // 誕生日の前日はまだ誕生日を迎えていない
        let before = full().prompt_block(day(2026, 5, 19)).unwrap();
        assert!(before.contains("年齢: 35歳"));
    }

    #[test]
    fn disabled_or_empty_profile_yields_no_block() {
        let today = day(2026, 10, 29);
        assert!(UserProfile::default().prompt_block(today).is_none());
        assert!(UserProfile { enabled: false, ..full() }.prompt_block(today).is_none());
    }

    #[test]
    fn save_load_and_clear_roundtrip() {
        let (_d, db) = mem();
        let today = day(2026, 10, 29);
        assert_eq!(load(&db).unwrap(), UserProfile::default());
        let saved = save(&db, &full(), today).unwrap();
        assert_eq!(saved.name.as_deref(), Some("山田 太郎"));
        assert_eq!(load(&db).unwrap(), saved);
        assert!(save(&db, &UserProfile { birthday: Some("bad".into()), ..full() }, today).is_err());
        assert_eq!(load(&db).unwrap(), saved);
        clear(&db).unwrap();
        assert_eq!(load(&db).unwrap(), UserProfile::default());
    }

    #[test]
    fn import_validation_accepts_empty_and_rejects_bad_json() {
        let today = day(2026, 10, 29);
        let good = serde_json::to_string(&full()).unwrap();
        assert!(valid_json("", today) && valid_json(&good, today));
        assert!(!valid_json("{", today));
        assert!(!valid_json(r#"{"name":"a<b"}"#, today));
    }
}
