"use client";

import { useEffect, useState } from "react";
import { speechProgress } from "@/lib/api";
import { revealedLength } from "@/lib/speech-sync";

export type Turn = { user: string; userAt: Date; reply?: string; replyAt?: Date; via?: string };

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
