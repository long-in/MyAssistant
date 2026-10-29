"use client";

import { tr, useT } from "@/lib/i18n";
import { useEffect, useRef } from "react";
import { Square, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { AgentEvent } from "@/lib/api";

/** 1エントリに保持する出力の最大文字数。超えたら古い方を捨てる。 */
const OUTPUT_MAX_CHARS = 20_000;

export type LogEntry = {
  id: string;
  /** コマンドなら実行したコマンド、それ以外は操作の説明。 */
  title: string;
  /** 承認済みの計画の手順番号(1始まり)。 */
  step?: number;
  isCommand: boolean;
  output: string;
  status: "running" | "ok" | "error";
  at: Date;
  endedAt?: Date;
};

export type LogState = {
  /** 提示された作業計画の手順。承認されたかは問わない。 */
  plan: string[] | null;
  entries: LogEntry[];
};

export const emptyLog: LogState = { plan: null, entries: [] };

/** ナレッジのファイル操作の表示名。 */
const TOOL_LABELS: Record<string, string> = {
  list_files: "一覧",
  read_file: "読み取り",
  search_files: "検索",
  create_file: "作成",
};

const clock = (d: Date) => d.toLocaleTimeString("ja-JP", { hour12: false });

const str = (v: unknown) => (typeof v === "string" ? v : "");

/** エージェントの進捗イベントを実行ログへ反映する。ログに関係しないイベントはそのまま返す。 */
export function applyAgentEvent(log: LogState, e: AgentEvent): LogState {
  if (e.type === "tool_call") {
    if (e.name === "propose_plan") {
      const steps = Array.isArray(e.input.steps) ? e.input.steps.filter((s): s is string => typeof s === "string") : [];
      return { plan: steps, entries: log.entries };
    }
    const isCommand = e.name === "run_command";
    const title = isCommand
      ? str(e.input.command)
      : `${tr(TOOL_LABELS[e.name] ?? e.name)} ${str(e.input.path) || str(e.input.query)}`.trim();
    const step = typeof e.input.step === "number" ? e.input.step : undefined;
    return { ...log, entries: [...log.entries, { id: e.id, title, step, isCommand, output: "", status: "running", at: new Date() }] };
  }
  const update = (id: string, fn: (x: LogEntry) => LogEntry): LogState => ({
    ...log,
    entries: log.entries.map((x) => (x.id === id ? fn(x) : x)),
  });
  if (e.type === "command_output") {
    return update(e.id, (x) => ({ ...x, output: (x.output + e.text).slice(-OUTPUT_MAX_CHARS) }));
  }
  if (e.type === "tool_result") {
    // コマンドは出力を逐次受け取っている。拒否など出力が無い結果だけ本文を使う
    return update(e.id, (x) => ({ ...x, status: e.ok ? "ok" : "error", output: x.output || e.preview, endedAt: new Date() }));
  }
  return log;
}

type NodeState = "pending" | "running" | "done" | "error";

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

function Entry({ x }: { x: LogEntry }) {
  const label = x.isCommand ? "EXEC" : "FILE";
  const head = (
    <span className="break-all">
      <span className="text-muted-foreground">[{clock(x.at)}]</span>{" "}
      <span className={x.status === "error" ? "text-destructive" : "text-primary"}>&gt; {label}</span>
      <span className="text-foreground/90"> :: {x.title}</span>
      {x.status === "running" && <span className="animate-hud-blink text-muted-foreground"> …</span>}
      {x.status === "error" && <span className="text-destructive"> &lt; FAILED</span>}
    </span>
  );
  if (!x.output) return <p>{head}</p>;
  const pre = (
    <pre className="my-0.5 max-h-48 overflow-auto border-l border-primary/30 pl-2 whitespace-pre-wrap break-all text-foreground/80">
      {x.output}
    </pre>
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

/** 依頼の進行を縦のタイムラインで見せる。作業計画があれば手順ごとの節点に実行内容をぶら下げ、無ければ操作を時系列に並べる。実行中は中断できる。 */
export default function ExecutionLog({
  log,
  busy,
  onCancel,
  onClose,
}: {
  log: LogState;
  busy: boolean;
  onCancel: () => void;
  onClose: () => void;
}) {
  const t = useT();
  const scroller = useRef<HTMLDivElement>(null);
  useEffect(() => {
    scroller.current?.scrollTo({ top: scroller.current.scrollHeight });
  }, [log]);

  if (!log.plan && log.entries.length === 0) return null;

  const total = log.plan?.length ?? 0;
  const stepState = (n: number): NodeState => {
    const mine = log.entries.filter((x) => x.step === n);
    if (mine.some((x) => x.status === "running")) return "running";
    if (mine.some((x) => x.status === "error")) return "error";
    if (mine.length === 0) return "pending";
    // 次の手順へ進んだか、依頼が終わっていれば完了
    return !busy || log.entries.some((x) => x.step !== undefined && x.step > n) ? "done" : "running";
  };
  const states = Array.from({ length: total }, (_, i) => stepState(i + 1));
  const finished = states.filter((x) => x === "done").length;
  const current = Math.min(states.findIndex((x) => x === "running" || x === "pending") + 1 || total, total);
  const loose = log.plan ? log.entries.filter((x) => x.step === undefined || x.step > total || x.step < 1) : log.entries;

  return (
    <aside
      aria-label={t("実行ログ")}
      className="hud-panel pointer-events-auto px-3 py-2 font-mono text-xs leading-relaxed"
    >
      <div className="mb-2 flex items-center justify-between gap-3">
        <span className="hud-label text-[0.6rem]">
          {t("Timeline / 進行")}
          {total > 0 && (
            <span className="ml-2 text-foreground/80 tabular-nums">
              {busy ? `STEP ${String(current).padStart(2, "0")}` : "DONE"} / {String(total).padStart(2, "0")}
            </span>
          )}
        </span>
        {busy ? (
          <Button size="xs" variant="destructive" onClick={onCancel}>
            <Square /> {t("中断")}
          </Button>
        ) : (
          <Button size="icon-xs" variant="ghost" aria-label={t("ログを閉じる")} onClick={onClose}>
            <X />
          </Button>
        )}
      </div>
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
      <div ref={scroller} className="max-h-[60vh] overflow-y-auto pr-1 pl-1.5">
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
    </aside>
  );
}
