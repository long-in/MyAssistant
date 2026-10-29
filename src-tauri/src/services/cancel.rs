//! 実行中の依頼を中断するためのトークン。複製して共有でき、どれか1つが`cancel`すれば全員に伝わる。
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tokio::sync::Notify;

#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<Inner>);

#[derive(Debug, Default)]
struct Inner {
    flag: AtomicBool,
    notify: Notify,
}

impl CancelToken {
    pub fn cancel(&self) {
        self.0.flag.store(true, Ordering::SeqCst);
        self.0.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.flag.load(Ordering::SeqCst)
    }

    /// 中断されるまで待つ。すでに中断済みなら即座に返る。
    pub async fn cancelled(&self) {
        loop {
            // 通知の登録をフラグの確認より先に済ませ、確認と待機の間の取りこぼしを防ぐ
            let notified = self.0.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn cancel_wakes_waiters_and_stays_cancelled() {
        let token = CancelToken::default();
        let waiter = tokio::spawn({
            let t = token.clone();
            async move { t.cancelled().await }
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!token.is_cancelled());
        token.cancel();
        tokio::time::timeout(Duration::from_secs(1), waiter).await.unwrap().unwrap();
        // 中断後に待ち始めても即座に返る
        tokio::time::timeout(Duration::from_millis(100), token.cancelled()).await.unwrap();
    }
}
