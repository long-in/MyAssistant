//! エージェントごとのDockerサンドボックス。`run_command`をそのエージェント専用のコンテナの中で実行する。
//!
//! - コンテナ・ボリューム・ネットワークはエージェントごとに別で、見えるのはそのエージェントのナレッジ(`/knowledge`。
//!   担当システムのフォルダ`/knowledge/data`を含む)と専用ボリューム(`/home/agent`)だけ。アプリのデータ・`~/.aws`・他エージェントのフォルダはマウントしない。
//! - イメージはエージェントのワークスペースにある`sandbox/Dockerfile`からビルドする(内容のハッシュをタグにして、変更で自動再ビルド)。
//! - 環境変数(`sandbox.env`と中継の設定)は実行のたびに`docker exec -e NAME`で渡す。値はdocker CLIの環境にだけ置き、引数には出さない。
//! - AWSの認証情報は直接渡さない。Bedrockはホストの中継が署名し、コンテナ用のAWSは一時キーを中継から取りに来る(`sandbox_bridge`)。
//! - Dockerが使えないときはホスト実行に切り替えず、エラーにする。
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::mpsc,
};

use super::{
    cancel::CancelToken,
    knowledge::DATA_DIR,
    sandbox_agent, sandbox_bridge, sandbox_env,
    shell::{self, Output, Stream},
    workspace::{LEADER_ID, PERSONAL_ID, WorkspaceCtx},
};

/// 標準のDockerfile。初めて使うとき、アプリデータの`sandbox/`へコピーする(全エージェント共通)。
pub const TEMPLATE: &str = include_str!("../../resources/sandbox/Dockerfile");
/// ビルドの進捗を画面へ知らせるイベント名。
pub const BUILD_EVENT: &str = "sandbox-build-progress";

/// 全エージェント共通のイメージ名(タグはDockerfileの内容のハッシュ)。
const IMAGE_REPO: &str = "myassistant-sandbox";
const CONTAINER_PREFIX: &str = "myassistant-";
const NETWORK_PREFIX: &str = "myassistant-net-";
const VOLUME_PREFIX: &str = "myassistant-home-";
/// このアプリが作ったリソースの印。値はエージェントID。
const LABEL_AGENT: &str = "myassistant.agent";
const LABEL_IMAGE: &str = "myassistant.image";
/// 共通イメージの印。
const LABEL_SHARED: &str = "myassistant.shared-image";
/// ビルド中の記録に使うキー(イメージは1つだけ)。
const BUILD_KEY: &str = "shared";
/// エージェント専用のDockerfileのフォルダ名(ワークスペース直下。共通の`sandbox/`とは別)。
const AGENT_DOCKERFILE_DIR: &str = "agent-sandbox";
/// Dockerfileの最大バイト数(画面から保存するとき)。
const DOCKERFILE_MAX_BYTES: usize = 256 * 1024;
/// コンテナを作ったときのマウント元(ナレッジ)。変わったら作り直す。
const LABEL_MOUNTS: &str = "myassistant.mounts";

const DOCKERFILE_DIR: &str = "sandbox";
const DOCKERFILE_NAME: &str = "Dockerfile";
const DOCKERIGNORE: &str = "# ビルドの文脈に含めないものを書きます。\n.dockerignore\nDockerfile\n";
/// コンテナ内のナレッジの場所。マウントするのはナレッジだけ(ワークスペースのフォルダ全体は、キーやDBがあるのでマウントしない)。
pub const KNOWLEDGE_MOUNT: &str = "/knowledge";
/// コンテナ内の担当システムのフォルダ(ソースコードなどを置く、ナレッジ直下の`data/`)の場所。作業ディレクトリの既定。
pub const DATA_MOUNT: &str = "/knowledge/data";
const REAP_INTERVAL: Duration = Duration::from_secs(60);
/// docker CLIの短い操作(inspect・rm・startなど)の待ち時間。
const CLI_TIMEOUT: Duration = Duration::from_secs(60);
/// 出力から伏せる値の最小バイト数(短い値を伏せると普通の出力まで壊れる)。
const MIN_MASK_BYTES: usize = 8;
const MASK: &str = "***";
/// ビルド失敗時に返すログの末尾の行数。
const BUILD_TAIL_LINES: usize = 40;

static APP: OnceLock<AppHandle> = OnceLock::new();
static APP_DIR: OnceLock<PathBuf> = OnceLock::new();
static LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
static BUILD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static BUILDING: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static USAGE: Mutex<Option<HashMap<String, Usage>>> = Mutex::new(None);

struct Usage {
    active: usize,
    last: Instant,
}

// ------------------------------------------------------------------ 名前・Dockerfile

/// サンドボックスを使えるのは、パーソナル・リーダー・ワークのエージェント。
/// リーダーはコマンドを実行しないが、Bedrockを使うときは、会話をコンテナ内の Claude Code に任せる(ツールは使わせない)。
pub fn is_supported(id: &str) -> bool {
    id == PERSONAL_ID || id == LEADER_ID || (!id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'))
}

/// そのエージェントの作業を常にコンテナで行うか。ワークのエージェントは常にコンテナで動く。
/// パーソナルとリーダーは、モデルにコンテナ内の Claude Code(`llm_router::SANDBOX_PROVIDER`)を選んだときだけコンテナを使う。
pub fn runs_in_sandbox(id: &str) -> bool {
    id != PERSONAL_ID && id != LEADER_ID && is_supported(id)
}

/// 新しいコンテナの名前。エージェントとの対応は名前ではなくラベル(`myassistant.agent`)で持ち、画面で確認できる。
fn new_container_name() -> String {
    let random: String = uuid::Uuid::new_v4().simple().to_string().chars().take(8).collect();
    format!("{CONTAINER_PREFIX}{random}")
}

fn network_name(id: &str) -> String {
    format!("{NETWORK_PREFIX}{id}")
}

fn volume_name(id: &str) -> String {
    format!("{VOLUME_PREFIX}{id}")
}

/// Dockerfileの内容から決めるイメージ名(全エージェント共通)。内容が変われば別のタグになる。
pub fn image_tag(dockerfile: &str) -> String {
    format!("{IMAGE_REPO}:{}", content_hash(dockerfile))
}

/// エージェント専用のDockerfileのイメージ名。共通のイメージとは名前が違うので、登録・解除でコンテナが作り直される。
pub fn agent_image_tag(id: &str, dockerfile: &str) -> String {
    format!("{IMAGE_REPO}-{}:{}", short(id), content_hash(dockerfile))
}

fn content_hash(dockerfile: &str) -> String {
    let digest = Sha256::digest(normalize(dockerfile).as_bytes());
    digest.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

fn short(id: &str) -> String {
    id.chars().filter(|c| *c != '-').take(8).collect()
}

/// テストで、アプリデータの代わりのフォルダを指定する(最初の1回だけ有効)。
#[cfg(test)]
pub fn set_app_dir_for_test(dir: &Path) {
    let _ = APP_DIR.set(dir.to_owned());
}

/// アプリデータのフォルダ。起動時(`init`)に決まる。
pub fn app_dir() -> Result<PathBuf, String> {
    APP_DIR.get().cloned().ok_or_else(|| "アプリデータのフォルダが未確定です。".to_owned())
}

/// 改行コードと末尾の空白だけの違いは同じ内容として扱う。
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_owned()
}

/// 共通のDockerfileのフォルダ(ビルドの文脈)。`app_dir`はアプリデータ。
pub fn dockerfile_dir(app_dir: &Path) -> PathBuf {
    app_dir.join(DOCKERFILE_DIR)
}

pub fn dockerfile_path(app_dir: &Path) -> PathBuf {
    dockerfile_dir(app_dir).join(DOCKERFILE_NAME)
}

/// 標準のDockerfileと`.dockerignore`を、無ければ作る。既にあるファイルは上書きしない。
pub fn ensure_template(app_dir: &Path) -> Result<PathBuf, String> {
    let dir = dockerfile_dir(app_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("sandboxフォルダを作れません: {e}"))?;
    let file = dir.join(DOCKERFILE_NAME);
    if !file.exists() {
        std::fs::write(&file, TEMPLATE).map_err(|e| format!("Dockerfileを作れません: {e}"))?;
    }
    let ignore = dir.join(".dockerignore");
    if !ignore.exists() {
        let _ = std::fs::write(&ignore, DOCKERIGNORE);
    }
    Ok(file)
}

/// 標準のDockerfileで上書きする(利用者の編集は失われる)。
pub fn reset_dockerfile(app_dir: &Path) -> Result<(), String> {
    write_dockerfile(app_dir, TEMPLATE)
}

/// Dockerfileの内容を読む。無ければ標準を作ってから読む。
pub fn read_dockerfile(app_dir: &Path) -> Result<String, String> {
    let file = ensure_template(app_dir)?;
    std::fs::read_to_string(file).map_err(|e| format!("Dockerfileを読めません: {e}"))
}

/// 画面から編集した内容を保存する。空・大きすぎる内容・NUL文字を含む内容は拒否する。
pub fn write_dockerfile(app_dir: &Path, content: &str) -> Result<(), String> {
    validate_dockerfile(content)?;
    ensure_template(app_dir)?;
    std::fs::write(dockerfile_path(app_dir), content).map_err(|e| format!("Dockerfileを書き込めません: {e}"))
}

fn validate_dockerfile(content: &str) -> Result<(), String> {
    if content.trim().is_empty() {
        return Err("Dockerfileが空です。".into());
    }
    if content.len() > DOCKERFILE_MAX_BYTES {
        return Err(format!("Dockerfileが大きすぎます(上限{}KB)。", DOCKERFILE_MAX_BYTES / 1024));
    }
    if content.contains('\0') {
        return Err("Dockerfileに使えない文字が含まれています。".into());
    }
    Ok(())
}

// ------------------------------------------------------------------ エージェント専用のDockerfile

pub fn agent_dockerfile_dir(ws_dir: &Path) -> PathBuf {
    ws_dir.join(AGENT_DOCKERFILE_DIR)
}

pub fn agent_dockerfile_path(ws_dir: &Path) -> PathBuf {
    agent_dockerfile_dir(ws_dir).join(DOCKERFILE_NAME)
}

/// このエージェント専用のDockerfileが登録されているか(あれば共通より優先する)。
pub fn agent_registered(ws_dir: &Path) -> bool {
    agent_dockerfile_path(ws_dir).is_file()
}

/// 共通のDockerfileをコピーして、専用として登録する。登録済みなら何もしない。
pub fn register_agent_dockerfile(app_dir: &Path, ws_dir: &Path) -> Result<(), String> {
    if agent_registered(ws_dir) {
        return Ok(());
    }
    let text = read_dockerfile(app_dir)?;
    let dir = agent_dockerfile_dir(ws_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("agent-sandboxフォルダを作れません: {e}"))?;
    std::fs::write(dir.join(DOCKERFILE_NAME), text).map_err(|e| format!("Dockerfileを作れません: {e}"))?;
    let _ = std::fs::write(dir.join(".dockerignore"), DOCKERIGNORE);
    Ok(())
}

/// 専用のDockerfileの登録を解除する(ファイルを消す。以後は共通を使う)。
pub fn unregister_agent_dockerfile(ws_dir: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(agent_dockerfile_dir(ws_dir)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("登録を解除できません: {e}")),
    }
}

pub fn read_agent_dockerfile(ws_dir: &Path) -> Result<String, String> {
    std::fs::read_to_string(agent_dockerfile_path(ws_dir)).map_err(|e| format!("Dockerfileを読めません: {e}"))
}

/// 専用のDockerfileを保存する。登録されていなければ拒否する(登録は`register_agent_dockerfile`で行う)。
pub fn write_agent_dockerfile(ws_dir: &Path, content: &str) -> Result<(), String> {
    validate_dockerfile(content)?;
    if !agent_registered(ws_dir) {
        return Err("専用のDockerfileが登録されていません。".into());
    }
    std::fs::write(agent_dockerfile_path(ws_dir), content).map_err(|e| format!("Dockerfileを書き込めません: {e}"))
}

/// ビルド・実行に使うイメージ。専用の登録があればそれ、なければ共通。
#[derive(Debug, Clone)]
pub struct ImageSpec {
    pub tag: String,
    /// `docker build`の文脈(Dockerfileのあるフォルダ)。
    pub context: PathBuf,
    /// 専用のイメージなら、持ち主のエージェントID。
    pub agent: Option<String>,
}

impl ImageSpec {
    /// ビルド中の記録や進捗の宛先に使うキー。
    pub fn scope(&self) -> &str {
        self.agent.as_deref().unwrap_or(BUILD_KEY)
    }
}

pub fn shared_image(app_dir: &Path) -> Result<ImageSpec, String> {
    let text = read_dockerfile(app_dir)?;
    Ok(ImageSpec { tag: image_tag(&text), context: dockerfile_dir(app_dir), agent: None })
}

/// そのエージェントが使うイメージ。専用の登録を優先する。
pub fn resolve_image(app_dir: &Path, id: &str, ws_dir: &Path) -> Result<ImageSpec, String> {
    if agent_registered(ws_dir) {
        let text = read_agent_dockerfile(ws_dir)?;
        return Ok(ImageSpec { tag: agent_image_tag(id, &text), context: agent_dockerfile_dir(ws_dir), agent: Some(id.to_owned()) });
    }
    shared_image(app_dir)
}

/// `resolve_image`のタグだけ。ファイルを作らず、読めなければ`None`(状態の表示用)。
fn peek_tag(app_dir: &Path, id: &str, ws_dir: &Path) -> Option<String> {
    if agent_registered(ws_dir) {
        return std::fs::read_to_string(agent_dockerfile_path(ws_dir)).ok().map(|t| agent_image_tag(id, &t));
    }
    std::fs::read_to_string(dockerfile_path(app_dir)).ok().map(|t| image_tag(&t))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DockerfileState {
    Missing,
    Template,
    Edited,
}

pub fn dockerfile_state(app_dir: &Path) -> DockerfileState {
    match std::fs::read_to_string(dockerfile_path(app_dir)) {
        Err(_) => DockerfileState::Missing,
        Ok(t) if normalize(&t) == normalize(TEMPLATE) => DockerfileState::Template,
        Ok(_) => DockerfileState::Edited,
    }
}

// ------------------------------------------------------------------ 作業ディレクトリ

/// コンテナにマウントするホストのフォルダ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mounts {
    /// ナレッジ(`/knowledge`)。
    pub knowledge: PathBuf,
    /// 既定の作業ディレクトリを`/knowledge/data`にするか。偽なら`/knowledge`(テスト用。実行時は常に真で、フォルダは用意済み)。
    pub data: bool,
}

impl Mounts {
    /// コンテナ内の既定の作業ディレクトリ。
    pub fn default_dir(&self) -> &'static str {
        if self.data { DATA_MOUNT } else { KNOWLEDGE_MOUNT }
    }

    /// コンテナのラベルに残す値。マウント元が変わったかどうかの比較に使う。
    fn label(&self) -> String {
        format!("knowledge={}", self.knowledge.display())
    }
}

/// `cwd`を、ナレッジ配下のホストのパスと、コンテナ内のパスへ解決する。省略時は既定の作業ディレクトリ。
/// 受け付けるのは、`/knowledge`始まり、ナレッジ配下の絶対パス、`~/`始まり、既定の作業ディレクトリからの相対パス。
pub fn map_cwd(mounts: &Mounts, arg: Option<&str>) -> Result<(PathBuf, String), String> {
    let outside = || "サンドボックスでは、作業ディレクトリを /knowledge の中にしてください。".to_owned();
    let root = mounts.knowledge.canonicalize().map_err(|_| format!("{KNOWLEDGE_MOUNT} にマウントするフォルダが見つかりません。"))?;
    let base = if mounts.data { root.join(DATA_DIR) } else { root.clone() };
    let arg = arg.map(str::trim).filter(|s| !s.is_empty());
    let candidate = match arg {
        None => base,
        Some(p) if p == KNOWLEDGE_MOUNT || p.starts_with(&format!("{KNOWLEDGE_MOUNT}/")) => {
            root.join(p[KNOWLEDGE_MOUNT.len()..].trim_start_matches('/'))
        }
        Some(p) => {
            if let Some(rest) = p.strip_prefix("~/") {
                PathBuf::from(std::env::var_os("HOME").ok_or_else(outside)?).join(rest)
            } else if p == "~" {
                PathBuf::from(std::env::var_os("HOME").ok_or_else(outside)?)
            } else if Path::new(p).is_absolute() {
                PathBuf::from(p)
            } else {
                base.join(p)
            }
        }
    };
    let host = candidate.canonicalize().map_err(|_| "指定された作業ディレクトリが見つかりません。".to_owned())?;
    let rel = host.strip_prefix(&root).map_err(|_| outside())?.to_owned();
    if !host.is_dir() {
        return Err("作業ディレクトリがディレクトリではありません。".into());
    }
    let mut inner = KNOWLEDGE_MOUNT.to_owned();
    for part in rel.components() {
        inner.push('/');
        inner.push_str(part.as_os_str().to_str().ok_or("作業ディレクトリの名前をUTF-8で扱えません。")?);
    }
    Ok((host, inner))
}

/// 担当システムのフォルダ(`/knowledge/data`)を、無ければ作って返す。
pub fn ensure_data_dir(knowledge: &Path) -> Result<PathBuf, String> {
    crate::utils::path::ensure_dir(knowledge, &[DATA_DIR.to_owned()]).map_err(|e| format!("{DATA_MOUNT} のフォルダを作れません: {e}"))
}

/// コンテナにマウントするフォルダ。
pub fn mounts_for(knowledge: &Path) -> Result<Mounts, String> {
    ensure_data_dir(knowledge)?;
    Ok(Mounts { knowledge: knowledge.to_owned(), data: true })
}

// ------------------------------------------------------------------ 出力の伏せ字

/// 登録済みの値を出力から伏せる。読み取りの境界で値が分かれても伏せられるよう、末尾を次の入力まで保留する。
/// 値がどれも改行を含まないときは、最後の改行までは保留しない(値は改行をまたげない)。
/// 応答を待って止まる相手(Claude Code の承認要求)の行が、保留されたまま届かなくならないようにする。
pub struct Masker {
    secrets: Vec<String>,
    keep: usize,
    /// 値がどれも改行を含まないか。
    line_safe: bool,
    carry: String,
}

impl Masker {
    pub fn new(values: &[String]) -> Self {
        let mut secrets: Vec<String> = values.iter().filter(|v| v.len() >= MIN_MASK_BYTES).cloned().collect();
        secrets.sort_by_key(|v| std::cmp::Reverse(v.len()));
        secrets.dedup();
        let keep = secrets.first().map_or(0, |s| s.len().saturating_sub(1));
        let line_safe = secrets.iter().all(|s| !s.contains('\n'));
        Self { secrets, keep, line_safe, carry: String::new() }
    }

    /// 伏せる値を加える(実行の途中で中継が配った一時キーなど)。
    pub fn add(&mut self, values: &[String]) {
        let mut all = std::mem::take(&mut self.secrets);
        all.extend(values.iter().cloned());
        let carry = std::mem::take(&mut self.carry);
        *self = Self::new(&all);
        self.carry = carry;
    }

    /// 完全に置き換える(まとめた出力用)。
    pub fn mask(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for s in &self.secrets {
            out = out.replace(s.as_str(), MASK);
        }
        out
    }

    /// 逐次の入力。値の途中で切れうる末尾は保留する。
    pub fn push(&mut self, chunk: &str) -> String {
        if self.secrets.is_empty() {
            return chunk.to_owned();
        }
        let mut buf = std::mem::take(&mut self.carry);
        buf.push_str(chunk);
        let mut cut = buf.len().saturating_sub(self.keep);
        while !buf.is_char_boundary(cut) {
            cut -= 1;
        }
        // 改行をまたぐ値は無いので、最後の改行までは出してよい
        if self.line_safe
            && let Some(i) = buf.rfind('\n')
        {
            cut = cut.max(i + 1);
        }
        // 切れ目が値の途中に当たるなら、その値の手前まで戻す
        loop {
            let mut moved = false;
            for s in &self.secrets {
                for (i, _) in buf.match_indices(s.as_str()) {
                    if i < cut && i + s.len() > cut {
                        cut = i;
                        moved = true;
                    }
                }
            }
            if !moved {
                break;
            }
        }
        self.carry = buf[cut..].to_owned();
        self.mask(&buf[..cut])
    }

    /// 保留した末尾を出す。
    pub fn flush(&mut self) -> String {
        let rest = std::mem::take(&mut self.carry);
        self.mask(&rest)
    }
}

// ------------------------------------------------------------------ docker CLI

/// docker CLIの場所。GUIアプリから起動されるとPATHに無いことがあるので、よくある場所を順に探す。
pub fn find_docker() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut candidates: Vec<PathBuf> = ["/usr/local/bin/docker", "/opt/homebrew/bin/docker", "/Applications/Docker.app/Contents/Resources/bin/docker"]
        .iter()
        .map(PathBuf::from)
        .collect();
    if let Some(h) = &home {
        candidates.push(h.join(".orbstack/bin/docker"));
    }
    if let Some(found) = candidates.into_iter().find(|p| p.is_file()) {
        return Some(found);
    }
    std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|d| d.join("docker")).find(|p| p.is_file()))
}

/// docker CLIが資格情報ヘルパー(`docker-credential-*`)を見つけられるよう、PATHを補う。
fn path_for(bin: &Path) -> String {
    let dir = bin.parent().map(|d| d.display().to_string()).unwrap_or_default();
    let base = std::env::var("PATH").unwrap_or_default();
    format!("{dir}:/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:{base}")
}

fn docker_cmd(bin: &Path) -> Command {
    let mut c = Command::new(bin);
    c.env("PATH", path_for(bin)).stdin(Stdio::null()).kill_on_drop(true);
    c
}

struct Out {
    ok: bool,
    stdout: String,
    stderr: String,
}

/// docker CLIを実行して結果を得る。起動できない・時間切れは`Err`。
async fn docker(bin: &Path, args: &[&str], timeout: Duration) -> Result<Out, String> {
    let mut cmd = docker_cmd(bin);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    let fut = cmd.output();
    match tokio::time::timeout(timeout, fut).await {
        Err(_) => Err(format!("docker {} が時間内に終わりませんでした。", args.first().copied().unwrap_or(""))),
        Ok(Err(e)) => Err(format!("dockerを起動できませんでした: {e}")),
        Ok(Ok(o)) => Ok(Out {
            ok: o.status.success(),
            stdout: String::from_utf8_lossy(&o.stdout).trim().to_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).trim().to_owned(),
        }),
    }
}

fn clip_err(s: &str) -> String {
    s.lines().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DockerState {
    /// dockerコマンドが見つからない。
    Missing,
    /// 入っているが、デーモンに接続できない(Docker Desktopなどが起動していない)。
    Stopped,
    Ready,
}

/// 共通イメージの状態(Dockerと、現在のDockerfileのイメージ)。
#[derive(Debug, Clone, Serialize)]
pub struct ImageStatus {
    pub docker: DockerState,
    /// 現在のDockerfileのイメージがビルド済みか。
    pub image_ready: bool,
    pub building: bool,
}

pub async fn image_status(app_dir: &Path) -> ImageStatus {
    let building = is_building(BUILD_KEY);
    let Some(bin) = find_docker() else {
        return ImageStatus { docker: DockerState::Missing, image_ready: false, building };
    };
    if !daemon_up(&bin).await {
        return ImageStatus { docker: DockerState::Stopped, image_ready: false, building };
    }
    let image_ready = match std::fs::read_to_string(dockerfile_path(app_dir)) {
        Ok(text) => image_exists(&bin, &image_tag(&text)).await,
        Err(_) => false,
    };
    ImageStatus { docker: DockerState::Ready, image_ready, building }
}

/// エージェントのコンテナと、そのエージェントが使うイメージの状態。
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub docker: DockerState,
    pub container_running: bool,
    /// 起動中で、いまのイメージから作られたコンテナがあるか(チャットを使える状態)。
    pub container_ready: bool,
    /// このエージェントのコンテナの名前(まだ作っていなければ`None`)。
    pub container_name: Option<String>,
    /// 専用のDockerfileが登録されているか(登録があれば共通より優先)。
    pub dockerfile_registered: bool,
    /// このエージェントが使うイメージがビルド済みか。
    pub image_ready: bool,
    pub building: bool,
}

pub async fn status(id: &str, app_dir: &Path, ws_dir: &Path) -> Status {
    let registered = agent_registered(ws_dir);
    let building = is_building(if registered { id } else { BUILD_KEY });
    let base = Status { docker: DockerState::Missing, container_running: false, container_ready: false, container_name: None, dockerfile_registered: registered, image_ready: false, building };
    let Some(bin) = find_docker() else { return base };
    if !daemon_up(&bin).await {
        return Status { docker: DockerState::Stopped, ..base };
    }
    let tag = peek_tag(app_dir, id, ws_dir);
    let image_ready = match &tag {
        Some(tag) => image_exists(&bin, tag).await,
        None => false,
    };
    let container = find_container(&bin, id).await;
    Status {
        docker: DockerState::Ready,
        container_running: container.as_ref().is_some_and(|c| c.running),
        container_ready: container.as_ref().is_some_and(|c| c.running && Some(&c.image) == tag.as_ref()),
        container_name: container.map(|c| c.name),
        image_ready,
        ..base
    }
}

async fn daemon_up(bin: &Path) -> bool {
    docker(bin, &["version", "--format", "{{.Server.Version}}"], Duration::from_secs(10)).await.is_ok_and(|o| o.ok)
}

/// Dockerが使える状態か確かめ、使えなければ利用者向けの文面で失敗する。
async fn require_docker() -> Result<PathBuf, String> {
    let bin = find_docker().ok_or("Dockerが見つかりません。Dockerをインストールして起動してください(サンドボックスが有効なので、ホストでは実行しません)。")?;
    if !daemon_up(&bin).await {
        return Err("Dockerに接続できません。Docker Desktopなどを起動してください(サンドボックスが有効なので、ホストでは実行しません)。".into());
    }
    Ok(bin)
}

async fn image_exists(bin: &Path, tag: &str) -> bool {
    docker(bin, &["image", "inspect", tag], CLI_TIMEOUT).await.is_ok_and(|o| o.ok)
}

// ------------------------------------------------------------------ ロックと使用状況

fn ws_lock(id: &str) -> Arc<tokio::sync::Mutex<()>> {
    let map = LOCKS.get_or_init(Default::default);
    map.lock().unwrap_or_else(|p| p.into_inner()).entry(id.to_owned()).or_default().clone()
}

fn is_building(scope: &str) -> bool {
    BUILDING.lock().unwrap_or_else(|p| p.into_inner()).as_ref().is_some_and(|s| s.contains(scope))
}

struct BuildingGuard(String);

impl BuildingGuard {
    fn new(scope: &str) -> Self {
        BUILDING.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(Default::default).insert(scope.to_owned());
        Self(scope.to_owned())
    }
}

impl Drop for BuildingGuard {
    fn drop(&mut self) {
        if let Some(s) = BUILDING.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
            s.remove(&self.0);
        }
    }
}

/// 実行中のコマンド数の記録。0の状態が続いたコンテナは止める。
struct UseGuard(String);

impl UseGuard {
    fn new(id: &str) -> Self {
        let mut map = USAGE.lock().unwrap_or_else(|p| p.into_inner());
        let u = map.get_or_insert_with(Default::default).entry(id.to_owned()).or_insert(Usage { active: 0, last: Instant::now() });
        u.active += 1;
        Self(id.to_owned())
    }
}

impl Drop for UseGuard {
    fn drop(&mut self) {
        if let Some(u) = USAGE.lock().unwrap_or_else(|p| p.into_inner()).as_mut().and_then(|m| m.get_mut(&self.0)) {
            u.active = u.active.saturating_sub(1);
            u.last = Instant::now();
        }
    }
}

// ------------------------------------------------------------------ ビルド

#[derive(Serialize, Clone)]
struct BuildProgress<'a> {
    /// `shared`(共通のイメージ)またはエージェントID(専用のイメージ)。
    scope: &'a str,
    line: &'a str,
}

fn emit_progress(scope: &str, line: &str) {
    if let Some(app) = APP.get() {
        let _ = app.emit(BUILD_EVENT, BuildProgress { scope, line });
    }
}

/// `spec`のDockerfileからイメージをビルドして、タグを返す。ビルド済みなら何もしない。
/// 進捗は`on_line`と画面のイベントへ流す。ビルドは全体で1本ずつ。
pub async fn build_image(spec: &ImageSpec, force: bool, cancel: &CancelToken, on_line: &mut impl FnMut(&str)) -> Result<String, String> {
    let bin = require_docker().await?;
    let tag = spec.tag.clone();
    if !force && image_exists(&bin, &tag).await {
        return Ok(tag);
    }
    let _guard = BuildingGuard::new(spec.scope());
    let _serial = BUILD.lock().await;
    // 待っている間に別の実行がビルドを終えていることがある
    if !force && image_exists(&bin, &tag).await {
        return Ok(tag);
    }
    let mut line = |l: &str| {
        emit_progress(spec.scope(), l);
        on_line(l);
    };
    line(&format!("イメージをビルドします({tag})"));
    let mut cmd = docker_cmd(&bin);
    let label = match &spec.agent {
        Some(id) => format!("{LABEL_AGENT}={id}"),
        None => format!("{LABEL_SHARED}=1"),
    };
    cmd.args(["build", "--progress=plain", "-t", &tag, "--label", &label])
        .arg(&spec.context)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("docker buildを起動できませんでした: {e}"))?;
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        readers.push(spawn_lines(out, tx.clone()));
    }
    if let Some(err) = child.stderr.take() {
        readers.push(spawn_lines(err, tx.clone()));
    }
    drop(tx);
    let mut tail: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let mut push = |l: String, line: &mut dyn FnMut(&str)| {
        line(&l);
        if tail.len() >= BUILD_TAIL_LINES {
            tail.pop_front();
        }
        tail.push_back(l);
    };
    let status = loop {
        tokio::select! {
            Some(l) = rx.recv() => push(l, &mut line),
            status = child.wait() => break status.ok(),
            () = cancel.cancelled() => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                for r in readers { r.abort(); }
                return Err("ビルドを中断しました。".into());
            }
        }
    };
    while let Ok(Some(l)) = tokio::time::timeout(Duration::from_millis(300), rx.recv()).await {
        push(l, &mut line);
    }
    for r in readers {
        r.abort();
    }
    if status.is_some_and(|s| s.success()) {
        Ok(tag)
    } else {
        let log: Vec<String> = tail.into_iter().collect();
        Err(format!("イメージのビルドに失敗しました。\n{}", log.join("\n")))
    }
}

fn spawn_lines<R: tokio::io::AsyncRead + Unpin + Send + 'static>(pipe: R, tx: mpsc::UnboundedSender<String>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut lines = BufReader::new(pipe).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            if tx.send(l).is_err() {
                break;
            }
        }
    })
}

// ------------------------------------------------------------------ コンテナ

struct Container {
    name: String,
    running: bool,
    image: String,
    mounts: String,
}

/// このエージェントのコンテナの名前をすべて返す(ラベルで探す。通常は0か1つ)。
async fn container_names(bin: &Path, id: &str) -> Vec<String> {
    let filter = format!("label={LABEL_AGENT}={id}");
    match docker(bin, &["ps", "-a", "--filter", &filter, "--format", "{{.Names}}"], CLI_TIMEOUT).await {
        Ok(o) if o.ok => o.stdout.lines().map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned).collect(),
        _ => Vec::new(),
    }
}

async fn find_container(bin: &Path, id: &str) -> Option<Container> {
    let fmt = format!("{{{{.State.Running}}}}|{{{{index .Config.Labels \"{LABEL_IMAGE}\"}}}}|{{{{index .Config.Labels \"{LABEL_MOUNTS}\"}}}}");
    for name in container_names(bin, id).await {
        let Ok(o) = docker(bin, &["inspect", "-f", &fmt, &name], CLI_TIMEOUT).await else { continue };
        if !o.ok {
            continue;
        }
        let mut parts = o.stdout.splitn(3, '|');
        let (Some(running), Some(image), Some(mounts)) = (parts.next(), parts.next(), parts.next()) else { continue };
        return Some(Container { name, running: running == "true", image: image.to_owned(), mounts: mounts.to_owned() });
    }
    None
}

/// このエージェントのコンテナをすべて消す(ボリュームは残る)。
async fn remove_containers(bin: &Path, id: &str) {
    for name in container_names(bin, id).await {
        let _ = docker(bin, &["rm", "-f", &name], CLI_TIMEOUT).await;
    }
}

/// `docker run`の引数。マウントは、ナレッジと専用ボリュームだけ。
fn run_args(id: &str, name: &str, tag: &str, mounts: &Mounts) -> Vec<String> {
    let mut a: Vec<String> = ["run", "-d", "--init", "--name"].iter().map(|s| (*s).to_owned()).collect();
    a.push(name.to_owned());
    let mut push = |items: &[&str]| a.extend(items.iter().map(|s| (*s).to_owned()));
    push(&["--label", &format!("{LABEL_AGENT}={id}")]);
    push(&["--label", &format!("{LABEL_IMAGE}={tag}")]);
    push(&["--label", &format!("{LABEL_MOUNTS}={}", mounts.label())]);
    push(&["--network", &network_name(id)]);
    push(&["--read-only"]);
    push(&["--tmpfs", "/tmp:rw,nosuid,size=512m"]);
    push(&["--tmpfs", "/var/tmp:rw,nosuid,size=256m"]);
    push(&["--cap-drop", "ALL"]);
    push(&["--security-opt", "no-new-privileges"]);
    push(&["--pids-limit", "512"]);
    push(&["--memory", "2g"]);
    push(&["--cpus", "2"]);
    push(&["--mount", &format!("type=bind,source={},target={KNOWLEDGE_MOUNT}", mounts.knowledge.display())]);
    push(&["--mount", &format!("type=volume,source={},target=/home/agent", volume_name(id))]);
    push(&["-u", "agent", "-w", mounts.default_dir(), "-e", "HOME=/home/agent"]);
    push(&[tag, "sleep", "infinity"]);
    a
}

/// コンテナが使える状態(イメージが現在のもの、マウント元が同じ、起動中)になるようにして、名前を返す。
async fn ensure_container(bin: &Path, id: &str, spec: &ImageSpec, mounts: &Mounts) -> Result<String, String> {
    let tag = spec.tag.as_str();
    let lock = ws_lock(id);
    let _held = lock.lock().await;
    if mounts.knowledge.display().to_string().contains(',') {
        return Err(format!("{KNOWLEDGE_MOUNT} のフォルダ名に「,」を含むため、サンドボックスにマウントできません。"));
    }
    let label = mounts.label();
    match find_container(bin, id).await {
        Some(c) if c.image == tag && c.mounts == label => {
            if !c.running {
                let o = docker(bin, &["start", &c.name], CLI_TIMEOUT).await?;
                if !o.ok {
                    return Err(format!("コンテナを起動できませんでした: {}", clip_err(&o.stderr)));
                }
            }
            return Ok(c.name);
        }
        // イメージかマウント元が変わった。ボリュームは残して作り直す
        Some(_) => remove_containers(bin, id).await,
        None => {}
    }
    let name = new_container_name();
    let net = network_name(id);
    let exists = docker(bin, &["network", "inspect", &net], CLI_TIMEOUT).await.is_ok_and(|o| o.ok);
    if !exists {
        let o = docker(bin, &["network", "create", "--label", &format!("{LABEL_AGENT}={id}"), &net], CLI_TIMEOUT).await?;
        if !o.ok {
            return Err(format!("ネットワークを作れませんでした: {}", clip_err(&o.stderr)));
        }
    }
    let args = run_args(id, &name, tag, mounts);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let o = docker(bin, &refs, Duration::from_secs(120)).await?;
    if !o.ok {
        return Err(format!("コンテナを作れませんでした: {}", clip_err(&o.stderr)));
    }
    // Dockerfileを編集した場合の前提(shとsetsid、agentユーザー、Claude Code)を確かめる
    let diag = docker(bin, &["exec", &name, "sh", "-c", DIAG], CLI_TIMEOUT).await?;
    if !diag.ok {
        let _ = docker(bin, &["rm", "-f", &name], CLI_TIMEOUT).await;
        return Err("イメージが前提を満たしていません。Dockerfileに、ユーザー agent、sh、setsid(util-linux)、claude(Claude Code)が必要です。".into());
    }
    prune_old_images(bin, id, spec).await;
    Ok(name)
}

/// コンテナを作った直後の診断。Dockerfileを編集しても崩してはいけない前提。
const DIAG: &str = "command -v setsid >/dev/null && id -u agent >/dev/null && command -v claude >/dev/null";

/// 古いタグのイメージを消す(別のエージェントのコンテナが使用中のものは消えない)。
/// 専用のイメージを使うときは、このエージェントの古い専用イメージだけ。共通を使うときは、共通の古いタグと、このエージェントの専用イメージ(登録解除後の残り)。
async fn prune_old_images(bin: &Path, id: &str, spec: &ImageSpec) {
    let mut targets = vec![(format!("label={LABEL_AGENT}={id}"), spec.agent.is_some())];
    if spec.agent.is_none() {
        targets.push((format!("label={LABEL_SHARED}=1"), true));
    }
    for (filter, keep_current) in targets {
        let Ok(o) = docker(bin, &["images", "--filter", &filter, "--format", "{{.Repository}}:{{.Tag}}"], CLI_TIMEOUT).await else { continue };
        for tag in o.stdout.lines().map(str::trim).filter(|t| !(t.is_empty() || (keep_current && *t == spec.tag))) {
            let _ = docker(bin, &["rmi", tag], CLI_TIMEOUT).await;
        }
    }
}

/// 準備済みのイメージでコンテナを作って起動する。イメージが未ビルドなら失敗する(ビルドは別の操作)。
pub async fn start_container(ctx: &WorkspaceCtx, mounts: &Mounts) -> Result<String, String> {
    let id = ctx.id.as_str();
    if !is_supported(id) {
        return Err("リーダーではサンドボックスを使えません。".into());
    }
    let bin = require_docker().await?;
    let spec = resolve_image(&app_dir()?, id, &ctx.dir)?;
    if !image_exists(&bin, &spec.tag).await {
        return Err("イメージがビルドされていません。先にイメージをビルドしてください。".into());
    }
    let name = ensure_container(&bin, id, &spec, mounts).await?;
    touch(id);
    Ok(name)
}

/// コンテナを止める(消さない。ボリュームも残る)。
pub async fn stop_container(id: &str) -> Result<(), String> {
    if !is_supported(id) {
        return Err("リーダーではサンドボックスを使えません。".into());
    }
    let bin = require_docker().await?;
    let lock = ws_lock(id);
    let _held = lock.lock().await;
    for name in container_names(&bin, id).await {
        let _ = docker(&bin, &["stop", "-t", "3", &name], Duration::from_secs(30)).await;
    }
    if let Some(m) = USAGE.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
        m.remove(id);
    }
    Ok(())
}

/// 使用状況に記録する(アイドル停止の起点にする)。
fn touch(id: &str) {
    let mut map = USAGE.lock().unwrap_or_else(|p| p.into_inner());
    let u = map.get_or_insert_with(Default::default).entry(id.to_owned()).or_insert(Usage { active: 0, last: Instant::now() });
    u.last = Instant::now();
}

/// コンテナを作り直す(ボリュームは残す)。
pub async fn reset_container(id: &str) -> Result<(), String> {
    if !is_supported(id) {
        return Err("リーダーではサンドボックスを使えません。".into());
    }
    let bin = require_docker().await?;
    let lock = ws_lock(id);
    let _held = lock.lock().await;
    remove_containers(&bin, id).await;
    Ok(())
}

// ------------------------------------------------------------------ 実行

const PIDFILE_DIR: &str = "/tmp";
/// 新しいセッションで起動し、プロセスグループのIDを記録してから`sh -c`で実行する。中断時にグループごと止めるため。
const WRAP: &str = r#"echo $$ > "$1"; exec sh -c "$2""#;
const KILL: &str = r#"p=$(cat "$1" 2>/dev/null); if [ -n "$p" ]; then kill -KILL -- "-$p" 2>/dev/null || kill -KILL "-$p" 2>/dev/null; fi; rm -f "$1""#;
/// 実行の仕組みが渡す固定の環境変数(秘密ではないので引数に書く)。
const FIXED_ENV: [&str; 5] = ["TERM=dumb", "NO_COLOR=1", "PAGER=cat", "GIT_PAGER=cat", "GIT_TERMINAL_PROMPT=0"];

/// `docker exec`の引数。環境変数は名前だけを渡し、値はdocker CLIの環境から読ませる。`stdin`なら標準入力をつなぐ(`-i`)。
fn exec_args(container: &str, cwd: &str, command: &str, pidfile: &str, env_names: &[String], stdin: bool) -> Vec<String> {
    let mut a: Vec<String> = vec!["exec".into()];
    if stdin {
        a.push("-i".into());
    }
    a.extend(["-w".into(), cwd.into()]);
    for e in FIXED_ENV {
        a.push("-e".into());
        a.push(e.into());
    }
    for n in env_names {
        a.push("-e".into());
        a.push(n.clone());
    }
    for s in [container, "setsid", "-w", "sh", "-c", WRAP, "sh", pidfile, command] {
        a.push(s.into());
    }
    a
}

/// コンテナ内で、記録したプロセスグループを止めてpidファイルを消す。
async fn kill_group(bin: &Path, container: &str, pidfile: &str) {
    let _ = docker(bin, &["exec", container, "sh", "-c", KILL, "sh", pidfile], Duration::from_secs(10)).await;
}

pub struct Request<'a> {
    pub ctx: &'a WorkspaceCtx,
    /// マウントするフォルダ(`mounts_for`の結果)。
    pub mounts: &'a Mounts,
    pub command: &'a str,
    /// コンテナ内の作業ディレクトリ(`map_cwd`の結果)。
    pub cwd: &'a str,
    pub timeout: Duration,
}

/// 1回の実行で渡す環境変数と、中継のトークン(実行が終わるまで持っておく。落とすと無効になる)。
pub(crate) struct ExecEnv {
    pub vars: BTreeMap<String, String>,
    pub grants: Vec<sandbox_bridge::Grant>,
}

/// 渡す環境変数(エージェントが選んだプロキシ・コンテナ用のAWSの中継・`sandbox.env`)。同じ名前は後のものが優先(`sandbox.env`が最優先)。
/// コンテナ用のAWS設定が無ければ、AWSの認証情報は渡さない(Bedrock用の設定は使わない)。
pub(crate) async fn env_for(ctx: &WorkspaceCtx) -> Result<ExecEnv, String> {
    let mut vars = proxy_vars(super::proxy::container_url_for(ctx));
    let mut grants = Vec::new();
    if let Some((grant, aws_vars)) = sandbox_bridge::aws_env(ctx).await? {
        vars.extend(aws_vars);
        grants.push(grant);
    }
    vars.extend(sandbox_env::load(&ctx.dir).map_err(|e| e.to_string())?);
    Ok(ExecEnv { vars, grants })
}

/// プロキシの環境変数(大文字・小文字の両方)。ホストの中継(`host.docker.internal`)とローカル宛ては通さない。
fn proxy_vars(url: Option<String>) -> BTreeMap<String, String> {
    let Some(url) = url else { return BTreeMap::new() };
    let no_proxy = format!("localhost,127.0.0.1,::1,{}", sandbox_bridge::HOST_FROM_CONTAINER);
    [("HTTP_PROXY", &url), ("HTTPS_PROXY", &url), ("NO_PROXY", &no_proxy)]
        .into_iter()
        .flat_map(|(k, v)| [(k.to_owned(), v.clone()), (k.to_ascii_lowercase(), v.clone())])
        .collect()
}

/// 伏せる値。リージョンや、アプリが決めるモデル名・内部の値(`MYASSISTANT_`)のように秘密でない値は除く。
fn secret_values(vars: &BTreeMap<String, String>) -> Vec<String> {
    vars.iter()
        .filter(|(k, _)| !k.ends_with("REGION") && !k.starts_with("MYASSISTANT_") && !sandbox_agent::NON_SECRET_NAMES.contains(&k.as_str()))
        .map(|(_, v)| v.clone())
        // 貼り付けた settings.json の`env`の値も伏せる(`MYASSISTANT_SETTINGS`自体は秘密でない値として除く)
        .chain(sandbox_agent::settings_secret_values(vars.get("MYASSISTANT_SETTINGS")))
        .collect()
}

/// コマンドをそのエージェントのコンテナで実行する。`shell::run`と同じ`Output`を返す。
pub async fn run(req: Request<'_>, cancel: &CancelToken, on_chunk: impl FnMut(Stream, &str)) -> Result<Output, String> {
    if !is_supported(&req.ctx.id) {
        return Err("リーダーではサンドボックスを使えません。".into());
    }
    let ExecEnv { vars, grants } = env_for(req.ctx).await?;
    exec(Exec { ctx: req.ctx, mounts: req.mounts, cwd: req.cwd, command: req.command, vars, grants, input: None, timeout: req.timeout }, cancel, on_chunk).await
}

/// コンテナでの1回の実行。`vars`は渡す環境変数のすべて(名前だけを引数に書き、値はdocker CLIの環境で渡す)。
pub struct Exec<'a> {
    pub ctx: &'a WorkspaceCtx,
    pub mounts: &'a Mounts,
    /// コンテナ内の作業ディレクトリ。
    pub cwd: &'a str,
    /// `sh -c`で実行する文字列。可変の値は埋め込まず、`vars`の環境変数で渡して参照する。
    pub command: &'a str,
    pub vars: BTreeMap<String, String>,
    /// 中継のトークン。実行が終わるまで持ち、終わったら落として無効にする。
    pub grants: Vec<sandbox_bridge::Grant>,
    /// 標準入力へ書き込む内容の送り口(無ければ標準入力は閉じる)。送り口がすべて閉じられると、標準入力も閉じる。
    pub input: Option<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>,
    pub timeout: Duration,
}

/// 事前に準備した(ビルド済みのイメージで起動中の)コンテナの名前。実行時にはビルドも起動もしない。
/// 準備が済んでいなければ`NOT_READY`で失敗する(画面でイメージのビルドとコンテナの起動を行う)。
async fn running_container(bin: &Path, id: &str, spec: &ImageSpec, mounts: &Mounts) -> Result<String, String> {
    match find_container(bin, id).await {
        Some(c) if c.running && c.image == spec.tag && c.mounts == mounts.label() => Ok(c.name),
        _ => Err(NOT_READY.to_owned()),
    }
}

/// イメージのビルドとコンテナの起動を事前に行っていないときのエラー文面。
pub const NOT_READY: &str = "サンドボックスの準備ができていません。Sandbox の設定で、イメージのビルドとコンテナの起動を行ってください。";

/// イメージとコンテナ(事前に準備したもの)で、`docker exec`で実行する。出力は伏せ字にしてから`on_chunk`へ渡し、終了後にまとめて返す。
/// 時間切れ・中断・通常の終了のたびに、コンテナ内のプロセスグループごと止める。
pub async fn exec(req: Exec<'_>, cancel: &CancelToken, mut on_chunk: impl FnMut(Stream, &str)) -> Result<Output, String> {
    let id = req.ctx.id.as_str();
    if !is_supported(id) {
        return Err("リーダーではサンドボックスを使えません。".into());
    }
    let bin = require_docker().await?;
    let spec = resolve_image(&app_dir()?, id, &req.ctx.dir)?;
    let container = running_container(&bin, id, &spec, req.mounts).await?;
    let _use = UseGuard::new(id);
    // コンテナ用のAWSを使うときは、一時キーを中継から取るための設定をコンテナ内に置く
    if req.grants.iter().any(sandbox_bridge::Grant::needs_aws_files) {
        let o = docker(&bin, &["exec", &container, "sh", "-c", sandbox_bridge::AWS_SETUP_SCRIPT, "sh", sandbox_bridge::aws_config()], CLI_TIMEOUT).await?;
        if !o.ok {
            return Err(format!("コンテナ用の AWS 設定を準備できませんでした: {}", clip_err(&o.stderr)));
        }
    }
    let _grants = req.grants;

    let pidfile = format!("{PIDFILE_DIR}/ma-{}.pid", uuid::Uuid::new_v4().simple());
    let names: Vec<String> = req.vars.keys().cloned().collect();
    let mut cmd = docker_cmd(&bin);
    cmd.args(exec_args(&container, req.cwd, req.command, &pidfile, &names, req.input.is_some()));
    for (k, v) in &req.vars {
        cmd.env(k, v);
    }

    let values = secret_values(&req.vars);
    let (mut out_mask, mut err_mask) = (Masker::new(&values), Masker::new(&values));
    // 中継がこの実行へ配った一時キーも伏せる(配られたら、次の出力から)
    let mut handed = 0;
    let mut add_handed = |out_mask: &mut Masker, err_mask: &mut Masker| {
        let values: Vec<String> = _grants.iter().flat_map(sandbox_bridge::Grant::handed_values).collect();
        if values.len() != handed {
            handed = values.len();
            out_mask.add(&values);
            err_mask.add(&values);
        }
    };
    let result = shell::run_prepared_streaming(cmd, req.input, req.timeout, cancel, |stream, text| {
        add_handed(&mut out_mask, &mut err_mask);
        let masked = match stream {
            Stream::Stdout => out_mask.push(text),
            Stream::Stderr => err_mask.push(text),
        };
        if !masked.is_empty() {
            on_chunk(stream, &masked);
        }
    })
    .await;
    // docker CLIを止めてもコンテナ内のプロセスは残るので、グループごと止める(背景に残した子孫も含む)
    kill_group(&bin, &container, &pidfile).await;
    add_handed(&mut out_mask, &mut err_mask);
    let mut out = result?;
    for (stream, m) in [(Stream::Stdout, &mut out_mask), (Stream::Stderr, &mut err_mask)] {
        let rest = m.flush();
        if !rest.is_empty() {
            on_chunk(stream, &rest);
        }
    }
    out.stdout = out_mask.mask(&out.stdout);
    out.stderr = err_mask.mask(&out.stderr);
    Ok(out)
}

// ------------------------------------------------------------------ 後始末・常駐

/// 画面へのイベント送出の準備(アプリデータの場所の決定)。アプリ起動時に1回呼ぶ。
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = APP_DIR.set(dir);
    }
}

/// 前回のアプリが動かしたままのコンテナを止め、前回の終了時に起動していたものを起動し直して、アイドル停止の見張りを始める。
/// `Workspaces`の登録後に1回呼ぶ。
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        stop_all().await;
        restore_running(&app).await;
        loop {
            tokio::time::sleep(REAP_INTERVAL).await;
            reap_idle().await;
        }
    });
}

/// エージェントごとの自動停止の時間。0(既定)なら止めない。
fn idle_limit(id: &str) -> Option<Duration> {
    let app = APP.get()?;
    let ctx = app.state::<super::workspace::Workspaces>().get(id)?;
    let mins = sandbox_agent::load(&ctx.db).ok()?.idle_stop_mins;
    (mins > 0).then(|| Duration::from_secs(u64::from(mins) * 60))
}

async fn reap_idle() {
    let active_zero: Vec<String> = {
        let map = USAGE.lock().unwrap_or_else(|p| p.into_inner());
        map.as_ref().map(|m| m.iter().filter(|(_, u)| u.active == 0).map(|(k, _)| k.clone()).collect()).unwrap_or_default()
    };
    let candidates: Vec<(String, Duration)> = active_zero.into_iter().filter_map(|id| idle_limit(&id).map(|d| (id, d))).collect();
    let Some(bin) = find_docker() else { return };
    for (id, limit) in candidates {
        let lock = ws_lock(&id);
        let _held = lock.lock().await;
        // 待っている間に使われていないか確かめる
        let still_idle = USAGE
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .and_then(|m| m.get(&id))
            .is_some_and(|u| u.active == 0 && u.last.elapsed() >= limit);
        if still_idle {
            for name in container_names(&bin, &id).await {
                let _ = docker(&bin, &["stop", "-t", "3", &name], Duration::from_secs(30)).await;
            }
            if let Some(m) = USAGE.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
                m.remove(&id);
            }
        }
    }
}

/// 起動中のコンテナのエージェントIDを記録するファイル(次回の起動時に、同じコンテナを起動し直す)。
const RUNNING_FILE: &str = "sandbox-running.json";

/// 起動中のエージェントIDを記録してから、すべてのコンテナを止める(アプリ終了時)。
pub async fn stop_all_remembering() {
    if let (Some(bin), Ok(dir)) = (find_docker(), app_dir()) {
        let filter = format!("label={LABEL_AGENT}");
        let fmt = format!("{{{{index .Labels \"{LABEL_AGENT}\"}}}}");
        if let Ok(o) = docker(&bin, &["ps", "--filter", &filter, "--format", &fmt], Duration::from_secs(10)).await {
            let ids: Vec<&str> = o.stdout.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
            if let Ok(text) = serde_json::to_string(&ids) {
                let _ = std::fs::write(dir.join(RUNNING_FILE), text);
            }
        }
    }
    stop_all().await;
}

/// 前回の終了時に起動していたコンテナを、ビルド済みのイメージがあるものだけ起動し直す。
/// Dockerが使えない・失敗したときは何もしない(チャットで準備を促す)。
pub async fn restore_running(app: &AppHandle) {
    let Ok(dir) = app_dir() else { return };
    let ids: Vec<String> = std::fs::read_to_string(dir.join(RUNNING_FILE)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    for id in ids {
        let Some(ctx) = app.state::<super::workspace::Workspaces>().get(&id) else { continue };
        let Ok(root) = super::batch::knowledge_root(&ctx) else { continue };
        let Ok(mounts) = mounts_for(&root) else { continue };
        if let Err(e) = start_container(&ctx, &mounts).await {
            log::warn!("sandbox restore failed ({id}): {e}");
        }
    }
}

/// このアプリのコンテナをすべて止める(アプリ起動時・終了時)。
pub async fn stop_all() {
    let Some(bin) = find_docker() else { return };
    let filter = format!("label={LABEL_AGENT}");
    let Ok(o) = docker(&bin, &["ps", "-q", "--filter", &filter], Duration::from_secs(10)).await else { return };
    let ids: Vec<&str> = o.stdout.split_whitespace().collect();
    if ids.is_empty() {
        return;
    }
    let mut args = vec!["stop", "-t", "3"];
    args.extend(ids);
    let _ = docker(&bin, &args, Duration::from_secs(30)).await;
}

/// エージェントのコンテナ・ボリューム・ネットワーク・専用のイメージを消す(共通のイメージは残す)。見つからなくても、失敗しても続ける。
/// 同期の関数で、別スレッドから呼ぶ(エージェントの削除は同期処理のため)。
pub fn remove_agent_blocking(id: &str) {
    if !is_supported(id) || cfg!(test) {
        return;
    }
    let Some(bin) = find_docker() else { return };
    let run = |args: &[&str]| {
        let _ = std::process::Command::new(&bin)
            .args(args)
            .env("PATH", path_for(&bin))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    };
    let filter = format!("label={LABEL_AGENT}={id}");
    if let Ok(o) = std::process::Command::new(&bin)
        .args(["ps", "-aq", "--filter", &filter])
        .env("PATH", path_for(&bin))
        .stdin(Stdio::null())
        .output()
    {
        for container in String::from_utf8_lossy(&o.stdout).split_whitespace() {
            run(&["rm", "-f", container]);
        }
    }
    run(&["network", "rm", &network_name(id)]);
    run(&["volume", "rm", "-f", &volume_name(id)]);
    // 専用のイメージ(共通のイメージは残す)
    if let Ok(o) = std::process::Command::new(&bin)
        .args(["images", "-q", "--filter", &filter])
        .env("PATH", path_for(&bin))
        .stdin(Stdio::null())
        .output()
    {
        for image in String::from_utf8_lossy(&o.stdout).split_whitespace() {
            run(&["rmi", "-f", image]);
        }
    }
    if let Some(m) = USAGE.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
        m.remove(id);
    }
}

/// エージェントの削除時に、後始末を別スレッドで始める。
pub fn remove_agent(id: &str) {
    if !is_supported(id) || cfg!(test) {
        return;
    }
    let id = id.to_owned();
    std::thread::spawn(move || remove_agent_blocking(&id));
}

#[cfg(test)]
/// `remove_agent_blocking`は単体テストでは何もしないので、同じ処理を実Dockerで行う。
pub fn remove_agent_blocking_for_test(id: &str) {
    let bin = find_docker().unwrap();
    let filter = format!("label={LABEL_AGENT}={id}");
    let ps = std::process::Command::new(&bin).args(["ps", "-aq", "--filter", &filter]).output().unwrap();
    for container in String::from_utf8_lossy(&ps.stdout).split_whitespace() {
        let _ = std::process::Command::new(&bin).args(["rm", "-f", container]).status();
    }
    for args in [vec!["network", "rm", &network_name(id)], vec!["volume", "rm", "-f", &volume_name(id)]] {
        let _ = std::process::Command::new(&bin).args(args).status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0b9c3f5e-1d2a-4c6b-8e7f-123456789abc";

    #[test]
    fn only_work_runs_in_the_sandbox_always_and_invalid_ids_are_rejected() {
        assert!(is_supported(ID) && is_supported(PERSONAL_ID));
        assert!(is_supported(LEADER_ID) && !runs_in_sandbox(LEADER_ID) && !runs_in_sandbox(PERSONAL_ID) && runs_in_sandbox(ID));
        assert!(!is_supported("") && !is_supported("../x"));
        assert!(!is_supported("abc def"));
    }

    #[test]
    fn proxy_is_passed_in_both_cases_and_skips_the_bridge() {
        assert!(proxy_vars(None).is_empty());
        let v = proxy_vars(Some("http://p:8080".into()));
        for k in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"] {
            assert_eq!(v[k], "http://p:8080", "{k}");
        }
        assert_eq!(v["NO_PROXY"], v["no_proxy"]);
        assert!(v["NO_PROXY"].split(',').any(|h| h == "host.docker.internal"));
    }

    #[test]
    fn image_tag_is_shared_and_follows_the_dockerfile_content() {
        let a = image_tag("FROM x\n");
        assert!(a.starts_with("myassistant-sandbox:"));
        // 改行コードと末尾の空白は同じ内容として扱う
        assert_eq!(a, image_tag("FROM x\r\n\n"));
        assert_ne!(a, image_tag("FROM y\n"));
    }

    #[test]
    fn dockerfile_can_be_written_and_is_validated() {
        let dir = tempfile::tempdir().unwrap();
        write_dockerfile(dir.path(), "FROM scratch\n").unwrap();
        assert_eq!(read_dockerfile(dir.path()).unwrap(), "FROM scratch\n");
        assert_eq!(dockerfile_state(dir.path()), DockerfileState::Edited);
        assert!(write_dockerfile(dir.path(), "  \n").is_err());
        assert!(write_dockerfile(dir.path(), "FROM x\0").is_err());
        assert!(write_dockerfile(dir.path(), &"#".repeat(DOCKERFILE_MAX_BYTES + 1)).is_err());
        // 拒否した内容では上書きしない
        assert_eq!(read_dockerfile(dir.path()).unwrap(), "FROM scratch\n");
    }

    #[test]
    fn agent_dockerfile_takes_priority_when_registered() {
        let app = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        write_dockerfile(app.path(), "FROM shared\n").unwrap();
        // 登録がなければ共通
        let spec = resolve_image(app.path(), ID, ws.path()).unwrap();
        assert!(spec.agent.is_none() && spec.tag == image_tag("FROM shared\n") && spec.context == dockerfile_dir(app.path()));
        assert!(write_agent_dockerfile(ws.path(), "FROM x\n").is_err());
        // 登録すると共通のコピーが専用になり、以後は専用が優先される
        register_agent_dockerfile(app.path(), ws.path()).unwrap();
        assert_eq!(read_agent_dockerfile(ws.path()).unwrap(), "FROM shared\n");
        write_agent_dockerfile(ws.path(), "FROM mine\n").unwrap();
        let spec = resolve_image(app.path(), ID, ws.path()).unwrap();
        assert_eq!(spec.agent.as_deref(), Some(ID));
        assert_eq!(spec.tag, agent_image_tag(ID, "FROM mine\n"));
        assert!(spec.tag.starts_with("myassistant-sandbox-0b9c3f5e:"));
        assert_eq!(spec.context, agent_dockerfile_dir(ws.path()));
        // 共通を後から変えても、専用は影響を受けない。登録済みの再登録は上書きしない
        write_dockerfile(app.path(), "FROM shared2\n").unwrap();
        register_agent_dockerfile(app.path(), ws.path()).unwrap();
        assert_eq!(read_agent_dockerfile(ws.path()).unwrap(), "FROM mine\n");
        assert!(write_agent_dockerfile(ws.path(), " ").is_err());
        // 解除すると共通へ戻る
        unregister_agent_dockerfile(ws.path()).unwrap();
        unregister_agent_dockerfile(ws.path()).unwrap();
        assert!(!agent_registered(ws.path()));
        assert!(resolve_image(app.path(), ID, ws.path()).unwrap().agent.is_none());
    }

    #[test]
    fn template_meets_the_container_prerequisites() {
        // Claude Code は Node.js 22 以上が必要(標準は 26 系)。版は固定し、読み取り専用のファイルシステムでは自動更新を止める
        assert!(TEMPLATE.contains("FROM node:26"));
        assert!(TEMPLATE.contains("ARG CLAUDE_CODE_VERSION=") && TEMPLATE.contains("@anthropic-ai/claude-code@${CLAUDE_CODE_VERSION}"));
        assert!(TEMPLATE.contains("DISABLE_AUTOUPDATER=1"));
        assert!(TEMPLATE.contains("mkdir -p /knowledge") && !TEMPLATE.contains("/workspace") && TEMPLATE.contains("USER agent") && TEMPLATE.contains("util-linux"));
    }

    #[test]
    fn template_is_copied_once_and_edits_are_detected() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(dockerfile_state(dir.path()), DockerfileState::Missing);
        ensure_template(dir.path()).unwrap();
        assert_eq!(dockerfile_state(dir.path()), DockerfileState::Template);
        assert!(dockerfile_dir(dir.path()).join(".dockerignore").is_file());
        std::fs::write(dockerfile_path(dir.path()), "FROM scratch\n").unwrap();
        ensure_template(dir.path()).unwrap();
        assert_eq!(dockerfile_state(dir.path()), DockerfileState::Edited);
        assert_eq!(std::fs::read_to_string(dockerfile_path(dir.path())).unwrap(), "FROM scratch\n");
        reset_dockerfile(dir.path()).unwrap();
        assert_eq!(dockerfile_state(dir.path()), DockerfileState::Template);
    }

    #[test]
    fn build_context_never_contains_the_env_file() {
        let dir = tempfile::tempdir().unwrap();
        sandbox_env::ensure_template(dir.path()).unwrap();
        ensure_template(dir.path()).unwrap();
        // 環境変数ファイルはワークスペース直下で、ビルドの文脈(sandbox/)の外にある
        assert!(!dockerfile_dir(dir.path()).join(sandbox_env::FILE_NAME).exists());
        assert_eq!(sandbox_env::path(dir.path()).parent(), dockerfile_dir(dir.path()).parent());
    }

    #[test]
    fn cwd_maps_into_the_knowledge_mount_without_the_data_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        let canon = root.canonicalize().unwrap();
        let m = Mounts { knowledge: root.clone(), data: false };
        assert_eq!(map_cwd(&m, None).unwrap(), (canon.clone(), "/knowledge".into()));
        assert_eq!(map_cwd(&m, Some("a/b")).unwrap().1, "/knowledge/a/b");
        assert_eq!(map_cwd(&m, Some("/knowledge/a")).unwrap().1, "/knowledge/a");
        assert_eq!(map_cwd(&m, Some(canon.join("a").to_str().unwrap())).unwrap().1, "/knowledge/a");
        // マウントしていない場所・親をたどる指定は拒否する
        assert!(map_cwd(&m, Some("/workspace")).is_err());
        assert!(map_cwd(&m, Some("/tmp")).is_err());
        assert!(map_cwd(&m, Some("..")).is_err());
        assert!(map_cwd(&m, Some("/knowledge/../..")).is_err());
        assert!(map_cwd(&m, Some("missing")).is_err());
    }

    #[test]
    fn cwd_defaults_to_the_data_dir_and_reaches_the_whole_knowledge() {
        let dir = tempfile::tempdir().unwrap();
        let knowledge = dir.path().join("knowledge");
        std::fs::create_dir_all(knowledge.join("notes")).unwrap();
        std::fs::create_dir_all(knowledge.join("data/app/lib")).unwrap();
        let m = Mounts { knowledge: knowledge.clone(), data: true };
        assert_eq!(map_cwd(&m, None).unwrap().1, "/knowledge/data");
        assert_eq!(map_cwd(&m, Some("app/lib")).unwrap().1, "/knowledge/data/app/lib");
        assert_eq!(map_cwd(&m, Some("/knowledge/notes")).unwrap().1, "/knowledge/notes");
        assert_eq!(map_cwd(&m, Some("..")).unwrap().1, "/knowledge");
        assert_eq!(map_cwd(&m, Some(knowledge.canonicalize().unwrap().join("data/app").to_str().unwrap())).unwrap().1, "/knowledge/data/app");
        // 以前の /workspace はもうマウントしない
        assert!(map_cwd(&m, Some("/workspace")).is_err());
        assert!(map_cwd(&m, Some("/knowledge/data/../..")).is_err());
        assert_eq!(m.default_dir(), DATA_MOUNT);
        // マウント元が変われば、コンテナのラベルも変わる(作り直しの判定)
        let other = Mounts { knowledge: dir.path().join("other"), data: true };
        assert_ne!(m.label(), other.label());
    }

    #[cfg(unix)]
    #[test]
    fn cwd_through_a_symlink_out_of_the_mounts_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        let m = Mounts { knowledge: root, data: false };
        assert!(map_cwd(&m, Some("link")).is_err());
    }

    #[test]
    fn data_dir_is_a_fixed_folder_inside_the_knowledge() {
        let dir = tempfile::tempdir().unwrap();
        let knowledge = dir.path().join("knowledge");
        let m = mounts_for(&knowledge).unwrap();
        // マウントするのはナレッジだけ(ワークスペースのフォルダにあるキーやDBは見えない)
        assert_eq!(m.knowledge, knowledge);
        assert!(knowledge.join("data").is_dir());
        assert_eq!(m.default_dir(), DATA_MOUNT);
        assert!(!dir.path().join("workspace").exists());
    }

    #[test]
    fn exec_args_carry_names_never_values() {
        let names = vec!["MY_TOKEN".to_owned(), "AWS_SECRET_ACCESS_KEY".to_owned()];
        let a = exec_args("c1", "/workspace/x", "echo hi", "/tmp/ma-1.pid", &names, false);
        let joined = a.join(" ");
        assert!(joined.contains("-e MY_TOKEN") && joined.contains("-e AWS_SECRET_ACCESS_KEY"));
        assert!(!a.iter().any(|s| s.contains("MY_TOKEN=")));
        assert_eq!(a.last().unwrap(), "echo hi");
        assert_eq!(a[1..3], ["-w".to_owned(), "/workspace/x".to_owned()]);
        // 標準入力をつなぐときだけ -i を付ける
        let b = exec_args("c1", "/workspace/x", "cat", "/tmp/ma-1.pid", &[], true);
        assert_eq!(b[1..4], ["-i".to_owned(), "-w".to_owned(), "/workspace/x".to_owned()]);
    }

    #[test]
    fn container_names_are_random_and_do_not_contain_the_agent() {
        let (a, b) = (new_container_name(), new_container_name());
        assert_ne!(a, b);
        for n in [&a, &b] {
            assert!(n.starts_with("myassistant-") && n.len() == "myassistant-".len() + 8, "{n}");
            assert!(n["myassistant-".len()..].bytes().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn container_mounts_only_the_chosen_folders_and_its_own_volume() {
        let mount_args = |args: &[String]| -> Vec<String> {
            args.iter().enumerate().filter(|(i, _)| *i > 0 && args[i - 1] == "--mount").map(|(_, a)| a.clone()).collect()
        };
        // マウントはナレッジと専用ボリュームだけ。作業ディレクトリは /knowledge/data
        let m = Mounts { knowledge: PathBuf::from("/data/knowledge"), data: true };
        let args = run_args(ID, "myassistant-1a2b3c4d", "img:1", &m);
        let mounts = mount_args(&args);
        assert_eq!(mounts.len(), 2);
        assert!(mounts[0].contains("source=/data/knowledge,target=/knowledge"));
        assert!(mounts[1].contains(&format!("source=myassistant-home-{ID},target=/home/agent")));
        assert!(args.windows(2).any(|w| w[0] == "-w" && w[1] == "/knowledge/data"));
        assert!(!args.iter().any(|a| a.contains("target=/workspace")));
        for flag in ["--read-only", "--cap-drop", "no-new-privileges", "--init"] {
            assert!(args.iter().any(|a| a == flag), "{flag}");
        }
        assert!(args.windows(2).any(|w| w[0] == "--name" && w[1] == "myassistant-1a2b3c4d"));
        assert!(args.contains(&format!("myassistant.agent={ID}")));
        assert!(!args.iter().any(|a| a == "-v" || a == "--privileged" || a == "--network=host"));
        assert!(args.windows(2).any(|w| w[0] == "--network" && w[1] == format!("myassistant-net-{ID}")));
    }

    #[test]
    fn masker_hides_values_even_across_chunks() {
        let secret = "sk-very-secret-value".to_owned();
        let mut m = Masker::new(&[secret.clone(), "short".into()]);
        let mut out = String::new();
        for chunk in ["token=sk-very-", "secret-va", "lue and short\n", "tail"] {
            out.push_str(&m.push(chunk));
        }
        out.push_str(&m.flush());
        assert_eq!(out, "token=*** and short\ntail");
        assert!(!out.contains("secret"));
        assert_eq!(Masker::new(&[secret]).mask("a sk-very-secret-value b"), "a *** b");
        // 値が無ければそのまま通す
        let mut none = Masker::new(&[]);
        assert_eq!(none.push("abc"), "abc");
        assert_eq!(none.flush(), "");
    }

    #[test]
    fn masker_releases_complete_lines_without_waiting_for_more_output() {
        let secret = "x".repeat(1_000);
        let mut m = Masker::new(std::slice::from_ref(&secret));
        // 承認要求の行は、続きの出力を待たずに出る(相手は応答を待って止まっている)
        let line = "{\"type\":\"control_request\",\"request_id\":\"r-1\"}\n";
        assert_eq!(m.push(line), line);
        // 値は改行の前後どちらで切れても伏せる
        assert_eq!(m.push(&format!("a {}", &secret[..10])), "");
        assert_eq!(m.push(&format!("{} b\n", &secret[10..])), "a *** b\n");
        // 改行を含む値があれば、従来どおり末尾を保留する
        let mut pem = Masker::new(&["-----BEGIN KEY-----\nabcdefgh\n-----END KEY-----".into()]);
        let out = pem.push("ok\n-----BEGIN KEY-----\n") + &pem.push("abcdefgh\n-----END KEY-----\n") + &pem.flush();
        assert_eq!(out, "ok\n***\n");
    }

    #[test]
    fn masker_keeps_multibyte_text_intact() {
        let mut m = Masker::new(&["パスワード-12345".to_owned()]);
        let mut out = String::new();
        for chunk in ["こんにちは、パスワ", "ード-12345です。さようなら"] {
            out.push_str(&m.push(chunk));
        }
        out.push_str(&m.flush());
        assert_eq!(out, "こんにちは、***です。さようなら");
    }

    #[test]
    fn region_values_are_not_masked() {
        let mut vars = BTreeMap::new();
        vars.insert("AWS_REGION".to_owned(), "ap-northeast-1".to_owned());
        vars.insert("AWS_SECRET_ACCESS_KEY".to_owned(), "abcdefghijkl".to_owned());
        assert_eq!(secret_values(&vars), vec!["abcdefghijkl".to_owned()]);
    }

    #[test]
    fn settings_env_values_are_masked() {
        let mut vars = BTreeMap::new();
        vars.insert("MYASSISTANT_SETTINGS".to_owned(), r#"{"env":{"TOKEN":"abcdefghijkl"},"model":"claude-x"}"#.to_owned());
        assert_eq!(secret_values(&vars), vec!["abcdefghijkl".to_owned()]);
    }

    /// 実Dockerでの通し確認。Dockerの起動とネットワーク(イメージの取得)が必要で、初回は数分かかる。
    /// `cargo test -- --ignored sandbox_end_to_end`で実行する。
    #[tokio::test]
    #[ignore]
    async fn sandbox_end_to_end() {
        let id = "ffffffff-0000-4000-8000-00000000e2e1";
        let other = "ffffffff-0000-4000-8000-00000000e2e2";
        let tmp = tempfile::tempdir().unwrap();
        // 共通のDockerfileの置き場所(アプリデータの代わり)
        let _ = APP_DIR.set(tmp.path().join("app"));
        let mk = |agent: &str| {
            let dir = tmp.path().join(agent);
            let ctx = WorkspaceCtx::open(agent, &dir).unwrap();
            let root = dir.join("knowledge");
            std::fs::create_dir_all(&root).unwrap();
            (ctx, root)
        };
        let (ctx, root) = mk(id);
        let (ctx2, root2) = mk(other);
        std::fs::write(sandbox_env::path(&ctx.dir), "MY_TOKEN=supersecretvalue123\n").unwrap();
        std::fs::write(root.join("hello.txt"), "hi").unwrap();
        std::fs::write(root2.join("other.txt"), "x").unwrap();
        let cancel = CancelToken::default();
        let req = |c: &'static str, ctx: &'static WorkspaceCtx, root: &'static Path| {
            let mounts: &'static Mounts = Box::leak(Box::new(Mounts { knowledge: root.to_owned(), data: false }));
            Request { ctx, mounts, command: c, cwd: KNOWLEDGE_MOUNT, timeout: Duration::from_secs(60) }
        };
        // テストの間だけ生きるよう、リークさせて'staticにする
        let (ctx, ctx2): (&'static WorkspaceCtx, &'static WorkspaceCtx) = (Box::leak(Box::new(ctx)), Box::leak(Box::new(ctx2)));
        let (root, root2): (&'static Path, &'static Path) = (Box::leak(root.into_boxed_path()), Box::leak(root2.into_boxed_path()));

        // 1. 実行できて、環境変数の値は伏せられ、ナレッジが見える
        let out = run(req("echo token=$MY_TOKEN; pwd; cat hello.txt; id -un", ctx, root), &cancel, |_, _| {}).await.unwrap();
        assert_eq!(out.code, Some(0), "{}{}", out.stdout, out.stderr);
        assert!(out.stdout.contains("token=***") && !out.stdout.contains("supersecretvalue123"), "{}", out.stdout);
        assert!(out.stdout.contains("/knowledge") && out.stdout.contains("hi") && out.stdout.contains("agent"));
        // 画面で確認できるコンテナ名は myassistant-<ランダム>(エージェントの名前やIDは含まれない)
        let name = status(id, &app_dir().unwrap(), &ctx.dir).await.container_name.expect("container name");
        assert!(name.starts_with("myassistant-") && !name.contains(id), "{name}");
        // 2. ホームの認証情報・アプリのデータは見えない。他エージェントのナレッジも見えない
        let out = run(req("ls ~/.aws /Users 2>&1; ls /knowledge; env | grep -c MY_TOKEN", ctx, root), &cancel, |_, _| {}).await.unwrap();
        assert!(!out.stdout.contains("other.txt") && !out.stdout.contains("credentials"), "{}", out.stdout);
        let out2 = run(req("ls /knowledge; echo [$MY_TOKEN]", ctx2, root2), &cancel, |_, _| {}).await.unwrap();
        assert!(out2.stdout.contains("other.txt") && !out2.stdout.contains("hello.txt") && out2.stdout.contains("[]"), "{}", out2.stdout);
        // 3. 時間切れでコンテナ内のプロセスも止まる
        let started = Instant::now();
        let mut r = req("sleep 300", ctx, root);
        r.timeout = Duration::from_secs(2);
        let out = run(r, &cancel, |_, _| {}).await.unwrap();
        assert!(out.timed_out && started.elapsed() < Duration::from_secs(30));
        let out = run(req("ps -eo args | grep -c '[s]leep 300'", ctx, root), &cancel, |_, _| {}).await.unwrap();
        assert_eq!(out.stdout.trim(), "0", "sleepが残っています");
        // 4. 後始末
        remove_agent_blocking_for_test(id);
        remove_agent_blocking_for_test(other);
    }

    /// 実Dockerでの、コンテナ用のAWS(中継から一時キーを取る)の通し確認。ビルド済みのイメージが必要(ネットワークは使わない)。
    /// `cargo test -- --ignored sandbox_aws_credentials_end_to_end`で実行する。
    #[tokio::test]
    #[ignore]
    async fn sandbox_aws_credentials_end_to_end() {
        use crate::services::secrets::AwsSlot;
        let id = "ffffffff-0000-4000-8000-00000000e2e3";
        let tmp = tempfile::tempdir().unwrap();
        let _ = APP_DIR.set(tmp.path().join("app"));
        let dir = tmp.path().join(id);
        let ctx = Arc::new(WorkspaceCtx::open(id, &dir).unwrap());
        sandbox_bridge::register_for_test(ctx.clone());
        let ctx: &'static WorkspaceCtx = Box::leak(Box::new(ctx));
        let root = dir.join("knowledge");
        std::fs::create_dir_all(&root).unwrap();
        let mounts: &'static Mounts = Box::leak(Box::new(Mounts { knowledge: root, data: false }));
        start_container(ctx, mounts).await.unwrap();
        let cancel = CancelToken::default();
        let sh = |command: &'static str| run(Request { ctx, mounts, command, cwd: KNOWLEDGE_MOUNT, timeout: Duration::from_secs(60) }, &cancel, |_, _| {});
        // Bedrock 用の設定は、コンテナへは渡らない
        ctx.secrets.set_aws_files(AwsSlot::Bedrock, "", "[b]\naws_access_key_id = AKIABEDROCKDUMMY0001\naws_secret_access_key = bedrock-dummy-secret-0001").unwrap();
        ctx.secrets.set_aws_profile(AwsSlot::Bedrock, "b").unwrap();

        // 1. コンテナ用が未登録なら、AWS の認証情報は無い
        let out = sh("aws configure export-credentials --format env-no-export 2>&1; echo [$AWS_ACCESS_KEY_ID][$AWS_CONFIG_FILE]; env | grep -c AKIA").await.unwrap();
        assert!(out.stdout.contains("[][]") && !out.stdout.contains("AKIA") && out.stdout.trim_end().ends_with('0'), "{}", out.stdout);

        // 2. 登録すると、aws CLI が中継から一時キーを取る。値は環境変数に無く、出力では伏せられる
        ctx.secrets
            .set_aws_files(AwsSlot::Sandbox, "[profile c]\nregion = ap-northeast-1", "[c]\naws_access_key_id = AKIASANDBOXDUMMY0001\naws_secret_access_key = sandbox-dummy-secret-0001")
            .unwrap();
        ctx.secrets.set_aws_profile(AwsSlot::Sandbox, "c").unwrap();
        let out = sh("aws configure export-credentials --format env-no-export; echo region=$AWS_REGION; env | grep -c -e sandbox-dummy -e AKIA; true").await.unwrap();
        assert_eq!(out.code, Some(0), "{}{}", out.stdout, out.stderr);
        assert!(out.stdout.contains("AWS_ACCESS_KEY_ID=***") && out.stdout.contains("AWS_SECRET_ACCESS_KEY=***"), "{}", out.stdout);
        assert!(!out.stdout.contains("sandbox-dummy-secret") && !out.stdout.contains("bedrock-dummy"), "{}", out.stdout);
        assert!(out.stdout.contains("region=ap-northeast-1") && out.stdout.trim_end().ends_with('0'), "{}", out.stdout);

        // 3. 実行が終わると、そのトークンは使えない
        sh("echo \"$MA_AWS_TOKEN\" > /tmp/t; echo \"$MYASSISTANT_AWS_CREDENTIALS_URL\" > /tmp/u").await.unwrap();
        let out = sh(r#"curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $(cat /tmp/t)" "$(cat /tmp/u)"; rm -f /tmp/t /tmp/u"#).await.unwrap();
        assert_eq!(out.stdout.trim(), "403", "{}", out.stdout);
        remove_agent_blocking_for_test(id);
    }
}
