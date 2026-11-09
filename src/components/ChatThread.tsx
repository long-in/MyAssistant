"use client";

import { useT } from "@/lib/i18n";
import { useEffect, useRef, useState } from "react";
import { Check, Loader2, Pencil, Square, Volume2, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { addSttCorrection, reportError } from "@/lib/api";
import { Reply, type Turn } from "./Reply";
import TurnLog from "./TurnLog";

/**
 * 会話のスレッド。一般的なAIチャットと同じく、発話(右の吹き出し)、実行ログ、返答(左)の順に並べる。
 * 最新を追いかけ、上へスクロールすると過去を読み返せる。
 */
export default function ChatThread({
  turns,
  busy,
  activity,
  speaking,
  onSpeak,
  onStop,
  onCancelTask,
  onCorrected,
}: {
  turns: Turn[];
  busy: boolean;
  /** 実行中の操作の説明(「ファイルを操作しています…」など)。 */
  activity: string | null;
  /** 読み上げ中か。 */
  speaking?: boolean;
  /** 返答を読み上げる。渡さなければ(読み上げの準備がないとき)、ボタンは出さない。 */
  onSpeak?: (text: string) => void;
  /** 読み上げを止める。 */
  onStop?: () => void;
  /** 実行中の作業を中断する。 */
  onCancelTask: () => void;
  /** 聞き取り訂正を保存したとき、正しい文を渡す。 */
  onCorrected: (text: string) => void;
}) {
  const t = useT();
  const last = turns.length - 1;
  const [editing, setEditing] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const save = async () => {
    const correct = editing?.trim();
    const turn = turns[last];
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
  const lastReply = turns[last]?.reply;
  useEffect(() => {
    follow.current = true;
    const el = box.current;
    if (!el) return;
    const id = setInterval(() => {
      if (follow.current) el.scrollTop = el.scrollHeight;
    }, 60);
    return () => clearInterval(id);
  }, [lastReply, turns.length]);

  return (
    <div
      ref={box}
      onScroll={(e) => {
        const el = e.currentTarget;
        follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 8;
      }}
      className="flex min-h-0 w-full flex-1 flex-col gap-4 overflow-y-auto px-1 pr-2"
      aria-live="polite"
    >
      <div className="flex-1" aria-hidden />
      {turns.length === 0 && <p className="hud-label m-auto text-center text-xs text-muted-foreground">{t("話しかけるか、メッセージを入力してください。")}</p>}
      {turns.map((turn, i) => {
        const isLast = i === last;
        const running = isLast && busy && !turn.reply;
        return (
          <div key={turn.userAt.getTime()} className="flex flex-col gap-2">
            {isLast && editing !== null ? (
              <form
                className="flex w-full max-w-3xl items-center gap-1.5 self-end"
                onSubmit={(e) => {
                  e.preventDefault();
                  void save();
                }}
              >
                <Input autoFocus value={editing} onChange={(e) => setEditing(e.target.value)} onKeyDown={(e) => e.key === "Escape" && setEditing(null)} aria-label={t("正しい発話")} />
                <Button type="submit" size="icon" aria-label={t("訂正を保存")} disabled={saving || !editing.trim()}>
                  <Check />
                </Button>
                <Button type="button" size="icon" variant="outline" aria-label={t("キャンセル")} onClick={() => setEditing(null)}>
                  <X />
                </Button>
              </form>
            ) : (
              <div className="group flex max-w-full items-start gap-1 self-end">
                {isLast && !busy && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-xs"
                    className="mt-1 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
                    aria-label={t("聞き取り間違いを訂正")}
                    title={t("聞き取り間違いを訂正(ナレッジに記録されます)")}
                    onClick={() => setEditing(turn.user)}
                  >
                    <Pencil />
                  </Button>
                )}
                <p className="max-w-3xl min-w-0 rounded-2xl rounded-br-sm bg-primary/10 px-4 py-2 text-sm leading-6 break-words whitespace-pre-wrap">{turn.user}</p>
              </div>
            )}
            {turn.log && <TurnLog log={turn.log} running={running} onCancel={onCancelTask} />}
            {turn.reply && (
              <div className="flex max-w-3xl flex-col items-start gap-1">
                <div className="text-sm leading-6 break-words whitespace-pre-wrap text-foreground">
                  {isLast && !turn.restored ? <Reply text={turn.reply} /> : turn.reply}
                </div>
                {onSpeak && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="xs"
                    className="text-muted-foreground"
                    aria-label={isLast && speaking ? t("読み上げを止める") : t("返答を読み上げる")}
                    title={isLast && speaking ? t("読み上げを止める") : t("返答を読み上げる")}
                    onClick={() => (isLast && speaking ? onStop?.() : onSpeak(turn.reply!))}
                  >
                    {isLast && speaking ? <Square /> : <Volume2 />}
                    {isLast && speaking ? t("停止") : t("読み上げ")}
                  </Button>
                )}
              </div>
            )}
            {running && (
              <p className="flex items-center gap-2 text-xs text-muted-foreground">
                <Loader2 className="size-3.5 animate-spin" />
                {activity ?? <span className="hud-label animate-hud-blink-slow text-[0.65rem]">Processing…</span>}
              </p>
            )}
          </div>
        );
      })}
    </div>
  );
}
