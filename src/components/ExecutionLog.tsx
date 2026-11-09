// 実行ログの状態と、進捗イベントの反映(表示は TurnLog)
"use client";

import { tr } from "@/lib/i18n";
import type { TaskEvent } from "@/lib/api";

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
  agent_list: "エージェントの一覧",
  ledger_list: "台帳の参照",
  draft_dispatch: "配布の下書き",
  remember: "記憶",
  forget: "記憶から外す",
  harness: "ハーネス",
  use_skill: "スキルの読み込み",
  create_skill: "スキルの作成",
  update_skill: "スキルの更新",
  delete_skill: "スキルの削除",
};

const str = (v: unknown) => (typeof v === "string" ? v : "");

/** エージェントの進捗イベントを実行ログへ反映する。ログに関係しないイベントはそのまま返す。 */
export function applyTaskEvent(log: LogState, e: TaskEvent): LogState {
  if (e.type === "tool_call") {
    if (e.name === "propose_plan") {
      const steps = Array.isArray(e.input.steps) ? e.input.steps.filter((s): s is string => typeof s === "string") : [];
      return { plan: steps, entries: log.entries };
    }
    const isCommand = e.name === "run_command";
    const title = isCommand
      ? str(e.input.command)
      : `${tr(TOOL_LABELS[e.name] ?? e.name)} ${str(e.input.path) || str(e.input.query) || str(e.input.text) || str(e.input.name)}`.trim();
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
