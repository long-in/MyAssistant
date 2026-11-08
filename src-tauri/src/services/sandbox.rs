//! エージェントごとのDockerサンドボックス。`run_command`をそのエージェント専用のコンテナの中で実行する。
//!
//! - コンテナ・ボリューム・ネットワークはエージェントごとに別で、見えるのはそのエージェントのナレッジ(`/workspace`)と
//!   専用ボリューム(`/home/agent`)だけ。アプリのデータ・`~/.aws`・他エージェントのフォルダはマウントしない。
//! - イメージはエージェントのワークスペースにある`sandbox/Dockerfile`からビルドする(内容のハッシュをタグにして、変更で自動再ビルド)。
//! - 環境変数(`sandbox.env`とAWSの一時キー)は実行のたびに`docker exec -e NAME`で渡す。値はdocker CLIの環境にだけ置き、引数には出さない。
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
use tauri::{AppHandle, Emitter};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::mpsc,
};

use super::{
    aws,
    cancel::CancelToken,
    sandbox_env,
    shell::{self, Output, Stream},
    workspace::{LEADER_ID, PERSONAL_ID, WorkspaceCtx},
};

/// 標準のDockerfile。初めて有効にしたとき、エージェントのワークスペースへコピーする。
pub const TEMPLATE: &str = include_str!("../../resources/sandbox/Dockerfile");
/// ビルドの進捗を画面へ知らせるイベント名。
pub const BUILD_EVENT: &str = "sandbox-build-progress";

const IMAGE_PREFIX: &str = "myassistant-sandbox-";
const CONTAINER_PREFIX: &str = "myassistant-";
const NETWORK_PREFIX: &str = "myassistant-net-";
const VOLUME_PREFIX: &str = "myassistant-home-";
/// このアプリが作ったリソースの印。値はエージェントID。
const LABEL_AGENT: &str = "myassistant.agent";
const LABEL_IMAGE: &str = "myassistant.image";
const LABEL_ROOT: &str = "myassistant.root";

const DOCKERFILE_DIR: &str = "sandbox";
const DOCKERFILE_NAME: &str = "Dockerfile";
const DOCKERIGNORE: &str = "# ビルドの文脈に含めないものを書きます。\n.dockerignore\nDockerfile\n";
/// コンテナ内のナレッジの場所。
pub const WORKDIR: &str = "/workspace";
/// 使われなくなったコンテナを止めるまでの時間。
const IDLE_STOP: Duration = Duration::from_secs(10 * 60);
const REAP_INTERVAL: Duration = Duration::from_secs(60);
/// docker CLIの短い操作(inspect・rm・startなど)の待ち時間。
const CLI_TIMEOUT: Duration = Duration::from_secs(60);
/// 出力から伏せる値の最小バイト数(短い値を伏せると普通の出力まで壊れる)。
const MIN_MASK_BYTES: usize = 8;
const MASK: &str = "***";
/// ビルド失敗時に返すログの末尾の行数。
const BUILD_TAIL_LINES: usize = 40;

static APP: OnceLock<AppHandle> = OnceLock::new();
static LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
static BUILD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static BUILDING: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static USAGE: Mutex<Option<HashMap<String, Usage>>> = Mutex::new(None);

struct Usage {
    active: usize,
    last: Instant,
}

// ------------------------------------------------------------------ 名前・Dockerfile

/// サンドボックスを使えるのはエージェントだけ(パーソナルとリーダーは対象外)。
pub fn is_agent(id: &str) -> bool {
    id != PERSONAL_ID && id != LEADER_ID && !id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

fn short(id: &str) -> String {
    id.chars().filter(|c| *c != '-').take(8).collect()
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

/// Dockerfileの内容から決めるイメージ名。内容が変われば別のタグになる。
pub fn image_tag(id: &str, dockerfile: &str) -> String {
    let digest = Sha256::digest(normalize(dockerfile).as_bytes());
    let hex: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("{IMAGE_PREFIX}{}:{hex}", short(id))
}

/// 改行コードと末尾の空白だけの違いは同じ内容として扱う。
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_owned()
}

pub fn dockerfile_dir(ws_dir: &Path) -> PathBuf {
    ws_dir.join(DOCKERFILE_DIR)
}

pub fn dockerfile_path(ws_dir: &Path) -> PathBuf {
    dockerfile_dir(ws_dir).join(DOCKERFILE_NAME)
}

/// 標準のDockerfileと`.dockerignore`を、無ければ作る。既にあるファイルは上書きしない。
pub fn ensure_template(ws_dir: &Path) -> Result<PathBuf, String> {
    let dir = dockerfile_dir(ws_dir);
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
pub fn reset_dockerfile(ws_dir: &Path) -> Result<(), String> {
    let dir = dockerfile_dir(ws_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(DOCKERFILE_NAME), TEMPLATE).map_err(|e| format!("Dockerfileを書き込めません: {e}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DockerfileState {
    Missing,
    Template,
    Edited,
}

pub fn dockerfile_state(ws_dir: &Path) -> DockerfileState {
    match std::fs::read_to_string(dockerfile_path(ws_dir)) {
        Err(_) => DockerfileState::Missing,
        Ok(t) if normalize(&t) == normalize(TEMPLATE) => DockerfileState::Template,
        Ok(_) => DockerfileState::Edited,
    }
}

// ------------------------------------------------------------------ 作業ディレクトリ

/// `cwd`をナレッジ配下のホストのパスと、コンテナ内のパスへ解決する。省略時はナレッジの直下。
/// 受け付けるのは、ナレッジ配下の絶対パス、`~/`始まり、`/workspace`始まり、ナレッジからの相対パス。
pub fn map_cwd(root: &Path, arg: Option<&str>) -> Result<(PathBuf, String), String> {
    let outside = || "サンドボックスでは、作業ディレクトリをナレッジの中にしてください。".to_owned();
    let root = root.canonicalize().map_err(|_| "ナレッジのフォルダが見つかりません。".to_owned())?;
    let arg = arg.map(str::trim).filter(|s| !s.is_empty());
    let candidate = match arg {
        None => root.clone(),
        Some(p) => {
            if p == WORKDIR || p.starts_with(&format!("{WORKDIR}/")) {
                root.join(p[WORKDIR.len()..].trim_start_matches('/'))
            } else if let Some(rest) = p.strip_prefix("~/") {
                PathBuf::from(std::env::var_os("HOME").ok_or_else(outside)?).join(rest)
            } else if p == "~" {
                PathBuf::from(std::env::var_os("HOME").ok_or_else(outside)?)
            } else if Path::new(p).is_absolute() {
                PathBuf::from(p)
            } else {
                root.join(p)
            }
        }
    };
    let host = candidate.canonicalize().map_err(|_| "指定された作業ディレクトリが見つかりません。".to_owned())?;
    let rel = host.strip_prefix(&root).map_err(|_| outside())?;
    if !host.is_dir() {
        return Err("作業ディレクトリがディレクトリではありません。".into());
    }
    let mut inner = WORKDIR.to_owned();
    for part in rel.components() {
        inner.push('/');
        inner.push_str(part.as_os_str().to_str().ok_or("作業ディレクトリの名前をUTF-8で扱えません。")?);
    }
    Ok((host, inner))
}

// ------------------------------------------------------------------ 出力の伏せ字

/// 登録済みの値を出力から伏せる。読み取りの境界で値が分かれても伏せられるよう、末尾を次の入力まで保留する。
pub struct Masker {
    secrets: Vec<String>,
    keep: usize,
    carry: String,
}

impl Masker {
    pub fn new(values: &[String]) -> Self {
        let mut secrets: Vec<String> = values.iter().filter(|v| v.len() >= MIN_MASK_BYTES).cloned().collect();
        secrets.sort_by_key(|v| std::cmp::Reverse(v.len()));
        secrets.dedup();
        let keep = secrets.first().map_or(0, |s| s.len().saturating_sub(1));
        Self { secrets, keep, carry: String::new() }
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

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub docker: DockerState,
    /// 現在のDockerfileのイメージがビルド済みか。
    pub image_ready: bool,
    pub container_running: bool,
    /// このエージェントのコンテナの名前(まだ作っていなければ`None`)。
    pub container_name: Option<String>,
    pub building: bool,
}

pub async fn status(id: &str, ws_dir: &Path) -> Status {
    let building = is_building(id);
    let Some(bin) = find_docker() else {
        return Status { docker: DockerState::Missing, image_ready: false, container_running: false, container_name: None, building };
    };
    if !daemon_up(&bin).await {
        return Status { docker: DockerState::Stopped, image_ready: false, container_running: false, container_name: None, building };
    }
    let image_ready = match std::fs::read_to_string(dockerfile_path(ws_dir)) {
        Ok(text) => image_exists(&bin, &image_tag(id, &text)).await,
        Err(_) => false,
    };
    let container = find_container(&bin, id).await;
    Status {
        docker: DockerState::Ready,
        image_ready,
        container_running: container.as_ref().is_some_and(|c| c.running),
        container_name: container.map(|c| c.name),
        building,
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

fn is_building(id: &str) -> bool {
    BUILDING.lock().unwrap_or_else(|p| p.into_inner()).as_ref().is_some_and(|s| s.contains(id))
}

struct BuildingGuard(String);

impl BuildingGuard {
    fn new(id: &str) -> Self {
        BUILDING.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(Default::default).insert(id.to_owned());
        Self(id.to_owned())
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
    workspace_id: &'a str,
    line: &'a str,
}

fn emit_progress(id: &str, line: &str) {
    if let Some(app) = APP.get() {
        let _ = app.emit(BUILD_EVENT, BuildProgress { workspace_id: id, line });
    }
}

/// そのエージェントのDockerfileからイメージをビルドして、タグを返す。ビルド済みなら何もしない。
/// 進捗は`on_line`と画面のイベントへ流す。ビルドは全体で1本ずつ。
pub async fn build_image(
    id: &str,
    ws_dir: &Path,
    force: bool,
    cancel: &CancelToken,
    on_line: &mut impl FnMut(&str),
) -> Result<String, String> {
    if !is_agent(id) {
        return Err("サンドボックスを使えるのはエージェントだけです。".into());
    }
    let bin = require_docker().await?;
    ensure_template(ws_dir)?;
    let text = std::fs::read_to_string(dockerfile_path(ws_dir)).map_err(|e| format!("Dockerfileを読めません: {e}"))?;
    let tag = image_tag(id, &text);
    if !force && image_exists(&bin, &tag).await {
        return Ok(tag);
    }
    let _guard = BuildingGuard::new(id);
    let _serial = BUILD.lock().await;
    // 待っている間に別の実行がビルドを終えていることがある
    if !force && image_exists(&bin, &tag).await {
        return Ok(tag);
    }
    let mut line = |l: &str| {
        emit_progress(id, l);
        on_line(l);
    };
    line(&format!("イメージをビルドします({tag})"));
    let mut cmd = docker_cmd(&bin);
    cmd.args(["build", "--progress=plain", "-t", &tag, "--label", &format!("{LABEL_AGENT}={id}")])
        .arg(dockerfile_dir(ws_dir))
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
    root: String,
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
    let fmt = format!("{{{{.State.Running}}}}|{{{{index .Config.Labels \"{LABEL_IMAGE}\"}}}}|{{{{index .Config.Labels \"{LABEL_ROOT}\"}}}}");
    for name in container_names(bin, id).await {
        let Ok(o) = docker(bin, &["inspect", "-f", &fmt, &name], CLI_TIMEOUT).await else { continue };
        if !o.ok {
            continue;
        }
        let mut parts = o.stdout.splitn(3, '|');
        let (Some(running), Some(image), Some(root)) = (parts.next(), parts.next(), parts.next()) else { continue };
        return Some(Container { name, running: running == "true", image: image.to_owned(), root: root.to_owned() });
    }
    None
}

/// このエージェントのコンテナをすべて消す(ボリュームは残る)。
async fn remove_containers(bin: &Path, id: &str) {
    for name in container_names(bin, id).await {
        let _ = docker(bin, &["rm", "-f", &name], CLI_TIMEOUT).await;
    }
}

/// `docker run`の引数。マウントはナレッジと専用ボリュームだけ。
fn run_args(id: &str, name: &str, tag: &str, root: &Path) -> Vec<String> {
    let root = root.display().to_string();
    let mut a: Vec<String> = ["run", "-d", "--init", "--name"].iter().map(|s| (*s).to_owned()).collect();
    a.push(name.to_owned());
    let mut push = |items: &[&str]| a.extend(items.iter().map(|s| (*s).to_owned()));
    push(&["--label", &format!("{LABEL_AGENT}={id}")]);
    push(&["--label", &format!("{LABEL_IMAGE}={tag}")]);
    push(&["--label", &format!("{LABEL_ROOT}={root}")]);
    push(&["--network", &network_name(id)]);
    push(&["--read-only"]);
    push(&["--tmpfs", "/tmp:rw,nosuid,size=512m"]);
    push(&["--tmpfs", "/var/tmp:rw,nosuid,size=256m"]);
    push(&["--cap-drop", "ALL"]);
    push(&["--security-opt", "no-new-privileges"]);
    push(&["--pids-limit", "512"]);
    push(&["--memory", "2g"]);
    push(&["--cpus", "2"]);
    push(&["--mount", &format!("type=bind,source={root},target={WORKDIR}")]);
    push(&["--mount", &format!("type=volume,source={},target=/home/agent", volume_name(id))]);
    push(&["-u", "agent", "-w", WORKDIR, "-e", "HOME=/home/agent"]);
    push(&[tag, "sleep", "infinity"]);
    a
}

/// コンテナが使える状態(イメージが現在のもの、ナレッジが同じ、起動中)になるようにして、名前を返す。
async fn ensure_container(bin: &Path, id: &str, tag: &str, root: &Path) -> Result<String, String> {
    let lock = ws_lock(id);
    let _held = lock.lock().await;
    let root_s = root.display().to_string();
    if root_s.contains(',') {
        return Err("ナレッジのフォルダ名に「,」を含むため、サンドボックスにマウントできません。".into());
    }
    match find_container(bin, id).await {
        Some(c) if c.image == tag && c.root == root_s => {
            if !c.running {
                let o = docker(bin, &["start", &c.name], CLI_TIMEOUT).await?;
                if !o.ok {
                    return Err(format!("コンテナを起動できませんでした: {}", clip_err(&o.stderr)));
                }
            }
            return Ok(c.name);
        }
        // イメージかナレッジの場所が変わった。ボリュームは残して作り直す
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
    let args = run_args(id, &name, tag, root);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let o = docker(bin, &refs, Duration::from_secs(120)).await?;
    if !o.ok {
        return Err(format!("コンテナを作れませんでした: {}", clip_err(&o.stderr)));
    }
    // Dockerfileを編集した場合の前提(shとsetsid、agentユーザー)を確かめる
    let diag = docker(bin, &["exec", &name, "sh", "-c", "command -v setsid >/dev/null && id -u agent >/dev/null"], CLI_TIMEOUT).await?;
    if !diag.ok {
        let _ = docker(bin, &["rm", "-f", &name], CLI_TIMEOUT).await;
        return Err("イメージが前提を満たしていません。Dockerfileに、ユーザー agent、sh、setsid(util-linux)が必要です。".into());
    }
    prune_old_images(bin, id, tag).await;
    Ok(name)
}

/// 古いタグのイメージを消す(使用中のものは消えない)。
async fn prune_old_images(bin: &Path, id: &str, keep: &str) {
    let filter = format!("label={LABEL_AGENT}={id}");
    let Ok(o) = docker(bin, &["images", "--filter", &filter, "--format", "{{.Repository}}:{{.Tag}}"], CLI_TIMEOUT).await else { return };
    for tag in o.stdout.lines().map(str::trim).filter(|t| !t.is_empty() && *t != keep) {
        let _ = docker(bin, &["rmi", tag], CLI_TIMEOUT).await;
    }
}

/// コンテナを作り直す(ボリュームは残す)。
pub async fn reset_container(id: &str) -> Result<(), String> {
    if !is_agent(id) {
        return Err("サンドボックスを使えるのはエージェントだけです。".into());
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

/// `docker exec`の引数。環境変数は名前だけを渡し、値はdocker CLIの環境から読ませる。
fn exec_args(container: &str, cwd: &str, command: &str, pidfile: &str, env_names: &[String]) -> Vec<String> {
    let mut a: Vec<String> = vec!["exec".into(), "-w".into(), cwd.into()];
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
    /// ナレッジのルート(ホストのパス)。
    pub root: &'a Path,
    pub command: &'a str,
    /// コンテナ内の作業ディレクトリ(`map_cwd`の結果)。
    pub cwd: &'a str,
    pub timeout: Duration,
}

/// 渡す環境変数(`sandbox.env`とAWSの一時キー)。同じ名前は`sandbox.env`が優先。
async fn env_for(ctx: &WorkspaceCtx) -> Result<BTreeMap<String, String>, String> {
    let mut vars = BTreeMap::new();
    if ctx.secrets.has_aws().unwrap_or(false) {
        let pairs = aws::env_pairs(ctx).await.map_err(|e| format!("AWSの認証情報を取得できませんでした: {e}"))?;
        vars.extend(pairs);
    }
    vars.extend(sandbox_env::load(&ctx.dir).map_err(|e| e.to_string())?);
    Ok(vars)
}

/// 伏せる値。リージョンのように秘密でない値は除く。
fn secret_values(vars: &BTreeMap<String, String>) -> Vec<String> {
    vars.iter().filter(|(k, _)| !k.ends_with("REGION")).map(|(_, v)| v.clone()).collect()
}

/// コマンドをそのエージェントのコンテナで実行する。`shell::run`と同じ`Output`を返す。
pub async fn run(
    req: Request<'_>,
    cancel: &CancelToken,
    mut on_chunk: impl FnMut(Stream, &str),
) -> Result<Output, String> {
    let id = req.ctx.id.as_str();
    if !is_agent(id) {
        return Err("サンドボックスを使えるのはエージェントだけです。".into());
    }
    let vars = env_for(req.ctx).await?;
    let bin = require_docker().await?;
    let tag = build_image(id, &req.ctx.dir, false, cancel, &mut |l| on_chunk(Stream::Stderr, &format!("[sandbox] {l}\n"))).await?;
    let container = ensure_container(&bin, id, &tag, req.root).await?;
    let _use = UseGuard::new(id);

    let pidfile = format!("{PIDFILE_DIR}/ma-{}.pid", uuid::Uuid::new_v4().simple());
    let names: Vec<String> = vars.keys().cloned().collect();
    let mut cmd = docker_cmd(&bin);
    cmd.args(exec_args(&container, req.cwd, req.command, &pidfile, &names));
    for (k, v) in &vars {
        cmd.env(k, v);
    }

    let values = secret_values(&vars);
    let (mut out_mask, mut err_mask) = (Masker::new(&values), Masker::new(&values));
    let result = shell::run_prepared(cmd, req.timeout, cancel, |stream, text| {
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

/// 画面へのイベント送出と、アイドルのコンテナの停止を始める。アプリ起動時に1回呼ぶ。
/// 前回のアプリが動かしたままのコンテナは、ここで止める。
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    tauri::async_runtime::spawn(async {
        stop_all().await;
        loop {
            tokio::time::sleep(REAP_INTERVAL).await;
            reap_idle().await;
        }
    });
}

async fn reap_idle() {
    let idle: Vec<String> = {
        let map = USAGE.lock().unwrap_or_else(|p| p.into_inner());
        map.as_ref()
            .map(|m| m.iter().filter(|(_, u)| u.active == 0 && u.last.elapsed() >= IDLE_STOP).map(|(k, _)| k.clone()).collect())
            .unwrap_or_default()
    };
    let Some(bin) = find_docker() else { return };
    for id in idle {
        let lock = ws_lock(&id);
        let _held = lock.lock().await;
        // 待っている間に使われていないか確かめる
        let still_idle = USAGE
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .and_then(|m| m.get(&id))
            .is_some_and(|u| u.active == 0 && u.last.elapsed() >= IDLE_STOP);
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

/// このアプリのコンテナをすべて止める(アプリ終了時・起動時)。
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

/// エージェントのコンテナ・ボリューム・ネットワーク・イメージを消す。見つからなくても、失敗しても続ける。
/// 同期の関数で、別スレッドから呼ぶ(エージェントの削除は同期処理のため)。
pub fn remove_agent_blocking(id: &str) {
    if !is_agent(id) || cfg!(test) {
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
    if !is_agent(id) || cfg!(test) {
        return;
    }
    let id = id.to_owned();
    std::thread::spawn(move || remove_agent_blocking(&id));
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0b9c3f5e-1d2a-4c6b-8e7f-123456789abc";

    #[test]
    fn only_agents_use_the_sandbox() {
        assert!(is_agent(ID));
        assert!(!is_agent(PERSONAL_ID) && !is_agent(LEADER_ID) && !is_agent("") && !is_agent("../x"));
        assert!(!is_agent("abc def"));
    }

    #[test]
    fn image_tag_follows_the_dockerfile_content() {
        let a = image_tag(ID, "FROM x\n");
        assert!(a.starts_with("myassistant-sandbox-0b9c3f5e:"));
        // 改行コードと末尾の空白は同じ内容として扱う
        assert_eq!(a, image_tag(ID, "FROM x\r\n\n"));
        assert_ne!(a, image_tag(ID, "FROM y\n"));
        assert_ne!(a, image_tag("1111111a-1d2a-4c6b-8e7f-123456789abc", "FROM x\n"));
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
    fn cwd_maps_into_the_workspace_mount() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        let canon = root.canonicalize().unwrap();
        assert_eq!(map_cwd(&root, None).unwrap(), (canon.clone(), "/workspace".into()));
        assert_eq!(map_cwd(&root, Some("a/b")).unwrap().1, "/workspace/a/b");
        assert_eq!(map_cwd(&root, Some("/workspace/a")).unwrap().1, "/workspace/a");
        assert_eq!(map_cwd(&root, Some(canon.join("a").to_str().unwrap())).unwrap().1, "/workspace/a");
        // ナレッジの外・親をたどる指定は拒否する
        assert!(map_cwd(&root, Some("/tmp")).is_err());
        assert!(map_cwd(&root, Some("..")).is_err());
        assert!(map_cwd(&root, Some("/workspace/../..")).is_err());
        assert!(map_cwd(&root, Some("missing")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn cwd_through_a_symlink_out_of_the_workspace_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("knowledge");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        assert!(map_cwd(&root, Some("link")).is_err());
    }

    #[test]
    fn exec_args_carry_names_never_values() {
        let names = vec!["MY_TOKEN".to_owned(), "AWS_SECRET_ACCESS_KEY".to_owned()];
        let a = exec_args("c1", "/workspace/x", "echo hi", "/tmp/ma-1.pid", &names);
        let joined = a.join(" ");
        assert!(joined.contains("-e MY_TOKEN") && joined.contains("-e AWS_SECRET_ACCESS_KEY"));
        assert!(!a.iter().any(|s| s.contains("MY_TOKEN=")));
        assert_eq!(a.last().unwrap(), "echo hi");
        assert_eq!(a[1..3], ["-w".to_owned(), "/workspace/x".to_owned()]);
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
    fn container_mounts_only_the_workspace_and_its_own_volume() {
        let args = run_args(ID, "myassistant-1a2b3c4d", "img:1", Path::new("/data/knowledge"));
        let mounts: Vec<&String> = args.iter().enumerate().filter(|(i, _)| *i > 0 && args[i - 1] == "--mount").map(|(_, a)| a).collect();
        assert_eq!(mounts.len(), 2);
        assert!(mounts[0].contains("source=/data/knowledge,target=/workspace"));
        assert!(mounts[1].contains(&format!("source=myassistant-home-{ID},target=/home/agent")));
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

    /// 実Dockerでの通し確認。Dockerの起動とネットワーク(イメージの取得)が必要で、初回は数分かかる。
    /// `cargo test -- --ignored sandbox_end_to_end`で実行する。
    #[tokio::test]
    #[ignore]
    async fn sandbox_end_to_end() {
        let id = "ffffffff-0000-4000-8000-00000000e2e1";
        let other = "ffffffff-0000-4000-8000-00000000e2e2";
        let tmp = tempfile::tempdir().unwrap();
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
        let req = |c: &'static str, ctx: &'static WorkspaceCtx, root: &'static Path| Request { ctx, root, command: c, cwd: "/workspace", timeout: Duration::from_secs(60) };
        // テストの間だけ生きるよう、リークさせて'staticにする
        let (ctx, ctx2): (&'static WorkspaceCtx, &'static WorkspaceCtx) = (Box::leak(Box::new(ctx)), Box::leak(Box::new(ctx2)));
        let (root, root2): (&'static Path, &'static Path) = (Box::leak(root.into_boxed_path()), Box::leak(root2.into_boxed_path()));

        // 1. 実行できて、環境変数の値は伏せられ、ナレッジが見える
        let out = run(req("echo token=$MY_TOKEN; pwd; cat hello.txt; id -un", ctx, root), &cancel, |_, _| {}).await.unwrap();
        assert_eq!(out.code, Some(0), "{}{}", out.stdout, out.stderr);
        assert!(out.stdout.contains("token=***") && !out.stdout.contains("supersecretvalue123"), "{}", out.stdout);
        assert!(out.stdout.contains("/workspace") && out.stdout.contains("hi") && out.stdout.contains("agent"));
        // 画面で確認できるコンテナ名は myassistant-<ランダム>(エージェントの名前やIDは含まれない)
        let name = status(id, &ctx.dir).await.container_name.expect("container name");
        assert!(name.starts_with("myassistant-") && !name.contains(id), "{name}");
        // 2. ホームの認証情報・アプリのデータは見えない。他エージェントのナレッジも見えない
        let out = run(req("ls ~/.aws /Users 2>&1; ls /workspace; env | grep -c MY_TOKEN", ctx, root), &cancel, |_, _| {}).await.unwrap();
        assert!(!out.stdout.contains("other.txt") && !out.stdout.contains("credentials"), "{}", out.stdout);
        let out2 = run(req("ls /workspace; echo [$MY_TOKEN]", ctx2, root2), &cancel, |_, _| {}).await.unwrap();
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

    /// `remove_agent_blocking`は単体テストでは何もしないので、同じ処理を実Dockerで行う。
    fn remove_agent_blocking_for_test(id: &str) {
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
}
