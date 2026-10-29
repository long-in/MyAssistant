"use client";

import { useT } from "@/lib/i18n";
import { useEffect, useRef, useState } from "react";
import { Check, Pencil, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { addSttCorrection, reportError } from "@/lib/api";
import { Reply, type Turn } from "./ExchangeLog";

/** 入力と回答を字幕のように表示する。回答は末尾を追いかけ、長文は上へスクロールして全文を読める。 */
export default function Subtitles({
  turn,
  pending,
  onCorrected,
}: {
  turn: Turn | null;
  pending: boolean;
  /** 聞き取り訂正を保存したとき、正しい文を渡す。 */
  onCorrected: (text: string) => void;
}) {
  const t = useT();
  const [editing, setEditing] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const save = async () => {
    const correct = editing?.trim();
    if (!turn || !correct) return;
    if (correct === turn.user) return setEditing(null);
    setSaving(true);
    try {
      await addSttCorrection(turn.user, correct);
      onCorrected(correct);
      setEditing(null);
      toast.success(t("訂正をナレッジに記録しました。"));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setSaving(false);
    }
  };

  const box = useRef<HTMLDivElement>(null);
  /** 末尾を追従中か。利用者が上へスクロールしたら止める。 */
  const follow = useRef(true);
  const reply = turn?.reply;
  useEffect(() => {
    follow.current = true;
    const el = box.current;
    if (!el) return;
    const id = setInterval(() => {
      if (follow.current) el.scrollTop = el.scrollHeight;
    }, 60);
    return () => clearInterval(id);
  }, [reply]);

  return (
    <div className="flex min-h-0 w-full max-w-3xl flex-col items-center justify-end gap-1.5 text-center" aria-live="polite">
      {turn && editing === null && (
        <p className="flex max-w-full items-center gap-1 text-xs text-muted-foreground [text-shadow:0_0_6px_var(--background)]">
          <span className="truncate">
            <span className="text-primary">&#9656;</span> {turn.user}
          </span>
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            aria-label={t("聞き取り間違いを訂正")}
            title={t("聞き取り間違いを訂正(ナレッジに記録されます)")}
            onClick={() => setEditing(turn.user)}
          >
            <Pencil />
          </Button>
        </p>
      )}
      {turn && editing !== null && (
        <form
          className="flex w-full max-w-xl items-center gap-1.5"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <Input
            autoFocus
            value={editing}
            onChange={(e) => setEditing(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && setEditing(null)}
            aria-label={t("正しい発話")}
          />
          <Button type="submit" size="icon" aria-label={t("訂正を保存")} disabled={saving || !editing.trim()}>
            <Check />
          </Button>
          <Button type="button" size="icon" variant="outline" aria-label={t("キャンセル")} onClick={() => setEditing(null)}>
            <X />
          </Button>
        </form>
      )}
      {reply ? (
        <div
          ref={box}
          onScroll={(e) => {
            const el = e.currentTarget;
            follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 8;
          }}
          className="min-h-0 overflow-y-auto rounded-sm bg-background/60 px-4 py-1.5 text-base leading-6 text-foreground [text-shadow:0_0_8px_var(--background)] backdrop-blur-sm"
        >
          <Reply text={reply} />
        </div>
      ) : (
        pending && (
          <p className="hud-label animate-hud-blink-slow text-[0.65rem]">Processing…</p>
        )
      )}
    </div>
  );
}
