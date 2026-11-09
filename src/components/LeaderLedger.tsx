"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Check } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { TASK_ATTENTION_EVENT, errorMessage, leaderOverview, leaderSetConfirmed, type LeaderTask } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** 状態の表示。 */
const STATUS_LABEL: Record<string, string> = {
  idle: "待機",
  running: "実行中",
  awaiting_approval: "承認待ち",
  done: "完了",
  failed: "失敗",
  interrupted: "中断",
};

function StatusBadge({ status }: { status: string }) {
  const t = useT();
  const variant = status === "failed" ? "destructive" : status === "done" ? "default" : status === "idle" ? "outline" : "secondary";
  return <Badge variant={variant}>{t(STATUS_LABEL[status] ?? status)}</Badge>;
}

/**
 * リーダーの台帳。配布した指示ごとの、宛先の状態を並べる(新しい順)。リーダーのチャット画面の脇に置く。
 * 状態は依頼の記録から読むだけなので、表示している間は定期的に読み直し、通知(完了・失敗・承認)でもすぐ読み直す。
 */
export default function LeaderLedger({ active, className }: { active: boolean; className?: string }) {
  const t = useT();
  const [tasks, setTasks] = useState<LeaderTask[]>([]);

  const refresh = useCallback(async () => {
    try {
      setTasks((await leaderOverview()).tasks);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  }, []);

  useEffect(() => {
    if (!active) return;
    const first = setTimeout(() => void refresh(), 0);
    const timer = setInterval(() => void refresh(), 3000);
    const un = listen(TASK_ATTENTION_EVENT, () => void refresh());
    return () => {
      clearTimeout(first);
      clearInterval(timer);
      void un.then((f) => f());
    };
  }, [active, refresh]);

  // 1回の配布(同じ`batch_id`)ごとにまとめる
  const batches = useMemo(() => {
    const map = new Map<string, LeaderTask[]>();
    for (const task of tasks) map.set(task.batch_id, [...(map.get(task.batch_id) ?? []), task]);
    return [...map.values()];
  }, [tasks]);

  const confirm = async (task: LeaderTask) => {
    try {
      await leaderSetConfirmed(task.id, !task.confirmed);
      await refresh();
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  return (
    <aside className={cn("flex min-h-0 flex-col border-t bg-sidebar/40", className)} aria-label={t("台帳")}>
      <div className="px-4 pt-3 pb-2">
        <p className="hud-label">{"// Ledger"}</p>
        <h2 className="text-sm font-semibold">{t("台帳")}</h2>
        <p className="mt-1 text-xs text-muted-foreground">{t("配布した指示ごとの、宛先の状態です。「確認済み」は、自分で付ける区切りです。")}</p>
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-4 pb-4">
        {batches.length === 0 && <p className="text-sm text-muted-foreground">{t("まだ配布した指示がありません。")}</p>}
        {batches.map((rows) => (
          <div key={rows[0].batch_id} className="hud-panel hud-accent px-4 py-3">
            <p className="line-clamp-4 whitespace-pre-wrap text-sm">{rows[0].instruction}</p>
            <p className="mt-1 text-xs text-muted-foreground">{new Date(rows[0].created_at).toLocaleString()}</p>
            <div className="mt-2 flex flex-col divide-y">
              {rows.map((r) => (
                <div key={r.id} className="flex items-center gap-2 py-1.5 text-sm">
                  <span className="size-3 shrink-0 rounded-full bg-muted" style={r.agent_color ? { backgroundColor: r.agent_color } : undefined} />
                  <span className="min-w-0 flex-1 truncate">
                    {r.agent_name}
                    {!r.agent_exists && <span className="ml-1 text-xs text-muted-foreground">{t("(削除済み)")}</span>}
                  </span>
                  <StatusBadge status={r.live_status} />
                  <Button
                    size="icon-sm"
                    variant={r.confirmed ? "default" : "outline"}
                    title={t("確認済み")}
                    aria-label={t("確認済み")}
                    aria-pressed={r.confirmed}
                    onClick={() => void confirm(r)}
                  >
                    <Check />
                  </Button>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
    </aside>
  );
}
