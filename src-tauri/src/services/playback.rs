//! 読み上げ音声の再生。WebView(WKWebView)の音声出力は、長く使わないと黙って無音になるため、
//! cpalで既定の出力デバイスへ直接鳴らす。出力ストリームは再生ごとに開いて閉じ、放置で古くなる経路を持たない。
//! cpalのStreamはSendでないため、[`super::audio::Recorder`]と同じく専用スレッドで保持する。
use std::{
    io::Cursor,
    sync::{
        Arc, Mutex, mpsc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use cpal::{
    FromSample, SampleFormat, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use symphonia::core::{
    codecs::audio::AudioDecoderOptions,
    errors::Error as SymError,
    formats::{FormatOptions, TrackType, probe::Hint},
    io::MediaSourceStream,
    meta::MetadataOptions,
};

use super::audio::{peaks, resample};
use crate::utils::error::AppError;

/// 末尾まで書き込んでから、出力のバッファが鳴り終わるまでストリームを閉じずに待つ時間。
const TAIL: Duration = Duration::from_millis(200);
/// 停止の指示と、末尾まで書き込んだかを確かめる間隔。
const POLL: Duration = Duration::from_millis(30);

fn audio_err(e: impl std::fmt::Display) -> AppError {
    AppError::Audio(e.to_string())
}

/// 音声データ(VOICEVOXのWAV、Google TTSのMP3)をモノラルのf32とサンプリングレートにする。
pub fn decode(bytes: &[u8]) -> Result<(Vec<f32>, u32), AppError> {
    if bytes.starts_with(b"RIFF") { decode_wav(bytes) } else { decode_mp3(bytes) }
}

/// 複数チャンネルをモノラルへ平均化する。
fn downmix(interleaved: &[f32], channels: usize, out: &mut Vec<f32>) {
    let channels = channels.max(1);
    out.extend(interleaved.chunks(channels).map(|f| f.iter().sum::<f32>() / f.len() as f32));
}

fn decode_wav(bytes: &[u8]) -> Result<(Vec<f32>, u32), AppError> {
    let reader = hound::WavReader::new(Cursor::new(bytes)).map_err(audio_err)?;
    let spec = reader.spec();
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.into_samples::<f32>().collect::<Result<_, _>>().map_err(audio_err)?,
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample.max(1) - 1)) as f32;
            reader.into_samples::<i32>().map(|s| s.map(|v| v as f32 / scale)).collect::<Result<_, _>>().map_err(audio_err)?
        }
    };
    let mut mono = Vec::with_capacity(interleaved.len() / spec.channels.max(1) as usize);
    downmix(&interleaved, spec.channels as usize, &mut mono);
    Ok((mono, spec.sample_rate))
}

fn decode_mp3(bytes: &[u8]) -> Result<(Vec<f32>, u32), AppError> {
    let mss = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(audio_err)?;
    let track = format.default_track(TrackType::Audio).ok_or_else(|| AppError::Audio("no audio track".into()))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| AppError::Audio("no audio track".into()))?;
    let mut rate = params.sample_rate.unwrap_or(0);
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(audio_err)?;
    let mut mono = Vec::new();
    let mut samples: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(audio_err(e)),
        };
        if packet.track_id != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            // 壊れたフレームは飛ばして続ける
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(audio_err(e)),
        };
        let spec = decoded.spec();
        rate = spec.rate();
        decoded.copy_to_vec_interleaved(&mut samples);
        downmix(&samples, spec.channels().count(), &mut mono);
    }
    if rate == 0 {
        return Err(AppError::Audio("unknown sample rate".into()));
    }
    Ok((mono, rate))
}

/// 出力スレッドの操作口。
struct Output {
    stop: mpsc::Sender<()>,
    thread: JoinHandle<()>,
}

impl Output {
    /// 再生を止め、ストリームを閉じるまで待つ。
    fn halt(self) {
        let _ = self.stop.send(());
        let _ = self.thread.join();
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    samples: Vec<f32>,
    done: Arc<AtomicBool>,
) -> Result<cpal::Stream, AppError>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = (config.channels as usize).max(1);
    let mut pos = 0usize;
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                for frame in data.chunks_mut(channels) {
                    let v = samples.get(pos).copied().unwrap_or(0.0);
                    pos += 1;
                    frame.fill(T::from_sample(v));
                }
                if pos >= samples.len() {
                    done.store(true, Ordering::Relaxed);
                }
            },
            |e| log::error!("audio output error: {e}"),
            None,
        )
        .map_err(audio_err)
}

/// 既定の出力デバイスを開いて`samples`(モノラル、`rate`Hz)を鳴らし始める。鳴り始めたら戻る。
/// 末尾まで鳴らすか、停止の指示でストリームを閉じる。
fn start_output(samples: Arc<Vec<f32>>, rate: u32) -> Result<Output, AppError> {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), AppError>>();
    let thread = std::thread::spawn(move || {
        let done = Arc::new(AtomicBool::new(false));
        let setup = || -> Result<cpal::Stream, AppError> {
            let device = cpal::default_host()
                .default_output_device()
                .ok_or_else(|| AppError::Audio("no output device".into()))?;
            let supported = device.default_output_config().map_err(audio_err)?;
            let out_rate = supported.sample_rate();
            let format = supported.sample_format();
            let config: cpal::StreamConfig = supported.into();
            let pcm = resample(&samples, rate, out_rate);
            let stream = match format {
                SampleFormat::F32 => build_stream::<f32>(&device, config, pcm, done.clone())?,
                SampleFormat::I16 => build_stream::<i16>(&device, config, pcm, done.clone())?,
                SampleFormat::U16 => build_stream::<u16>(&device, config, pcm, done.clone())?,
                other => return Err(AppError::Audio(format!("unsupported sample format: {other:?}"))),
            };
            stream.play().map_err(audio_err)?;
            log::info!("output started ({out_rate}Hz, {:.1}s)", samples.len() as f64 / rate as f64);
            Ok(stream)
        };
        let stream = match setup() {
            Ok(s) => s,
            Err(e) => {
                let _ = ready_tx.send(Err(e));
                return;
            }
        };
        let _ = ready_tx.send(Ok(()));
        loop {
            match stop_rx.recv_timeout(POLL) {
                Ok(()) => break,
                // 操作口を手放した再生(効果音)は、末尾まで鳴らす
                Err(mpsc::RecvTimeoutError::Disconnected) => std::thread::sleep(POLL),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if done.load(Ordering::Relaxed) {
                std::thread::sleep(TAIL);
                break;
            }
        }
        drop(stream);
    });
    match ready_rx.recv() {
        Ok(Ok(())) => Ok(Output { stop: stop_tx, thread }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err(AppError::Audio("audio thread terminated".into())),
    }
}

struct Active {
    output: Output,
    /// 再生中の音声(モノラル、元のレート)。波形表示に使う。
    samples: Arc<Vec<f32>>,
    rate: u32,
    started: Instant,
}

/// 読み上げの再生。同時に鳴らすのは1つだけで、新しい再生は前の再生を止める。
#[derive(Default)]
pub struct Player(Mutex<Option<Active>>);

impl Player {
    fn take(&self) -> Option<Active> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).take()
    }

    /// 音声データを再生し始め、長さ(秒)を返す。直前の再生は止める。ブロッキングなので`spawn_blocking`から呼ぶ。
    pub fn play(&self, bytes: &[u8]) -> Result<f64, AppError> {
        let (samples, rate) = decode(bytes)?;
        if samples.is_empty() {
            return Err(AppError::Audio("empty audio".into()));
        }
        if let Some(old) = self.take() {
            old.output.halt();
        }
        let samples = Arc::new(samples);
        let output = start_output(samples.clone(), rate)?;
        let duration = samples.len() as f64 / rate as f64;
        let active = Active { output, samples, rate, started: Instant::now() };
        // 開いている間に別の再生が始まっていたら、そちらを止める
        if let Some(prev) = self.0.lock().unwrap_or_else(|p| p.into_inner()).replace(active) {
            prev.output.halt();
        }
        Ok(duration)
    }

    /// 再生中の音声を止める。再生していなければ何もしない。
    pub fn stop(&self) {
        if let Some(a) = self.take() {
            a.output.halt();
        }
    }

    /// 再生位置の直前約100msを`points`個の代表値にして返す。再生していなければ空。波形表示用。
    pub fn waveform(&self, points: usize) -> Vec<f32> {
        let slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let Some(a) = slot.as_ref() else { return Vec::new() };
        let pos = (a.started.elapsed().as_secs_f64() * a.rate as f64) as usize;
        window_before(&a.samples, pos, a.rate as usize / 10).map(|w| peaks(w, points)).unwrap_or_default()
    }
}

/// `pos`の直前`len`サンプル。`pos`が末尾を過ぎていれば`None`(再生し終えた)。
fn window_before(samples: &[f32], pos: usize, len: usize) -> Option<&[f32]> {
    if pos >= samples.len() {
        return None;
    }
    let end = pos.max(len.min(samples.len()));
    Some(&samples[end.saturating_sub(len)..end])
}

/// 効果音を鳴らす。読み上げの再生には触れず、鳴り終えたら自分で閉じる。
pub fn play_effect(bytes: &[u8]) -> Result<(), AppError> {
    let (samples, rate) = decode(bytes)?;
    drop(start_output(Arc::new(samples), rate)?.stop);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(channels: u16, rate: u32, frames: &[[i16; 2]]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let spec = hound::WavSpec { channels, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::new(&mut out, spec).unwrap();
        for f in frames {
            for s in &f[..channels as usize] {
                w.write_sample(*s).unwrap();
            }
        }
        w.finalize().unwrap();
        out.into_inner()
    }

    #[test]
    fn decodes_wav_to_mono() {
        let (mono, rate) = decode(&wav(2, 24_000, &[[16384, 0], [-16384, -16384]])).unwrap();
        assert_eq!(rate, 24_000);
        assert_eq!(mono, vec![0.25, -0.5]);
        let (mono, _) = decode(&wav(1, 24_000, &[[16384, 0]])).unwrap();
        assert_eq!(mono, vec![0.5]);
    }

    #[test]
    fn decodes_bundled_error_sound() {
        let (mono, rate) = decode(include_bytes!("../../sounds/error.wav")).unwrap();
        assert!(rate > 0 && !mono.is_empty());
    }

    #[test]
    fn rejects_broken_audio() {
        assert!(decode(b"RIFF broken").is_err());
        assert!(decode(b"not audio at all").is_err());
        assert!(decode(&[]).is_err());
    }

    #[test]
    fn window_before_follows_position() {
        let s: Vec<f32> = (0..10).map(|i| i as f32).collect();
        assert_eq!(window_before(&s, 5, 3).unwrap(), &[2.0, 3.0, 4.0]);
        // 出だしは先頭から`len`サンプル
        assert_eq!(window_before(&s, 1, 3).unwrap(), &[0.0, 1.0, 2.0]);
        assert!(window_before(&s, 10, 3).is_none());
        assert_eq!(window_before(&s[..2], 0, 3).unwrap(), &[0.0, 1.0]);
    }

    #[test]
    fn idle_player_has_no_waveform() {
        let p = Player::default();
        p.stop();
        assert!(p.waveform(128).is_empty());
    }

    /// 既定の出力デバイスで0.5秒の正弦波を鳴らす(出力デバイスが必要)。
    #[test]
    #[ignore]
    fn plays_on_default_output() {
        let frames: Vec<[i16; 2]> = (0..12_000).map(|i| [((i as f32 / 24_000.0 * 440.0 * std::f32::consts::TAU).sin() * 8000.0) as i16, 0]).collect();
        let p = Player::default();
        let secs = p.play(&wav(1, 24_000, &frames)).unwrap();
        assert!((secs - 0.5).abs() < 1e-6);
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(p.waveform(128).len(), 128);
        std::thread::sleep(Duration::from_millis(500));
        p.stop();
    }
}
