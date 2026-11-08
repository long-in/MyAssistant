"use client";

import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { AGENT_NOTE_MAX_CHARS, errorMessage, type AgentNote } from "@/lib/api";
import { useT } from "@/lib/i18n";

type Props = {
  load: () => Promise<AgentNote>;
  save: (note: AgentNote) => Promise<AgentNote>;
  /** 入力欄とスイッチの読み上げ用の名前。 */
  label: string;
  placeholder: string;
};

/**
 * エージェントデータ・共通データの編集欄。スイッチは切り替えた時点で、本文は入力欄を離れたときに保存する。
 * 無効の間は、入力済みでもAIへ何も渡さない。
 */
export default function NotePanel({ load, save, label, placeholder }: Props) {
  const t = useT();
  const [saved, setSaved] = useState<AgentNote | null>(null);
  const [value, setValue] = useState<AgentNote | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    load()
      .then((n) => {
        setSaved(n);
        setValue(n);
      })
      .catch((e) => toast.error(errorMessage(e)));
  }, [load]);

  if (!value || !saved) return null;

  const commit = async (next: AgentNote) => {
    setBusy(true);
    try {
      const n = await save(next);
      setSaved(n);
      setValue(n);
      toast.success(t("保存しました。"));
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-4">
        <p className="text-xs text-muted-foreground">{t("入力した内容は、AIのプロバイダへ送信されます。")}</p>
        <Switch
          checked={value.enabled}
          disabled={busy}
          onCheckedChange={(enabled) => {
            const next = { ...value, enabled };
            setValue(next);
            void commit(next);
          }}
          aria-label={t("{label}をAIへ渡す", { label })}
        />
      </div>
      <Textarea
        rows={6}
        value={value.text}
        placeholder={placeholder}
        maxLength={AGENT_NOTE_MAX_CHARS}
        onChange={(e) => setValue({ ...value, text: e.target.value })}
        onBlur={() => value.text !== saved.text && !busy && void commit(value)}
        aria-label={label}
      />
      <p className="text-right text-xs text-muted-foreground">
        {Array.from(value.text).length} / {AGENT_NOTE_MAX_CHARS}
      </p>
    </div>
  );
}
