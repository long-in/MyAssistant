"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  FilePlus,
  FileText,
  Folder,
  FolderOpen,
  FolderPlus,
} from "lucide-react";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import type { KnowledgeFile } from "@/lib/api";

type Dir = { key: string; name: string; dirs: Dir[]; files: KnowledgeFile[] };
export type NodeKind = "file" | "dir";

/** ツリー上で進行中の入力。`parent`は作成先フォルダ、`path`は名前変更の対象。 */
type Editing =
  | { mode: "new-file" | "new-dir"; parent: string }
  | { mode: "rename"; path: string; kind: NodeKind };

/** フォルダ名として使える文字(バックエンドの検証と揃える)。 */
const DIR_NAME_RE = /^[\p{L}\p{N}_-]+$/u;
/** ノート名(`.md`を除く)として使える文字。 */
const FILE_NAME_RE = /^[\p{L}\p{N}_-][\p{L}\p{N}_.-]*$/u;
export const MAX_DEPTH = 3;

/** 名前がバックエンドで受理されるか。 */
export const isValidName = (kind: NodeKind, name: string) =>
  kind === "dir" ? DIR_NAME_RE.test(name) && name.length <= 40 : FILE_NAME_RE.test(name) && !name.includes("..") && name.length <= 96;

/** ファイルの階層とフォルダ(空含む)をツリーに展開する。 */
function buildTree(files: KnowledgeFile[], dirs: string[]): Dir {
  const root: Dir = { key: "", name: "", dirs: [], files: [] };
  const ensure = (path: string) => {
    let cur = root;
    for (const part of path.split("/").filter(Boolean)) {
      const key = cur.key ? `${cur.key}/${part}` : part;
      let next = cur.dirs.find((d) => d.key === key);
      if (!next) {
        next = { key, name: part, dirs: [], files: [] };
        cur.dirs.push(next);
      }
      cur = next;
    }
    return cur;
  };
  dirs.forEach(ensure);
  for (const f of files) ensure(f.category).files.push(f);
  const sort = (d: Dir) => {
    d.dirs.sort((a, b) => a.name.localeCompare(b.name));
    d.files.sort((a, b) => fileName(a.path).localeCompare(fileName(b.path)));
    d.dirs.forEach(sort);
  };
  sort(root);
  return root;
}

const fileName = (path: string) => path.split("/").pop() ?? path;
const parentOf = (path: string) => path.split("/").slice(0, -1).join("/");

const INDENT = 12;
const BASE = 8;

/** VSCodeのエクスプローラ風のフォルダツリー。作成・名前変更・削除に対応する。 */
export default function KnowledgeTree({
  files,
  dirs,
  selected,
  onOpen,
  onCreate,
  onRename,
  onDelete,
  onInvalidName,
}: {
  files: KnowledgeFile[];
  dirs: string[];
  selected: string | null;
  onOpen: (path: string) => void;
  onCreate: (kind: NodeKind, path: string) => Promise<boolean>;
  onRename: (kind: NodeKind, from: string, to: string) => Promise<boolean>;
  onDelete: (kind: NodeKind, path: string) => void;
  onInvalidName: () => void;
}) {
  const t = useT();
  const tree = useMemo(() => buildTree(files, dirs), [files, dirs]);
  // 開いているフォルダ。初期状態はすべて閉じる
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  // 作成先の基準になるフォルダ(最後にクリックしたフォルダ、またはファイルの親)
  const [focusDir, setFocusDir] = useState("");
  const [editing, setEditing] = useState<Editing | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; kind: NodeKind | "root"; path: string } | null>(null);

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    window.addEventListener("click", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("blur", close);
    };
  }, [menu]);

  const toggle = (key: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (!next.delete(key)) next.add(key);
      return next;
    });

  const startNew = (mode: "new-file" | "new-dir", parent: string) => {
    // 新規入力欄が見えるよう、親フォルダと祖先を開く
    setExpanded((prev) => {
      const next = new Set(prev);
      const parts = parent.split("/").filter(Boolean);
      for (let i = 1; i <= parts.length; i++) next.add(parts.slice(0, i).join("/"));
      return next;
    });
    setEditing({ mode, parent });
  };

  const commit = async (value: string) => {
    const ed = editing;
    setEditing(null);
    const name = value.trim();
    if (!ed || !name) return;
    const kind: NodeKind = ed.mode === "rename" ? ed.kind : ed.mode === "new-file" ? "file" : "dir";
    const base = kind === "file" ? name.replace(/\.md$/, "") : name;
    if (!isValidName(kind, base)) return void onInvalidName();
    const leaf = kind === "file" ? `${base}.md` : base;
    if (ed.mode === "rename") {
      const to = [parentOf(ed.path), leaf].filter(Boolean).join("/");
      if (to !== ed.path) await onRename(kind, ed.path, to);
    } else {
      await onCreate(kind, [ed.parent, leaf].filter(Boolean).join("/"));
    }
  };

  const row = "flex h-[22px] w-full items-center gap-1 pr-2 text-left text-[13px] hover:bg-muted/70";

  /** 名前入力行。`offset`はファイルアイコン分の字下げ。 */
  const input = (depth: number, icon: React.ReactNode, offset: number, initial = "") => (
    <div className="flex h-[22px] items-center gap-1 pr-2" style={{ paddingLeft: BASE + depth * INDENT + offset }}>
      {icon}
      <NameInput initial={initial} onCommit={commit} onCancel={() => setEditing(null)} />
    </div>
  );
  const dirIcon = <Folder className="size-4 shrink-0 text-muted-foreground" />;
  const fileIcon = <FileText className="size-4 shrink-0 text-muted-foreground" />;

  const render = (dir: Dir, depth: number) => (
    <>
      {editing?.mode === "new-dir" && editing.parent === dir.key && <li>{input(depth, dirIcon, 20)}</li>}
      {dir.dirs.map((d) => {
        const isOpen = expanded.has(d.key);
        const renaming = editing?.mode === "rename" && editing.path === d.key;
        return (
          <li key={d.key}>
            {renaming ? (
              input(depth, dirIcon, 20, d.name)
            ) : (
              <button
                className={cn(row, focusDir === d.key && !selected && "bg-muted/70")}
                style={{ paddingLeft: BASE + depth * INDENT }}
                onClick={() => {
                  setFocusDir(d.key);
                  toggle(d.key);
                }}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setFocusDir(d.key);
                  setMenu({ x: e.clientX, y: e.clientY, kind: "dir", path: d.key });
                }}
                onKeyDown={(e) => {
                  if (e.key === "F2") setEditing({ mode: "rename", path: d.key, kind: "dir" });
                  if (e.key === "Delete") onDelete("dir", d.key);
                }}
                aria-expanded={isOpen}
              >
                {isOpen ? (
                  <ChevronDown className="size-4 shrink-0 text-muted-foreground" />
                ) : (
                  <ChevronRight className="size-4 shrink-0 text-muted-foreground" />
                )}
                {isOpen ? (
                  <FolderOpen className="size-4 shrink-0 text-muted-foreground" />
                ) : (
                  <Folder className="size-4 shrink-0 text-muted-foreground" />
                )}
                <span className="truncate">{d.name}</span>
              </button>
            )}
            {isOpen && <ul>{render(d, depth + 1)}</ul>}
          </li>
        );
      })}
      {editing?.mode === "new-file" && editing.parent === dir.key && <li>{input(depth, fileIcon, 20)}</li>}
      {dir.files.map((f) => {
        const renaming = editing?.mode === "rename" && editing.path === f.path;
        if (renaming) {
          return (
            <li key={f.path}>
              {input(depth, fileIcon, 20, fileName(f.path).replace(/\.md$/, ""))}
            </li>
          );
        }
        return (
          <li key={f.path}>
            <button
              className={cn(row, selected === f.path && "bg-primary/15 text-foreground hover:bg-primary/20")}
              style={{ paddingLeft: BASE + depth * INDENT + 20 }}
              onClick={() => {
                setFocusDir(dir.key);
                onOpen(f.path);
              }}
              onContextMenu={(e) => {
                e.preventDefault();
                setFocusDir(dir.key);
                if (f.path.endsWith(".md")) setMenu({ x: e.clientX, y: e.clientY, kind: "file", path: f.path });
              }}
              onKeyDown={(e) => {
                if (!f.path.endsWith(".md")) return;
                if (e.key === "F2") setEditing({ mode: "rename", path: f.path, kind: "file" });
                if (e.key === "Delete") onDelete("file", f.path);
              }}
              title={f.title ? `${f.title} (${f.path})` : f.path}
            >
              <FileText className="size-4 shrink-0 text-muted-foreground" />
              <span className="truncate">{f.path.endsWith(".md") ? fileName(f.path).replace(/\.md$/, "") : fileName(f.path)}</span>
            </button>
          </li>
        );
      })}
    </>
  );

  const depthOf = (path: string) => (path ? path.split("/").length : 0);
  const canNestDir = (parent: string) => depthOf(parent) < MAX_DEPTH;
  const menuItem = "block w-full px-3 py-1 text-left text-[13px] hover:bg-muted";

  return (
    <div
      className="flex min-h-full flex-col"
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY, kind: "root", path: "" });
      }}
    >
      <div className="flex items-center justify-end gap-0.5 px-2 pb-1">
        <button
          className="rounded p-1 text-muted-foreground hover:bg-muted hover:text-foreground"
          title={t("新規ノート")}
          onClick={() => startNew("new-file", focusDir)}
        >
          <FilePlus className="size-4" />
        </button>
        <button
          className="rounded p-1 text-muted-foreground hover:bg-muted hover:text-foreground disabled:opacity-40"
          title={t("新規フォルダ")}
          disabled={!canNestDir(focusDir)}
          onClick={() => startNew("new-dir", focusDir)}
        >
          <FolderPlus className="size-4" />
        </button>
      </div>
      <ul className="select-none">
        {render(tree, 0)}
      </ul>
      <div className="flex-1" onClick={() => setFocusDir("")} />
      {menu && (
        <div
          className="fixed z-50 min-w-36 rounded-md border bg-popover py-1 text-popover-foreground shadow-md"
          style={{ left: menu.x, top: menu.y }}
          onClick={() => setMenu(null)}
        >
          {menu.kind !== "file" && (
            <button className={menuItem} onClick={() => startNew("new-file", menu.path)}>
              {t("新規ノート")}
            </button>
          )}
          {menu.kind !== "file" && canNestDir(menu.path) && (
            <button className={menuItem} onClick={() => startNew("new-dir", menu.path)}>
              {t("新規フォルダ")}
            </button>
          )}
          {menu.kind !== "root" && (
            <>
              <button
                className={menuItem}
                onClick={() => setEditing({ mode: "rename", path: menu.path, kind: menu.kind as NodeKind })}
              >
                {t("名前を変更")}
              </button>
              <button
                className={cn(menuItem, "text-destructive")}
                onClick={() => onDelete(menu.kind as NodeKind, menu.path)}
              >
                {t("削除")}
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
}

/** Enterで確定、Escで取消、フォーカスが外れたら確定する名前入力。 */
function NameInput({
  initial,
  onCommit,
  onCancel,
}: {
  initial: string;
  onCommit: (v: string) => void;
  onCancel: () => void;
}) {
  const ref = useRef<HTMLInputElement>(null);
  const done = useRef(false);
  useEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);
  const finish = (commit: boolean) => {
    if (done.current) return;
    done.current = true;
    if (commit) onCommit(ref.current?.value ?? "");
    else onCancel();
  };
  return (
    <input
      ref={ref}
      defaultValue={initial}
      className="h-[18px] min-w-0 flex-1 rounded-sm border border-primary bg-background px-1 text-[13px] outline-none"
      onKeyDown={(e) => {
        if (e.key === "Enter") finish(true);
        if (e.key === "Escape") finish(false);
      }}
      onBlur={() => finish(true)}
    />
  );
}
