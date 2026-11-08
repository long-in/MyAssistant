"use client";

import { useT } from "@/lib/i18n";
import { useCallback, useEffect, useState } from "react";
import { BookOpen, ChevronDown, ChevronRight, Eye, FileText, FolderOpen, Library, Loader2, Pencil, Save } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { ask } from "@tauri-apps/plugin-dialog";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import KnowledgeTree, { type NodeKind } from "@/components/KnowledgeTree";
import { EmptyState, PaneHeader, StatusPill } from "@/components/GlobalPanels";
import {
  type SpaceKnowledge,
  createKnowledgeDirIn,
  createKnowledgeFileIn,
  deleteKnowledgeIn,
  listAllKnowledge,
  readKnowledgeIn,
  renameKnowledgeIn,
  reportError,
  revealKnowledgeInFinder,
  writeKnowledgeIn,
} from "@/lib/api";
import { splitFrontmatter } from "@/lib/knowledgeDoc";

/** 編集できるのはMarkdownのみ。それ以外は閲覧専用。 */
const isNote = (path: string) => path.endsWith(".md");
/** 同じパスか配下かを判定する(フォルダ操作の影響範囲)。 */
const within = (path: string, base: string) => path === base || path.startsWith(`${base}/`);

type Sel = { spaceId: string; path: string };

/** 全スペースのナレッジを、スペースを切り替えずに一覧・作成・閲覧・編集・削除する。 */
export default function AllKnowledge() {
  const t = useT();
  const [spaces, setSpaces] = useState<SpaceKnowledge[]>([]);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [selected, setSelected] = useState<Sel | null>(null);
  const [content, setContent] = useState("");
  // 編集中の本文。`content`(保存済み)と異なれば未保存
  const [draft, setDraft] = useState("");
  const [editMode, setEditMode] = useState(false);
  const [saving, setSaving] = useState(false);

  const refresh = useCallback(
    () =>
      listAllKnowledge()
        .then(setSpaces)
        .catch((e) => toast.error(reportError(e))),
    [],
  );
  useEffect(() => {
    refresh();
  }, [refresh]);
  // 会話ごとのリアルタイム更新(選択中のスペース分)を反映する
  useEffect(() => {
    const un = listen("knowledge-updated", () => {
      refresh();
    });
    return () => {
      un.then((f) => f());
    };
  }, [refresh]);

  const dirty = selected !== null && draft !== content;
  const confirmDiscard = async () =>
    !dirty ||
    (await ask(t("保存していない変更があります。破棄しますか?"), {
      title: t("未保存の変更"),
      kind: "warning",
      okLabel: t("破棄する"),
      cancelLabel: t("キャンセル"),
    }));

  const open = async (spaceId: string, path: string) => {
    if (selected?.spaceId === spaceId && selected.path === path) return;
    if (!(await confirmDiscard())) return;
    try {
      const text = await readKnowledgeIn(spaceId, path);
      setSelected({ spaceId, path });
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
      await writeKnowledgeIn(selected.spaceId, selected.path, draft);
      setContent(draft);
      await refresh();
      toast.success(t("保存しました。"));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setSaving(false);
    }
  };

  const createNode = async (spaceId: string, kind: NodeKind, path: string) => {
    try {
      if (kind === "dir") {
        await createKnowledgeDirIn(spaceId, path);
        await refresh();
      } else {
        await createKnowledgeFileIn(spaceId, path);
        await refresh();
        await open(spaceId, path);
        setEditMode(true);
      }
      return true;
    } catch (e) {
      toast.error(reportError(e));
      return false;
    }
  };

  const renameNode = async (spaceId: string, _kind: NodeKind, from: string, to: string) => {
    const affected = selected?.spaceId === spaceId && within(selected.path, from);
    if (affected && !(await confirmDiscard())) return false;
    try {
      await renameKnowledgeIn(spaceId, from, to);
      if (affected && selected) {
        setSelected({ spaceId, path: to + selected.path.slice(from.length) });
        setDraft(content);
      }
      await refresh();
      return true;
    } catch (e) {
      toast.error(reportError(e));
      return false;
    }
  };

  const deleteNode = async (space: SpaceKnowledge, kind: NodeKind, path: string) => {
    const name = path.split("/").pop() ?? path;
    const ok = await ask(
      kind === "dir"
        ? t("フォルダ「{name}」と中のノートをすべて削除します。この操作は元に戻せません。続行しますか?", { name })
        : t("ノート「{name}」を削除します。この操作は元に戻せません。続行しますか?", { name }),
      { title: `${t("削除の確認")} (${space.name})`, kind: "warning", okLabel: t("削除する"), cancelLabel: t("キャンセル") },
    );
    if (!ok) return;
    try {
      await deleteKnowledgeIn(space.id, path);
      if (selected?.spaceId === space.id && within(selected.path, path)) clearSelection();
      await refresh();
    } catch (e) {
      toast.error(reportError(e));
    }
  };

  const reveal = (spaceId: string, path?: string) =>
    revealKnowledgeInFinder(spaceId, path).catch((e) => toast.error(reportError(e)));

  const toggle = (id: string) =>
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });

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

  const total = spaces.reduce((n, s) => n + s.files.length, 0);
  const readOnly = selected !== null && !isNote(selected.path);
  const doc = selected ? splitFrontmatter(draft) : null;
  const selectedSpace = selected ? spaces.find((s) => s.id === selected.spaceId) : undefined;

  return (
    <div className="flex h-full">
      <aside className="flex w-80 shrink-0 flex-col border-r bg-background/40 backdrop-blur">
        <PaneHeader
          icon={Library}
          title="All Knowledge bases"
          chips={<StatusPill on={total > 0}>{t("ノート {n}件", { n: total })}</StatusPill>}
        />
        <div className="flex-1 overflow-y-auto py-2">
          {spaces.map((space) => {
            const isCollapsed = collapsed.has(space.id);
            return (
              <section key={space.id} className="mb-2">
                <div className="flex items-center gap-1 px-2 py-1">
                  <button className="flex min-w-0 flex-1 items-center gap-1.5 text-left text-sm font-medium" onClick={() => toggle(space.id)}>
                    {isCollapsed ? <ChevronRight className="size-4 shrink-0" /> : <ChevronDown className="size-4 shrink-0" />}
                    <span className="size-2.5 shrink-0 rounded-full" style={{ background: space.color ?? "var(--primary)" }} />
                    <span className="truncate">{space.name}</span>
                    <span className="text-xs text-muted-foreground">{space.files.length}</span>
                  </button>
                  <button
                    className="rounded p-1 text-muted-foreground hover:bg-muted hover:text-foreground"
                    title={t("Finderで開く")}
                    onClick={() => reveal(space.id)}
                  >
                    <FolderOpen className="size-4" />
                  </button>
                </div>
                {!isCollapsed &&
                  (space.error ? (
                    <p className="px-4 py-2 text-xs text-destructive">{space.error}</p>
                  ) : (
                    <KnowledgeTree
                      files={space.files}
                      dirs={space.dirs}
                      selected={selected?.spaceId === space.id ? selected.path : null}
                      onOpen={(p) => open(space.id, p)}
                      onCreate={(k, p) => createNode(space.id, k, p)}
                      onRename={(k, from, to) => renameNode(space.id, k, from, to)}
                      onDelete={(k, p) => deleteNode(space, k, p)}
                      onInvalidName={() => toast.error(t("名前には英数字・日本語・「_」「-」「.」のみ使えます(フォルダは「.」不可)。"))}
                    />
                  ))}
              </section>
            );
          })}
        </div>
      </aside>
      <article className="flex min-w-0 flex-1 flex-col">
        {doc && selected ? (
          <>
            <div className="flex items-center justify-between gap-3 border-b px-4 py-2">
              <span className="flex min-w-0 items-center gap-2 text-sm">
                <FileText className="size-4 shrink-0 text-primary" />
                <Badge variant="secondary">{selectedSpace?.name ?? selected.spaceId}</Badge>
                <span className="truncate">{selected.path}</span>
                {dirty && <StatusPill on>{t("未保存")}</StatusPill>}
                {readOnly && <StatusPill on={false}>{t("読み取り専用")}</StatusPill>}
              </span>
              <div className="flex shrink-0 gap-2">
                <Button size="sm" variant="outline" onClick={() => reveal(selected.spaceId, selected.path)}>
                  <FolderOpen /> {t("Finderで表示")}
                </Button>
                {!readOnly && (
                  <>
                    <Button size="sm" variant="outline" onClick={() => setEditMode((v) => !v)}>
                      {editMode ? <Eye /> : <Pencil />}
                      {editMode ? t("プレビュー") : t("編集")}
                    </Button>
                    <Button size="sm" disabled={!dirty || saving} onClick={save}>
                      {saving ? <Loader2 className="animate-spin" /> : <Save />}
                      {t("保存")}
                    </Button>
                  </>
                )}
              </div>
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
                      {doc.tags.map((tag) => (
                        <Badge key={tag} variant="secondary">{tag}</Badge>
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
          <EmptyState icon={BookOpen}>{t("ノートを選択してください。")}</EmptyState>
        )}
      </article>
    </div>
  );
}
