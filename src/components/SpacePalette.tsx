"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { createPortal } from "react-dom";
import { Search } from "lucide-react";
import { toast } from "sonner";
import { LEADER_COLOR, LEADER_ID, LEADER_NAME, PERSONAL_ID, errorMessage } from "@/lib/api";
import { copyTargets } from "@/lib/copyTargets";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { switchTo, useWorkspace } from "@/lib/workspace";
import SpaceChip from "./SpaceChip";

/**
 * スペースの検索パレット(⌘K)。名称で絞り込み、Enterかクリックで切り替える。
 * 呼び出し元の`backdrop-filter`・`clip-path`に切り取られないよう、bodyへ描画する。
 */
export default function SpacePalette({ onClose }: { onClose: () => void }) {
  const t = useT();
  const router = useRouter();
  const { active, spaces, workOn } = useWorkspace();
  const [query, setQuery] = useState("");
  const [cursor, setCursor] = useState(0);
  const listRef = useRef<HTMLUListElement>(null);

  const items = useMemo(() => {
    const q = query.trim().toLowerCase();
    // パーソナルは、表示の言語によらず日本語・英語のどちらでも見つかるようにする
    const names = (x: { id: string; name: string }) => (x.id === PERSONAL_ID ? [x.name, "パーソナル", "Personal"] : [x.name]);
    // リーダー(別枠)は、パーソナルの次に出す
    const all = copyTargets(spaces);
    if (workOn) all.splice(1, 0, { id: LEADER_ID, name: LEADER_NAME, color: LEADER_COLOR });
    return all.filter((x) => names(x).some((n) => n.toLowerCase().includes(q)));
  }, [spaces, workOn, query]);
  const at = Math.min(cursor, Math.max(items.length - 1, 0));

  useEffect(() => {
    listRef.current?.children[at]?.scrollIntoView({ block: "nearest" });
  }, [at]);

  const pick = (id: string) => {
    onClose();
    // 行き先は、そのスペースのチャット。現在のスペースを選んだときも、チャットへ移る
    router.push("/");
    if (id !== active.id) switchTo(id).catch((e) => toast.error(errorMessage(e)));
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.nativeEvent.isComposing) return; // 変換の確定のEnterで切り替えない
    // 対話画面のEsc(強制中断)などへ伝えない
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (items.length) setCursor((at + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (items[at]) pick(items[at].id);
    }
  };

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-background/40 p-6 pt-[15vh] backdrop-blur-sm"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div role="dialog" aria-modal="true" aria-label={t("スペースの検索")} className="w-full max-w-md overflow-hidden rounded-lg border bg-popover text-popover-foreground shadow-xl">
        <div className="flex items-center gap-2 border-b px-3">
          <Search className="size-4 shrink-0 text-muted-foreground" />
          <input
            autoFocus
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setCursor(0);
            }}
            onKeyDown={onKeyDown}
            placeholder={t("スペースを検索…")}
            aria-label={t("スペースの検索")}
            className="h-11 w-full bg-transparent text-sm outline-none placeholder:text-muted-foreground"
          />
        </div>
        {items.length === 0 ? (
          <p className="px-3 py-6 text-center text-sm text-muted-foreground">{t("該当するスペースがありません")}</p>
        ) : (
          <ul ref={listRef} className="max-h-72 overflow-y-auto p-1">
            {items.map((x, i) => (
              <li key={x.id}>
                <button
                  type="button"
                  onClick={() => pick(x.id)}
                  onMouseMove={() => setCursor(i)}
                  className={cn("flex w-full items-center justify-between gap-3 rounded-md px-3 py-2 text-left text-sm", i === at && "bg-primary/10")}
                >
                  <SpaceChip name={x.name} color={x.color} />
                  {x.id === active.id && <span className="shrink-0 text-xs text-muted-foreground">{t("現在")}</span>}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>,
    document.body,
  );
}
