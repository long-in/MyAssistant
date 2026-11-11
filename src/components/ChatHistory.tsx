"use client";

import { useEffect, useRef, useState } from "react";
import { History } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useT } from "@/lib/i18n";
import { chatSessionList, type ChatSessionSummary } from "@/lib/api";
import { cn } from "@/lib/utils";

type Props = {
  /** 今の会話のID。一覧で印を付ける。 */
  currentId: () => string;
  /** 選んだ会話へ切り替える。 */
  onResume: (id: string) => void;
};

/** 過去の会話(直近10件)を開いて、選んだ会話へ切り替える。開くたびに読み直す。 */
export default function ChatHistory({ currentId, onResume }: Props) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [items, setItems] = useState<ChatSessionSummary[] | null>(null);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    let live = true;
    chatSessionList()
      .then((r) => live && setItems(r))
      .catch(() => live && setItems([]));
    const away = (e: MouseEvent) => {
      if (box.current && !box.current.contains(e.target as Node)) setOpen(false);
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", away);
    document.addEventListener("keydown", esc);
    return () => {
      live = false;
      document.removeEventListener("mousedown", away);
      document.removeEventListener("keydown", esc);
    };
  }, [open]);

  const now = open ? currentId() : "";
  return (
    <div ref={box} className="relative shrink-0">
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        aria-label={t("会話の履歴")}
        aria-expanded={open}
        title={t("過去の会話に切り替える")}
        onClick={() => setOpen((o) => !o)}
      >
        <History />
      </Button>
      {open && (
        <div role="listbox" aria-label={t("会話の履歴")} className="absolute bottom-full left-0 z-30 mb-2 max-h-80 w-72 overflow-y-auto rounded-md border bg-popover p-1 text-popover-foreground shadow-lg">
          {items === null ? null : items.length === 0 ? (
            <p className="px-2 py-3 text-xs text-muted-foreground">{t("過去の会話はまだありません。")}</p>
          ) : (
            items.map((s) => (
              <button
                key={s.session_id}
                type="button"
                role="option"
                aria-selected={s.session_id === now}
                className={cn("flex w-full flex-col items-start gap-0.5 rounded-sm px-2 py-1.5 text-left hover:bg-accent", s.session_id === now && "bg-primary/10")}
                onClick={() => {
                  setOpen(false);
                  if (s.session_id !== now) onResume(s.session_id);
                }}
              >
                <span className="w-full truncate text-sm">{s.title}</span>
                <span className="text-xs text-muted-foreground">
                  {new Date(s.last_at).toLocaleString()} · {t("{n}往復", { n: s.turns })}
                  {s.session_id === now ? ` · ${t("現在の会話")}` : ""}
                </span>
              </button>
            ))
          )}
        </div>
      )}
    </div>
  );
}
