"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { Loader2 } from "lucide-react";
import { useT } from "@/lib/i18n";
import type { DownloadProgress } from "@/lib/api";
import { cn } from "@/lib/utils";

const MB = 1024 ** 2;
const GB = 1024 ** 3;

/** バイト数を、1GB未満はMB、それ以上はGBで表す。 */
function size(bytes: number): string {
  return bytes >= GB ? `${(bytes / GB).toFixed(2)} GB` : `${Math.round(bytes / MB)} MB`;
}

/** 取得の進み具合を、バー・割合(整数%)・取得量で表す。合計が分からない間は「取得中…」だけを出す。 */
export function DownloadBar({ progress, className }: { progress: DownloadProgress | null; className?: string }) {
  const t = useT();
  const percent = progress && progress.total > 0 ? Math.min(100, Math.floor((progress.downloaded / progress.total) * 100)) : null;
  return (
    <div className={cn("flex items-center gap-3", className)} role="status">
      <Loader2 className="size-3.5 shrink-0 animate-spin text-muted-foreground" />
      <div className="h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-muted">
        <div className="h-full bg-primary transition-[width]" style={{ width: `${percent ?? 0}%` }} />
      </div>
      <span className="shrink-0 whitespace-nowrap text-right text-xs tabular-nums text-muted-foreground">
        {progress && percent !== null ? (progress.bytes ? `${percent}% · ${size(progress.downloaded)} / ${size(progress.total)}` : `${percent}%`) : t("取得中…")}
      </span>
    </div>
  );
}

/**
 * バックエンドが持つ取得の進み具合を追う。
 * 画面を切り替えても取得は続くので、開いた時点で問い合わせ、取得中の間は1秒ごとに見直す。
 * `local`はこの画面から取得を始めて待っている間。別の画面で始めた取得が終わったら`onFinished`を呼ぶ。
 */
export function useDownloadProgress(fetchProgress: () => Promise<DownloadProgress | null>, local: boolean, onFinished?: () => void) {
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const was = useRef(false);
  const finished = useRef(onFinished);
  const own = useRef(local);
  useEffect(() => {
    finished.current = onFinished;
    own.current = local;
  });
  const tick = useCallback(
    () =>
      fetchProgress()
        .then((p) => {
          if (was.current && !p && !own.current) finished.current?.();
          was.current = !!p;
          setProgress(p);
        })
        .catch(() => {}),
    [fetchProgress],
  );
  useEffect(() => {
    void tick();
  }, [tick]);
  const active = local || progress !== null;
  useEffect(() => {
    if (!active) return;
    const timer = setInterval(tick, 1000);
    return () => clearInterval(timer);
  }, [active, tick]);
  return { progress, active };
}
