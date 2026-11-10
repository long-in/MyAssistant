"use client";

import { useCallback, useEffect, useState } from "react";
import { Download, Loader2, Trash2, X } from "lucide-react";
import { toast } from "sonner";
import { useT } from "@/lib/i18n";
import {
  cancelLocalLlmDownload,
  type DownloadProgress,
  downloadLocalLlm,
  localLlmStatus,
  type LocalLlmStatus,
  removeLocalLlm,
  reportError,
} from "@/lib/api";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { StatusPill, UsedBy, linkCardClass } from "@/components/GlobalPanels";

const GB = 1024 ** 3;

/**
 * 端末内のLLM(Qwen 2.5 7B)の取得と削除。llama-serverとモデルは同梱せず、公式の配布物をアプリデータへ取得して使う。
 * 取得物は全ワークスペース共通。
 */
export default function LocalLlmPanel({ onChanged, usedBy = [] }: { onChanged: () => void; usedBy?: string[] }) {
  const t = useT();
  const [status, setStatus] = useState<LocalLlmStatus | null>(null);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [action, setAction] = useState<"download" | "remove" | null>(null);
  const [confirming, setConfirming] = useState(false);

  const refresh = useCallback(
    () =>
      localLlmStatus()
        .then(setStatus)
        .catch((e) => toast.error(reportError(e))),
    [],
  );
  useEffect(() => {
    refresh();
  }, [refresh]);

  // 別の画面で始めた取得が続いている間は、終わるまで状況を見直す
  const elsewhere = !!status?.downloading && action !== "download";
  useEffect(() => {
    if (!elsewhere) return;
    const timer = setInterval(refresh, 2000);
    return () => clearInterval(timer);
  }, [elsewhere, refresh]);

  const download = async () => {
    setAction("download");
    setProgress(null);
    try {
      await downloadLocalLlm(setProgress);
      toast.success(t("ローカルLLMを取得しました。"));
      onChanged();
    } catch (e) {
      if ((e as { code?: string })?.code === "download_cancelled") toast(t("取得を中断しました。"));
      else toast.error(reportError(e));
    } finally {
      setAction(null);
      setProgress(null);
      await refresh();
    }
  };

  const remove = async () => {
    setConfirming(false);
    setAction("remove");
    try {
      await removeLocalLlm();
      toast.success(t("ローカルLLMを削除しました。"));
      onChanged();
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setAction(null);
      await refresh();
    }
  };

  const installed = !!status?.installed;
  const busy = action !== null;
  const downloading = action === "download" || elsewhere;
  const percent = progress && progress.total > 0 ? Math.floor((progress.downloaded / progress.total) * 100) : null;
  const size = status ? (status.size / GB).toFixed(1) : "4.7";

  return (
    <div className={linkCardClass(installed)}>
      <div className="flex items-center gap-3">
        <span
          aria-hidden
          className={cn("size-2 shrink-0 rounded-full border", installed ? "border-primary bg-primary shadow-[0_0_8px_var(--primary)]" : "border-muted-foreground/50")}
        />
        <div className="min-w-0 flex-1">
          <div className="text-sm font-medium">{status?.label ?? "Qwen 2.5 7B (Local)"}</div>
          <div className="text-xs text-muted-foreground">{t("端末内で動くLLMです(キー不要・無料)。llama.cppとモデル(約{size}GB)をダウンロードします。", { size })}</div>
        </div>
        <span className="hidden sm:inline" aria-live="polite">
          {installed && <StatusPill on>linked</StatusPill>}
        </span>
        {installed && !downloading && (
          <Button type="button" variant="ghost" size="icon-sm" aria-label={t("削除")} disabled={busy} onClick={() => setConfirming((c) => !c)}>
            <Trash2 />
          </Button>
        )}
        {downloading ? (
          <Button type="button" variant="outline" size="sm" onClick={() => cancelLocalLlmDownload().catch((e) => toast.error(reportError(e)))}>
            <X /> {t("中断")}
          </Button>
        ) : (
          !installed && (
            <Button type="button" variant="outline" size="sm" disabled={busy} onClick={download}>
              <Download /> {t("ダウンロード")}
            </Button>
          )
        )}
      </div>
      <UsedBy features={usedBy} linked={installed} />
      {downloading && (
        <div className="mt-3 flex items-center gap-3 pl-5" role="status">
          <Loader2 className="size-3.5 shrink-0 animate-spin text-muted-foreground" />
          <div className="h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-muted">
            <div className="h-full bg-primary transition-[width]" style={{ width: `${percent ?? 0}%` }} />
          </div>
          <span className="w-24 shrink-0 text-right text-xs tabular-nums text-muted-foreground">
            {progress ? `${(progress.downloaded / GB).toFixed(2)} / ${(progress.total / GB).toFixed(1)} GB` : t("取得中…")}
          </span>
        </div>
      )}
      {confirming && (
        <div className="mt-3 flex items-center gap-2 pl-5 text-xs">
          <span className="min-w-0 flex-1">{t("取得したllama.cppとモデルを削除します。よろしいですか?")}</span>
          <Button type="button" size="sm" variant="destructive" disabled={busy} onClick={remove}>
            {t("削除")}
          </Button>
          <Button type="button" size="sm" variant="outline" onClick={() => setConfirming(false)}>
            {t("キャンセル")}
          </Button>
        </div>
      )}
      <ul className="mt-2 list-disc pl-9 text-xs text-muted-foreground">
        <li>{t("インターネット検索・ページ取得は使えません(ナレッジのファイル操作とコマンド実行は使えます)。")}</li>
        <li>{t("メモリは16GB以上を推奨します。最初の応答はモデルの読み込みに時間がかかります。")}</li>
        <li>{t("モデルはQwen 2.5 7B Instruct(Apache 2.0)、実行にはllama.cpp(MIT)を使います。")}</li>
      </ul>
    </div>
  );
}
