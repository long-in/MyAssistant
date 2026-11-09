"use client";

import { useEffect, useState } from "react";
import { useLang, useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** 一般的な情報を並べるカード。表示する項目は、行を足すだけで増やせる。 */
export default function InfoCard({ turns, modelLabel, className }: { turns: number; modelLabel?: string; className?: string }) {
  const t = useT();
  const locale = useLang() === "ja" ? "ja-JP" : "en-US";
  // 時計は秒ごとに更新する。サーバー描画との差を避けるため、表示は読み込み後
  const [now, setNow] = useState<Date | null>(null);
  useEffect(() => {
    const tick = () => setNow(new Date());
    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, []);

  const rows: { label: string; value: string }[] = [
    { label: t("会話"), value: t("{n}往復", { n: turns }) },
    ...(modelLabel ? [{ label: t("モデル"), value: modelLabel }] : []),
  ];

  return (
    <section className={cn("hud-panel hud-accent flex min-h-0 flex-col gap-3 overflow-y-auto px-5 py-4", className)} aria-label={t("情報")}>
      <p className="hud-label">{"// Info"}</p>
      <div>
        <p className="text-3xl font-semibold tracking-tight tabular-nums">{now ? now.toLocaleTimeString(locale, { hour12: false }) : "--:--:--"}</p>
        <p className="text-sm text-muted-foreground">{now ? now.toLocaleDateString(locale, { year: "numeric", month: "long", day: "numeric", weekday: "short" }) : ""}</p>
      </div>
      <dl className="flex flex-col gap-1.5 text-sm">
        {rows.map((r) => (
          <div key={r.label} className="flex items-baseline justify-between gap-3">
            <dt className="shrink-0 text-xs text-muted-foreground">{r.label}</dt>
            <dd className="min-w-0 truncate text-right">{r.value}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}
