"use client";

import { useT } from "@/lib/i18n";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useRouter } from "next/navigation";
import { Loader2, RefreshCw, Trash2 } from "lucide-react";
import { ask } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import {
  type DebugLogFile,
  deleteDebugLogs,
  getDebugMode,
  listDebugLogs,
  readDebugLog,
  reportError,
} from "@/lib/api";

type Entry = { time: string; kind: string; data: Record<string, unknown> };

const KIND_LABELS: Record<string, string> = { api: "API", transcription: "文字起こし" };

/** JSON Linesを読む。壊れた行は捨てず、本文をそのまま表示できる形で残す。 */
function parseLog(text: string): Entry[] {
  return text
    .split("\n")
    .filter(Boolean)
    .map((line) => {
      try {
        return JSON.parse(line) as Entry;
      } catch {
        return { time: "", kind: "raw", data: { line } };
      }
    })
    .reverse();
}

/** 一覧に出す1行の要約。 */
function summary(e: Entry): string {
  const d = e.data;
  if (e.kind === "api") return `${d.api} ${d.status}`;
  if (e.kind === "transcription") return String(d.text ?? "");
  return "";
}

const fmtTime = (t: string) => (t ? new Date(t).toLocaleTimeString("ja-JP") : "");
const fmtSize = (n: number) => (n < 1024 ? `${n} B` : n < 1024 * 1024 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`);

export default function Debug() {
  const t = useT();
  const router = useRouter();
  const [files, setFiles] = useState<DebugLogFile[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [entries, setEntries] = useState<Entry[]>([]);
  const [loading, setLoading] = useState(false);
  const [kind, setKind] = useState<string>("all");

  const load = useCallback(async (name: string | null) => {
    if (!name) return setEntries([]);
    setLoading(true);
    try {
      setEntries(parseLog(await readDebugLog(name)));
    } catch (e) {
      setEntries([]);
      toast.error(reportError(e));
    } finally {
      setLoading(false);
    }
  }, []);

  const refresh = useCallback(
    async (keep?: string | null) => {
      try {
        const list = await listDebugLogs();
        setFiles(list);
        const next = list.some((f) => f.name === keep) ? keep! : (list[0]?.name ?? null);
        setSelected(next);
        await load(next);
      } catch (e) {
        toast.error(reportError(e));
      }
    },
    [load],
  );

  useEffect(() => {
    // デバッグモードが無効なら、このページは開かない
    getDebugMode()
      .then((on) => (on ? refresh() : router.replace("/setup")))
      .catch(() => router.replace("/setup"));
  }, [refresh, router]);

  const kinds = useMemo(() => ["all", ...new Set(entries.map((e) => e.kind))], [entries]);
  const shown = kind === "all" ? entries : entries.filter((e) => e.kind === kind);

  const remove = async (name?: string) => {
    const ok = await ask(name ? t("{name} を削除します。元に戻せません。", { name }) : t("すべてのデバッグログを削除します。元に戻せません。"), {
      title: t("ログの削除"),
      kind: "warning",
      okLabel: t("削除する"),
      cancelLabel: t("キャンセル"),
    });
    if (!ok) return;
    try {
      const n = await deleteDebugLogs(name);
      toast.success(t("{n}件のログを削除しました。", { n }));
      await refresh(name ? null : undefined);
    } catch (e) {
      toast.error(reportError(e));
    }
  };

  return (
    <div className="flex h-full">
      <aside className="flex w-64 shrink-0 flex-col border-r bg-background/40 backdrop-blur">
        <div className="flex items-center justify-between border-b px-4 py-3">
          <h1 className="hud-label text-[0.8rem] font-semibold text-foreground">Debug Logs</h1>
          <Button size="sm" variant="ghost" onClick={() => refresh(selected)} aria-label={t("再読み込み")}>
            <RefreshCw />
          </Button>
        </div>
        <div className="flex-1 overflow-y-auto p-3 text-sm">
          {files.length === 0 && <p className="px-2 py-6 text-center text-muted-foreground">{t("ログはまだありません。")}</p>}
          {files.map((f) => (
            <button
              key={f.name}
              onClick={() => {
                setSelected(f.name);
                load(f.name);
              }}
              className={cn(
                "flex w-full items-center justify-between rounded-sm px-2 py-1.5 text-left hover:bg-primary/10",
                selected === f.name && "bg-primary/10 text-primary",
              )}
            >
              <span>{f.name.replace(".jsonl", "")}</span>
              <span className="text-xs text-muted-foreground">{fmtSize(f.bytes)}</span>
            </button>
          ))}
        </div>
        <div className="flex flex-col gap-2 border-t p-3">
          <Button size="sm" variant="outline" disabled={!selected} onClick={() => remove(selected!)}>
            <Trash2 /> {t("選択中のログを削除")}
          </Button>
          <Button size="sm" variant="outline" disabled={files.length === 0} onClick={() => remove()}>
            <Trash2 /> {t("すべて削除")}
          </Button>
        </div>
      </aside>

      <section className="flex min-w-0 flex-1 flex-col">
        <div className="flex items-center gap-2 border-b px-4 py-3">
          {kinds.map((k) => (
            <Button key={k} size="sm" variant={kind === k ? "default" : "outline"} onClick={() => setKind(k)}>
              {k === "all" ? t("すべて") : t(KIND_LABELS[k] ?? k)}
            </Button>
          ))}
          <span className="ml-auto text-xs text-muted-foreground">{t("{n}件(新しい順)", { n: shown.length })}</span>
        </div>
        <div className="flex-1 space-y-2 overflow-y-auto p-4">
          {loading && <Loader2 className="mx-auto animate-spin text-muted-foreground" />}
          {!loading && shown.length === 0 && <p className="py-10 text-center text-sm text-muted-foreground">{t("表示できる記録がありません。")}</p>}
          {shown.map((e, i) => (
            <details key={`${e.time}-${i}`} className="hud-panel px-4 py-2 text-sm">
              <summary className="flex cursor-pointer items-center gap-3">
                <Badge variant="outline">{t(KIND_LABELS[e.kind] ?? e.kind)}</Badge>
                <span className="text-xs text-muted-foreground">{fmtTime(e.time)}</span>
                <span className="min-w-0 flex-1 truncate">{summary(e)}</span>
              </summary>
              <pre className="mt-2 max-h-96 overflow-auto whitespace-pre-wrap break-words text-xs">
                {JSON.stringify(e.data, null, 2)}
              </pre>
            </details>
          ))}
        </div>
      </section>
    </div>
  );
}
