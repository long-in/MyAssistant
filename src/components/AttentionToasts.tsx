"use client";

import { useRouter } from "next/navigation";
import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { AGENT_ATTENTION_EVENT, type AgentAttention, errorMessage } from "@/lib/api";
import { tr } from "@/lib/i18n";
import { notifyOsIfBackground } from "@/lib/osNotify";
import { switchTo, useWorkspace } from "@/lib/workspace";

/** 通知の識別子。同じスペース・同じ種類の通知は、重ねずに置き換える。 */
const toastId = (workspaceId: string, kind: AgentAttention["kind"]) => `attention:${workspaceId}:${kind}`;
const KINDS: AgentAttention["kind"][] = ["approval", "finished", "failed"];

/** 承認を求められている内容を、言語設定に合わせた1行にする。`detail`は、ファイルは作成先、コマンドはその内容。 */
export function approvalLine(kind: AgentAttention["approval_kind"], detail: string): string | undefined {
  switch (kind) {
    case "file":
      return tr("ファイルを作成: {detail}", { detail });
    case "plan":
      return tr("作業計画の確認");
    case "command":
      return tr("コマンドを実行: {detail}", { detail });
    case "allow_command":
      return tr("コマンドを許可: {detail}", { detail });
    default:
      return undefined;
  }
}

const approvalText = (a: AgentAttention) => approvalLine(a.approval_kind, a.detail ?? "");

/**
 * 開いていないスペースで、依頼が完了・失敗したときや、承認が必要になったときに、トーストで知らせる。
 * 「開く」で、そのスペースへ切り替えて対話画面を開く(承認は、戻れば答えられる)。
 * 承認の通知は、対応するまで消えない。そのスペースを開いたら、そのスペースの通知は消す。
 * 文言は、受け取った時点の言語設定に従う。
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
      const who = a.workspace_name ?? tr("パーソナル");
      const id = toastId(a.workspace_id, a.kind);
      const action = { label: tr("開く"), onClick: () => void open(a.workspace_id) };
      if (a.kind === "approval") {
        const title = tr("{who}: 承認が必要です", { who });
        toast.warning(title, { id, description: approvalText(a), duration: Infinity, action });
        void notifyOsIfBackground(title, approvalText(a));
      } else if (a.kind === "finished") {
        const title = tr("{who}: 依頼が完了しました", { who });
        toast.success(title, { id, duration: 30_000, action });
        void notifyOsIfBackground(title);
      } else {
        const title = tr("{who}: 依頼が失敗しました", { who });
        const description = a.error ? errorMessage(a.error) : undefined;
        toast.error(title, { id, description, duration: 30_000, action });
        void notifyOsIfBackground(title, description);
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
