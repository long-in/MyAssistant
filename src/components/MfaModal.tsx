"use client";

import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { Loader2, ShieldCheck } from "lucide-react";
import { useT } from "@/lib/i18n";
import { AWS_MFA_EVENT, type AwsMfaPrompt, cancelAwsMfa, reportError, submitAwsMfa } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

/**
 * Bedrockの呼び出し中にMFAコードが必要になったとき、画面中央に入力欄を出す。
 * コードを送るまで呼び出し側(対話・整理など)は待っているので、入力後はそのまま続行する。
 */
export default function MfaModal() {
  const t = useT();
  // 複数のワークスペースが同時にコードを求めることがあるので、順に出す(同じワークスペースの再要求は置き換える)
  const [queue, setQueue] = useState<AwsMfaPrompt[]>([]);
  const prompt = queue[0] ?? null;
  const done = () => setQueue((q) => q.slice(1));
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const un = listen<AwsMfaPrompt>(AWS_MFA_EVENT, (e) => {
      setCode("");
      setQueue((q) => [...q.filter((p) => p.workspace_id !== e.payload.workspace_id), e.payload]);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  if (!prompt) return null;

  const submit = async () => {
    setBusy(true);
    try {
      await submitAwsMfa(prompt.workspace_id, code);
      done();
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(false);
    }
  };
  const cancel = () => {
    cancelAwsMfa(prompt.workspace_id).catch(() => {});
    done();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/40 p-6 backdrop-blur-sm">
      <form
        role="dialog"
        aria-modal="true"
        aria-labelledby="mfa-title"
        className="w-full max-w-sm rounded-xl border border-primary/40 bg-card/70 p-5 text-sm shadow-2xl backdrop-blur-md"
        onSubmit={(e) => {
          e.preventDefault();
          if (code.length === 6 && !busy) submit();
        }}
      >
        <h2 id="mfa-title" className="flex items-center gap-2 text-base font-medium">
          <ShieldCheck className="size-4 text-primary" />
          {t("MFAコードを入力")}
        </h2>
        <p className="mt-2 flex items-center gap-2 text-xs font-medium" style={{ color: prompt.workspace_color ?? undefined }}>
          {prompt.workspace_color && <span className="size-2.5 shrink-0 rounded-full" style={{ backgroundColor: prompt.workspace_color }} />}
          {t("スペース: {name}", { name: prompt.workspace_name ?? t("パーソナル") })}
        </p>
        <p className="mt-2 text-xs text-muted-foreground">
          {t("AWSプロファイル「{profile}」のスイッチロールにMFAが必要です。", { profile: prompt.profile })}
        </p>
        <p className="mt-1 break-all font-mono text-[0.65rem] text-muted-foreground">{prompt.mfa_serial}</p>
        {prompt.error && <p className="mt-2 break-words text-xs text-destructive">{prompt.error}</p>}
        <Input
          autoFocus
          inputMode="numeric"
          autoComplete="one-time-code"
          maxLength={6}
          placeholder="123456"
          className="mt-3 text-center font-mono text-lg tracking-[0.4em]"
          value={code}
          onChange={(e) => setCode(e.target.value.replace(/\D/g, ""))}
        />
        <div className="mt-4 flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={cancel}>
            {t("キャンセル")}
          </Button>
          <Button type="submit" disabled={busy || code.length !== 6}>
            {busy && <Loader2 className="animate-spin" />}
            {t("送信")}
          </Button>
        </div>
      </form>
    </div>
  );
}
