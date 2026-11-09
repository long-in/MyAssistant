"use client";

import { Brain, Pencil, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { PaneHeader } from "@/components/GlobalPanels";
import { useT } from "@/lib/i18n";
import { MEMORY_PINNED_PATH, type MemoryView } from "@/lib/api";

/** エージェントの記憶(覚えておくこと)の表示。削除できる。 */
export default function MemoryPanel({
  memory,
  onForget,
  onEdit,
}: {
  memory: MemoryView;
  onForget: (text: string) => void;
  /** 記憶のファイルを通常のノートの画面で開いて編集する。 */
  onEdit: (path: string) => void;
}) {
  const t = useT();
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
                </span>
              </h2>
              {memory.pins.length > 0 && (
                <Button size="sm" variant="ghost" onClick={() => onEdit(MEMORY_PINNED_PATH)}>
                  <Pencil /> {t("ファイルで編集")}
                </Button>
              )}
            </div>
            <p className="mb-3 text-xs text-muted-foreground">
              {t("頼まれたことです。毎回の会話で踏まえます。")}
            </p>
            {memory.pins.length === 0 ? (
              <p className="text-sm text-muted-foreground">{t("まだありません。「これは覚えておいて」と話しかけると記録されます。")}</p>
            ) : (
              <ul className="flex flex-col divide-y rounded-md border">
                {memory.pins.map((p) => (
                  <li key={p.text} className="flex items-start gap-2 px-3 py-2 text-sm">
                    <span className="min-w-0 flex-1 break-words">
                      {p.text}
                      {p.date && <span className="ml-2 text-xs text-muted-foreground">{p.date}</span>}
                    </span>
                    <Button size="icon" variant="ghost" className="size-7 text-destructive" title={t("削除")} onClick={() => onForget(p.text)}>
                      <Trash2 />
                    </Button>
                  </li>
                ))}
              </ul>
            )}
          </section>
        </div>
      </div>
    </div>
  );
}
