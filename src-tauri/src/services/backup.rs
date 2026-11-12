//! アプリ全体のバックアップとリストア。
//!
//! - バックアップは1つのzipにまとめる。パスワードを指定すると、すべてのファイル(manifestを含む)をAES-256で暗号化する(ファイル名は見える)。中身は`manifest.json`(ファイルごとのSHA-256つき)と、次の3種類。
//!   - `shared/`: エージェント一覧・全体設定・通信のプロキシのパスワード(平文のまま)・削除済みエージェントから残したナレッジ
//!   - `ws/<id>/timeline.db`: `VACUUM INTO`で取った一貫したスナップショット(稼働中のDBを生コピーしない)
//!   - `ws/<id>/`のそのほか: `secrets.json`(APIキー。平文のまま)・`sandbox.env`(サンドボックスの環境変数。平文のまま)・`sandbox/`(共通のDockerfile)・`agent-sandbox/`(エージェント専用のDockerfile)・読み上げ辞書・ナレッジ(保存先がアプリデータの外でも含める)
//! - Whisper・VOICEVOX・固定音声などの再取得できるものは含めない。
//! - リストアは2段階。`prepare`がzipを検証しながら展開し(何も差し替えない)、確認後に再起動して、
//!   次回起動のDBを開く前に`apply_pending`が現在のデータを`restore-backup-<日時>/`へ退避して差し替える。
//!   失敗したら退避から元に戻す。ナレッジは標準の位置へ戻し、ナレッジの保存先の設定は解除する。
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, Write},
    path::{Component, Path, PathBuf},
};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::{AesMode, CompressionMethod, ZipArchive, ZipWriter, result::ZipError, read::ZipFile, write::{FileOptions, SimpleFileOptions}};

use crate::{
    db::schema,
    services::{
        batch::KNOWLEDGE_DIR_KEY,
        global_settings, knowledge,
        pronunciation::DICTIONARY_FILE,
        sandbox_env,
        workspace::{DB_FILE, DIR_NAME as WORK_DIR_NAME, KEPT_DIR_NAME, PERSONAL_ID, REGISTRY_FILE},
    },
    utils::error::AppError,
};

/// アーカイブの形式の版。互換性を壊したら上げる。
pub const FORMAT_VERSION: u32 = 1;
const APP_NAME: &str = "MyAssistant";
const MANIFEST: &str = "manifest.json";
/// `Secrets`の保存ファイル名。
const SECRETS_FILE: &str = "secrets.json";
/// サンドボックスの環境変数ファイル(平文)と、Dockerfileなどを置くフォルダ。エージェントだけが持つ。
const SANDBOX_ENV_FILE: &str = sandbox_env::FILE_NAME;
const SANDBOX_DIR: &str = "sandbox";
const AGENT_SANDBOX_DIR: &str = "agent-sandbox";
/// 全体設定のDB。アーカイブ内では`shared/`の下に置く。以前のバックアップのJSON(`LEGACY_GLOBAL_FILE`)も復元でき、起動時に取り込まれる。
const GLOBAL_FILE: &str = global_settings::DB_FILE;
const LEGACY_GLOBAL_FILE: &str = global_settings::LEGACY_JSON_FILE;
/// 通信のプロキシのパスワードの保管庫(`<アプリデータ>/proxy/secrets.json`)の置き場所と、アーカイブ内の名前。
const PROXY_DIR: &str = "proxy";
const PROXY_SECRETS_FILE: &str = "proxy-secrets.json";
const KNOWLEDGE_DIR: &str = "knowledge";
/// 展開先。リストアの確認が済むまで、ここから外へは何も出さない。
pub const STAGING_DIR: &str = "restore-staging";
/// 次回起動時に差し替える印。
const MARKER: &str = "pending-restore.json";
/// manifestの大きさの上限。
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_WORKSPACES: usize = 64;
/// 展開する合計の大きさの上限。細工したzip(展開すると巨大になるもの)でディスクを埋めさせない。
const MAX_RESTORE_BYTES: u64 = 64 << 30;
/// ZIP64にしないと入らない大きさ。
const LARGE_FILE: u64 = 3 << 30;

#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    app: String,
    format_version: u32,
    created_at: String,
    /// 書き出したときのDBの最新の版。これより新しいものは読み込まない(古いものは起動時のマイグレーションに任せる)。
    schema_version: u32,
    workspaces: Vec<WsEntry>,
    /// アーカイブ内のパス → SHA-256(16進)。
    files: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct WsEntry {
    id: String,
    /// 書き出したときに設定されていたナレッジの保存先(情報用。復元では使わない)。
    knowledge_source: Option<String>,
}

/// バックアップの結果。
#[derive(Debug, Serialize)]
pub struct Summary {
    pub workspaces: usize,
    pub files: usize,
    pub bytes: u64,
}

/// リストアの確認用に、アーカイブの内容を要約する。
#[derive(Debug, Serialize)]
pub struct Preview {
    pub created_at: String,
    pub workspaces: usize,
    pub files: usize,
}

fn io(e: impl std::fmt::Display) -> AppError {
    AppError::Io(e.to_string())
}

fn bad(msg: impl Into<String>) -> AppError {
    AppError::InvalidBackup(msg.into())
}

/// ワークスペースIDとして使える文字だけか(パスの一部になるので厳しく見る)。
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// ワークスペースのフォルダ。パーソナルだけはアプリデータ直下。
fn workspace_dir(root: &Path, id: &str) -> PathBuf {
    if id == PERSONAL_ID { root.to_owned() } else { root.join(WORK_DIR_NAME).join(id) }
}

/// ディスク上のワークスペースのID(DBを持つもの)。パーソナルが先頭。
fn discover(root: &Path) -> Vec<String> {
    let mut ids = vec![PERSONAL_ID.to_owned()];
    let mut agents: Vec<String> = std::fs::read_dir(root.join(WORK_DIR_NAME))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| valid_id(n) && n != KEPT_DIR_NAME && n != PERSONAL_ID && workspace_dir(root, n).join(DB_FILE).is_file())
        .collect();
    agents.sort();
    ids.extend(agents);
    ids
}

/// アーカイブ内の名前が、安全な相対パスで、期待する場所のものか。
fn entry_allowed(name: &str, ids: &[String]) -> bool {
    if name.is_empty() || name.contains(['\\', '\0']) {
        return false;
    }
    let mut parts: Vec<&str> = Vec::new();
    for c in Path::new(name).components() {
        match c {
            Component::Normal(n) => match n.to_str() {
                Some(s) => parts.push(s),
                None => return false,
            },
            _ => return false,
        }
    }
    match parts.as_slice() {
        ["shared", f] => [REGISTRY_FILE, GLOBAL_FILE, LEGACY_GLOBAL_FILE, PROXY_SECRETS_FILE].contains(f),
        ["shared", k, _, ..] => *k == KEPT_DIR_NAME,
        ["ws", id, f] => ids.iter().any(|i| i == id) && [DB_FILE, SECRETS_FILE, DICTIONARY_FILE, SANDBOX_ENV_FILE].contains(f),
        ["ws", id, k, _, ..] => ids.iter().any(|i| i == id) && (*k == KNOWLEDGE_DIR || *k == SANDBOX_DIR || *k == AGENT_SANDBOX_DIR),
        _ => false,
    }
}

// ---------- バックアップ ----------

struct Packer<W: Write + Seek> {
    zip: ZipWriter<W>,
    files: BTreeMap<String, String>,
    bytes: u64,
    /// 指定があれば、すべてのエントリをAES-256で暗号化する。
    password: Option<String>,
    /// 出力ファイル自身を取り込まないための除外。
    exclude: Vec<PathBuf>,
}

/// エントリの書き込み設定。パスワードがあればAES-256。
fn options(password: &Option<String>) -> FileOptions<'_, ()> {
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    match password {
        Some(pw) => opts.with_aes_encryption(AesMode::Aes256, pw),
        None => opts,
    }
}

impl<W: Write + Seek> Packer<W> {
    fn add_file(&mut self, name: &str, path: &Path) -> Result<(), AppError> {
        let mut f = File::open(path).map_err(io)?;
        let len = f.metadata().map_err(io)?.len();
        let opts = options(&self.password).large_file(len > LARGE_FILE);
        self.zip.start_file(name, opts).map_err(io)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = f.read(&mut buf).map_err(io)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            self.zip.write_all(&buf[..n]).map_err(io)?;
            self.bytes += n as u64;
        }
        self.files.insert(name.to_owned(), format!("{:x}", hasher.finalize()));
        Ok(())
    }

    /// `dir`の中身を`prefix/`以下へ入れる。シンボリックリンクと名前がUTF-8でないものは入れない。
    fn add_tree(&mut self, prefix: &str, dir: &Path) -> Result<(), AppError> {
        let Ok(rd) = std::fs::read_dir(dir) else { return Ok(()) };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let path = e.path();
            let Ok(ty) = e.file_type() else { continue };
            let Some(name) = e.file_name().to_str().map(str::to_owned) else {
                log::warn!("backup skipped non-UTF-8 name: {}", path.display());
                continue;
            };
            if self.exclude.contains(&path) {
                continue;
            }
            let entry = format!("{prefix}/{name}");
            if ty.is_dir() {
                self.add_tree(&entry, &path)?;
            } else if ty.is_file() {
                self.add_file(&entry, &path)?;
            }
        }
        Ok(())
    }
}

/// DBの一貫したスナップショットを`dest`へ作る。
fn snapshot_db(src: &Path, dest: &Path) -> Result<(), AppError> {
    let conn = Connection::open_with_flags(src, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    conn.execute("VACUUM INTO ?1", [dest.to_string_lossy()])?;
    Ok(())
}

/// スナップショットに設定されているナレッジの保存先。
fn custom_knowledge_dir(db: &Path) -> Option<String> {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    conn.query_row("SELECT value FROM app_settings WHERE key = ?1", [KNOWLEDGE_DIR_KEY], |r| r.get::<_, String>(0))
        .optional()
        .ok()
        .flatten()
        .filter(|p| !p.is_empty())
}

/// アプリデータ全体を`dest`のzipへ書き出す。
pub fn create(root: &Path, dest: &Path, now: String, password: Option<String>) -> Result<Summary, AppError> {
    let work = root.join(format!("backup-tmp-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&work).map_err(io)?;
    let part = dest.with_extension("zip.part");
    let result = create_inner(root, dest, &part, &work, now, password);
    let _ = std::fs::remove_dir_all(&work);
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

fn create_inner(root: &Path, dest: &Path, part: &Path, work: &Path, now: String, password: Option<String>) -> Result<Summary, AppError> {
    let file = File::create(part).map_err(io)?;
    #[cfg(unix)]
    {
        // APIキーを含むので、自分だけが読めるようにする
        use std::os::unix::fs::PermissionsExt;
        let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    let mut p = Packer {
        zip: ZipWriter::new(file),
        files: BTreeMap::new(),
        bytes: 0,
        password,
        exclude: vec![part.to_owned(), dest.to_owned()],
    };
    let shared = root.join(WORK_DIR_NAME);
    if shared.join(REGISTRY_FILE).is_file() {
        p.add_file(&format!("shared/{REGISTRY_FILE}"), &shared.join(REGISTRY_FILE))?;
    }
    // 全体設定のDBは、稼働中でも一貫したスナップショットを取る
    if shared.join(GLOBAL_FILE).is_file() {
        let snap = work.join("global.db");
        snapshot_db(&shared.join(GLOBAL_FILE), &snap)?;
        p.add_file(&format!("shared/{GLOBAL_FILE}"), &snap)?;
    }
    p.add_tree(&format!("shared/{KEPT_DIR_NAME}"), &shared.join(KEPT_DIR_NAME))?;
    // 通信のプロキシのパスワード(APIキーと同じく平文のまま)
    let proxy_secrets = root.join(PROXY_DIR).join(SECRETS_FILE);
    if proxy_secrets.is_file() {
        p.add_file(&format!("shared/{PROXY_SECRETS_FILE}"), &proxy_secrets)?;
    }

    let root_canon = root.canonicalize().map_err(io)?;
    let mut workspaces = Vec::new();
    for id in discover(root) {
        let dir = workspace_dir(root, &id);
        let snap = work.join(format!("{id}.db"));
        snapshot_db(&dir.join(DB_FILE), &snap)?;
        let custom = custom_knowledge_dir(&snap);
        p.add_file(&format!("ws/{id}/{DB_FILE}"), &snap)?;
        for name in [SECRETS_FILE, DICTIONARY_FILE, SANDBOX_ENV_FILE] {
            if dir.join(name).is_file() {
                p.add_file(&format!("ws/{id}/{name}"), &dir.join(name))?;
            }
        }
        // サンドボックスのDockerfileなど(ビルドの文脈。環境変数ファイルは含まれない)
        p.add_tree(&format!("ws/{id}/{SANDBOX_DIR}"), &dir.join(SANDBOX_DIR))?;
        // エージェント専用のDockerfile(登録があるときだけ)
        p.add_tree(&format!("ws/{id}/{AGENT_SANDBOX_DIR}"), &dir.join(AGENT_SANDBOX_DIR))?;
        let knowledge = custom.as_deref().map(PathBuf::from).unwrap_or_else(|| dir.join(KNOWLEDGE_DIR));
        // アプリデータごと含んでしまう保存先(モデル等まで入る)は取り込まない
        let encloses_root = knowledge.canonicalize().is_ok_and(|k| root_canon.starts_with(&k));
        if knowledge.is_absolute() && !encloses_root {
            // 担当システムのフォルダ(`data/`。ソースコードなど)は含めない(以前の`/workspace`のフォルダも含めていなかった)
            let data = std::fs::read_dir(&knowledge).into_iter().flatten().flatten().map(|e| e.path());
            p.exclude.extend(data.filter(|path| path.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(knowledge::DATA_DIR))));
            p.add_tree(&format!("ws/{id}/{KNOWLEDGE_DIR}"), &knowledge)?;
        } else {
            log::warn!("backup skipped knowledge of {id}: {}", knowledge.display());
        }
        workspaces.push(WsEntry { id, knowledge_source: custom });
    }

    let manifest = Manifest {
        app: APP_NAME.into(),
        format_version: FORMAT_VERSION,
        created_at: now,
        schema_version: schema::LATEST,
        workspaces,
        files: std::mem::take(&mut p.files),
    };
    let summary = Summary { workspaces: manifest.workspaces.len(), files: manifest.files.len(), bytes: p.bytes };
    let json = serde_json::to_vec_pretty(&manifest).map_err(io)?;
    let opts = options(&p.password);
    p.zip.start_file(MANIFEST, opts).map_err(io)?;
    p.zip.write_all(&json).map_err(io)?;
    p.zip.finish().map_err(io)?;
    std::fs::rename(part, dest).map_err(io)?;
    Ok(summary)
}

// ---------- リストア(準備) ----------

/// エントリを開く。暗号化されていて、パスワードが無い・違うときは専用のエラーにする。
fn open_entry<'a>(archive: &'a mut ZipArchive<File>, name: &str, password: Option<&str>) -> Result<ZipFile<'a, File>, AppError> {
    let r = match password {
        Some(pw) => archive.by_name_decrypt(name, pw.as_bytes()),
        None => archive.by_name(name),
    };
    r.map_err(|e| match e {
        ZipError::UnsupportedArchive(m) if m == ZipError::PASSWORD_REQUIRED => AppError::BackupPasswordRequired,
        ZipError::InvalidPassword => AppError::BackupPasswordWrong,
        ZipError::FileNotFound => bad(format!("ファイルがありません: {name}")),
        e => bad(format!("{name} を読めません: {e}")),
    })
}

fn read_manifest(archive: &mut ZipArchive<File>, password: Option<&str>) -> Result<(Manifest, Vec<u8>), AppError> {
    let entry = open_entry(archive, MANIFEST, password)?;
    let mut raw = Vec::new();
    entry.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut raw).map_err(io)?;
    if raw.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(bad("manifest.json が大きすぎます"));
    }
    let m: Manifest = serde_json::from_slice(&raw).map_err(|e| bad(format!("manifest.json を読めません: {e}")))?;
    check_manifest(&m)?;
    Ok((m, raw))
}

fn check_manifest(m: &Manifest) -> Result<(), AppError> {
    if m.app != APP_NAME {
        return Err(bad("このアプリのバックアップではありません"));
    }
    if m.format_version != FORMAT_VERSION {
        return Err(bad(format!("未対応の形式です(版 {})", m.format_version)));
    }
    if m.schema_version > schema::LATEST {
        return Err(bad("より新しいバージョンのアプリで作られたバックアップです。アプリを更新してください"));
    }
    if m.workspaces.is_empty() || m.workspaces.len() > MAX_WORKSPACES {
        return Err(bad("ワークスペースの数が正しくありません"));
    }
    let ids: Vec<String> = m.workspaces.iter().map(|w| w.id.clone()).collect();
    if !ids.iter().all(|i| valid_id(i)) || !ids.iter().any(|i| i == PERSONAL_ID) {
        return Err(bad("ワークスペースの一覧が正しくありません"));
    }
    if let Some(name) = m.files.keys().find(|n| !entry_allowed(n, &ids)) {
        return Err(bad(format!("不正なファイル名です: {name}")));
    }
    for id in &ids {
        if !m.files.contains_key(&format!("ws/{id}/{DB_FILE}")) {
            return Err(bad(format!("{id} のデータベースがありません")));
        }
    }
    Ok(())
}

fn staging(root: &Path) -> PathBuf {
    root.join(STAGING_DIR)
}

/// `src`のzipを検証しながら展開する。現在のデータには触れない。
pub fn prepare(root: &Path, src: &Path, password: Option<&str>) -> Result<Preview, AppError> {
    let stage = staging(root);
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage).map_err(io)?;
    let result = prepare_inner(src, &stage, password);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&stage);
    }
    result
}

fn prepare_inner(src: &Path, stage: &Path, password: Option<&str>) -> Result<Preview, AppError> {
    let mut archive = ZipArchive::new(File::open(src).map_err(io)?).map_err(|e| bad(format!("zip を開けません: {e}")))?;
    let (manifest, raw) = read_manifest(&mut archive, password)?;
    let mut total = 0u64;
    for (name, expected) in &manifest.files {
        let mut entry = open_entry(&mut archive, name, password)?;
        let target = name.split('/').fold(stage.to_owned(), |acc, part| acc.join(part));
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        let mut out = File::create(&target).map_err(io)?;
        #[cfg(unix)]
        if name.ends_with(SECRETS_FILE) || name.ends_with(SANDBOX_ENV_FILE) {
            use std::os::unix::fs::PermissionsExt;
            let _ = out.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = entry.read(&mut buf).map_err(io)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > MAX_RESTORE_BYTES {
                return Err(bad("バックアップが大きすぎます"));
            }
            hasher.update(&buf[..n]);
            out.write_all(&buf[..n]).map_err(io)?;
        }
        if format!("{:x}", hasher.finalize()) != *expected {
            return Err(bad(format!("内容が壊れています: {name}")));
        }
    }
    for w in &manifest.workspaces {
        check_db(&stage.join("ws").join(&w.id).join(DB_FILE))?;
    }
    let global = stage.join("shared").join(GLOBAL_FILE);
    if global.is_file() {
        check_db(&global)?;
    }
    std::fs::write(stage.join(MANIFEST), raw).map_err(io)?;
    Ok(Preview { created_at: manifest.created_at, workspaces: manifest.workspaces.len(), files: manifest.files.len() })
}

/// 展開したDBが壊れておらず、このアプリが扱える版であること。
fn check_db(path: &Path) -> Result<(), AppError> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| bad(format!("データベースを開けません: {e}")))?;
    let ok: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0)).map_err(|e| bad(e.to_string()))?;
    let version: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(|e| bad(e.to_string()))?;
    if ok != "ok" || version > schema::LATEST {
        return Err(bad("データベースが壊れているか、新しすぎます"));
    }
    Ok(())
}

/// 展開済みの内容を、次回起動で差し替える予約をする。
pub fn schedule(root: &Path) -> Result<(), AppError> {
    if !staging(root).join(MANIFEST).is_file() {
        return Err(bad("先にバックアップを選んで検証してください"));
    }
    std::fs::write(root.join(MARKER), "{}").map_err(io)
}

/// 展開済みの内容を捨てる(確認で取り消されたとき)。
pub fn discard(root: &Path) {
    let _ = std::fs::remove_dir_all(staging(root));
    let _ = std::fs::remove_file(root.join(MARKER));
}

// ---------- リストア(差し替え) ----------

/// アプリデータ直下にある、パーソナルのファイル。差し替えで退避するもの。
const PERSONAL_ITEMS: [&str; 6] = [DB_FILE, "timeline.db-wal", "timeline.db-shm", SECRETS_FILE, DICTIONARY_FILE, KNOWLEDGE_DIR];

/// 予約があれば、現在のデータを退避して展開済みの内容へ差し替える。DBを開く前に呼ぶ。
/// 差し替えたら`true`。失敗したら元に戻して`Err`(起動は妨げない)。予約は必ず消す(失敗を繰り返さない)。
pub fn apply_pending(root: &Path) -> Result<bool, AppError> {
    let marker = root.join(MARKER);
    if !marker.exists() {
        return Ok(false);
    }
    let _ = std::fs::remove_file(&marker);
    let stage = staging(root);
    let raw = std::fs::read(stage.join(MANIFEST)).map_err(|e| bad(format!("展開済みのバックアップがありません: {e}")))?;
    let manifest: Manifest = serde_json::from_slice(&raw).map_err(|e| bad(e.to_string()))?;
    check_manifest(&manifest)?;

    let backup = root.join(format!("restore-backup-{}", chrono::Utc::now().format("%Y%m%d%H%M%S")));
    std::fs::create_dir_all(&backup).map_err(io)?;
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut installing = false;
    match swap(root, &stage, &backup, &manifest, &mut moved, &mut installing) {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&stage);
            Ok(true)
        }
        Err(e) => {
            rollback(root, &moved, installing);
            Err(e)
        }
    }
}

fn move_to(from: &Path, to: &Path) -> Result<(), AppError> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    std::fs::rename(from, to).map_err(io)
}

fn exists(p: &Path) -> bool {
    p.symlink_metadata().is_ok()
}

fn swap(
    root: &Path,
    stage: &Path,
    backup: &Path,
    manifest: &Manifest,
    moved: &mut Vec<(PathBuf, PathBuf)>,
    installing: &mut bool,
) -> Result<(), AppError> {
    // 1. いまのデータを退避する
    let mut retire: Vec<PathBuf> = PERSONAL_ITEMS.iter().map(|n| root.join(n)).collect();
    retire.extend(
        std::fs::read_dir(root)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("timeline.db.bak-"))
            .map(|e| e.path()),
    );
    retire.push(root.join(WORK_DIR_NAME));
    // プロキシのパスワードは、バックアップに含まれているときだけ差し替える(古いバックアップなら、いまのものを残す)
    if exists(&stage.join("shared").join(PROXY_SECRETS_FILE)) {
        retire.push(root.join(PROXY_DIR));
    }
    for from in retire.into_iter().filter(|p| exists(p)) {
        let to = backup.join(from.file_name().unwrap_or_default());
        move_to(&from, &to)?;
        moved.push((from, to));
    }

    // 2. 展開済みの内容を置く
    *installing = true;
    let shared = root.join(WORK_DIR_NAME);
    std::fs::create_dir_all(&shared).map_err(io)?;
    for name in [REGISTRY_FILE, GLOBAL_FILE, LEGACY_GLOBAL_FILE, KEPT_DIR_NAME] {
        let from = stage.join("shared").join(name);
        if exists(&from) {
            move_to(&from, &shared.join(name))?;
        }
    }
    let from = stage.join("shared").join(PROXY_SECRETS_FILE);
    if exists(&from) {
        move_to(&from, &root.join(PROXY_DIR).join(SECRETS_FILE))?;
    }
    for w in &manifest.workspaces {
        let from_dir = stage.join("ws").join(&w.id);
        let dir = workspace_dir(root, &w.id);
        std::fs::create_dir_all(&dir).map_err(io)?;
        for name in [DB_FILE, SECRETS_FILE, DICTIONARY_FILE, SANDBOX_ENV_FILE, SANDBOX_DIR, AGENT_SANDBOX_DIR, KNOWLEDGE_DIR] {
            let from = from_dir.join(name);
            if exists(&from) {
                move_to(&from, &dir.join(name))?;
            }
        }
        if !dir.join(DB_FILE).is_file() {
            return Err(bad(format!("{} のデータベースを置けませんでした", w.id)));
        }
        clear_knowledge_dir(&dir.join(DB_FILE));
    }
    Ok(())
}

/// ナレッジは標準の位置へ戻したので、保存先の設定は解除する。
fn clear_knowledge_dir(db: &Path) {
    let r = Connection::open(db).and_then(|c| c.execute("DELETE FROM app_settings WHERE key = ?1", [KNOWLEDGE_DIR_KEY]));
    if let Err(e) = r {
        log::warn!("knowledge location not cleared for {}: {e}", db.display());
    }
}

/// 置いたものを消し、退避したものを元に戻す。
fn rollback(root: &Path, moved: &[(PathBuf, PathBuf)], installing: bool) {
    if installing {
        for name in PERSONAL_ITEMS {
            let p = root.join(name);
            if p.is_dir() { let _ = std::fs::remove_dir_all(&p); } else { let _ = std::fs::remove_file(&p); }
        }
        let _ = std::fs::remove_dir_all(root.join(WORK_DIR_NAME));
        // プロキシのパスワードは、差し替えるために退避したときだけ、置いたものを消す
        let proxy = root.join(PROXY_DIR);
        if moved.iter().any(|(from, _)| *from == proxy) {
            let _ = std::fs::remove_dir_all(&proxy);
        }
    }
    for (from, to) in moved.iter().rev() {
        if let Err(e) = std::fs::rename(to, from) {
            log::error!("restore rollback failed for {}: {e}", from.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db::Db, services::global_settings::SettingsStore};

    /// 実データに近い構成を作る: パーソナル(外部ナレッジ)とエージェント1つ。
    fn sample(root: &Path, external: &Path, marker: &str) {
        std::fs::create_dir_all(external.join("Dev")).unwrap();
        std::fs::write(external.join("Dev/メモ 1.md"), marker).unwrap();
        let db = Db::open(&root.join(DB_FILE)).unwrap();
        db.set_setting(KNOWLEDGE_DIR_KEY, &external.to_string_lossy()).unwrap();
        db.set_setting("marker", marker).unwrap();
        drop(db);
        std::fs::write(root.join(SECRETS_FILE), format!("{{\"k\":\"{marker}\"}}")).unwrap();
        std::fs::create_dir_all(root.join(PROXY_DIR)).unwrap();
        std::fs::write(root.join(PROXY_DIR).join(SECRETS_FILE), format!("{{\"proxy_password:p\":\"{marker}\"}}")).unwrap();
        let agent = root.join(WORK_DIR_NAME).join("agent-a");
        std::fs::create_dir_all(agent.join(KNOWLEDGE_DIR)).unwrap();
        std::fs::write(agent.join(KNOWLEDGE_DIR).join("a.md"), marker).unwrap();
        // 担当システムのフォルダ(ソースコードなど)はバックアップに含めない
        std::fs::create_dir_all(agent.join(KNOWLEDGE_DIR).join("data/src")).unwrap();
        std::fs::write(agent.join(KNOWLEDGE_DIR).join("data/src/main.rs"), marker).unwrap();
        std::fs::create_dir_all(external.join("Data")).unwrap();
        std::fs::write(external.join("Data/README.md"), marker).unwrap();
        std::fs::write(agent.join(SANDBOX_ENV_FILE), format!("TOKEN={marker}\n")).unwrap();
        std::fs::create_dir_all(agent.join(SANDBOX_DIR)).unwrap();
        std::fs::write(agent.join(SANDBOX_DIR).join("Dockerfile"), format!("FROM {marker}\n")).unwrap();
        let sdb = Db::open(&agent.join(DB_FILE)).unwrap();
        sdb.set_setting("marker", marker).unwrap();
        drop(sdb);
        std::fs::write(root.join(WORK_DIR_NAME).join(REGISTRY_FILE), format!("[\"{marker}\"]")).unwrap();
        global_settings::GlobalStore::open(root).unwrap().set_setting(global_settings::LANGUAGE_KEY, if marker == "new" { "en" } else { "ja" }).unwrap();
    }

    fn language_of(root: &Path) -> Option<String> {
        global_settings::GlobalStore::open(root).unwrap().get_setting(global_settings::LANGUAGE_KEY).unwrap()
    }

    fn marker_of(root: &Path, id: &str) -> Option<String> {
        Db::open(&workspace_dir(root, id).join(DB_FILE)).unwrap().get_setting("marker").unwrap()
    }

    #[test]
    fn roundtrip_restores_everything_and_keeps_old_data() {
        let d = tempfile::tempdir().unwrap();
        let (src, ext, dst) = (d.path().join("src"), d.path().join("ext"), d.path().join("dst"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        sample(&src, &ext, "new");
        // models は含めない
        std::fs::create_dir_all(src.join("models")).unwrap();
        std::fs::write(src.join("models/big.bin"), "x").unwrap();
        let zip = d.path().join("b.zip");
        let s = create(&src, &zip, "2026-01-01T00:00:00Z".into(), None).unwrap();
        assert_eq!(s.workspaces, 2);

        sample(&dst, &d.path().join("ext2"), "old");
        std::fs::create_dir_all(dst.join("models")).unwrap();
        std::fs::write(dst.join("models/keep.bin"), "k").unwrap();
        let p = prepare(&dst, &zip, None).unwrap();
        assert_eq!(p.workspaces, 2);
        assert_eq!(std::fs::read_to_string(dst.join(WORK_DIR_NAME).join(REGISTRY_FILE)).unwrap(), "[\"old\"]");
        schedule(&dst).unwrap();
        assert!(apply_pending(&dst).unwrap());

        assert_eq!(marker_of(&dst, PERSONAL_ID).as_deref(), Some("new"));
        assert_eq!(marker_of(&dst, "agent-a").as_deref(), Some("new"));
        // 全体設定のDBも戻る
        assert_eq!(language_of(&dst).as_deref(), Some("en"));
        // 外部のナレッジは標準の位置へ戻り、保存先の設定は解除される
        assert_eq!(std::fs::read_to_string(dst.join("knowledge/Dev/メモ 1.md")).unwrap(), "new");
        let db = Db::open(&dst.join(DB_FILE)).unwrap();
        assert_eq!(db.get_setting(KNOWLEDGE_DIR_KEY).unwrap(), None);
        assert_eq!(std::fs::read_to_string(dst.join(SECRETS_FILE)).unwrap(), "{\"k\":\"new\"}");
        assert_eq!(std::fs::read_to_string(dst.join("work/agent-a/knowledge/a.md")).unwrap(), "new");
        // プロキシのパスワードも戻り、古いものは退避に残る
        assert_eq!(std::fs::read_to_string(dst.join(PROXY_DIR).join(SECRETS_FILE)).unwrap(), "{\"proxy_password:p\":\"new\"}");
        // サンドボックスの環境変数とDockerfileも戻る
        assert_eq!(std::fs::read_to_string(dst.join("work/agent-a/sandbox.env")).unwrap(), "TOKEN=new\n");
        assert_eq!(std::fs::read_to_string(dst.join("work/agent-a/sandbox/Dockerfile")).unwrap(), "FROM new\n");
        assert_eq!(std::fs::read_to_string(dst.join(WORK_DIR_NAME).join(REGISTRY_FILE)).unwrap(), "[\"new\"]");
        // モデルは触らない。展開先と予約は片付く
        assert!(dst.join("models/keep.bin").is_file());
        assert!(!dst.join(STAGING_DIR).exists() && !dst.join(MARKER).exists());
        // 古いデータは退避に残る
        let old = std::fs::read_dir(&dst).unwrap().flatten().find(|e| e.file_name().to_string_lossy().starts_with("restore-backup-")).unwrap();
        assert_eq!(std::fs::read_to_string(old.path().join(SECRETS_FILE)).unwrap(), "{\"k\":\"old\"}");
        assert!(old.path().join("work/agent-a/timeline.db").is_file());
        assert_eq!(std::fs::read_to_string(old.path().join(PROXY_DIR).join(SECRETS_FILE)).unwrap(), "{\"proxy_password:p\":\"old\"}");
        // 取り込まれたのはモデル以外
        let names: Vec<_> = ZipArchive::new(File::open(&zip).unwrap()).unwrap().file_names().map(str::to_owned).collect();
        assert!(names.iter().all(|n| !n.contains("models")));
        assert!(names.iter().all(|n| !n.contains("/knowledge/data/") && !n.contains("/knowledge/Data/")), "{names:?}");
        assert!(names.iter().any(|n| n.ends_with("/knowledge/a.md")));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&zip).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(dst.join(SECRETS_FILE)).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(dst.join("work/agent-a/sandbox.env")).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    /// manifestを書き換えたzipを作る。
    fn forge(path: &Path, manifest: &Manifest, files: &[(&str, &[u8])]) {
        let mut z = ZipWriter::new(File::create(path).unwrap());
        for (n, b) in files {
            z.start_file(*n, SimpleFileOptions::default()).unwrap();
            z.write_all(b).unwrap();
        }
        z.start_file(MANIFEST, SimpleFileOptions::default()).unwrap();
        z.write_all(&serde_json::to_vec(manifest).unwrap()).unwrap();
        z.finish().unwrap();
    }

    fn manifest(files: &[(&str, &str)], schema_version: u32) -> Manifest {
        Manifest {
            app: APP_NAME.into(),
            format_version: FORMAT_VERSION,
            created_at: "t".into(),
            schema_version,
            workspaces: vec![WsEntry { id: PERSONAL_ID.into(), knowledge_source: None }],
            files: files.iter().map(|(a, b)| ((*a).into(), (*b).into())).collect(),
        }
    }

    #[test]
    fn rejects_path_traversal_and_foreign_entries() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        for evil in ["../evil", "ws/personal/../../evil", "/abs", "ws/other/timeline.db", "etc/passwd", "ws/personal/x.txt"] {
            let zip = d.path().join("e.zip");
            let m = manifest(&[("ws/personal/timeline.db", "x"), (evil, "x")], schema::LATEST);
            forge(&zip, &m, &[("ws/personal/timeline.db", b"x"), (evil, b"x")]);
            assert!(matches!(prepare(&root, &zip, None), Err(AppError::InvalidBackup(_))), "{evil}");
            assert!(!d.path().join("evil").exists());
            assert!(!root.join(STAGING_DIR).exists());
        }
    }

    #[test]
    fn rejects_checksum_mismatch_and_newer_schema() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        let zip = d.path().join("e.zip");
        forge(&zip, &manifest(&[("ws/personal/timeline.db", "deadbeef")], schema::LATEST), &[("ws/personal/timeline.db", b"x")]);
        assert!(matches!(prepare(&root, &zip, None), Err(AppError::InvalidBackup(_))));
        forge(&zip, &manifest(&[("ws/personal/timeline.db", "deadbeef")], schema::LATEST + 1), &[("ws/personal/timeline.db", b"x")]);
        assert!(matches!(prepare(&root, &zip, None), Err(AppError::InvalidBackup(_))));
        assert!(!root.join(STAGING_DIR).exists());
    }

    #[test]
    fn failed_swap_rolls_back_to_old_data() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        sample(&root, &d.path().join("ext"), "old");
        // 展開済みだが、パーソナルのDBが無い(差し替え中に失敗する)
        let stage = staging(&root);
        std::fs::create_dir_all(stage.join("ws/personal")).unwrap();
        let m = manifest(&[("ws/personal/timeline.db", "x")], schema::LATEST);
        std::fs::write(stage.join(MANIFEST), serde_json::to_vec(&m).unwrap()).unwrap();
        schedule(&root).unwrap();
        assert!(apply_pending(&root).is_err());
        assert_eq!(marker_of(&root, PERSONAL_ID).as_deref(), Some("old"));
        assert_eq!(marker_of(&root, "agent-a").as_deref(), Some("old"));
        assert_eq!(std::fs::read_to_string(root.join(SECRETS_FILE)).unwrap(), "{\"k\":\"old\"}");
        assert!(!root.join(MARKER).exists());
    }

    #[test]
    fn encrypted_backup_needs_the_right_password() {
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (d.path().join("src"), d.path().join("dst"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        sample(&src, &d.path().join("ext"), "secret-data");
        let zip = d.path().join("b.zip");
        create(&src, &zip, "t".into(), Some("pw-1234".into())).unwrap();
        // 平文の内容がzipの中に見えない
        let raw = std::fs::read(&zip).unwrap();
        assert!(!raw.windows(11).any(|w| w == b"secret-data"));
        assert!(matches!(prepare(&dst, &zip, None), Err(AppError::BackupPasswordRequired)));
        assert!(matches!(prepare(&dst, &zip, Some("wrong")), Err(AppError::BackupPasswordWrong)));
        assert!(!dst.join(STAGING_DIR).exists());
        prepare(&dst, &zip, Some("pw-1234")).unwrap();
        schedule(&dst).unwrap();
        assert!(apply_pending(&dst).unwrap());
        assert_eq!(marker_of(&dst, PERSONAL_ID).as_deref(), Some("secret-data"));
        assert_eq!(std::fs::read_to_string(dst.join(SECRETS_FILE)).unwrap(), "{\"k\":\"secret-data\"}");
    }

    #[test]
    fn password_is_ignored_for_plain_backup() {
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (d.path().join("src"), d.path().join("dst"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        sample(&src, &d.path().join("ext"), "x");
        let zip = d.path().join("b.zip");
        create(&src, &zip, "t".into(), None).unwrap();
        assert!(prepare(&dst, &zip, Some("anything")).is_ok());
    }

    #[test]
    fn no_marker_means_nothing_happens() {
        let d = tempfile::tempdir().unwrap();
        assert!(!apply_pending(d.path()).unwrap());
    }
}
