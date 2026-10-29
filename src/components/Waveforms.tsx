"use client";

import { useEffect, useRef } from "react";
import { getOutputAnalyser, recordingWaveform } from "@/lib/api";
import { PHASE_HUE, type Phase } from "./OrbReadout";

/** canvasを表示サイズ×devicePixelRatioに合わせ、CSS座標で描けるようにして描画領域を返す。 */
function prepare(canvas: HTMLCanvasElement) {
  const dpr = window.devicePixelRatio || 1;
  const { width, height } = canvas.getBoundingClientRect();
  if (canvas.width !== Math.round(width * dpr) || canvas.height !== Math.round(height * dpr)) {
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
  }
  const ctx = canvas.getContext("2d")!;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, width, height);
  return { ctx, width, height };
}

/** 毎フレーム`draw`を呼ぶ。 */
function useFrameLoop(draw: (canvas: HTMLCanvasElement, t: number) => void) {
  const ref = useRef<HTMLCanvasElement>(null);
  const drawRef = useRef(draw);
  useEffect(() => {
    drawRef.current = draw;
  });
  useEffect(() => {
    let id = 0;
    const loop = (t: number) => {
      if (ref.current) drawRef.current(ref.current, t);
      id = requestAnimationFrame(loop);
    };
    id = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(id);
  }, []);
  return ref;
}

const hsl = (h: number, l = 60, a = 1) => `hsla(${h}, 100%, ${l}%, ${a})`;
/** 基準色相`h`を中心に±20の幅でグラデーションさせる。 */
const hueAt = (t: number, h: number) => h - 20 + t * 40;

const BARS = 96;
const POINTS = 180;

/**
 * 音声の円形波形。HUD風のリングと周波数バー、中心の波形リングで構成する。
 * 読み上げ音声と、録音中(`micActive`)のマイク入力を同じ枠に描き、処理段階(`phase`)ごとに色相を変える。
 */
export function VoiceOrb({
  phase,
  micActive,
  thinking,
  className,
}: {
  phase: Phase;
  micActive?: boolean;
  thinking?: boolean;
  className?: string;
}) {
  const time = useRef<Uint8Array<ArrayBuffer> | null>(null);
  const freq = useRef<Uint8Array<ArrayBuffer> | null>(null);
  const bars = useRef(new Float32Array(BARS));
  const level = useRef(0);
  const spin = useRef(0);
  const last = useRef(0);
  const hue = useRef(PHASE_HUE.standby);
  const mic = useRef<number[]>([]);
  const micWave = useRef<Uint8Array<ArrayBuffer> | null>(null);
  const micSpec = useRef<Uint8Array<ArrayBuffer> | null>(null);

  useEffect(() => {
    if (!micActive) {
      mic.current = [];
      return;
    }
    let busy = false;
    const id = setInterval(async () => {
      if (busy) return;
      busy = true;
      try {
        mic.current = await recordingWaveform();
      } catch {
        mic.current = [];
      } finally {
        busy = false;
      }
    }, 40);
    return () => clearInterval(id);
  }, [micActive]);

  const ref = useFrameLoop((canvas, t) => {
    const { ctx, width, height } = prepare(canvas);
    const cx = width / 2;
    const cy = height / 2;
    const size = Math.min(width, height);
    // 小さい画面でも線が太く見えすぎず、大きい画面でも細くなりすぎないよう線幅・発光をサイズに比例させる
    const k = Math.max(0.6, Math.min(1.6, size / 420));
    const base = size * 0.24;
    const dt = Math.min(64, t - (last.current || t));
    last.current = t;

    // 色相は最短経路で滑らかに目標へ寄せる
    const diff = ((PHASE_HUE[phase] - hue.current + 540) % 360) - 180;
    hue.current = (hue.current + diff * 0.06 + 360) % 360;
    const H = hue.current;

    const analyser = getOutputAnalyser();
    let wave: Uint8Array<ArrayBuffer> | null = null;
    let spec: Uint8Array<ArrayBuffer> | null = null;
    if (micActive && mic.current.length) {
      // マイク入力は小さいので増幅し、出力側の波形・周波数と同じ形式(0..255)に変換する
      const n = mic.current.length;
      if (micWave.current?.length !== n) {
        micWave.current = new Uint8Array(n);
        micSpec.current = new Uint8Array(n);
      }
      for (let i = 0; i < n; i++) {
        const v = Math.max(-1, Math.min(1, mic.current[i] * 4));
        micWave.current[i] = 128 + v * 127;
        micSpec.current![i] = Math.abs(v) * 255;
      }
      wave = micWave.current;
      spec = micSpec.current;
    } else if (analyser) {
      if (time.current?.length !== analyser.fftSize) time.current = new Uint8Array(analyser.fftSize);
      if (freq.current?.length !== analyser.frequencyBinCount) freq.current = new Uint8Array(analyser.frequencyBinCount);
      analyser.getByteTimeDomainData(time.current);
      analyser.getByteFrequencyData(freq.current);
      wave = time.current;
      spec = freq.current;
    }

    let peak = 0;
    if (wave) for (let i = 0; i < wave.length; i += 4) peak = Math.max(peak, Math.abs(wave[i] - 128) / 128);
    level.current += (peak - level.current) * 0.15;
    const lv = level.current;
    spin.current += dt * (0.00012 + lv * 0.0006 + (thinking ? 0.0012 : 0));
    const rot = spin.current;

    // 周波数バー: 低音域を中心に使い、左右対称にして円全体へ配置する
    const half = BARS / 2;
    for (let i = 0; i < half; i++) {
      const bin = spec ? Math.floor(micActive ? (i / half) * (spec.length - 1) : (i / half) ** 1.6 * spec.length * 0.35) : 0;
      const v = spec ? spec[bin] / 255 : 0;
      const prev = bars.current[i];
      bars.current[i] = v > prev ? prev + (v - prev) * 0.5 : prev + (v - prev) * 0.12;
      bars.current[BARS - 1 - i] = bars.current[i];
    }

    ctx.globalCompositeOperation = "lighter";
    ctx.lineCap = "round";

    // 中心の光(音量で膨らむ)
    const glow = ctx.createRadialGradient(cx, cy, 0, cx, cy, base * (1.5 + lv * 1.5));
    glow.addColorStop(0, hsl(H-5, 60, 0.18 + lv * 0.35));
    glow.addColorStop(0.6, hsl(H+15, 55, 0.06 + lv * 0.1));
    glow.addColorStop(1, hsl(H+30, 50, 0));
    ctx.fillStyle = glow;
    ctx.fillRect(0, 0, width, height);

    // 外周の目盛りリング(逆回転)
    ctx.shadowBlur = 0;
    ctx.strokeStyle = hsl(H-7, 70, 0.45);
    const tickR = size * 0.47;
    for (let i = 0; i < 120; i++) {
      const a = (i / 120) * Math.PI * 2 - rot * 0.6;
      const long = i % 10 === 0;
      const r1 = tickR - (long ? size * 0.025 : size * 0.012);
      ctx.lineWidth = (long ? 1.6 : 1) * k;
      ctx.beginPath();
      ctx.moveTo(cx + Math.cos(a) * r1, cy + Math.sin(a) * r1);
      ctx.lineTo(cx + Math.cos(a) * tickR, cy + Math.sin(a) * tickR);
      ctx.stroke();
    }

    // 回転する弧(応答生成中は速く回る)
    ctx.shadowColor = hsl(H-10, 60, 0.9);
    ctx.shadowBlur = 12 * k;
    ctx.lineWidth = 2 * k;
    const arcR = size * 0.42;
    for (let i = 0; i < 3; i++) {
      const a0 = rot * 1.4 + (i * Math.PI * 2) / 3;
      ctx.strokeStyle = hsl(hueAt(i / 2, H), 62, 0.9);
      ctx.beginPath();
      ctx.arc(cx, cy, arcR, a0, a0 + 0.5 + lv * 0.5);
      ctx.stroke();
    }
    ctx.shadowBlur = 0;
    ctx.lineWidth = k;
    ctx.strokeStyle = hsl(H+10, 60, 0.3);
    ctx.setLineDash([2 * k, 6 * k]);
    ctx.beginPath();
    ctx.arc(cx, cy, size * 0.37, rot * 0.5, rot * 0.5 + Math.PI * 2);
    ctx.stroke();
    ctx.setLineDash([]);

    // 放射状の周波数バー
    const maxLen = size * 0.12;
    ctx.lineWidth = Math.max(2 * k, (Math.PI * 2 * (base * 1.15)) / BARS * 0.5);
    for (let i = 0; i < BARS; i++) {
      const a = (i / BARS) * Math.PI * 2 - Math.PI / 2 + rot * 0.3;
      const len = 3 + bars.current[i] * maxLen;
      const r0 = base * 1.15;
      const u = Math.abs(i - half) / half; // 0..1 (下端が中心)
      ctx.strokeStyle = hsl(hueAt(u, H), 62, 0.5 + bars.current[i] * 0.4);
      ctx.shadowColor = hsl(hueAt(u, H), 60, 0.9);
      ctx.shadowBlur = (6 + bars.current[i] * 10) * k;
      ctx.beginPath();
      ctx.moveTo(cx + Math.cos(a) * r0, cy + Math.sin(a) * r0);
      ctx.lineTo(cx + Math.cos(a) * (r0 + len), cy + Math.sin(a) * (r0 + len));
      ctx.stroke();
    }

    // 内側の波形リング
    const breathe = Math.sin(t / 700) * 0.02;
    const r0 = base * (1 + breathe);
    const radiusAt = (i: number) => {
      if (!wave) return r0;
      // 波形を左右対称に巻き付け、始点と終点が滑らかにつながるようにする
      const h = POINTS / 2;
      const j = i < h ? i : POINTS - i;
      const v = (wave[Math.floor((j / h) * (wave.length / 2))] - 128) / 128;
      return r0 + v * base * 0.7;
    };
    ctx.shadowColor = hsl(H-10, 70, 1);
    ctx.shadowBlur = 16 * k;
    ctx.strokeStyle = hsl(H-10, 80, 0.85);
    ctx.lineWidth = 2.5 * k;
    ctx.lineJoin = "round";
    ctx.beginPath();
    for (let i = 0; i <= POINTS; i++) {
      const a = ((i % POINTS) / POINTS) * Math.PI * 2 - Math.PI / 2;
      const r = radiusAt(i % POINTS);
      const x = cx + Math.cos(a) * r;
      const y = cy + Math.sin(a) * r;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.closePath();
    ctx.stroke();

    // 内側の細いガイド円
    ctx.shadowBlur = 0;
    ctx.lineWidth = k;
    ctx.strokeStyle = hsl(H+10, 70, 0.4);
    ctx.beginPath();
    ctx.arc(cx, cy, base * 0.72, 0, Math.PI * 2);
    ctx.stroke();
    ctx.globalCompositeOperation = "source-over";
  });

  return <canvas ref={ref} className={className} />;
}
