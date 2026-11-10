//! 取得(ダウンロード)の進み具合の保持。
//!
//! 進み具合は呼び出しごとのチャネルではなくここに置き、画面が切り替わって受け手を失っても、
//! 戻った画面が現在の状況を問い合わせて続きを表示できるようにする。
use std::sync::Mutex;

use serde::Serialize;

/// 取得の進み具合。`bytes`が`false`のときは単位のない進み具合(複数ファイルの工程数など)で、割合だけを使う。
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Progress {
    pub downloaded: u64,
    pub total: u64,
    pub bytes: bool,
}

impl Progress {
    /// バイト数での進み具合。`total`が分からないときは0。
    pub fn bytes(downloaded: u64, total: u64) -> Self {
        Self { downloaded, total, bytes: true }
    }

    /// `n`個の工程のうち`done`個が済み、いまの工程が`fraction`(0〜1)まで進んだ状態。
    pub fn steps(done: usize, n: usize, fraction: f64) -> Self {
        Self { downloaded: ((done as f64 + fraction.clamp(0.0, 1.0)) * 1000.0) as u64, total: n as u64 * 1000, bytes: false }
    }
}

/// いま取得中のものの進み具合(取得中でなければ`None`)。
#[derive(Default)]
pub struct Slot(Mutex<Option<Progress>>);

impl Slot {
    pub const fn new() -> Self {
        Self(Mutex::new(None))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Progress>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn get(&self) -> Option<Progress> {
        *self.lock()
    }

    pub fn set(&self, progress: Progress) {
        *self.lock() = Some(progress);
    }

    pub fn clear(&self) {
        *self.lock() = None;
    }

    /// 取得を始める。すでに取得中なら`None`。返したガードを手放す(成功・失敗・中断のいずれでも)と取得中でなくなる。
    pub fn begin(&self) -> Option<Guard<'_>> {
        let mut slot = self.lock();
        if slot.is_some() {
            return None;
        }
        *slot = Some(Progress::bytes(0, 0));
        Some(Guard(self))
    }
}

pub struct Guard<'a>(&'a Slot);

impl Guard<'_> {
    pub fn set(&self, progress: Progress) {
        self.0.set(progress);
    }
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_rejects_a_second_download_until_the_guard_drops() {
        let slot = Slot::new();
        assert!(slot.get().is_none());
        let guard = slot.begin().expect("first begin");
        assert!(slot.begin().is_none());
        guard.set(Progress::bytes(5, 10));
        assert_eq!(slot.get().map(|p| p.downloaded), Some(5));
        drop(guard);
        assert!(slot.get().is_none());
        assert!(slot.begin().is_some());
    }

    #[test]
    fn steps_combine_finished_and_current_fractions() {
        let p = Progress::steps(1, 4, 0.5);
        assert_eq!((p.downloaded, p.total, p.bytes), (1500, 4000, false));
        assert_eq!(Progress::steps(0, 4, 2.0).downloaded, 1000);
    }
}
