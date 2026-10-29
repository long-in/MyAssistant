"use client";

import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

const APP_VERSION = process.env.NEXT_PUBLIC_APP_VERSION;

export type Phase = "standby" | "initializing" | "listening" | "decoding" | "thinking" | "speaking";

/** 処理段階ごとの基準色相。円形波形と読み出し表示で共有する。 */
export const PHASE_HUE: Record<Phase, number> = {
  standby: 185, // シアン
  initializing: 45, // アンバー
  listening: 350, // レッド
  decoding: 215, // ブルー
  thinking: 275, // バイオレット
  speaking: 150, // グリーン
};

const PHASES: Record<Phase, { en: string; ja: string }> = {
  standby: { en: "STANDBY", ja: "待機中" },
  initializing: { en: "INITIALIZING", ja: "初期化中" },
  listening: { en: "LISTENING", ja: "音声入力中" },
  decoding: { en: "DECODING", ja: "音声解析中" },
  thinking: { en: "THINKING", ja: "思考中" },
  speaking: { en: "TRANSMITTING", ja: "音声出力中" },
};

/** 円形波形の中心に重ねる、処理段階の読み出し表示。 */
export default function OrbReadout({ phase }: { phase: Phase }) {
  const t = useT();
  const p = PHASES[phase];
  const order = Object.keys(PHASES) as Phase[];
  const color = `hsl(${PHASE_HUE[phase]} 90% 62%)`;
  return (
    <div
      className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center text-center"
      aria-live="polite"
    >
      <span className="font-mono text-[clamp(0.5rem,2.2cqw,0.7rem)] tracking-[0.3em] text-muted-foreground">STATUS</span>
      <span
        className={cn(
          "mt-1 font-mono text-[clamp(0.75rem,5cqw,1.5rem)] font-semibold tracking-[0.25em] transition-colors",
          phase !== "standby" && "animate-hud-blink-slow",
        )}
        style={{ color, textShadow: `0 0 10px ${color}` }}
      >
        {p.en}
      </span>
      <span className="mt-1 text-[clamp(0.6rem,3cqw,0.9rem)] tracking-[0.4em] text-foreground/80">{t(p.ja)}</span>
      <div className="mt-3 flex items-center gap-1" aria-hidden>
        {order.map((k) => (
          <span
            key={k}
            className={cn(
              "h-0.5 w-[3cqw] bg-muted-foreground/30 transition-colors",
            )}
            style={k === phase ? { backgroundColor: color, boxShadow: `0 0 6px ${color}` } : undefined}
          />
        ))}
      </div>
      {APP_VERSION && (
        <span className="mt-3 font-mono text-[clamp(0.45rem,1.8cqw,0.6rem)] tracking-[0.3em] text-muted-foreground/60">
          v{APP_VERSION}
        </span>
      )}
    </div>
  );
}
