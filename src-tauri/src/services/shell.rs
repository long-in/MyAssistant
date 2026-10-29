//! `run_command`の実行。`/bin/sh -c`で子プロセスを起動し、出力を逐次通知しながらタイムアウトと中断を監視する。
//! 実行してよいかの判定は`command_policy`で済ませてから呼ぶこと。
use std::{path::Path, process::Stdio, time::Duration};

use serde::Serialize;
use tokio::{io::AsyncReadExt, process::Command, sync::mpsc};

use super::cancel::CancelToken;

/// 1ストリームあたりに保持する最大バイト数。超えた分は捨てる。
const CAPTURE_MAX_BYTES: usize = 1_000_000;
/// LLMへ返す出力の先頭と末尾の最大文字数(ストリームごと)。
const RESULT_HEAD_CHARS: usize = 8_000;
const RESULT_TAIL_CHARS: usize = 4_000;
/// 終了後に残りの出力を待つ時間。孫プロセスがパイプを持ち続けても止まらないようにする。
const DRAIN_WAIT: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug, Default)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    /// シグナルで終了した場合や、強制終了した場合は`None`。
    pub code: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
    /// 保持上限を超えて出力を捨てたか。
    pub dropped: bool,
}

#[derive(Default)]
struct Capture {
    bytes: Vec<u8>,
    dropped: bool,
}

impl Capture {
    fn push(&mut self, chunk: &[u8]) {
        let room = CAPTURE_MAX_BYTES.saturating_sub(self.bytes.len());
        if chunk.len() > room {
            self.dropped = true;
        }
        self.bytes.extend_from_slice(&chunk[..chunk.len().min(room)]);
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

/// 子プロセスとその子孫をまとめて終了させる。
fn kill_tree(pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pid.and_then(|p| i32::try_from(p).ok()) {
        // SAFETY: 起動時に`process_group(0)`で作ったプロセスグループへSIGKILLを送るだけ
        unsafe {
            libc::killpg(pid, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    let _ = pid;
}

/// `command`を`cwd`で実行する。出力は`on_chunk`へ逐次渡し、終了後にまとめて返す。
pub async fn run(
    command: &str,
    cwd: &Path,
    timeout: Duration,
    cancel: &CancelToken,
    mut on_chunk: impl FnMut(Stream, &str),
) -> Result<Output, String> {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c")
        .arg(command)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        // 対話やページャで止まらないようにする
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
        .env("PAGER", "cat")
        .env("GIT_PAGER", "cat")
        .env("GIT_TERMINAL_PROMPT", "0");
    // GUIアプリから起動されたときのPATHにはHomebrewが含まれないので補う
    let path = std::env::var("PATH").unwrap_or_default();
    cmd.env("PATH", format!("/opt/homebrew/bin:/usr/local/bin:{path}"));
    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = cmd.spawn().map_err(|e| format!("コマンドを起動できませんでした: {e}"))?;
    let pid = child.id();
    let (tx, mut rx) = mpsc::unbounded_channel::<(Stream, Vec<u8>)>();
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        readers.push(spawn_reader(out, Stream::Stdout, tx.clone()));
    }
    if let Some(err) = child.stderr.take() {
        readers.push(spawn_reader(err, Stream::Stderr, tx.clone()));
    }
    drop(tx);

    let mut stdout = Capture::default();
    let mut stderr = Capture::default();
    let mut result = Output::default();
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);

    let mut handle = |stream: Stream, bytes: Vec<u8>, on_chunk: &mut dyn FnMut(Stream, &str)| {
        on_chunk(stream, &String::from_utf8_lossy(&bytes));
        match stream {
            Stream::Stdout => stdout.push(&bytes),
            Stream::Stderr => stderr.push(&bytes),
        }
    };

    let status = loop {
        tokio::select! {
            Some((stream, bytes)) = rx.recv() => handle(stream, bytes, &mut on_chunk),
            status = child.wait() => break status.ok(),
            () = &mut deadline => {
                result.timed_out = true;
                kill_tree(pid);
                break child.wait().await.ok();
            }
            () = cancel.cancelled() => {
                result.cancelled = true;
                kill_tree(pid);
                break child.wait().await.ok();
            }
        }
    };
    // 終了までに届いた分を取り込む
    while let Ok(Some((stream, bytes))) = tokio::time::timeout(DRAIN_WAIT, rx.recv()).await {
        handle(stream, bytes, &mut on_chunk);
    }
    for r in readers {
        r.abort();
    }
    // 起動したコマンドが背景に残した子孫も止める
    kill_tree(pid);

    result.code = status.and_then(|s| s.code());
    result.dropped = stdout.dropped || stderr.dropped;
    result.stdout = stdout.text();
    result.stderr = stderr.text();
    Ok(result)
}

/// パイプを読み切るまで、届いた分を順にチャネルへ送る。
fn spawn_reader<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
    mut pipe: R,
    stream: Stream,
    tx: mpsc::UnboundedSender<(Stream, Vec<u8>)>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        while let Ok(n) = pipe.read(&mut buf).await {
            if n == 0 || tx.send((stream, buf[..n].to_vec())).is_err() {
                break;
            }
        }
    })
}

fn clip(s: &str) -> String {
    let total = s.chars().count();
    if total <= RESULT_HEAD_CHARS + RESULT_TAIL_CHARS {
        return s.to_owned();
    }
    let head: String = s.chars().take(RESULT_HEAD_CHARS).collect();
    let tail: String = s.chars().skip(total - RESULT_TAIL_CHARS).collect();
    format!("{head}\n…(中略 {}文字)…\n{tail}", total - RESULT_HEAD_CHARS - RESULT_TAIL_CHARS)
}

/// LLMへ返す文面。終了コードと、標準出力・標準エラーの先頭と末尾。
pub fn format_output(out: &Output) -> String {
    let mut s = String::new();
    if out.timed_out {
        s.push_str("(時間切れで強制終了しました)\n");
    } else if out.cancelled {
        s.push_str("(ユーザーが中断しました)\n");
    }
    match out.code {
        Some(c) => s.push_str(&format!("終了コード: {c}\n")),
        None => s.push_str("終了コード: なし(シグナルで終了)\n"),
    }
    if !out.stdout.is_empty() {
        s.push_str(&format!("--- 標準出力 ---\n{}\n", clip(out.stdout.trim_end())));
    }
    if !out.stderr.is_empty() {
        s.push_str(&format!("--- 標準エラー ---\n{}\n", clip(out.stderr.trim_end())));
    }
    if out.stdout.is_empty() && out.stderr.is_empty() {
        s.push_str("(出力なし)\n");
    }
    if out.dropped {
        s.push_str("(出力が大きすぎるため一部を捨てました)\n");
    }
    s.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn sh(command: &str, timeout_ms: u64, cancel: &CancelToken) -> (Output, Vec<(Stream, String)>) {
        let dir = tempfile::tempdir().unwrap();
        let mut chunks = Vec::new();
        let out = run(command, dir.path(), Duration::from_millis(timeout_ms), cancel, |s, t| chunks.push((s, t.to_owned())))
            .await
            .unwrap();
        (out, chunks)
    }

    #[tokio::test]
    async fn captures_output_and_exit_code() {
        let (out, chunks) = sh("echo hello; echo oops >&2; exit 3", 5_000, &CancelToken::default()).await;
        assert_eq!(out.stdout.trim(), "hello");
        assert_eq!(out.stderr.trim(), "oops");
        assert_eq!(out.code, Some(3));
        assert!(chunks.iter().any(|(s, t)| *s == Stream::Stdout && t.contains("hello")));
        let text = format_output(&out);
        assert!(text.contains("終了コード: 3") && text.contains("hello") && text.contains("oops"));
    }

    #[tokio::test]
    async fn runs_in_the_given_directory() {
        let dir = tempfile::tempdir().unwrap();
        let out = run("pwd", dir.path(), Duration::from_secs(5), &CancelToken::default(), |_, _| {}).await.unwrap();
        let expected = dir.path().canonicalize().unwrap();
        assert_eq!(Path::new(out.stdout.trim()).canonicalize().unwrap(), expected);
    }

    #[tokio::test]
    async fn timeout_kills_the_whole_process_tree() {
        let started = std::time::Instant::now();
        // 背景の孫プロセスが標準出力を持ち続けても、時間切れで返ってくる
        let (out, _) = sh("sleep 30 & sleep 30", 300, &CancelToken::default()).await;
        assert!(out.timed_out);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(format_output(&out).contains("時間切れ"));
    }

    #[tokio::test]
    async fn cancel_stops_a_running_command() {
        let cancel = CancelToken::default();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            trigger.cancel();
        });
        let started = std::time::Instant::now();
        let (out, _) = sh("echo start; sleep 30", 20_000, &cancel).await;
        assert!(out.cancelled && !out.timed_out);
        assert!(out.stdout.contains("start"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn stdin_is_closed_and_long_output_is_clipped() {
        let (out, _) = sh("cat; seq 1 20000", 10_000, &CancelToken::default()).await;
        assert_eq!(out.code, Some(0));
        let text = format_output(&out);
        assert!(text.contains("中略") && text.contains("20000") && text.starts_with("終了コード: 0"));
        assert!(text.chars().count() < RESULT_HEAD_CHARS + RESULT_TAIL_CHARS + 200);
    }

    #[tokio::test]
    async fn missing_cwd_is_an_error() {
        let r = run("true", Path::new("/definitely/not/here"), Duration::from_secs(1), &CancelToken::default(), |_, _| {}).await;
        assert!(r.is_err());
    }
}
