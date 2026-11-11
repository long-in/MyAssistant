"use client";

import { useEditShortcuts } from "@/lib/useEditShortcuts";
import { Kbd } from "@/components/Kbd";
import { useT } from "@/lib/i18n";
import { useCallback, useEffect, useState } from "react";
import { BookOpen, Brain, Download, FileText, FolderOpen, History, Lightbulb, ListChecks, Loader2, Pencil, RotateCcw, Save, Sparkles, Trash2, Upload } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { ask, open as openDialog } from "@tauri-apps/plugin-dialog";
import { ChatMarkdown } from "@/components/ChatMarkdown";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { splitFrontmatter } from "@/lib/knowledgeDoc";
import { useWorkspace } from "@/lib/workspace";
import KnowledgeTree, { type NodeKind } from "@/components/KnowledgeTree";
import { isReadOnlyPath } from "@/lib/knowledgePaths";
import MemoryPanel from "@/components/MemoryPanel";
import ProposalsPanel from "@/components/ProposalsPanel";
import NoteVersions, { versionLabel } from "@/components/NoteVersions";
import { EmptyState, PaneHeader, StatusPill } from "@/components/GlobalPanels";
import {
  LEADER_ID,
  type KnowledgeFile,
  type MemoryView,
  type Proposal,
  listProposals,
  decideProposal,
  runMaintenance,
  restoreKnowledgeVersion,
  memoryView,
  memoryForget,
  knowledgeDir,
  openKnowledgeDir,
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
  SYSTEM_DIRS,
  reportError,
  exportKnowledge,
  importKnowledge,
  resetKnowledge,
  runBatch,
  setKnowledgeDir,
} from "@/lib/api";

/** 編集できるのはMarkdownのみ。それ以外は閲覧専用。 */
const isNote = (path: string) => path.endsWith(".md");

export default function Knowledge() {
  const t = useT();
  // リーダーは記憶を持たない
  const hasMemory = useWorkspace().active.id !== LEADER_ID;
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
  const [memory, setMemory] = useState<MemoryView>({ pins: [] });
  const [proposals, setProposals] = useState<Proposal[]>([]);
  // 処理中の提案(統合はLLMを使うため時間がかかる)
  const [deciding, setDeciding] = useState<string | null>(null);
  const [maintaining, setMaintaining] = useState(false);
  // 右側に出すもの。記憶・提案・版はノートを開くと閉じる
  const [panel, setPanel] = useState<"note" | "memory" | "proposals" | "versions">("note");

  const refresh = useCallback(
    () =>
      Promise.all([listKnowledge(), listOtherKnowledge(), listKnowledgeDirs(), memoryView(), listProposals()])
        .then(([f, o, d, m, p]) => {
          setFiles([...f, ...o]);
          setDirs(d);
          setMemory(m);
          setProposals(p);
        })
        .catch((e) => toast.error(reportError(e))),
    [],
  );
  useEffect(() => {
    refresh();
    knowledgeDir().then(setDir).catch(() => {});
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

  const forgetPin = async (text: string) => {
    const ok = await ask(t("「{text}」を忘れます。この操作は元に戻せません。続行しますか?", { text }), {
      title: t("削除の確認"),
      kind: "warning",
      okLabel: t("削除する"),
      cancelLabel: t("キャンセル"),
    });
    if (!ok) return;
    try {
      await memoryForget(text);
      await refresh();
    } catch (e) {
      toast.error(reportError(e));
    }
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
    if (path === selected) {
      setPanel("note");
      return;
    }
    if (!(await confirmDiscard())) return;
    try {
      const text = await (isNote(path) ? readKnowledge(path) : readOtherKnowledge(path));
      setPanel("note");
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
    if (!selected || saving) return;
    if (!dirty) {
      setEditMode(false); // 変更が無ければ保存せず閲覧に戻る
      return;
    }
    setSaving(true);
    try {
      await writeKnowledge(selected, draft);
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
    if (kind === "dir" && SYSTEM_DIRS.includes(path.toLowerCase())) {
      toast.warning(t("「{name}」はシステムが管理するフォルダのため、削除できません。", { name }));
      return;
    }
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
      if (!r) toast.info(t("整理するログも削除するキャプチャもないか、すでに実行中です。"));
      else if (!r.processed_logs && !r.failed_segments) {
        toast.success(t("整理済みの会話のキャプチャを{captures_removed}件削除しました。", { captures_removed: r.captures_removed }));
        await refresh();
      } else {
        let msg = t("{processed_logs}件のログを整理しました(新規{created}件・更新{updated}件)。", { processed_logs: r.processed_logs, created: r.notes_created, updated: r.notes_updated });
        if (r.proposals_added) msg += t("学びの提案が{n}件あります。", { n: r.proposals_added });
        if (r.rewrites_skipped) msg += t("({n}件は内容が大きく短くなるため書き換えず、経緯への追記だけにしました)", { n: r.rewrites_skipped });
        if (r.captures_removed) msg += t("(キャプチャ{captures_removed}件を削除)", { captures_removed: r.captures_removed });
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

  const maintain = async () => {
    setMaintaining(true);
    try {
      const n = await runMaintenance();
      await refresh();
      if (n) {
        toast.success(t("棚卸しの提案が{n}件あります。", { n }));
        setPanel("proposals");
      } else toast.info(t("新しい提案はありません。"));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setMaintaining(false);
    }
  };

  const decide = async (p: Proposal, accept: boolean) => {
    setDeciding(p.id);
    try {
      await decideProposal(p.id, accept);
      // 統合で開いているノートが消えたり書き換わったりするため、表示を読み直す
      if (accept && p.kind === "merge" && selected && (selected === p.payload?.drop || selected === p.payload?.keep)) clearSelection();
      await refresh();
      if (accept) toast.success(t("反映しました。"));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setDeciding(null);
    }
  };

  const restore = async (version: string) => {
    if (!selected) return;
    const ok = await ask(t("「{path}」を {date} の版に戻します。今の内容も版として残ります。続行しますか?", { path: selected, date: versionLabel(version) }), {
      title: t("以前の版に戻す"),
      kind: "warning",
      okLabel: t("戻す"),
      cancelLabel: t("キャンセル"),
    });
    if (!ok) return;
    try {
      await restoreKnowledgeVersion(selected, version);
      const text = await readKnowledge(selected);
      setContent(text);
      setDraft(text);
      setPanel("note");
      await refresh();
      toast.success(t("以前の版に戻しました。"));
    } catch (e) {
      toast.error(reportError(e));
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

  const readOnly = selected !== null && (!isNote(selected) || isReadOnlyPath(selected));
  const doc = selected ? splitFrontmatter(draft) : null;

  // `e`で編集、Cmd/Ctrl+Sで保存
  useEditShortcuts({ canEdit: selected !== null && !readOnly && panel === "note", editMode, onEdit: () => setEditMode(true), onSave: save });

  return (
    <div className="flex h-full">
      <aside className="flex w-72 shrink-0 flex-col border-r bg-background/40 backdrop-blur">
        <PaneHeader
          icon={BookOpen}
          title="Knowledge"
          chips={
            <>
              <StatusPill on={files.length > 0}>{t("ノート {n}件", { n: files.length })}</StatusPill>
              <StatusPill on={running}>{running ? t("整理中…") : t("待機")}</StatusPill>
            </>
          }
        />
        <div className="hud-panel hud-accent m-3 px-4 py-3">
          <p className="hud-heading mb-2">Organize</p>
          <p className="text-xs text-muted-foreground">{t("自動では整理されません。会話ログは、ここから手動で整理します。同じ話題のノートは、最新の内容に書き直されます。")}</p>
          <Button size="sm" className="mt-2 w-full" disabled={running} onClick={organize}>
            {running ? <Loader2 className="animate-spin" /> : <Sparkles />}
            {running ? t("整理中…") : t("今すぐ整理")}
          </Button>
          <Button size="sm" variant="outline" className="mt-2 w-full" disabled={maintaining} onClick={maintain} title={t("似たノート・長く更新されていないノート・inboxのノートを探します(AIは使いません)")}>
            {maintaining ? <Loader2 className="animate-spin" /> : <ListChecks />}
            {t("棚卸し")}
          </Button>
        </div>
        <div className="hud-panel mx-3 mb-3 px-4 py-3">
          <p className="hud-heading mb-2">Proposals</p>
          <p className="mb-2 text-xs text-muted-foreground">{t("承認待ちの提案 {n}件", { n: proposals.length })}</p>
          <Button size="sm" variant={panel === "proposals" ? "default" : "outline"} className="mt-2 w-full" onClick={() => setPanel("proposals")}>
            <Lightbulb /> {t("提案を見る")}
          </Button>
        </div>
        {hasMemory && (
        <div className="hud-panel mx-3 mb-3 px-4 py-3">
          <p className="hud-heading mb-2">Memory</p>
          <p className="mb-2 text-xs text-muted-foreground">
            {t("覚えておくこと {n}件", { n: memory.pins.length })}
          </p>
          <Button size="sm" variant={panel === "memory" ? "default" : "outline"} className="mt-2 w-full" onClick={() => setPanel("memory")}>
            <Brain /> {t("記憶を見る")}
          </Button>
        </div>
        )}
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
        <div className="flex flex-wrap gap-2 border-t px-4 py-3">
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
          <div className="border-t px-4 py-3 text-xs text-muted-foreground">
            <p className="hud-label mb-1">Location</p>
            <p className="break-all">
              {t("保存先:")} {dir}
            </p>
            <div className="mt-2 flex gap-2">
              <Button size="sm" variant="outline" onClick={() => openKnowledgeDir().catch((e) => toast.error(reportError(e)))}>
                <FolderOpen /> {t("Finder で開く")}
              </Button>
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
        {panel === "memory" && hasMemory ? (
          <MemoryPanel memory={memory} onForget={forgetPin} onEdit={open} />
        ) : panel === "proposals" ? (
          <ProposalsPanel proposals={proposals} busyId={deciding} onDecide={decide} onOpenNote={open} />
        ) : panel === "versions" && selected ? (
          <NoteVersions path={selected} onRestore={restore} onClose={() => setPanel("note")} />
        ) : doc && selected ? (
          <>
            <div className="flex items-center justify-between gap-3 border-b px-4 py-2">
              <span className="flex min-w-0 items-center gap-2 text-sm">
                <FileText className="size-4 shrink-0 text-primary" />
                <span className="truncate">{selected}</span>
                {dirty && <StatusPill on>{t("未保存")}</StatusPill>}
                {readOnly && <StatusPill on={false}>{t("読み取り専用")}</StatusPill>}
              </span>
              {!readOnly && (
              <div className="flex shrink-0 gap-2">
                {!editMode && (
                  <Button size="sm" variant="ghost" onClick={() => setPanel("versions")} title={t("整理で書き換える前の内容を見る・戻す")}>
                    <History /> {t("履歴")}
                  </Button>
                )}
                {!editMode && (
                  <Button size="sm" variant="outline" onClick={() => setEditMode(true)}>
                    <Pencil /> {t("編集")} <Kbd>E</Kbd>
                  </Button>
                )}
                <Button size="sm" disabled={saving || !editMode} onClick={save}>
                  {saving ? <Loader2 className="animate-spin" /> : <Save />}
                  {t("保存")} <Kbd>⌘S</Kbd>
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
