"use client";

import { useRouter } from "next/navigation";
import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { AGENT_ATTENTION_EVENT, type AgentAttention, errorMessage } from "@/lib/api";
import { switchTo, useWorkspace } from "@/lib/workspace";

/** 通知の識別子。同じスペース・同じ種類の通知は、重ねずに置き換える。 */
const toastId = (workspaceId: string, kind: AgentAttention["kind"]) => `attention:${workspaceId}:${kind}`;
const KINDS: AgentAttention["kind"][] = ["approval", "finished", "failed"];

/**
 * 開いていないスペースで、依頼が完了・失敗したときや、承認が必要になったときに、トーストで知らせる。
 * 「Open」で、そのスペースへ切り替えて対話画面を開く(承認は、戻れば答えられる)。
 * 承認の通知は、対応するまで消えない。そのスペースを開いたら、そのスペースの通知は消す。
 */
export default function AttentionToasts() {
  const router = useRouter();
  const { active } = useWorkspace();

  useEffect(() => {
    const open = (workspaceId: string) =>
      switchTo(workspaceId)
        .then(() => router.push("/"))
        .catch((e) => toast.error(errorMessage(e)));
    const un = listen<AgentAttention>(AGENT_ATTENTION_EVENT, (e) => {
      const a = e.payload;
      const who = a.workspace_name ?? "Personal";
      const id = toastId(a.workspace_id, a.kind);
      const action = { label: "Open", onClick: () => void open(a.workspace_id) };
      if (a.kind === "approval") {
        toast.warning(`${who}: approval needed`, { id, description: a.detail ?? undefined, duration: Infinity, action });
      } else if (a.kind === "finished") {
        toast.success(`${who}: request completed`, { id, duration: 30_000, action });
      } else {
        toast.error(`${who}: request failed`, { id, description: a.error ? errorMessage(a.error) : undefined, duration: 30_000, action });
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, [router]);

  // そのスペースを開いたら、対応済みとして、そのスペースの通知を消す
  useEffect(() => {
    for (const kind of KINDS) toast.dismiss(toastId(active.id, kind));
  }, [active.id]);

  return null;
}
