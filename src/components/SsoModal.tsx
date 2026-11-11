"use client";

import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { ExternalLink, Loader2, ShieldCheck } from "lucide-react";
import { useT } from "@/lib/i18n";
import { AWS_SSO_EVENT, AWS_SSO_FINISHED_EVENT, type AwsSsoPrompt, cancelAwsSso, openAwsSso, reportError } from "@/lib/api";
import { Button } from "@/components/ui/button";

/**
 * Bedrockの呼び出し中にIAM Identity Center(SSO)の承認が必要になったとき、画面中央に確認コードを出す。
 * ブラウザは勝手に開かず、ボタンで開く(裏のエージェントが求めたときに、突然ブラウザが前面に出ないようにする)。
 * 承認が済む(または期限切れ・取り消し)まで呼び出し側は待っていて、終わるとRustからの通知で閉じる。
 */
export default function SsoModal() {
  const t = useT();
  // 複数のワークスペースが同時に求めることがあるので、順に出す(同じワークスペースの再要求は置き換える)
  const [queue, setQueue] = useState<AwsSsoPrompt[]>([]);
  const prompt = queue[0] ?? null;
  // ブラウザを開いたワークスペース。開いた後は、承認待ちの表示にする
  const [opened, setOpened] = useState<string | null>(null);

  useEffect(() => {
    const required = listen<AwsSsoPrompt>(AWS_SSO_EVENT, (e) => {
      setOpened((id) => (id === e.payload.workspace_id ? null : id));
      setQueue((q) => [...q.filter((p) => p.workspace_id !== e.payload.workspace_id), e.payload]);
    });
    const finished = listen<string>(AWS_SSO_FINISHED_EVENT, (e) => {
      setQueue((q) => q.filter((p) => p.workspace_id !== e.payload));
    });
    return () => {
      required.then((f) => f());
      finished.then((f) => f());
    };
  }, []);

  if (!prompt) return null;

  const waiting = opened === prompt.workspace_id;
  const openBrowser = async () => {
    try {
      await openAwsSso(prompt.workspace_id);
      setOpened(prompt.workspace_id);
    } catch (e) {
      toast.error(reportError(e));
    }
  };
  const cancel = () => {
    cancelAwsSso(prompt.workspace_id).catch(() => {});
    setQueue((q) => q.filter((p) => p.workspace_id !== prompt.workspace_id));
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/40 p-6 backdrop-blur-sm">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="sso-title"
        className="w-full max-w-sm rounded-xl border border-primary/40 bg-card/70 p-5 text-sm shadow-2xl backdrop-blur-md"
      >
        <h2 id="sso-title" className="flex items-center gap-2 text-base font-medium">
          <ShieldCheck className="size-4 text-primary" />
          {t("AWS SSOでサインイン")}
        </h2>
        <p className="mt-2 flex items-center gap-2 text-xs font-medium" style={{ color: prompt.workspace_color ?? undefined }}>
          {prompt.workspace_color && <span className="size-2.5 shrink-0 rounded-full" style={{ backgroundColor: prompt.workspace_color }} />}
          {t("エージェント: {name}", { name: prompt.workspace_name ?? t("パーソナル") })}
        </p>
        <p className="mt-2 text-xs text-muted-foreground">
          {t("AWSプロファイル「{profile}」の利用に、ブラウザでの承認が必要です。ブラウザに表示されるコードが下と同じことを確かめてから承認してください。", {
            profile: prompt.profile,
          })}
        </p>
        <p className="mt-3 select-all text-center font-mono text-lg tracking-[0.3em]">{prompt.user_code}</p>
        <p className="mt-1 break-all text-center font-mono text-[0.65rem] text-muted-foreground">{prompt.verification_uri}</p>
        <p className="mt-2 flex items-center justify-center gap-1.5 text-xs text-muted-foreground" aria-live="polite">
          {waiting && <Loader2 className="size-3 animate-spin" />}
          {waiting
            ? t("ブラウザでの承認を待っています…")
            : t("承認の期限: {time}", { time: new Date(prompt.expires_at * 1000).toLocaleTimeString() })}
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={cancel}>
            {t("キャンセル")}
          </Button>
          <Button type="button" autoFocus variant={waiting ? "outline" : "default"} onClick={openBrowser}>
            <ExternalLink />
            {waiting ? t("もう一度開く") : t("ブラウザで開く")}
          </Button>
        </div>
      </div>
    </div>
  );
}
