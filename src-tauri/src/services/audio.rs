//! マイク入力のキャプチャ。cpalのStreamはSendでないため専用スレッドで保持する。
use std::{
    sync::{
        Arc, Mutex, mpsc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use cpal::{
    FromSample, Sample, SampleFormat, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

use crate::utils::error::AppError;

/// Whisperが要求するサンプリングレート。
pub const WHISPER_RATE: u32 = 16_000;
/// 録音の上限(秒)。メモリ使用量の暴走を防ぐ。
const MAX_SECONDS: usize = 120;
/// デバイスが実際に音を届け始めるまで待つ上限。
/// 開始直後は一定時間、無音(厳密に0)のデータしか届かないデバイスがあり、
/// 「録音中」表示の直後に話し始めると冒頭が欠けるため。
const WARMUP_TIMEOUT: Duration = Duration::from_millis(2500);

struct Active {
    stop: mpsc::Sender<()>,
    thread: JoinHandle<()>,
    buf: Arc<Mutex<Vec<f32>>>,
    rate: u32,
}

#[derive(Default)]
pub struct Recorder(Mutex<Option<Active>>);

fn audio_err(e: impl std::fmt::Display) -> AppError {
    AppError::Audio(e.to_string())
}

/// 複数チャンネルをモノラルへ平均化して`out`へ追記する(上限超過分は捨てる)。
fn push_mono<T: Sample>(
    data: &[T],
    channels: usize,
    limit: usize,
    out: &Mutex<Vec<f32>>,
    live: &AtomicBool,
) where
    f32: FromSample<T>,
{
    let mut out = out.lock().unwrap_or_else(|p| p.into_inner());
    for frame in data.chunks(channels) {
        if out.len() >= limit {
            return;
        }
        let sum: f32 = frame.iter().map(|s| f32::from_sample(*s)).sum();
        if sum != 0.0 {
            live.store(true, Ordering::Relaxed);
        }
        out.push(sum / frame.len() as f32);
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    buf: Arc<Mutex<Vec<f32>>>,
    live: Arc<AtomicBool>,
    limit: usize,
) -> Result<cpal::Stream, AppError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = config.channels as usize;
    device
        .build_input_stream(
            config,
            move |data: &[T], _| push_mono(data, channels, limit, &buf, &live),
            |e| log::error!("audio input error: {e}"),
            None,
        )
        .map_err(audio_err)
}

impl Recorder {
    pub fn start(&self) -> Result<(), AppError> {
        let mut slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_some() {
            return Err(AppError::AlreadyRecording);
        }
        let buf = Arc::new(Mutex::new(Vec::new()));
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, AppError>>();

        let thread_buf = buf.clone();
        let thread_buf_clear = buf.clone();
        let live = Arc::new(AtomicBool::new(false));
        let thread = std::thread::spawn(move || {
            let setup = || -> Result<(cpal::Stream, u32), AppError> {
                let device = cpal::default_host()
                    .default_input_device()
                    .ok_or_else(|| AppError::Audio("no input device".into()))?;
                let supported = device.default_input_config().map_err(audio_err)?;
                let rate = supported.sample_rate();
                let format = supported.sample_format();
                let config: cpal::StreamConfig = supported.into();
                let limit = rate as usize * MAX_SECONDS;
                let stream = match format {
                    SampleFormat::F32 => build_stream::<f32>(&device, config, thread_buf, live.clone(), limit)?,
                    SampleFormat::I16 => build_stream::<i16>(&device, config, thread_buf, live.clone(), limit)?,
                    SampleFormat::U16 => build_stream::<u16>(&device, config, thread_buf, live.clone(), limit)?,
                    other => return Err(AppError::Audio(format!("unsupported sample format: {other:?}"))),
                };
                stream.play().map_err(audio_err)?;
                let started = Instant::now();
                while !live.load(Ordering::Relaxed) && started.elapsed() < WARMUP_TIMEOUT {
                    std::thread::sleep(Duration::from_millis(20));
                }
                // 起動直後のポップノイズを捨て、ここから先だけを録音データとする
                std::thread::sleep(Duration::from_millis(100));
                thread_buf_clear.lock().unwrap_or_else(|p| p.into_inner()).clear();
                log::info!("input ready after {:?} ({rate}Hz, live={})", started.elapsed(), live.load(Ordering::Relaxed));
                Ok((stream, rate))
            };
            match setup() {
                Ok((stream, rate)) => {
                    let _ = ready_tx.send(Ok(rate));
                    let _ = stop_rx.recv();
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        });

        match ready_rx.recv() {
            Ok(Ok(rate)) => {
                *slot = Some(Active { stop: stop_tx, thread, buf, rate });
                Ok(())
            }
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err(AppError::Audio("audio thread terminated".into())),
        }
    }

    /// 直近約100msの入力を`points`個の代表値(各区間でもっとも振幅の大きいサンプル)に縮約して返す。
    /// 録音中でなければ空。波形表示用。
    pub fn waveform(&self, points: usize) -> Vec<f32> {
        let slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let Some(active) = slot.as_ref() else { return Vec::new() };
        let buf = active.buf.lock().unwrap_or_else(|p| p.into_inner());
        let window = (active.rate as usize / 10).min(buf.len());
        if points == 0 || window < points {
            return Vec::new();
        }
        buf[buf.len() - window..]
            .chunks(window / points)
            .take(points)
            .map(|c| c.iter().copied().fold(0.0f32, |m, x| if x.abs() > m.abs() { x } else { m }))
            .collect()
    }

    /// 直近`keep_ms`ミリ秒だけを残し、それより古い入力を捨てる。
    /// 常時待機中に無音が溜まり続けないようにするためのもの。録音中でなければ何もしない。
    pub fn trim(&self, keep_ms: u32) {
        let slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let Some(active) = slot.as_ref() else { return };
        let keep = active.rate as usize * keep_ms as usize / 1000;
        let mut buf = active.buf.lock().unwrap_or_else(|p| p.into_inner());
        if buf.len() > keep {
            let drop = buf.len() - keep;
            buf.drain(..drop);
        }
    }

    /// 録音を止め、Whisper入力形式(16kHz mono f32)で返す。
    pub fn stop(&self) -> Result<Vec<f32>, AppError> {
        let active = self.0.lock().unwrap_or_else(|p| p.into_inner()).take().ok_or(AppError::NotRecording)?;
        let _ = active.stop.send(());
        let _ = active.thread.join();
        let samples = std::mem::take(&mut *active.buf.lock().unwrap_or_else(|p| p.into_inner()));
        Ok(resample(&samples, active.rate, WHISPER_RATE))
    }
}

/// 線形補間によるリサンプリング。
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let out_len = (input.len() as f64 / ratio) as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = input[idx];
            let b = *input.get(idx + 1).unwrap_or(&a);
            a + (b - a) * frac
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_length_and_identity() {
        let x: Vec<f32> = (0..48_000).map(|i| (i as f32 / 100.0).sin()).collect();
        assert_eq!(resample(&x, 48_000, 16_000).len(), 16_000);
        assert_eq!(resample(&x, 16_000, 16_000), x);
        assert!(resample(&[], 48_000, 16_000).is_empty());
    }

    #[test]
    fn push_mono_downmixes_and_limits() {
        let out = Mutex::new(Vec::new());
        push_mono(&[1.0f32, 0.0, 0.5, 0.5, 1.0, 1.0], 2, 2, &out, &AtomicBool::new(false));
        assert_eq!(*out.lock().unwrap(), vec![0.5, 0.5]);
    }

    #[test]
    fn trim_keeps_only_the_latest_input() {
        let r = Recorder::default();
        r.trim(100); // 録音中でなければ何もしない
        let buf = Arc::new(Mutex::new((0..1000).map(|i| i as f32).collect::<Vec<_>>()));
        let (stop, _rx) = mpsc::channel();
        *r.0.lock().unwrap() = Some(Active { stop, thread: std::thread::spawn(|| {}), buf: buf.clone(), rate: 1000 });
        r.trim(100);
        let kept = buf.lock().unwrap().clone();
        assert_eq!(kept.len(), 100);
        assert_eq!(kept[0], 900.0);
    }

    /// 実マイクを2秒録音して診断情報を出す(マイクと権限が必要)。
    #[test]
    #[ignore]
    fn records_from_default_mic() {
        let r = Recorder::default();
        r.start().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(2));
        let s = r.stop().unwrap();
        let peak = s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        println!("samples={} (~{:.2}s) peak={peak:.4}", s.len(), s.len() as f32 / WHISPER_RATE as f32);
    }
}
