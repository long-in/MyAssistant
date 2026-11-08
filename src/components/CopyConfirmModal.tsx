"use client";

import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ArrowRight, Loader2, TriangleAlert } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { useT } from "@/lib/i18n";
import { PERSONAL_ID, copyWorkspaceSettings, errorMessage, type CopyReport } from "@/lib/api";
import type { CopyRequest, CopyTarget } from "@/lib/copyTargets";
import AgentChip from "./AgentChip";

type Props = {
  request: CopyRequest;
  targets: CopyTarget[];
  /** 閉じる(取りやめ、または実行後)。 */
  onClose: () => void;
  /** コピーに成功した。 */
  onCopied: (report: CopyReport, request: CopyRequest) => void;
};

/**
 * 設定のコピーの最終確認。コピー先の同じ設定は上書きされ、元に戻せないため、実行前に必ずここで確かめる。
 * 既定のフォーカスは「Cancel」。実行中は閉じられない。
 */
export default function CopyConfirmModal({ request, targets, onClose, onCopied }: Props) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const from = targets.find((t) => t.id === request.from);
  const to = targets.find((t) => t.id === request.to);

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

  // どちらかが存在しなくなっていたら(削除など)、実行させない
  if (!from || !to) return null;

  const run = async () => {
    setBusy(true);
    try {
      const report = await copyWorkspaceSettings(request.from, request.to, request.includeKeys, request.includeDictionary);
      onCopied(report, request);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
      onClose();
    }
  };

  // どこから呼ばれても、画面全体を基準に表示する(カードの`backdrop-filter`・`clip-path`に切り取られない)
  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/40 p-6 backdrop-blur-sm">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="copy-title"
        className="flex max-h-full w-full max-w-md flex-col rounded-xl border border-destructive/40 bg-card/80 p-5 text-sm shadow-2xl backdrop-blur-md"
      >
        <h2 id="copy-title" className="shrink-0 text-base font-medium">
          {t("設定のコピー")}
        </h2>
        <div className="min-h-0 flex-1 overflow-y-auto pr-1">
        <div className="mt-3 flex items-center gap-2 rounded-lg border bg-muted/30 px-3 py-2 font-medium">
          <AgentChip name={from.name} color={from.color} />
          <ArrowRight className="size-4 shrink-0 text-muted-foreground" />
          <AgentChip name={to.name} color={to.color} />
        </div>

        <p className="mt-4 text-xs font-medium text-muted-foreground">{t("コピーするもの")}</p>
        <ul className="mt-1 list-disc space-y-0.5 pl-5 text-xs">
          <li>{t("設定: モデル・音声・調整値・デバッグモード")}</li>
          <li>{t("コマンド権限")}</li>
          <li>{t("Bedrockのモデル一覧")}</li>
          {request.includeKeys && <li className="font-medium text-destructive">{t("APIキー(Anthropic・AWS・Google・Aqua)")}</li>}
          {request.includeDictionary && <li className="font-medium text-destructive">{t("読み上げ辞書(コピー先の辞書は置き換わる)")}</li>}
        </ul>
        <p className="mt-3 text-xs font-medium text-muted-foreground">{t("コピーしないもの")}</p>
        <p className="mt-1 text-xs text-muted-foreground">
          {t("ナレッジ・会話、ナレッジとデバッグログの保存先")}
          {request.includeKeys ? "" : t("、APIキー")}
          {t("、全体設定(ベースプロンプト・パーソナルデータ・起動ワードなど。全エージェント共通)、エージェントデータ")}
          {request.includeDictionary ? "" : t("、読み上げ辞書")}
          {t("。")}
        </p>

        <div className="mt-4 flex items-start gap-2 rounded-lg border border-destructive/50 bg-destructive/10 p-3 text-xs">
          <TriangleAlert className="mt-0.5 size-4 shrink-0 text-destructive" />
          <p>
            {t("「{to}」の同じ設定は上書きされます。元に戻せません。", { to: to.name })}
            {to.id === PERSONAL_ID ? " " + t("コピー先は、既存のパーソナルです。") : ""}
          </p>
        </div>
        </div>

        <div className="mt-4 flex shrink-0 justify-end gap-2 border-t pt-4">
          <Button ref={cancelRef} type="button" variant="outline" disabled={busy} onClick={onClose}>
            {t("キャンセル")}
          </Button>
          <Button type="button" variant="destructive" disabled={busy} onClick={() => void run()}>
            {busy && <Loader2 className="animate-spin" />}
            {t("上書きしてコピー")}
          </Button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
