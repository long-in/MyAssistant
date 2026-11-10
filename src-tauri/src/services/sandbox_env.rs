//! エージェントごとの環境変数ファイル`<ワークスペース>/sandbox.env`(dotenv形式)の読み込み。
//! サンドボックス(Dockerコンテナ)でコマンドを実行するたびに読み直し、その実行のプロセスにだけ渡す。
//! 値はRustの内部だけで扱い、画面へは名前と検証結果だけを返す。エラーの文面にも値を含めない。
use std::{collections::BTreeMap, fs, path::{Path, PathBuf}};

use serde::Serialize;

/// 環境変数ファイルの名前。コンテナにはマウントしない(ワークスペース直下に置く)。
pub const FILE_NAME: &str = "sandbox.env";
/// 定義できる個数の上限。
const MAX_VARS: usize = 100;
/// 値の最大バイト数。
const MAX_VALUE_BYTES: usize = 8 * 1024;
/// 名前の最大バイト数。
const MAX_NAME_BYTES: usize = 128;
/// ファイルの最大バイト数。
const MAX_FILE_BYTES: u64 = 256 * 1024;

/// 実行の仕組みが自分で設定する名前、またはdocker CLIの動作を変えてしまう名前。ファイルでは指定できない。
const RESERVED: [&str; 14] = [
    "PATH", "HOME", "USER", "LOGNAME", "SHELL", "TMPDIR", "PWD", "OLDPWD", "TERM", "NO_COLOR", "PAGER", "GIT_PAGER",
    "GIT_TERMINAL_PROMPT", "HOSTNAME",
];
/// docker CLIが読む(読み込みの挙動を変える)名前と、アプリが内部で使う名前の接頭辞。
/// `MYASSISTANT_`はアプリがコンテナへ渡す内部の値(システムプロンプトなど)に使う。
const RESERVED_PREFIXES: [&str; 4] = ["DOCKER_", "LD_", "DYLD_", "MYASSISTANT_"];

/// 検証のエラー。値は含めず、行番号と理由だけを持つ。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnvError {
    /// 1始まりの行番号。ファイル全体の問題は`None`。
    pub line: Option<usize>,
    pub message: String,
}

impl std::fmt::Display for EnvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(n) => write!(f, "{FILE_NAME}の{n}行目: {}", self.message),
            None => write!(f, "{FILE_NAME}: {}", self.message),
        }
    }
}

fn err(line: usize, message: &str) -> EnvError {
    EnvError { line: Some(line), message: message.to_owned() }
}

pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// 初回に作る説明入りの空ファイル。
const TEMPLATE: &str = "\
# このエージェントのサンドボックス(Dockerコンテナ)へ渡す環境変数です。コンテナ内の Claude Code とコマンドから使えます。
# 1行に1つ、NAME=value の形で書きます。保存すると次の指示から反映されます(再起動は不要)。
# 値は必要なら \"...\" か '...' で囲めます(複数行の値は使えません)。値が空の行は渡しません。
# AWS の認証情報は、このエージェントの AWS の設定から一時キーを発行して渡すので、ここには書かなくてかまいません。
# このファイルは他のエージェントやナレッジからは見えず、コンテナの中にも置かれません。

# Claude Code の API キー(接続先が Anthropic API のとき必要)
ANTHROPIC_API_KEY=
";

/// ファイルが無ければ、説明と既定の名前(値は空)を入れたファイルを、所有者のみ読み書きできる権限で作る。
pub fn ensure_template(dir: &Path) -> Result<PathBuf, String> {
    let file = path(dir);
    if file.exists() {
        return Ok(file);
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    write_private(&file, TEMPLATE)?;
    Ok(file)
}

#[cfg(unix)]
fn write_private(file: &Path, text: &str) -> Result<(), String> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(file)
        .map_err(|e| e.to_string())?;
    f.write_all(text.as_bytes()).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn write_private(file: &Path, text: &str) -> Result<(), String> {
    fs::write(file, text).map_err(|e| e.to_string())
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn reserved(name: &str) -> bool {
    RESERVED.contains(&name) || RESERVED_PREFIXES.iter().any(|p| name.starts_with(p))
}

/// 値の前後の空白を除き、全体が同じ引用符で囲まれていれば外す。
fn unquote(raw: &str) -> Result<String, &'static str> {
    let v = raw.trim();
    for q in ['"', '\''] {
        if let Some(rest) = v.strip_prefix(q) {
            return match rest.strip_suffix(q) {
                Some(inner) if !inner.contains(q) || q == '"' => Ok(inner.replace("\\\"", "\"").replace("\\\\", "\\")),
                _ => Err("引用符が閉じていません"),
            };
        }
    }
    Ok(v.to_owned())
}

/// 本文を解析する。同じ名前は後の行が勝つ。1つでも不正な行があれば失敗する。
pub fn parse(text: &str) -> Result<BTreeMap<String, String>, EnvError> {
    let mut vars = BTreeMap::new();
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").map_or(line, str::trim_start);
        let Some((name, value)) = line.split_once('=') else {
            return Err(err(n, "NAME=value の形式ではありません"));
        };
        let name = name.trim();
        if name.len() > MAX_NAME_BYTES || !valid_name(name) {
            return Err(err(n, "名前は英数字と_だけで、数字から始められません"));
        }
        if reserved(name) {
            return Err(err(n, &format!("{name} は予約されているため指定できません")));
        }
        let value = unquote(value).map_err(|m| err(n, m))?;
        if value.len() > MAX_VALUE_BYTES {
            return Err(err(n, "値が長すぎます"));
        }
        if value.chars().any(char::is_control) {
            return Err(err(n, "値に制御文字は使えません"));
        }
        vars.insert(name.to_owned(), value);
        if vars.len() > MAX_VARS {
            return Err(err(n, &format!("定義できるのは{MAX_VARS}個までです")));
        }
    }
    Ok(vars)
}

/// `dir`のファイルを読んで解析する。無ければ空。
/// ファイルの内容(値を含む)。無ければ既定の内容で作ってから読む。画面での編集に使う。
pub fn read(dir: &Path) -> Result<String, String> {
    let file = ensure_template(dir)?;
    fs::read_to_string(file).map_err(|_| "sandbox.env を UTF-8 のテキストとして読めません。".to_owned())
}

/// 画面で編集した内容を保存する。書式を検証し、誤りがあれば保存しない。権限は所有者のみ(0600)。
/// 一時ファイルに書いてから置き換えるので、途中で失敗しても元の内容は壊れない。
pub fn write(dir: &Path, text: &str) -> Result<(), EnvError> {
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err(EnvError { line: None, message: "ファイルが大きすぎます".into() });
    }
    parse(text)?;
    let io = |e: String| EnvError { line: None, message: e };
    fs::create_dir_all(dir).map_err(|e| io(e.to_string()))?;
    let tmp = dir.join(format!("{FILE_NAME}.tmp"));
    let _ = fs::remove_file(&tmp);
    write_private(&tmp, text).map_err(io)?;
    fs::rename(&tmp, path(dir)).map_err(|e| io(e.to_string()))
}

/// 渡す環境変数。値が空の行は除く(空のキーで上書きしないため)。
pub fn load(dir: &Path) -> Result<BTreeMap<String, String>, EnvError> {
    let file = path(dir);
    let meta = match fs::metadata(&file) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(EnvError { line: None, message: e.to_string() }),
    };
    if meta.len() > MAX_FILE_BYTES {
        return Err(EnvError { line: None, message: "ファイルが大きすぎます".into() });
    }
    let text = fs::read_to_string(&file).map_err(|_| EnvError { line: None, message: "UTF-8のテキストとして読めません".into() })?;
    let mut vars = parse(&text)?;
    vars.retain(|_, v| !v.is_empty());
    Ok(vars)
}

/// 名前と検証結果の要約(値が空の名前は含めない)。
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub names: Vec<String>,
    pub error: Option<EnvError>,
}

pub fn summary(dir: &Path) -> Summary {
    match load(dir) {
        Ok(vars) => Summary { names: vars.into_keys().collect(), error: None },
        Err(e) => Summary { names: Vec::new(), error: Some(e) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comments_quotes_and_export() {
        let text = "# c\n\nA=1\nexport B = two words \nC=\"x y\"\nD='q'\nE=\nA=3\n";
        let v = parse(text).unwrap();
        assert_eq!(v["A"], "3");
        assert_eq!(v["B"], "two words");
        assert_eq!(v["C"], "x y");
        assert_eq!(v["D"], "q");
        assert_eq!(v["E"], "");
        assert_eq!(v.len(), 5);
    }

    #[test]
    fn reports_the_line_number_without_the_value() {
        let e = parse("A=1\nnot a pair\n").unwrap_err();
        assert_eq!(e.line, Some(2));
        let e = parse("A=1\n1BAD=secretvalue\n").unwrap_err();
        assert_eq!(e.line, Some(2));
        assert!(!e.to_string().contains("secretvalue"));
        let e = parse("OK=1\nC=\"unclosed secretvalue\n").unwrap_err();
        assert_eq!(e.line, Some(2));
        assert!(!e.to_string().contains("secretvalue"));
    }

    #[test]
    fn rejects_reserved_names_and_prefixes() {
        for name in ["PATH", "HOME", "TERM", "DOCKER_HOST", "LD_PRELOAD", "DYLD_INSERT_LIBRARIES"] {
            assert!(parse(&format!("{name}=x")).is_err(), "{name}");
        }
        // AWSの認証情報は指定できる
        assert!(parse("AWS_ACCESS_KEY_ID=x\nAWS_SECRET_ACCESS_KEY=y").is_ok());
    }

    #[test]
    fn rejects_control_characters_and_limits() {
        assert!(parse("A=\"x\u{7}y\"").is_err());
        assert!(parse(&format!("A={}", "x".repeat(MAX_VALUE_BYTES + 1))).is_err());
        let many: String = (0..=MAX_VARS).map(|i| format!("V{i}=1\n")).collect();
        assert!(parse(&many).is_err());
    }

    #[test]
    fn template_is_created_once_with_private_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let file = ensure_template(dir.path()).unwrap();
        assert!(load(dir.path()).unwrap().is_empty());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        }
        fs::write(&file, "A=1\n").unwrap();
        ensure_template(dir.path()).unwrap();
        assert_eq!(load(dir.path()).unwrap()["A"], "1");
    }

    #[test]
    fn missing_file_is_empty_and_summary_hides_values() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).unwrap().is_empty());
        fs::write(path(dir.path()), "TOKEN=abc123\n").unwrap();
        let s = summary(dir.path());
        assert_eq!(s.names, vec!["TOKEN".to_owned()]);
        assert!(s.error.is_none());
        fs::write(path(dir.path()), "TOKEN abc123\n").unwrap();
        assert!(summary(dir.path()).error.is_some());
    }

    #[test]
    fn template_has_the_default_names_with_empty_values() {
        let dir = tempfile::tempdir().unwrap();
        let text = read(dir.path()).unwrap();
        assert!(text.contains("\nANTHROPIC_API_KEY=\n"));
        assert_eq!(parse(&text).unwrap().get("ANTHROPIC_API_KEY").map(String::as_str), Some(""));
        // 値が空の名前は渡さない
        assert!(load(dir.path()).unwrap().is_empty());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(path(dir.path())).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn write_validates_before_saving_and_keeps_permissions() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "ANTHROPIC_API_KEY=sk-1\nEMPTY=\n").unwrap();
        assert_eq!(load(dir.path()).unwrap().into_iter().collect::<Vec<_>>(), vec![("ANTHROPIC_API_KEY".to_owned(), "sk-1".to_owned())]);
        // 不正な内容は保存しない(元の内容が残る)
        let err = write(dir.path(), "OK=1\nPATH=/x\n").unwrap_err();
        assert_eq!(err.line, Some(2));
        assert!(read(dir.path()).unwrap().contains("sk-1"));
        assert!(!dir.path().join("sandbox.env.tmp").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(path(dir.path())).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}
