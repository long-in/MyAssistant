//! 作業の依頼(ファイル操作・コマンド実行)の実行状態。ワークスペースごとに持つ。
//!
//! ワークスペースを切り替えても依頼は止めない。依頼の進捗・承認待ち・結果をここに記録しておき、
//! そのワークスペースへ戻ったとき(`snapshot_and_attach`)に、画面が受け取って続きから再開する。
//! 記録はメモリ上のみで、アプリを再起動すると消える(会話の本文はDBに残る)。
use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

use serde::Serialize;
use serde_json::Value;
use tauri::ipc::Channel;
use tokio::sync::oneshot;

use super::{
    tool_loop::{TaskEvent, Approval},
    cancel::CancelToken,
};

/// 記録しておく進捗イベントの上限。超えたら古いものから捨てる。
const MAX_RECORDED_EVENTS: usize = 500;

/// 画面へイベントを流す接続。`resumed`は、切り替えから戻った画面が接続し直したもの。
struct Sink {
    channel: Channel<TaskEvent>,
    resumed: bool,
}

/// 完了した依頼の結果(画面が見ていない間に終わったとき)。`reply`か`error`のどちらか。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Finished {
    pub reply: Option<Value>,
    pub error: Option<Value>,
}

/// 依頼の状態(リーダーの台帳が、中身を読まずに進捗だけを知るためのもの)。
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// 動いていない(まだ始まっていない、または、記録が無い)。
    Idle,
    /// 実行中。
    Running,
    /// 承認の回答を待っている。
    AwaitingApproval,
    /// 直近の依頼が完了した。
    Done,
    /// 直近の依頼が失敗した。
    Failed,
}

#[derive(Default)]
struct Inner {
    running: bool,
    /// 直近に終わった依頼の結果(完了/失敗)。画面が見ていても残し、`begin`で消す。結果の本文は持たない。
    outcome: Option<RunStatus>,
    session_id: String,
    user_text: String,
    events: Vec<TaskEvent>,
    /// 回答待ちの承認(あれば1件)。
    pending: Option<Approval>,
    finished: Option<Finished>,
    sink: Option<Sink>,
}

/// 戻ってきた画面へ渡す、依頼の状況。
#[derive(Debug, Serialize)]
pub struct RunSnapshot {
    pub running: bool,
    pub session_id: String,
    pub user_text: String,
    pub events: Vec<TaskEvent>,
    pub pending: Option<Approval>,
    pub finished: Option<Finished>,
}

/// 承認の応答口、実行中の依頼の中断トークン、依頼の記録。
#[derive(Default)]
pub struct TaskState {
    /// 承認要求の`id`ごとに、画面からの応答を待つ。
    approvals: Mutex<HashMap<String, oneshot::Sender<bool>>>,
    /// 実行中の依頼。同時には1つだけ。
    cancel: Mutex<Option<CancelToken>>,
    inner: Mutex<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl TaskState {
    // ---- 承認 ----

    /// 承認の応答を待つ口を作る。
    pub fn register_approval(&self, id: &str) -> oneshot::Receiver<bool> {
        let (tx, rx) = oneshot::channel();
        lock(&self.approvals).insert(id.to_owned(), tx);
        rx
    }

    /// 承認への応答。待っていない`id`は無視する。
    pub fn answer(&self, id: &str, approved: bool) {
        if let Some(tx) = lock(&self.approvals).remove(id) {
            let _ = tx.send(approved);
        }
        self.clear_pending(id);
    }

    /// 承認待ちを終える(応答・時間切れ・中断のどれでも)。
    pub fn finish_approval(&self, id: &str) {
        lock(&self.approvals).remove(id);
        self.clear_pending(id);
    }

    fn clear_pending(&self, id: &str) {
        let mut inner = lock(&self.inner);
        if inner.pending.as_ref().is_some_and(|a| a.id == id) {
            inner.pending = None;
        }
    }

    // ---- 中断 ----

    pub fn set_cancel(&self, token: CancelToken) {
        *lock(&self.cancel) = Some(token);
    }

    pub fn clear_cancel(&self) {
        lock(&self.cancel).take();
    }

    /// 実行中の依頼を中断する。動いているコマンドは強制終了し、承認待ちは拒否として扱う。実行中でなければ何もしない。
    pub fn cancel(&self) {
        if let Some(token) = lock(&self.cancel).as_ref() {
            token.cancel();
        }
    }

    /// 依頼を中断し、承認待ちも拒否にする(エージェントの削除時)。
    pub fn abort(&self) {
        self.cancel();
        lock(&self.approvals).clear(); // 送り口を捨てると、待っている側は拒否として扱う
        lock(&self.inner).pending = None;
    }

    // ---- 記録 ----

    /// 依頼の開始。前回の記録は捨て、`channel`(始めた画面)へ進捗を流す。
    pub fn begin(&self, session_id: &str, user_text: &str, channel: Channel<TaskEvent>) {
        *lock(&self.inner) = Inner {
            running: true,
            session_id: session_id.to_owned(),
            user_text: user_text.to_owned(),
            sink: Some(Sink { channel, resumed: false }),
            ..Inner::default()
        };
    }

    /// 画面を介さない依頼(リーダーからの配布)の開始。実行中なら始めずに`false`を返す(確認と開始は同じロックの中)。
    /// 接続先が無いので、終わった結果は取っておかれ、そのエージェントを開いたときに受け取れる。
    pub fn try_begin_detached(&self, session_id: &str, user_text: &str) -> bool {
        let mut inner = lock(&self.inner);
        if inner.running {
            return false;
        }
        *inner = Inner {
            running: true,
            session_id: session_id.to_owned(),
            user_text: user_text.to_owned(),
            ..Inner::default()
        };
        true
    }

    /// 直近の依頼のセッションID(依頼が無ければ空)。リーダーの台帳の行が、いまの状態と同じ依頼のものかを確かめるのに使う。
    pub fn session_id(&self) -> String {
        lock(&self.inner).session_id.clone()
    }

    /// 依頼の状態。記録を消費せず、本文も返さない。
    pub fn status(&self) -> RunStatus {
        let inner = lock(&self.inner);
        if inner.pending.is_some() {
            RunStatus::AwaitingApproval
        } else if inner.running {
            RunStatus::Running
        } else {
            inner.outcome.unwrap_or(RunStatus::Idle)
        }
    }

    /// 進捗を記録し、画面が接続していれば流す。
    pub fn emit(&self, event: TaskEvent) {
        let mut inner = lock(&self.inner);
        if let TaskEvent::Approval(a) = &event {
            inner.pending = Some(a.clone());
        }
        if inner.events.len() >= MAX_RECORDED_EVENTS {
            inner.events.remove(0);
        }
        inner.events.push(event.clone());
        if let Some(sink) = &inner.sink {
            let _ = sink.channel.send(event);
        }
    }

    /// 依頼の終わり。
    /// - 始めた画面がまだ見ている: その画面は、コマンドの戻り値で結果を受け取るので、ここでは何もしない。
    /// - 切り替えから戻った画面が接続している: イベントで結果を渡す。
    /// - 誰も見ていない: 結果を取っておき、戻ってきた画面に渡す。
    pub fn finish(&self, result: Result<Value, Value>) {
        let finished = match result {
            Ok(reply) => Finished { reply: Some(reply), error: None },
            Err(error) => Finished { reply: None, error: Some(error) },
        };
        let mut inner = lock(&self.inner);
        inner.running = false;
        inner.pending = None;
        inner.outcome = Some(if finished.error.is_some() { RunStatus::Failed } else { RunStatus::Done });
        match inner.sink.take() {
            Some(Sink { resumed: true, channel }) => {
                let _ = channel.send(TaskEvent::Finished { reply: finished.reply, error: finished.error });
            }
            Some(_) => {}
            None => inner.finished = Some(finished),
        }
    }

    /// いま記録している進捗(実行ログの保存に使う)。
    pub fn events(&self) -> Vec<TaskEvent> {
        lock(&self.inner).events.clone()
    }

    /// 回答待ちの承認(あれば)。
    pub fn pending_approval(&self) -> Option<Approval> {
        lock(&self.inner).pending.clone()
    }

    /// 画面が見なくなった(ワークスペースを切り替えた)。以降の進捗は記録だけにする。
    pub fn detach(&self) {
        lock(&self.inner).sink = None;
    }

    /// 戻ってきた画面へ、依頼の状況を渡して接続し直す。実行中なら、以降の進捗とその結果をこの接続へ流す。
    /// 取っておいた結果は、渡したら捨てる。
    pub fn snapshot_and_attach(&self, channel: Channel<TaskEvent>) -> RunSnapshot {
        let mut inner = lock(&self.inner);
        let snapshot = RunSnapshot {
            running: inner.running,
            session_id: inner.session_id.clone(),
            user_text: inner.user_text.clone(),
            events: inner.events.clone(),
            pending: inner.pending.clone(),
            finished: inner.finished.take(),
        };
        if inner.running {
            inner.sink = Some(Sink { channel, resumed: true });
        }
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use crate::services::tool_loop::ApprovalKind;

    #[test]
    fn status_follows_the_run_without_consuming_it() {
        let a = TaskState::default();
        assert_eq!(a.status(), RunStatus::Idle);
        assert!(a.try_begin_detached("s1", "調べて"));
        assert!(!a.try_begin_detached("s2", "別件"), "実行中は二重に始めない");
        assert_eq!((a.status(), a.session_id().as_str()), (RunStatus::Running, "s1"));
        a.finish(Ok(Value::Null));
        // 何度読んでも変わらず、結果は消費されない。誰も見ていないので、結果は取っておかれる
        assert_eq!((a.status(), a.status()), (RunStatus::Done, RunStatus::Done));
        assert!(a.snapshot_and_attach(counting_channel().0).finished.is_some());
        assert_eq!(a.status(), RunStatus::Done, "結果を受け取った後も、状態は残る");
        assert!(a.try_begin_detached("s3", "次"));
        assert_eq!(a.status(), RunStatus::Running, "新しい依頼で前回の結果は消える");
        a.finish(Err(serde_json::json!({ "code": "timeout" })));
        assert_eq!(a.status(), RunStatus::Failed);
    }

    /// 受け取ったメッセージを数えるチャンネル。
    fn counting_channel() -> (Channel<TaskEvent>, Arc<AtomicUsize>) {
        let n = Arc::new(AtomicUsize::new(0));
        let count = n.clone();
        let channel = Channel::new(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        (channel, n)
    }

    fn approval(id: &str) -> Approval {
        Approval { id: id.into(), kind: ApprovalKind::Command, path: String::new(), preview: "ls".into(), steps: vec![], reason: String::new() }
    }

    fn text(t: &str) -> TaskEvent {
        TaskEvent::Text { text: t.into() }
    }

    #[test]
    fn a_run_finished_while_detached_is_handed_over_once() {
        let state = TaskState::default();
        let (channel, seen) = counting_channel();
        state.begin("s1", "ファイルを作って", channel);
        state.emit(text("a"));
        assert_eq!(seen.load(Ordering::SeqCst), 1, "見ている間は画面へ流す");
        // ワークスペースを切り替えた: 以降は記録だけ
        state.detach();
        state.emit(text("b"));
        state.emit(TaskEvent::Approval(approval("t1")));
        assert_eq!(seen.load(Ordering::SeqCst), 1);
        // 承認待ちのまま、戻ってきた
        let (channel, resumed) = counting_channel();
        let snap = state.snapshot_and_attach(channel);
        assert!(snap.running);
        assert_eq!((snap.session_id.as_str(), snap.user_text.as_str()), ("s1", "ファイルを作って"));
        assert_eq!(snap.events.len(), 3, "見ていない間の進捗も受け取れる");
        assert_eq!(snap.pending.as_ref().map(|a| a.id.as_str()), Some("t1"), "承認待ちが残っている");
        // 回答して、続きの進捗と結果は、戻ってきた画面へ届く
        state.answer("t1", true);
        state.emit(text("c"));
        assert_eq!(resumed.load(Ordering::SeqCst), 1);
        state.finish(Ok(serde_json::json!({ "content": "done" })));
        assert_eq!(resumed.load(Ordering::SeqCst), 2, "完了もイベントで渡す");
        let snap = state.snapshot_and_attach(counting_channel().0);
        assert!(!snap.running && snap.finished.is_none() && snap.pending.is_none(), "二重には渡さない");
    }

    #[test]
    fn a_result_nobody_watched_is_kept_until_the_workspace_is_reopened() {
        let state = TaskState::default();
        state.begin("s1", "x", counting_channel().0);
        state.detach();
        state.finish(Err(serde_json::json!({ "code": "timeout" })));
        let snap = state.snapshot_and_attach(counting_channel().0);
        assert!(!snap.running);
        assert_eq!(snap.finished.as_ref().and_then(|f| f.error.as_ref()).unwrap()["code"], "timeout");
        assert!(state.snapshot_and_attach(counting_channel().0).finished.is_none(), "渡したら捨てる");
    }

    #[test]
    fn the_original_screen_receives_its_result_from_the_command_not_the_record() {
        let state = TaskState::default();
        let (channel, seen) = counting_channel();
        state.begin("s1", "x", channel);
        state.finish(Ok(serde_json::json!({ "content": "done" })));
        assert_eq!(seen.load(Ordering::SeqCst), 0, "戻り値で受け取る画面には、イベントを重ねて送らない");
        assert!(state.snapshot_and_attach(counting_channel().0).finished.is_none(), "取っておかない(二重に表示しない)");
    }

    #[test]
    fn recorded_events_are_capped_and_a_new_run_starts_clean() {
        let state = TaskState::default();
        state.begin("s1", "x", counting_channel().0);
        for i in 0..(MAX_RECORDED_EVENTS + 50) {
            state.emit(text(&i.to_string()));
        }
        let snap = state.snapshot_and_attach(counting_channel().0);
        assert_eq!(snap.events.len(), MAX_RECORDED_EVENTS);
        assert!(matches!(snap.events.last(), Some(TaskEvent::Text { text }) if text == &(MAX_RECORDED_EVENTS + 49).to_string()), "新しい側を残す");
        state.begin("s2", "y", counting_channel().0);
        let snap = state.snapshot_and_attach(counting_channel().0);
        assert!(snap.events.is_empty() && snap.session_id == "s2");
    }

    #[tokio::test]
    async fn abort_cancels_the_run_and_denies_waiting_approvals() {
        let state = TaskState::default();
        let token = CancelToken::default();
        state.set_cancel(token.clone());
        state.begin("s1", "x", counting_channel().0);
        state.emit(TaskEvent::Approval(approval("t1")));
        let rx = state.register_approval("t1");
        state.abort();
        assert!(rx.await.is_err(), "承認待ちは、答えのないまま閉じる(=拒否として扱われる)");
        token.cancelled().await; // 中断されている(すぐ戻る)
        assert!(state.snapshot_and_attach(counting_channel().0).pending.is_none());
    }

    #[tokio::test]
    async fn answering_resolves_the_waiting_side_and_clears_the_pending_approval() {
        let state = TaskState::default();
        state.begin("s1", "x", counting_channel().0);
        state.emit(TaskEvent::Approval(approval("t1")));
        let rx = state.register_approval("t1");
        state.answer("t1", true);
        assert!(rx.await.unwrap());
        assert!(state.snapshot_and_attach(counting_channel().0).pending.is_none());
        // 待っていないIDへの応答は無視する
        state.answer("unknown", false);
    }
}
