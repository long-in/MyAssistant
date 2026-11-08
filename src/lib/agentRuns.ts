"use client";

import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { TASK_ATTENTION_EVENT, leaderOverview, type AgentRun } from "@/lib/api";

/** エージェントごとの依頼の状態と、同時に指示できる数の使用状況。 */
export type AgentRuns = {
  /** エージェントIDから、いまの依頼の状態を引く(直接の指示によるものも含む)。 */
  status: Record<string, AgentRun["status"]>;
  /** 同時に指示できるエージェント数の上限。 */
  limit: number;
  /** リーダーが配布して、いま動いている(実行中・承認待ち)エージェントの数。 */
  active: number;
};

const EMPTY: AgentRuns = { status: {}, limit: 0, active: 0 };

/**
 * 各エージェントの依頼の状態を、`enabled`の間だけ定期的に(3秒ごと)、通知(完了・失敗・承認)のたびに読み直す。
 * 依頼の記録から読むだけで、エージェントのLLMは呼ばない。
 */
export function useAgentRuns(enabled: boolean): AgentRuns {
  const [runs, setRuns] = useState<AgentRuns>(EMPTY);

  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    const refresh = () =>
      leaderOverview()
        .then((o) => {
          if (alive) setRuns({ status: Object.fromEntries(o.agents.map((s) => [s.id, s.status])), limit: o.limit, active: o.active });
        })
        .catch(() => {});
    const first = setTimeout(refresh, 0);
    const timer = setInterval(refresh, 3000);
    const un = listen(TASK_ATTENTION_EVENT, refresh);
    return () => {
      alive = false;
      clearTimeout(first);
      clearInterval(timer);
      void un.then((f) => f());
    };
  }, [enabled]);

  return enabled ? runs : EMPTY;
}
