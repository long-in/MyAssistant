"use client";

import { Brain, Check, Pencil, Trash2 } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { PaneHeader } from "@/components/GlobalPanels";
import { useT } from "@/lib/i18n";
import { MEMORY_PINNED_PATH, MEMORY_UNDERSTANDING_PATH, type MemoryView } from "@/lib/api";

/** エージェントの記憶(覚えておくこと・担当の理解)の表示。推定で覚えたものは、確定・削除できる。 */
export default function MemoryPanel({
  memory,
  onConfirm,
  onForget,
  onEdit,
}: {
  memory: MemoryView;
  onConfirm: (text: string) => void;
  onForget: (text: string) => void;
  /** 記憶のファイルを通常のノートの画面で開いて編集する。 */
  onEdit: (path: string) => void;
}) {
  const t = useT();
  const inferred = memory.pins.filter((p) => p.inferred).length;
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PaneHeader icon={Brain} title="Memory" />
      <div className="flex-1 overflow-y-auto px-10 py-6">
        <div className="mx-auto flex max-w-3xl flex-col gap-8">
          <section>
            <div className="mb-2 flex items-center justify-between gap-2">
              <h2 className="text-sm font-semibold">
                {t("覚えておくこと")}
                <span className="ml-2 text-xs font-normal text-muted-foreground">
                  {t("{n}件", { n: memory.pins.length })}
                  {inferred > 0 && ` · ${t("推定 {n}件", { n: inferred })}`}
                </span>
              </h2>
              {memory.pins.length > 0 && (
                <Button size="sm" variant="ghost" onClick={() => onEdit(MEMORY_PINNED_PATH)}>
                  <Pencil /> {t("ファイルで編集")}
                </Button>
              )}
            </div>
            <p className="mb-3 text-xs text-muted-foreground">
              {t("頼まれたことと、会話から大事だと判断したことです。毎回の会話で踏まえます。「推定」は会話から推測したもので、確定するか削除してください。")}
            </p>
            {memory.pins.length === 0 ? (
              <p className="text-sm text-muted-foreground">{t("まだありません。「これは覚えておいて」と話しかけると記録されます。")}</p>
            ) : (
              <ul className="flex flex-col divide-y rounded-md border">
                {memory.pins.map((p) => (
                  <li key={p.text} className="flex items-start gap-2 px-3 py-2 text-sm">
                    <span className="min-w-0 flex-1 break-words">
                      {p.inferred && (
                        <Badge variant="outline" className="mr-2 border-amber-500/60 text-amber-600 dark:text-amber-400">
                          {t("推定")}
                        </Badge>
                      )}
                      {p.text}
                      {p.date && <span className="ml-2 text-xs text-muted-foreground">{p.date}</span>}
                    </span>
                    {p.inferred && (
                      <Button size="icon" variant="ghost" className="size-7" title={t("確定する")} onClick={() => onConfirm(p.text)}>
                        <Check />
                      </Button>
                    )}
                    <Button size="icon" variant="ghost" className="size-7 text-destructive" title={t("削除")} onClick={() => onForget(p.text)}>
                      <Trash2 />
                    </Button>
                  </li>
                ))}
              </ul>
            )}
          </section>
          <section>
            <div className="mb-2 flex items-center justify-between gap-2">
              <h2 className="text-sm font-semibold">{t("担当の理解")}</h2>
              {memory.understanding && (
                <Button size="sm" variant="ghost" onClick={() => onEdit(MEMORY_UNDERSTANDING_PATH)}>
                  <Pencil /> {t("編集")}
                </Button>
              )}
            </div>
            <p className="mb-3 text-xs text-muted-foreground">
              {t("会話が途切れるたびに書き直される、担当業務についての理解です。毎回の会話で踏まえます。手で直した内容を前提に、次の更新が行われます。")}
            </p>
            {memory.understanding ? (
              <div className="prose prose-sm max-w-none rounded-md border px-4 py-3 dark:prose-invert">
                <ReactMarkdown remarkPlugins={[remarkGfm]}>{memory.understanding}</ReactMarkdown>
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">{t("まだありません。会話を重ねると、ここに担当業務の理解が育っていきます。")}</p>
            )}
          </section>
        </div>
      </div>
    </div>
  );
}
