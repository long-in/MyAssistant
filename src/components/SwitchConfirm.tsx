"use client";

import { useT } from "@/lib/i18n";
import type { Listening } from "./ApprovalModal";

const LISTENING_LABEL: Record<Listening, string> = {
  preparing: "マイクを準備しています…",
  listening: "「はい」「いいえ」でお答えください",
  transcribing: "聞き取っています…",
  idle: "「はい」「いいえ」でお答えください",
};

/** 会話を切り替えてよいかの確認。回答は音声が主で、画面は字幕の位置に小さく出すだけ。判定と録音は呼び出し側で行う。 */
export default function SwitchConfirm({
  listening,
  onAnswer,
}: {
  listening: Listening;
  onAnswer: (yes: boolean) => void;
}) {
  const t = useT();
  return (
    <div
      role="status"
      className="flex shrink-0 items-center gap-2 rounded-full border border-primary/30 bg-card/60 px-3 py-1 text-xs text-muted-foreground backdrop-blur-sm"
    >
      <span className="text-foreground">{t("新しい会話を始めてもよいですか?")}</span>
      <span>{t(LISTENING_LABEL[listening])}</span>
      <button type="button" className="underline-offset-2 hover:underline" onClick={() => onAnswer(true)}>
        {t("はい")}
      </button>
      <button type="button" className="underline-offset-2 hover:underline" onClick={() => onAnswer(false)}>
        {t("いいえ")}
      </button>
    </div>
  );
}
