"use client";

import { useT } from "@/lib/i18n";
import { useEffect, useRef, useState } from "react";
import { Check, Pencil, Square, Volume2, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { addSttCorrection, reportError } from "@/lib/api";
import { Reply, type Turn } from "./Reply";

/** 会話の流れを字幕のように表示する。過去のやり取りは上に薄く積み、現在の回答は末尾を追いかける。上へスクロールすると過去を読み返せる。 */
export default function Subtitles({
  turns,
  pending,
  onCorrected,
  speaking,
  onSpeak,
  onStop,
}: {
  turns: Turn[];
  pending: boolean;
  /** 聞き取り訂正を保存したとき、正しい文を渡す。 */
  onCorrected: (text: string) => void;
  /** 読み上げ中か。 */
  speaking?: boolean;
  /** 返答を読み上げる。渡さなければ(読み上げの準備がないとき)、ボタンは出さない。 */
  onSpeak?: (text: string) => void;
  /** 読み上げを止める。 */
  onStop?: () => void;
}) {
  const t = useT();
  const turn = turns.length ? turns[turns.length - 1] : null;
  const past = turns.slice(0, -1);
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
  }, [reply, turns.length]);

  return (
    <div
      ref={box}
      onScroll={(e) => {
        const el = e.currentTarget;
        follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 8;
      }}
      className="flex min-h-0 w-full max-w-3xl flex-col items-center gap-1.5 overflow-y-auto text-center [mask-image:linear-gradient(to_bottom,transparent,black_2rem)]"
      aria-live="polite"
    >
      <div className="flex-1" aria-hidden />
      {past.map((p) => (
        <PastTurn key={p.userAt.getTime()} turn={p} onToggle={() => (follow.current = false)} />
      ))}
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
      {reply && (
        <div className="rounded-sm bg-background/60 px-4 py-1.5 text-base leading-6 text-foreground [text-shadow:0_0_8px_var(--background)] backdrop-blur-sm">
          <Reply text={reply} />
        </div>
      )}
      {reply && onSpeak && (
        <Button
          type="button"
          variant="ghost"
          size="xs"
          className="text-muted-foreground"
          aria-label={speaking ? t("読み上げを止める") : t("返答を読み上げる")}
          title={speaking ? t("読み上げを止める") : t("返答を読み上げる")}
          onClick={() => (speaking ? onStop?.() : onSpeak(reply))}
        >
          {speaking ? <Square /> : <Volume2 />}
          {speaking ? t("停止") : t("読み上げ")}
        </Button>
      )}
      {!reply && pending && <p className="hud-label animate-hud-blink-slow text-[0.65rem]">Processing…</p>}
    </div>
  );
}

/** 過去のやり取り1件。返答は3行に絞って薄く表示し、クリックで全文を開閉する。 */
function PastTurn({ turn, onToggle }: { turn: Turn; onToggle: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <div className={`flex w-full flex-col items-center gap-1 opacity-60 transition-opacity hover:opacity-100`}>
      <p className="max-w-full truncate text-xs text-muted-foreground">
        <span className="text-primary">&#9656;</span> {turn.user}
      </p>
      {turn.reply && (
        <button
          type="button"
          aria-expanded={open}
          onClick={() => {
            onToggle();
            setOpen((o) => !o);
          }}
          className={`cursor-pointer rounded-sm bg-background/40 px-4 py-1 text-sm leading-5 text-foreground ${open ? "" : "line-clamp-3"}`}
        >
          {turn.reply}
        </button>
      )}
    </div>
  );
}
