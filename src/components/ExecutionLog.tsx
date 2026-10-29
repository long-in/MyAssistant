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

/** 直前のやり取り(ExchangeLog)と同じターミナル風の表示で、実行の経過の下に並べて表示する。実行中は中断できる。 */
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
  const done = new Set(log.entries.filter((x) => x.step && x.status !== "running").map((x) => x.step));
  const running = log.entries.find((x) => x.status === "running")?.step;

  return (
    <aside
      aria-label={t("実行ログ")}
      className="hud-panel pointer-events-auto px-3 py-2 font-mono text-xs leading-relaxed"
    >
      <div className="mb-2 flex items-center justify-between">
        <span className="hud-label text-[0.6rem]">{t("Execution log / 実行")}</span>
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
      <div ref={scroller} className="flex max-h-[30vh] flex-col gap-2 overflow-y-auto pr-1">
        {log.plan && (
          <div>
            <p>
              <span className="text-primary">&gt; PLAN</span>
              <span className="text-foreground/90"> :: {log.plan.length} steps</span>
            </p>
            <ol className="mt-0.5">
              {log.plan.map((s, i) => (
                <li
                  key={i}
                  className={cn(
                    "flex gap-2 pl-2",
                    done.has(i + 1) ? "text-muted-foreground line-through" : "text-foreground/90",
                    running === i + 1 && "text-primary",
                  )}
                >
                  <span className="shrink-0 tabular-nums">{String(i + 1).padStart(2, "0")}</span>
                  <span className="min-w-0 break-words">{s}</span>
                </li>
              ))}
            </ol>
          </div>
        )}
        {log.entries.map((x) => (
          <div key={x.id}>
            <p className="break-all">
              <span className="text-muted-foreground">[{clock(x.at)}]</span>{" "}
              <span className="text-primary">&gt; {x.isCommand ? "EXEC" : "FILE"}</span>
              <span className="text-foreground/90"> :: {x.title}</span>
            </p>
            {x.output && (
              <pre className="my-0.5 max-h-28 overflow-auto border-l border-primary/30 pl-2 whitespace-pre-wrap break-all text-foreground/80">
                {x.output}
              </pre>
            )}
            {x.status === "running" ? (
              <p className="text-muted-foreground">
                &lt; RUNNING<span className="animate-hud-blink">…</span>
              </p>
            ) : (
              <p>
                <span className="text-muted-foreground">[{x.endedAt && clock(x.endedAt)}]</span>{" "}
                {x.status === "ok" ? (
                  <span className="text-chart-2">&lt; DONE</span>
                ) : (
                  <span className="text-destructive">&lt; FAILED</span>
                )}
              </p>
            )}
          </div>
        ))}
      </div>
    </aside>
  );
}
