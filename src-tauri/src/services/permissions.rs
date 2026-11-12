//! API 方式(ホストのツールループ)の権限。Claude Code の settings.json と同じ書式(`permissions.allow` / `ask` / `deny`)で、
//! `run_command`のシェルコマンドと、Web 検索・ページ取得(Anthropic のサーバーツール)の可否を決める。
//! - 保存はエージェントのDB(`app_settings`のキー`api_claude_settings`、JSON の文字列)。コマンドごとに読み直す。
//!   初期値はアプリ側で持つプリセット(`Preset`)から選ぶ。未保存なら、初めて読んだときに標準のプリセットを書き出す。
//! - 判定は Claude Code と同じく deny > ask > allow。どれにも一致しないコマンドは毎回承認し、Web のツールは渡さない。
//! - 設定とは別に、アプリデータ(APIキー・DB・ほかのワークスペース)の場所を指すコマンドは常に拒否する(設定では外せない)。
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::{global_settings::SettingsStore, sandbox_agent};

/// settings.json を保存する設定のキー(エージェントのDB)。空・未設定なら雛形を使う。
pub const KEY: &str = "api_claude_settings";
/// 再帰的に解析する入れ子(`sh -c`、`$(...)`など)の深さの上限。超えたら承認にする。
const MAX_DEPTH: usize = 4;
/// 1リストのルール数と、1ルールの長さの上限。
const MAX_RULES: usize = 500;
const MAX_RULE_CHARS: usize = 300;

/// 雛形で許可する読み取り系のコマンド。
const DEFAULT_ALLOW: [&str; 48] = [
    "ls", "pwd", "cat", "head", "tail", "wc", "file", "stat", "du", "df", "which", "whoami", "date", "uname", "sw_vers",
    "echo", "grep", "rg", "find", "tree", "git status", "git log", "git diff", "git show", "ps", "id", "hostname",
    "uptime", "printenv", "basename", "dirname", "realpath", "readlink", "diff", "cut", "tr", "lsof", "git rev-parse",
    "git ls-files", "git blame", "git remote -v", "git branch --show-current", "git branch -a", "git branch --list",
    "git branch -v", "git tag -l", "git tag --list", "git config --get",
];

/// 雛形で毎回確認する、破壊的になりうる操作。
const DEFAULT_ASK: [&str; 22] = [
    "git push", "git reset --hard", "git clean", "git checkout --", "git restore", "git branch -D", "git stash drop",
    "git stash clear", "git rebase", "mv", "dd", "truncate", "kill", "killall", "pkill", "osascript", "chmod -R",
    "chown -R", "brew uninstall", "brew remove", "npm publish", "cargo publish",
];

/// 雛形で毎回確認する、読み取り系のコマンドに別の動作(ファイルへの書き出し・別コマンドの起動)をさせるオプション。
const DEFAULT_ASK_PATTERNS: [&str; 10] = [
    "Bash(find *-exec*)", "Bash(find *-execdir*)", "Bash(find *-ok*)", "Bash(find *-fprint*)", "Bash(find *-fls*)",
    "Bash(* --output*)", "Bash(rg *--pre*)", "Bash(git *--ext-diff*)", "Bash(git *--textconv*)", "Bash(tree *-o *)",
];

/// 拒否するコマンド。OSの動作に影響するもの、設定の書き換え、`find -delete`。サンドボックスの既定の拒否にも使う。
pub const DEFAULT_DENY: [&str; 22] = [
    "shutdown", "reboot", "halt", "poweroff", "launchctl", "systemsetup", "diskutil", "csrutil", "nvram", "pmset",
    "scutil", "networksetup", "dscl", "spctl", "tmutil", "mount", "defaults write", "defaults delete",
    "defaults import", "defaults rename", "defaults delete-all", "find -delete",
];

/// 削除・権限昇格のコマンド。サンドボックスの既定の拒否にも使う。
pub const DEFAULT_DENY_WORDS: [&str; 7] = ["rm", "rmdir", "unlink", "shred", "sudo", "su", "doas"];

/// 雛形で、引数に含むコマンドを拒否するファイル名(APIキー・DB・認証情報)。
const DEFAULT_PROTECTED: [&str; 7] =
    ["secrets.json", "timeline.db", "sandbox.env", ".aws/credentials", ".ssh/id_", ".netrc", ".gnupg/"];

const PRIVATE_REASON: &str = "アプリのデータ(APIキー・会話の記録など)の場所を扱うコマンドは実行できません。";
const PRIVATE_SEARCH_REASON: &str = "アプリのデータを含む範囲を再帰的に読むコマンドは実行できません。対象のフォルダを絞ってください。";

const SHELLS: [&str; 5] = ["sh", "bash", "zsh", "dash", "ksh"];
const INTERPRETERS: [&str; 9] = ["sh", "bash", "zsh", "dash", "ksh", "python", "python3", "node", "ruby"];
/// コマンドの前に付けて別のコマンドを起動する語。
const WRAPPERS: [&str; 9] = ["command", "builtin", "exec", "nohup", "time", "env", "nice", "xargs", "stdbuf"];
/// サブコマンドごとに性質が変わるツール。許可へは`git log`のようにサブコマンドまで含めて加える。
const SUBCOMMAND_TOOLS: [&str; 11] = ["git", "npm", "pnpm", "yarn", "cargo", "brew", "docker", "gh", "kubectl", "go", "pip"];

/// コマンドの接頭辞を、Claude Code の`Bash(<接頭辞>:*)`のルールにする。区切り(,)・括弧・制御文字を含むものは書式を壊すので除く。
pub fn bash_prefix_rules<'a>(items: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut rules: Vec<String> = Vec::new();
    for p in items.into_iter().map(str::trim) {
        if p.is_empty() || p.contains([',', '(', ')']) || p.chars().any(char::is_control) {
            continue;
        }
        let rule = format!("Bash({p}:*)");
        if !rules.contains(&rule) {
            rules.push(rule);
        }
    }
    rules
}

/// 既定の拒否(OSに影響するコマンドと、削除・権限昇格)。サンドボックスの settings.json の雛形にも使う。
pub fn default_deny_rules() -> Vec<String> {
    bash_prefix_rules(DEFAULT_DENY.iter().chain(DEFAULT_DENY_WORDS.iter()).copied())
}

/// 初期値のプリセット。アプリ側で持ち、画面の「初期値に戻す」で選んで保存する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    /// すべてのコマンドと Web 検索・ページ取得を許可し、確認・拒否は持たない(アプリデータの保護だけは効く)。
    Permissive,
    /// 読み取り系と Web 検索・ページ取得を許可し、破壊的な操作は確認、OS・削除・機密ファイルは拒否する。未保存のときの初期値。
    Standard,
    /// 読み取り系のコマンドだけを許可する(Web は使わない)。書き出し系のオプションは確認、OS・削除・機密ファイルは拒否する。
    ReadOnly,
}

/// 画面に出すプリセットの情報。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PresetInfo {
    pub id: Preset,
    pub label: &'static str,
    pub description: &'static str,
    pub text: String,
}

impl Preset {
    pub const ALL: [Self; 3] = [Self::Standard, Self::ReadOnly, Self::Permissive];

    fn label(self) -> &'static str {
        match self {
            Self::Permissive => "すべて許可",
            Self::Standard => "標準",
            Self::ReadOnly => "読み取りのみ",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Permissive => "すべてのコマンドと Web 検索・ページ取得を、承認なしで実行します。拒否のルールは持ちません(アプリのデータの保護だけは効きます)。",
            Self::Standard => "読み取り系のコマンドと Web 検索・ページ取得は承認なしで実行し、破壊的な操作は毎回確認、rm・sudo・OS の設定・機密ファイルは拒否します。",
            Self::ReadOnly => "読み取り系のコマンドだけを承認なしで実行します。Web 検索・ページ取得は使いません。rm・sudo・OS の設定・機密ファイルは拒否します。",
        }
    }

    /// プリセットの settings.json。
    pub fn text(self) -> String {
        let read_only = || bash_prefix_rules(DEFAULT_ALLOW);
        let web = ["WebSearch".to_owned(), "WebFetch".to_owned()];
        let option_asks = || DEFAULT_ASK_PATTERNS.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let safe_deny = || {
            let mut deny = default_deny_rules();
            deny.push("Bash(find *-delete*)".to_owned());
            deny.extend(DEFAULT_PROTECTED.iter().map(|p| format!("Bash(*{p}*)")));
            deny
        };
        let (allow, ask, deny): (Vec<String>, Vec<String>, Vec<String>) = match self {
            Self::Permissive => (std::iter::once("Bash".to_owned()).chain(web).collect(), Vec::new(), Vec::new()),
            Self::Standard => {
                let mut ask = bash_prefix_rules(DEFAULT_ASK);
                ask.extend(option_asks());
                (read_only().into_iter().chain(web).collect(), ask, safe_deny())
            }
            Self::ReadOnly => (read_only(), option_asks(), safe_deny()),
        };
        let v = json!({ "permissions": { "allow": allow, "ask": ask, "deny": deny } });
        serde_json::to_string_pretty(&v).unwrap_or_default()
    }

    fn info(self) -> PresetInfo {
        PresetInfo { id: self, label: self.label(), description: self.description(), text: self.text() }
    }
}

/// 選べるプリセットの一覧(先頭が既定)。
pub fn presets() -> Vec<PresetInfo> {
    Preset::ALL.iter().map(|p| p.info()).collect()
}

/// settings.json の`permissions`のルール。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rules {
    pub allow: Vec<String>,
    pub ask: Vec<String>,
    pub deny: Vec<String>,
}

/// JSON から`permissions`のルールを取り出す。形が違えば理由を返す(内容は含めない)。
fn rules_of(v: &Value) -> Result<Rules, String> {
    let Value::Object(root) = v else { return Err("settings.json は JSON のオブジェクト({ ... })で書いてください。".into()) };
    let perms = match root.get("permissions") {
        None => return Ok(Rules::default()),
        Some(Value::Object(p)) => p,
        Some(_) => return Err("permissions はオブジェクトで書いてください。".into()),
    };
    let list = |name: &str| -> Result<Vec<String>, String> {
        let Some(value) = perms.get(name) else { return Ok(Vec::new()) };
        let bad = || format!("permissions.{name} は文字列の配列にしてください。");
        let items = value.as_array().ok_or_else(bad)?;
        if items.len() > MAX_RULES {
            return Err(format!("permissions.{name} は{MAX_RULES}件までです。"));
        }
        let mut out = Vec::new();
        for item in items {
            let rule = item.as_str().ok_or_else(bad)?.trim();
            if rule.chars().count() > MAX_RULE_CHARS {
                return Err(format!("permissions.{name} の1項目は{MAX_RULE_CHARS}文字までです。"));
            }
            if !rule.is_empty() {
                out.push(rule.to_owned());
            }
        }
        Ok(out)
    };
    Ok(Rules { allow: list("allow")?, ask: list("ask")?, deny: list("deny")? })
}

/// 保存する settings.json を検証する。空は受け付けない(初期値へ戻すときはプリセットを選ぶ)。
pub fn validate(text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("settings.json が空です。初期値へ戻すときは「初期値に戻す」でプリセットを選んでください。".into());
    }
    sandbox_agent::validate_settings(text)?;
    let v: Value = serde_json::from_str(text.trim()).map_err(|e| format!("settings.json を JSON として読めません({}行{}列目)。", e.line(), e.column()))?;
    rules_of(&v).map(|_| ())
}

/// 保存した settings.json。未保存なら標準のプリセットを書き出して返す(書けなくても、その内容を返す)。
pub fn load_settings(store: &(impl SettingsStore + ?Sized)) -> String {
    match store.get_setting(KEY) {
        Ok(Some(text)) if !text.trim().is_empty() => text,
        Ok(_) => {
            let text = Preset::Standard.text();
            if let Err(e) = store.set_setting(KEY, &text) {
                log::warn!("api settings write failed: {e}");
            }
            text
        }
        Err(e) => {
            log::warn!("api settings not readable: {e}");
            Preset::Standard.text()
        }
    }
}

/// 検証して保存する。
pub fn save(store: &(impl SettingsStore + ?Sized), text: &str) -> Result<(), String> {
    validate(text)?;
    store.set_setting(KEY, text.trim()).map_err(|e| e.to_string())
}

/// 選んだプリセットを保存し(いまの内容は捨てる)、保存した内容を返す。
pub fn reset(store: &(impl SettingsStore + ?Sized), preset: Preset) -> Result<String, String> {
    let text = preset.text();
    save(store, &text)?;
    Ok(text)
}

/// 判定に使うルール。保存した内容が壊れていれば、許可・確認は空で、拒否は既定の拒否だけにする(すべて承認が要る)。
pub fn load(store: &(impl SettingsStore + ?Sized)) -> Rules {
    let text = load_settings(store);
    serde_json::from_str::<Value>(&text).map_err(|e| e.to_string()).and_then(|v| rules_of(&v)).unwrap_or_else(|e| {
        log::warn!("api settings are invalid: {e}");
        Rules { deny: default_deny_rules(), ..Rules::default() }
    })
}

/// `permissions.allow`へ`Bash(<パターン>:*)`を加えて保存する(ユーザーが承認したときだけ呼ぶ)。
pub fn add_allow(store: &(impl SettingsStore + ?Sized), patterns: &[String]) -> Result<(), String> {
    let text = load_settings(store);
    let mut v: Value = serde_json::from_str(&text).map_err(|_| "settings.json を JSON として読めません。".to_owned())?;
    let root = v.as_object_mut().ok_or("settings.json が JSON のオブジェクトではありません。")?;
    let perms = root.entry("permissions").or_insert_with(|| Value::Object(Map::new()));
    let perms = perms.as_object_mut().ok_or("permissions がオブジェクトではありません。")?;
    let allow = perms.entry("allow").or_insert_with(|| Value::Array(Vec::new()));
    let allow = allow.as_array_mut().ok_or("permissions.allow が配列ではありません。")?;
    for rule in bash_prefix_rules(patterns.iter().map(String::as_str)) {
        if !allow.iter().any(|r| r.as_str() == Some(rule.as_str())) {
            allow.push(Value::String(rule));
        }
    }
    let text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    save(store, &text)
}

/// ルールを`(ツール名, 指定子)`に分ける。`Bash(git status:*)`は`("Bash", Some("git status:*"))`、`WebSearch`は`("WebSearch", None)`。
fn split_rule(rule: &str) -> (&str, Option<&str>) {
    let rule = rule.trim();
    match rule.find('(') {
        Some(open) if rule.ends_with(')') => (rule[..open].trim(), Some(&rule[open + 1..rule.len() - 1])),
        _ => (rule, None),
    }
}

/// Web 検索・ページ取得(サーバーツール)を渡すか。1回ごとの確認ができないため、allow に指定子なしで書かれ、
/// ask・deny のどちらにも(指定子つきを含め)書かれていないときだけ渡す。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WebAccess {
    pub search: bool,
    pub fetch: bool,
}

pub fn web_access(rules: &Rules) -> WebAccess {
    let allowed = |tool: &str| {
        let named = |list: &[String]| list.iter().any(|r| split_rule(r).0 == tool);
        !named(&rules.deny) && !named(&rules.ask) && rules.allow.iter().any(|r| split_rule(r) == (tool, None))
    };
    WebAccess { search: allowed("WebSearch"), fetch: allowed("WebFetch") }
}

/// 空白を1つにそろえる。
fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `*`を任意の文字列(空・空白を含む)として照合する。
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// `Bash`のルールの指定子が、コマンド(空白をそろえた文字列)に一致するか。Claude Code と同じ書き方を受け付ける。
/// - 指定子なし・`*`: すべて
/// - `git log:*`: `git log`そのもの、または`git log `で始まるもの
/// - `npm run *`: `*`は任意の文字列。末尾の` *`は引数なしにも一致する
/// - それ以外: 完全一致
fn bash_rule_matches(spec: Option<&str>, text: &str) -> bool {
    let Some(spec) = spec.map(str::trim).filter(|s| !s.is_empty() && *s != "*") else { return true };
    if let Some(prefix) = spec.strip_suffix(":*") {
        let prefix = squash(prefix);
        return glob_match(&prefix, text) || glob_match(&format!("{prefix} *"), text);
    }
    let spec = squash(spec);
    glob_match(&spec, text) || spec.strip_suffix(" *").is_some_and(|base| glob_match(base, text))
}

/// `list`のうち、コマンドのどれかに一致する最初の`Bash`のルール。
fn find_bash_rule<'a>(list: &'a [String], texts: &[&str]) -> Option<&'a String> {
    list.iter().find(|r| {
        let (tool, spec) = split_rule(r);
        tool == "Bash" && texts.iter().any(|t| bash_rule_matches(spec, t))
    })
}

/// すべてのコマンドに一致する`Bash`の許可(`Bash`、`Bash(*)`)があるか。
fn allows_any_bash(list: &[String]) -> bool {
    list.iter().any(|r| {
        let (tool, spec) = split_rule(r);
        tool == "Bash" && spec.map(str::trim).is_none_or(|s| s.is_empty() || s == "*")
    })
}

/// アプリデータを守るための範囲。設定とは別に、常に検査する。
#[derive(Debug, Clone)]
pub struct Scope {
    /// ナレッジのルート(正規化済み)。アプリデータの中でも、ここは扱える。
    root: PathBuf,
    /// コマンドの作業ディレクトリ(正規化済み)。
    cwd: PathBuf,
    /// アプリデータのフォルダ(正規化済み)。この中(ナレッジを除く)は、読み取り系でも触れさせない。
    private: Option<PathBuf>,
}

impl Scope {
    pub fn new(root: &Path, cwd: &Path) -> Self {
        Self { root: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()), cwd: cwd.to_path_buf(), private: None }
    }

    /// アプリデータのフォルダを保護の対象にする(APIキー・DB・ほかのワークスペースのデータ)。
    pub fn with_private(mut self, app_data: Option<&Path>) -> Self {
        self.private = app_data.map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()));
        self
    }

    /// 語が指すパス。`--out=/x`は値の部分、`~`はホーム、相対パスは作業ディレクトリから解決する。
    /// ワイルドカードを含む語は、その手前のフォルダまで。変数を含む語は行き先が分からないので`None`。
    fn word_path(&self, word: &str) -> Option<PathBuf> {
        let w = word.split_once('=').filter(|(k, _)| !k.contains('/')).map_or(word, |(_, v)| v);
        if w.is_empty() || has_variable(w) {
            return None;
        }
        let w = match w.find(['*', '?', '[']) {
            Some(i) => w[..i].rsplit_once('/').map_or("", |(dir, _)| if dir.is_empty() { "/" } else { dir }),
            None => w,
        };
        let path = if let Some(rest) = w.strip_prefix('~') {
            let home = std::env::var_os("HOME").map(PathBuf::from)?;
            match rest.strip_prefix('/') {
                Some(r) => home.join(r),
                None if rest.is_empty() => home,
                None => return None,
            }
        } else if w.starts_with('/') {
            PathBuf::from(w)
        } else {
            self.cwd.join(w)
        };
        Some(Self::resolve(&Self::normalize(&path)))
    }

    /// 解決済みのパスが、アプリデータの中(ナレッジを除く)か。
    fn is_private(&self, path: &Path) -> bool {
        self.private.as_ref().is_some_and(|app| path.starts_with(app) && !path.starts_with(&self.root))
    }

    fn touches_private(&self, word: &str) -> bool {
        self.private.is_some() && self.word_path(word).is_some_and(|p| self.is_private(&p))
    }

    fn cwd_private(&self) -> bool {
        self.is_private(&Self::resolve(&Self::normalize(&self.cwd)))
    }

    /// 解決済みのパスが、アプリデータを含む(祖先か、同じ場所)か。
    fn encloses_private(&self, path: &Path) -> bool {
        self.private.as_ref().is_some_and(|app| app.starts_with(path))
    }

    /// `.`と`..`を字面で畳む。
    fn normalize(path: &Path) -> PathBuf {
        let mut out = PathBuf::new();
        for c in path.components() {
            match c {
                Component::ParentDir => {
                    out.pop();
                }
                Component::CurDir => {}
                c => out.push(c),
            }
        }
        out
    }

    /// 存在する最も深い祖先を正規化(シンボリックリンクを解決)して、残りを付け直す。
    fn resolve(path: &Path) -> PathBuf {
        let mut rest = Vec::new();
        let mut base = path.to_path_buf();
        loop {
            if let Ok(c) = base.canonicalize() {
                return rest.iter().rev().fold(c, |acc, r| acc.join(r));
            }
            let Some(name) = base.file_name().map(ToOwned::to_owned) else { return path.to_path_buf() };
            rest.push(name);
            if !base.pop() {
                return path.to_path_buf();
            }
        }
    }
}

/// 判定結果。数が大きいほど厳しい。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// `permissions.allow`に一致。承認なしで実行する。
    Allow,
    /// どのルールにも一致しない。承認されれば、パターンを`permissions.allow`へ加えて(または今回だけ)実行する。
    Unlisted(Vec<String>),
    /// 毎回承認を求める。理由つき(`permissions.ask`に一致、または内容を確かめられない形)。
    Ask(String),
    /// 実行しない。理由つき。
    Deny(String),
}

impl Verdict {
    fn rank(&self) -> u8 {
        match self {
            Self::Allow => 0,
            Self::Unlisted(_) => 1,
            Self::Ask(_) => 2,
            Self::Deny(_) => 3,
        }
    }

    /// より厳しい方を返す。どちらも`Unlisted`なら、パターンをまとめる。
    fn max(self, other: Self) -> Self {
        if let (Self::Unlisted(mut a), Self::Unlisted(b)) = (self.clone(), &other) {
            for p in b {
                if !a.contains(p) {
                    a.push(p.clone());
                }
            }
            return Self::Unlisted(a);
        }
        if other.rank() > self.rank() { other } else { self }
    }
}

fn deny(reason: &str) -> Verdict {
    Verdict::Deny(reason.to_owned())
}

fn ask(reason: &str) -> Verdict {
    Verdict::Ask(reason.to_owned())
}

/// 構文を分解した結果。
#[derive(Debug, Default)]
struct Parsed {
    segments: Vec<Segment>,
    /// `$(...)`とバッククォートの中身。別のコマンドとして判定する。
    subs: Vec<String>,
}

#[derive(Debug, Default)]
struct Segment {
    words: Vec<String>,
}

/// クォートと`\`を解釈してコマンドを区切り単位(`;` `&` `|` 改行 括弧)と語に分ける。
/// 完全なシェル構文解析ではないので、解釈できないものは承認が要る側に倒れるようにしている。
fn parse(cmd: &str) -> Parsed {
    let chars: Vec<char> = cmd.chars().collect();
    let mut out = Parsed::default();
    let mut seg = Segment::default();
    let mut word = String::new();
    let mut in_word = false;
    let mut i = 0;

    macro_rules! end_word {
        () => {
            if in_word {
                seg.words.push(std::mem::take(&mut word));
                in_word = false;
            }
        };
    }
    macro_rules! end_segment {
        () => {
            end_word!();
            if !seg.words.is_empty() {
                out.segments.push(std::mem::take(&mut seg));
            }
        };
    }

    // `(`の次から対応する`)`までの中身を取り出し、`)`の次の位置を返す
    let take_paren = |start: usize| -> (String, usize) {
        let mut depth = 1;
        let mut j = start;
        while j < chars.len() {
            match chars[j] {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return (chars[start..j].iter().collect(), j + 1);
                    }
                }
                _ => {}
            }
            j += 1;
        }
        (chars[start..].iter().collect(), chars.len())
    };
    let take_backtick = |start: usize| -> (String, usize) {
        match chars[start..].iter().position(|&c| c == '`') {
            Some(n) => (chars[start..start + n].iter().collect(), start + n + 1),
            None => (chars[start..].iter().collect(), chars.len()),
        }
    };

    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' => {
                in_word = true;
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    word.push(chars[i]);
                    i += 1;
                }
                i += 1;
            }
            '"' => {
                in_word = true;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    match chars[i] {
                        '\\' if i + 1 < chars.len() => {
                            word.push(chars[i + 1]);
                            i += 2;
                        }
                        '$' if chars.get(i + 1) == Some(&'(') => {
                            let (inner, next) = take_paren(i + 2);
                            out.subs.push(inner);
                            i = next;
                        }
                        '`' => {
                            let (inner, next) = take_backtick(i + 1);
                            out.subs.push(inner);
                            i = next;
                        }
                        ch => {
                            word.push(ch);
                            i += 1;
                        }
                    }
                }
                i += 1;
            }
            '\\' => {
                in_word = true;
                if let Some(&n) = chars.get(i + 1)
                    && n != '\n'
                {
                    word.push(n);
                }
                i += 2;
            }
            '$' if chars.get(i + 1) == Some(&'(') => {
                in_word = true;
                let (inner, next) = take_paren(i + 2);
                out.subs.push(inner);
                i = next;
            }
            '$' if chars.get(i + 1) == Some(&'{') => {
                in_word = true;
                while i < chars.len() && chars[i] != '}' {
                    word.push(chars[i]);
                    i += 1;
                }
                if i < chars.len() {
                    word.push('}');
                    i += 1;
                }
            }
            '`' => {
                in_word = true;
                let (inner, next) = take_backtick(i + 1);
                out.subs.push(inner);
                i = next;
            }
            '#' if !in_word => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            ' ' | '\t' => {
                end_word!();
                i += 1;
            }
            '\n' | ';' | '(' | ')' | '{' | '}' => {
                end_segment!();
                i += 1;
            }
            // `2>&1`のfdの複製は区切りではない
            '&' if i > 0 && chars[i - 1] == '>' => {
                in_word = true;
                word.push('&');
                i += 1;
            }
            '&' => {
                // `&&`、`&`、`&>`のいずれも区切りとして扱う(`&>`の`>`は次の語の先頭に残る)
                end_segment!();
                i += if chars.get(i + 1) == Some(&'&') { 2 } else { 1 };
            }
            '|' => {
                let double = chars.get(i + 1) == Some(&'|');
                end_segment!();
                i += if double || chars.get(i + 1) == Some(&'&') { 2 } else { 1 };
            }
            ch => {
                in_word = true;
                word.push(ch);
                i += 1;
            }
        }
    }
    if in_word {
        seg.words.push(word);
    }
    if !seg.words.is_empty() {
        out.segments.push(seg);
    }
    out
}

fn basename(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

fn is_assignment(w: &str) -> bool {
    w.split_once('=').is_some_and(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_alphanumeric() || c == '_'))
}

/// 変数の参照(`$VAR`、`${VAR}`)を含むか。正規表現の行末の`$`は含めない。
fn has_variable(word: &str) -> bool {
    let b = word.as_bytes();
    b.iter().enumerate().any(|(i, c)| *c == b'$' && b.get(i + 1).is_some_and(|n| n.is_ascii_alphanumeric() || matches!(n, b'_' | b'{')))
}

/// 環境変数の指定とラッパー(とそのオプション)を飛ばして、実際のコマンドの位置を返す。
fn program_index(words: &[String]) -> Option<usize> {
    let mut i = 0;
    let mut after_wrapper = false;
    while i < words.len() {
        let w = words[i].as_str();
        if is_assignment(w) || (after_wrapper && (w.starts_with('-') || w == "{}")) {
            i += 1;
        } else if WRAPPERS.contains(&basename(w)) {
            after_wrapper = true;
            i += 1;
        } else {
            return Some(i);
        }
    }
    None
}

/// 語がリダイレクト(`>`、`>>`、`2>`など)なら、語に続けて書かれた書き込み先(空なら次の語)を返す。
/// `2>&1`のようなfdの複製はリダイレクトとして扱わない。
fn redirect_of(word: &str) -> Option<&str> {
    let rest = word.trim_start_matches(|c: char| c.is_ascii_digit());
    if let Some(target) = rest.strip_prefix(">>") {
        Some(target)
    } else if let Some(target) = rest.strip_prefix('>') {
        if target.starts_with('&') { None } else { Some(target.trim_start_matches('|')) }
    } else {
        None
    }
}

/// 許可へ加える候補のパターン。シェル・インタプリタ・ラッパーなど、何でも実行できるものは候補にしない。
fn allow_pattern(words: &[String], prog: usize) -> Option<String> {
    // パスで指したコマンド(`./tool`)は、許可のルール(`Bash(tool:*)`)に一致しないので候補にしない
    if words[prog].contains('/') {
        return None;
    }
    let name = words[prog].as_str();
    if SHELLS.contains(&name) || INTERPRETERS.contains(&name) || WRAPPERS.contains(&name) || name == "eval" {
        return None;
    }
    if !SUBCOMMAND_TOOLS.contains(&name) {
        return Some(name.to_owned());
    }
    let sub = words.get(prog + 1)?;
    (!sub.starts_with('-') && sub.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then(|| format!("{name} {sub}"))
}

/// 配下のファイルの中身を再帰的に読むコマンドか(`rg`、`grep -r`、`find -exec`)。
fn reads_recursively(prog: &str, args: &[String]) -> bool {
    match prog {
        "rg" => true,
        "grep" | "egrep" | "fgrep" => args.iter().any(|a| {
            matches!(a.as_str(), "--recursive" | "--dereference-recursive")
                || (a.starts_with('-') && !a.starts_with("--") && a.contains(['r', 'R']))
        }),
        "find" => args.iter().any(|a| matches!(a.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir")),
        _ => false,
    }
}

/// 再帰的に読むコマンドの起点(解決済み)。パスの指定が無さそうなら、作業ディレクトリも起点とみなす。
fn search_roots(scope: &Scope, prog: &str, args: &[String]) -> Vec<PathBuf> {
    let plain: Vec<&String> = args.iter().take_while(|a| !matches!(a.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir")).filter(|a| !a.starts_with('-')).collect();
    let mut roots: Vec<PathBuf> = plain.iter().filter_map(|a| scope.word_path(a)).collect();
    // `grep`・`rg`は最初の語が検索語なので、語が2つ以上あるときだけパスの指定がある
    let needed = if prog == "find" { 1 } else { 2 };
    if plain.len() < needed {
        roots.push(Scope::resolve(&Scope::normalize(&scope.cwd)));
    }
    roots
}

/// アプリデータを守る検査(設定では外せない)。当たれば拒否の理由を返す。
fn private_violation(scope: &Scope, words: &[String], targets: &[String]) -> Option<&'static str> {
    if scope.cwd_private() {
        return Some(PRIVATE_REASON);
    }
    for w in words.iter().chain(targets) {
        if scope.touches_private(w) {
            return Some(PRIVATE_REASON);
        }
        // グロブでアプリのデータ置き場(Library配下)を探られるのを防ぐ
        if w.contains("Library/") && w.contains(['*', '?', '[']) {
            return Some("Library配下へのワイルドカード指定は実行できません。具体的なパスを指定してください。");
        }
    }
    let p = program_index(words)?;
    let (prog, args) = (basename(&words[p]), &words[p + 1..]);
    (reads_recursively(prog, args) && search_roots(scope, prog, args).iter().any(|r| scope.encloses_private(r))).then_some(PRIVATE_SEARCH_REASON)
}

fn classify_segment(seg: &Segment, rules: &Rules, scope: Option<&Scope>, depth: usize) -> Verdict {
    // リダイレクトを取り除いて、書き込み先を分ける
    let mut words: Vec<String> = Vec::new();
    let mut targets: Vec<String> = Vec::new();
    let mut i = 0;
    while i < seg.words.len() {
        let w = &seg.words[i];
        if let Some(inline) = redirect_of(w) {
            let target = if inline.is_empty() {
                i += 1;
                seg.words.get(i).cloned().unwrap_or_default()
            } else {
                inline.to_owned()
            };
            if target != "/dev/null" {
                targets.push(target);
            }
        } else {
            words.push(w.clone());
        }
        i += 1;
    }
    if let Some(reason) = scope.and_then(|s| private_violation(s, &words, &targets)) {
        return deny(reason);
    }

    let raw = words.join(" ");
    let Some(p) = program_index(&words) else {
        // 変数への代入だけの区切りは、後のコマンドの引数を実行時に変えうる
        return if words.is_empty() { Verdict::Allow } else { ask("変数への代入を含むため、確認が必要です。") };
    };
    // 拒否・確認は、ラッパーを外した形と、コマンド名をパスなしにした形(`/bin/rm`)にも効かせる
    let unwrapped = words[p..].join(" ");
    let bare = std::iter::once(basename(&words[p])).chain(words[p + 1..].iter().map(String::as_str)).collect::<Vec<_>>().join(" ");
    let texts = [raw.as_str(), unwrapped.as_str(), bare.as_str()];
    if find_bash_rule(&rules.deny, &texts).is_some() {
        return deny("settings.json の permissions.deny に一致するため実行できません。必要ならユーザー自身に行ってもらってください。");
    }

    // 中身を別のコマンドとして判定する形(拒否は中身にも効かせる)
    let prog = basename(&words[p]);
    let args = &words[p + 1..];
    let inner = if SHELLS.contains(&prog)
        && let Some(n) = args.iter().position(|a| a.starts_with('-') && a.contains('c'))
        && let Some(script) = args.get(n + 1)
    {
        Some(classify_depth(script, rules, scope, depth + 1))
    } else if prog == "eval" {
        Some(classify_depth(&args.join(" "), rules, scope, depth + 1))
    } else if prog == "find"
        && let Some(n) = args.iter().position(|a| matches!(a.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir"))
    {
        let words: Vec<String> = args[n + 1..].iter().take_while(|a| *a != ";" && *a != "+").cloned().collect();
        Some(classify_segment(&Segment { words }, rules, scope, depth + 1))
    } else {
        None
    };
    if let Some(v @ Verdict::Deny(_)) = &inner {
        return v.clone();
    }

    if let Some(rule) = find_bash_rule(&rules.ask, &texts) {
        return Verdict::Ask(format!("settings.json の permissions.ask の「{rule}」に一致するため、確認が必要です。"));
    }
    if allows_any_bash(&rules.allow) {
        return inner.unwrap_or(Verdict::Allow);
    }
    // 許可のルールは、コマンドの字面どおりに実行されるものにだけ効かせる
    let unclear = if inner.is_some() {
        Some("中で別のコマンドを実行するため、確認が必要です。")
    } else if p > 0 {
        Some("環境変数の前置きや xargs などを含むため、確認が必要です。")
    } else if args.iter().any(|a| has_variable(a)) {
        Some("実行時に決まる引数(変数)を含むため、確認が必要です。")
    } else if !targets.is_empty() {
        Some("ファイルへ書き込むリダイレクトを含むため、確認が必要です。")
    } else {
        None
    };
    if let Some(reason) = unclear {
        return ask(reason);
    }
    if find_bash_rule(&rules.allow, &[raw.as_str()]).is_some() {
        return Verdict::Allow;
    }
    match allow_pattern(&words, p) {
        Some(pattern) => Verdict::Unlisted(vec![pattern]),
        None => ask("settings.json の permissions.allow に一致しないコマンドです。"),
    }
}

fn classify_depth(cmd: &str, rules: &Rules, scope: Option<&Scope>, depth: usize) -> Verdict {
    if depth > MAX_DEPTH {
        return ask("コマンドの入れ子が深く、内容を確認できません。");
    }
    let parsed = parse(cmd);
    let mut verdict = Verdict::Allow;
    for seg in &parsed.segments {
        verdict = verdict.max(classify_segment(seg, rules, scope, depth));
    }
    for sub in &parsed.subs {
        verdict = verdict.max(classify_depth(sub, rules, scope, depth + 1));
    }
    // コマンド置換の結果は引数になり、行き先を確認できない
    if !parsed.subs.is_empty() {
        verdict = verdict.max(ask("コマンド置換を含むため、確認が必要です。"));
    }
    verdict
}

/// コマンドの可否。`&&`・`;`・パイプ・`$(...)`などで分かれる場合は、それぞれを判定して最も厳しいものを採る。
/// `scope`を渡すと、アプリデータの場所を指すコマンドを拒否する。
pub fn classify(cmd: &str, rules: &Rules, scope: Option<&Scope>) -> Verdict {
    if cmd.trim().is_empty() {
        return deny("コマンドが空です。");
    }
    classify_depth(cmd, rules, scope, 0)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn store() -> crate::db::Db {
        crate::db::Db::open_in_memory(crate::db::Role::Agent)
    }

    fn preset(p: Preset) -> Rules {
        rules_of(&serde_json::from_str(&p.text()).unwrap()).unwrap()
    }

    fn rules() -> Rules {
        preset(Preset::Standard)
    }

    fn v(cmd: &str) -> Verdict {
        classify(cmd, &rules(), None)
    }

    fn with(allow: &[&str], ask: &[&str], deny: &[&str]) -> Rules {
        let owned = |l: &[&str]| l.iter().map(|s| (*s).to_owned()).collect();
        Rules { allow: owned(allow), ask: owned(ask), deny: owned(deny) }
    }

    #[test]
    fn rule_syntax_follows_claude_code() {
        assert!(bash_rule_matches(None, "anything"));
        assert!(bash_rule_matches(Some("*"), "anything"));
        assert!(bash_rule_matches(Some("git log:*"), "git log"));
        assert!(bash_rule_matches(Some("git log:*"), "git log --oneline"));
        assert!(!bash_rule_matches(Some("git log:*"), "git logx"));
        assert!(!bash_rule_matches(Some("ls:*"), "lsof"));
        assert!(bash_rule_matches(Some("npm run *"), "npm run build"));
        assert!(bash_rule_matches(Some("npm run *"), "npm run"));
        assert!(bash_rule_matches(Some("git * main"), "git push origin main"));
        assert!(bash_rule_matches(Some("make"), "make"));
        assert!(!bash_rule_matches(Some("make"), "make install"));
        assert_eq!(split_rule("Bash(git status:*)"), ("Bash", Some("git status:*")));
        assert_eq!(split_rule("WebSearch"), ("WebSearch", None));
    }

    #[test]
    fn template_allows_reads_and_asks_or_denies_the_rest() {
        for c in ["ls -la ~/work", "git status", "git log --oneline -n 5", "ls | grep foo | wc -l", "cat a.txt 2>&1", "echo hi > /dev/null", "git diff && git status"] {
            assert_eq!(v(c), Verdict::Allow, "{c}");
        }
        for c in ["git push origin main", "git reset --hard HEAD~1", "mv a b", "kill 1", "find . -exec cat {} +", "git log --output=x", "rg --pre ./t x"] {
            assert!(matches!(v(c), Verdict::Ask(_)), "{c}: {:?}", v(c));
        }
        for c in ["rm -rf /", "sudo ls", "shutdown -h now", "defaults write a b", "find . -name x -delete", "cat ~/.aws/credentials", "cat /x/secrets.json"] {
            assert!(matches!(v(c), Verdict::Deny(_)), "{c}: {:?}", v(c));
        }
        assert_eq!(v("npm install"), Verdict::Unlisted(vec!["npm install".into()]));
        assert_eq!(v("mkdir -p a"), Verdict::Unlisted(vec!["mkdir".into()]));
    }

    #[test]
    fn deny_reaches_through_wrappers_and_nesting() {
        for c in ["ls && rm x", "ls | xargs rm", "env FOO=1 rm x", "sh -c 'rm x'", "bash -lc \"ls; rm x\"", "eval rm x", "echo $(rm x)", "echo `rm x`", "find . -exec rm {} \\;", "(rm x)", "/bin/rm x", "command rm x"] {
            assert!(matches!(v(c), Verdict::Deny(_)), "{c}: {:?}", v(c));
        }
    }

    #[test]
    fn allow_applies_only_to_literal_commands() {
        let r = with(&["Bash(cat:*)", "Bash(ls:*)", "Bash(echo:*)", "Bash(sh:*)"], &[], &[]);
        for c in ["FOO=1 cat x", "cat $HOME/x", "echo $(ls)", "ls | xargs cat", "echo hi > out.txt", "sh -c 'ls'", "a=1"] {
            assert!(matches!(classify(c, &r, None), Verdict::Ask(_)), "{c}: {:?}", classify(c, &r, None));
        }
        assert_eq!(classify("cat a && ls", &r, None), Verdict::Allow);
        // `Bash`だけの許可はすべてを通す(拒否・確認は残る)
        let all = with(&["Bash"], &["Bash(git push:*)"], &["Bash(rm:*)"]);
        assert_eq!(classify("echo hi > out.txt", &all, None), Verdict::Allow);
        assert!(matches!(classify("sh -c 'git push'", &all, None), Verdict::Ask(_)), "中身の確認は残る");
        assert!(matches!(classify("git push", &all, None), Verdict::Ask(_)));
        assert!(matches!(classify("rm x", &all, None), Verdict::Deny(_)));
    }

    #[test]
    fn unlisted_segments_are_combined_and_ask_wins() {
        let r = with(&["Bash(ls:*)"], &["Bash(git push:*)"], &[]);
        assert_eq!(classify("mkdir a && touch b && ls", &r, None), Verdict::Unlisted(vec!["mkdir".into(), "touch".into()]));
        assert!(matches!(classify("mkdir a && git push", &r, None), Verdict::Ask(_)));
        // 何でも実行できるものは、許可へ加える候補にしない
        assert!(matches!(classify("python3 x.py", &r, None), Verdict::Ask(_)));
        assert!(matches!(classify("   ", &r, None), Verdict::Deny(_)));
    }

    #[test]
    fn web_tools_need_a_plain_allow_and_no_ask_or_deny() {
        let w = |r: &Rules| web_access(r);
        assert_eq!(w(&rules()), WebAccess { search: true, fetch: true });
        assert_eq!(w(&Rules::default()), WebAccess::default(), "未一致は渡さない");
        assert_eq!(w(&with(&["WebSearch"], &[], &[])), WebAccess { search: true, fetch: false });
        assert_eq!(w(&with(&["WebSearch", "WebFetch"], &["WebSearch"], &["WebFetch(domain:x.com)"])), WebAccess::default());
        assert_eq!(w(&with(&["WebFetch(domain:x.com)"], &[], &[])), WebAccess::default(), "指定子つきの許可は対象外");
    }

    #[test]
    fn app_data_is_out_of_reach_even_when_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        let root = app.join("knowledge");
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::create_dir_all(app.join("work/abc")).unwrap();
        fs::write(app.join("secrets.json"), "{}").unwrap();
        let (app, root) = (app.canonicalize().unwrap(), root.canonicalize().unwrap());
        let home = dir.path().canonicalize().unwrap();
        let all = with(&["Bash"], &[], &[]);
        let at = |cwd: &Path| Scope::new(&root, cwd).with_private(Some(&app));
        let denied = |cmd: &str, cwd: &Path| matches!(classify(cmd, &all, Some(&at(cwd))), Verdict::Deny(_));
        assert!(denied("cat secrets.jso?", &app));
        assert!(denied("ls", &app.join("work/abc")));
        assert!(denied(&format!("ls {}", app.display()), &home));
        assert!(denied("cat ../secrets.jso?", &root));
        assert!(denied(&format!("echo x > {}/a", app.display()), &home));
        assert!(denied("rg sk-ant", &home));
        assert!(denied("grep -r sk-ant .", &home));
        assert!(denied("find . -name 'sec*' -exec cat {} +", &home));
        assert_eq!(classify("ls sub", &all, Some(&at(&root))), Verdict::Allow);
        assert_eq!(classify("rg foo", &all, Some(&at(&root))), Verdict::Allow);
        assert_eq!(classify("ls", &all, Some(&at(&home))), Verdict::Allow);
    }

    #[cfg(unix)]
    #[test]
    fn app_data_is_not_reachable_through_a_symlink_in_the_knowledge() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        let root = app.join("knowledge");
        fs::create_dir_all(&root).unwrap();
        let (app, root) = (app.canonicalize().unwrap(), root.canonicalize().unwrap());
        std::os::unix::fs::symlink(&app, root.join("up")).unwrap();
        let scope = Scope::new(&root, &root).with_private(Some(&app));
        assert!(matches!(classify("cat up/secrets.jso?", &with(&["Bash"], &[], &[]), Some(&scope)), Verdict::Deny(_)));
    }

    #[test]
    fn standard_preset_is_written_on_first_read_and_saves_are_validated() {
        let db = store();
        assert_eq!(db.get_setting(KEY).unwrap(), None);
        assert_eq!(load_settings(&db), Preset::Standard.text());
        assert_eq!(db.get_setting(KEY).unwrap(), Some(Preset::Standard.text()), "初回に書き出す");
        assert_eq!(load(&db), rules());
        for bad in [r#"{ "permissions": { "allow": "Bash" } }"#, r#"{ "permissions": [] }"#, r#"{ "permissions": { "deny": [1] } }"#, "[]", "{ not json", "", "  "] {
            assert!(save(&db, bad).is_err(), "{bad}");
        }
        save(&db, r#"{ "model": "x", "permissions": { "allow": ["Bash(make:*)"] } }"#).unwrap();
        assert_eq!(load(&db), with(&["Bash(make:*)"], &[], &[]));
    }

    #[test]
    fn reset_saves_the_chosen_preset() {
        let db = store();
        save(&db, r#"{ "permissions": { "allow": ["Bash(make:*)"] } }"#).unwrap();
        for p in Preset::ALL {
            assert_eq!(reset(&db, p).unwrap(), p.text());
            assert_eq!(load_settings(&db), p.text());
            assert!(validate(&p.text()).is_ok());
        }
        let ids: Vec<_> = presets().iter().map(|p| p.id).collect();
        assert_eq!(ids, [Preset::Standard, Preset::ReadOnly, Preset::Permissive], "先頭が既定");
        assert_eq!(serde_json::to_value(Preset::ReadOnly).unwrap(), json!("read_only"));
    }

    #[test]
    fn presets_differ_in_what_they_allow() {
        let (all, read) = (preset(Preset::Permissive), preset(Preset::ReadOnly));
        // すべて許可: 拒否のルールは無く、アプリデータの保護だけが効く
        assert!(all.ask.is_empty() && all.deny.is_empty());
        for c in ["rm -rf x", "sudo ls", "git push", "npm install", "echo a > b"] {
            assert_eq!(classify(c, &all, None), Verdict::Allow, "{c}");
        }
        assert_eq!(web_access(&all), WebAccess { search: true, fetch: true });
        // 読み取りのみ: 読み取り系だけ通り、Web は使わない
        assert_eq!(classify("git status", &read, None), Verdict::Allow);
        assert_eq!(classify("npm install", &read, None), Verdict::Unlisted(vec!["npm install".into()]));
        assert!(matches!(classify("rm x", &read, None), Verdict::Deny(_)));
        assert!(matches!(classify("git log --output=x", &read, None), Verdict::Ask(_)));
        assert_eq!(web_access(&read), WebAccess::default());
    }

    #[test]
    fn broken_saved_value_falls_back_to_default_denials() {
        let db = store();
        db.set_setting(KEY, "{ not json").unwrap();
        let r = load(&db);
        assert!(r.allow.is_empty() && r.deny == default_deny_rules());
        assert!(matches!(classify("rm x", &r, None), Verdict::Deny(_)));
        assert!(matches!(classify("ls", &r, None), Verdict::Unlisted(_)));
    }

    #[test]
    fn approved_patterns_are_added_to_the_allow_list() {
        let db = store();
        assert_eq!(classify("mkdir a", &load(&db), None), Verdict::Unlisted(vec!["mkdir".into()]));
        add_allow(&db, &["mkdir".to_owned(), "mkdir".to_owned()]).unwrap();
        let r = load(&db);
        assert_eq!(r.allow.iter().filter(|a| *a == "Bash(mkdir:*)").count(), 1);
        assert_eq!(classify("mkdir a", &r, None), Verdict::Allow);
        // 拒否は許可に加えても緩まない
        add_allow(&db, &["rm".to_owned()]).unwrap();
        assert!(matches!(classify("rm x", &load(&db), None), Verdict::Deny(_)));
        // permissions が無い設定にも足せる。ほかの項目は残る
        save(&db, r#"{ "model": "m" }"#).unwrap();
        add_allow(&db, &["touch".to_owned()]).unwrap();
        let saved: Value = serde_json::from_str(&load_settings(&db)).unwrap();
        assert_eq!(saved["model"], "m");
        assert_eq!(saved["permissions"]["allow"], json!(["Bash(touch:*)"]));
    }

    #[test]
    fn default_denials_are_prefix_rules() {
        let rules = default_deny_rules();
        assert!(rules.contains(&"Bash(rm:*)".to_owned()) && rules.contains(&"Bash(sudo:*)".to_owned()));
        assert!(rules.contains(&"Bash(defaults write:*)".to_owned()));
        assert_eq!(bash_prefix_rules(["git push --force", "bad,rule", "x(y)", "git push --force", " "]), vec!["Bash(git push --force:*)".to_owned()]);
        assert!(validate(&Preset::Standard.text()).is_ok());
    }

    #[test]
    fn deep_nesting_asks() {
        let deep = "sh -c \"sh -c \\\"sh -c \\\\\\\"sh -c 'sh -c ls' \\\\\\\" \\\" \"";
        assert!(!matches!(v(deep), Verdict::Allow));
    }
}
