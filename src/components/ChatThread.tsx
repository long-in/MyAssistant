"use client";

import { useT } from "@/lib/i18n";
import { useEffect, useRef, useState } from "react";
import { Check, Pencil, RotateCw, Square, Volume2, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { addSttCorrection, reportError } from "@/lib/api";
import { cn } from "@/lib/utils";
import { ChatMarkdown, CopyButton } from "./ChatMarkdown";
import { Reply, type Turn } from "./Reply";
import TurnLog from "./TurnLog";

/**
 * 会話のスレッド。コーディング支援のチャット(VS Code の Claude Code 拡張)のように、1カラムで、発話(枠つき)、実行ログ、返答(丸印つき)の順に並べる。
 * 最新を追いかけ、上へスクロールすると過去を読み返せる。
 */
/** 回答にかかった時間。1分未満は小数1桁の秒、それ以上は分と秒で表す。 */
function elapsed(ms: number, t: (key: string, params?: Record<string, string | number>) => string): string {
  const sec = Math.max(0, ms) / 1000;
  if (sec < 60) return t("{s}秒", { s: sec.toFixed(1) });
  const total = Math.round(sec);
  return t("{m}分{s}秒", { m: Math.floor(total / 60), s: total % 60 });
}

export default function ChatThread({
  turns,
  busy,
  activity,
  speaking,
  onSpeak,
  onStop,
  onCancelTask,
  onCorrected,
  onRetry,
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
  /** 送信に失敗した発話を送り直す。 */
  onRetry: (turn: Turn) => void;
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
      className="flex min-h-0 w-full flex-1 flex-col overflow-y-auto px-1 pr-2"
      aria-live="polite"
    >
      <div className="flex-1" aria-hidden />
      {turns.length === 0 && <p className="m-auto text-center text-sm text-muted-foreground">{t("話しかけるか、メッセージを入力してください。")}</p>}
      <div className="mx-auto flex w-full max-w-3xl flex-col gap-5 py-2">
        {turns.map((turn, i) => {
          const isLast = i === last;
          const running = isLast && busy && !turn.reply;
          return (
            <div key={turn.userAt.getTime()} className="flex flex-col gap-3">
              {/* 発話: 枠で囲んだ1ブロック。重ねると、訂正とコピーのボタンが出る */}
              {isLast && editing !== null ? (
                <form
                  className="flex w-full items-center gap-1.5"
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
                <div className={cn("group relative rounded-md border bg-muted/40 px-3 py-2", turn.failed && "border-destructive/50")}>
                  <p className="min-w-0 text-sm leading-6 break-words whitespace-pre-wrap">{turn.user}</p>
                  <div className="absolute top-1 right-1 flex items-center rounded bg-background/80 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                    {isLast && !busy && !turn.failed && (
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
                    )}
                    <CopyButton text={turn.user} />
                  </div>
                  {turn.failed && (
                    <div className="mt-2 flex items-center gap-2 text-xs text-destructive">
                      <span>{t("送信できませんでした。")}</span>
                      <Button type="button" size="xs" variant="outline" disabled={busy} onClick={() => onRetry(turn)}>
                        <RotateCw /> {t("再送")}
                      </Button>
                    </div>
                  )}
                </div>
              )}
              {turn.log && <TurnLog log={turn.log} running={running} onCancel={onCancelTask} />}
              {/* 返答: 先頭に丸印を置き、本文はMarkdownで描く。下に、コピー・読み上げ・かかった時間 */}
              {turn.reply && (
                <div className="group flex gap-2.5">
                  <span aria-hidden className="mt-2 size-2 shrink-0 rounded-full bg-primary" />
                  <div className="flex min-w-0 flex-1 flex-col gap-1">
                    <div className="max-w-full min-w-0 text-sm leading-6 break-words text-foreground">
                      {isLast && !turn.restored ? <Reply text={turn.reply} /> : <ChatMarkdown text={turn.reply} />}
                    </div>
                    <div className="-ml-1.5 flex items-center gap-1 text-muted-foreground">
                      <CopyButton text={turn.reply} label={t("コピー")} />
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
                      {turn.replyAt && (
                        <span className="px-1 text-xs tabular-nums" title={t("回答にかかった時間")}>
                          {elapsed(turn.replyAt.getTime() - turn.userAt.getTime(), t)}
                        </span>
                      )}
                      {turn.via && <span className="truncate px-1 font-mono text-[0.65rem] opacity-70">{turn.via}</span>}
                    </div>
                  </div>
                </div>
              )}
              {running && (
                <p className="flex items-center gap-2.5 text-xs text-muted-foreground">
                  <span aria-hidden className="size-2 shrink-0 animate-hud-blink-slow rounded-full bg-primary" />
                  {activity ?? <span className="animate-hud-blink-slow">{t("考えています…")}</span>}
                </p>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
