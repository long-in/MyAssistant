"use client";

import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** 面取りした角(右上・左下を斜めに落とす)。 */
const CHAMFER = "[clip-path:polygon(0_0,calc(100%-14px)_0,100%_14px,100%_100%,14px_100%,0_calc(100%-14px))]";

/** 四隅のブラケット。 */
function Bracket({ className }: { className: string }) {
  return <span aria-hidden className={cn("pointer-events-none absolute size-2.5 border-red-400", className)} />;
}

/** 円形波形の画面に重ねる、強制中断の操作。処理中だけ表示する。 */
export default function AbortButton({ onAbort }: { onAbort: () => void }) {
  const t = useT();
  return (
    <button
      type="button"
      aria-label={t("強制中断")}
      title={t("進行中の処理・読み上げ・録音をすべて止める (Esc)")}
      onClick={onAbort}
      className={cn(
        "group absolute right-4 bottom-4 z-20 w-56 animate-in fade-in slide-in-from-right-6 duration-300",
        "cursor-pointer text-left font-mono outline-none transition-transform active:scale-95",
        "drop-shadow-[0_0_14px_rgba(255,40,70,0.55)] hover:drop-shadow-[0_0_22px_rgba(255,40,70,0.9)] focus-visible:drop-shadow-[0_0_22px_rgba(255,40,70,0.9)]",
      )}
    >
      {/* 外枠: 赤い面取りの板。内側を一回り小さく抜いて縁取りにする */}
      <div className={cn("relative bg-red-500 p-px", CHAMFER)}>
        <div className={cn("relative overflow-hidden bg-[#12060a]/90 backdrop-blur-md transition-colors group-hover:bg-red-950/90", CHAMFER)}>
          {/* 上端: 流れる警告帯 */}
          <div
            aria-hidden
            className="animate-hud-hazard h-1.5 [background:repeating-linear-gradient(135deg,#ef4444_0_7px,#12060a_7px_14px)] [background-size:28px_100%]"
          />
          {/* 走査線 */}
          <div
            aria-hidden
            className="animate-hud-scan pointer-events-none absolute inset-x-0 top-0 h-6 bg-gradient-to-b from-transparent via-red-500/20 to-transparent"
          />
          <div className="relative flex items-center gap-3 px-3.5 py-3">
            {/* 左: 照準風のアイコン */}
            <svg viewBox="0 0 40 40" aria-hidden className="size-11 shrink-0 text-red-400 transition-transform duration-500 group-hover:rotate-90">
              <circle cx="20" cy="20" r="15" fill="none" stroke="currentColor" strokeWidth="1.5" strokeDasharray="5 3" className="origin-center animate-[spin_8s_linear_infinite]" />
              <circle cx="20" cy="20" r="9" fill="none" stroke="currentColor" strokeWidth="1" opacity=".6" />
              <path d="M20 1v8M20 31v8M1 20h8M31 20h8" stroke="currentColor" strokeWidth="1.5" />
              <rect x="16" y="16" width="8" height="8" className="fill-red-500 animate-hud-blink-slow" />
            </svg>
            <div className="min-w-0 flex-1">
              <div className="text-[0.6rem] tracking-[0.35em] text-red-400/80">EMERGENCY</div>
              <div className="text-lg leading-tight font-bold tracking-[0.2em] text-red-50 [text-shadow:0_0_10px_rgb(239_68_68)]">
                TERMINATE
              </div>
              <div className="text-[0.7rem] tracking-[0.5em] text-red-200/80">{t("強制中断")}</div>
            </div>
          </div>
          {/* 下端: 操作キーの表示 */}
          <div className="relative flex items-center justify-between border-t border-red-500/40 px-3.5 py-1 text-[0.6rem] tracking-[0.3em] text-red-300/80">
            <span className="animate-hud-blink-slow">● ARMED</span>
            <span>
              [ <span className="text-red-100">ESC</span> ]
            </span>
          </div>
        </div>
      </div>
      <Bracket className="-top-1 -left-1 border-t border-l" />
      <Bracket className="-right-1 -bottom-1 border-r border-b" />
    </button>
  );
}
