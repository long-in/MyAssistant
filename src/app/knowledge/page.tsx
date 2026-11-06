"use client";

import { useT } from "@/lib/i18n";
import { useCallback, useEffect, useState } from "react";
import { Download, Eye, FolderOpen, Loader2, Pencil, RotateCcw, Save, Sparkles, Trash2, Upload } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { ask, open as openDialog } from "@tauri-apps/plugin-dialog";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import KnowledgeTree, { type NodeKind } from "@/components/KnowledgeTree";
import ModelSelector from "@/components/ModelSelector";
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
  listKnowledgeDirs,
  listOtherKnowledge,
  readOtherKnowledge,
  readKnowledge,
  writeKnowledge,
  createKnowledgeFile,
  createKnowledgeDir,
  renameKnowledge,
  deleteKnowledge,
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

/** 編集できるのはMarkdownのみ。それ以外は閲覧専用。 */
const isNote = (path: string) => path.endsWith(".md");

export default function Knowledge() {
  const t = useT();
  const [files, setFiles] = useState<KnowledgeFile[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [dirs, setDirs] = useState<string[]>([]);
  const [content, setContent] = useState("");
  // 編集中の本文。`content`(保存済み)と異なれば未保存
  const [draft, setDraft] = useState("");
  const [editMode, setEditMode] = useState(false);
  const [saving, setSaving] = useState(false);
  const [running, setRunning] = useState(false);
  const [dir, setDir] = useState("");
  const [route, setRoute] = useState<Route | null>(null);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [keyStatus, setKeyStatus] = useState<Partial<KeyStatus>>({});

  const refresh = useCallback(
    () =>
      Promise.all([listKnowledge(), listOtherKnowledge(), listKnowledgeDirs()])
        .then(([f, o, d]) => {
          setFiles([...f, ...o]);
          setDirs(d);
        })
        .catch((e) => toast.error(reportError(e))),
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

  const changeRoute = (next: Route) => {
    setRoute(next);
    saveBatchRoute(next).catch((e) => toast.error(reportError(e)));
  };

  const dirty = selected !== null && draft !== content;

  /** 未保存の編集があれば破棄してよいか確認する。 */
  const confirmDiscard = async () =>
    !dirty ||
    (await ask(t("保存していない変更があります。破棄しますか?"), {
      title: t("未保存の変更"),
      kind: "warning",
      okLabel: t("破棄する"),
      cancelLabel: t("キャンセル"),
    }));

  const open = async (path: string) => {
    if (path === selected || !(await confirmDiscard())) return;
    try {
      const text = await (isNote(path) ? readKnowledge(path) : readOtherKnowledge(path));
      setSelected(path);
      setContent(text);
      setDraft(text);
      setEditMode(false);
    } catch (e) {
      toast.error(reportError(e));
    }
  };

  const clearSelection = () => {
    setSelected(null);
    setContent("");
    setDraft("");
    setEditMode(false);
  };

  const save = async () => {
    if (!selected || !dirty || saving) return;
    setSaving(true);
    try {
      await writeKnowledge(selected, draft);
      setContent(draft);
      await refresh();
      toast.success(t("保存しました。"));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setSaving(false);
    }
  };

  // 同じパスか配下かを判定する(フォルダ操作の影響範囲)
  const within = (path: string, base: string) => path === base || path.startsWith(`${base}/`);

  const createNode = async (kind: NodeKind, path: string) => {
    try {
      if (kind === "dir") await createKnowledgeDir(path);
      else {
        await createKnowledgeFile(path);
        await refresh();
        await open(path);
        setEditMode(true);
        return true;
      }
      await refresh();
      return true;
    } catch (e) {
      toast.error(reportError(e));
      return false;
    }
  };

  const renameNode = async (kind: NodeKind, from: string, to: string) => {
    if (selected && within(selected, from) && !(await confirmDiscard())) return false;
    try {
      await renameKnowledge(from, to);
      if (selected && within(selected, from)) {
        const moved = to + selected.slice(from.length);
        setSelected(moved);
        setDraft(content);
      }
      await refresh();
      return true;
    } catch (e) {
      toast.error(reportError(e));
      return false;
    }
  };

  const deleteNode = async (kind: NodeKind, path: string) => {
    const name = path.split("/").pop() ?? path;
    const ok = await ask(
      kind === "dir"
        ? t("フォルダ「{name}」と中のノートをすべて削除します。この操作は元に戻せません。続行しますか?", { name })
        : t("ノート「{name}」を削除します。この操作は元に戻せません。続行しますか?", { name }),
      { title: t("削除の確認"), kind: "warning", okLabel: t("削除する"), cancelLabel: t("キャンセル") },
    );
    if (!ok) return;
    try {
      await deleteKnowledge(path);
      if (selected && within(selected, path)) clearSelection();
      await refresh();
    } catch (e) {
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
    if (!(await confirmDiscard())) return;
    try {
      const path = await setKnowledgeDir(next);
      setDir(path);
      clearSelection();
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
      clearSelection();
      await refresh();
      toast.success(t("{files}件のノートを削除しました。", { files: r.files }));
    } catch (e) {
      toast.error(reportError(e));
    }
  };

  const readOnly = selected !== null && !isNote(selected);
  const doc = selected ? splitFrontmatter(draft) : null;

  // Cmd/Ctrl+Sで保存
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "s") {
        e.preventDefault();
        save();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  return (
    <div className="flex h-full">
      <aside className="flex w-72 shrink-0 flex-col border-r bg-background/40 backdrop-blur">
        <div className="border-b px-4 py-3">
          <h1 className="hud-label mb-3 text-[0.8rem] font-semibold text-foreground">Knowledge</h1>
          <p className="mb-2 text-xs text-muted-foreground">{t("整理に使うモデル")}</p>
          <ModelSelector route={route} models={models} keyStatus={keyStatus} onChange={changeRoute} />
          <Button size="sm" className="mt-2 w-full" disabled={running} onClick={organize}>
            {running ? <Loader2 className="animate-spin" /> : <Sparkles />}
            {running ? t("整理中…") : t("今すぐ整理")}
          </Button>
        </div>
        <div className="flex-1 overflow-y-auto py-2">
          <KnowledgeTree
            files={files}
            dirs={dirs}
            selected={selected}
            onOpen={open}
            onCreate={createNode}
            onRename={renameNode}
            onDelete={deleteNode}
            onInvalidName={() => toast.error(t("名前には英数字・日本語・「_」「-」「.」のみ使えます(フォルダは「.」不可)。"))}
          />
          {files.length === 0 && dirs.length === 0 && (
            <p className="px-4 py-6 text-center text-sm text-muted-foreground">{t("ノートはまだありません。")}</p>
          )}
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
      <article className="flex min-w-0 flex-1 flex-col">
        {doc && selected ? (
          <>
            <div className="flex items-center justify-between gap-3 border-b px-4 py-2">
              <span className="truncate text-sm text-muted-foreground">
                {selected}
                {dirty && <span className="ml-2 text-foreground">●</span>}
              </span>
              {!readOnly && (
              <div className="flex shrink-0 gap-2">
                <Button size="sm" variant="outline" onClick={() => setEditMode((v) => !v)}>
                  {editMode ? <Eye /> : <Pencil />}
                  {editMode ? t("プレビュー") : t("編集")}
                </Button>
                <Button size="sm" disabled={!dirty || saving} onClick={save}>
                  {saving ? <Loader2 className="animate-spin" /> : <Save />}
                  {t("保存")}
                </Button>
              </div>
              )}
            </div>
            {readOnly ? (
              <pre className="flex-1 overflow-auto whitespace-pre-wrap break-all px-10 py-6 font-mono text-xs leading-relaxed">{content}</pre>
            ) : editMode ? (
              <textarea
                className="flex-1 resize-none bg-transparent px-10 py-6 font-mono text-sm leading-relaxed outline-none"
                value={draft}
                spellCheck={false}
                onChange={(e) => setDraft(e.target.value)}
              />
            ) : (
              <div className="flex-1 overflow-y-auto px-10 py-8">
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
              </div>
            )}
          </>
        ) : (
          <div className="mt-32 text-center text-sm text-muted-foreground">{t("ノートを選択してください。")}</div>
        )}
      </article>
    </div>
  );
}
