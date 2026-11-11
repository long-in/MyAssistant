"use client";

import { useEditShortcuts } from "@/lib/useEditShortcuts";
import { Kbd } from "@/components/Kbd";
import { useT } from "@/lib/i18n";
import { useCallback, useEffect, useState } from "react";
import { BookOpen, ChevronDown, ChevronRight, FileText, FolderOpen, Library, Loader2, Pencil, Save } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { ask } from "@tauri-apps/plugin-dialog";
import { ChatMarkdown } from "@/components/ChatMarkdown";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import KnowledgeTree, { type NodeKind } from "@/components/KnowledgeTree";
import { EmptyState, PaneHeader, StatusPill } from "@/components/GlobalPanels";
import {
  type AgentKnowledge,
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

type Sel = { agentId: string; path: string };

/** 全エージェントのナレッジを、エージェントを切り替えずに一覧・作成・閲覧・編集・削除する。 */
export default function AllKnowledge() {
  const t = useT();
  const [agents, setAgents] = useState<AgentKnowledge[]>([]);
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
        .then(setAgents)
        .catch((e) => toast.error(reportError(e))),
    [],
  );
  useEffect(() => {
    refresh();
  }, [refresh]);
  // 会話ごとのリアルタイム更新(選択中のエージェント分)を反映する
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

  const open = async (agentId: string, path: string) => {
    if (selected?.agentId === agentId && selected.path === path) return;
    if (!(await confirmDiscard())) return;
    try {
      const text = await readKnowledgeIn(agentId, path);
      setSelected({ agentId, path });
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
    if (!selected || saving) return;
    if (!dirty) {
      setEditMode(false); // 変更が無ければ保存せず閲覧に戻る
      return;
    }
    setSaving(true);
    try {
      await writeKnowledgeIn(selected.agentId, selected.path, draft);
      setContent(draft);
      await refresh();
      setEditMode(false);
      toast.success(t("保存しました。"));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setSaving(false);
    }
  };

  const createNode = async (agentId: string, kind: NodeKind, path: string) => {
    try {
      if (kind === "dir") {
        await createKnowledgeDirIn(agentId, path);
        await refresh();
      } else {
        await createKnowledgeFileIn(agentId, path);
        await refresh();
        await open(agentId, path);
        setEditMode(true);
      }
      return true;
    } catch (e) {
      toast.error(reportError(e));
      return false;
    }
  };

  const renameNode = async (agentId: string, _kind: NodeKind, from: string, to: string) => {
    const affected = selected?.agentId === agentId && within(selected.path, from);
    if (affected && !(await confirmDiscard())) return false;
    try {
      await renameKnowledgeIn(agentId, from, to);
      if (affected && selected) {
        setSelected({ agentId, path: to + selected.path.slice(from.length) });
        setDraft(content);
      }
      await refresh();
      return true;
    } catch (e) {
      toast.error(reportError(e));
      return false;
    }
  };

  const deleteNode = async (agent: AgentKnowledge, kind: NodeKind, path: string) => {
    const name = path.split("/").pop() ?? path;
    const ok = await ask(
      kind === "dir"
        ? t("フォルダ「{name}」と中のノートをすべて削除します。この操作は元に戻せません。続行しますか?", { name })
        : t("ノート「{name}」を削除します。この操作は元に戻せません。続行しますか?", { name }),
      { title: `${t("削除の確認")} (${agent.name})`, kind: "warning", okLabel: t("削除する"), cancelLabel: t("キャンセル") },
    );
    if (!ok) return;
    try {
      await deleteKnowledgeIn(agent.id, path);
      if (selected?.agentId === agent.id && within(selected.path, path)) clearSelection();
      await refresh();
    } catch (e) {
      toast.error(reportError(e));
    }
  };

  const reveal = (agentId: string, path?: string) =>
    revealKnowledgeInFinder(agentId, path).catch((e) => toast.error(reportError(e)));

  const toggle = (id: string) =>
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });


  const total = agents.reduce((n, s) => n + s.files.length, 0);
  const readOnly = selected !== null && !isNote(selected.path);
  const doc = selected ? splitFrontmatter(draft) : null;

  // `e`で編集、Cmd/Ctrl+Sで保存
  useEditShortcuts({ canEdit: selected !== null && !readOnly, editMode, onEdit: () => setEditMode(true), onSave: save });
  const selectedAgent = selected ? agents.find((s) => s.id === selected.agentId) : undefined;

  return (
    <div className="flex h-full">
      <aside className="flex w-80 shrink-0 flex-col border-r bg-background/40 backdrop-blur">
        <PaneHeader
          icon={Library}
          title="All Knowledge bases"
          chips={<StatusPill on={total > 0}>{t("ノート {n}件", { n: total })}</StatusPill>}
        />
        <div className="flex-1 overflow-y-auto py-2">
          {agents.map((agent) => {
            const isCollapsed = collapsed.has(agent.id);
            return (
              <section key={agent.id} className="mb-2">
                <div className="flex items-center gap-1 px-2 py-1">
                  <button className="flex min-w-0 flex-1 items-center gap-1.5 text-left text-sm font-medium" onClick={() => toggle(agent.id)}>
                    {isCollapsed ? <ChevronRight className="size-4 shrink-0" /> : <ChevronDown className="size-4 shrink-0" />}
                    <span className="size-2.5 shrink-0 rounded-full" style={{ background: agent.color ?? "var(--primary)" }} />
                    <span className="truncate">{agent.name}</span>
                    <span className="text-xs text-muted-foreground">{agent.files.length}</span>
                  </button>
                  <button
                    className="rounded p-1 text-muted-foreground hover:bg-muted hover:text-foreground"
                    title={t("Finderで開く")}
                    onClick={() => reveal(agent.id)}
                  >
                    <FolderOpen className="size-4" />
                  </button>
                </div>
                {!isCollapsed &&
                  (agent.error ? (
                    <p className="px-4 py-2 text-xs text-destructive">{agent.error}</p>
                  ) : (
                    <KnowledgeTree
                      files={agent.files}
                      dirs={agent.dirs}
                      selected={selected?.agentId === agent.id ? selected.path : null}
                      onOpen={(p) => open(agent.id, p)}
                      onCreate={(k, p) => createNode(agent.id, k, p)}
                      onRename={(k, from, to) => renameNode(agent.id, k, from, to)}
                      onDelete={(k, p) => deleteNode(agent, k, p)}
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
                <Badge variant="secondary">{selectedAgent?.name ?? selected.agentId}</Badge>
                <span className="truncate">{selected.path}</span>
                {dirty && <StatusPill on>{t("未保存")}</StatusPill>}
                {readOnly && <StatusPill on={false}>{t("読み取り専用")}</StatusPill>}
              </span>
              <div className="flex shrink-0 gap-2">
                <Button size="sm" variant="outline" onClick={() => reveal(selected.agentId, selected.path)}>
                  <FolderOpen /> {t("Finderで表示")}
                </Button>
                {!readOnly && (
                  <>
                    {!editMode && (
                      <Button size="sm" variant="outline" onClick={() => setEditMode(true)}>
                        <Pencil /> {t("編集")} <Kbd>E</Kbd>
                      </Button>
                    )}
                    <Button size="sm" disabled={saving || !editMode} onClick={save}>
                      {saving ? <Loader2 className="animate-spin" /> : <Save />}
                      {t("保存")} <Kbd>⌘S</Kbd>
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
                  <ChatMarkdown text={doc.body} />
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
