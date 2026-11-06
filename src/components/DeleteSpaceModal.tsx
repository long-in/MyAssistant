"use client";

import { useEffect, useRef, useState } from "react";
import { Loader2, TriangleAlert } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { useT } from "@/lib/i18n";
import { deleteSpace, errorMessage, type DeleteReport, type Space } from "@/lib/api";
import SpaceChip from "./SpaceChip";

type Props = {
  space: Space;
  /** 閉じる(取りやめ、または実行後)。 */
  onClose: () => void;
  /** 削除に成功した。 */
  onDeleted: (report: DeleteReport) => void;
};

/**
 * スペースの削除の最終確認。元に戻せないため、実行前に必ずここで確かめる。
 * ナレッジを一緒に消すかは選べて、既定は「消さない」。既定のフォーカスは「キャンセル」。実行中は閉じられない。
 */
export default function DeleteSpaceModal({ space, onClose, onDeleted }: Props) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [withKnowledge, setWithKnowledge] = useState(false);
  const cancelRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    cancelRef.current?.focus();
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busy) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onClose]);

  const run = async () => {
    setBusy(true);
    try {
      onDeleted(await deleteSpace(space.id, withKnowledge));
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
      onClose();
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/40 p-6 backdrop-blur-sm">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="delete-space-title"
        className="max-h-full w-full max-w-md overflow-y-auto rounded-xl border border-destructive/40 bg-card/80 p-5 text-sm shadow-2xl backdrop-blur-md"
      >
        <h2 id="delete-space-title" className="text-base font-medium">
          {t("スペースの削除")}
        </h2>
        <div className="mt-3 flex items-center gap-2 rounded-lg border bg-muted/30 px-3 py-2 font-medium">
          <SpaceChip name={space.name} color={space.color} />
        </div>

        <p className="mt-4 text-xs font-medium text-muted-foreground">{t("削除するもの")}</p>
        <ul className="mt-1 list-disc space-y-0.5 pl-5 text-xs">
          <li>{t("会話・要約・設定・コマンド権限・読み上げ辞書")}</li>
          <li>{t("APIキー・AWS設定")}</li>
          <li>{t("実行中の依頼(中断されます)")}</li>
        </ul>

        <label className="mt-4 flex cursor-pointer items-start justify-between gap-3 rounded-lg border p-3">
          <span className="text-xs">
            <span className="block font-medium">{t("ナレッジも一緒に削除する")}</span>
            <span className="mt-0.5 block text-muted-foreground">
              {withKnowledge
                ? t("既定の場所のナレッジとデバッグログも削除されます。")
                : t("既定の場所のナレッジは、アプリデータ内の「kept-knowledge」フォルダへ移して残します。")}
              {" " + t("独自に指定した保存先は、どちらでも削除されません。")}
            </span>
          </span>
          <Switch checked={withKnowledge} disabled={busy} onCheckedChange={setWithKnowledge} aria-label={t("ナレッジも一緒に削除する")} />
        </label>

        <div className="mt-4 flex items-start gap-2 rounded-lg border border-destructive/50 bg-destructive/10 p-3 text-xs">
          <TriangleAlert className="mt-0.5 size-4 shrink-0 text-destructive" />
          <p>{t("「{name}」を削除します。元に戻せません。", { name: space.name })}</p>
        </div>

        <div className="mt-5 flex justify-end gap-2">
          <Button ref={cancelRef} type="button" variant="outline" disabled={busy} onClick={onClose}>
            {t("キャンセル")}
          </Button>
          <Button type="button" variant="destructive" disabled={busy} onClick={() => void run()}>
            {busy && <Loader2 className="animate-spin" />}
            {t("削除する")}
          </Button>
        </div>
      </div>
    </div>
  );
}
