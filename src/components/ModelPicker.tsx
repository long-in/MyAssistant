"use client";

import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown, Sparkles } from "lucide-react";
import { useT } from "@/lib/i18n";
import { MODEL_PROVIDER_LABELS, type ModelInfo, type ModelKeyStatus, type ModelProvider, type Route } from "@/lib/api";
import { cn } from "@/lib/utils";

type Props = {
  route: Route | null;
  models: ModelInfo[];
  keyStatus: ModelKeyStatus;
  onChange: (route: Route) => void;
  /** 入力欄の中に置く小さな見た目(枠なし)。 */
  compact?: boolean;
  /** 応答の生成中など、切り替えさせないとき。 */
  disabled?: boolean;
  /** プロバイダの見出しを出さず、一覧だけを並べる(サンドボックスのモデル)。 */
  flat?: boolean;
};

/**
 * チャットのモデル切り替え。VS Codeの拡張機能のチャットのように、現在のモデルを示すボタンを押すと、
 * プロバイダごとに分けたモデルの一覧が開き、選ぶとすぐ切り替わる。
 */
export default function ModelPicker({ route, models, keyStatus, onChange, compact, disabled, flat }: Props) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [rect, setRect] = useState<DOMRect | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const usable = models.filter((m) => keyStatus[m.provider]);
  const providers = [...new Set(usable.map((m) => m.provider))] as ModelProvider[];
  const current = usable.find((m) => m.provider === route?.provider && m.id === route?.model);

  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    const key = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("resize", close);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("resize", close);
      window.removeEventListener("keydown", key);
    };
  }, [open]);

  if (!route) return null;
  const toggle = () => {
    setRect(trigger.current?.getBoundingClientRect() ?? null);
    setOpen((o) => !o);
  };
  // ボタンの真上に、ボタンと同じ幅(最低16rem)で開く。画面の左端からはみ出さないよう寄せる
  const width = Math.max(rect?.width ?? 0, 256);
  const style: React.CSSProperties | undefined = rect
    ? { left: Math.max(8, Math.min(rect.left, window.innerWidth - width - 8)), bottom: window.innerHeight - rect.top + 6, width }
    : undefined;

  return (
    <>
      <button
        ref={trigger}
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={t("モデル")}
        disabled={disabled}
        onClick={toggle}
        className={cn(
          "flex items-center gap-1.5 text-left text-xs transition-colors hover:bg-accent/50 disabled:pointer-events-none disabled:opacity-50",
          compact ? "max-w-full rounded-md px-2 py-1 text-muted-foreground hover:text-foreground" : "w-full rounded-md border bg-background/60 px-2.5 py-1.5",
        )}
      >
        <Sparkles className="size-3.5 shrink-0 text-primary" />
        <span className="min-w-0 flex-1 truncate">{current?.label ?? route.model}</span>
        <ChevronDown className={cn("size-3.5 shrink-0 text-muted-foreground transition-transform", open && "rotate-180")} />
      </button>
      {open &&
        createPortal(
          <>
            <div className="fixed inset-0 z-50" onClick={() => setOpen(false)} />
            <div role="listbox" style={style} className="fixed z-50 max-h-80 overflow-y-auto rounded-md border bg-popover p-1 text-popover-foreground shadow-lg">
              {providers.map((p) => (
                <div key={p} className="py-1">
                  {!flat && <div className="px-2 py-1 text-[0.65rem] font-medium tracking-wide text-muted-foreground">{t(MODEL_PROVIDER_LABELS[p])}</div>}
                  {usable
                    .filter((m) => m.provider === p)
                    .map((m) => {
                      const selected = m.provider === route.provider && m.id === route.model;
                      return (
                        <button
                          key={m.id}
                          type="button"
                          role="option"
                          aria-selected={selected}
                          onClick={() => {
                            setOpen(false);
                            if (!selected) onChange({ provider: m.provider, model: m.id });
                          }}
                          className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs hover:bg-accent hover:text-accent-foreground"
                        >
                          <span className="min-w-0 flex-1 truncate">{m.label}</span>
                          {selected && <Check className="size-3.5 shrink-0 text-primary" />}
                        </button>
                      );
                    })}
                </div>
              ))}
              {providers.length === 0 && <p className="px-2 py-2 text-xs text-muted-foreground">{t("使えるモデルがありません。設定でAPIキーを登録してください。")}</p>}
            </div>
          </>,
          document.body,
        )}
    </>
  );
}
