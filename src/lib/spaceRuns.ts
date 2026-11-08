"use client";

import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AGENT_ATTENTION_EVENT, leaderOverview, type SpaceRun } from "@/lib/api";

/** スペースごとの依頼の状態と、同時に指示できる数の使用状況。 */
export type SpaceRuns = {
  /** スペースIDから、いまの依頼の状態を引く(直接の指示によるものも含む)。 */
  status: Record<string, SpaceRun["status"]>;
  /** 同時に指示できるスペース数の上限。 */
  limit: number;
  /** リーダーが配布して、いま動いている(実行中・承認待ち)スペースの数。 */
  active: number;
};

const EMPTY: SpaceRuns = { status: {}, limit: 0, active: 0 };

/**
 * 各スペースの依頼の状態を、`enabled`の間だけ定期的に(3秒ごと)、通知(完了・失敗・承認)のたびに読み直す。
 * 依頼の記録から読むだけで、スペースのLLMは呼ばない。
 */
export function useSpaceRuns(enabled: boolean): SpaceRuns {
  const [runs, setRuns] = useState<SpaceRuns>(EMPTY);

  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    const refresh = () =>
      leaderOverview()
        .then((o) => {
          if (alive) setRuns({ status: Object.fromEntries(o.spaces.map((s) => [s.id, s.status])), limit: o.limit, active: o.active });
        })
        .catch(() => {});
    const first = setTimeout(refresh, 0);
    const timer = setInterval(refresh, 3000);
    const un = listen(AGENT_ATTENTION_EVENT, refresh);
    return () => {
      alive = false;
      clearTimeout(first);
      clearInterval(timer);
      void un.then((f) => f());
    };
  }, [enabled]);

  return enabled ? runs : EMPTY;
}
