"use client";

import { useT } from "@/lib/i18n";
import { useState } from "react";
import { ChevronDown, Loader2, Square } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { LogEntry, LogState } from "./ExecutionLog";

type NodeState = "pending" | "running" | "done" | "error";

const clock = (d: Date) => d.toLocaleTimeString("ja-JP", { hour12: false });

/** タイムラインの節点。実行中は脈打つ輪、完了は光る点、失敗は赤、未着手は中空。 */
function Node({ state }: { state: NodeState }) {
  return (
    <span
      aria-hidden
      className={cn(
        "absolute top-[0.35rem] -left-[0.3125rem] size-2.5 rounded-full border bg-background",
        state === "pending" && "border-muted-foreground/50",
        state === "running" && "border-primary bg-primary/30 shadow-[0_0_10px_var(--primary)] animate-hud-blink-slow",
        state === "done" && "border-primary bg-primary shadow-[0_0_6px_var(--primary)]",
        state === "error" && "border-destructive bg-destructive shadow-[0_0_6px_var(--destructive)]",
      )}
    />
  );
}

/** 1件の操作。「操作名(対象)」を1行で示し、出力は「⎿」の下にぶら下げる。 */
function Entry({ x }: { x: LogEntry }) {
  const head = (
    <span className="break-all" title={clock(x.at)}>
      <span className={cn("font-semibold", x.status === "error" ? "text-destructive" : "text-foreground")}>{x.label}</span>
      {x.target && (
        <span className="text-muted-foreground">
          (<span className="text-foreground/80">{x.target}</span>)
        </span>
      )}
      {x.status === "running" && <span className="animate-hud-blink text-muted-foreground"> …</span>}
      {x.status === "error" && <span className="text-destructive"> — failed</span>}
    </span>
  );
  if (!x.output) return <p>{head}</p>;
  const pre = (
    <div className="my-0.5 flex gap-1.5 text-foreground/80">
      <span aria-hidden className="shrink-0 text-muted-foreground select-none">
        ⎿
      </span>
      <pre className="max-h-48 min-w-0 flex-1 overflow-auto whitespace-pre-wrap break-all">{x.output}</pre>
    </div>
  );
  // 実行中と失敗は出力を開いたまま、成功したものは畳んで経過を見やすくする
  return x.status === "ok" ? (
    <details>
      <summary className="cursor-pointer list-none">{head}</summary>
      {pre}
    </details>
  ) : (
    <div>
      {head}
      {pre}
    </div>
  );
}

/**
 * 1回の発話に紐づく実行ログ。会話の流れの中で、発話と返答のあいだに置く。
 * 実行中は開いて進捗を見せ、終わると畳む(押せば開き直せる)。作業計画があれば手順ごとの節点に実行内容をぶら下げ、無ければ操作を時系列に並べる。
 * 実行中は中断できる。
 */
export default function TurnLog({ log, running, onCancel }: { log: LogState; running: boolean; onCancel: () => void }) {
  const t = useT();
  // 利用者が開閉するまでは、実行中だけ開く
  const [manual, setManual] = useState<boolean | null>(null);
  const open = manual ?? running;

  if (!log.plan && log.entries.length === 0) return null;

  const total = log.plan?.length ?? 0;
  const stepState = (n: number): NodeState => {
    const mine = log.entries.filter((x) => x.step === n);
    if (mine.some((x) => x.status === "running")) return "running";
    if (mine.some((x) => x.status === "error")) return "error";
    if (mine.length === 0) return "pending";
    // 次の手順へ進んだか、依頼が終わっていれば完了
    return !running || log.entries.some((x) => x.step !== undefined && x.step > n) ? "done" : "running";
  };
  const states = Array.from({ length: total }, (_, i) => stepState(i + 1));
  const finished = states.filter((x) => x === "done").length;
  const current = Math.min(states.findIndex((x) => x === "running" || x === "pending") + 1 || total, total);
  const loose = log.plan ? log.entries.filter((x) => x.step === undefined || x.step > total || x.step < 1) : log.entries;
  const failed = log.entries.some((x) => x.status === "error");

  return (
    <section aria-label={t("実行ログ")} className="w-full rounded-md border border-border/60 bg-muted/20 font-mono text-xs leading-relaxed">
      <div className="flex items-center gap-2 px-3 py-1.5">
        <button
          type="button"
          aria-expanded={open}
          onClick={() => setManual(!open)}
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
        >
          <ChevronDown className={cn("size-3.5 shrink-0 text-muted-foreground transition-transform", !open && "-rotate-90")} />
          {running && <Loader2 className="size-3.5 shrink-0 animate-spin text-primary" />}
          <span className="hud-label truncate text-[0.6rem]">
            {t("実行ログ")}
            <span className="ml-2 text-foreground/80 tabular-nums">
              {total > 0 && (running ? `STEP ${String(current).padStart(2, "0")} / ${String(total).padStart(2, "0")}` : `DONE ${finished} / ${total}`)}
              {total === 0 && t("{n}件", { n: log.entries.length })}
            </span>
            {!running && failed && <span className="ml-2 text-destructive">FAILED</span>}
          </span>
        </button>
        {running && (
          <Button type="button" size="xs" variant="destructive" onClick={onCancel}>
            <Square /> {t("中断")}
          </Button>
        )}
      </div>
      {open && (
        <div className="border-t border-primary/20 px-3 py-2">
          {total > 0 && (
            <div
              role="progressbar"
              aria-valuemin={0}
              aria-valuemax={total}
              aria-valuenow={finished}
              className="mb-2 h-0.5 overflow-hidden bg-primary/15"
            >
              <div
                className="h-full bg-primary shadow-[0_0_8px_var(--primary)] transition-[width] duration-500"
                style={{ width: `${(finished / total) * 100}%` }}
              />
            </div>
          )}
          <div className="max-h-80 overflow-y-auto pr-1 pl-1.5">
            <ol className="relative ml-[0.3125rem] border-l border-primary/30">
              {log.plan?.map((step, i) => {
                const state = states[i];
                return (
                  <li key={i} className="relative pb-2 pl-4 last:pb-0">
                    <Node state={state} />
                    <p
                      className={cn(
                        "flex gap-2",
                        state === "pending" && "text-muted-foreground",
                        state === "running" && "text-primary",
                        state === "done" && "text-foreground/90",
                        state === "error" && "text-destructive",
                      )}
                    >
                      <span className="shrink-0 tabular-nums">{String(i + 1).padStart(2, "0")}</span>
                      <span className="min-w-0 break-words">{step}</span>
                    </p>
                    <div className="mt-0.5 flex flex-col gap-1 pl-6 text-[0.7rem]">
                      {log.entries.filter((x) => x.step === i + 1).map((x) => (
                        <Entry key={x.id} x={x} />
                      ))}
                    </div>
                  </li>
                );
              })}
              {loose.map((x) => (
                <li key={x.id} className="relative pb-2 pl-4 last:pb-0">
                  <Node state={x.status === "running" ? "running" : x.status === "ok" ? "done" : "error"} />
                  <Entry x={x} />
                </li>
              ))}
            </ol>
          </div>
        </div>
      )}
    </section>
  );
}
