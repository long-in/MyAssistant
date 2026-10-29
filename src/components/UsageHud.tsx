"use client";

import { useT } from "@/lib/i18n";
import { type UsageRow } from "@/lib/api";
import { estimateCost, formatUsd } from "@/lib/pricing";

const compact = (n: number) =>
  n >= 1_000_000 ? `${(n / 1_000_000).toFixed(1)}M` : n >= 1_000 ? `${(n / 1_000).toFixed(1)}k` : String(n);

/** 波形エリア右上に出す、当月のトークン使用量と推定コスト。このアプリ経由の呼び出しのローカル集計。 */
export default function UsageHud({ rows }: { rows: UsageRow[] }) {
  const t = useT();
  const costs = rows.map((r) => estimateCost(r.model, r.input_tokens, r.output_tokens));
  const total = costs.reduce<number>((a, c) => a + (c ?? 0), 0);
  const unknown = costs.some((c) => c === null);
  return (
    <div
      className="hud-panel absolute right-4 top-4 min-w-56 px-3 py-2 text-xs"
      title={t("このアプリ経由の当月分(ローカル集計)。単価は公開価格に基づく概算で、請求額とは一致しない場合があります。")}
    >
      <div className="mb-1.5 flex items-center justify-between">
        <span className="hud-label text-[0.6rem]">{t("Usage / 今月")}</span>
        <span className="hud-label text-[0.6rem]">{rows.length ? `${formatUsd(total)}${unknown ? "+" : ""}` : "$0.00"}</span>
      </div>
      {rows.length === 0 ? (
        <p className="text-muted-foreground">{t("まだ利用なし")}</p>
      ) : (
        <ul className="flex flex-col gap-1.5">
          {rows.map((r, i) => (
            <li key={`${r.provider}/${r.model}`}>
              <div className="flex items-center justify-between gap-3 font-mono text-[0.7rem]">
                <span className="truncate">{r.model}</span>
                <span className="text-primary">{costs[i] === null ? "—" : formatUsd(costs[i]!)}</span>
              </div>
              <div className="font-mono text-[0.65rem] text-muted-foreground">
                {r.requests} req · in {compact(r.input_tokens)} · out {compact(r.output_tokens)}
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
