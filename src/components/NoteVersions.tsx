"use client";

import { useEffect, useState } from "react";
import { History, RotateCcw, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { ChatMarkdown } from "@/components/ChatMarkdown";
import { useT } from "@/lib/i18n";
import { splitFrontmatter } from "@/lib/knowledgeDoc";
import { knowledgeVersions, readKnowledgeVersion, reportError } from "@/lib/api";

/** 版の名前(`2026-11-11T03-00-00Z_000.md`)を、表示用の日時にする。読めなければ名前のまま。 */
export function versionLabel(name: string): string {
  const m = /^(\d{4}-\d{2}-\d{2})T(\d{2})-(\d{2})-(\d{2})Z/.exec(name);
  if (!m) return name;
  const d = new Date(`${m[1]}T${m[2]}:${m[3]}:${m[4]}Z`);
  return Number.isNaN(d.getTime()) ? name : d.toLocaleString();
}

/** ノートの退避済みの版(整理で書き換える前の内容)の一覧と中身。選んだ版へ戻せる。 */
export default function NoteVersions({
  path,
  onRestore,
  onClose,
}: {
  path: string;
  onRestore: (version: string) => void;
  onClose: () => void;
}) {
  const t = useT();
  const [versions, setVersions] = useState<string[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [text, setText] = useState("");

  useEffect(() => {
    knowledgeVersions(path)
      .then((v) => {
        setVersions(v);
        setSelected(v[0] ?? null);
      })
      .catch((e) => toast.error(reportError(e)));
  }, [path]);

  useEffect(() => {
    // 版が無いときは本文を出さない(下の描画で`selected`を見る)
    if (!selected) return;
    readKnowledgeVersion(path, selected)
      .then(setText)
      .catch((e) => toast.error(reportError(e)));
  }, [path, selected]);

  const doc = splitFrontmatter(text);
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center justify-between gap-3 border-b px-4 py-2">
        <span className="flex min-w-0 items-center gap-2 text-sm">
          <History className="size-4 shrink-0 text-primary" />
          <span className="truncate">{t("{path} の以前の版", { path })}</span>
        </span>
        <div className="flex shrink-0 gap-2">
          <Button size="sm" variant="outline" disabled={!selected} onClick={() => selected && onRestore(selected)}>
            <RotateCcw /> {t("この版に戻す")}
          </Button>
          <Button size="sm" variant="ghost" onClick={onClose}>
            <X /> {t("閉じる")}
          </Button>
        </div>
      </div>
      <div className="flex min-h-0 flex-1">
        <ul className="w-56 shrink-0 overflow-y-auto border-r py-2 text-sm">
          {versions?.length === 0 && <li className="px-4 py-2 text-xs text-muted-foreground">{t("以前の版はありません。整理で書き換えると、書き換える前の内容が残ります。")}</li>}
          {versions?.map((v) => (
            <li key={v}>
              <button
                type="button"
                className={`w-full px-4 py-1.5 text-left hover:bg-muted/60 ${v === selected ? "bg-muted font-medium" : ""}`}
                onClick={() => setSelected(v)}
              >
                {versionLabel(v)}
              </button>
            </li>
          ))}
        </ul>
        <div className="flex-1 overflow-y-auto px-10 py-8">
          <div className="mx-auto max-w-3xl">{selected && <ChatMarkdown text={doc.body} />}</div>
        </div>
      </div>
    </div>
  );
}
