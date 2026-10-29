"use client";

import { useT } from "@/lib/i18n";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Download, FolderOpen, FileText, Loader2, RotateCcw, Sparkles, Trash2, Upload } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { ask, open as openDialog } from "@tauri-apps/plugin-dialog";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import ModelSelector from "@/components/ModelSelector";
import { cn } from "@/lib/utils";
import {
  type KeyStatus,
  type KnowledgeFile,
  type ModelInfo,
  type Route,
  getKeyStatus,
  listModels,
  loadBatchRoute,
  saveBatchRoute,
  knowledgeDir,
  listKnowledge,
  readKnowledge,
  reportError,
  exportKnowledge,
  importKnowledge,
  resetKnowledge,
  runBatch,
  setKnowledgeDir,
} from "@/lib/api";

/** 先頭のYAML Frontmatterを本文から分離し、タグだけ取り出す。 */
function splitFrontmatter(text: string): { tags: string[]; body: string } {
  const m = text.match(/^---\n([\s\S]*?)\n---\n/);
  if (!m) return { tags: [], body: text };
  let tags: string[] = [];
  const line = m[1].split("\n").find((l) => l.startsWith("tags: "));
  try {
    if (line) tags = JSON.parse(line.slice(6));
  } catch {
    // 手編集で壊れていてもタグ表示を省くだけ
  }
  return { tags, body: text.slice(m[0].length) };
}

export default function Knowledge() {
  const t = useT();
  const [files, setFiles] = useState<KnowledgeFile[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [content, setContent] = useState("");
  const [running, setRunning] = useState(false);
  const [dir, setDir] = useState("");
  const [route, setRoute] = useState<Route | null>(null);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [keyStatus, setKeyStatus] = useState<Partial<KeyStatus>>({});

  const refresh = useCallback(
    () => listKnowledge().then(setFiles).catch((e) => toast.error(reportError(e))),
    [],
  );
  useEffect(() => {
    refresh();
    knowledgeDir().then(setDir).catch(() => {});
    Promise.all([listModels(), loadBatchRoute(), getKeyStatus()])
      .then(([m, r, k]) => {
        setModels(m);
        setRoute(r);
        setKeyStatus(k);
      })
      .catch((e) => toast.error(reportError(e)));
  }, [refresh]);

  // 会話ごとのリアルタイム更新を一覧へ反映する
  useEffect(() => {
    const un = listen("knowledge-updated", () => {
      refresh();
    });
    return () => {
      un.then((f) => f());
    };
  }, [refresh]);

  const grouped = useMemo(() => {
    const m = new Map<string, KnowledgeFile[]>();
    for (const f of files) m.set(f.category, [...(m.get(f.category) ?? []), f]);
    return [...m.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [files]);

  const changeRoute = (next: Route) => {
    setRoute(next);
    saveBatchRoute(next).catch((e) => toast.error(reportError(e)));
  };

  const open = async (path: string) => {
    setSelected(path);
    try {
      setContent(await readKnowledge(path));
    } catch (e) {
      setContent("");
      toast.error(reportError(e));
    }
  };

  const organize = async () => {
    setRunning(true);
    try {
      const r = await runBatch();
      if (!r) toast.info(t("整理対象のログがないか、すでに実行中です。"));
      else {
        const msg = t("{processed_logs}件のログを整理し、{files_written}件のノートを更新しました。", { processed_logs: r.processed_logs, files_written: r.files_written });
        if (r.failed_segments) toast.warning(t("{msg}({failed_segments}件は失敗。次回再試行します)", { msg, failed_segments: r.failed_segments }));
        else toast.success(msg);
        await refresh();
      }
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setRunning(false);
    }
  };

  const changeDir = async (next: string | null) => {
    try {
      const path = await setKnowledgeDir(next);
      setDir(path);
      setSelected(null);
      setContent("");
      await refresh();
      toast.success(t("保存先を変更しました。既存のノートは移動されません。"));
    } catch (e) {
      toast.error(reportError(e));
    }
  };

  const pickDir = async () => {
    const picked = await openDialog({ directory: true, multiple: false, defaultPath: dir || undefined });
    if (typeof picked === "string") await changeDir(picked);
  };

  const exportAll = async () => {
    const picked = await openDialog({ directory: true, multiple: false, title: t("エクスポート先のフォルダ") });
    if (typeof picked !== "string") return;
    try {
      const r = await exportKnowledge(picked);
      toast.success(t("{files}件のノートを書き出しました: {dest}", { files: r.files, dest: r.dest ?? "" }));
    } catch (e) {
      toast.error(reportError(e));
    }
  };

  const importAll = async () => {
    const picked = await openDialog({ directory: true, multiple: false, title: t("インポートするフォルダ") });
    if (typeof picked !== "string") return;
    try {
      const r = await importKnowledge(picked);
      const msg = t("{files}件のノートを取り込みました。", { files: r.files });
      if (r.skipped) toast.warning(t("{msg}({skipped}件は既存または未対応の名前のため除外)", { msg, skipped: r.skipped }));
      else toast.success(msg);
      await refresh();
    } catch (e) {
      toast.error(reportError(e));
    }
  };

  const resetAll = async () => {
    const ok = await ask(t("すべてのノート(Markdown)を削除します。この操作は元に戻せません。先にエクスポートすることをおすすめします。続行しますか?"), {
      title: t("ナレッジの初期化"),
      kind: "warning",
      okLabel: t("削除する"),
      cancelLabel: t("キャンセル"),
    });
    if (!ok) return;
    try {
      const r = await resetKnowledge();
      setSelected(null);
      setContent("");
      await refresh();
      toast.success(t("{files}件のノートを削除しました。", { files: r.files }));
    } catch (e) {
      toast.error(reportError(e));
    }
  };

  const doc = selected ? splitFrontmatter(content) : null;

  return (
    <div className="flex h-full">
      <aside className="flex w-72 shrink-0 flex-col border-r bg-background/40 backdrop-blur">
        <div className="flex items-center justify-between border-b px-4 py-3">
          <h1 className="hud-label text-[0.8rem] font-semibold text-foreground">Knowledge</h1>
          <Button size="sm" disabled={running} onClick={organize}>
            {running ? <Loader2 className="animate-spin" /> : <Sparkles />}
            {running ? t("整理中…") : t("今すぐ整理")}
          </Button>
        </div>
        <div className="flex-1 overflow-y-auto p-3 text-sm">
          {grouped.length === 0 && (
            <p className="px-2 py-6 text-center text-muted-foreground">{t("ノートはまだありません。")}</p>
          )}
          {grouped.map(([cat, items]) => (
            <section key={cat} className="mb-4">
              <h2 className="mb-1 px-2 text-xs font-medium uppercase tracking-wide text-muted-foreground">{cat}</h2>
              <ul className="flex flex-col gap-0.5">
                {items.map((f) => (
                  <li key={f.path}>
                    <button
                      className={cn(
                        "flex w-full items-center gap-2 truncate rounded-md px-2 py-1.5 text-left transition-colors hover:bg-muted",
                        selected === f.path && "bg-muted font-medium",
                      )}
                      onClick={() => open(f.path)}
                    >
                      <FileText className="size-3.5 shrink-0 text-muted-foreground" />
                      <span className="truncate">{f.title || f.path}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          ))}
        </div>
        <div className="border-t px-4 py-2 text-xs text-muted-foreground">
          <p className="mb-2">{t("整理に使うモデル")}</p>
          <ModelSelector route={route} models={models} keyStatus={keyStatus} onChange={changeRoute} />
        </div>
        <div className="flex flex-wrap gap-2 border-t px-4 py-2">
          <Button size="sm" variant="outline" onClick={exportAll}>
            <Download /> {t("エクスポート")}
          </Button>
          <Button size="sm" variant="outline" onClick={importAll}>
            <Upload /> {t("インポート")}
          </Button>
          <Button size="sm" variant="ghost" className="text-destructive" onClick={resetAll}>
            <Trash2 /> {t("初期化")}
          </Button>
        </div>
        {dir && (
          <div className="border-t px-4 py-2 text-xs text-muted-foreground">
            <p className="break-all" title={t("Obsidianのvaultとして開けます")}>
              {t("保存先:")} {dir}
            </p>
            <div className="mt-2 flex gap-2">
              <Button size="sm" variant="outline" onClick={pickDir}>
                <FolderOpen /> {t("変更")}
              </Button>
              <Button size="sm" variant="ghost" onClick={() => changeDir(null)}>
                <RotateCcw /> {t("既定に戻す")}
              </Button>
            </div>
          </div>
        )}
      </aside>
      <article className="flex-1 overflow-y-auto px-10 py-8">
        {doc ? (
          <div className="mx-auto max-w-3xl">
            {doc.tags.length > 0 && (
              <div className="mb-4 flex flex-wrap gap-1.5">
                {doc.tags.map((t) => (
                  <Badge key={t} variant="secondary">{t}</Badge>
                ))}
              </div>
            )}
            <div className="prose prose-sm max-w-none dark:prose-invert">
              <ReactMarkdown remarkPlugins={[remarkGfm]}>{doc.body}</ReactMarkdown>
            </div>
          </div>
        ) : (
          <div className="mt-32 text-center text-sm text-muted-foreground">{t("ノートを選択してください。")}</div>
        )}
      </article>
    </div>
  );
}
