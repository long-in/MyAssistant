"use client";

import { useEffect, useState } from "react";
import { useLang, useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { Loader2, Moon, Play, Square, Sun } from "lucide-react";
import { Button } from "@/components/ui/button";

/** 大時計(東京)の下に並べる都市。タイムゾーンは IANA 名で、サマータイムは Intl が反映する。 */
const WORLD_CLOCKS = [
  { name: "香港", zone: "Asia/Hong_Kong" },
  { name: "韓国", zone: "Asia/Seoul" },
  { name: "サンフランシスコ", zone: "America/Los_Angeles" },
  { name: "ドイツ", zone: "Europe/Berlin" },
] as const;

/** 指定ゾーンの「日付(YYYY-MM-DD)・時・分」を取り出す。 */
function zoned(now: Date, zone: string) {
  const parts = new Intl.DateTimeFormat("en-CA", { timeZone: zone, hourCycle: "h23", year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" }).formatToParts(now);
  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? "00";
  return { date: `${get("year")}-${get("month")}-${get("day")}`, hour: Number(get("hour")), minute: get("minute") };
}

/** 一般的な情報を並べるカード。表示する項目は、行を足すだけで増やせる。 */
export default function InfoCard({
  turns,
  modelLabel,
  container,
  className,
}: {
  turns: number;
  modelLabel?: string;
  /** サンドボックスのコンテナの状態と操作(ワークのみ)。 */
  container?: { running: boolean; busy: boolean; disabled: boolean; onToggle: () => void };
  className?: string;
}) {
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
      <p className="hud-heading">{"// Info"}</p>
      <div>
        <p className="text-xs text-muted-foreground">{t("東京")}</p>
        <p className="text-3xl font-semibold tracking-tight tabular-nums">{now ? now.toLocaleTimeString(locale, { hour12: false, timeZone: "Asia/Tokyo" }) : "--:--:--"}</p>
        <p className="text-sm text-muted-foreground">{now ? now.toLocaleDateString(locale, { year: "numeric", month: "long", day: "numeric", weekday: "short", timeZone: "Asia/Tokyo" }) : ""}</p>
      </div>
      <ul className="flex flex-col gap-1 border-y border-border/60 py-2 text-sm" aria-label={t("世界時計")}>
        {WORLD_CLOCKS.map((c) => {
          const z = now ? zoned(now, c.zone) : null;
          const base = now ? zoned(now, "Asia/Tokyo").date : "";
          const diff = z ? (z.date < base ? t("前日") : z.date > base ? t("翌日") : "") : "";
          const day = z ? z.hour >= 6 && z.hour < 18 : true;
          return (
            <li key={c.zone} className="flex items-center gap-2">
              {day ? <Sun className="size-3.5 shrink-0 text-amber-500" aria-hidden /> : <Moon className="size-3.5 shrink-0 text-sky-400" aria-hidden />}
              <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">{t(c.name)}</span>
              <span className="w-12 text-[10px] text-muted-foreground">{diff}</span>
              <span className="font-medium tabular-nums">{z ? `${String(z.hour).padStart(2, "0")}:${z.minute}` : "--:--"}</span>
            </li>
          );
        })}
      </ul>
      <dl className="flex flex-col gap-1.5 text-sm">
        {rows.map((r) => (
          <div key={r.label} className="flex items-baseline justify-between gap-3">
            <dt className="shrink-0 text-xs text-muted-foreground">{r.label}</dt>
            <dd className="min-w-0 truncate text-right">{r.value}</dd>
          </div>
        ))}
      </dl>
      {container && (
        <div className="mt-auto flex flex-col gap-2 rounded-md border border-border/60 bg-background/40 p-3">
          <div className="flex items-center gap-2">
            <span
              aria-hidden
              className={cn("size-2 shrink-0 rounded-full", container.busy ? "animate-pulse bg-amber-500" : container.running ? "bg-emerald-500 shadow-[0_0_6px] shadow-emerald-500/70" : "bg-muted-foreground/50")}
            />
            <span className="hud-label flex-1 text-xs">{t("コンテナ")}</span>
            <span className="text-xs text-muted-foreground">{container.busy ? t("処理中…") : container.running ? t("稼働中") : t("停止中")}</span>
          </div>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={container.busy || container.disabled}
            title={container.disabled && !container.running ? t("イメージのビルドが必要です。Sandbox 設定を確認してください") : undefined}
            className={cn("w-full", container.running ? "border-destructive/50 text-destructive hover:bg-destructive/10 hover:text-destructive" : "border-primary/50 text-primary hover:bg-primary/10")}
            onClick={container.onToggle}
          >
            {container.busy ? <Loader2 className="animate-spin" /> : container.running ? <Square /> : <Play />}
            {container.running ? t("コンテナを停止") : t("コンテナを起動")}
          </Button>
        </div>
      )}
    </section>
  );
}
