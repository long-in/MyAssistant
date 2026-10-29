"use client";

import { useT } from "@/lib/i18n";
import { useEffect, useState } from "react";
import { Volume2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { speechProgress } from "@/lib/api";
import { revealedLength } from "@/lib/speech-sync";

export type Turn = { user: string; userAt: Date; reply?: string; replyAt?: Date; via?: string };

const clock = (d: Date) => d.toLocaleTimeString("ja-JP", { hour12: false });

/** テキストを1文字ずつ表示する。長文でも約2.5秒で出し切る。 */
export function useTypewriter(text: string) {
  const [shown, setShown] = useState({ text, n: 0 });
  // テキストが変わったら先頭から打ち直す(描画中の状態調整)
  if (shown.text !== text) setShown({ text, n: 0 });
  useEffect(() => {
    const per = Math.max(1, Math.ceil(text.length / 80));
    const id = setInterval(() => {
      setShown((s) => (s.n >= text.length ? s : { ...s, n: Math.min(text.length, s.n + per) }));
    }, 30);
    return () => clearInterval(id);
  }, [text]);
  return { text: text.slice(0, shown.n), done: shown.n >= text.length };
}

/**
 * 読み上げ中は、再生位置に合わせて`text`の読み終えた部分までを返す(字幕)。
 * 読み上げていないときはnull。読み上げが終わった(止めた)後は全文を返す。
 */
export function useSpokenReveal(text: string): number | null {
  const [state, setState] = useState<{ text: string; n: number | null; ended: boolean }>({ text, n: null, ended: false });
  if (state.text !== text) setState({ text, n: null, ended: false });
  useEffect(() => {
    const id = setInterval(() => {
      const p = speechProgress(text);
      setState((s) => {
        if (s.text !== text) return s;
        if (p === null) return s.n !== null && !s.ended ? { ...s, ended: true } : s;
        const n = revealedLength(text, p);
        return n === s.n && !s.ended ? s : { ...s, n, ended: false };
      });
    }, 50);
    return () => clearInterval(id);
  }, [text]);
  if (state.ended) return text.length;
  return state.n;
}

export function Reply({ text }: { text: string }) {
  const { text: typed, done } = useTypewriter(text);
  const spoken = useSpokenReveal(text);
  const synced = spoken !== null;
  return (
    <>
      {synced ? text.slice(0, spoken) : typed}
      {(synced ? spoken < text.length : !done) && <span className="animate-hud-blink ml-0.5 inline-block h-3.5 w-1.5 translate-y-0.5 bg-primary" />}
    </>
  );
}

/** 直前のやり取りをターミナル風のログとして表示する。 */
export default function ExchangeLog({
  turn,
  pending,
  canSpeak,
  onSpeak,
}: {
  turn: Turn | null;
  pending: boolean;
  canSpeak: boolean;
  onSpeak: (text: string) => void;
}) {
  const t = useT();
  return (
    <div className="hud-panel pointer-events-auto hidden px-3 py-2 font-mono text-xs leading-relaxed lg:block">
      <div className="mb-2 flex items-center justify-between">
        <span className="hud-label text-[0.6rem]">{t("Last exchange / 直前")}</span>
        {turn?.via && <span className="truncate pl-2 text-[0.6rem] text-muted-foreground">{turn.via}</span>}
      </div>
      {!turn ? (
        <p className="text-muted-foreground">
          &gt; NO DATA<span className="animate-hud-blink">_</span>
        </p>
      ) : (
        <div className="flex max-h-64 flex-col gap-2 overflow-y-auto pr-1">
          <p>
            <span className="text-muted-foreground">[{clock(turn.userAt)}]</span>{" "}
            <span className="text-primary">&gt; USER</span>
            <span className="text-foreground/90"> :: {turn.user}</span>
          </p>
          {turn.reply ? (
            <p className="whitespace-pre-wrap">
              <span className="text-muted-foreground">[{turn.replyAt && clock(turn.replyAt)}]</span>{" "}
              <span className="text-chart-2">&lt; MYASSISTANT</span>
              <span> :: </span>
              {turn.reply}
              {canSpeak && (
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-xs"
                  className="ml-1 align-middle"
                  aria-label={t("もう一度読み上げ")}
                  onClick={() => onSpeak(turn.reply!)}
                >
                  <Volume2 />
                </Button>
              )}
            </p>
          ) : (
            pending && (
              <p className="text-muted-foreground">
                &lt; MYASSISTANT :: PROCESSING<span className="animate-hud-blink">…</span>
              </p>
            )
          )}
        </div>
      )}
    </div>
  );
}
