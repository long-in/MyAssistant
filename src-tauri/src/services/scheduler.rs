//! 整理バッチの定期実行。ワークスペース(パーソナルと各スペース)ごとに、自分の設定の間隔で独立して動かす。
//!
//! - 一定間隔(`TICK`)でワークスペースを見直し、前回から自分の間隔が過ぎたものだけを実行する。
//! - 実行はワークスペースごとに別のタスクで行うので、遅い・失敗するワークスペースが他を待たせない。
//! - 読み込んでいないスペースは、間隔を読むときと実行するときだけ一時的に開き、開いたままにしない。
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use tauri::{AppHandle, Manager};

use super::{
    batch::{self, DEFAULT_INTERVAL_MINUTES},
    workspace::{WorkspaceCtx, Workspaces},
};
use crate::utils::error::AppError;

/// ワークスペースを見直す間隔。設定の変更は、この間隔のうちに反映される。
const TICK: Duration = Duration::from_secs(30);

/// ワークスペース1件の実行の状況。
#[derive(Debug)]
struct Slot {
    last_run: Instant,
    interval: Duration,
    running: bool,
}

type Slots = HashMap<String, Slot>;

fn minutes(m: u32) -> Duration {
    Duration::from_secs(u64::from(m) * 60)
}

fn interval_of(ctx: &WorkspaceCtx) -> Duration {
    minutes(batch::interval_minutes(&ctx.db).unwrap_or(DEFAULT_INTERVAL_MINUTES))
}

/// 見直しの1回分。実行すべきワークスペースのIDを返し、その分を実行中にする。
/// - 一覧から消えたワークスペースは捨てる。
/// - 初めて見るワークスペースは、いまを前回として扱う(最初の実行は、1間隔あとになる)。
/// - `interval`が間隔(分からなければ`None`で、前回までの値を使う)を返す。
fn plan(slots: &mut Slots, ids: &[String], now: Instant, mut interval: impl FnMut(&str, bool) -> Option<Duration>) -> Vec<String> {
    slots.retain(|id, _| ids.contains(id));
    let mut due = Vec::new();
    for id in ids {
        let known = slots.contains_key(id);
        let latest = interval(id, known);
        let slot = slots.entry(id.clone()).or_insert_with(|| Slot {
            last_run: now,
            interval: latest.unwrap_or(minutes(DEFAULT_INTERVAL_MINUTES)),
            running: false,
        });
        if let Some(i) = latest {
            slot.interval = i;
        }
        if !slot.running && now.saturating_duration_since(slot.last_run) >= slot.interval {
            slot.running = true;
            due.push(id.clone());
        }
    }
    due
}

fn lock(slots: &Mutex<Slots>) -> std::sync::MutexGuard<'_, Slots> {
    slots.lock().unwrap_or_else(|p| p.into_inner())
}

/// 定期実行を始める。アプリの起動時に1回呼ぶ。
pub fn spawn(handle: AppHandle) {
    let slots: Arc<Mutex<Slots>> = Arc::default();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(TICK).await;
            // DBを開く可能性があるので、非同期の実行を妨げないよう別のスレッドで見直す
            let (h, s) = (handle.clone(), slots.clone());
            let due = tauri::async_runtime::spawn_blocking(move || {
                let ws = h.state::<Workspaces>();
                let ids = ws.batch_ids();
                let mut map = lock(&s);
                plan(&mut map, &ids, Instant::now(), |id, known| match ws.get(id) {
                    // 読み込み済みなら、毎回読み直す(間隔の変更を早く反映する)
                    Some(ctx) => Some(interval_of(&ctx)),
                    // 未読み込みなら、初めて見たときだけ一時的に開いて読む
                    None if !known => ws.open_for_batch(id).ok().map(|ctx| interval_of(&ctx)),
                    None => None,
                })
            })
            .await
            .unwrap_or_default();
            for id in due {
                let (h, s) = (handle.clone(), slots.clone());
                tauri::async_runtime::spawn(async move { run_one(&h, &s, &id).await });
            }
        }
    });
}

/// 1つのワークスペースを整理する。終わったら(失敗しても)前回の時刻と間隔を更新する。
async fn run_one(handle: &AppHandle, slots: &Mutex<Slots>, id: &str) {
    let outcome: Result<Duration, AppError> = async {
        let ctx = handle.state::<Workspaces>().open_for_batch(id)?;
        let result = batch::run_guarded(&ctx).await;
        let interval = interval_of(&ctx);
        result.map(|_| interval)
    }
    .await;
    if let Some(slot) = lock(slots).get_mut(id) {
        slot.running = false;
        slot.last_run = Instant::now();
        if let Ok(interval) = &outcome {
            slot.interval = *interval;
        }
    }
    match outcome {
        Err(AppError::SpaceNotFound) => {} // 実行中に削除された
        Err(e) => log::warn!("periodic batch failed ({id}): {e}"),
        Ok(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn first_sight_waits_one_interval_then_runs() {
        let mut slots = Slots::new();
        let t0 = Instant::now();
        let ten = Some(Duration::from_secs(600));
        assert!(plan(&mut slots, &ids(&["a"]), t0, |_, _| ten).is_empty(), "初回はすぐ実行しない");
        assert!(plan(&mut slots, &ids(&["a"]), t0 + Duration::from_secs(599), |_, _| ten).is_empty());
        assert_eq!(plan(&mut slots, &ids(&["a"]), t0 + Duration::from_secs(600), |_, _| ten), ids(&["a"]));
    }

    #[test]
    fn each_workspace_uses_its_own_interval() {
        let mut slots = Slots::new();
        let t0 = Instant::now();
        let by = |id: &str, _| Some(Duration::from_secs(if id == "fast" { 300 } else { 3600 }));
        plan(&mut slots, &ids(&["fast", "slow"]), t0, by);
        // 5分後は、5分間隔のものだけが対象(60分間隔のものを、5分間隔に引きずらない)
        assert_eq!(plan(&mut slots, &ids(&["fast", "slow"]), t0 + Duration::from_secs(300), by), ids(&["fast"]));
        // 実行中は、重ねて対象にしない
        assert!(plan(&mut slots, &ids(&["fast", "slow"]), t0 + Duration::from_secs(900), by).iter().all(|i| i != "fast"));
        assert_eq!(plan(&mut slots, &ids(&["fast", "slow"]), t0 + Duration::from_secs(3600), by), ids(&["slow"]));
    }

    #[test]
    fn a_running_workspace_does_not_block_others() {
        let mut slots = Slots::new();
        let t0 = Instant::now();
        let m = Some(Duration::from_secs(60));
        plan(&mut slots, &ids(&["a", "b"]), t0, |_, _| m);
        let due = plan(&mut slots, &ids(&["a", "b"]), t0 + Duration::from_secs(60), |_, _| m);
        assert_eq!(due.len(), 2, "遅いものを待たずに、両方が動き出す");
        // aが終わっていなくても、bは終わって次の間隔で再び対象になる
        slots.get_mut("b").unwrap().running = false;
        slots.get_mut("b").unwrap().last_run = t0 + Duration::from_secs(60);
        let due = plan(&mut slots, &ids(&["a", "b"]), t0 + Duration::from_secs(120), |_, _| m);
        assert_eq!(due, ids(&["b"]));
    }

    #[test]
    fn interval_changes_apply_without_waiting_for_the_next_run() {
        let mut slots = Slots::new();
        let t0 = Instant::now();
        plan(&mut slots, &ids(&["a"]), t0, |_, _| Some(Duration::from_secs(3600)));
        // 60分 → 5分へ変えると、次の見直しで反映される
        let due = plan(&mut slots, &ids(&["a"]), t0 + Duration::from_secs(300), |_, _| Some(Duration::from_secs(300)));
        assert_eq!(due, ids(&["a"]));
    }

    #[test]
    fn unknown_interval_keeps_the_previous_value_and_removed_ids_are_dropped() {
        let mut slots = Slots::new();
        let t0 = Instant::now();
        plan(&mut slots, &ids(&["a", "b"]), t0, |_, _| Some(Duration::from_secs(60)));
        // 間隔が分からなくても(未読み込み)、前回までの値で判定する
        assert_eq!(plan(&mut slots, &ids(&["a", "b"]), t0 + Duration::from_secs(60), |_, known| {
            assert!(known, "2回目以降は、すでに知っているものとして呼ばれる");
            None
        }).len(), 2);
        // 一覧から消えたワークスペース(削除)は捨てる
        plan(&mut slots, &ids(&["a"]), t0 + Duration::from_secs(61), |_, _| None);
        assert!(!slots.contains_key("b"));
    }
}
