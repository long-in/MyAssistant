"use client";

import { useState } from "react";
import { Send, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { SPACE_NOTE_MAX_CHARS, errorMessage, leaderDispatch, type DispatchDraft, type DispatchReason } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { useWorkspace } from "@/lib/workspace";

/** 配布できなかった理由。 */
const REASON_LABEL: Record<DispatchReason, string> = {
  not_found: "スペースが見つかりません",
  active: "開いているため配布できません",
  busy: "別の依頼の最中です",
  no_provider: "APIキーが未登録です",
  error: "配布に失敗しました",
};

/**
 * リーダーが作った配布の下書き。指示を直し、宛先を確かめて、「配布」を押したときだけ配布する(AIは配布できない)。
 * 宛先の初期値はAIの案で、自由に変えられる。
 */
export default function DispatchDraftCard({ draft, onClose, className }: { draft: DispatchDraft; onClose: () => void; className?: string }) {
  const t = useT();
  const { spaces } = useWorkspace();
  const [text, setText] = useState(draft.instruction);
  const [picked, setPicked] = useState<Set<string>>(() => new Set(draft.targets.map((x) => x.id)));
  const [busy, setBusy] = useState(false);

  const toggle = (id: string) =>
    setPicked((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  const dispatch = async () => {
    setBusy(true);
    try {
      const results = await leaderDispatch(text, [...picked]);
      const started = results.filter((r) => r.started).length;
      if (started > 0) toast.success(t("{count}件のスペースへ配布しました。", { count: started }));
      for (const r of results.filter((x) => !x.started)) {
        const name = spaces.find((s) => s.id === r.space_id)?.name ?? r.space_id;
        toast.error(`${name}: ${t(REASON_LABEL[r.reason ?? "error"])}`);
      }
      if (started > 0) onClose();
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div role="region" aria-label={t("配布の下書き")} className={`flex flex-col gap-3 rounded-lg border bg-popover p-4 text-popover-foreground shadow-lg ${className ?? ""}`}>
      <div className="flex items-center justify-between gap-2">
        <p className="text-sm font-semibold">{t("配布の下書き")}</p>
        <Button type="button" variant="ghost" size="icon" aria-label={t("破棄")} title={t("破棄")} onClick={onClose}>
          <X />
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">{t("内容と宛先を確かめて、「配布」を押すと、選んだスペースで依頼が始まります。配布はまだされていません。")}</p>
      <Textarea rows={3} value={text} maxLength={SPACE_NOTE_MAX_CHARS} aria-label={t("配布する指示")} onChange={(e) => setText(e.target.value)} />
      <div className="flex max-h-40 flex-col divide-y overflow-y-auto rounded-md border">
        {spaces.length === 0 && <p className="p-3 text-sm text-muted-foreground">{t("まだスペースがありません。")}</p>}
        {spaces.map((s) => (
          <label key={s.id} className="flex items-center gap-3 px-3 py-1.5 text-sm">
            <input type="checkbox" checked={picked.has(s.id)} onChange={() => toggle(s.id)} aria-label={s.name} />
            <span className="size-3 shrink-0 rounded-full" style={{ backgroundColor: s.color }} />
            <span className="min-w-0 flex-1 truncate">{s.name}</span>
            {draft.targets.some((x) => x.id === s.id) && <span className="text-xs text-muted-foreground">{t("推奨")}</span>}
          </label>
        ))}
      </div>
      <div className="flex justify-end">
        <Button type="button" disabled={busy || text.trim().length === 0 || picked.size === 0} onClick={() => void dispatch()}>
          <Send />
          {t("配布")}
        </Button>
      </div>
    </div>
  );
}
