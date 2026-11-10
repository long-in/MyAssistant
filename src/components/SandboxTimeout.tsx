"use client";

import { useEffect, useState } from "react";
import { toast } from "sonner";
import { useT } from "@/lib/i18n";
import { type SandboxLlm, getSandboxLlm, reportError, setSandboxLlm } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

/**
 * コンテナ内の Claude Code が、1回の指示に使える上限時間(分)。超えると中断する。settings.json には書けないので、アプリ側の設定にする。
 * あわせて、使われなくなったコンテナを自動で止めるまでの時間(分)。0なら自動では止めない(既定)。
 */
export default function SandboxTimeout() {
  const t = useT();
  const [saved, setSaved] = useState<SandboxLlm | null>(null);
  const [mins, setMins] = useState("30");
  const [idle, setIdle] = useState("0");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getSandboxLlm()
      .then((c) => {
        setSaved(c);
        setMins(String(c.timeout_mins));
        setIdle(String(c.idle_stop_mins));
      })
      .catch((e) => toast.error(reportError(e)));
  }, []);

  const value = Number.parseInt(mins, 10) || 30;
  const idleValue = Math.max(0, Number.parseInt(idle, 10) || 0);
  const dirty = saved !== null && (value !== saved.timeout_mins || idleValue !== saved.idle_stop_mins);

  const save = async () => {
    if (!saved) return;
    setBusy(true);
    try {
      const c = await setSandboxLlm({ ...saved, timeout_mins: value, idle_stop_mins: idleValue });
      setSaved(c);
      setMins(String(c.timeout_mins));
      setIdle(String(c.idle_stop_mins));
      toast.success(t("保存しました。次の指示から反映されます。"));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-2">
      <span className="hud-label">{t("1回の指示の上限時間(分)")}</span>
      <div className="flex flex-wrap items-center gap-2">
        <Input type="number" min={1} max={180} value={mins} onChange={(e) => setMins(e.target.value)} className="w-28" aria-label={t("1回の指示の上限時間(分)")} />
      </div>
      <p className="text-xs text-muted-foreground">{t("超えたときは中断します。続きは、もう一度指示すると同じ会話で再開できます。")}</p>
      <span className="hud-label mt-2">{t("コンテナの自動停止(分)")}</span>
      <div className="flex flex-wrap items-center gap-2">
        <Input type="number" min={0} max={1440} value={idle} onChange={(e) => setIdle(e.target.value)} className="w-28" aria-label={t("コンテナの自動停止(分)")} />
        <Button size="sm" disabled={!dirty || busy} onClick={() => void save()}>
          {t("保存")}
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">{t("指示が行われない時間がこの長さを超えると、コンテナを停止します。0なら自動では止めません(既定)。停止後は、設定画面からコンテナを起動してください。")}</p>
    </div>
  );
}
