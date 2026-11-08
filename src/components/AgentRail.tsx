"use client";

import { useRouter } from "next/navigation";
import { ClipboardList, Plus, User } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import CreateAgentDialog from "./CreateAgentDialog";
import { LEADER_COLOR, LEADER_ID, PERSONAL_ID, errorMessage } from "@/lib/api";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { useAgentRuns } from "@/lib/agentRuns";
import { switchTo, useWorkspace } from "@/lib/workspace";

/** 状態の印の色と読み上げ用の名前。待機(`idle`)は印なし。 */
const RUN_MARK: Record<string, { dot: string; label: string }> = {
  running: { dot: "bg-sky-500", label: "実行中" },
  awaiting_approval: { dot: "bg-amber-500 animate-pulse", label: "承認待ち" },
  done: { dot: "bg-emerald-500", label: "完了" },
  failed: { dot: "bg-red-500", label: "失敗" },
};

/** エージェント名の頭文字(絵文字などのサロゲートペアも1文字として扱う)。 */
const initial = (name: string) => Array.from(name.trim())[0]?.toUpperCase() ?? "?";

/**
 * 左端のエージェントレール。パーソナルと各エージェントを、色付きのアイコンで常に並べて見せ、1クリックで切り替える。
 * メニューを畳んでも幅は変わらず、いま開いているエージェントと、ほかのエージェントへの切り替えが常に見える。
 */
export default function AgentRail() {
  const router = useRouter();
  const t = useT();
  const { active, agents, hasLeader, workOn } = useWorkspace();
  const runs = useAgentRuns(workOn);
  const onLeader = active.id === LEADER_ID;
  const [creating, setCreating] = useState(false);

  const go = (id: string) => {
    if (id === active.id) return;
    switchTo(id).catch((e) => toast.error(errorMessage(e)));
  };

  const item = (id: string, label: string, color: string | null, child: React.ReactNode) => {
    const on = active.id === id;
    const run = runs.status[id] ?? "idle";
    const mark = RUN_MARK[run];
    const working = run === "running" || run === "awaiting_approval";
    return (
      <button
        key={id}
        type="button"
        onClick={() => go(id)}
        title={mark ? `${label} (${t(mark.label)})` : label}
        aria-label={mark ? `${label} (${t(mark.label)})` : label}
        aria-current={on ? "true" : undefined}
        className="group relative flex w-full items-center justify-center py-0.5"
      >
        {/* 選択中の印(左のバー) */}
        <span
          className={cn("absolute left-0 w-1 rounded-r-full transition-all", on ? "h-8 opacity-100" : "h-2 opacity-0 group-hover:opacity-60")}
          style={{ backgroundColor: color ?? "var(--primary)" }}
        />
        <span
          className={cn(
            "relative flex size-9 items-center justify-center text-sm font-semibold text-white transition-all",
            color ? "rounded-full" : "rounded-lg bg-muted text-foreground",
            on ? "ring-2 ring-offset-2 ring-offset-sidebar" : working || mark ? "opacity-100" : "opacity-60 group-hover:opacity-100",
          )}
          style={color ? { backgroundColor: color, ["--tw-ring-color" as string]: color } : on ? { ["--tw-ring-color" as string]: "var(--primary)" } : undefined}
        >
          {child}
          {/* 動いているエージェントは、周りを回るリングで示す(閉じている=待機のエージェントは、淡く表示する) */}
          {run === "running" && !on && (
            <span aria-hidden className="pointer-events-none absolute -inset-1 animate-spin rounded-full border-2 border-transparent" style={{ borderTopColor: color ?? "var(--primary)" }} />
          )}
          {mark && <span aria-hidden className={cn("absolute -right-1 -bottom-1 size-3 rounded-full ring-2 ring-sidebar", mark.dot)} />}
        </span>
      </button>
    );
  };

  return (
    <div className="flex w-14 shrink-0 flex-col items-center gap-1 border-r bg-sidebar/90 py-3 backdrop-blur" aria-label="Agents">
      {/* パーソナル: 単独のエージェントとして先頭に置く */}
      {item(PERSONAL_ID, "Personal", null, <User className="size-4" />)}
      <div className="my-1 h-px w-6 bg-border" />
      {/* ワークが使えないあいだ(未有効・無効)は、枠を出さず、全体設定の「Work」への入口だけを出す */}
      {!workOn && (
        <button
          type="button"
          onClick={() => router.push("/global#work")}
          title={hasLeader ? "Work is off (turn on in Global Settings)" : "Enable Work"}
          aria-label={hasLeader ? "Work is off" : "Enable Work"}
          className="flex w-12 flex-col items-center gap-1 rounded-xl border border-dashed py-2 text-muted-foreground transition-colors hover:border-primary hover:text-primary"
        >
          <Plus className="size-4" />
          <span className="text-[9px] font-semibold tracking-wider">WORK</span>
        </button>
      )}
      {workOn && <>
      <div className="px-1 text-[9px] font-semibold tracking-wider text-muted-foreground">WORK</div>
      {/* リーダーが配布して動いているエージェント数 / 同時に指示できる上限。上限に達したら琥珀色 */}
      {runs.limit > 0 && (
        <div
          title={t("リーダーが配布して動いているエージェント数 / 同時に指示できる上限")}
          aria-label={t("実行中 {active}/{limit}", { active: runs.active, limit: runs.limit })}
          className={cn("-mt-1 text-[9px] font-semibold tabular-nums", runs.active >= runs.limit ? "text-amber-500" : "text-muted-foreground")}
        >
          {runs.active}/{runs.limit}
        </div>
      )}
      {/* ワーク: リーダーと各エージェントを、ひとつの枠にまとめて包含関係を示す */}
      <div className="flex min-h-0 w-12 flex-col items-center gap-1 rounded-xl bg-muted/50 py-2" aria-label="Work">
      {/* リーダー: エージェントと同じように切り替えて、チャットを開く */}
      <button
        type="button"
        onClick={() => {
          router.push("/");
          if (active.id !== LEADER_ID) switchTo(LEADER_ID).catch((e) => toast.error(errorMessage(e)));
        }}
        title="Leader"
        aria-label="Leader"
        aria-current={active.id === LEADER_ID ? "true" : onLeader ? "page" : undefined}
        className={cn(
          "flex size-9 shrink-0 items-center justify-center rounded-lg border text-muted-foreground transition-colors hover:border-primary hover:text-primary",
          onLeader && "border-primary text-primary",
        )}
        style={active.id === LEADER_ID ? { borderColor: LEADER_COLOR, color: LEADER_COLOR } : undefined}
      >
        <ClipboardList className="size-4" />
      </button>
      {agents.length > 0 && <div className="my-1 h-px w-6 bg-border" />}
      {/* エージェントの一覧。追加ボタンは、最後のエージェントの直下に置く(一覧が長ければ、一覧の中でスクロールする) */}
      <div className="flex min-h-0 w-full flex-col items-center gap-1 overflow-y-auto overflow-x-hidden">
        {agents.map((p) => item(p.id, p.name, p.color, initial(p.name)))}
        <button
          type="button"
          onClick={() => setCreating(true)}
          title="Create an agent"
          aria-label="Create an agent"
          className="flex size-9 shrink-0 items-center justify-center rounded-full border border-dashed text-muted-foreground transition-colors hover:border-primary hover:text-primary"
        >
          <Plus className="size-4" />
        </button>
      </div>
      </div>
      </>}
      {creating && <CreateAgentDialog onClose={() => setCreating(false)} />}
    </div>
  );
}
